//! Executing web-issued commands against the connected simulator.
//!
//! Commands arrive from the service over a long-poll and are parked on a queue.
//! Whichever sim monitor is running drains it: MSFS on its own thread, because
//! SimConnect calls belong to the thread that owns the connection, and X-Plane
//! in its async loop, because its Web API is just HTTP.
//!
//! Nothing here runs unless the user has switched remote control on. It is off
//! by default: logging in should not hand anyone a lever on your simulator.

use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::Value;
use std::collections::VecDeque;
use tauri::{AppHandle, Manager};

/// Longest the service will hold a long-poll open.
const WAIT_SECS: u64 = 25;

#[derive(Debug, Clone, Deserialize)]
pub struct SimCommand {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub stability: String,
}

/// What actually happened, reported back to the service.
///
/// `Unsupported` is a first-class outcome rather than an error: it is what the
/// app returns when the connected sim has no binding for a command, and it is
/// what lets the web UI stop offering it.
#[derive(Debug, Clone)]
pub enum Outcome {
    Applied,
    Unsupported(String),
    Rejected(String),
}

impl SimCommand {
    /// Label for logs, flagging the ones that may quietly do nothing.
    pub fn label(&self) -> String {
        if self.stability == "beta" {
            format!("{} (beta)", self.kind)
        } else {
            self.kind.clone()
        }
    }
}

impl Outcome {
    fn parts(&self) -> (&'static str, Option<String>) {
        match self {
            Outcome::Applied => ("applied", None),
            Outcome::Unsupported(d) => ("unsupported", Some(d.clone())),
            Outcome::Rejected(d) => ("rejected", Some(d.clone())),
        }
    }
}

#[derive(Default)]
pub struct CommandQueue {
    inner: Mutex<VecDeque<SimCommand>>,
}

impl CommandQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_all(&self, cmds: Vec<SimCommand>) {
        let mut q = self.inner.lock();
        for c in cmds {
            q.push_back(c);
        }
    }

    /// Take everything queued. Called by the sim monitors each loop.
    pub fn drain(&self) -> Vec<SimCommand> {
        let mut q = self.inner.lock();
        q.drain(..).collect()
    }

    pub fn clear(&self) {
        self.inner.lock().clear();
    }
}

/// Every command the app knows how to execute, with its stability.
pub const CATALOG: &[(&str, bool)] = &[
    // (type, is_beta)
    ("pause", false),
    ("set_heading_bug", true),
    ("ap_heading_mode", true),
    ("ap_nav_mode", true),
    ("set_vertical_speed", true),
    ("set_altitude", true),
];

/// Which commands to advertise to the service, given the user's settings.
///
/// Reported in `statistics.capabilities.commands` so the live page renders only
/// controls that can actually do something. With remote control off the list is
/// empty and the page shows no control strip at all.
pub fn capabilities(allow_remote: bool, allow_beta: bool) -> Vec<&'static str> {
    if !allow_remote {
        return Vec::new();
    }
    CATALOG
        .iter()
        .filter(|(_, beta)| allow_beta || !*beta)
        .map(|(kind, _)| *kind)
        .collect()
}

fn f(params: &Value, key: &str) -> Option<f64> {
    params.get(key).and_then(|v| v.as_f64())
}

fn b(params: &Value, key: &str) -> Option<bool> {
    params.get(key).and_then(|v| v.as_bool())
}

fn s<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(|v| v.as_str())
}

