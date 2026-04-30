use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Emitter};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher, Config as NotifyConfig};
use std::time::{Duration, UNIX_EPOCH};
use regex::Regex;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Screenshot {
    pub id: i64,
    pub flight_id: String,
    pub aircraft_title: String,
    pub path: String,
    pub timestamp: String,
    pub latitude: f64,
    pub longitude: f64,
}

pub struct ScreenshotManager {
    db_path: PathBuf,
}

impl ScreenshotManager {
    pub fn new(app: &AppHandle) -> Self {
        let app_dir = app.path().app_data_dir().expect("Failed to get app data dir");
        let db_path = app_dir.join("screenshots.db");

        if !app_dir.exists() {
            std::fs::create_dir_all(&app_dir).expect("Failed to create app data dir");
        }

        let conn = Connection::open(&db_path).expect("Failed to open screenshots database");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS screenshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                flight_id TEXT NOT NULL,
                aircraft_title TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                timestamp TEXT NOT NULL,
                latitude REAL NOT NULL,
                longitude REAL NOT NULL
            )",
            [],
        ).expect("Failed to create screenshots table");

        Self { db_path }
    }

    fn get_connection(&self) -> Result<Connection, String> {
        Connection::open(&self.db_path).map_err(|e| e.to_string())
    }

    pub fn record_screenshot(&self, flight_id: &str, aircraft_title: &str, path: &str, timestamp: &str, lat: f64, lon: f64) -> Result<(), String> {
        let conn = self.get_connection()?;
        conn.execute(
            "INSERT OR IGNORE INTO screenshots (flight_id, aircraft_title, path, timestamp, latitude, longitude)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![flight_id, aircraft_title, path, timestamp, lat, lon],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_screenshots_for_flight(&self, flight_id: &str) -> Result<Vec<Screenshot>, String> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare("SELECT id, flight_id, aircraft_title, path, timestamp, latitude, longitude FROM screenshots WHERE flight_id = ?1")
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
        let mut stmt = conn.prepare("SELECT id, flight_id, aircraft_title, path, timestamp, latitude, longitude FROM screenshots WHERE aircraft_title = ?1 ORDER BY RANDOM() LIMIT 1")
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
        let mut last_config = None;
        let mut _watcher: Option<RecommendedWatcher> = None;
        
        loop {
            let config = app_clone.state::<crate::config::ConfigManager>().get_config();
            
            if last_config.as_ref() != Some(&config) {
                _watcher = None;
                
                if let (Some(dir), true) = (&config.screenshot_directory, config.screenshot_regex_enabled) {
                    if dir.exists() {
                        let app_inner = app_clone.clone();
                        let dir_inner = dir.clone();
                        let regex_str = config.screenshot_regex.clone();
                        
                        let (tx, rx) = std::sync::mpsc::channel();
                        
                        if let Ok(mut w) = RecommendedWatcher::new(tx, NotifyConfig::default()) {
                            if let Ok(_) = w.watch(&dir_inner, RecursiveMode::NonRecursive) {
                                _watcher = Some(w);
                                crate::append_log(&app_inner, format!("Started watching for screenshots in: {:?}", dir_inner));
                                
                                last_config = Some(config.clone());
                                
                                loop {
                                    match rx.recv_timeout(Duration::from_secs(2)) {
                                        Ok(res) => {
                                            match res {
                                                Ok(event) => {
                                                    if let EventKind::Create(_) = event.kind {
                                                        for path in event.paths {
                                                            handle_new_file(&app_inner, path, &regex_str);
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    crate::append_log(&app_inner, format!("Watcher error: {:?}", e));
                                                    break;
                                                }
                                            }
                                        }
                                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                            // timeout, proceed to check config
                                        }
                                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                            break;
                                        }
                                    }
                                    
                                    let current_config = app_inner.state::<crate::config::ConfigManager>().get_config();
                                    if current_config.screenshot_directory != Some(dir_inner.clone()) || 
                                       current_config.screenshot_regex != regex_str {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}

fn handle_new_file(app: &AppHandle, path: PathBuf, regex_str: &str) {
    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return,
    };

    let re = match Regex::new(regex_str) {
        Ok(r) => r,
        Err(_) => return,
    };

    if !re.is_match(file_name) {
        return;
    }

    // Check if a flight is ongoing
    let unified = app.state::<crate::UnifiedMonitor>();
    let connected_sims = app.state::<crate::UnifiedMonitor>().get_all_monitors();
    
    for monitor in connected_sims {
        if monitor.is_monitoring() {
            let flight_id = monitor.get_current_flight_id();
            let aircraft_title = monitor.get_aircraft_info().title;
            let metrics = monitor.get_metrics();
            
            if !flight_id.is_empty() {
                let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                let manager = app.state::<ScreenshotManager>();
                
                if let Err(e) = manager.record_screenshot(
                    &flight_id,
                    &aircraft_title,
                    path.to_str().unwrap_or(""),
                    &timestamp,
                    metrics.latitude,
                    metrics.longitude
                ) {
                    crate::append_log(app, format!("Failed to record screenshot: {}", e));
                } else {
                    crate::append_log(app, format!("Captured screenshot for flight {}: {:?}", flight_id, file_name));
                    let _ = app.emit("new-screenshot", ());
                }
            }
        }
    }
}

pub fn scan_screenshots_for_flight(app: &AppHandle, flight_id: &str, aircraft_title: &str, start_ts: &str, end_ts: &str) -> Result<(), String> {
    let config = app.state::<crate::config::ConfigManager>().get_config();
    let screenshot_dir = match &config.screenshot_directory {
        Some(dir) => dir,
        None => return Ok(()),
    };

    if !screenshot_dir.exists() || !config.screenshot_regex_enabled {
        return Ok(());
    }

    let re = Regex::new(&config.screenshot_regex).map_err(|e| e.to_string())?;
    let manager = app.state::<ScreenshotManager>();

    // Parse timestamps
    let start_time = parse_ts(start_ts)?;
    let end_time = parse_ts(end_ts)?;

    if let Ok(entries) = std::fs::read_dir(screenshot_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if re.is_match(file_name) {
                        if let Ok(metadata) = entry.metadata() {
                            if let Ok(created) = metadata.created() {
                                let created_ts = created.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
                                if created_ts >= start_time && created_ts <= end_time {
                                    // Found a potential screenshot!
                                    // For imported flights, we don't have exact metrics at that second,
                                    // but we can try to find the closest metrics in the database if we really wanted to.
                                    // For now, let's just record it with 0,0 or try to fetch from DB.
                                    let (lat, lon) = find_closest_metrics(app, flight_id, created_ts).unwrap_or((0.0, 0.0));
                                    let timestamp = chrono::DateTime::<chrono::Utc>::from(created).format("%Y-%m-%d %H:%M:%S").to_string();
                                    
                                    let _ = manager.record_screenshot(
                                        flight_id,
                                        aircraft_title,
                                        path.to_str().unwrap_or(""),
                                        &timestamp,
                                        lat,
                                        lon
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn parse_ts(ts: &str) -> Result<i64, String> {
    chrono::NaiveDateTime::parse_from_str(ts.split('.').next().unwrap_or(ts), "%Y-%m-%d %H:%M:%S")
        .map(|dt| dt.and_utc().timestamp())
        .map_err(|e| e.to_string())
}

fn find_closest_metrics(app: &AppHandle, flight_id: &str, timestamp: i64) -> Result<(f64, f64), String> {
    let log_dir = app.state::<crate::config::ConfigManager>().get_config().log_directory.ok_or("No log directory")?;
    let db_path = log_dir.join(format!("{}.db", flight_id));
    if !db_path.exists() {
        return Err("Flight DB not found".to_string());
    }

    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT latitude, longitude FROM metrics ORDER BY ABS(strftime('%s', timestamp) - ?1) LIMIT 1")
        .map_err(|e| e.to_string())?;
    
    let mut rows = stmt.query_map(params![timestamp], |row| {
        Ok((row.get(0)?, row.get(1)?))
    }).map_err(|e| e.to_string())?;

    if let Some(row) = rows.next() {
        Ok(row.map_err(|e| e.to_string())?)
    } else {
        Ok((0.0, 0.0))
    }
}

#[tauri::command]
pub async fn get_screenshots_for_flight(app: AppHandle, flight_id: String) -> Result<Vec<Screenshot>, String> {
    let manager = app.state::<ScreenshotManager>();
    manager.get_screenshots_for_flight(&flight_id)
}

#[tauri::command]
pub async fn get_random_screenshot_for_aircraft(app: AppHandle, aircraft_title: String) -> Result<Option<Screenshot>, String> {
    let manager = app.state::<ScreenshotManager>();
    manager.get_random_screenshot_for_aircraft(&aircraft_title)
}
