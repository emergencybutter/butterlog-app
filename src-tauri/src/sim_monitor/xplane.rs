use crate::airports::AirportsDatabase;
use crate::flight_log_manager::{init_sqlite_db, insert_sqlite_row};
use crate::models::{AircraftInfo, FlightMetrics};
use crate::sim_monitor::{calculate_distance, SimMonitor};
use chrono::Local;
use futures_util::{SinkExt, StreamExt};
use rusqlite::{params, Connection};
use serde_json::json;
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

pub struct XPlaneMonitor {
    metrics: Arc<Mutex<FlightMetrics>>,
    running: Arc<Mutex<bool>>,
    connected: Arc<Mutex<bool>>,
    monitoring: Arc<Mutex<bool>>,
}

impl XPlaneMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(FlightMetrics::default())),
            running: Arc::new(Mutex::new(false)),
            connected: Arc::new(Mutex::new(false)),
            monitoring: Arc::new(Mutex::new(false)),
        }
    }

    async fn run_monitor_async(
        app: AppHandle,
        metrics: Arc<Mutex<FlightMetrics>>,
        running: Arc<Mutex<bool>>,
        connected: Arc<Mutex<bool>>,
        monitoring: Arc<Mutex<bool>>,
        _requested_log_path: Option<PathBuf>,
    ) -> anyhow::Result<()> {
        let url = "ws://localhost:8080/api/v1/telemetry"; // Default port for common XP REST plugins

        loop {
            if !*running.lock().unwrap() {
                break;
            }

            match connect_async(url).await {
                Ok((mut ws_stream, _)) => {
                    crate::append_log(
                        &app,
                        format!(
                            "[{}] Successfully connected to X-Plane 12 WebSocket.",
                            Local::now().format("%Y-%m-%d %H:%M:%S")
                        ),
                    );
                    {
                        let mut c = connected.lock().unwrap();
                        *c = true;
                    }

                    // Subscribe to common datarefs
                    let sub_msg = json!({
                        "command": "subscribe",
                        "datarefs": [
                            "sim/flightmodel/position/latitude",
                            "sim/flightmodel/position/longitude",
                            "sim/flightmodel/position/elevation",
                            "sim/flightmodel/position/phi",
                            "sim/flightmodel/position/theta",
                            "sim/flightmodel/position/psi",
                            "sim/flightmodel/position/indicated_airspeed",
                            "sim/flightmodel/position/groundspeed",
                            "sim/flightmodel/position/vh_ind",
                            "sim/flightmodel/position/y_accel",
                            "sim/flightmodel/position/z_accel",
                            "sim/flightmodel/engine/ENGN_RPM",
                            "sim/flightmodel/failures/onground_any",
                            "sim/aircraft/view/acf_title",
                            "sim/aircraft/view/acf_ICAO"
                        ]
                    });

                    let _ = ws_stream
                        .send(Message::Text(sub_msg.to_string().into()))
                        .await;

                    let mut last_log_time = Local::now();
                    let mut flight_ongoing = false;
                    {
                        let mut m = monitoring.lock().unwrap();
                        *m = false;
                    }
                    let mut db_conn: Option<Connection> = None;
                    let mut current_log_path: Option<PathBuf> = None;
                    let mut analyzer = crate::flight_analyzer::FlightAnalyzer::new();
                    let mut aircraft_info = AircraftInfo::default();

                    while let Some(Ok(msg)) = ws_stream.next().await {
                        if !*running.lock().unwrap() {
                            break;
                        }

                        if let Message::Text(text) = msg {
                            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                                // Map X-Plane data to FlightMetrics
                                let mut m = metrics.lock().unwrap();

                                if let Some(lat) =
                                    data["sim/flightmodel/position/latitude"].as_f64()
                                {
                                    m.latitude = lat;
                                }
                                if let Some(lon) =
                                    data["sim/flightmodel/position/longitude"].as_f64()
                                {
                                    m.longitude = lon;
                                }
                                if let Some(alt) =
                                    data["sim/flightmodel/position/elevation"].as_f64()
                                {
                                    m.gps_altitude_msl = alt * 3.28084;
                                    m.indicated_altitude = alt * 3.28084;
                                }
                                if let Some(roll) = data["sim/flightmodel/position/phi"].as_f64() {
                                    m.roll_angle = roll;
                                }
                                if let Some(pitch) = data["sim/flightmodel/position/theta"].as_f64()
                                {
                                    m.pitch_angle = pitch;
                                }
                                if let Some(hdg) = data["sim/flightmodel/position/psi"].as_f64() {
                                    m.heading = hdg;
                                    m.track = hdg;
                                }
                                if let Some(ias) =
                                    data["sim/flightmodel/position/indicated_airspeed"].as_f64()
                                {
                                    m.indicated_airspeed = ias;
                                }
                                if let Some(gs) =
                                    data["sim/flightmodel/position/groundspeed"].as_f64()
                                {
                                    m.ground_speed = gs * 1.94384;
                                }
                                if let Some(vs) = data["sim/flightmodel/position/vh_ind"].as_f64() {
                                    m.vertical_speed = vs * 196.85;
                                }
                                if let Some(onground) =
                                    data["sim/flightmodel/failures/onground_any"].as_i64()
                                {
                                    m.is_on_ground = onground as f64;
                                }

                                // X-Plane specific
                                if let Some(rpm) = data["sim/flightmodel/engine/ENGN_RPM"]
                                    .get(0)
                                    .and_then(|v| v.as_f64())
                                {
                                    m.xp_prop_rpm = rpm;
                                }

                                // Handle SimStart/SimStop simulation
                                // Heuristic: airborne OR moving > 10.0 knots
                                let ongoing = m.is_on_ground < 0.5 || m.ground_speed > 10.0;
                                if ongoing && !flight_ongoing {
                                    flight_ongoing = true;
                                    {
                                        let mut m = monitoring.lock().unwrap();
                                        *m = true;
                                    }
                                    crate::append_log(
                                        &app,
                                        format!(
                                            "[{}] [X-Plane] Detected ongoing flight. Starting log.",
                                            Local::now().format("%H:%M:%S")
                                        ),
                                    );

                                    // Initialize DB
                                    let app_data_dir = app.path().app_data_dir().unwrap();
                                    let internal_log_dir = app_data_dir.join("flightlogs");
                                    let _ = create_dir_all(&internal_log_dir);
                                    let filename = format!(
                                        "butterlog_xp_{}.db",
                                        Local::now().format("%Y%m%d_%H%M%S")
                                    );
                                    let path = internal_log_dir.join(filename);
                                    current_log_path = Some(path.clone());

                                    match Connection::open(&path) {
                                        Ok(conn) => {
                                            let _ = init_sqlite_db(&conn);
                                            db_conn = Some(conn);
                                            crate::append_log(
                                                &app,
                                                format!(
                                                    "[X-Plane] Created new flight log: {:?}",
                                                    path.file_name().unwrap()
                                                ),
                                            );
                                            let _ = app.emit("flight-logs-updated", ());
                                        }
                                        Err(e) => {
                                            crate::append_log(
                                                &app,
                                                format!(
                                                    "[X-Plane] Failed to create log file: {}",
                                                    e
                                                ),
                                            );
                                        }
                                    }

                                    if let Some(title) =
                                        data["sim/aircraft/view/acf_title"].as_str()
                                    {
                                        aircraft_info.title = title.to_string();
                                    }
                                }

                                if flight_ongoing {
                                    let now = Local::now();
                                    let mut sample_rate_ms = 1000;
                                    if m.is_on_ground < 0.5 {
                                        if let Some(db) = app.try_state::<AirportsDatabase>() {
                                            if let Some(nearest) =
                                                db.find_nearest(m.latitude, m.longitude, 1).first()
                                            {
                                                let dist = calculate_distance(
                                                    m.latitude,
                                                    m.longitude,
                                                    nearest.latitude_deg.unwrap_or(0.0),
                                                    nearest.longitude_deg.unwrap_or(0.0),
                                                );
                                                let elevation =
                                                    nearest.elevation_ft.unwrap_or(0) as f64;
                                                let agl = m.gps_altitude_msl - elevation;

                                                if dist <= 5.0 && agl <= 500.0 {
                                                    sample_rate_ms = 200;
                                                }
                                            }
                                        }
                                    }

                                    if now.signed_duration_since(last_log_time)
                                        >= chrono::Duration::milliseconds(sample_rate_ms)
                                    {
                                        last_log_time = now;
                                        let now_str =
                                            now.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

                                        if let Some(new_phase) = analyzer.update(&m, &now_str) {
                                            let _ = app.emit("flight-phase-change", new_phase);
                                        }

                                        if let Some(ref conn) = db_conn {
                                            if let Err(e) = insert_sqlite_row(conn, &now_str, &m) {
                                                crate::append_log(
                                                    &app,
                                                    format!("Failed to insert SQLite row: {}", e),
                                                );
                                            }
                                        }
                                    }

                                    // Stop detection (simplified: ground speed < 1.0 for 10s)
                                    if m.ground_speed < 1.0 && m.is_on_ground > 0.5 {
                                        if let Some(ref conn) = db_conn {
                                            if let Some(db) = app.try_state::<AirportsDatabase>() {
                                                let start_icao = analyzer.find_start_icao(&db);
                                                let end_icao = analyzer.find_end_icao(&db);
                                                let start_name = if start_icao == "Airborne" {
                                                    "Airborne".to_string()
                                                } else {
                                                    db.get_by_ident(&start_icao)
                                                        .map(|a| a.name.clone())
                                                        .unwrap_or_else(|| "Unknown".to_string())
                                                };
                                                let end_name = db
                                                    .get_by_ident(&end_icao)
                                                    .map(|a| a.name.clone())
                                                    .unwrap_or_else(|| "Unknown".to_string());

                                                let summary_data = [
                                                    ("departure_icao", start_icao),
                                                    ("departure_name", start_name),
                                                    ("aircraft_title", aircraft_info.title.clone()),
                                                    ("max_altitude", analyzer.max_alt.to_string()),
                                                    ("max_ground_speed", analyzer.max_gs.to_string()),
                                                    ("fuel_consumed", (analyzer.initial_fuel - analyzer.final_fuel).to_string()),
                                                ];

                                                for (k, v) in summary_data {
                                                    if let Err(e) = conn.execute(
                                                        "INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)",
                                                        params![k, v],
                                                    ) {
                                                        crate::append_log(&app, format!("Failed to update summary key {}: {}", k, e));
                                                    }
                                                }

                                                if let Ok(events_json) =
                                                    serde_json::to_string(&analyzer.events)
                                                {
                                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["flight_events", events_json]) {
                                                        crate::append_log(&app, format!("Failed to update flight events: {}", e));
                                                    }
                                                }

                                                drop(db_conn.take());
                                                flight_ongoing = false;
                                                {
                                                    let mut m = monitoring.lock().unwrap();
                                                    *m = false;
                                                }
                                                let _ = app.emit("flight-logs-updated", ());
                                            }
                                        }
                                        db_conn = None;
                                    }
                                }
                            }
                        }
                    }

                    {
                        let mut c = connected.lock().unwrap();
                        *c = false;
                    }
                    {
                        let mut m = monitoring.lock().unwrap();
                        *m = false;
                    }
                    crate::append_log(&app, "[X-Plane] Connection closed.".to_string());
                }
                Err(e) => {
                    // Silently retry
                    thread::sleep(Duration::from_secs(2));
                }
            }

            thread::sleep(Duration::from_millis(100));
        }

        Ok(())
    }
}

impl SimMonitor for XPlaneMonitor {
    fn id(&self) -> &'static str {
        "xplane"
    }

    fn start(&self, app: AppHandle, log_path: Option<PathBuf>) -> anyhow::Result<()> {
        let mut running = self.running.lock().unwrap();
        if *running {
            return Ok(());
        }
        *running = true;

        let metrics = self.metrics.clone();
        let running_clone = self.running.clone();
        let connected_clone = self.connected.clone();
        let monitoring_clone = self.monitoring.clone();

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async {
                let _ = Self::run_monitor_async(
                    app,
                    metrics,
                    running_clone,
                    connected_clone,
                    monitoring_clone,
                    log_path,
                )
                .await;
            });
        });

        Ok(())
    }

    fn stop(&self) {
        let mut running = self.running.lock().unwrap();
        *running = false;
    }

    fn get_metrics(&self) -> FlightMetrics {
        *self.metrics.lock().unwrap()
    }

    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }

    fn is_monitoring(&self) -> bool {
        *self.monitoring.lock().unwrap()
    }
}
