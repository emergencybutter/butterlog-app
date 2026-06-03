use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Emitter};
use chrono::Utc;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher, Config as NotifyConfig};
use std::time::{Duration, UNIX_EPOCH};
use crate::config::ConfigManager;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Screenshot {
    pub id: i64,
    pub flight_id: String,
    pub aircraft_title: String,
    pub path: String,
    pub timestamp: String,
    pub latitude: f64,
    pub longitude: f64,
    pub remote_hash: Option<String>,
    pub remote_url: Option<String>,
}

pub struct ScreenshotManager {
    pub db_path: PathBuf,
}

impl ScreenshotManager {
    pub fn new(app: &AppHandle) -> Self {
        let app_dir = app.path().app_data_dir().expect("Failed to get app data dir");
        let db_path = app_dir.join("screenshots.db");

        if !app_dir.exists() {
            std::fs::create_dir_all(&app_dir).expect("Failed to create app data dir");
        }

        crate::append_log(app, format!("Initializing Screenshots database at {:?}", db_path));
        let conn = Connection::open(&db_path).expect("Failed to open screenshots database");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS screenshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                flight_id TEXT NOT NULL,
                aircraft_title TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                timestamp TEXT NOT NULL,
                latitude REAL NOT NULL,
                longitude REAL NOT NULL,
                remote_hash TEXT
            )",
            [],
        ).expect("Failed to create screenshots table");

        // Migrations for existing databases
        let _ = conn.execute("ALTER TABLE screenshots ADD COLUMN remote_hash TEXT", []);
        let _ = conn.execute("ALTER TABLE screenshots ADD COLUMN remote_url TEXT", []);

        Self { db_path }
    }

    fn get_connection(&self) -> Result<Connection, String> {
        Connection::open(&self.db_path).map_err(|e| e.to_string())
    }

    pub fn record_screenshot(&self, flight_id: &str, aircraft_title: &str, path: &str, timestamp: &str, lat: f64, lon: f64) -> Result<i64, String> {
        let conn = self.get_connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO screenshots (flight_id, aircraft_title, path, timestamp, latitude, longitude)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![flight_id, aircraft_title, path, timestamp, lat, lon],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn mark_as_uploaded(&self, id: i64, hash: &str, url: &str) -> Result<(), String> {
        let conn = self.get_connection()?;
        conn.execute(
            "UPDATE screenshots SET remote_hash = ?1, remote_url = ?2 WHERE id = ?3",
            params![hash, url, id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_screenshots_for_flight(&self, flight_id: &str) -> Result<Vec<Screenshot>, String> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare("SELECT id, flight_id, aircraft_title, path, timestamp, latitude, longitude, remote_hash, remote_url FROM screenshots WHERE flight_id = ?1")
            .map_err(|e| e.to_string())?;
        
        let rows = stmt.query_map(params![flight_id], |row| {
            Ok(Screenshot {
                id: row.get(0)?,
                flight_id: row.get(1)?,
                aircraft_title: row.get(2)?,
                path: row.get(3)?,
                timestamp: row.get(4)?,
                latitude: row.get(5)?,
                longitude: row.get(6)?,
                remote_hash: row.get(7)?,
                remote_url: row.get(8)?,
            })
        }).map_err(|e| e.to_string())?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| e.to_string())?);
        }
        Ok(result)
    }

    pub fn get_random_screenshot_for_aircraft(&self, aircraft_title: &str) -> Result<Option<Screenshot>, String> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare("SELECT id, flight_id, aircraft_title, path, timestamp, latitude, longitude, remote_hash, remote_url FROM screenshots WHERE aircraft_title = ?1 ORDER BY RANDOM() LIMIT 1")
            .map_err(|e| e.to_string())?;
        
        let mut rows = stmt.query_map(params![aircraft_title], |row| {
            Ok(Screenshot {
                id: row.get(0)?,
                flight_id: row.get(1)?,
                aircraft_title: row.get(2)?,
                path: row.get(3)?,
                timestamp: row.get(4)?,
                latitude: row.get(5)?,
                longitude: row.get(6)?,
                remote_hash: row.get(7)?,
                remote_url: row.get(8)?,
            })
        }).map_err(|e| e.to_string())?;

        if let Some(row) = rows.next() {
            Ok(Some(row.map_err(|e| e.to_string())?))
        } else {
            Ok(None)
        }
    }
}