/// The MSFS "K event" for a command, as `(event_name, data)`.
///
/// Re-validates every parameter rather than trusting the service, so a buggy or
/// compromised client cannot drive an out-of-range value into a running sim.
/// Returns `None` for anything unrecognised or out of range.
pub fn msfs_event(kind: &str, params: &Value) -> Option<(&'static str, u32)> {
    match kind {
        "pause" => match s(params, "state")? {
            "on" => Some(("PAUSE_SET", 1)),
            "off" => Some(("PAUSE_SET", 0)),
            "toggle" => Some(("PAUSE_TOGGLE", 0)),
            _ => None,
        },
        "set_heading_bug" => {
            let h = f(params, "heading")?;
            if !(0.0..360.0).contains(&h) {
                return None;
            }
            Some(("HEADING_BUG_SET", h.round() as u32))
        }
        "ap_heading_mode" => Some(if b(params, "enabled")? {
            ("AP_HDG_HOLD_ON", 0)
        } else {
            ("AP_HDG_HOLD_OFF", 0)
        }),
        "ap_nav_mode" => Some(if b(params, "enabled")? {
            ("AP_NAV1_HOLD_ON", 0)
        } else {
            ("AP_NAV1_HOLD_OFF", 0)
        }),
        "set_vertical_speed" => {
            let v = f(params, "fpm")?;
            if !(-6000.0..=6000.0).contains(&v) {
                return None;
            }
            // Signed fpm crosses the wire as a two's-complement DWORD.
            Some(("AP_VS_VAR_SET_ENGLISH", v.round() as i32 as u32))
        }
        "set_altitude" => {
            let a = f(params, "feet")?;
            if !(0.0..=60000.0).contains(&a) {
                return None;
            }
            Some(("AP_ALT_VAR_SET_ENGLISH", a.round() as u32))
        }
        _ => None,
    }
}

/// How a command maps onto the X-Plane Web API.
#[derive(Debug, PartialEq)]
pub enum XPlaneAction {
    /// Activate a named command.
    Command(&'static str),
    /// Write a value to a named dataref.
    Dataref(&'static str, f64),
    /// Pause/unpause to a specific state. X-Plane only exposes a toggle, so the
    /// executor reads `sim/time/paused` first and toggles only on a mismatch.
    PauseTo(bool),
}

pub fn xplane_action(kind: &str, params: &Value) -> Option<XPlaneAction> {
    match kind {
        "pause" => match s(params, "state")? {
            "on" => Some(XPlaneAction::PauseTo(true)),
            "off" => Some(XPlaneAction::PauseTo(false)),
            "toggle" => Some(XPlaneAction::Command("sim/operation/pause_toggle")),
            _ => None,
        },
        "set_heading_bug" => {
            let h = f(params, "heading")?;
            if !(0.0..360.0).contains(&h) {
                return None;
            }
            Some(XPlaneAction::Dataref(
                "sim/cockpit/autopilot/heading_mag",
                h,
            ))
        }
        "ap_heading_mode" => b(params, "enabled")
            .map(|_| XPlaneAction::Command("sim/autopilot/heading")),
        "ap_nav_mode" => b(params, "enabled").map(|_| XPlaneAction::Command("sim/autopilot/NAV")),
        "set_vertical_speed" => {
            let v = f(params, "fpm")?;
            if !(-6000.0..=6000.0).contains(&v) {
                return None;
            }
            Some(XPlaneAction::Dataref(
                "sim/cockpit/autopilot/vertical_velocity",
                v,
            ))
        }
        "set_altitude" => {
            let a = f(params, "feet")?;
            if !(0.0..=60000.0).contains(&a) {
                return None;
            }
            Some(XPlaneAction::Dataref("sim/cockpit/autopilot/altitude", a))
        }
        _ => None,
    }
}

/// Refuse a beta command when the user has not opted into beta controls.
pub fn allowed_by_settings(kind: &str, allow_remote: bool, allow_beta: bool) -> Result<(), String> {
    if !allow_remote {
        return Err("Remote control is disabled in ButterLog settings".to_string());
    }
    let beta = CATALOG
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, beta)| *beta)
        .ok_or_else(|| format!("Unknown command `{}`", kind))?;
    if beta && !allow_beta {
        return Err("Beta controls are disabled in ButterLog settings".to_string());
    }
    Ok(())
}

// ── X-Plane execution over the Web API ────────────────────────────────────────

const XPLANE_BASE: &str = "http://localhost:8086/api/v3";

/// Resolve a Web API object id by name, e.g. a command or dataref path.
async fn xplane_lookup_id(client: &reqwest::Client, collection: &str, name: &str) -> Option<i64> {
    let url = format!("{}/{}", XPLANE_BASE, collection);
    let resp = client.get(&url).send().await.ok()?.json::<Value>().await.ok()?;
    resp["data"]
        .as_array()?
        .iter()
        .find(|item| item["name"].as_str() == Some(name))
        .and_then(|item| item["id"].as_i64())
}

