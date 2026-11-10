mod auto_update;
mod identity;
mod traffic;
mod legacy_autostart;
mod config;
mod log_file;
mod models;
mod multiplayer;
mod screenshot_manager;
mod service;
mod sim_commands;
mod sim_monitor;
mod telemetry;
mod tray;
mod xp_plugin_install;

// Type and airline resolution, shared with the service (butterlog-service/identity).
use butterlog_identity::{aircraft_characteristics, airlines};

use std::sync::Arc;
use parking_lot::Mutex;
use std::sync::OnceLock;

static CUSTOM_SERVICE_URL: OnceLock<Option<String>> = OnceLock::new();

pub fn get_custom_service_url() -> Option<String> {
    CUSTOM_SERVICE_URL.get().cloned().flatten()
}

use tauri::{AppHandle, Emitter, Manager, State};

use config::{Config, ConfigManager};
use models::FlightMetrics;
use multiplayer::MultiplayerManager;
use screenshot_manager::ScreenshotManager;
#[cfg(msfs)]
use sim_monitor::msfs::SimConnectMonitor;
use sim_monitor::xplane::XPlaneMonitor;
use sim_monitor::SimMonitor;
use std::path::PathBuf;

struct LogState(Mutex<Vec<String>>);

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
        let mut m = self.monitors.lock();
        m.push(monitor);
    }

    /// The currently connected monitor, or None when no simulator is running.
    /// (Start/stop commands iterate get_all_monitors instead of relying on this.)
    pub fn get_connected_monitor(&self) -> Option<Arc<dyn SimMonitor>> {
        let monitors = self.monitors.lock();
        monitors.iter().find(|m| m.is_connected()).cloned()
    }

    pub fn get_all_monitors(&self) -> Vec<Arc<dyn SimMonitor>> {
        self.monitors.lock().clone()
    }
}

/// Maximum number of log lines kept in memory; the app runs for days in the
/// tray, so the buffer must not grow unbounded.
const MAX_LOG_LINES: usize = 2000;

pub(crate) fn append_log(app: &AppHandle, message: String) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let formatted = format!("[{}] {}", timestamp, message);
    println!("{}", formatted);
    if let Some(file) = app.try_state::<log_file::LogFile>() {
        file.write(&formatted);
    }
    let state = app.state::<LogState>();
    let mut logs = state.0.lock();
    logs.push(formatted.clone());
    if logs.len() > MAX_LOG_LINES {
        let excess = logs.len() - MAX_LOG_LINES;
        logs.drain(..excess);
    }
    let _ = app.emit("log-update", formatted);
}

/// Clear the saved ButterLog credentials after the service rejects our token
/// (HTTP 401). Idempotent: only the auth fields are wiped, so the user's
/// traffic-injection / multiplayer preferences are preserved and resume on the
/// next successful login. Emits `auth-logout` so the UI can react.
pub(crate) fn force_logout(app: &AppHandle, reason: &str) {
    let config_manager = app.state::<ConfigManager>();
    let config = config_manager.get_config();

    // Already logged out — nothing to do (also stops concurrent 401s from
    // emitting repeated events).
    if config.api_token.is_empty() {
        return;
    }

    let new_config = Config {
        api_token: String::new(),
        webhook_url: String::new(),
        ..config
    };

    if let Err(e) = config_manager.update_config(new_config) {
        append_log(app, format!("[Auth] Failed to persist forced logout: {}", e));
        return;
    }

    append_log(app, format!("[Auth] Logged out: {}", reason));
    let _ = app.emit("auth-logout", reason.to_string());
}

#[tauri::command]
fn get_config(state: State<'_, ConfigManager>) -> Config {
    state.get_config()
}

#[tauri::command]
fn set_config(app: AppHandle, state: State<'_, ConfigManager>, config: Config) -> Result<(), String> {
    state.update_config(config)?;
    // Signed in or out, say: the Service panel shows it now rather than at the next poll.
    app.state::<service::Service>().poke();
    Ok(())
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
fn get_multiplayer_status(
    multiplayer: State<'_, Arc<MultiplayerManager>>,
) -> multiplayer::MultiplayerDebugInfo {
    multiplayer.get_debug_info()
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
    let base_service_url = get_custom_service_url().unwrap_or_else(|| "https://butterlog.flyvoyager.net".to_string());
    let login_url = format!("{}/api/v0/auth/login?port={}", base_service_url, port);
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
            // Save to configuration, pointing at whichever service performed the
            // login (matters when running against a --service-url dev instance).
            let state = app.state::<ConfigManager>();
            let mut current_config = state.get_config();
            current_config.api_token = token.clone();
            current_config.service_url = base_service_url;
            state.update_config(current_config).map_err(|e| format!("Failed to save config: {}", e))?;
            app.state::<service::Service>().poke();
            Ok(token)
        }
        Ok(Err(e)) => Err(format!("Login failed: {}", e)),
        Err(_) => Err("Login timed out after 2 minutes. Please try again.".to_string()),
    }
}