pub fn start_screenshot_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let app_clone = app.clone();
        let mut last_config: Option<crate::config::Config> = None;
        let mut _watcher: Option<RecommendedWatcher> = None;

        loop {
            let config = app_clone.state::<crate::config::ConfigManager>().get_config();

            if last_config.as_ref() != Some(&config) {
                _watcher = None;

                if !config.screenshot_directories.is_empty() {
                    let existing_dirs: Vec<PathBuf> = config.screenshot_directories.iter()
                        .filter(|d| d.exists())
                        .cloned()
                        .collect();

                    if !existing_dirs.is_empty() {
                        let app_inner = app_clone.clone();
                        let (tx, rx) = std::sync::mpsc::channel();

                        if let Ok(mut w) = RecommendedWatcher::new(tx, NotifyConfig::default()) {
                            let mut watched = Vec::new();
                            for dir in &existing_dirs {
                                if w.watch(dir, RecursiveMode::NonRecursive).is_ok() {
                                    watched.push(dir.clone());
                                    crate::append_log(&app_inner, format!("Watching for screenshots in: {:?}", dir));
                                }
                            }

                            if !watched.is_empty() {
                                _watcher = Some(w);
                                last_config = Some(config.clone());

                                loop {
                                    match rx.recv_timeout(Duration::from_secs(2)) {
                                        Ok(Ok(event)) => {
                                            if let EventKind::Create(_) = event.kind {
                                                for path in event.paths {
                                                    handle_new_file(&app_inner, path);
                                                }
                                            }
                                        }
                                        Ok(Err(e)) => {
                                            crate::append_log(&app_inner, format!("Screenshot watcher error: {:?}", e));
                                            break;
                                        }
                                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                            crate::append_log(&app_inner, "Screenshot watcher channel disconnected.".to_string());
                                            break;
                                        }
                                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                                    }

                                    let current = app_inner.state::<crate::config::ConfigManager>().get_config();
                                    if current.screenshot_directories != config.screenshot_directories {
                                        crate::append_log(&app_inner, "Screenshot watcher config changed, restarting...".to_string());
                                        break;
                                    }
                                }
                            }
                        }
                    } else {
                        last_config = Some(config.clone());
                    }
                } else {
                    last_config = Some(config.clone());
                }
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}

/// Returns whether a screenshot file should be auto-uploaded. When the
/// screenshot regex is enabled it acts purely as an auto-upload filter; an
/// invalid regex is treated as "match all" so uploads are not silently dropped.
fn passes_upload_filter(config: &crate::config::Config, file_name: &str) -> bool {
    if !config.screenshot_regex_enabled {
        return true;
    }
    regex::RegexBuilder::new(&config.screenshot_regex)
        .case_insensitive(true)
        .build()
        .map(|re| re.is_match(file_name))
        .unwrap_or(true)
}

