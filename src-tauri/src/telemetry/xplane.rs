//! The X-Plane sampler. Its datarefs come from one of two captures:
//!
//! - `xp_plugin` (design §4.2): `butterlog_xp_plugin` reads them every sim frame, stamped
//!   with the sim clock, and sends them over its telemetry link (`xp_plugin.rs`).
//! - `xp_webapi` (design §4.1, the fallback without the plugin): the monitor's WebSocket
//!   subscription, about 8 updates a second, only for datarefs that changed exactly, and
//!   nothing while paused.
//!
//! Either way the values go through the shared sampler. Pause, replay and crash are
//! inferred from their datarefs changing, a position change from the monitor's teleport
//! check; the plugin adds aircraft and airport loads. A session is one aircraft on one
//! connection, as for MSFS. If the plugin link drops mid-session, the session ends and a
//! new one goes on through the Web API.
//!
//! Channel names are the datarefs, with `[i]` for an array element, as the plugin's
//! capture names them and the service's normaliser reads them.

use super::recorder::{aircraft, enabled, Recorder};
use crate::models::AircraftInfo;
use butterlog_wire::{
    flags, Channel, DeltaOrder, Event, EventKind, Kind, PhaseInputs, RateClass, SessionHeader,
    SessionOpen, PROTOCOL_VERSION,
};
use serde_json::Value;
use butterlog_wire::SimRunways;
use std::collections::HashMap;
use std::sync::mpsc;
use tauri::AppHandle;

use DeltaOrder::{Delta, DeltaOfDelta};
use Kind::{Bool, Enum, F64};
use RateClass::{Frame, OnChange, Slow};

/// The sim clock, which goes in each tick's header, not a channel.
const SIM_TIME: &str = "sim/time/total_running_time_sec";

