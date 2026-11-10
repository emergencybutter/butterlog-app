//! The connection to the ButterLog service, as the window's Service panel and status line
//! show it: who the app is signed in as, whether the service answers, and the flights it
//! made from this app's telemetry (`GET /api/v0/me`).
//!
//! The app logs no flight itself: the service makes them. So the flight being flown (web
//! commands are its) and the last one finished (the tray offers its debrief) come from
//! here, asked every [`POLL_SECS`] while signed in.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// How often the service is asked while signed in.
const POLL_SECS: u64 = 20;
/// While a flight is live, so its end shows soon after.
const POLL_LIVE_SECS: u64 = 10;

/// A flight the service made, as `GET /api/v0/me` gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct FlightRef {
    #[ts(type = "number")]
    pub id: i64,
    pub url: String,
    pub debrief_url: String,
    pub departure: String,
    pub arrival: Option<String>,
    pub aircraft: Option<String>,
    pub ended_at: Option<String>,
}

impl FlightRef {
    /// `KPAE → KSEA`, or what is known of it.
    pub fn route(&self) -> String {
        match self.arrival.as_deref().filter(|a| !a.is_empty()) {
            Some(arrival) => format!("{} → {}", self.departure, arrival),
            None => self.departure.clone(),
        }
    }
}

/// The signed-in account, as `GET /api/v0/me` gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Me {
    #[ts(type = "number")]
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub flights_url: String,
    pub settings_url: String,
    pub flysto_connected: bool,
    pub live: Option<FlightRef>,
    pub last_ended: Option<FlightRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "snake_case")]
pub enum Link {
    /// Not signed in.
    SignedOut,
    /// Signed in, not asked yet.
    Connecting,
    Connected,
    /// Signed in, but the service doesn't answer.
    Unreachable,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub link: Link,
    /// The service's web address, e.g. `https://butterlog.flyvoyager.net`.
    pub service_url: String,
    pub me: Option<Me>,
    /// When the service last answered.
    #[ts(type = "number | null")]
    pub last_ok_ms: Option<i64>,
    pub error: Option<String>,
    /// A flight finished that the pilot hasn't looked at yet: the tray shows a dot.
    pub flight_done: Option<FlightRef>,
    /// A telemetry session is being recorded.
    pub recording: bool,
    /// Telemetry waiting to be sent, in kB.
    #[ts(type = "number")]
    pub unsent_kb: u64,
    /// Why the last upload of telemetry stopped, when it did.
    pub upload_error: Option<String>,
}

struct Inner {
    link: Link,
    me: Option<Me>,
    last_ok_ms: Option<i64>,
    error: Option<String>,
    /// The last finished flight already known, once the service answered: a newer one is
    /// a flight just done. `None` until the first answer, so old flights don't notify.
    known_ended: Option<Option<i64>>,
    flight_done: Option<FlightRef>,
    upload_error: Option<String>,
}

/// Managed state.
pub struct Service {
    inner: Mutex<Inner>,
    poke: tokio::sync::Notify,
}

impl Service {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                link: Link::SignedOut,
                me: None,
                last_ok_ms: None,
                error: None,
                known_ended: None,
                flight_done: None,
                upload_error: None,
            }),
            poke: tokio::sync::Notify::new(),
        }
    }

    /// Asks the service now rather than at the next poll: after signing in, say.
    pub fn poke(&self) {
        self.poke.notify_one();
    }

    pub fn last_ended(&self) -> Option<FlightRef> {
        self.inner.lock().me.as_ref().and_then(|m| m.last_ended.clone())
    }

    /// Clears the flight-done notification; what it was, if any.
    pub fn take_flight_done(&self) -> Option<FlightRef> {
        self.inner.lock().flight_done.take()
    }

    pub fn set_upload_error(&self, error: Option<String>) {
        self.inner.lock().upload_error = error;
    }

    /// Takes the service's answer. Returns a flight just finished, if this answer shows one.
    fn answered(&self, me: Me, now_ms: i64) -> Option<FlightRef> {
        let mut inner = self.inner.lock();
        inner.link = Link::Connected;
        inner.last_ok_ms = Some(now_ms);
        inner.error = None;
        let ended = me.last_ended.as_ref().map(|f| f.id);
        let done = match inner.known_ended {
            // A flight newer than the last one known.
            Some(known) if ended.is_some() && ended != known => me.last_ended.clone(),
            _ => None,
        };
        inner.known_ended = Some(ended);
        if done.is_some() {
            inner.flight_done = done.clone();
        }
        inner.me = Some(me);
        done
    }

    fn failed(&self, error: String) {
        let mut inner = self.inner.lock();
        inner.link = Link::Unreachable;
        inner.error = Some(error);
    }

    fn signed_out(&self) {
        let mut inner = self.inner.lock();
        inner.link = Link::SignedOut;
        inner.me = None;
        inner.error = None;
        inner.known_ended = None;
        inner.flight_done = None;
    }

    fn connecting(&self) {
        let mut inner = self.inner.lock();
        if inner.link == Link::SignedOut {
            inner.link = Link::Connecting;
        }
    }

    fn live_id(&self) -> Option<i64> {
        self.inner.lock().me.as_ref().and_then(|m| m.live.as_ref()).map(|f| f.id)
    }
}

