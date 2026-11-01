//! The MSFS sampler (design §3.2, rollout phase 2): asks SimConnect for every SimVar on
//! every sim frame, changed values only (`CHANGED | TAGGED`), and feeds them through the
//! shared [`Sampler`] into [`Telemetry`].
//!
//! A session is one aircraft loaded on one connection: it starts when the aircraft's title
//! arrives, and ends on `SimStop`, when another aircraft loads, or when the connection
//! closes (on drop).
//!
//! Behind a hidden switch while the pipeline runs in shadow mode: a file named
//! [`SWITCH_FILE`] in the app's data directory, and being signed in.

use super::{RecordError, Telemetry};
use crate::models::AircraftInfo;
use butterlog_wire::{
    flags, Aircraft, Channel, DeltaOrder, Kind, PhaseInputs, RateClass, Sampler, SessionHeader,
    SessionOpen, PROTOCOL_VERSION,
};
use simplesimconnect::{
    SimConnect, SimInfo, DATA_REQUEST_FLAG_CHANGED, DATA_REQUEST_FLAG_TAGGED, DWORD,
    OBJECT_ID_USER, PERIOD_SIM_FRAME,
};
use tauri::{AppHandle, Manager};

/// Ids on the monitor's SimConnect connection, clear of the monitor's own.
pub const DEFINE_ID: DWORD = 20;
pub const REQUEST_ID: DWORD = 20;
pub const EVENT_PAUSE: DWORD = 20;

/// The datum id of `SIMULATION TIME`, which goes in each tick's header, not a channel.
const SIM_TIME_DATUM: DWORD = 0xFFFF;

pub const SWITCH_FILE: &str = "telemetry_beta";

use DeltaOrder::{Delta, DeltaOfDelta};
use RateClass::{Frame, OnChange, Slow};

