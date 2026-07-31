//! Streams the in-progress flight's track to the service so it can be watched
//! live on the web.
//!
//! Samples are read back out of the flight's own SQLite log rather than buffered
//! in memory. The logger already writes every sample there at 1 Hz, so this
//! needs no hook in either sim monitor, and a crash mid-flight costs nothing —
//! the next tick simply resumes from the cursor.

use crate::flight_log_manager::{map_row_to_metrics, ts_to_epoch, Downsampler, FlightLogRow};
use crate::models::FlightEvent;
use parking_lot::Mutex;
use reqwest::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::io::Write;
use tauri::{AppHandle, Manager};

/// How often to flush new samples. Short enough that a viewer sees the aircraft
/// move, long enough that cruise costs almost nothing.
const FLUSH_INTERVAL_SECS: u64 = 20;

/// Cap on samples per batch, matching the service's limit.
const MAX_POINTS_PER_BATCH: usize = 2000;

/// SQLite `summary` key holding the last uploaded sample's timestamp.
const CURSOR_KEY: &str = "live_last_epoch";

#[derive(Serialize)]
struct TransposedPoints {
    timestamps: Vec<i64>,
    latitudes: Vec<f32>,
    longitudes: Vec<f32>,
    altitudes: Vec<f32>,
    ias: Vec<f32>,
    vspeed: Vec<f32>,
    pitch: Vec<f32>,
    roll: Vec<f32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackBatch {
    start_epoch: i64,
    points: TransposedPoints,
    events: Vec<FlightEvent>,
}

/// Per-flight upload state. The `Downsampler` is held across flushes on purpose:
/// restarting the thinning policy every batch would keep the first sample of
/// each one and hold cruise at one sample per flush instead of one per 5 minutes.
struct FlightState {
    remote_id: i64,
    log_path: String,
    policy: Downsampler,
}

pub struct LiveTrackUploader {
    client: Client,
    state: Mutex<Option<FlightState>>,
}

impl LiveTrackUploader {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            state: Mutex::new(None),
        }
    }
}

impl Default for LiveTrackUploader {
    fn default() -> Self {
        Self::new()
    }
}