/// The flight being flown, as the service made it: web commands are for it.
pub fn live_flight_id(app: &AppHandle) -> Option<i64> {
    app.try_state::<Service>().and_then(|s| s.live_id())
}

/// The service's web address, without `/api/v0`.
pub fn web_base(app: &AppHandle) -> String {
    let config = app.state::<crate::config::ConfigManager>().get_config();
    let mut base = crate::get_custom_service_url().unwrap_or_else(|| {
        if config.service_url.is_empty() {
            crate::config::default_service_url()
        } else {
            config.service_url.clone()
        }
    });
    while base.ends_with('/') {
        base.pop();
    }
    base
}

pub fn status(app: &AppHandle) -> ServiceStatus {
    let service = app.state::<Service>();
    let inner = service.inner.lock();
    let telemetry = app.try_state::<crate::telemetry::Telemetry>();
    ServiceStatus {
        link: inner.link,
        service_url: web_base(app),
        me: inner.me.clone(),
        last_ok_ms: inner.last_ok_ms,
        error: inner.error.clone(),
        flight_done: inner.flight_done.clone(),
        recording: telemetry.as_ref().is_some_and(|t| t.current_session().is_some()),
        unsent_kb: telemetry.as_ref().map_or(0, |t| t.spool().used_bytes() / 1024),
        upload_error: inner.upload_error.clone(),
    }
}

fn changed(app: &AppHandle) {
    let _ = app.emit("service-status", status(app));
}

/// Asks the service once.
async fn ask(client: &reqwest::Client, app: &AppHandle) {
    let config = app.state::<crate::config::ConfigManager>().get_config();
    let service = app.state::<Service>();
    let Some((api, token)) = config.api_auth() else {
        service.signed_out();
        crate::tray::flight_done(app, None);
        changed(app);
        return;
    };
    service.connecting();
    let answer = client.get(format!("{api}/me")).bearer_auth(&token).send().await;
    match answer {
        Ok(r) if r.status().as_u16() == 401 => {
            crate::force_logout(app, "service rejected token");
            service.signed_out();
        }
        Ok(r) if r.status().is_success() => match r.json::<Me>().await {
            Ok(me) => {
                if let Some(done) = service.answered(me, chrono::Utc::now().timestamp_millis()) {
                    crate::append_log(app, format!("[Service] Flight {} done: {}", done.id, done.route()));
                    crate::tray::flight_done(app, Some(&done));
                    let _ = app.emit("flight-done", done);
                }
            }
            Err(e) => service.failed(format!("Unexpected answer: {e}")),
        },
        Ok(r) => service.failed(format!("The service answered {}", r.status())),
        Err(e) => service.failed(if e.is_timeout() { "Timed out".into() } else { e.to_string() }),
    }
    crate::tray::last_flight(app, service.last_ended().as_ref());
    changed(app);
}

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        loop {
            ask(&client, &app).await;
            let every = if live_flight_id(&app).is_some() { POLL_LIVE_SECS } else { POLL_SECS };
            let service = app.state::<Service>();
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(every)) => {}
                _ = service.poke.notified() => {}
            }
        }
    });
}

/// What the service said went wrong: its `{"error": ...}`, or its status.
fn refusal(status: reqwest::StatusCode, body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| format!("The service answered {status}"))
}

/// Sends the pilot's FlySto API token to the service (`PUT /api/v0/me/flysto`), which
/// checks it with FlySto and keeps it. The app keeps no copy.
#[tauri::command]
pub async fn set_flysto_token(app: AppHandle, token: String) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("Paste your FlySto API token first".into());
    }
    flysto(&app, reqwest::Method::PUT, Some(serde_json::json!({ "token": token }))).await
}

/// Has the service forget the pilot's FlySto token: flights no longer go there.
#[tauri::command]
pub async fn clear_flysto_token(app: AppHandle) -> Result<(), String> {
    flysto(&app, reqwest::Method::DELETE, None).await
}

