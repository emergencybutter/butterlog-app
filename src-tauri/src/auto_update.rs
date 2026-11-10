//! Keeps the app up to date from the background, whether or not its window is open: the
//! window is destroyed while the app sits in the tray, so a check in the window's code
//! only ran once someone opened it.
//!
//! Checks [`FIRST_CHECK`] after startup, then every [`EVERY`]. Never during a flight: an
//! update restarts the app, so it waits for the flight to end, looking again every
//! [`WHILE_FLYING`]. With the window open it asks first, as before; in the tray it installs
//! on its own, and the installer starts the app again.

use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::UnifiedMonitor;

const FIRST_CHECK: Duration = Duration::from_secs(30);
const EVERY: Duration = Duration::from_secs(6 * 3600);
const WHILE_FLYING: Duration = Duration::from_secs(5 * 60);

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        let mut waiting_for: Option<String> = None;
        loop {
            let wait = check(&app, &mut waiting_for).await;
            tokio::time::sleep(wait).await;
        }
    });
}

/// A sim is connected with its aircraft in the air or moving: not the moment to restart.
fn flying(app: &AppHandle) -> bool {
    app.state::<UnifiedMonitor>().get_all_monitors().iter().any(|m| {
        let metrics = m.get_metrics();
        m.is_connected() && metrics.has_position() && (metrics.is_on_ground < 0.5 || metrics.ground_speed > 1.0)
    })
}

/// One check; how long until the next.
async fn check(app: &AppHandle, waiting_for: &mut Option<String>) -> Duration {
    let update = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let update = match update {
        Ok(Some(update)) => update,
        Ok(None) => return EVERY,
        Err(e) => {
            crate::append_log(app, format!("[Update] Couldn't check for an update: {e}"));
            return EVERY;
        }
    };
    if flying(app) {
        if waiting_for.as_deref() != Some(update.version.as_str()) {
            crate::append_log(app, format!("[Update] Version {} is available; it will install once the flight ends.", update.version));
            *waiting_for = Some(update.version.clone());
        }
        return WHILE_FLYING;
    }
    if app.get_webview_window("main").is_some() && !ask(app, &update).await {
        crate::append_log(app, format!("[Update] Version {} declined for now.", update.version));
        return EVERY;
    }
    install(app, update).await
}

/// Asks whoever has the window open; `true` to install.
async fn ask(app: &AppHandle, update: &Update) -> bool {
    let notes = update.body.clone().unwrap_or_default();
    let text = format!("A new version ({}) is available. Would you like to install it now?\n\nRelease notes: {notes}", update.version);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .message(text)
            .title("Update Available")
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::OkCancelCustom("Install".into(), "Later".into()))
            .blocking_show()
    })
    .await
    .unwrap_or(false)
}

/// Downloads and installs `update`, which restarts the app; how long until the next check
/// when it didn't.
async fn install(app: &AppHandle, update: Update) -> Duration {
    crate::append_log(app, format!("[Update] Downloading version {}.", update.version));
    let bytes = match update.download(|_, _| {}, || {}).await {
        Ok(bytes) => bytes,
        Err(e) => {
            crate::append_log(app, format!("[Update] Download failed: {e}"));
            return EVERY;
        }
    };
    // A flight may have started while it downloaded.
    if flying(app) {
        crate::append_log(app, format!("[Update] A flight started; version {} will install after it.", update.version));
        return WHILE_FLYING;
    }
    crate::append_log(app, format!("[Update] Installing version {}; the app restarts.", update.version));
    // On Windows the installer takes over and the app exits here; elsewhere it restarts.
    match update.install(bytes) {
        Ok(()) => app.restart(),
        Err(e) => crate::append_log(app, format!("[Update] Install failed: {e}")),
    }
    EVERY
}
