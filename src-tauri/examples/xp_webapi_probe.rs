//! Web API probe for the sim-native telemetry rollout, phase 1 (design §4.1): how often
//! X-Plane's Web API pushes `dataref_update_values`, with how much jitter, and whether it
//! resends unchanged values. Run it next to the plugin's capture spike to compare the two.
//!
//! ```text
//! cargo run --release --example xp_webapi_probe -- [--secs 600] [--out xp_webapi_probe.csv]
//! ```
//!
//! Writes one CSV row per message (arrival time, then each dataref's value or empty when
//! the message left it out) and prints a summary at the end.

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const DATAREFS: &[&str] = &[
    "sim/time/total_running_time_sec",
    "sim/flightmodel/position/latitude",
    "sim/flightmodel/position/longitude",
    "sim/flightmodel/position/theta",
    "sim/flightmodel/position/phi",
    "sim/flightmodel/position/vh_ind",
    "sim/flightmodel/position/y_agl",
    "sim/flightmodel/forces/g_nrml",
    "sim/cockpit2/switches/landing_lights_on",
    "sim/cockpit2/radios/actuators/com1_frequency_hz_833",
];

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut secs = 600u64;
    let mut out_path = "xp_webapi_probe.csv".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--secs" => secs = args.next().and_then(|v| v.parse().ok()).expect("--secs N"),
            "--out" => out_path = args.next().expect("--out PATH"),
            other => anyhow::bail!("unknown argument {other}"),
        }
    }

    let listing: Value = reqwest::get("http://localhost:8086/api/v3/datarefs").await?.json().await?;
    let mut id_to_column = HashMap::new();
    for item in listing["data"].as_array().into_iter().flatten() {
        if let (Some(name), Some(id)) = (item["name"].as_str(), item["id"].as_i64()) {
            if let Some(col) = DATAREFS.iter().position(|d| *d == name) {
                id_to_column.insert(id.to_string(), col);
            }
        }
    }
    anyhow::ensure!(id_to_column.len() == DATAREFS.len(), "some datarefs were not found");

    let (mut ws, _) = connect_async("ws://localhost:8086/api/v3").await?;
    let ids: Vec<Value> = id_to_column
        .keys()
        .map(|id| serde_json::json!({ "id": id.parse::<i64>().unwrap() }))
        .collect();
    let subscribe = serde_json::json!({
        "req_id": 1,
        "type": "dataref_subscribe_values",
        "params": { "datarefs": ids },
    });
    ws.send(Message::Text(subscribe.to_string().into())).await?;

    let mut out = std::io::BufWriter::new(std::fs::File::create(&out_path)?);
    writeln!(out, "wall_ms,{}", DATAREFS.join(","))?;
    let start = Instant::now();
    let deadline = start + Duration::from_secs(secs);
    let mut arrivals: Vec<f64> = Vec::new();
    let mut sim_times: Vec<f64> = Vec::new();
    let mut present = vec![0u64; DATAREFS.len()];
    let mut repeated = vec![0u64; DATAREFS.len()];
    let mut last: Vec<Option<String>> = vec![None; DATAREFS.len()];
    println!("Recording {secs} s of Web API updates to {out_path}…");

    while let Ok(Some(msg)) = tokio::time::timeout_at(deadline.into(), ws.next()).await {
        let Ok(Message::Text(text)) = msg else { continue };
        let Ok(data) = serde_json::from_str::<Value>(&text) else { continue };
        if data["type"] != "dataref_update_values" {
            continue;
        }
        let at = start.elapsed().as_secs_f64();
        let wall_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let mut row = vec![String::new(); DATAREFS.len()];
        for (id, v) in data["data"].as_object().into_iter().flatten() {
            if let Some(&col) = id_to_column.get(id) {
                let s = v.to_string();
                present[col] += 1;
                if last[col].as_deref() == Some(s.as_str()) {
                    repeated[col] += 1;
                }
                if col == 0 {
                    if let Some(t) = v.as_f64() {
                        sim_times.push(t);
                        arrivals.push(at);
                    }
                }
                last[col] = Some(s.clone());
                row[col] = s;
            }
        }
        writeln!(out, "{wall_ms},{}", row.join(","))?;
    }
    out.flush()?;

    anyhow::ensure!(arrivals.len() > 2, "too few updates; is X-Plane running and unpaused?");
    let mut gaps: Vec<f64> = arrivals.windows(2).map(|w| (w[1] - w[0]) * 1000.0).collect();
    // Arrival jitter against the sim clock: how far each arrival gap is from the sim time
    // that actually elapsed between the two samples.
    let mut jitter: Vec<f64> = arrivals
        .windows(2)
        .zip(sim_times.windows(2))
        .map(|(a, s)| ((a[1] - a[0]) - (s[1] - s[0])).abs() * 1000.0)
        .collect();
    gaps.sort_by(f64::total_cmp);
    jitter.sort_by(f64::total_cmp);
    let span = arrivals.last().unwrap() - arrivals[0];
    println!(
        "{} messages with sim time in {:.0} s: {:.1} Hz",
        arrivals.len(),
        span,
        (arrivals.len() - 1) as f64 / span
    );
    println!(
        "arrival gap ms: p50 {:.1}, p90 {:.1}, p99 {:.1}, max {:.1}",
        percentile(&gaps, 0.5),
        percentile(&gaps, 0.9),
        percentile(&gaps, 0.99),
        percentile(&gaps, 1.0)
    );
    println!(
        "arrival vs sim time ms: p50 {:.1}, p90 {:.1}, p99 {:.1}, max {:.1}",
        percentile(&jitter, 0.5),
        percentile(&jitter, 0.9),
        percentile(&jitter, 0.99),
        percentile(&jitter, 1.0)
    );
    println!("\nper dataref: messages carrying it, and how many of those repeated the last value");
    for (i, name) in DATAREFS.iter().enumerate() {
        println!("  {:<56} {:>7} {:>7}", name, present[i], repeated[i]);
    }
    Ok(())
}
