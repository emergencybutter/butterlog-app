use directories::UserDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub log_directory: Option<PathBuf>,
    pub screenshot_directory: Option<PathBuf>,
    pub geotag_screenshots: bool,
    pub screenshot_regex_enabled: bool,
    pub screenshot_regex: String,
    pub auto_upload_screenshots: bool,
    pub enable_webhook: bool,
    pub webhook_address: String,
    pub xplane_websocket_url: String,
    pub open_at_login: bool,
    pub start_minimized: bool,
}

impl Config {
    pub fn default_with_app_handle(app: &AppHandle) -> Self {
        let log_dir = UserDirs::new()
            .and_then(|dirs| dirs.document_dir().map(|p| p.join("butterlog")))
            .or_else(|| {
                // Fallback if Documents is not found
                UserDirs::new().map(|dirs| dirs.home_dir().join("Documents").join("butterlog"))
            })
            .unwrap_or_else(|| {
                // Final fallback to app data if everything else fails
                app.path()
                    .app_data_dir()
                    .expect("Failed to get app data dir")
                    .join("logs")
            });

        let screenshot_dir = UserDirs::new()
            .and_then(|dirs| dirs.video_dir().map(|p| p.join("Captures")))
            .or_else(|| {
                // Fallback for Windows if video_dir is not enough
                UserDirs::new().map(|dirs| dirs.home_dir().join("Videos").join("Captures"))
            });

        Self {
            log_directory: Some(log_dir),
            screenshot_directory: screenshot_dir,
            geotag_screenshots: false,
            screenshot_regex_enabled: true,
            screenshot_regex: "^(Microsoft Flight Simulator|X-Plane) .*".to_string(),
            auto_upload_screenshots: false,
            enable_webhook: false,
            webhook_address: "".to_string(),
            xplane_websocket_url: "ws://localhost:8080/api/v1/telemetry".to_string(),
            open_at_login: false,
            start_minimized: false,
        }
    }
}

// Keep Default trait but it might not be very useful without AppHandle
impl Default for Config {
    fn default() -> Self {
        Self {
            log_directory: None,
            screenshot_directory: None,
            geotag_screenshots: false,
            screenshot_regex_enabled: true,
            screenshot_regex: "^(Microsoft Flight Simulator|X-Plane) .*".to_string(),
            auto_upload_screenshots: false,
            enable_webhook: false,
            webhook_address: "".to_string(),
            xplane_websocket_url: "ws://localhost:8080/api/v1/telemetry".to_string(),
            open_at_login: false,
            start_minimized: false,
        }
    }
}

pub struct ConfigManager {
    pub config: Mutex<Config>,
    config_path: PathBuf,
}

impl ConfigManager {
    pub fn new(app: &AppHandle) -> Self {
        let app_dir = app
            .path()
            .app_data_dir()
            .expect("Failed to get app data dir");
        let config_path = app_dir.join("config.json");

        if !app_dir.exists() {
            fs::create_dir_all(&app_dir).expect("Failed to create app data dir");
        }

        let config = if config_path.exists() {
            crate::append_log(app, format!("Loading config from: {:?}", config_path));
            let content = fs::read_to_string(&config_path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            crate::append_log(
                app,
                format!("No config found at: {:?}. Using defaults.", config_path),
            );
            Config::default_with_app_handle(app)
        };

        let manager = Self {
            config: Mutex::new(config),
            config_path,
        };

        // Save defaults if it's a new config
        if !manager.config_path.exists() {
            let _ = manager.save();
        }

        manager
    }

    pub fn save(&self) -> Result<(), String> {
        let config = self.config.lock().unwrap();
        let content = serde_json::to_string_pretty(&*config).map_err(|e| e.to_string())?;
        fs::write(&self.config_path, content).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_config(&self) -> Config {
        self.config.lock().unwrap().clone()
    }

    pub fn update_config(&self, new_config: Config) -> Result<(), String> {
        {
            let mut config = self.config.lock().unwrap();
            *config = new_config;
        }
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();
        
        assert_eq!(config.screenshot_regex, deserialized.screenshot_regex);
        assert_eq!(config.open_at_login, deserialized.open_at_login);
    }
}
