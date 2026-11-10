use directories::UserDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use parking_lot::Mutex;
use tauri::AppHandle;
use tauri::Manager;

/// A folder screenshots are picked up from, while a sim is connected.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotFolder {
    #[ts(type = "string")]
    pub path: PathBuf,
    /// Its screenshots go to the service by themselves.
    #[serde(default)]
    pub auto_upload: bool,
    /// Only files whose name matches, case aside; empty for every file. A pattern that
    /// doesn't compile matches every file, so nothing is silently dropped.
    #[serde(default)]
    pub regex: String,
}

/// The file names sim screenshots get in the desktop's own folders (Game Bar, Windows).
pub const SIM_FILE_NAMES: &str = "^(Microsoft Flight Simulator|X-Plane) .*";

impl ScreenshotFolder {
    pub fn new(path: PathBuf, regex: &str) -> Self {
        Self { path, auto_upload: false, regex: regex.to_string() }
    }

    /// Whether a file of this folder is a screenshot to keep.
    pub fn takes(&self, file_name: &str) -> bool {
        self.regex.trim().is_empty()
            || regex::RegexBuilder::new(self.regex.trim())
                .case_insensitive(true)
                .build()
                .map(|re| re.is_match(file_name))
                .unwrap_or(true)
    }
}

/// Where X-Plane 12 saves its screenshots, in each install found.
pub fn xplane_screenshot_folders() -> Vec<PathBuf> {
    crate::xp_plugin_install::installs()
        .into_iter()
        .map(|install| install.join("Output").join("screenshots"))
        .collect()
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub screenshot_folders: Vec<ScreenshotFolder>,
    // Before folders had settings of their own: one list, and one switch and pattern for
    // all. Read to migrate, never written back.
    #[serde(skip_serializing, default)]
    #[ts(skip)]
    pub screenshot_directories: Vec<PathBuf>,
    #[serde(skip_serializing, default)]
    #[ts(skip)]
    pub screenshot_directory: Option<PathBuf>,
    #[serde(skip_serializing, default)]
    #[ts(skip)]
    pub screenshot_regex_enabled: bool,
    #[serde(skip_serializing, default)]
    #[ts(skip)]
    pub screenshot_regex: String,
    #[serde(skip_serializing, default)]
    #[ts(skip)]
    pub auto_upload_screenshots: bool,
    // Legacy combined field ({service}/api/v0/users/{token}); split into
    // service_url + api_token on load and no longer written back.
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub webhook_url: String,
    #[serde(default = "default_service_url")]
    pub service_url: String,
    #[serde(default)]
    pub api_token: String,
    pub open_at_login: bool,
    pub start_minimized: bool,
    pub inject_butterlog_traffic: bool,
}

pub fn default_service_url() -> String {
    "https://butterlog.flyvoyager.net".to_string()
}

/// Split a legacy webhook URL ({service}/api/v0/users/{token}) into
/// (service_url, api_token), or None if it doesn't match that shape.
fn split_legacy_webhook_url(url: &str) -> Option<(String, String)> {
    let legacy = url.trim_end_matches('/');
    let (base, token) = legacy.rsplit_once("/api/v0/users/")?;
    if base.is_empty() || token.is_empty() {
        return None;
    }
    Some((base.to_string(), token.to_string()))
}

impl Config {
    /// Service API base (`{service}/api/v0`) and bearer token, or None when
    /// not logged in. A `--service-url` CLI override takes precedence over the
    /// saved service URL.
    /// Whether the app is linked to the service.
    ///
    /// Putting flights on the web, streaming them live, and accepting sim
    /// commands from the pilot's own flight page all follow from this rather
    /// than from switches of their own: they are the point of signing in, and
    /// the service only ever offers the controls to the flight's owner.
    pub fn is_logged_in(&self) -> bool {
        !self.api_token.trim().is_empty()
    }

