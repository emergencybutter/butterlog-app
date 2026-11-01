//! The X-Plane sampler over the Web API (design §4.1, the `xp_webapi` fallback): the
//! monitor's WebSocket subscription carries the datarefs below, and each update goes
//! through the shared sampler. Until the plugin captures every frame (rollout phase 4),
//! this is how X-Plane sessions reach the service.
//!
//! The Web API sends about 8 updates a second, only for datarefs that changed exactly,
//! and nothing while paused. X-Plane reports no events over it, so they are inferred:
//! pause, replay and crash from their datarefs changing, a position change from the
//! monitor's teleport check. A session is one aircraft on one connection, as for MSFS.
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
use std::collections::HashMap;
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

pub struct XplaneSampler {
    recorder: Recorder,
    /// Web API dataref id to the channels it feeds: (element, channel id).
    by_id: HashMap<String, Vec<(Option<usize>, u16)>>,
    sim_time_id: Option<String>,
    version: Option<f64>,
    paused: Option<bool>,
    replay: Option<bool>,
    crashed: Option<bool>,
    sim_rate: f64,
    time_rate: f64,
}

impl XplaneSampler {
    /// Records over this connection when recording is switched on. `path_to_id` is the
    /// Web API's ids for the datarefs it found; `version` X-Plane's internal version.
    pub fn register(app: &AppHandle, path_to_id: &HashMap<String, String>, version: Option<f64>) -> Option<Self> {
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
        if sim_time_id.is_none() {
            crate::append_log(app, format!("[Telemetry] {SIM_TIME} not found; not recording."));
            return None;
        }
        crate::append_log(app, format!("[Telemetry] Recording X-Plane telemetry ({found} of {} channels).", elements().len()));
        let phase = PhaseInputs::xplane(
            id("sim/flightmodel/position/y_agl"),
            id("sim/flightmodel/failures/onground_any"),
            id("sim/flightmodel/position/vh_ind"),
            id("sim/flightmodel/position/phi"),
        );
        Some(Self {
            recorder: Recorder::new(app, &channels(), phase),
            by_id,
            sim_time_id,
            version,
            paused: None,
            replay: None,
            crashed: None,
            sim_rate: 1.0,
            time_rate: 1.0,
        })
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

    /// One `dataref_update_values` message: dataref id to value, for those that changed.
    pub fn on_update(&mut self, data: &serde_json::Map<String, Value>) {
        let (paused, replay, crashed) = (id(PAUSED), id(REPLAY), id(CRASHED));
        let (sim_speed, time_speed) = (id(SIM_SPEED), id(TIME_SPEED));
        for (api_id, value) in data {
            if Some(api_id) == self.sim_time_id.as_ref() {
                if let Some(t) = number(value, None) {
                    self.recorder.set_sim_time(t);
                }
                continue;
            }
            let Some(targets) = self.by_id.get(api_id).cloned() else { continue };
            for (element, channel) in targets {
                let Some(v) = number(value, element) else { continue };
                self.recorder.set(channel, v);
                let on = v != 0.0;
                match channel {
                    c if c == paused => {
                        self.state(self.paused, on, EventKind::Paused, EventKind::Unpaused);
                        self.paused = Some(on);
                    }
                    c if c == replay => {
                        self.state(self.replay, on, EventKind::ReplayStarted, EventKind::ReplayEnded);
                        self.replay = Some(on);
                    }
                    c if c == crashed => {
                        self.state(self.crashed, on, EventKind::Crashed, EventKind::CrashReset);
                        self.crashed = Some(on);
                    }
                    c if c == sim_speed => self.sim_rate = v,
                    c if c == time_speed => self.time_rate = v,
                    _ => {}
                }
            }
        }
        let flags = self.flags();
        self.recorder.tick(flags);
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
        let open = SessionOpen {
            header: SessionHeader {
                protocol_version: PROTOCOL_VERSION,
                sim: sim_name(self.version).into(),
                sim_version: self.version.map(|v| format!("{v:.0}")).unwrap_or_default(),
                capture: "xp_webapi".into(),
                client_version: self.recorder.app_version(),
                aircraft: aircraft(info),
            },
            channels: channels(),
        };
        let flags = self.flags();
        self.recorder.start(info, &open, flags);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