/// SimVar, unit, quantum, rate class and delta order. A channel's id is its position here.
/// Quanta and classes are the ones rollout phase 1 measured (`measure_capture`).
const SIMVARS: &[(&str, &str, f64, RateClass, DeltaOrder)] = &[
    ("PLANE LATITUDE", "degrees", 1e-7, Frame, DeltaOfDelta),
    ("PLANE LONGITUDE", "degrees", 1e-7, Frame, DeltaOfDelta),
    ("INDICATED ALTITUDE", "feet", 0.1, Frame, DeltaOfDelta),
    ("KOHLSMAN SETTING HG", "inHg", 0.01, OnChange, Delta),
    ("PLANE ALTITUDE", "feet", 0.1, Frame, DeltaOfDelta),
    ("AMBIENT TEMPERATURE", "celsius", 0.1, OnChange, Delta),
    ("AIRSPEED INDICATED", "knots", 0.1, Frame, Delta),
    ("GROUND VELOCITY", "knots", 0.1, Frame, Delta),
    ("VERTICAL SPEED", "feet per minute", 1.0, Frame, Delta),
    ("PLANE PITCH DEGREES", "degrees", 0.01, Frame, DeltaOfDelta),
    ("PLANE BANK DEGREES", "degrees", 0.01, Frame, DeltaOfDelta),
    ("ACCELERATION BODY X", "G force", 0.001, Frame, Delta),
    ("ACCELERATION BODY Z", "G force", 0.001, Frame, Delta),
    ("PLANE HEADING DEGREES TRUE", "degrees", 0.01, Frame, DeltaOfDelta),
    ("GPS GROUND TRUE TRACK", "degrees", 0.01, Frame, DeltaOfDelta),
    ("ELECTRICAL MAIN BUS VOLTAGE:1", "volts", 0.1, Slow, Delta),
    ("ELECTRICAL MAIN BUS VOLTAGE:2", "volts", 0.1, Slow, Delta),
    ("ELECTRICAL MAIN BUS AMPS:1", "amps", 0.1, Slow, Delta),
    ("FUEL LEFT QUANTITY", "gallons", 0.01, Slow, Delta),
    ("FUEL RIGHT QUANTITY", "gallons", 0.01, Slow, Delta),
    ("ENG FUEL FLOW GPH:1", "gallons per hour", 0.1, Slow, Delta),
    ("ENG OIL TEMPERATURE:1", "farenheit", 0.1, Slow, Delta),
    ("ENG OIL PRESSURE:1", "psi", 0.1, Slow, Delta),
    ("ENG MANIFOLD PRESSURE:1", "inHg", 0.01, Slow, Delta),
    ("GENERAL ENG RPM:1", "rpm", 1.0, Slow, Delta),
    ("GENERAL ENG PCT MAX RPM:1", "percent", 0.1, Slow, Delta),
    ("ENG CYLINDER HEAD TEMPERATURE:1", "farenheit", 0.1, Slow, Delta),
    ("ENG EXHAUST GAS TEMPERATURE:1", "farenheit", 0.1, Slow, Delta),
    ("GPS POSITION ALT", "feet", 0.1, Frame, DeltaOfDelta),
    ("AIRSPEED TRUE", "knots", 0.1, Frame, Delta),
    ("GPS DRIVES NAV1", "bool", 1.0, OnChange, Delta),
    ("NAV OBS:1", "degrees", 0.1, OnChange, Delta),
    ("NAV ACTIVE FREQUENCY:1", "MHz", 0.001, OnChange, Delta),
    ("NAV ACTIVE FREQUENCY:2", "MHz", 0.001, OnChange, Delta),
    ("COM ACTIVE FREQUENCY:1", "MHz", 0.001, OnChange, Delta),
    ("COM ACTIVE FREQUENCY:2", "MHz", 0.001, OnChange, Delta),
    ("NAV CDI:1", "number", 1.0, OnChange, Delta),
    ("NAV GSI:1", "number", 1.0, OnChange, Delta),
    ("AMBIENT WIND VELOCITY", "knots", 0.1, OnChange, Delta),
    ("AMBIENT WIND DIRECTION", "degrees", 1.0, OnChange, Delta),
    ("GPS WP DISTANCE", "nautical miles", 0.01, OnChange, Delta),
    ("GPS WP BEARING", "degrees", 0.1, OnChange, Delta),
    ("MAGVAR", "degrees", 0.1, OnChange, Delta),
    ("AUTOPILOT MASTER", "bool", 1.0, OnChange, Delta),
    ("AUTOPILOT BANK HOLD", "bool", 1.0, OnChange, Delta),
    ("AUTOPILOT PITCH HOLD", "bool", 1.0, OnChange, Delta),
    ("AUTOPILOT BANK HOLD REF", "degrees", 0.1, OnChange, Delta),
    ("AUTOPILOT PITCH HOLD REF", "degrees", 0.1, OnChange, Delta),
    ("AUTOPILOT VERTICAL HOLD VAR", "feet per minute", 1.0, OnChange, Delta),
    ("SIM ON GROUND", "bool", 1.0, OnChange, Delta),
    ("PLANE ALT ABOVE GROUND", "feet", 0.1, Frame, DeltaOfDelta),
    ("G FORCE", "gforce", 0.001, Frame, Delta),
    ("PRESSURE ALTITUDE", "feet", 1.0, Frame, DeltaOfDelta),
    ("DENSITY ALTITUDE", "feet", 1.0, OnChange, DeltaOfDelta),
    ("PRESSURIZATION CABIN ALTITUDE", "feet", 1.0, OnChange, DeltaOfDelta),
    ("GEAR TOTAL PCT EXTENDED", "percent over 100", 0.01, OnChange, Delta),
    ("BRAKE PARKING POSITION", "bool", 1.0, OnChange, Delta),
    ("LIGHT NAV", "bool", 1.0, OnChange, Delta),
    ("LIGHT BEACON", "bool", 1.0, OnChange, Delta),
    ("LIGHT STROBE", "bool", 1.0, OnChange, Delta),
    ("LIGHT TAXI", "bool", 1.0, OnChange, Delta),
    ("LIGHT LANDING", "bool", 1.0, OnChange, Delta),
    ("SIMULATION RATE", "number", 0.01, OnChange, Delta),
    ("IS SLEW ACTIVE", "bool", 1.0, OnChange, Delta),
];