/// Dataref, array elements (none for a scalar), quantum in X-Plane's units, rate class,
/// delta order and kind. Quanta are the ones rollout phase 1 measured (`measure_capture`).
const DATAREFS: &[(&str, &[usize], f64, RateClass, DeltaOrder, Kind)] = &[
    ("sim/flightmodel/position/latitude", &[], 1e-7, Frame, DeltaOfDelta, F64),
    ("sim/flightmodel/position/longitude", &[], 1e-7, Frame, DeltaOfDelta, F64),
    ("sim/flightmodel/position/elevation", &[], 0.03, Frame, DeltaOfDelta, F64),
    ("sim/flightmodel/position/y_agl", &[], 0.03, Frame, DeltaOfDelta, F64),
    ("sim/flightmodel2/position/ellipsoid_height", &[], 0.03, Frame, DeltaOfDelta, F64),
    ("sim/flightmodel2/position/pressure_altitude", &[], 1.0, Frame, DeltaOfDelta, F64),
    ("sim/cockpit2/gauges/indicators/altitude_ft_pilot", &[], 0.1, Frame, DeltaOfDelta, F64),
    ("sim/flightmodel/position/indicated_airspeed", &[], 0.1, Frame, Delta, F64),
    ("sim/flightmodel/position/true_airspeed", &[], 0.05, Frame, Delta, F64),
    ("sim/flightmodel/position/groundspeed", &[], 0.05, Frame, Delta, F64),
    ("sim/flightmodel/position/vh_ind", &[], 0.005, Frame, Delta, F64),
    ("sim/flightmodel/position/theta", &[], 0.01, Frame, DeltaOfDelta, F64),
    ("sim/flightmodel/position/phi", &[], 0.01, Frame, DeltaOfDelta, F64),
    ("sim/flightmodel/position/psi", &[], 0.01, Frame, DeltaOfDelta, F64),
    ("sim/flightmodel/position/mag_psi", &[], 0.01, Frame, DeltaOfDelta, F64),
    ("sim/flightmodel/position/hpath", &[], 0.01, Frame, DeltaOfDelta, F64),
    ("sim/flightmodel/forces/g_nrml", &[], 0.001, Frame, Delta, F64),
    ("sim/flightmodel/forces/g_side", &[], 0.001, Frame, Delta, F64),
    ("sim/flightmodel/failures/onground_any", &[], 1.0, OnChange, Delta, Bool),
    ("sim/flightmodel/position/magnetic_variation", &[], 0.1, OnChange, Delta, F64),
    ("sim/cockpit/misc/barometer_setting", &[], 0.01, OnChange, Delta, F64),
    ("sim/weather/temperature_ambient_c", &[], 0.1, OnChange, Delta, F64),
    ("sim/weather/wind_speed_kt", &[], 0.1, OnChange, Delta, F64),
    ("sim/weather/wind_direction_degt", &[], 1.0, OnChange, Delta, F64),
    ("sim/cockpit2/fuel/fuel_quantity", &[0, 1], 0.1, Slow, Delta, F64),
    ("sim/cockpit2/engine/indicators/fuel_flow_kg_sec", &[0, 1], 0.0001, Slow, Delta, F64),
    ("sim/cockpit2/engine/indicators/oil_temperature_deg_C", &[0, 1], 0.1, Slow, Delta, F64),
    ("sim/cockpit2/engine/indicators/oil_pressure_psi", &[0, 1], 0.1, Slow, Delta, F64),
    ("sim/cockpit2/engine/indicators/MPR_in_hg", &[0, 1], 0.01, Slow, Delta, F64),
    ("sim/cockpit2/engine/indicators/engine_speed_rpm", &[0, 1], 1.0, Slow, Delta, F64),
    ("sim/cockpit2/engine/indicators/CHT_deg_cel", &[0, 1], 0.1, Slow, Delta, F64),
    ("sim/cockpit2/engine/indicators/EGT_deg_cel", &[0, 1], 0.1, Slow, Delta, F64),
    ("sim/cockpit2/engine/indicators/ITT_deg_C", &[0, 1], 0.1, Slow, Delta, F64),
    ("sim/cockpit2/engine/indicators/power_watts", &[0, 1], 100.0, Slow, Delta, F64),
    ("sim/cockpit2/electrical/bus_volts", &[0, 1], 0.1, Slow, Delta, F64),
    ("sim/cockpit2/electrical/generator_amps", &[0, 1], 0.1, Slow, Delta, F64),
    ("sim/cockpit2/autopilot/autopilot_on", &[], 1.0, OnChange, Delta, Bool),
    ("sim/cockpit2/autopilot/roll_status", &[], 1.0, OnChange, Delta, Enum),
    ("sim/cockpit2/autopilot/pitch_status", &[], 1.0, OnChange, Delta, Enum),
    ("sim/cockpit2/autopilot/sync_hold_pitch_deg", &[], 0.1, OnChange, Delta, F64),
    ("sim/cockpit2/autopilot/sync_hold_roll_deg", &[], 0.1, OnChange, Delta, F64),
    ("sim/cockpit2/autopilot/vvi_dial_fpm", &[], 1.0, OnChange, Delta, F64),
    ("sim/cockpit2/radios/actuators/HSI_source_select_pilot", &[], 1.0, OnChange, Delta, Enum),
    ("sim/cockpit2/radios/actuators/hsi_obs_deg_mag_pilot", &[], 0.1, OnChange, Delta, F64),
    ("sim/cockpit2/radios/actuators/nav1_frequency_hz", &[], 1.0, OnChange, Delta, Enum),
    ("sim/cockpit2/radios/actuators/nav2_frequency_hz", &[], 1.0, OnChange, Delta, Enum),
    ("sim/cockpit2/radios/actuators/com1_frequency_hz_833", &[], 1.0, OnChange, Delta, Enum),
    ("sim/cockpit2/radios/actuators/com2_frequency_hz_833", &[], 1.0, OnChange, Delta, Enum),
    ("sim/cockpit2/radios/indicators/nav1_hdef_dots_pilot", &[], 0.01, OnChange, Delta, F64),
    ("sim/cockpit2/radios/indicators/nav1_vdef_dots_pilot", &[], 0.01, OnChange, Delta, F64),
    ("sim/cockpit2/radios/indicators/gps_dme_distance_nm", &[], 0.01, OnChange, Delta, F64),
    ("sim/cockpit2/radios/indicators/gps_bearing_deg_mag", &[], 0.1, OnChange, Delta, F64),
    ("sim/cockpit2/pressurization/indicators/cabin_altitude_ft", &[], 1.0, OnChange, DeltaOfDelta, F64),
    ("sim/aircraft/parts/acf_gear_deploy", &[0, 1, 2], 0.01, OnChange, Delta, F64),
    ("sim/cockpit2/controls/parking_brake_ratio", &[], 0.01, OnChange, Delta, F64),
    ("sim/cockpit2/switches/navigation_lights_on", &[], 1.0, OnChange, Delta, Bool),
    ("sim/cockpit2/switches/beacon_on", &[], 1.0, OnChange, Delta, Bool),
    ("sim/cockpit2/switches/strobe_lights_on", &[], 1.0, OnChange, Delta, Bool),
    ("sim/cockpit2/switches/taxi_light_on", &[], 1.0, OnChange, Delta, Bool),
    ("sim/cockpit2/switches/landing_lights_on", &[], 1.0, OnChange, Delta, Bool),
    (PAUSED, &[], 1.0, OnChange, Delta, Bool),
    (REPLAY, &[], 1.0, OnChange, Delta, Enum),
    (SIM_SPEED, &[], 1.0, OnChange, Delta, Enum),
    (TIME_SPEED, &[], 1.0, OnChange, Delta, Enum),
    (CRASHED, &[], 1.0, OnChange, Delta, Bool),
];