/// Read the persisted cursor, so a restarted app resumes rather than replaying
/// the whole flight. Losing it is survivable — the service dedups by timestamp —
/// but replaying hours of samples on every restart would not be.
fn read_cursor(conn: &Connection) -> Option<String> {
    conn.query_row(
        "SELECT value FROM summary WHERE key = ?1",
        params![CURSOR_KEY],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
}

fn write_cursor(conn: &Connection, cursor: &str) {
    let _ = conn.execute(
        "INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)",
        params![CURSOR_KEY, cursor],
    );
}

fn read_events(conn: &Connection) -> Vec<FlightEvent> {
    conn.query_row(
        "SELECT value FROM summary WHERE key = 'flight_events'",
        [],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .and_then(|json| serde_json::from_str(&json).ok())
    .unwrap_or_default()
}

/// Rows logged after `cursor`, oldest first.
fn read_rows_after(conn: &Connection, cursor: &str) -> rusqlite::Result<Vec<FlightLogRow>> {
    let mut stmt =
        conn.prepare("SELECT * FROM metrics WHERE timestamp > ?1 ORDER BY timestamp ASC LIMIT ?2")?;
    let rows = stmt.query_map(params![cursor, MAX_POINTS_PER_BATCH as i64 * 60], |row| {
        Ok(FlightLogRow {
            timestamp: row.get(0)?,
            metrics: map_row_to_metrics(row)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Build the batch body for the samples the policy keeps.
///
/// Returns `None` when nothing survives thinning — a normal outcome in cruise,
/// where the batch is still sent (empty) because it doubles as the liveness
/// heartbeat.
fn build_batch(
    rows: &[FlightLogRow],
    events: &[FlightEvent],
    policy: &mut Downsampler,
) -> (TrackBatch, Option<String>) {
    let event_epochs: Vec<i64> = events
        .iter()
        .filter(|e| e.event_type == "takeoff" || e.event_type == "landing")
        .map(|e| ts_to_epoch(&e.timestamp))
        .filter(|&t| t > 0)
        .collect();

    let kept: Vec<&FlightLogRow> = rows
        .iter()
        .filter(|row| policy.accept(row, &event_epochs))
        .take(MAX_POINTS_PER_BATCH)
        .collect();

    // The cursor advances over every row examined, not just the kept ones:
    // rows the policy rejected are decided forever and must not be re-read.
    let cursor = rows.last().map(|r| r.timestamp.clone());

    let start_epoch = kept.first().map(|r| ts_to_epoch(&r.timestamp)).unwrap_or(0);
    let n = kept.len();
    let mut points = TransposedPoints {
        timestamps: Vec::with_capacity(n),
        latitudes: Vec::with_capacity(n),
        longitudes: Vec::with_capacity(n),
        altitudes: Vec::with_capacity(n),
        ias: Vec::with_capacity(n),
        vspeed: Vec::with_capacity(n),
        pitch: Vec::with_capacity(n),
        roll: Vec::with_capacity(n),
    };

    let mut prev = start_epoch;
    for row in &kept {
        let epoch = ts_to_epoch(&row.timestamp);
        points.timestamps.push(epoch - prev);
        prev = epoch;
        points.latitudes.push(row.metrics.latitude as f32);
        points.longitudes.push(row.metrics.longitude as f32);
        points.altitudes.push(row.metrics.indicated_altitude as f32);
        points.ias.push(row.metrics.indicated_airspeed as f32);
        points.vspeed.push(row.metrics.vertical_speed as f32);
        points.pitch.push(row.metrics.pitch_angle as f32);
        points.roll.push(row.metrics.roll_angle as f32);
    }

    (
        TrackBatch {
            start_epoch,
            points,
            events: events.to_vec(),
        },
        cursor,
    )
}

fn gzip(json: &str) -> Result<Vec<u8>, String> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())
}

impl LiveTrackUploader {
    /// One flush. Returns quietly when there is nothing to do — no flight, not
    /// logged in, or the feature switched off.
    async fn tick(&self, app: &AppHandle) {
        let config = app.state::<crate::config::ConfigManager>().get_config();
        if !config.enable_webhook || !config.share_live_flights {
            return;
        }
        let Some((base_url, api_token)) = config.api_auth() else {
            return;
        };
        let Some((log_path, remote_id)) = app
            .state::<crate::webhook_manager::WebhookManager>()
            .current_flight()
        else {
            // Between flights: drop per-flight state so the next one starts its
            // thinning policy fresh.
            *self.state.lock() = None;
            return;
        };

        // A new flight (or the first tick of this one) starts a fresh policy and
        // picks up any cursor left by a previous run of the app.
        {
            let mut guard = self.state.lock();
            let stale = guard
                .as_ref()
                .map(|s| s.remote_id != remote_id || s.log_path != log_path)
                .unwrap_or(true);
            if stale {
                *guard = None;
            }
        }

        let path = log_path.clone();
        let loaded = tauri::async_runtime::spawn_blocking(move || -> Option<(String, Vec<FlightLogRow>, Vec<FlightEvent>)> {
            let conn = Connection::open(&path).ok()?;
            let cursor = read_cursor(&conn).unwrap_or_default();
            let events = read_events(&conn);
            let rows = read_rows_after(&conn, &cursor).ok()?;
            Some((cursor, rows, events))
        })
        .await
        .ok()
        .flatten();

        let Some((_cursor, rows, events)) = loaded else {
            return;
        };

        // Scoped so the (non-Send) guard is released before any await below.
        let (json, new_cursor, sent_points) = {
            let mut guard = self.state.lock();
            let state = guard.get_or_insert_with(|| FlightState {
                remote_id,
                log_path: log_path.clone(),
                policy: Downsampler::new(),
            });

            let (batch, new_cursor) = build_batch(&rows, &events, &mut state.policy);
            let sent_points = batch.points.timestamps.len();
            match serde_json::to_string(&batch) {
                Ok(json) => (json, new_cursor, sent_points),
                Err(e) => {
                    crate::append_log(app, format!("[LiveTrack] Encode failed: {}", e));
                    return;
                }
            }
        };

        let body = match gzip(&json) {
            Ok(b) => b,
            Err(e) => {
                crate::append_log(app, format!("[LiveTrack] Compress failed: {}", e));
                return;
            }
        };

        let url = format!("{}/flights/{}/track", base_url, remote_id);
        let res = self
            .client
            .post(&url)
            .bearer_auth(&api_token)
            .header("Content-Type", "application/octet-stream")
            .body(body)
            .send()
            .await;

        match res {
            Ok(r) if r.status().is_success() => {
                // Only advance the persisted cursor once the service has the
                // samples. A failed flush is retried from the same point; the
                // service dedups by timestamp, so a double send is harmless.
                if let Some(new_cursor) = new_cursor {
                    let path = log_path.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        if let Ok(conn) = Connection::open(&path) {
                            write_cursor(&conn, &new_cursor);
                        }
                    })
                    .await;
                }
                if sent_points > 0 {
                    crate::append_log(
                        app,
                        format!(
                            "[LiveTrack] Uploaded {} samples for flight {}",
                            sent_points, remote_id
                        ),
                    );
                }
            }
            Ok(r) if r.status().as_u16() == 401 => {
                crate::force_logout(app, "service rejected token during live track upload");
            }
            Ok(r) if r.status().as_u16() == 410 => {
                // The flight ended server-side; stop pushing to it.
                crate::append_log(
                    app,
                    format!("[LiveTrack] Flight {} already ended; stopping", remote_id),
                );
                *self.state.lock() = None;
            }
            Ok(r) => {
                crate::append_log(
                    app,
                    format!("[LiveTrack] Upload failed ({}): flight {}", r.status(), remote_id),
                );
            }
            Err(e) => {
                crate::append_log(app, format!("[LiveTrack] Upload error: {}", e));
            }
        }
    }
}

/// Tell the service a flight is over, so the live page can hand off to the
/// permanent share instead of showing a frozen track.
pub async fn end_flight(app: &AppHandle, remote_id: i64, reason: &str) {
    let config = app.state::<crate::config::ConfigManager>().get_config();
    if !config.enable_webhook {
        return;
    }
    let Some((base_url, api_token)) = config.api_auth() else {
        return;
    };

    let url = format!("{}/flights/{}/end", base_url, remote_id);
    let res = Client::new()
        .post(&url)
        .bearer_auth(&api_token)
        .json(&serde_json::json!({ "reason": reason }))
        .send()
        .await;

    match res {
        Ok(r) if r.status().is_success() => {
            crate::append_log(app, format!("[LiveTrack] Ended flight {} ({})", remote_id, reason));
        }
        Ok(r) => crate::append_log(
            app,
            format!("[LiveTrack] End failed ({}) for flight {}", r.status(), remote_id),
        ),
        Err(e) => crate::append_log(app, format!("[LiveTrack] End error: {}", e)),
    }
}

/// Final sync, then end the flight, then clear the sync state — in that order,
/// as one task.
///
/// The ordering matters: the flight id lives in `WebhookManager`, so a `reset()`
/// running synchronously alongside a spawned end call would usually win the race
/// and leave the flight to be reaped 30 minutes later instead of ended cleanly.
/// Sending the final statistics before the end call also means the live page
/// hands off to a complete summary.
pub fn spawn_finalize(
    app: &AppHandle,
    summary: crate::models::WebhookFlightSummary,
    reason: &'static str,
) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let wm = app.state::<crate::webhook_manager::WebhookManager>();
        wm.sync_flight(&app, &summary, true).await;
        if let Some((_, remote_id)) = wm.current_flight() {
            end_flight(&app, remote_id, reason).await;
        }
        wm.reset();
    });
}

