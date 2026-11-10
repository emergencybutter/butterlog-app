//! What every capture path shares (`msfs.rs`, `xplane.rs`): whether to record, sessions
//! keyed by the aircraft loaded, and ticks from the shared [`Sampler`] into [`Telemetry`].

use super::live::{LiveTelemetry, LiveView};
use super::runways::TouchdownWatch;
use super::{RecordError, Telemetry};
use crate::models::AircraftInfo;
use butterlog_wire::{Aircraft, Channel, Event, EventKind, PhaseInputs, Sampler, SessionOpen, SimRunways};
use tauri::{AppHandle, Manager};

/// Whether to record: a signed-in app with a spool. Telemetry is how the service knows of
/// a flight at all, so there is no switch for it.
pub fn enabled(app: &AppHandle) -> bool {
    let config = app.state::<crate::config::ConfigManager>().get_config();
    config.is_logged_in() && app.try_state::<Telemetry>().is_some()
}

/// How often the window's Telemetry page gets a fresh copy of what is read.
const PUBLISH_EVERY_MS: i64 = 250;

/// The session header's aircraft, from what the monitor identified.
pub fn aircraft(info: &AircraftInfo) -> Aircraft {
    Aircraft {
        title: info.title.clone(),
        icao: info.atc_model.clone(),
        tail: info.atc_id.clone(),
        livery: info.livery.clone(),
        engine_type: info.engine_type.clone(),
        engine_count: info.num_engines.clamp(0, 255) as u8,
    }
}

pub struct Recorder {
    app: AppHandle,
    sampler: Sampler,
    sim_time: Option<f64>,
    /// The next tick's wall time, when the sim stamped it; otherwise the time of the tick.
    wall_ms: Option<i64>,
    /// Title and livery of the aircraft being recorded.
    recording: Option<(String, String)>,
    touchdowns: Option<TouchdownWatch>,
    /// Which monitor this is (`msfs`, `xplane`), for the Telemetry page.
    monitor: &'static str,
    live: LiveTelemetry,
    published_ms: i64,
}

impl Recorder {
    pub fn new(app: &AppHandle, monitor: &'static str, channels: &[Channel], phase: PhaseInputs) -> Self {
        Self {
            app: app.clone(),
            sampler: Sampler::new(channels, phase),
            sim_time: None,
            wall_ms: None,
            recording: None,
            touchdowns: None,
            monitor,
            live: LiveTelemetry::new(channels),
            published_ms: 0,
        }
    }

    /// Hands the Telemetry page a copy, at most every [`PUBLISH_EVERY_MS`].
    fn publish(&mut self, now_ms: i64, force: bool) {
        if !force && now_ms - self.published_ms < PUBLISH_EVERY_MS {
            return;
        }
        self.published_ms = now_ms;
        self.live.session = self.app.try_state::<Telemetry>().and_then(|t| t.current_session()).filter(|_| self.recording.is_some()).map(|id| id.to_string());
        if let Some(view) = self.app.try_state::<LiveView>() {
            view.publish(self.monitor, &self.live);
        }
    }

    pub fn log(&self, message: String) {
        crate::append_log(&self.app, message);
    }

    /// For the session header.
    pub fn app_version(&self) -> String {
        self.app.package_info().version.to_string()
    }

    pub fn set(&mut self, channel: u16, value: f64) {
        self.sampler.set(channel, value);
        self.live.set(channel, value);
        if let Some(watch) = self.touchdowns.as_mut() {
            watch.set(channel, value, chrono::Utc::now().timestamp_millis());
        }
    }

    /// Notices touchdowns from these channels: the on-ground flag and the position.
    pub fn watch_touchdowns(&mut self, on_ground: u16, latitude: u16, longitude: u16) {
        self.touchdowns = Some(TouchdownWatch::new(on_ground, latitude, longitude));
    }

