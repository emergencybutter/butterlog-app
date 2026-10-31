//! MSFS capture spike for the sim-native telemetry rollout, phase 1: requests the SimVars
//! the app logs today on every sim frame and writes them in the X-Plane capture format, so
//! `butterlog-wire`'s `measure_capture` example measures both sims the same way.
//!
//! ```text
//! cargo run --release --example msfs_capture -- [--secs 3600] [--out msfs_capture.blc]
//! ```
//!
//! Every SimVar every frame, unfiltered: quantise-then-compare runs offline. The real sampler
//! will request `CHANGED|TAGGED` instead (design §3.2), which the `simplesimconnect` wrapper
//! does not expose yet.

use simplesimconnect::*;
use simplesimconnect_sys::*;
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The app's `sim_monitor/msfs.rs` definition, in the same units, then sampler state.
const SIMVARS: &[(&str, &str)] = &[
    ("SIMULATION TIME", "seconds"),
    ("PLANE LATITUDE", "degrees"),
    ("PLANE LONGITUDE", "degrees"),
    ("INDICATED ALTITUDE", "feet"),
    ("KOHLSMAN SETTING HG", "inHg"),
    ("PLANE ALTITUDE", "feet"),
    ("AMBIENT TEMPERATURE", "celsius"),
    ("AIRSPEED INDICATED", "knots"),
    ("GROUND VELOCITY", "knots"),
    ("VERTICAL SPEED", "feet per minute"),
    ("PLANE PITCH DEGREES", "degrees"),
    ("PLANE BANK DEGREES", "degrees"),
    ("ACCELERATION BODY X", "G force"),
    ("ACCELERATION BODY Z", "G force"),
    ("PLANE HEADING DEGREES TRUE", "degrees"),
    ("GPS GROUND TRUE TRACK", "degrees"),
    ("ELECTRICAL MAIN BUS VOLTAGE:1", "volts"),
    ("ELECTRICAL MAIN BUS VOLTAGE:2", "volts"),
    ("ELECTRICAL MAIN BUS AMPS:1", "amps"),
    ("FUEL LEFT QUANTITY", "gallons"),
    ("FUEL RIGHT QUANTITY", "gallons"),
    ("ENG FUEL FLOW GPH:1", "gallons per hour"),
    ("ENG OIL TEMPERATURE:1", "farenheit"),
    ("ENG OIL PRESSURE:1", "psi"),
    ("ENG MANIFOLD PRESSURE:1", "inHg"),
    ("GENERAL ENG RPM:1", "rpm"),
    ("GENERAL ENG PCT MAX RPM:1", "percent"),
    ("ENG CYLINDER HEAD TEMPERATURE:1", "farenheit"),
    ("ENG EXHAUST GAS TEMPERATURE:1", "farenheit"),
    ("GPS POSITION ALT", "feet"),
    ("AIRSPEED TRUE", "knots"),
    ("GPS DRIVES NAV1", "bool"),
    ("NAV OBS:1", "degrees"),
    ("NAV ACTIVE FREQUENCY:1", "MHz"),
    ("NAV ACTIVE FREQUENCY:2", "MHz"),
    ("COM ACTIVE FREQUENCY:1", "MHz"),
    ("COM ACTIVE FREQUENCY:2", "MHz"),
    ("NAV CDI:1", "number"),
    ("NAV GSI:1", "number"),
    ("AMBIENT WIND VELOCITY", "knots"),
    ("AMBIENT WIND DIRECTION", "degrees"),
    ("GPS WP DISTANCE", "nautical miles"),
    ("GPS WP BEARING", "degrees"),
    ("MAGVAR", "degrees"),
    ("AUTOPILOT MASTER", "bool"),
    ("AUTOPILOT BANK HOLD", "bool"),
    ("AUTOPILOT PITCH HOLD", "bool"),
    ("AUTOPILOT BANK HOLD REF", "degrees"),
    ("AUTOPILOT PITCH HOLD REF", "degrees"),
    ("AUTOPILOT VERTICAL HOLD VAR", "feet per minute"),
    ("SIM ON GROUND", "bool"),
    ("PLANE ALT ABOVE GROUND", "feet"),
    ("G FORCE", "gforce"),
    ("PRESSURE ALTITUDE", "feet"),
    ("DENSITY ALTITUDE", "feet"),
    ("PRESSURIZATION CABIN ALTITUDE", "feet"),
    ("GEAR TOTAL PCT EXTENDED", "percent over 100"),
    ("BRAKE PARKING POSITION", "bool"),
    ("LIGHT NAV", "bool"),
    ("LIGHT BEACON", "bool"),
    ("LIGHT STROBE", "bool"),
    ("LIGHT TAXI", "bool"),
    ("LIGHT LANDING", "bool"),
    ("SIMULATION RATE", "number"),
    ("IS SLEW ACTIVE", "bool"),
];
const N: usize = SIMVARS.len();

const DEFINE_ID: DWORD = 1;
const REQUEST_ID: DWORD = 1;
const EVENT_PAUSE: DWORD = 1;

fn main() -> anyhow::Result<()> {
    let mut secs = 3600u64;
    let mut out_path = "msfs_capture.blc".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--secs" => secs = args.next().and_then(|v| v.parse().ok()).expect("--secs N"),
            "--out" => out_path = args.next().expect("--out PATH"),
            other => anyhow::bail!("unknown argument {other}"),
        }
    }

    let sc = SimConnect::open("ButterLog capture spike")?;
    for (name, unit) in SIMVARS {
        sc.add_to_data_definition::<f64>(DEFINE_ID, name, unit)?;
    }
    sc.subscribe_to_system_event(EVENT_PAUSE, "Pause")?;
    sc.request_data_on_sim_object(
        REQUEST_ID,
        DEFINE_ID,
        OBJECT_ID_USER,
        SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_SIM_FRAME,
    )?;

    // Channel 0 (SIMULATION TIME) goes in the record header; the pause event becomes a
    // channel named like the X-Plane one so the measuring tool flags it the same way.
    let mut names: Vec<String> = SIMVARS[1..].iter().map(|(n, _)| (*n).to_string()).collect();
    names.push("paused".to_string());
    let mut out = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&out_path)?);
    out.write_all(b"BLCAP1\n")?;
    writeln!(out, "{}", serde_json::json!({ "channels": names }))?;

    println!("Recording {secs} s of sim frames to {out_path}…");
    let start = Instant::now();
    let (mut frames, mut paused) = (0i64, 0.0f64);
    while start.elapsed() < Duration::from_secs(secs) {
        let Some(msg) = sc.get_next_dispatch()? else {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        };
        if msg.is_quit() {
            println!("Simulator quit.");
            break;
        }
        if let Some(event) = msg.as_event() {
            if event.event_id == EVENT_PAUSE {
                paused = event.data.into();
            }
            continue;
        }
        let Some(values) = msg.as_sim_object_data::<[f64; N]>() else { continue };
        let wall_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
        out.write_all(&values[0].to_le_bytes())?;
        out.write_all(&wall_ms.to_le_bytes())?;
        out.write_all(&frames.to_le_bytes())?;
        for v in &values[1..] {
            out.write_all(&v.to_le_bytes())?;
        }
        out.write_all(&paused.to_le_bytes())?;
        frames += 1;
        if frames % 3600 == 0 {
            println!("  {frames} frames, {:.0} s", start.elapsed().as_secs_f64());
        }
    }
    out.flush()?;
    println!("Wrote {frames} frames to {out_path}");
    Ok(())
}