fn handle_new_file(app: &AppHandle, path: PathBuf) {
    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return,
    };

    // Wait until the file size is stable for 500ms and the file can be opened (ensuring write is complete and lock is released)
    let mut last_size = 0;
    let mut last_change = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(5);
    let start_wait = std::time::Instant::now();

    loop {
        let current_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if current_size != last_size {
            last_size = current_size;
            last_change = std::time::Instant::now();
        } else if last_size > 0 && last_change.elapsed() >= std::time::Duration::from_millis(500) {
            if std::fs::File::open(&path).is_ok() {
                break;
            }
        }

        if start_wait.elapsed() > timeout {
            crate::append_log(app, format!("[Watcher] Timeout waiting for screenshot size stability: {:?}", file_name));
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Check if a flight is ongoing
    let connected_sims = app.state::<crate::UnifiedMonitor>().get_all_monitors();
    
    for monitor in connected_sims {
        if monitor.is_monitoring() {
            let flight_id = monitor.get_current_flight_id();
            let aircraft_info = monitor.get_aircraft_info();
            let aircraft_title = aircraft_info.title;
            let metrics = monitor.get_metrics();
            
            if !flight_id.is_empty() {
                let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
                let manager = app.state::<ScreenshotManager>();
                
                match manager.record_screenshot(
                    &flight_id,
                    &aircraft_title,
                    path.to_str().unwrap_or(""),
                    &timestamp,
                    metrics.latitude,
                    metrics.longitude
                ) {
                    Err(e) => crate::append_log(app, format!("Failed to record screenshot: {}", e)),
                    Ok(screenshot_id) => {
                        crate::append_log(app, format!("Captured screenshot for flight {}: {:?}", flight_id, file_name));
                        let _ = app.emit("new-screenshot", ());

                        // Auto-upload if enabled. The screenshot regex, when
                        // enabled, acts purely as an auto-upload filter.
                        let config = app.state::<ConfigManager>().get_config();
                        if config.auto_upload_screenshots && passes_upload_filter(&config, file_name) {
                            let app_clone = app.clone();
                            let flight_id_clone = flight_id.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = perform_screenshot_upload(app_clone.clone(), screenshot_id, flight_id_clone).await {
                                    crate::append_log(&app_clone, format!("Auto-upload failed for screenshot {}: {}", screenshot_id, e));
                                }
                            });
                        }
                    }
                }
            }
        }
    }
}

pub fn scan_screenshots_for_flight(app: &AppHandle, flight_id: &str, aircraft_title: &str, start_ts: &str, end_ts: &str) -> Result<(), String> {
    let config = app.state::<crate::config::ConfigManager>().get_config();

    if config.screenshot_directories.is_empty() {
        crate::append_log(app, "Screenshot scan skipped: No screenshot directories configured.".to_string());
        return Ok(());
    }

    let manager = app.state::<ScreenshotManager>();
    let start_time = parse_ts(start_ts)?;
    let end_time = parse_ts(end_ts)?;

    let mut total = 0;
    for screenshot_dir in &config.screenshot_directories {
        if !screenshot_dir.exists() { continue; }

        crate::append_log(app, format!("Scanning {:?} for screenshots between {} and {}", screenshot_dir, start_ts, end_ts));

        let Ok(entries) = std::fs::read_dir(screenshot_dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() { continue; }
            let Ok(metadata) = entry.metadata() else { continue };
            let created_res = metadata.created().or_else(|_| metadata.modified());
            let Ok(created) = created_res else { continue };
            let created_ts = created.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
            if created_ts < start_time || created_ts > end_time { continue; }

            let (lat, lon) = find_closest_metrics(app, flight_id, created_ts).unwrap_or((0.0, 0.0));
            let timestamp = chrono::DateTime::<chrono::Utc>::from(created).format("%Y-%m-%d %H:%M:%S").to_string();
            match manager.record_screenshot(flight_id, aircraft_title, path.to_str().unwrap_or(""), &timestamp, lat, lon) {
                Ok(_) => total += 1,
                Err(e) => crate::append_log(app, format!("Failed to record scanned screenshot: {}", e)),
            }
        }
    }

    crate::append_log(app, format!("Screenshot scan complete. Imported {} screenshots.", total));
    Ok(())
}

fn parse_ts(ts: &str) -> Result<i64, String> {
    chrono::NaiveDateTime::parse_from_str(ts.split('.').next().unwrap_or(ts), "%Y-%m-%d %H:%M:%S")
        .map(|dt| dt.and_utc().timestamp())
        .map_err(|e| e.to_string())
}

