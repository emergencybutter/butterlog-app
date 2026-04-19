use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Clone)]
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
}

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
        }
    }
}

pub struct ConfigManager {
    pub config: Mutex<Config>,
    config_path: PathBuf,
}

impl ConfigManager {
    pub fn new(app: &AppHandle) -> Self {
        let app_dir = app.path().app_data_dir().expect("Failed to get app data dir");
        let config_path = app_dir.join("config.json");
        
        if !app_dir.exists() {
            fs::create_dir_all(&app_dir).expect("Failed to create app data dir");
        }

        let config = if config_path.exists() {
            let content = fs::read_to_string(&config_path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Config::default()
        };

        Self {
            config: Mutex::new(config),
            config_path,
        }
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