const PAUSED: &str = "sim/time/paused";
/// Nonzero while a replay plays.
const REPLAY: &str = "sim/operation/prefs/replay_mode";
/// Flight model and time acceleration.
const SIM_SPEED: &str = "sim/time/sim_speed";
const TIME_SPEED: &str = "sim/time/ground_speed";
const CRASHED: &str = "sim/flightmodel2/misc/has_crashed";
const GROUNDSPEED: &str = "sim/flightmodel/position/groundspeed";
const HPATH: &str = "sim/flightmodel/position/hpath";
/// Below this groundspeed (m/s, about 2 kt) `hpath`, the direction of travel, is held at its
/// last value: X-Plane derives it from a near-zero velocity, so it jitters through every
/// heading while the aircraft stands still, and that made almost every frame a change.
const HPATH_MIN_GROUNDSPEED: f64 = 1.0;

/// One channel per scalar dataref or array element: (channel name, dataref, element).
fn elements() -> Vec<(String, &'static str, Option<usize>)> {
    DATAREFS
        .iter()
        .flat_map(|(dataref, indices, ..)| {
            if indices.is_empty() {
                vec![(dataref.to_string(), *dataref, None)]
            } else {
                indices.iter().map(|i| (format!("{dataref}[{i}]"), *dataref, Some(*i))).collect()
            }
        })
        .collect()
}

fn channels() -> Vec<Channel> {
    let specs = DATAREFS.iter().flat_map(|spec| std::iter::repeat_n(spec, spec.1.len().max(1)));
    elements()
        .into_iter()
        .zip(specs)
        .enumerate()
        .map(|(id, ((name, ..), (_, _, quantum, rate_class, delta, kind)))| Channel {
            id: id as u16,
            name,
            unit: String::new(),
            kind: *kind,
            quantum: *quantum,
            rate_class: *rate_class,
            delta: *delta,
        })
        .collect()
}

fn id(name: &str) -> u16 {
    elements().iter().position(|(n, ..)| n == name).expect("dataref in the table") as u16
}

/// The datarefs to subscribe to for telemetry, beside the monitor's own.
pub fn datarefs() -> impl Iterator<Item = &'static str> {
    DATAREFS.iter().map(|(d, ..)| *d).chain([SIM_TIME])
}

/// A dataref value as the Web API sends it: a number, a boolean, or an array of them.
fn number(value: &Value, element: Option<usize>) -> Option<f64> {
    let v = match element {
        Some(i) => value.as_array()?.get(i)?,
        None => value.as_array().and_then(|a| a.first()).unwrap_or(value),
    };
    v.as_f64().or_else(|| v.as_bool().map(f64::from))
}