fn find_closest_metrics(app: &AppHandle, flight_id: &str, timestamp: i64) -> Result<(f64, f64), String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let log_dir = app_data_dir.join(crate::config::get_flightlogs_dir_name());
    let db_path = log_dir.join(format!("{}.db", flight_id));
    
    if !db_path.exists() {
        crate::append_log(app, format!("[Debug] Coordinate lookup failed: DB not found at {:?}", db_path));
        return Err("Flight DB not found".to_string());
    }

    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;

    // Convert target timestamp to string format used in DB
    let target_ts_str = chrono::DateTime::from_timestamp(timestamp, 0)
        .ok_or("Invalid timestamp")?
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    // Debug: Check range of DB
    let range = conn.query_row(
        "SELECT MIN(timestamp), MAX(timestamp) FROM metrics",
        [],
        |r| Ok((r.get::<usize, String>(0)?, r.get::<usize, String>(1)?))
    ).map_err(|e| {
        crate::append_log(app, format!("[Screenshots] Database error (range check): {}", e));
        e.to_string()
    });
    
    if let Ok((min, max)) = range {
        crate::append_log(app, format!("[Debug] Linking screenshot at {}. DB Range: {} to {}", target_ts_str, min, max));
    }

    // Find the closest point by checking one before and one after
    let mut stmt_before = conn.prepare("
        SELECT latitude, longitude, timestamp FROM metrics 
        WHERE timestamp <= ?1 
        ORDER BY timestamp DESC LIMIT 1
    ").map_err(|e| {
        crate::append_log(app, format!("[Screenshots] Database error (prepare before): {}", e));
        e.to_string()
    })?;

    let before = stmt_before.query_row(params![target_ts_str], |row| {
        Ok((row.get::<usize, f64>(0)?, row.get::<usize, f64>(1)?, row.get::<usize, String>(2)?))
    }).optional().map_err(|e: rusqlite::Error| {
        crate::append_log(app, format!("[Screenshots] Database error (query before): {}", e));
        e.to_string()
    })?;

    let mut stmt_after = conn.prepare("
        SELECT latitude, longitude, timestamp FROM metrics 
        WHERE timestamp > ?1 
        ORDER BY timestamp ASC LIMIT 1
    ").map_err(|e| {
        crate::append_log(app, format!("[Screenshots] Database error (prepare after): {}", e));
        e.to_string()
    })?;

    let after = stmt_after.query_row(params![target_ts_str], |row| {
        Ok((row.get::<usize, f64>(0)?, row.get::<usize, f64>(1)?, row.get::<usize, String>(2)?))
    }).optional().map_err(|e: rusqlite::Error| {
        crate::append_log(app, format!("[Screenshots] Database error (query after): {}", e));
        e.to_string()
    })?;

    match (before, after) {
        (Some((lat1, lon1, ts1)), Some((lat2, lon2, ts2))) => {
            let d1 = (parse_ts(&ts1).unwrap_or(0) - timestamp).abs();
            let d2 = (parse_ts(&ts2).unwrap_or(0) - timestamp).abs();
            if d1 < d2 {
                Ok((lat1, lon1))
            } else {
                Ok((lat2, lon2))
            }
        },
        (Some((lat, lon, _)), None) => Ok((lat, lon)),
        (None, Some((lat, lon, _))) => Ok((lat, lon)),
        (None, None) => {
            crate::append_log(app, format!("[Debug] No metrics found in DB for timestamp {}", target_ts_str));
            Ok((0.0, 0.0))
        }
    }
}

#[tauri::command]
pub async fn get_screenshots_for_flight(app: AppHandle, flight_id: String) -> Result<Vec<Screenshot>, String> {
    let manager = app.state::<ScreenshotManager>();
    let res = manager.get_screenshots_for_flight(&flight_id);
    
    match &res {
        Ok(_scrs) => {
        }
        Err(e) => {
            crate::append_log(&app, format!("[Debug] Failed to retrieve screenshots for flight {}: {}", flight_id, e));
        }
    }
    
    res
}

#[tauri::command]
pub async fn get_random_screenshot_for_aircraft(app: AppHandle, aircraft_title: String) -> Result<Option<Screenshot>, String> {
    let manager = app.state::<ScreenshotManager>();
    manager.get_random_screenshot_for_aircraft(&aircraft_title)
}