#[tauri::command]
fn get_logs(state: State<'_, LogState>) -> Vec<String> {
    state.0.lock().clone()
}

/// Shows today's log file in the file manager (its folder, when nothing was written yet).
#[tauri::command]
fn reveal_log_file(app: AppHandle, file: State<'_, log_file::LogFile>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let current = file.current();
    if current.exists() {
        app.opener().reveal_item_in_dir(&current).map_err(|e| e.to_string())
    } else {
        std::fs::create_dir_all(file.dir()).map_err(|e| e.to_string())?;
        app.opener().open_path(file.dir().to_string_lossy(), None::<String>).map_err(|e| e.to_string())
    }
}

/// What each connected sim's capture reads, for the window's Telemetry page.
#[derive(serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SimTelemetry {
    /// `msfs` or `xplane`.
    pub monitor: String,
    /// What the session records; absent until the sampler has run.
    pub live: Option<telemetry::live::LiveTelemetry>,
    /// What the app reads for multiplayer traffic, in its own units.
    pub metrics: FlightMetrics,
    pub aircraft: String,
}

#[tauri::command]
fn get_sim_telemetry(app: AppHandle, state: State<'_, UnifiedMonitor>) -> Vec<SimTelemetry> {
    let view = app.try_state::<telemetry::live::LiveView>();
    state
        .get_all_monitors()
        .iter()
        .filter(|m| m.is_connected())
        .map(|m| SimTelemetry {
            monitor: m.id().to_string(),
            live: view.as_ref().and_then(|v| v.get(m.id())),
            metrics: m.get_metrics(),
            aircraft: m.get_aircraft_info().title,
        })
        .collect()
}