fn channels() -> Vec<Channel> {
    SIMVARS
        .iter()
        .enumerate()
        .map(|(id, (name, unit, quantum, rate_class, delta))| Channel {
            id: id as u16,
            name: (*name).into(),
            unit: (*unit).into(),
            kind: if *unit == "bool" { Kind::Bool } else { Kind::F64 },
            quantum: *quantum,
            rate_class: *rate_class,
            delta: *delta,
        })
        .collect()
}

fn id(name: &str) -> u16 {
    SIMVARS.iter().position(|(n, ..)| *n == name).expect("SimVar in the table") as u16
}

/// "msfs2024" for MSFS 2024 (application version 12), "msfs2020" for version 11.
fn sim_name(info: &SimInfo) -> &'static str {
    match info.version[0] {
        12 => "msfs2024",
        11 => "msfs2020",
        _ => "msfs",
    }
}

/// Whether to record: the hidden switch, a signed-in app, and a spool.
fn enabled(app: &AppHandle) -> bool {
    let switch = app
        .path()
        .app_data_dir()
        .is_ok_and(|d| d.join(SWITCH_FILE).exists());
    switch
        && app.state::<crate::config::ConfigManager>().get_config().is_logged_in()
        && app.try_state::<Telemetry>().is_some()
}

pub struct MsfsSampler {
    app: AppHandle,
    sampler: Sampler,
    sim: Option<SimInfo>,
    sim_time: Option<f64>,
    paused: bool,
    slew: bool,
    sim_rate: f64,
    /// Title and livery of the aircraft being recorded.
    recording: Option<(String, String)>,
}

impl MsfsSampler {
    /// Asks SimConnect for the telemetry when recording is switched on; `None` otherwise,
    /// and for the whole connection.
    pub fn register(app: &AppHandle, sc: &SimConnect) -> anyhow::Result<Option<Self>> {
        if !enabled(app) {
            return Ok(None);
        }
        sc.add_tagged_f64_to_data_definition(DEFINE_ID, "SIMULATION TIME", "seconds", 0.0, SIM_TIME_DATUM)?;
        for (n, (name, unit, quantum, ..)) in SIMVARS.iter().enumerate() {
            // Half a quantum: the sim reports a move before it changes the quantised value.
            sc.add_tagged_f64_to_data_definition(DEFINE_ID, name, unit, (quantum / 2.0) as f32, n as DWORD)?;
        }
        sc.subscribe_to_system_event(EVENT_PAUSE, "Pause")?;
        sc.request_data_on_sim_object_with_flags(
            REQUEST_ID,
            DEFINE_ID,
            OBJECT_ID_USER,
            PERIOD_SIM_FRAME,
            DATA_REQUEST_FLAG_CHANGED | DATA_REQUEST_FLAG_TAGGED,
        )?;
        crate::append_log(app, format!("[Telemetry] Recording MSFS telemetry ({} SimVars).", SIMVARS.len()));
        let phase = PhaseInputs::msfs(
            id("PLANE ALT ABOVE GROUND"),
            id("SIM ON GROUND"),
            id("VERTICAL SPEED"),
            id("PLANE BANK DEGREES"),
        );
        Ok(Some(Self {
            app: app.clone(),
            sampler: Sampler::new(&channels(), phase),
            sim: None,
            sim_time: None,
            paused: false,
            slew: false,
            sim_rate: 1.0,
            recording: None,
        }))
    }

    pub fn on_open(&mut self, info: SimInfo) {
        self.sim = Some(info);
    }

    fn flags(&self) -> u8 {
        let mut f = 0;
        if self.paused {
            f |= flags::PAUSED;
        }
        if self.slew {
            f |= flags::SLEW;
        }
        if self.sim_rate != 1.0 {
            f |= flags::SIM_RATE;
        }
        f
    }