async fn xplane_activate(client: &reqwest::Client, name: &str) -> Result<(), String> {
    let id = xplane_lookup_id(client, "commands", name)
        .await
        .ok_or_else(|| format!("X-Plane does not expose command `{}`", name))?;
    let url = format!("{}/command/{}/activate", XPLANE_BASE, id);
    let res = client
        .post(&url)
        .json(&serde_json::json!({ "duration": 0 }))
        .send()
        .await
        .map_err(|e| format!("X-Plane command request failed: {}", e))?;
    if !res.status().is_success() {
        return Err(format!("X-Plane rejected the command ({})", res.status()));
    }
    Ok(())
}

async fn xplane_read(client: &reqwest::Client, name: &str) -> Option<f64> {
    let id = xplane_lookup_id(client, "datarefs", name).await?;
    let url = format!("{}/datarefs/{}/value", XPLANE_BASE, id);
    let resp = client.get(&url).send().await.ok()?.json::<Value>().await.ok()?;
    resp["data"].as_f64()
}

async fn xplane_write(client: &reqwest::Client, name: &str, value: f64) -> Result<(), String> {
    let id = xplane_lookup_id(client, "datarefs", name)
        .await
        .ok_or_else(|| format!("X-Plane does not expose dataref `{}`", name))?;
    let url = format!("{}/datarefs/{}/value", XPLANE_BASE, id);
    let res = client
        .patch(&url)
        .json(&serde_json::json!({ "data": value }))
        .send()
        .await
        .map_err(|e| format!("X-Plane dataref write failed: {}", e))?;
    if !res.status().is_success() {
        return Err(format!("X-Plane rejected the write ({})", res.status()));
    }
    Ok(())
}

/// Execute one command against X-Plane. Needs no plugin — the same Web API the
/// monitor already reads through can also write.
pub async fn execute_xplane(cmd: &SimCommand) -> Outcome {
    let Some(action) = xplane_action(&cmd.kind, &cmd.params) else {
        return Outcome::Rejected(format!(
            "`{}` is not a command X-Plane can service with these parameters",
            cmd.kind
        ));
    };
    let client = reqwest::Client::new();

    let result = match action {
        XPlaneAction::Command(name) => xplane_activate(&client, name).await,
        XPlaneAction::Dataref(name, value) => xplane_write(&client, name, value).await,
        XPlaneAction::PauseTo(want) => {
            match xplane_read(&client, "sim/time/paused").await {
                Some(cur) => {
                    if (cur > 0.5) == want {
                        Ok(()) // already in the requested state
                    } else {
                        xplane_activate(&client, "sim/operation/pause_toggle").await
                    }
                }
                None => Err("Could not read X-Plane's pause state".to_string()),
            }
        }
    };

    match result {
        Ok(()) => Outcome::Applied,
        Err(e) => Outcome::Unsupported(e),
    }
}

// ── Service plumbing ──────────────────────────────────────────────────────────

/// Report a command's outcome. A lost ack costs the status display, not the
/// at-most-once guarantee — the service stamped delivery when it handed the
/// command over.
pub async fn ack(app: &AppHandle, flight_id: i64, command_id: &str, outcome: Outcome) {
    let config = app.state::<crate::config::ConfigManager>().get_config();
    let Some((base_url, api_token)) = config.api_auth() else {
        return;
    };
    let (result, detail) = outcome.parts();
    let url = format!("{}/flights/{}/commands/{}/ack", base_url, flight_id, command_id);
    let _ = reqwest::Client::new()
        .post(&url)
        .bearer_auth(&api_token)
        .json(&serde_json::json!({ "result": result, "detail": detail }))
        .send()
        .await;
    crate::append_log(
        app,
        format!("[Command] {} -> {}{}", command_id, result, detail.map(|d| format!(" ({})", d)).unwrap_or_default()),
    );
}