/// Bring the main window to the foreground, rebuilding it first if it was
/// destroyed on close. We close (not hide) the window so its WebView2 process
/// is released while the app lives in the tray; reopening recreates it from the
/// configured window definition in tauri.conf.json.
pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    let cfg = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned();
    match cfg {
        Some(cfg) => match tauri::WebviewWindowBuilder::from_config(app, &cfg) {
            Ok(builder) => {
                if let Err(e) = builder.build() {
                    eprintln!("Failed to rebuild main window: {e}");
                }
            }
            Err(e) => eprintln!("Failed to prepare main window config: {e}"),
        },
        None => eprintln!("No 'main' window configured to rebuild"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().collect();

    // One-shot SimConnect availability probe, launched by the MSFS monitor as a
    // child of ourselves. Must run before any other startup work: it opens a
    // SimConnect client, reports the result through its exit code, and exits so
    // the SDK's per-attempt leak dies with the process. See `msfs::run_probe_child`.
    #[cfg(msfs)]
    if args.iter().any(|a| a == sim_monitor::msfs::PROBE_FLAG) {
        sim_monitor::msfs::run_probe_child();
    }

    let mut custom_url = None;
    for i in 0..args.len() {
        if args[i] == "--service-url" && i + 1 < args.len() {
            custom_url = Some(args[i + 1].clone());
        } else if args[i].starts_with("--service-url=") {
            custom_url = Some(args[i].trim_start_matches("--service-url=").to_string());
        }
    }
    
    // Normalize trailing slash
    let custom_url = custom_url.map(|mut url| {
        if url.ends_with('/') {
            url.pop();
        }
        url
    });

    let _ = CUSTOM_SERVICE_URL.set(custom_url);

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .manage(LogState(Mutex::new(Vec::new())))
        .manage(UnifiedMonitor::new())
        .manage(service::Service::new())
        .manage(telemetry::live::LiveView::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // The log file first, so everything after is in it.
            match app.path().app_data_dir() {
                Ok(data) => {
                    let args: Vec<String> = std::env::args().collect();
                    let file = log_file::LogFile::new(data.join(log_file::dir_name_from_args(&args)));
                    let path = file.current();
                    app.manage(file);
                    append_log(app.handle(), format!(
                        "ButterLog {} starting; logging to {}",
                        app.package_info().version,
                        path.display()
                    ));
                }
                Err(e) => eprintln!("No app data directory, so no log file: {e}"),
            }

            legacy_autostart::cleanup();

            // Keep the X-Plane plugin in step with this app where it's installed.
            let plugin_app = app.handle().clone();
            std::thread::spawn(move || xp_plugin_install::update_installed(&plugin_app));

            // Initialize ConfigManager
            let config_manager = ConfigManager::new(app.handle());
            app.manage(config_manager);

            // Initialize ScreenshotManager
            let screenshot_manager = ScreenshotManager::new(app.handle());
            app.manage(screenshot_manager);

            // Sim-native telemetry: the spool and its uploader. Idle until a sampler
            // records a session, or while a previous run left one unsent.
            match app.path().app_data_dir().map_err(|e| e.to_string()).and_then(|d| {
                telemetry::spool::Spool::open(d.join(telemetry::SPOOL_DIR_NAME), telemetry::SPOOL_MAX_BYTES)
                    .map_err(|e| e.to_string())
            }) {
                Ok(spool) => {
                    app.manage(telemetry::Telemetry::new(spool));
                }
                Err(e) => append_log(app.handle(), format!("[Telemetry] Spool unavailable: {}", e)),
            }
            telemetry::uploader::spawn(app.handle().clone());

            // Web-issued sim commands. The poller is a no-op until the user
            // turns remote control on, which is off by default.
            app.manage(sim_commands::CommandQueue::new());
            sim_commands::spawn_poller(app.handle().clone());

            // Initialize MultiplayerManager
            let multiplayer = Arc::new(MultiplayerManager::new());
            app.manage(multiplayer.clone());
            multiplayer.start(app.handle().clone());



            // Start screenshot watcher
            screenshot_manager::start_screenshot_watcher(app.handle().clone());

            // Updates, from here rather than the window: it is gone while in the tray.
            auto_update::start(app.handle().clone());

            let pkg_info = app.package_info();

            append_log(
                app.handle(),
                format!(
                    "Startup - App: {} v{}",
                    pkg_info.name, pkg_info.version
                ),
            );

            let startup_config = app.state::<ConfigManager>().get_config();
            let screenshot_dir = startup_config.screenshot_directory.as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "None".to_string());
            append_log(
                app.handle(),
                format!("Screenshot directory path: {}", screenshot_dir),
            );

            tray::build(app.handle())?;
            // Who the app is signed in as, and the flights the service made.
            service::spawn(app.handle().clone());

            // The aircraft and airline tables are built in; parsing them and building the
            // type and airline indexes takes a moment, so do it now rather than on the
            // first aircraft load.
            std::thread::spawn(|| {
                butterlog_identity::Resolver::embedded();
            });

            // Automatically start monitoring based on config
            let config = app.state::<ConfigManager>().get_config();
            let unified = app.state::<UnifiedMonitor>();

            // MSFS is Windows only (and needs the `msfs` feature); see build.rs.
            #[cfg(msfs)]
            {
                let msfs_monitor = Arc::new(SimConnectMonitor::new());
                unified.add_monitor(msfs_monitor.clone());
                let _ = msfs_monitor.start(app.app_handle().clone(), None);
            }
            #[cfg(not(msfs))]
            append_log(app.app_handle(), "MSFS support isn't in this build (Windows only).".into());

            let xplane_monitor = Arc::new(XPlaneMonitor::new());
            unified.add_monitor(xplane_monitor.clone());
            let _ = xplane_monitor.start(app.app_handle().clone(), None);

            if config.start_minimized {
                // Destroy (rather than hide) the window so its WebView2 process
                // isn't held in memory while we sit in the tray. It is rebuilt
                // on demand by show_main_window when the user reopens it.
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.close();
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_logs,
            reveal_log_file,
            xp_plugin_install::get_xplane_plugin_status,
            xp_plugin_install::install_xplane_plugin,
            start_monitoring,
            stop_monitoring,
            get_metrics,
            is_sim_connected,
            get_connected_sims,
            get_sim_telemetry,
            get_multiplayer_status,
            start_discord_login,
            get_config,
            set_config,
            get_config_async,
            set_config_async,
            service::get_service_status,
            service::open_service_page,
            service::set_flysto_token,
            service::clear_flysto_token,
            screenshot_manager::get_recent_screenshots,
            screenshot_manager::get_screenshot_thumbnail,
            screenshot_manager::upload_screenshot,
            screenshot_manager::reveal_screenshot,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, event| {
            // Closing the window destroys it to release WebView2 memory, which
            // leaves zero windows and would normally exit the app. A `None` exit
            // code marks that window-driven exit, so keep running in the tray;
            // an explicit code (the Quit menu's app.exit(0)) is allowed through.
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