    /// Where the aircraft touched down, once per touchdown, while a session records.
    pub fn take_touchdown(&mut self) -> Option<(f64, f64)> {
        let at = self.touchdowns.as_mut()?.take()?;
        self.recording.as_ref().map(|_| at)
    }

    /// The runways the sim draws around a touchdown, for the service to measure it
    /// against. Goes with the next tick.
    pub fn runways(&mut self, found: &SimRunways) {
        crate::append_log(
            &self.app,
            format!("[Telemetry] {} runway(s) of {} from the sim.", found.runways.len(), found.airport),
        );
        self.live.event(
            chrono::Utc::now().timestamp_millis(),
            "Runways".into(),
            format!("{} runway(s) of {}", found.runways.len(), found.airport),
        );
        self.sampler.event(Event { kind: EventKind::Runways, detail: found.encode() });
    }

    pub fn set_sim_time(&mut self, sim_time: f64) {
        self.sim_time = Some(sim_time);
    }

    /// For the next tick only.
    pub fn set_wall_time(&mut self, wall_ms: i64) {
        self.wall_ms = Some(wall_ms);
    }

    /// Goes with the next tick. Before a session starts, with its first.
    pub fn event(&mut self, event: Event) {
        let detail = event.detail.chars().take(120).collect();
        self.live.event(chrono::Utc::now().timestamp_millis(), format!("{:?}", event.kind), detail);
        self.sampler.event(event);
    }

    /// Samples the latest values, once the sim clock is known and a session is open.
    pub fn tick(&mut self, flags: u8) {
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.live.ticks += 1;
        self.live.updated_ms = now_ms;
        self.publish(now_ms, false);
        let (Some(sim_time), Some(_)) = (self.sim_time, &self.recording) else {
            return;
        };
        let wall_ms = self.wall_ms.take().unwrap_or(now_ms);
        let Some(tick) = self.sampler.tick(sim_time, wall_ms, flags) else {
            return;
        };
        match self.app.state::<Telemetry>().record(tick) {
            Ok(()) => {}
            Err(e @ RecordError::Encode(_)) => crate::append_log(&self.app, format!("[Telemetry] {e}")),
            Err(e @ RecordError::Spool(_)) => {
                crate::append_log(&self.app, format!("[Telemetry] {e}"));
                self.recording = None;
            }
        }
    }

    /// Starts a session for this aircraft, unless it is already being recorded. The first
    /// tick carries every channel's latest value.
    pub fn start(&mut self, info: &AircraftInfo, open: &SessionOpen, flags: u8) {
        self.live.sim = open.header.sim.clone();
        self.live.sim_version = open.header.sim_version.clone();
        self.live.capture = open.header.capture.clone();
        self.live.aircraft = info.title.clone();
        let key = (info.title.clone(), info.livery.clone());
        if info.title.is_empty() || self.recording.as_ref() == Some(&key) || !enabled(&self.app) {
            return;
        }
        match self.app.state::<Telemetry>().start(open) {
            Ok(session) => {
                crate::append_log(&self.app, format!("[Telemetry] Session {session} started: {}", info.title));
                self.sampler.restart();
                self.recording = Some(key);
                self.tick(flags);
                self.publish(chrono::Utc::now().timestamp_millis(), true);
            }
            Err(e) => {
                crate::append_log(&self.app, format!("[Telemetry] Could not start a session: {e}"));
                self.recording = None;
            }
        }
    }

    /// Ends the session being recorded, if any.
    pub fn stop(&mut self) {
        // Already sent with the last tick, or from before the session: either way, not the
        // next session's.
        self.sampler.discard_events();
        if self.recording.take().is_none() {
            return;
        }
        if let Err(e) = self.app.state::<Telemetry>().stop() {
            crate::append_log(&self.app, format!("[Telemetry] Could not end the session: {e}"));
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop();
        if let Some(view) = self.app.try_state::<LiveView>() {
            view.clear(self.monitor);
        }
    }
}
