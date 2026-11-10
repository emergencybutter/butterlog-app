//! Screenshots: the watched folders' new pictures, taken while a sim is connected, kept
//! with the telemetry session recording at the time. The window lists the recent ones; each
//! goes to the service by itself (Settings) or with a click, and the service puts it on the
//! flight it made from that session.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Emitter};
use chrono::Utc;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher, Config as NotifyConfig};
use std::time::Duration;
use crate::config::ConfigManager;

/// How many screenshots the window lists.
const RECENT_LIMIT: i64 = 60;

/// Thumbnails' longest side, in pixels: a list of 4K pictures would take the window's
/// memory, so it shows these instead.
const THUMB_PX: u32 = 360;

#[derive(Debug, Serialize, Deserialize, Clone, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Screenshot {
    // JSON (and therefore Tauri's invoke) carries this as a plain number
    #[ts(type = "number")]
    pub id: i64,
    pub aircraft_title: String,
    pub path: String,
    /// When it was taken, UTC `YYYY-MM-DD HH:MM:SS`.
    pub timestamp: String,
    pub latitude: f64,
    pub longitude: f64,
    /// The telemetry session recording when it was taken; without one it can't go up.
    pub session_id: Option<String>,
    pub remote_hash: Option<String>,
    pub remote_url: Option<String>,
}

pub struct ScreenshotManager {
    pub db_path: PathBuf,
    thumbs: PathBuf,
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
        // The telemetry session recording when it was taken: what it is uploaded against.
        let _ = conn.execute("ALTER TABLE screenshots ADD COLUMN session_id TEXT", []);