    /// One SimConnect message: the datums that changed this frame.
    pub fn on_data(&mut self, data: &[(DWORD, f64)]) {
        let (slew, rate) = (id("IS SLEW ACTIVE") as DWORD, id("SIMULATION RATE") as DWORD);
        for (datum, value) in data {
            match *datum {
                SIM_TIME_DATUM => self.sim_time = Some(*value),
                d if d == slew => self.slew = *value != 0.0,
                d if d == rate => self.sim_rate = *value,
                _ => {}
            }
            if let Ok(channel) = u16::try_from(*datum) {
                self.sampler.set(channel, *value);
            }
        }
        self.tick();
    }

    pub fn on_pause(&mut self, paused: bool) {
        self.paused = paused;
        self.tick();
    }

    fn tick(&mut self) {
        let (Some(sim_time), Some(_)) = (self.sim_time, &self.recording) else {
            return;
        };
        let wall_ms = chrono::Utc::now().timestamp_millis();
        let Some(tick) = self.sampler.tick(sim_time, wall_ms, self.flags()) else {
            return;
        };
        let telemetry = self.app.state::<Telemetry>();
        match telemetry.record(tick) {
            Ok(()) => {}
            Err(e @ RecordError::Encode(_)) => {
                crate::append_log(&self.app, format!("[Telemetry] {e}"));
            }
            Err(e @ RecordError::Spool(_)) => {
                crate::append_log(&self.app, format!("[Telemetry] {e}"));
                self.recording = None;
            }
        }
    }

    /// The aircraft's identity arrived: starts a session unless this aircraft is already
    /// being recorded.
    pub fn aircraft_loaded(&mut self, info: &AircraftInfo) {
        let key = (info.title.clone(), info.livery.clone());
        if info.title.is_empty() || self.recording.as_ref() == Some(&key) || !enabled(&self.app) {
            return;
        }
        let sim = self.sim.as_ref();
        let open = SessionOpen {
            header: SessionHeader {
                protocol_version: PROTOCOL_VERSION,
                sim: sim.map_or("msfs", sim_name).into(),
                sim_version: sim
                    .map(|s| s.version.map(|v| v.to_string()).join("."))
                    .unwrap_or_default(),
                capture: "simconnect".into(),
                client_version: self.app.package_info().version.to_string(),
                aircraft: Aircraft {
                    title: info.title.clone(),
                    icao: info.atc_model.clone(),
                    tail: info.atc_id.clone(),
                    livery: info.livery.clone(),
                    engine_type: info.engine_type.clone(),
                    engine_count: info.num_engines.clamp(0, 255) as u8,
                },
            },
            channels: channels(),
        };
        match self.app.state::<Telemetry>().start(&open) {
            Ok(session) => {
                crate::append_log(&self.app, format!("[Telemetry] Session {session} started: {}", info.title));
                self.sampler.restart();
                self.recording = Some(key);
                self.tick();
            }
            Err(e) => {
                crate::append_log(&self.app, format!("[Telemetry] Could not start a session: {e}"));
                self.recording = None;
            }
        }
    }

    /// Ends the session being recorded, if any.
    pub fn stop(&mut self) {
        if self.recording.take().is_none() {
            return;
        }
        if let Err(e) = self.app.state::<Telemetry>().stop() {
            crate::append_log(&self.app, format!("[Telemetry] Could not end the session: {e}"));
        }
    }
}

impl Drop for MsfsSampler {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_valid_and_has_the_phase_inputs() {
        let table = channels();
        let encoded = butterlog_wire::encode_channels(&table);
        assert_eq!(butterlog_wire::decode_channels(&encoded).unwrap(), table);
        for name in ["PLANE ALT ABOVE GROUND", "SIM ON GROUND", "VERTICAL SPEED", "PLANE BANK DEGREES"] {
            id(name);
        }
        assert!(SIMVARS.len() < SIM_TIME_DATUM as usize);
        let mut names: Vec<_> = SIMVARS.iter().map(|(n, ..)| n).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), SIMVARS.len(), "no SimVar twice");
    }
}