async fn get_remote_id_internal(app: &AppHandle, flight_id: &str) -> Result<Option<i64>, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let log_dir = app_data_dir.join(crate::config::get_flightlogs_dir_name());
    let path = log_dir.join(format!("{}.db", flight_id));
    
    if !path.exists() {
        return Ok(None);
    }

    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    let res: Option<String> = conn.query_row(
        "SELECT value FROM summary WHERE key = 'remote_id'",
        [],
        |r| r.get(0)
    ).optional().map_err(|e| e.to_string())?;

    if let Some(id_str) = res {
        return Ok(id_str.parse::<i64>().ok());
    }
    Ok(None)
}

pub async fn perform_screenshot_upload(
    app: AppHandle,
    screenshot_id: i64,
    flight_id: String,
) -> Result<String, String> {
    let remote_id = get_remote_id_internal(&app, &flight_id).await?.ok_or("Flight not synced with remote server (no remote ID)")?;
    
    let config = app.state::<ConfigManager>().get_config();
    if !config.enable_webhook || config.webhook_url.is_empty() {
        return Err("Webhook service is not enabled or URL is missing".to_string());
    }

    let mut base_url = config.webhook_url.clone();
    if base_url.ends_with('/') {
        base_url.pop();
    }

    if let Some(custom_url) = crate::get_custom_service_url() {
        base_url = base_url.replace("https://butterlog.flyvoyager.net", &custom_url);
    }

    // Get screenshot path
    let sc_manager = app.state::<ScreenshotManager>();
    let screenshot = {
        let conn = Connection::open(&sc_manager.db_path).map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT path FROM screenshots WHERE id = ?1",
            rusqlite::params![screenshot_id],
            |r| r.get::<_, String>(0)
        ).map_err(|e| format!("Screenshot not found in database: {}", e))?
    };

    let path = std::path::PathBuf::from(&screenshot);
    if !path.exists() {
        return Err(format!("Screenshot file not found at {:?}", path));
    }

    let file_bytes = std::fs::read(&path).map_err(|e| format!("Failed to read screenshot file: {}", e))?;

    // Load and process image: resize if larger than 1600px width or height, keeping aspect ratio
    let img = image::load_from_memory(&file_bytes).map_err(|e| format!("Failed to parse screenshot image: {}", e))?;
    let (width, height) = (img.width(), img.height());
    let resized_img = if width > 1600 || height > 1600 {
        img.resize(1600, 1600, image::imageops::FilterType::Triangle)
    } else {
        img
    };

    // Convert to WebP and compress (lossy encoding, quality 80.0)
    let webp_vec = {
        let webp_encoder = webp::Encoder::from_image(&resized_img)
            .map_err(|e| format!("Failed to initialize WebP encoder: {:?}", e))?;
        let webp_bytes = webp_encoder.encode(80.0);
        webp_bytes.to_vec()
    };

    // Prepare filename with .webp extension
    let mut path_webp = path.clone();
    path_webp.set_extension("webp");
    let file_name = path_webp.file_name().and_then(|n| n.to_str()).unwrap_or("screenshot.webp").to_string();

    let client = reqwest::Client::new();
    let url = format!("{}/flights/{}/screenshots", base_url, remote_id);

    let part = reqwest::multipart::Part::bytes(webp_vec)
        .file_name(file_name)
        .mime_str("image/webp")
        .map_err(|e| e.to_string())?;

    let form = reqwest::multipart::Form::new().part("screenshot", part);

    let res = client.post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Upload request failed: {}", e))?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!("Upload failed with status {}: {}", status, text));
    }

    #[derive(serde::Deserialize)]
    struct UploadResponse {
        hash: String,
        url: Option<String>,
    }

    let data = res.json::<UploadResponse>().await.map_err(|e| format!("Failed to parse upload response: {}", e))?;

    // Mark as uploaded in DB, storing the public URL for use in shares
    let url = data.url.as_deref().unwrap_or("");
    sc_manager.mark_as_uploaded(screenshot_id, &data.hash, url)?;

    crate::append_log(&app, format!("Successfully uploaded screenshot {} to remote flight {}", screenshot_id, remote_id));

    // Notify UI
    let _ = app.emit("screenshot-uploaded", screenshot_id);

    Ok(data.hash)
}
