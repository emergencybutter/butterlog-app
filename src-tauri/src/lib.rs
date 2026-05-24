mod airports;
mod config;
mod flight_analyzer;
mod flight_log_manager;
mod models;
mod multiplayer;
mod runways;
mod screenshot_manager;
mod sim_monitor;
mod webhook_manager;

use std::sync::Arc;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::menu::{Menu, MenuItem};
use tauri::path::BaseDirectory;
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

use config::{Config, ConfigManager};
use flight_log_manager::{
    export_flight_to_csv, get_flight_data, import_flight_from_csv, scan_logs, FlightSummary,
};
use models::FlightMetrics;
use multiplayer::MultiplayerManager;
use rusqlite::OptionalExtension;
use screenshot_manager::ScreenshotManager;
use sim_monitor::msfs::SimConnectMonitor;
use sim_monitor::xplane::XPlaneMonitor;
use sim_monitor::SimMonitor;
use std::path::PathBuf;

struct LogState(Mutex<Vec<String>>);

pub struct RegenerateSummaryFlag(pub bool);

pub struct UnifiedMonitor {
    monitors: Mutex<Vec<Arc<dyn SimMonitor>>>,
}

impl UnifiedMonitor {
    pub fn new() -> Self {
        Self {
            monitors: Mutex::new(Vec::new()),
        }
    }

    pub fn add_monitor(&self, monitor: Arc<dyn SimMonitor>) {
        let mut m = self.monitors.lock().unwrap();
        m.push(monitor);
    }

    pub fn get_connected_monitor(&self) -> Option<Arc<dyn SimMonitor>> {
        let monitors = self.monitors.lock().unwrap();
        for m in monitors.iter() {
            if m.is_connected() {
                return Some(m.clone());
            }
        }
        // Fallback to first one if none connected, for start/stop commands
        monitors.first().cloned()
    }

    pub fn get_all_monitors(&self) -> Vec<Arc<dyn SimMonitor>> {
        self.monitors.lock().unwrap().clone()
    }
}

pub(crate) fn append_log(app: &AppHandle, message: String) {
    let state = app.state::<LogState>();
    let mut logs = state.0.lock().unwrap();
    logs.push(message.clone());
    let _ = app.emit("log-update", message);
}

#[tauri::command]
async fn get_flight_summaries(app: AppHandle) -> Result<Vec<FlightSummary>, String> {
    scan_logs(app)
}

#[tauri::command]
async fn get_flight_summary(app: AppHandle, filename: String) -> Result<FlightSummary, String> {
    let app_data_dir = app.path().app_data_dir().unwrap();
    let log_dir = app_data_dir.join("flightlogs");
    let path = log_dir.join(&filename);
    
    if !path.exists() {
        return Err("Flight log not found".to_string());
    }

    crate::flight_log_manager::parse_db_file(&app, &path)
        .ok_or_else(|| "Failed to parse flight summary".to_string())
}

#[tauri::command]
fn get_config(state: State<'_, ConfigManager>) -> Config {
    state.get_config()
}

#[tauri::command]
fn set_config(state: State<'_, ConfigManager>, config: Config) -> Result<(), String> {
    state.update_config(config)
}

#[tauri::command]
async fn get_config_async(state: State<'_, ConfigManager>) -> Result<Config, String> {
    Ok(state.get_config())
}

#[tauri::command]
async fn set_config_async(
    _app: AppHandle,
    state: State<'_, ConfigManager>,
    config: Config,
) -> Result<(), String> {
    state.update_config(config)
}

#[tauri::command]
fn start_monitoring(
    app: AppHandle,
    state: State<'_, UnifiedMonitor>,
    log_path: Option<String>,
) -> Result<(), String> {
    let monitors = state.get_all_monitors();
    if monitors.is_empty() {
        return Err("No monitors initialized".to_string());
    }

    let path = log_path.map(PathBuf::from);
    for m in monitors {
        let _ = m.start(app.clone(), path.clone());
    }
    Ok(())
}

#[tauri::command]
fn stop_monitoring(state: State<'_, UnifiedMonitor>) {
    for m in state.get_all_monitors() {
        m.stop();
    }
}

#[tauri::command]
fn get_metrics(state: State<'_, UnifiedMonitor>) -> FlightMetrics {
    if let Some(m) = state.get_connected_monitor() {
        m.get_metrics()
    } else {
        FlightMetrics::default()
    }
}

#[tauri::command]
fn is_sim_connected(state: State<'_, UnifiedMonitor>) -> bool {
    state.get_all_monitors().iter().any(|m| m.is_connected())
}

#[tauri::command]
fn get_connected_sims(state: State<'_, UnifiedMonitor>) -> Vec<String> {
    state
        .get_all_monitors()
        .iter()
        .filter(|m| m.is_connected())
        .map(|m| m.id().to_string())
        .collect()
}

#[tauri::command]
fn is_flight_ongoing(state: State<'_, UnifiedMonitor>) -> bool {
    state.get_all_monitors().iter().any(|m| m.is_monitoring())
}