    pub fn api_auth(&self) -> Option<(String, String)> {
        if self.api_token.is_empty() {
            return None;
        }
        let mut base = crate::get_custom_service_url().unwrap_or_else(|| {
            if self.service_url.is_empty() {
                default_service_url()
            } else {
                self.service_url.clone()
            }
        });
        while base.ends_with('/') {
            base.pop();
        }
        Some((format!("{}/api/v0", base), self.api_token.clone()))
    }
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
    pub fn default_with_app_handle(_app: &AppHandle) -> Self {
        let user_dirs = UserDirs::new();
        let home = user_dirs.as_ref().map(|d| d.home_dir().to_path_buf());

        let mut screenshot_directories = Vec::new();
        let mut screenshot_folders: Vec<ScreenshotFolder> = Vec::new();

        // X-Plane's own folder, in each install: every file in it is a screenshot.
        screenshot_folders.extend(xplane_screenshot_folders().into_iter().map(|p| ScreenshotFolder::new(p, "")));

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
        // The desktop's folders hold other screenshots too: only the sims' by name.
        screenshot_folders.extend(screenshot_directories.into_iter().map(|p| ScreenshotFolder::new(p, SIM_FILE_NAMES)));

        Self {
            screenshot_folders,
            screenshot_directories: Vec::new(),
            screenshot_directory: None,
            screenshot_regex_enabled: false,
            screenshot_regex: String::new(),
            auto_upload_screenshots: false,
            webhook_url: "".to_string(),
            service_url: default_service_url(),
            api_token: "".to_string(),
            open_at_login: false,
            start_minimized: false,
            inject_butterlog_traffic: false,
        }
    }
}

// Keep Default trait but it might not be very useful without AppHandle
impl Default for Config {
    fn default() -> Self {
        Self {
            screenshot_folders: Vec::new(),
            screenshot_directories: Vec::new(),
            screenshot_directory: None,
            screenshot_regex_enabled: false,
            screenshot_regex: String::new(),
            auto_upload_screenshots: false,
            webhook_url: "".to_string(),
            service_url: default_service_url(),
            api_token: "".to_string(),
            open_at_login: false,
            start_minimized: false,
            inject_butterlog_traffic: false,
        }
    }
}