/// "xplane12" for X-Plane 12 (internal version 12xxxx), "xplane11" for 11.
fn sim_name(version: Option<f64>) -> &'static str {
    match version.map(|v| (v / 10_000.0) as u32) {
        Some(12) => "xplane12",
        Some(11) => "xplane11",
        _ => "xplane",
    }
}

/// What the plugin link subscribes to: each channel's dataref and element, in channel order.
pub fn plugin_entries() -> Vec<(&'static str, Option<usize>)> {
    elements().into_iter().map(|(_, dataref, element)| (dataref, element)).collect()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Capture {
    Plugin,
    WebApi,
}

impl Capture {
    fn name(self) -> &'static str {
        match self {
            Capture::Plugin => "xp_plugin",
            Capture::WebApi => "xp_webapi",
        }
    }
}

pub struct XplaneSampler {
    recorder: Recorder,
    capture: Capture,
    /// The aircraft being recorded, to start again when the capture changes.
    info: Option<AircraftInfo>,
    /// Web API dataref id to the channels it feeds: (element, channel id).
    by_id: HashMap<String, Vec<(Option<usize>, u16)>>,
    sim_time_id: Option<String>,
    version: Option<f64>,
    paused: Option<bool>,
    replay: Option<bool>,
    crashed: Option<bool>,
    sim_rate: f64,
    time_rate: f64,
    /// The latest groundspeed, m/s; see [`HPATH_MIN_GROUNDSPEED`].
    groundspeed: f64,
    ids: StateIds,
    /// The runways of a touchdown being looked up in X-Plane's scenery.
    runways: Option<mpsc::Receiver<Vec<SimRunways>>>,
}

/// The channels that carry sim state.
struct StateIds {
    paused: u16,
    replay: u16,
    crashed: u16,
    sim_speed: u16,
    time_speed: u16,
    groundspeed: u16,
    hpath: u16,
}

impl XplaneSampler {
    /// Records over this connection when recording is switched on. `path_to_id` is the
    /// Web API's ids for the datarefs it found; `version` X-Plane's internal version;
    /// `plugin` whether the plugin link is up and frames will come from it.
    pub fn register(
        app: &AppHandle,
        path_to_id: &HashMap<String, String>,
        version: Option<f64>,
        plugin: bool,
    ) -> Option<Self> {
        if !enabled(app) {
            return None;
        }
        let mut by_id: HashMap<String, Vec<(Option<usize>, u16)>> = HashMap::new();
        let mut found = 0;
        for (channel, (_, dataref, element)) in elements().into_iter().enumerate() {
            if let Some(api_id) = path_to_id.get(dataref) {
                by_id.entry(api_id.clone()).or_default().push((element, channel as u16));
                found += 1;
            }
        }
        let sim_time_id = path_to_id.get(SIM_TIME).cloned();
        if sim_time_id.is_none() && !plugin {
            crate::append_log(app, format!("[Telemetry] {SIM_TIME} not found; not recording."));
            return None;
        }
        let capture = if plugin { Capture::Plugin } else { Capture::WebApi };
        if !plugin {
            crate::append_log(app, format!("[Telemetry] Recording X-Plane telemetry ({found} of {} channels).", elements().len()));
        }
        let phase = PhaseInputs::xplane(
            id("sim/flightmodel/position/y_agl"),
            id("sim/flightmodel/failures/onground_any"),
            id("sim/flightmodel/position/vh_ind"),
            id("sim/flightmodel/position/phi"),
        );
        let mut recorder = Recorder::new(app, "xplane", &channels(), phase);
        recorder.watch_touchdowns(
            id("sim/flightmodel/failures/onground_any"),
            id("sim/flightmodel/position/latitude"),
            id("sim/flightmodel/position/longitude"),
        );
        Some(Self {
            recorder,
            capture,
            info: None,
            by_id,
            sim_time_id,
            version,
            paused: None,
            replay: None,
            crashed: None,
            sim_rate: 1.0,
            time_rate: 1.0,
            groundspeed: 0.0,
            ids: StateIds {
                paused: id(PAUSED),
                replay: id(REPLAY),
                crashed: id(CRASHED),
                sim_speed: id(SIM_SPEED),
                time_speed: id(TIME_SPEED),
                groundspeed: id(GROUNDSPEED),
                hpath: id(HPATH),
            },
            runways: None,
        })
    }

