//! Installs and updates `butterlog_xp_plugin` in the user's X-Plane 12 installs (sim-native
//! telemetry design §4.2, rollout phase 4), so the plugin and the app that talks to it ship
//! together and the hand-built zip releases go away.
//!
//! The app bundles the plugin as the `xp_plugin` resource (staged by
//! `tools/stage-xp-plugin.ps1`): `64/<platform>.xpl`, `aircraft-characteristics.csv` and
//! `version.txt`. X-Plane lists its installs, one per line, in `x-plane_install_12.txt` in
//! the OS's per-user preferences directory.
//!
//! - Installs that already have the plugin are brought up to date at startup.
//! - A fresh install only happens when the user asks, from Settings.
//!
//! A running X-Plane has the plugin loaded, and Windows won't overwrite a loaded library but
//! will rename it. So the old file is renamed aside (`<platform>.xpl.replaced-<time>`) and the
//! new one written in its place; X-Plane picks it up on its next start, and the old file is
//! deleted once nothing holds it.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};

const PLUGIN_DIR: &str = "butterlog_xp_plugin";
const CSV: &str = "aircraft-characteristics.csv";
/// The bundled version, and the installed one (written by this installer only; a plugin
/// installed by hand from a zip has none).
const VERSION_FILE: &str = "version.txt";
const INSTALLED_VERSION_FILE: &str = "butterlog-version.txt";
const REPLACED: &str = ".replaced-";

#[cfg(target_os = "windows")]
const XPL: &str = "win.xpl";
#[cfg(target_os = "macos")]
const XPL: &str = "mac.xpl";
#[cfg(target_os = "linux")]
const XPL: &str = "lin.xpl";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstallStatus {
    /// The X-Plane folder.
    pub path: String,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub up_to_date: bool,
    /// Updated while X-Plane had the old plugin loaded: the new one loads on its next start.
    pub restart_needed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    /// `None` when this build of the app carries no plugin (a dev build that didn't stage one).
    pub bundled_version: Option<String>,
    pub installs: Vec<InstallStatus>,
}

/// The bundled plugin's folder, if this build has one.
fn bundled(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().resolve("xp_plugin", BaseDirectory::Resource).ok()?;
    dir.join("64").join(XPL).is_file().then_some(dir)
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Where X-Plane 12 lists its installs.
fn install_list() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    return std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("x-plane_install_12.txt"));
    #[cfg(target_os = "macos")]
    return std::env::var_os("HOME").map(|d| PathBuf::from(d).join("Library/Preferences/x-plane_install_12.txt"));
    #[cfg(target_os = "linux")]
    return std::env::var_os("HOME").map(|d| PathBuf::from(d).join(".x-plane/x-plane_install_12.txt"));
}

/// The installs listed, that still exist.
pub fn parse_install_list(text: &str) -> Vec<PathBuf> {
    let mut installs: Vec<PathBuf> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let path = PathBuf::from(line.trim_end_matches(['/', '\\']));
        if path.join("Resources").join("plugins").is_dir() && !installs.contains(&path) {
            installs.push(path);
        }
    }
    installs
}

pub(crate) fn installs() -> Vec<PathBuf> {
    install_list().and_then(|p| fs::read_to_string(p).ok()).map_or_else(Vec::new, |t| parse_install_list(&t))
}

fn plugin_dir(xplane: &Path) -> PathBuf {
    xplane.join("Resources").join("plugins").join(PLUGIN_DIR)
}

/// Deletes the plugin files renamed aside by earlier updates. True if one is still held
/// (X-Plane hasn't restarted since).
fn clean_replaced(bin: &Path) -> bool {
    let Ok(entries) = fs::read_dir(bin) else { return false };
    let mut held = false;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(&format!("{XPL}{REPLACED}")) && fs::remove_file(entry.path()).is_err() {
            held = true;
        }
    }
    held
}