#[tauri::command]
fn get_current_flight_id(state: State<'_, UnifiedMonitor>) -> String {
    if let Some(m) = state.get_connected_monitor() {
        m.get_current_flight_id()
    } else {
        "".to_string()
    }
}

#[tauri::command]
async fn get_remote_id(app: AppHandle, filename: String) -> Result<Option<i64>, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let log_dir = app_data_dir.join("flightlogs");
    let path = log_dir.join(&filename);
    
    if !path.exists() {
        return Ok(None);
    }

    let conn = rusqlite::Connection::open(path).map_err(|e| e.to_string())?;
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

#[tauri::command]
async fn upload_screenshot(
    app: AppHandle,
    screenshot_id: i64,
    flight_filename: String,
) -> Result<String, String> {
    let flight_id = flight_filename.replace(".db", "");
    screenshot_manager::perform_screenshot_upload(app, screenshot_id, flight_id).await
}

#[tauri::command]
async fn start_discord_login(app: AppHandle) -> Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // 1. Bind to localhost on random free port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind to local port: {}", e))?;
    let port = listener.local_addr().map_err(|e| format!("Failed to get local port: {}", e))?.port();
    
    // 2. Open default browser
    let login_url = format!("https://butterlog.flyvoyager.net/login?port={}", port);
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(&login_url, None::<String>).map_err(|e| format!("Failed to open browser: {}", e))?;
    
    // 3. Await one connection with timeout of 120s
    let listen_future = async move {
        let (mut stream, _) = listener.accept().await?;
        
        let mut buffer = [0; 2048];
        let n = stream.read(&mut buffer).await?;
        let request_str = String::from_utf8_lossy(&buffer[..n]);
        
        // Parse GET /?token=XYZ HTTP/1.1
        let mut token = None;
        if let Some(first_line) = request_str.lines().next() {
            let parts: Vec<&str> = first_line.split_whitespace().collect();
            if parts.len() >= 2 {
                let path = parts[1];
                if let Some(pos) = path.find("token=") {
                    let t = &path[pos + 6..];
                    let end_pos = t.find('&').unwrap_or(t.len());
                    token = Some(t[..end_pos].to_string());
                }
            }
        }
        
        if let Some(t) = token {
            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n\
                <!DOCTYPE html>\
                <html>\
                <head>\
                    <title>Logged In</title>\
                    <style>\
                        body { font-family: -apple-system, BlinkMacSystemFont, sans-serif; background: #1e1e2e; color: #cdd6f4; text-align: center; padding-top: 100px; margin: 0; }\
                        .card { display: inline-block; background: #313244; padding: 2rem; border-radius: 12px; box-shadow: 0 4px 12px rgba(0,0,0,0.3); }\
                        h1 { color: #a6e3a1; margin-top: 0; }\
                        p { color: #a6adc8; }\
                    </style>\
                </head>\
                <body>\
                    <div class='card'>\
                        <h1>Login Successful!</h1>\
                        <p>You can close this tab and return to the ButterLog app.</p>\
                    </div>\
                </body>\
                </html>";
            stream.write_all(response.as_bytes()).await?;
            Ok(t)
        } else {
            let response = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n\
                <!DOCTYPE html>\
                <html>\
                <head>\
                    <title>Login Failed</title>\
                    <style>\
                        body { font-family: -apple-system, BlinkMacSystemFont, sans-serif; background: #1e1e2e; color: #f38ba8; text-align: center; padding-top: 100px; margin: 0; }\
                        .card { display: inline-block; background: #313244; padding: 2rem; border-radius: 12px; box-shadow: 0 4px 12px rgba(0,0,0,0.3); }\
                        h1 { color: #f38ba8; margin-top: 0; }\
                        p { color: #a6adc8; }\
                    </style>\
                </head>\
                <body>\
                    <div class='card'>\
                        <h1>Login Failed</h1>\
                        <p>No token could be extracted from the login redirect.</p>\
                    </div>\
                </body>\
                </html>";
            stream.write_all(response.as_bytes()).await?;
            Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "No token found"))
        }
    };
    
    match tokio::time::timeout(std::time::Duration::from_secs(120), listen_future).await {
        Ok(Ok(token)) => {
            // Save to configuration
            let state = app.state::<ConfigManager>();
            let mut current_config = state.get_config();
            current_config.webhook_url = format!("https://butterlog.flyvoyager.net/api/v0/users/{}", token);
            current_config.enable_webhook = true;
            state.update_config(current_config).map_err(|e| format!("Failed to save config: {}", e))?;
            Ok(token)
        }
        Ok(Err(e)) => Err(format!("Login failed: {}", e)),
        Err(_) => Err("Login timed out after 2 minutes. Please try again.".to_string()),
    }
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(app: AppHandle, name: &str) -> String {
    append_log(&app, format!("Received greet request for name: '{}'", name));
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn get_logs(state: State<'_, LogState>) -> Vec<String> {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
fn find_nearest_airports(
    lat: f64,
    lon: f64,
    state: State<'_, airports::AirportsDatabase>,
) -> Result<Vec<airports::Airport>, String> {
    // The find_nearest method returns a Vec<&Airport>, so we clone each item to return an owned Vec<Airport>.
    let nearest = state.find_nearest(lat, lon, 10);
    Ok(nearest.into_iter().cloned().collect())
}

#[tauri::command]
fn get_runways(
    ident: String,
    state: State<'_, runways::RunwaysDatabase>,
) -> Result<Vec<runways::Runway>, String> {
    Ok(state.find_for_ident(&ident))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let regenerate_summary = args.contains(&"--regenerate_summary".to_string());

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .manage(LogState(Mutex::new(Vec::new())))
        .manage(UnifiedMonitor::new())
        .manage(RegenerateSummaryFlag(regenerate_summary))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialize ConfigManager
            let config_manager = ConfigManager::new(app.handle());
            app.manage(config_manager);

            // Initialize ScreenshotManager
            let screenshot_manager = ScreenshotManager::new(app.handle());
            app.manage(screenshot_manager);

            // Initialize WebhookManager
            app.manage(webhook_manager::WebhookManager::new());

            // Initialize MultiplayerManager
            let multiplayer = Arc::new(MultiplayerManager::new());
            app.manage(multiplayer.clone());
            multiplayer.start(app.handle().clone());



            // Start screenshot watcher
            screenshot_manager::start_screenshot_watcher(app.handle().clone());

            let pkg_info = app.package_info();
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            append_log(
                app.handle(),
                format!(
                    "[{}] Startup - App: {} v{}",
                    timestamp, pkg_info.name, pkg_info.version
                ),
            );

            // Tray menu
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Open Window", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            // Load the icon from the public folder and create the tray icon
            let tray_icon =
                tauri::image::Image::from_bytes(include_bytes!("../../public/icon.png"))
                    .expect("Failed to load tray icon");
            TrayIconBuilder::with_id("main")
                .icon(tray_icon)
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::DoubleClick { .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            let airports_app_handle = app.handle().clone();
            std::thread::spawn(move || {
                // Load airports.csv and register into Tauri managed state
                let airports_path = airports_app_handle
                    .path()
                    .resolve("../public/airports.csv", BaseDirectory::Resource)
                    .expect("Failed to resolve airports.csv resource");

                match airports::AirportsDatabase::load_from_csv(&airports_path) {
                    Ok(db) => {
                        append_log(
                            &airports_app_handle,
                            format!(
                                "Successfully loaded {} airports into backend memory.",
                                db.airports.len()
                            ),
                        );
                        airports_app_handle.manage(db);
                    }
                    Err(err) => {
                        append_log(
                            &airports_app_handle,
                            format!("Failed to load airports.csv at {:?}: {}", airports_path, err),
                        );
                    }
                }
            });

            let runways_app_handle = app.handle().clone();
            std::thread::spawn(move || {
                // Load runways.csv and register into Tauri managed state
                let runways_path = runways_app_handle
                    .path()
                    .resolve("../public/runways.csv", BaseDirectory::Resource)
                    .expect("Failed to resolve runways.csv resource");

                match runways::RunwaysDatabase::load_from_csv(&runways_path) {
                    Ok(db) => {
                        append_log(
                            &runways_app_handle,
                            format!(
                                "Successfully loaded {} runways into backend memory.",
                                db.runways.len()
                            ),
                        );
                        runways_app_handle.manage(db);
                    }
                    Err(err) => {
                        append_log(
                            &runways_app_handle,
                            format!("Failed to load runways.csv at {:?}: {}", runways_path, err),
                        );
                    }
                }
            });

            // Automatically start monitoring based on config
            let config = app.state::<ConfigManager>().get_config();
            let unified = app.state::<UnifiedMonitor>();

            let msfs_monitor = Arc::new(SimConnectMonitor::new());
            let xplane_monitor = Arc::new(XPlaneMonitor::new());

            unified.add_monitor(msfs_monitor.clone());
            unified.add_monitor(xplane_monitor.clone());

            let _ = msfs_monitor.start(app.app_handle().clone(), None);
            let _ = xplane_monitor.start(app.app_handle().clone(), None);

            if config.start_minimized {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_logs,
            find_nearest_airports,
            start_monitoring,
            stop_monitoring,
            get_metrics,
            is_sim_connected,
            get_connected_sims,
            is_flight_ongoing,
            get_current_flight_id,
            get_remote_id,
            upload_screenshot,
            start_discord_login,
            get_config,
            set_config,
            get_config_async,
            set_config_async,
            get_flight_summaries,
            get_flight_summary,
            get_flight_data,
            export_flight_to_csv,
            import_flight_from_csv,
            get_runways,
            flight_log_manager::get_aircraft_stats,
            screenshot_manager::get_screenshots_for_flight,
            screenshot_manager::get_random_screenshot_for_aircraft
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