        Self { db_path, thumbs: app_dir.join("screenshot_thumbs") }
    }

    fn get_connection(&self) -> Result<Connection, String> {
        Connection::open(&self.db_path).map_err(|e| e.to_string())
    }

    /// Keeps a screenshot, once per path; its id. `flight_id` is a column of the app's own
    /// flight logs, gone now, so it is left empty.
    pub fn record_screenshot(&self, aircraft_title: &str, path: &str, timestamp: &str, lat: f64, lon: f64, session_id: Option<&str>) -> Result<i64, String> {
        let conn = self.get_connection()?;
        conn.execute(
            "INSERT INTO screenshots (flight_id, aircraft_title, path, timestamp, latitude, longitude, session_id)
             VALUES ('', ?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET session_id = COALESCE(screenshots.session_id, excluded.session_id)",
            params![aircraft_title, path, timestamp, lat, lon, session_id],
        ).map_err(|e| e.to_string())?;
        conn.query_row("SELECT id FROM screenshots WHERE path = ?1", params![path], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())
    }

    pub fn mark_as_uploaded(&self, id: i64, hash: &str, url: &str) -> Result<(), String> {
        let conn = self.get_connection()?;
        conn.execute(
            "UPDATE screenshots SET remote_hash = ?1, remote_url = ?2 WHERE id = ?3",
            params![hash, url, id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// The newest first, those whose file is still there.
    pub fn recent(&self, limit: i64) -> Result<Vec<Screenshot>, String> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, aircraft_title, path, timestamp, latitude, longitude, session_id, remote_hash, remote_url
               FROM screenshots ORDER BY timestamp DESC, id DESC LIMIT ?1",
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![limit * 2], |row| {
            Ok(Screenshot {
                id: row.get(0)?,
                aircraft_title: row.get(1)?,
                path: row.get(2)?,
                timestamp: row.get(3)?,
                latitude: row.get(4)?,
                longitude: row.get(5)?,
                session_id: row.get(6)?,
                remote_hash: row.get(7)?,
                remote_url: row.get(8)?,
            })
        }).map_err(|e| e.to_string())?;
        let mut kept = Vec::new();
        for row in rows {
            let shot = row.map_err(|e| e.to_string())?;
            if std::path::Path::new(&shot.path).exists() {
                kept.push(shot);
            }
            if kept.len() as i64 >= limit {
                break;
            }
        }
        Ok(kept)
    }

    fn path_of(&self, id: i64) -> Result<String, String> {
        self.get_connection()?
            .query_row("SELECT path FROM screenshots WHERE id = ?1", params![id], |r| r.get(0))
            .map_err(|e| format!("No such screenshot: {e}"))
    }

    /// A small copy of the screenshot, made the first time it is asked for.
    pub fn thumbnail(&self, id: i64) -> Result<PathBuf, String> {
        let thumb = self.thumbs.join(format!("{id}.jpg"));
        if thumb.exists() {
            return Ok(thumb);
        }
        let source = self.path_of(id)?;
        let img = image::open(&source).map_err(|e| format!("Can't read {source}: {e}"))?;
        std::fs::create_dir_all(&self.thumbs).map_err(|e| e.to_string())?;
        img.thumbnail(THUMB_PX, THUMB_PX)
            .to_rgb8()
            .save_with_format(&thumb, image::ImageFormat::Jpeg)
            .map_err(|e| e.to_string())?;
        Ok(thumb)
    }
}

pub fn start_screenshot_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let app_clone = app.clone();
        let mut last_watched: Option<Vec<PathBuf>> = None;
        let mut _watcher: Option<RecommendedWatcher> = None;

        loop {
            // The folders that exist: X-Plane makes its own at its first screenshot, so a
            // folder appearing restarts the watch too.
            let existing_dirs = existing_folders(&app_clone);

            if last_watched.as_ref() != Some(&existing_dirs) {
                _watcher = None;
                last_watched = Some(existing_dirs.clone());

                {

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

                                    if existing_folders(&app_inner) != existing_dirs {
                                        crate::append_log(&app_inner, "Screenshot folders changed, watching again...".to_string());
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

/// The configured folders that exist, each once.
fn existing_folders(app: &AppHandle) -> Vec<PathBuf> {
    let config = app.state::<ConfigManager>().get_config();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for folder in config.screenshot_folders {
        if folder.path.is_dir() && !dirs.contains(&folder.path) {
            dirs.push(folder.path);
        }
    }
    dirs
}

/// The folder a new file is in, as configured.
fn folder_of(config: &crate::config::Config, path: &std::path::Path) -> Option<crate::config::ScreenshotFolder> {
    let parent = path.parent()?;
    config.screenshot_folders.iter().find(|f| f.path == parent).cloned()
}

fn handle_new_file(app: &AppHandle, path: PathBuf) {
    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return,
    };
    // Only pictures, and only those the folder's pattern takes.
    let picture = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["png", "jpg", "jpeg", "bmp", "webp"].contains(&e.to_ascii_lowercase().as_str()));
    let Some(folder) = folder_of(&app.state::<ConfigManager>().get_config(), &path) else {
        return;
    };
    if !picture || !folder.takes(file_name) {
        return;
    }

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

    // Only while a sim is connected: these folders hold the desktop's screenshots too.
    let monitors = app.state::<crate::UnifiedMonitor>().get_all_monitors();
    let Some(monitor) = monitors.iter().find(|m| m.is_connected()) else {
        return;
    };
    let aircraft_title = monitor.get_aircraft_info().title;
    let metrics = monitor.get_metrics();
    let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let session = app
        .try_state::<crate::telemetry::Telemetry>()
        .and_then(|t| t.current_session())
        .map(|id| id.to_string());

    let manager = app.state::<ScreenshotManager>();
    match manager.record_screenshot(
        &aircraft_title,
        path.to_str().unwrap_or(""),
        &timestamp,
        metrics.latitude,
        metrics.longitude,
        session.as_deref(),
    ) {
        Err(e) => crate::append_log(app, format!("Failed to record screenshot: {}", e)),
        Ok(screenshot_id) => {
            crate::append_log(app, format!("Captured screenshot {:?}", file_name));
            let _ = app.emit("new-screenshot", screenshot_id);

            // Its folder says whether it goes up by itself.
            if folder.auto_upload && session.is_some() {
                let app_clone = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = perform_screenshot_upload(app_clone.clone(), screenshot_id).await {
                        crate::append_log(&app_clone, format!("Auto-upload failed for screenshot {}: {}", screenshot_id, e));
                    }
                });
            }
        }
    }
}

/// A screenshot's recorded time, UTC "YYYY-MM-DD HH:MM:SS", as the RFC 3339 the service
/// takes.
fn taken_at(timestamp: &str) -> Result<String, String> {
    parse_ts(timestamp)
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0).ok_or_else(|| "Invalid timestamp".to_string()))
        .map(|t| t.to_rfc3339())
}