pub fn status_of(bundled: Option<&Path>, xplane: &Path) -> InstallStatus {
    let dir = plugin_dir(xplane);
    let bin = dir.join("64");
    let installed_xpl = bin.join(XPL);
    let installed = installed_xpl.is_file();
    let up_to_date = installed
        && bundled.is_some_and(|b| match (fs::read(b.join("64").join(XPL)), fs::read(&installed_xpl)) {
            (Ok(new), Ok(old)) => new == old,
            _ => false,
        });
    InstallStatus {
        path: xplane.to_string_lossy().into_owned(),
        installed,
        installed_version: read_trimmed(&dir.join(INSTALLED_VERSION_FILE)),
        up_to_date,
        restart_needed: clean_replaced(&bin),
    }
}

/// Installs or updates the plugin from `bundled` into `xplane`. Leaves everything else in the
/// plugin's folder (its own CSL models, say) alone.
pub fn install_into(bundled: &Path, xplane: &Path) -> std::io::Result<()> {
    let dir = plugin_dir(xplane);
    let bin = dir.join("64");
    fs::create_dir_all(&bin)?;
    clean_replaced(&bin);
    let target = bin.join(XPL);
    let new = fs::read(bundled.join("64").join(XPL))?;
    if fs::read(&target).ok().as_deref() != Some(&new[..]) {
        // Written beside it first, so a failure never leaves a half-written plugin.
        let staged = bin.join(format!("{XPL}.new"));
        fs::write(&staged, &new)?;
        if target.exists() {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            if let Err(e) = fs::rename(&target, bin.join(format!("{XPL}{REPLACED}{secs}"))) {
                let _ = fs::remove_file(&staged);
                return Err(e);
            }
        }
        fs::rename(&staged, &target)?;
    }
    fs::copy(bundled.join(CSV), dir.join(CSV))?;
    match read_trimmed(&bundled.join(VERSION_FILE)) {
        Some(v) => fs::write(dir.join(INSTALLED_VERSION_FILE), v)?,
        None => {
            let _ = fs::remove_file(dir.join(INSTALLED_VERSION_FILE));
        }
    }
    Ok(())
}

pub fn status(app: &AppHandle) -> PluginStatus {
    let bundled = bundled(app);
    PluginStatus {
        bundled_version: bundled.as_deref().and_then(|b| read_trimmed(&b.join(VERSION_FILE))),
        installs: installs().iter().map(|x| status_of(bundled.as_deref(), x)).collect(),
    }
}

/// Brings every install that has the plugin up to date. Run at startup.
pub fn update_installed(app: &AppHandle) {
    let Some(bundled) = bundled(app) else { return };
    let version = read_trimmed(&bundled.join(VERSION_FILE)).unwrap_or_else(|| "?".into());
    for xplane in installs() {
        let s = status_of(Some(&bundled), &xplane);
        if !s.installed || s.up_to_date {
            continue;
        }
        let result = install_into(&bundled, &xplane);
        crate::append_log(app, match result {
            Ok(()) => format!(
                "[X-Plane] Updated the plugin in {} from {} to {version}; X-Plane loads it on its next start.",
                xplane.display(),
                s.installed_version.as_deref().unwrap_or("an earlier version")
            ),
            Err(e) => format!("[X-Plane] Could not update the plugin in {}: {e}", xplane.display()),
        });
    }
}

#[tauri::command]
pub fn get_xplane_plugin_status(app: AppHandle) -> PluginStatus {
    status(&app)
}