/// Gives a config from before folders had settings of their own its folders: each old one
/// with the old switch and pattern, and X-Plane's, which weren't offered. A config without
/// any takes the defaults. Whether it changed.
fn migrate_screenshot_folders(
    config: &mut Config,
    defaults: impl FnOnce() -> Vec<ScreenshotFolder>,
    xplane: Vec<PathBuf>,
) -> bool {
    if !config.screenshot_folders.is_empty() {
        return false;
    }
    let mut old = std::mem::take(&mut config.screenshot_directories);
    old.extend(config.screenshot_directory.take());
    if old.is_empty() {
        config.screenshot_folders = defaults();
        return true;
    }
    let regex = if config.screenshot_regex_enabled { config.screenshot_regex.clone() } else { String::new() };
    let mut folders: Vec<ScreenshotFolder> = xplane
        .iter()
        .cloned()
        .filter(|p| !old.contains(p))
        .map(|p| ScreenshotFolder { path: p, auto_upload: config.auto_upload_screenshots, regex: String::new() })
        .collect();
    // The old pattern names the desktop's sim screenshots; X-Plane's own aren't named so.
    folders.extend(old.into_iter().map(|path| ScreenshotFolder {
        regex: if xplane.contains(&path) { String::new() } else { regex.clone() },
        path,
        auto_upload: config.auto_upload_screenshots,
    }));
    config.screenshot_folders = folders;
    true
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

        let folders_migrated = migrate_screenshot_folders(&mut config, || Config::default_with_app_handle(app).screenshot_folders, xplane_screenshot_folders());

        // Migrate the legacy webhook_url ({service}/api/v0/users/{token}) into
        // the split service_url + api_token fields.
        let mut migrated = false;
        if config.api_token.is_empty() && !config.webhook_url.is_empty() {
            if let Some((base, token)) = split_legacy_webhook_url(&config.webhook_url) {
                config.service_url = base;
                config.api_token = token;
                crate::append_log(app, "Migrated legacy webhook URL to service_url + api_token".to_string());
            }
            config.webhook_url = String::new();
            migrated = true;
        }
        if config.service_url.is_empty() {
            config.service_url = default_service_url();
        }

        let manager = Self {
            config: Mutex::new(config),
            config_path,
        };

        // Save defaults for a new config, and persist the webhook_url migration
        // (webhook_url is skip_serializing, so saving removes it from the file).
        if !manager.config_path.exists() || migrated || folders_migrated {
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
        // The legacy field is read-only: accepted on load, never written back.
        assert!(!json.contains("webhookUrl"));
    }

    #[test]
    fn old_screenshot_settings_become_each_folders() {
        let old: Config = serde_json::from_str(r#"{
            "screenshotDirectories": ["C:\\Captures", "C:\\XP\\Output\\screenshots"],
            "screenshotRegexEnabled": true, "screenshotRegex": "^X-Plane", "autoUploadScreenshots": true,
            "openAtLogin": false, "startMinimized": false, "injectButterlogTraffic": false
        }"#).unwrap();
        let mut config = old.clone();
        let xplane = vec![PathBuf::from("C:\\XP\\Output\\screenshots"), PathBuf::from("D:\\XP2\\Output\\screenshots")];
        assert!(migrate_screenshot_folders(&mut config, Vec::new, xplane.clone()));
        let got: Vec<_> = config.screenshot_folders.iter().map(|f| (f.path.to_string_lossy().into_owned(), f.auto_upload, f.regex.as_str())).collect();
        assert_eq!(got, [
            ("D:\\XP2\\Output\\screenshots".to_string(), true, ""),
            ("C:\\Captures".to_string(), true, "^X-Plane"),
            ("C:\\XP\\Output\\screenshots".to_string(), true, ""),
        ]);
        // Written back, only the folders: once migrated, it stays as set.
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("screenshotDirectories") && !json.contains("autoUploadScreenshots"), "{json}");
        let mut again: Config = serde_json::from_str(&json).unwrap();
        assert!(!migrate_screenshot_folders(&mut again, Vec::new, xplane));
        assert_eq!(again.screenshot_folders, config.screenshot_folders);

        // Nothing saved at all: the defaults.
        let mut empty = Config::default();
        assert!(migrate_screenshot_folders(&mut empty, || vec![ScreenshotFolder::new(PathBuf::from("X"), "")], Vec::new()));
        assert_eq!(empty.screenshot_folders.len(), 1);
    }

    #[test]
    fn a_folders_pattern_picks_its_files() {
        let all = ScreenshotFolder::new(PathBuf::from("X"), " ");
        assert!(all.takes("anything.png"));
        let sims = ScreenshotFolder::new(PathBuf::from("X"), SIM_FILE_NAMES);
        assert!(sims.takes("Microsoft Flight Simulator 2024 2026-11-10 09-12-01.png"));
        assert!(sims.takes("x-plane 2026.png"));
        assert!(!sims.takes("Screenshot 2026-11-10 091201.png"));
        // A pattern that doesn't compile drops nothing.
        assert!(ScreenshotFolder::new(PathBuf::from("X"), "(").takes("a.png"));
    }

    #[test]
    fn test_split_legacy_webhook_url() {
        assert_eq!(
            split_legacy_webhook_url("https://butterlog.flyvoyager.net/api/v0/users/tok123"),
            Some(("https://butterlog.flyvoyager.net".to_string(), "tok123".to_string()))
        );
        assert_eq!(
            split_legacy_webhook_url("http://localhost:8080/api/v0/users/tok123/"),
            Some(("http://localhost:8080".to_string(), "tok123".to_string()))
        );
        assert_eq!(split_legacy_webhook_url("https://example.com/api/v0/users/"), None);
        assert_eq!(split_legacy_webhook_url("https://example.com/other"), None);
        assert_eq!(split_legacy_webhook_url(""), None);
    }

    #[test]
    fn test_api_auth() {
        let mut config = Config::default();
        assert!(config.api_auth().is_none(), "no token means logged out");

        config.api_token = "tok".to_string();
        config.service_url = "https://example.com/".to_string();
        let (base, token) = config.api_auth().expect("logged in");
        assert_eq!(base, "https://example.com/api/v0");
        assert_eq!(token, "tok");
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


}
