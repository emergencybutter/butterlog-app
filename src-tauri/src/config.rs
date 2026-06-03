use directories::UserDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use parking_lot::Mutex;
use tauri::AppHandle;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub log_directory: Option<PathBuf>,
    #[serde(default)]
    pub screenshot_directories: Vec<PathBuf>,
    // Legacy single-dir field kept for migration from old configs
    #[serde(skip_serializing, default)]
    pub screenshot_directory: Option<PathBuf>,
    pub screenshot_regex_enabled: bool,
    pub screenshot_regex: String,
    pub auto_upload_screenshots: bool,
    pub enable_webhook: bool,
    pub webhook_url: String,
    pub open_at_login: bool,
    pub start_minimized: bool,
    pub enable_multiplayer_hubs: bool,
    pub inject_butterlog_traffic: bool,
    pub auto_share_flights: bool,
}

fn find_steam_screenshot_dirs() -> Vec<PathBuf> {
    let steam_roots = [
        PathBuf::from("C:\\Program Files (x86)\\Steam"),
        PathBuf::from("C:\\Program Files\\Steam"),
    ];
    let mut result = Vec::new();
    for root in &steam_roots {
        let userdata = root.join("userdata");
        if !userdata.exists() { continue; }
        let Ok(users) = std::fs::read_dir(&userdata) else { continue };
        for user in users.flatten() {
            let remote = user.path().join("760").join("remote");
            if !remote.exists() { continue; }
            let Ok(games) = std::fs::read_dir(&remote) else { continue };
            for game in games.flatten() {
                let scr = game.path().join("screenshots");
                if scr.exists() {
                    result.push(scr);
                }
            }
        }
    }
    result
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

        let user_dirs = UserDirs::new();
        let home = user_dirs.as_ref().map(|d| d.home_dir().to_path_buf());

        let mut screenshot_directories = Vec::new();

        // Xbox Game Bar default: Videos/Captures
        if let Some(game_bar) = user_dirs.as_ref()
            .and_then(|d| d.video_dir().map(|v| v.join("Captures")))
            .or_else(|| home.as_ref().map(|h| h.join("Videos").join("Captures")))
        {
            screenshot_directories.push(game_bar);
        }

        // Windows default screenshot: Pictures/Screenshots
        if let Some(win_scr) = user_dirs.as_ref()
            .and_then(|d| d.picture_dir().map(|p| p.join("Screenshots")))
            .or_else(|| home.as_ref().map(|h| h.join("Pictures").join("Screenshots")))
        {
            screenshot_directories.push(win_scr);
        }

        // Steam: enumerate existing per-game screenshot dirs
        screenshot_directories.extend(find_steam_screenshot_dirs());

        Self {
            log_directory: Some(log_dir),
            screenshot_directories,
            screenshot_directory: None,
            screenshot_regex_enabled: true,
            screenshot_regex: ".*".to_string(),
            auto_upload_screenshots: false,
            enable_webhook: false,
            webhook_url: "".to_string(),
            open_at_login: false,
            start_minimized: false,
            enable_multiplayer_hubs: false,
            inject_butterlog_traffic: false,
            auto_share_flights: false,
        }
    }
}

// Keep Default trait but it might not be very useful without AppHandle
impl Default for Config {
    fn default() -> Self {
        Self {
            log_directory: None,
            screenshot_directories: Vec::new(),
            screenshot_directory: None,
            screenshot_regex_enabled: true,
            screenshot_regex: ".*".to_string(),
            auto_upload_screenshots: false,
            enable_webhook: false,
            webhook_url: "".to_string(),
            open_at_login: false,
            start_minimized: false,
            enable_multiplayer_hubs: false,
            inject_butterlog_traffic: false,
            auto_share_flights: false,
        }
    }
}

pub fn get_flightlogs_dir_name_from_args(args: &[String]) -> &'static str {
    if args.contains(&"--dev".to_string()) {
        "flightlogs-dev"
    } else {
        "flightlogs"
    }
}

pub fn get_flightlogs_dir_name() -> &'static str {
    let args: Vec<String> = std::env::args().collect();
    get_flightlogs_dir_name_from_args(&args)
}

pub struct ConfigManager {
    pub config: Mutex<Config>,
    config_path: PathBuf,
}

impl ConfigManager {
    pub fn get_config_filename(args: &[String]) -> &'static str {
        if args.contains(&"--dev".to_string()) {
            "config-dev.json"
        } else {
            "config.json"
        }
    }

    pub fn new(app: &AppHandle) -> Self {
        let app_dir = app
            .path()
            .app_data_dir()
            .expect("Failed to get app data dir");
        let args: Vec<String> = std::env::args().collect();
        let config_filename = Self::get_config_filename(&args);
        let config_path = app_dir.join(config_filename);

        if !app_dir.exists() {
            fs::create_dir_all(&app_dir).expect("Failed to create app data dir");
        }

        let mut config: Config = if config_path.exists() {
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

        // Migrate old single screenshot_directory to the new Vec
        if config.screenshot_directories.is_empty() {
            if let Some(old_dir) = config.screenshot_directory.take() {
                config.screenshot_directories.push(old_dir);
            } else {
                // No saved dirs at all — populate with defaults
                let defaults = Config::default_with_app_handle(app);
                config.screenshot_directories = defaults.screenshot_directories;
            }
        }

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
        let config = self.config.lock();
        let content = serde_json::to_string_pretty(&*config).map_err(|e| e.to_string())?;
        fs::write(&self.config_path, content).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_config(&self) -> Config {
        self.config.lock().clone()
    }

    pub fn update_config(&self, new_config: Config) -> Result<(), String> {
        {
            let mut config = self.config.lock();
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

    #[test]
    fn test_get_config_filename() {
        assert_eq!(
            ConfigManager::get_config_filename(&[]),
            "config.json"
        );
        assert_eq!(
            ConfigManager::get_config_filename(&["--dev".to_string()]),
            "config-dev.json"
        );
        assert_eq!(
            ConfigManager::get_config_filename(&["--some-flag".to_string(), "--dev".to_string()]),
            "config-dev.json"
        );
        assert_eq!(
            ConfigManager::get_config_filename(&["--dev-something".to_string()]),
            "config.json"
        );
    }

    #[test]
    fn test_get_flightlogs_dir_name() {
        assert_eq!(
            get_flightlogs_dir_name_from_args(&[]),
            "flightlogs"
        );
        assert_eq!(
            get_flightlogs_dir_name_from_args(&["--dev".to_string()]),
            "flightlogs-dev"
        );
        assert_eq!(
            get_flightlogs_dir_name_from_args(&["--some-flag".to_string(), "--dev".to_string()]),
            "flightlogs-dev"
        );
        assert_eq!(
            get_flightlogs_dir_name_from_args(&["--dev-something".to_string()]),
            "flightlogs"
        );
    }
}