async fn flysto(app: &AppHandle, method: reqwest::Method, body: Option<serde_json::Value>) -> Result<(), String> {
    let config = app.state::<crate::config::ConfigManager>().get_config();
    let (api, token) = config.api_auth().ok_or("Sign in on the Service page first")?;
    let mut request = reqwest::Client::new()
        .request(method, format!("{api}/me/flysto"))
        .bearer_auth(&token)
        .timeout(Duration::from_secs(30));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let r = request.send().await.map_err(|e| format!("Couldn't reach the service: {e}"))?;
    let status = r.status();
    if status.as_u16() == 401 {
        crate::force_logout(app, "service rejected token");
    }
    if !status.is_success() {
        return Err(refusal(status, &r.text().await.unwrap_or_default()));
    }
    crate::append_log(app, "[FlySto] Connection changed".to_string());
    // The Service page and Settings show it from the next answer: ask now.
    app.state::<Service>().poke();
    Ok(())
}

#[tauri::command]
pub fn get_service_status(app: AppHandle) -> ServiceStatus {
    status(&app)
}

/// Opens a page of the service in the browser: `flights` (the pilot's flights), `settings`,
/// `debrief` (the last flight's, which clears the notification), or `live`.
#[tauri::command]
pub fn open_service_page(app: AppHandle, page: String) -> Result<(), String> {
    let url = page_url(&app, &page).ok_or_else(|| format!("No {page} page to open"))?;
    open_url(&app, &url)
}

pub fn page_url(app: &AppHandle, page: &str) -> Option<String> {
    let service = app.state::<Service>();
    let me = service.inner.lock().me.clone();
    let base = web_base(app);
    match page {
        "debrief" => {
            let flight = service.take_flight_done().or_else(|| service.last_ended());
            crate::tray::flight_done(app, None);
            changed(app);
            flight.map(|f| f.debrief_url)
        }
        "live" => me.and_then(|m| m.live).map(|f| f.url),
        "settings" => Some(me.map(|m| m.settings_url).unwrap_or(format!("{base}/content/settings"))),
        _ => Some(me.map(|m| m.flights_url).unwrap_or(format!("{base}/content"))),
    }
}

pub fn open_url(app: &AppHandle, url: &str) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flight(id: i64) -> FlightRef {
        FlightRef {
            id,
            url: format!("https://b/content/flights/{id}"),
            debrief_url: format!("https://b/content/flights/{id}#debrief"),
            departure: "KPAE".into(),
            arrival: Some("KSEA".into()),
            aircraft: None,
            ended_at: None,
        }
    }

    fn me(last_ended: Option<i64>) -> Me {
        Me {
            user_id: 1,
            username: "p".into(),
            display_name: "P".into(),
            avatar_url: None,
            flights_url: "https://b/content".into(),
            settings_url: "https://b/content/settings".into(),
            flysto_connected: false,
            live: None,
            last_ended: last_ended.map(flight),
        }
    }

    #[test]
    fn a_flight_finished_since_the_first_answer_is_done() {
        let service = Service::new();
        // Flights finished before the app asked are not news.
        assert_eq!(service.answered(me(Some(7)), 1), None);
        assert_eq!(service.answered(me(Some(7)), 2), None);
        assert_eq!(service.answered(me(Some(8)), 3).map(|f| f.id), Some(8));
        assert_eq!(service.inner.lock().flight_done.clone().map(|f| f.id), Some(8));
        // Still there until looked at, without telling again.
        assert_eq!(service.answered(me(Some(8)), 4), None);
        assert_eq!(service.take_flight_done().map(|f| f.id), Some(8));
        assert_eq!(service.inner.lock().flight_done.clone(), None);
    }

    #[test]
    fn a_first_flight_ever_is_done_too() {
        let service = Service::new();
        assert_eq!(service.answered(me(None), 1), None);
        assert_eq!(service.answered(me(Some(1)), 2).map(|f| f.id), Some(1));
    }

    #[test]
    fn signing_out_forgets_the_flights() {
        let service = Service::new();
        service.answered(me(Some(1)), 1);
        service.answered(me(Some(2)), 2);
        service.signed_out();
        assert_eq!(service.inner.lock().flight_done.clone(), None);
        // Signed in again: what is there is old news.
        assert_eq!(service.answered(me(Some(2)), 3), None);
    }

    #[test]
    fn the_services_reason_is_what_the_pilot_reads() {
        let bad = reqwest::StatusCode::BAD_REQUEST;
        assert_eq!(
            refusal(bad, r#"{"error":"FlySto doesn't recognise this token"}"#),
            "FlySto doesn't recognise this token"
        );
        assert_eq!(refusal(reqwest::StatusCode::BAD_GATEWAY, "<html>"), "The service answered 502 Bad Gateway");
    }

    #[test]
    fn a_route_reads_from_departure_to_arrival() {
        assert_eq!(flight(1).route(), "KPAE → KSEA");
        let mut f = flight(1);
        f.arrival = None;
        assert_eq!(f.route(), "KPAE");
    }
}
