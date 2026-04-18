mod airports;
mod runways;

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

struct LogState(Mutex<Vec<String>>);

fn append_log(app: &AppHandle, message: String) {
    let state = app.state::<LogState>();
    let mut logs = state.0.lock().unwrap();
    logs.push(message.clone());
    let _ = app.emit("log-update", message);
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(LogState(Mutex::new(Vec::new())))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let pkg_info = app.package_info();
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            append_log(
                app.handle(),
                format!("[{}] Startup - App: {} v{}", timestamp, pkg_info.name, pkg_info.version),
            );

            // Load the icon from the public folder and create the tray icon
            let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../../public/icon.png"))
                .expect("Failed to load tray icon");
            tauri::tray::TrayIconBuilder::with_id("main")
                .icon(tray_icon)
                .build(app)?;

            let airports_app_handle = app.handle().clone();
            std::thread::spawn(move || {
                // Load airports.csv and register into Tauri managed state
                match airports::AirportsDatabase::load_from_csv("../public/airports.csv") {
                    Ok(db) => {
                        append_log(
                            &airports_app_handle,
                            format!("Successfully loaded {} airports into backend memory.", db.airports.len()),
                        );
                        airports_app_handle.manage(db);
                    }
                    Err(err) => {
                        append_log(&airports_app_handle, format!("Failed to load airports.csv: {}", err));
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
                            format!("Successfully loaded {} runways into backend memory.", db.runways.len()),
                        );
                        runways_app_handle.manage(db);
                    }
                    Err(err) => {
                        append_log(&runways_app_handle, format!("Failed to load runways.csv: {}", err));
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet, get_logs, find_nearest_airports])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