/// Long-poll the service for commands and park them on the queue.
///
/// Deliberately not a ride-along on the 20s track upload: pause is a reflex
/// action, and a 20s worst case would make it useless.
pub fn spawn_poller(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(WAIT_SECS + 10))
            .build()
            .unwrap_or_default();

        loop {
            let config = app.state::<crate::config::ConfigManager>().get_config();
            let flight = app
                .state::<crate::webhook_manager::WebhookManager>()
                .current_flight();

            let (Some((base_url, api_token)), true, Some((_, flight_id))) =
                (config.api_auth(), config.allow_remote_commands, flight)
            else {
                // Nothing to poll for: no flight, logged out, or the user has
                // remote control switched off.
                if let Some(q) = app.try_state::<CommandQueue>() {
                    q.clear();
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            };

            let url = format!(
                "{}/flights/{}/commands?wait={}",
                base_url, flight_id, WAIT_SECS
            );
            match client.get(&url).bearer_auth(&api_token).send().await {
                Ok(res) if res.status().as_u16() == 204 => {}
                Ok(res) if res.status().is_success() => {
                    match res.json::<Vec<SimCommand>>().await {
                        Ok(cmds) if !cmds.is_empty() => {
                            crate::append_log(
                                &app,
                                format!(
                                    "[Command] Received {}",
                                    cmds.iter()
                                        .map(|c| c.label())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                            );
                            if let Some(q) = app.try_state::<CommandQueue>() {
                                q.push_all(cmds);
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            crate::append_log(&app, format!("[Command] Bad payload: {}", e))
                        }
                    }
                }
                Ok(res) if res.status().as_u16() == 401 => {
                    crate::force_logout(&app, "service rejected token during command poll");
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                }
                Ok(res) if res.status().as_u16() == 409 || res.status().as_u16() == 404 => {
                    // Flight ended or vanished; back off until a new one starts.
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                }
                Ok(res) => {
                    crate::append_log(&app, format!("[Command] Poll failed: {}", res.status()));
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                Err(_) => {
                    // Timeouts are the normal shape of a long-poll that found
                    // no work; just go round again.
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pause_maps_to_a_deterministic_msfs_event() {
        assert_eq!(msfs_event("pause", &json!({"state":"on"})), Some(("PAUSE_SET", 1)));
        assert_eq!(msfs_event("pause", &json!({"state":"off"})), Some(("PAUSE_SET", 0)));
        assert_eq!(msfs_event("pause", &json!({"state":"toggle"})), Some(("PAUSE_TOGGLE", 0)));
    }

    #[test]
    fn negative_vertical_speed_survives_the_dword_round_trip() {
        // AP_VS_VAR_SET_ENGLISH takes a signed fpm in a DWORD; a naive `as u32`
        // on the f64 would saturate to 0 and silently command level flight.
        let (name, data) = msfs_event("set_vertical_speed", &json!({"fpm": -1200.0})).unwrap();
        assert_eq!(name, "AP_VS_VAR_SET_ENGLISH");
        assert_eq!(data as i32, -1200);
    }

    #[test]
    fn the_app_revalidates_ranges_it_was_sent() {
        // The service checks these too. Checking again here is what stops a
        // buggy or hostile client putting a nonsense value into a running sim.
        assert!(msfs_event("set_heading_bug", &json!({"heading": 400.0})).is_none());
        assert!(msfs_event("set_altitude", &json!({"feet": 99999.0})).is_none());
        assert!(msfs_event("set_vertical_speed", &json!({"fpm": -9999.0})).is_none());
        assert!(xplane_action("set_heading_bug", &json!({"heading": -5.0})).is_none());
    }

    #[test]
    fn unknown_commands_map_to_nothing_in_either_sim() {
        assert!(msfs_event("eject", &json!({})).is_none());
        assert!(xplane_action("eject", &json!({})).is_none());
    }

    #[test]
    fn xplane_pause_resolves_to_a_state_not_a_blind_toggle() {
        // Toggling blindly would invert the sim when it is already paused.
        assert_eq!(
            xplane_action("pause", &json!({"state":"on"})),
            Some(XPlaneAction::PauseTo(true))
        );
        assert_eq!(
            xplane_action("pause", &json!({"state":"toggle"})),
            Some(XPlaneAction::Command("sim/operation/pause_toggle"))
        );
    }

    #[test]
    fn capabilities_follow_the_two_settings() {
        assert!(capabilities(false, false).is_empty());
        assert!(capabilities(false, true).is_empty(), "beta cannot re-enable remote control");
        assert_eq!(capabilities(true, false), vec!["pause"]);
        assert_eq!(capabilities(true, true).len(), CATALOG.len());
    }

    #[test]
    fn beta_commands_are_refused_unless_opted_into() {
        assert!(allowed_by_settings("pause", true, false).is_ok());
        assert!(allowed_by_settings("set_altitude", true, false).is_err());
        assert!(allowed_by_settings("set_altitude", true, true).is_ok());
        assert!(allowed_by_settings("pause", false, true).is_err());
        assert!(allowed_by_settings("nonsense", true, true).is_err());
    }
}
