//! What every capture path shares (`msfs.rs`, `xplane.rs`): the hidden switch, sessions
//! keyed by the aircraft loaded, and ticks from the shared [`Sampler`] into [`Telemetry`].

use super::{RecordError, Telemetry};
use crate::models::AircraftInfo;
use butterlog_wire::{Aircraft, Channel, Event, PhaseInputs, Sampler, SessionOpen};
use tauri::{AppHandle, Manager};

/// While the pipeline is in beta, recording is opt-in: the Settings toggle, or this file in
/// the app's data directory (how the first beta installs were switched on).
pub const SWITCH_FILE: &str = "telemetry_beta";

/// Whether to record: opted in, a signed-in app, and a spool.
pub fn enabled(app: &AppHandle) -> bool {
    let config = app.state::<crate::config::ConfigManager>().get_config();
    let opted_in = config.telemetry_beta
        || app.path().app_data_dir().is_ok_and(|d| d.join(SWITCH_FILE).exists());
    opted_in && config.is_logged_in() && app.try_state::<Telemetry>().is_some()
}

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
}

impl Recorder {
    pub fn new(app: &AppHandle, channels: &[Channel], phase: PhaseInputs) -> Self {
        Self { app: app.clone(), sampler: Sampler::new(channels, phase), sim_time: None, wall_ms: None, recording: None }
    }

    /// For the session header.
    pub fn app_version(&self) -> String {
        self.app.package_info().version.to_string()
    }

    pub fn set(&mut self, channel: u16, value: f64) {
        self.sampler.set(channel, value);
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
        self.sampler.event(event);
    }

    /// Samples the latest values, once the sim clock is known and a session is open.
    pub fn tick(&mut self, flags: u8) {
        let (Some(sim_time), Some(_)) = (self.sim_time, &self.recording) else {
            return;
        };
        let wall_ms = self.wall_ms.take().unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
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
    }
}