/// Start the flush loop. Cheap when idle: one config read and one state check.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker =
            tokio::time::interval(std::time::Duration::from_secs(FLUSH_INTERVAL_SECS));
        loop {
            ticker.tick().await;
            if let Some(uploader) = app.try_state::<LiveTrackUploader>() {
                uploader.tick(&app).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::FlightMetrics;

    fn row(secs: i64, alt: f64, vs: f64, roll: f64) -> FlightLogRow {
        let ts = chrono::DateTime::from_timestamp(secs, 0)
            .unwrap()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let mut metrics = FlightMetrics::default();
        metrics.latitude = 45.0;
        metrics.longitude = 5.0;
        metrics.indicated_altitude = alt;
        metrics.vertical_speed = vs;
        metrics.roll_angle = roll;
        FlightLogRow { timestamp: ts, metrics }
    }

    #[test]
    fn timestamps_are_deltas_with_a_leading_zero() {
        // Climbing, so every sample is kept and the deltas are visible.
        let rows: Vec<_> = (0..5).map(|i| row(1000 + i * 3, 1000.0 + i as f64 * 25.0, 500.0, 0.0)).collect();
        let mut policy = Downsampler::new();
        let (batch, _) = build_batch(&rows, &[], &mut policy);
        assert_eq!(batch.start_epoch, 1000);
        assert_eq!(batch.points.timestamps, vec![0, 3, 3, 3, 3]);
    }

    #[test]
    fn the_policy_carries_across_batches() {
        // Two consecutive cruise windows. If the policy restarted per batch the
        // second would keep its own first sample; carried across, the 300s tier
        // holds and the second window contributes nothing.
        let first: Vec<_> = (0..20).map(|i| row(1000 + i, 35_000.0, 0.0, 0.0)).collect();
        let second: Vec<_> = (20..40).map(|i| row(1000 + i, 35_000.0, 0.0, 0.0)).collect();

        let mut policy = Downsampler::new();
        let (b1, c1) = build_batch(&first, &[], &mut policy);
        let (b2, _) = build_batch(&second, &[], &mut policy);

        assert_eq!(b1.points.timestamps.len(), 1, "only the opening sample");
        assert_eq!(b2.points.timestamps.len(), 0, "still inside the 300s tier");
        assert!(c1.is_some());
    }

    #[test]
    fn cursor_advances_over_rejected_rows() {
        // The cursor must cover every row examined. Advancing only over kept
        // rows would re-read the rejected ones forever, and in cruise — where
        // almost everything is rejected — the flush would never make progress.
        let rows: Vec<_> = (0..20).map(|i| row(1000 + i, 35_000.0, 0.0, 0.0)).collect();
        let mut policy = Downsampler::new();
        let (batch, cursor) = build_batch(&rows, &[], &mut policy);
        assert_eq!(batch.points.timestamps.len(), 1);
        assert_eq!(cursor.as_deref(), Some(rows[19].timestamp.as_str()));
    }

    #[test]
    fn an_empty_window_still_produces_a_heartbeat_batch() {
        let mut policy = Downsampler::new();
        let (batch, cursor) = build_batch(&[], &[], &mut policy);
        assert_eq!(batch.points.timestamps.len(), 0);
        assert_eq!(cursor, None, "nothing examined, so nothing to advance past");
    }
}
