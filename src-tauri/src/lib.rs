mod airports;
mod config;
mod flight_analyzer;
mod flight_log_manager;
mod models;
mod runways;
mod sim_monitor;

use std::sync::Arc;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

use config::{Config, ConfigManager, SimulatorType};
use flight_log_manager::{
    export_flight_to_csv, get_flight_data, import_flight_from_csv, scan_logs, FlightSummary,
};
use models::FlightMetrics;
use sim_monitor::msfs::SimConnectMonitor;
use sim_monitor::xplane::XPlaneMonitor;
use sim_monitor::SimMonitor;
use std::path::PathBuf;

struct LogState(Mutex<Vec<String>>);

pub struct UnifiedMonitor {
    monitor: Mutex<Option<Arc<dyn SimMonitor>>>,
}

impl UnifiedMonitor {
    pub fn new() -> Self {
        Self {
            monitor: Mutex::new(None),
        }
    }

    pub fn set_monitor(&self, monitor: Arc<dyn SimMonitor>) {
        let mut m = self.monitor.lock().unwrap();
        *m = Some(monitor);
    }

    pub fn get_monitor(&self) -> Option<Arc<dyn SimMonitor>> {
        self.monitor.lock().unwrap().clone()
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
    app: AppHandle,
    state: State<'_, ConfigManager>,
    config: Config,
) -> Result<(), String> {
    let old_config = state.get_config();
    let res = state.update_config(config.clone());

    if res.is_ok() && old_config.simulator_type != config.simulator_type {
        // Restart monitor with new sim type
        let unified = app.state::<UnifiedMonitor>();
        if let Some(m) = unified.get_monitor() {
            m.stop();
        }

        let new_monitor: Arc<dyn SimMonitor> = match config.simulator_type {
            SimulatorType::Msfs => Arc::new(SimConnectMonitor::new()),
            SimulatorType::Xplane => Arc::new(XPlaneMonitor::new()),
        };

        unified.set_monitor(new_monitor.clone());
        let _ = new_monitor.start(app.app_handle().clone(), None);
    }

    res
}

#[tauri::command]
fn start_monitoring(
    app: AppHandle,
    state: State<'_, UnifiedMonitor>,
    log_path: Option<String>,
) -> Result<(), String> {
    if let Some(m) = state.get_monitor() {
        let path = log_path.map(PathBuf::from);
        m.start(app, path).map_err(|e| e.to_string())
    } else {
        Err("No monitor initialized".to_string())
    }
}

#[tauri::command]
fn stop_monitoring(state: State<'_, UnifiedMonitor>) {
    if let Some(m) = state.get_monitor() {
        m.stop();
    }
}

#[tauri::command]
fn get_metrics(state: State<'_, UnifiedMonitor>) -> FlightMetrics {
    if let Some(m) = state.get_monitor() {
        m.get_metrics()
    } else {
        FlightMetrics::default()
    }
}

#[tauri::command]
fn is_sim_connected(state: State<'_, UnifiedMonitor>) -> bool {
    if let Some(m) = state.get_monitor() {
        m.is_connected()
    } else {
        false
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
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .manage(LogState(Mutex::new(Vec::new())))
        .manage(UnifiedMonitor::new())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialize ConfigManager
            let config_manager = ConfigManager::new(app.handle());
            app.manage(config_manager);

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
                match airports::AirportsDatabase::load_from_csv("../public/airports.csv") {
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
                            format!("Failed to load airports.csv: {}", err),
                        );
                    }
                }
            });

            let runways_app_handle = app.handle().clone();
            std::thread::spawn(move || {
                // Load runways.csv and register into Tauri managed state
                match runways::RunwaysDatabase::load_from_csv("../public/runways.csv") {
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
                            format!("Failed to load runways.csv: {}", err),
                        );
                    }
                }
            });

            // Automatically start monitoring based on config
            let config = app.state::<ConfigManager>().get_config();
            let unified = app.state::<UnifiedMonitor>();

            let monitor: Arc<dyn SimMonitor> = match config.simulator_type {
                SimulatorType::Msfs => Arc::new(SimConnectMonitor::new()),
                SimulatorType::Xplane => Arc::new(XPlaneMonitor::new()),
            };

            unified.set_monitor(monitor.clone());
            let _ = monitor.start(app.app_handle().clone(), None);

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
            get_config,
            set_config,
            get_config_async,
            set_config_async,
            get_flight_summaries,
            get_flight_data,
            export_flight_to_csv,
            import_flight_from_csv,
            get_runways
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