fn parse_ts(ts: &str) -> Result<i64, String> {
    chrono::NaiveDateTime::parse_from_str(ts.split('.').next().unwrap_or(ts), "%Y-%m-%d %H:%M:%S")
        .map(|dt| dt.and_utc().timestamp())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_recent_screenshots(app: AppHandle) -> Result<Vec<Screenshot>, String> {
    let db_path = app.state::<ScreenshotManager>().db_path.clone();
    let thumbs = app.state::<ScreenshotManager>().thumbs.clone();
    tauri::async_runtime::spawn_blocking(move || ScreenshotManager { db_path, thumbs }.recent(RECENT_LIMIT))
        .await
        .map_err(|e| e.to_string())?
}

/// The path of a screenshot's thumbnail, for the window to show through the asset protocol.
#[tauri::command]
pub async fn get_screenshot_thumbnail(app: AppHandle, id: i64) -> Result<String, String> {
    let db_path = app.state::<ScreenshotManager>().db_path.clone();
    let thumbs = app.state::<ScreenshotManager>().thumbs.clone();
    tauri::async_runtime::spawn_blocking(move || ScreenshotManager { db_path, thumbs }.thumbnail(id))
        .await
        .map_err(|e| e.to_string())?
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn upload_screenshot(app: AppHandle, screenshot_id: i64) -> Result<String, String> {
    perform_screenshot_upload(app, screenshot_id).await
}

/// Shows the screenshot in the file manager.
#[tauri::command]
pub fn reveal_screenshot(app: AppHandle, id: i64) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = app.state::<ScreenshotManager>().path_of(id)?;
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}

/// Sends a screenshot to the service, against the telemetry session it was taken in: the
/// service puts it on the flight it made from that session, by its time.
pub async fn perform_screenshot_upload(app: AppHandle, screenshot_id: i64) -> Result<String, String> {
    let config = app.state::<ConfigManager>().get_config();
    let (base_url, api_token) = config.api_auth()
        .ok_or("Not logged in to the ButterLog service")?;

    // Reading, decoding, and WebP-encoding a 4K screenshot is heavy blocking
    // work; run it off the async runtime threads.
    let db_path = app.state::<ScreenshotManager>().db_path.clone();
    let (webp_vec, file_name, session_id, timestamp) = tauri::async_runtime::spawn_blocking(move || -> Result<(Vec<u8>, String, Option<String>, String), String> {
        let (screenshot, session_id, timestamp) = {
            let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT path, session_id, timestamp FROM screenshots WHERE id = ?1",
                rusqlite::params![screenshot_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?))
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

        Ok((webp_vec, file_name, session_id, timestamp))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::bytes(webp_vec)
        .file_name(file_name)
        .mime_str("image/webp")
        .map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new().part("screenshot", part);
    let Some(session) = session_id else {
        return Err("Taken while no telemetry was recorded, so no flight has it".to_string());
    };
    let url = format!("{}/sessions/{}/screenshots", base_url, session);
    let form = form.text("taken_at", taken_at(&timestamp)?);
    let target = format!("telemetry session {}", session);

    let res = client.post(&url)
        .bearer_auth(&api_token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Upload request failed: {}", e))?;

    if !res.status().is_success() {
        let status = res.status();
        if status.as_u16() == 401 {
            crate::force_logout(&app, "service rejected token during screenshot upload");
        }
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
    app.state::<ScreenshotManager>().mark_as_uploaded(screenshot_id, &data.hash, url)?;

    crate::append_log(&app, format!("Successfully uploaded screenshot {} to {}", screenshot_id, target));

    // Notify UI
    let _ = app.emit("screenshot-uploaded", screenshot_id);

    Ok(data.hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_time_goes_up_as_rfc3339_utc() {
        assert_eq!(taken_at("2026-11-05 01:57:58").unwrap(), "2026-11-05T01:57:58+00:00");
        assert!(taken_at("not a time").is_err());
    }

    #[test]
    fn the_recent_ones_come_newest_first_and_only_while_their_file_is_there() {
        let dir = std::env::temp_dir().join(format!("butterlog-shots-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("screenshots.db");
        Connection::open(&db_path).unwrap().execute_batch(
            "CREATE TABLE screenshots (id INTEGER PRIMARY KEY AUTOINCREMENT, flight_id TEXT NOT NULL,
               aircraft_title TEXT NOT NULL, path TEXT NOT NULL UNIQUE, timestamp TEXT NOT NULL,
               latitude REAL NOT NULL, longitude REAL NOT NULL, remote_hash TEXT, remote_url TEXT, session_id TEXT)",
        ).unwrap();
        let manager = ScreenshotManager { db_path, thumbs: dir.join("thumbs") };
        let file = |name: &str| {
            let p = dir.join(name);
            image::RgbImage::new(800, 450).save(&p).unwrap();
            p.to_string_lossy().into_owned()
        };
        let old = manager.record_screenshot("C172", &file("a.png"), "2026-11-10 08:00:00", 1.0, 2.0, Some("s1")).unwrap();
        let new = manager.record_screenshot("C172", &file("b.png"), "2026-11-10 09:00:00", 1.0, 2.0, None).unwrap();
        manager.record_screenshot("C172", &dir.join("gone.png").to_string_lossy(), "2026-11-10 10:00:00", 1.0, 2.0, None).unwrap();
        // Seen again: the same one, and its session kept.
        assert_eq!(manager.record_screenshot("C172", &dir.join("a.png").to_string_lossy(), "2026-11-10 08:00:00", 0.0, 0.0, None).unwrap(), old);

        let recent = manager.recent(10).unwrap();
        assert_eq!(recent.iter().map(|s| s.id).collect::<Vec<_>>(), [new, old]);
        assert_eq!(recent[1].session_id.as_deref(), Some("s1"));

        let thumb = manager.thumbnail(old).unwrap();
        let (w, h) = image::image_dimensions(&thumb).unwrap();
        assert_eq!((w, h), (THUMB_PX, 203));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