/// Installs or updates the plugin in one X-Plane folder, which must be one X-Plane lists.
#[tauri::command]
pub fn install_xplane_plugin(app: AppHandle, path: String) -> Result<PluginStatus, String> {
    let bundled = bundled(&app).ok_or("This build of ButterLog doesn't include the X-Plane plugin.")?;
    let xplane = installs()
        .into_iter()
        .find(|x| x.to_string_lossy() == path)
        .ok_or_else(|| format!("{path} isn't an X-Plane 12 install."))?;
    install_into(&bundled, &xplane).map_err(|e| format!("Could not install the plugin in {path}: {e}"))?;
    crate::append_log(&app, format!("[X-Plane] Installed the plugin in {path}."));
    Ok(status(&app))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("butterlog-xpinstall-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn bundle(root: &Path, xpl: &[u8], version: &str) -> PathBuf {
        let b = root.join("bundle");
        fs::create_dir_all(b.join("64")).unwrap();
        fs::write(b.join("64").join(XPL), xpl).unwrap();
        fs::write(b.join(CSV), "icao\n").unwrap();
        fs::write(b.join(VERSION_FILE), format!("{version}\n")).unwrap();
        b
    }

    fn xplane(root: &Path) -> PathBuf {
        let x = root.join("X-Plane 12");
        fs::create_dir_all(x.join("Resources").join("plugins")).unwrap();
        x
    }

    #[test]
    fn the_install_list_keeps_installs_that_exist() {
        let root = temp("list");
        let x = xplane(&root);
        let text = format!("C:\\Nowhere\\X-Plane 12/\n{}/\n\n{}\n", x.display(), x.display());
        assert_eq!(parse_install_list(&text), vec![x]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn installs_then_updates_beside_the_users_files() {
        let root = temp("install");
        let x = xplane(&root);
        let b1 = bundle(&root, b"plugin 1", "0.3.0");
        let s = status_of(Some(&b1), &x);
        assert!(!s.installed && !s.up_to_date && s.installed_version.is_none());

        install_into(&b1, &x).unwrap();
        let dir = plugin_dir(&x);
        assert_eq!(fs::read(dir.join("64").join(XPL)).unwrap(), b"plugin 1");
        assert!(dir.join(CSV).is_file());
        let s = status_of(Some(&b1), &x);
        assert!(s.installed && s.up_to_date && !s.restart_needed);
        assert_eq!(s.installed_version.as_deref(), Some("0.3.0"));

        // The user's own CSL models stay.
        fs::create_dir_all(dir.join("CSL")).unwrap();
        fs::write(dir.join("CSL").join("model.obj"), "obj").unwrap();
        let b2 = bundle(&root.join("v2"), b"plugin 2", "0.3.1");
        assert!(!status_of(Some(&b2), &x).up_to_date);
        install_into(&b2, &x).unwrap();
        assert_eq!(fs::read(dir.join("64").join(XPL)).unwrap(), b"plugin 2");
        assert!(dir.join("CSL").join("model.obj").is_file());
        let s = status_of(Some(&b2), &x);
        assert_eq!((s.up_to_date, s.installed_version.as_deref()), (true, Some("0.3.1")));
        // Nothing held the old file, so it's gone.
        let leftovers: Vec<_> = fs::read_dir(dir.join("64")).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(leftovers, [std::ffi::OsString::from(XPL)]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_hand_installed_plugin_has_no_version_and_is_replaced() {
        let root = temp("manual");
        let x = xplane(&root);
        let bin = plugin_dir(&x).join("64");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join(XPL), b"from a zip").unwrap();
        let b = bundle(&root, b"bundled", "0.3.0");
        let s = status_of(Some(&b), &x);
        assert!(s.installed && !s.up_to_date && s.installed_version.is_none());
        install_into(&b, &x).unwrap();
        assert!(status_of(Some(&b), &x).up_to_date);
        let _ = fs::remove_dir_all(&root);
    }

    /// Windows only: a file open without delete sharing can be neither deleted nor
    /// overwritten, like a library X-Plane has loaded.
    #[cfg(windows)]
    #[test]
    fn a_loaded_plugin_is_renamed_aside() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = temp("loaded");
        let x = xplane(&root);
        let b1 = bundle(&root, b"plugin 1", "0.3.0");
        install_into(&b1, &x).unwrap();
        let target = plugin_dir(&x).join("64").join(XPL);
        // Read and delete sharing, no write sharing: the file can be renamed but not
        // overwritten, as with a loaded library.
        let held = fs::OpenOptions::new().read(true).share_mode(0x1 | 0x4).open(&target).unwrap();
        let b2 = bundle(&root.join("v2"), b"plugin 2", "0.3.1");
        install_into(&b2, &x).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"plugin 2");
        drop(held);
        let s = status_of(Some(&b2), &x);
        assert!(s.up_to_date && !s.restart_needed, "cleaned once released");
        let _ = fs::remove_dir_all(&root);
    }
}