    /// Looks up the runways of a touchdown in X-Plane's scenery, off this thread, and
    /// records those found.
    fn runways(&mut self) {
        if let Some(rx) = &self.runways {
            match rx.try_recv() {
                Ok(found) => {
                    found.iter().for_each(|r| self.recorder.runways(r));
                    self.runways = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.runways = None,
            }
        }
        let Some(at) = self.recorder.take_touchdown() else { return };
        let (tx, rx) = mpsc::channel();
        let installs = crate::xp_plugin_install::installs();
        std::thread::spawn(move || {
            let _ = tx.send(super::runways::xplane_near(&installs, at));
        });
        self.runways = Some(rx);
    }

    fn flags(&self) -> u8 {
        let mut f = 0;
        if self.paused == Some(true) {
            f |= flags::PAUSED;
        }
        if self.replay == Some(true) {
            f |= flags::REPLAY;
        }
        if self.sim_rate != 1.0 || self.time_rate != 1.0 {
            f |= flags::SIM_RATE;
        }
        f
    }

    /// Records `on` or `off` when a state seen before changes, or is first seen on.
    fn state(&mut self, previous: Option<bool>, now: bool, on: EventKind, off: EventKind) {
        if previous != Some(now) && (previous.is_some() || now) {
            self.recorder.event(Event::new(if now { on } else { off }));
        }
    }

    /// A channel's new value, noting the sim state it carries.
    fn apply(&mut self, channel: u16, v: f64) {
        if channel == self.ids.groundspeed {
            self.groundspeed = v;
        }
        if !keeps(channel, self.ids.hpath, self.groundspeed) {
            return;
        }
        self.recorder.set(channel, v);
        let on = v != 0.0;
        match channel {
            c if c == self.ids.paused => {
                self.state(self.paused, on, EventKind::Paused, EventKind::Unpaused);
                self.paused = Some(on);
            }
            c if c == self.ids.replay => {
                self.state(self.replay, on, EventKind::ReplayStarted, EventKind::ReplayEnded);
                self.replay = Some(on);
            }
            c if c == self.ids.crashed => {
                self.state(self.crashed, on, EventKind::Crashed, EventKind::CrashReset);
                self.crashed = Some(on);
            }
            c if c == self.ids.sim_speed => self.sim_rate = v,
            c if c == self.ids.time_speed => self.time_rate = v,
            _ => {}
        }
    }

    /// One `dataref_update_values` message: dataref id to value, for those that changed.
    /// Ignored while the plugin captures.
    pub fn on_update(&mut self, data: &serde_json::Map<String, Value>) {
        if self.capture != Capture::WebApi {
            return;
        }
        for (api_id, value) in data {
            if Some(api_id) == self.sim_time_id.as_ref() {
                if let Some(t) = number(value, None) {
                    self.recorder.set_sim_time(t);
                }
                continue;
            }
            let Some(targets) = self.by_id.get(api_id).cloned() else { continue };
            for (element, channel) in targets {
                if let Some(v) = number(value, element) {
                    self.apply(channel, v);
                }
            }
        }
        self.runways();
        let flags = self.flags();
        self.recorder.tick(flags);
    }

    /// One frame from the plugin: a value per channel, in channel order, and whether X-Plane
    /// has each one's dataref.
    pub fn on_frame(&mut self, sim_time: f64, wall_ms: i64, values: &[f64], found: &[bool]) {
        if self.capture != Capture::Plugin {
            return;
        }
        for (channel, (v, found)) in values.iter().zip(found).enumerate() {
            if *found {
                self.apply(channel as u16, *v);
            }
        }
        self.recorder.set_sim_time(sim_time);
        self.recorder.set_wall_time(wall_ms);
        self.runways();
        let flags = self.flags();
        self.recorder.tick(flags);
    }

    /// A sim event from the plugin (`xp_plugin::EVENT_*`).
    pub fn on_plugin_event(&mut self, kind: u8, detail: String) {
        let kind = match kind {
            super::xp_plugin::EVENT_AIRCRAFT_LOADED => EventKind::AircraftLoaded,
            super::xp_plugin::EVENT_AIRPORT_LOADED => EventKind::FlightLoaded,
            _ => return,
        };
        self.recorder.event(Event { kind, detail });
    }

    /// The plugin link went away: the session ends, and the aircraft is recorded again
    /// through the Web API, in a new session, so each session has one capture.
    pub fn plugin_lost(&mut self) {
        if self.capture != Capture::Plugin {
            return;
        }
        self.recorder.stop();
        self.capture = Capture::WebApi;
        if self.sim_time_id.is_none() {
            return;
        }
        if let Some(info) = self.info.clone() {
            self.aircraft_loaded(&info);
        }
    }

    /// The monitor saw the aircraft jump: recorded, then the session ends with the
    /// connection.
    pub fn on_teleport(&mut self) {
        self.recorder.event(Event::new(EventKind::PositionChanged));
        let flags = self.flags();
        self.recorder.tick(flags);
        self.recorder.stop();
    }

    /// The aircraft's identity arrived: starts a session unless this aircraft is already
    /// being recorded.
    pub fn aircraft_loaded(&mut self, info: &AircraftInfo) {
        self.info = Some(info.clone());
        let open = SessionOpen {
            header: SessionHeader {
                protocol_version: PROTOCOL_VERSION,
                sim: sim_name(self.version).into(),
                sim_version: self.version.map(|v| format!("{v:.0}")).unwrap_or_default(),
                capture: self.capture.name().into(),
                client_version: self.recorder.app_version(),
                aircraft: aircraft(info),
            },
            channels: channels(),
        };
        let flags = self.flags();
        self.recorder.start(info, &open, flags);
    }
}

/// Whether a channel's new value is recorded: all are but `hpath` when nearly stopped.
fn keeps(channel: u16, hpath: u16, groundspeed: f64) -> bool {
    channel != hpath || groundspeed >= HPATH_MIN_GROUNDSPEED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_track_is_held_while_nearly_stopped() {
        let (hpath, gs) = (id(HPATH), id(GROUNDSPEED));
        assert!(!keeps(hpath, hpath, 0.0), "parked");
        assert!(!keeps(hpath, hpath, 0.9));
        assert!(keeps(hpath, hpath, 1.0), "taxiing at 2 kt");
        assert!(keeps(gs, hpath, 0.0), "other channels always");
        // Frames come in channel order: groundspeed is applied before hpath.
        assert!(gs < hpath);
    }

    #[test]
    fn the_table_is_valid_and_has_the_phase_inputs() {
        let table = channels();
        assert_eq!(table.len(), elements().len());
        let encoded = butterlog_wire::encode_channels(&table);
        assert_eq!(butterlog_wire::decode_channels(&encoded).unwrap(), table);
        let mut names: Vec<_> = table.iter().map(|c| c.name.clone()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), table.len(), "no channel twice");
        assert_eq!(table[id("sim/cockpit2/fuel/fuel_quantity[1]") as usize].quantum, 0.1);
        assert_eq!(table[id(CRASHED) as usize].kind, Bool);
    }

    #[test]
    fn values_come_as_numbers_booleans_or_arrays() {
        assert_eq!(number(&serde_json::json!(3.5), None), Some(3.5));
        assert_eq!(number(&serde_json::json!(true), None), Some(1.0));
        assert_eq!(number(&serde_json::json!([1.0, 2.0]), Some(1)), Some(2.0));
        assert_eq!(number(&serde_json::json!([1.0, 2.0]), None), Some(1.0));
        assert_eq!(number(&serde_json::json!([1.0]), Some(3)), None);
    }

    #[test]
    fn sim_names_follow_the_version() {
        assert_eq!(sim_name(Some(121_400.0)), "xplane12");
        assert_eq!(sim_name(Some(114_100.0)), "xplane11");
        assert_eq!(sim_name(None), "xplane");
    }
}
