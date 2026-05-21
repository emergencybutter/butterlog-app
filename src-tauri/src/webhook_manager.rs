use crate::models::WebhookFlightSummary;
use reqwest::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookFlightResponse {
    pub id: i64,
    pub peers: Option<Vec<String>>,
}

pub struct WebhookManager {
    client: Client,
    current_remote_id: Mutex<Option<i64>>,
    last_update_time: Mutex<Option<std::time::Instant>>,
}

impl WebhookManager {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            current_remote_id: Mutex::new(None),
            last_update_time: Mutex::new(None),
        }
    }

    fn get_base_url(&self, app: &AppHandle) -> Option<String> {
        let config = app.state::<crate::config::ConfigManager>().get_config();
        if !config.enable_webhook || config.webhook_url.is_empty() {
            return None;
        }
        
        let mut url = config.webhook_url.clone();
        if url.ends_with('/') {
            url.pop();
        }
        Some(url)
    }

    pub fn reset(&self) {
        let mut id = self.current_remote_id.lock().unwrap();
        *id = None;
        let mut time = self.last_update_time.lock().unwrap();
        *time = None;
    }

    pub async fn sync_flight(
        &self, 
        app: &AppHandle, 
        summary: &WebhookFlightSummary,
        force_update: bool
    ) {
        let base_url = match self.get_base_url(app) {
            Some(url) => url,
            None => return,
        };

        let mut current_id = self.current_remote_id.lock().unwrap().clone();
        let last_time = self.last_update_time.lock().unwrap().clone();

        // 1. Try to recover ID from DB if memory is empty
        if current_id.is_none() && !summary.log_path.is_empty() {
            if let Ok(conn) = Connection::open(&summary.log_path) {
                let existing: Option<String> = conn.query_row(
                    "SELECT value FROM summary WHERE key = 'remote_id'",
                    [],
                    |r| r.get(0)
                ).optional().unwrap_or(None);

                if let Some(id_str) = existing {
                    if let Ok(id) = id_str.parse::<i64>() {
                        current_id = Some(id);
                        *self.current_remote_id.lock().unwrap() = Some(id);
                    }
                }
            }
        }

        let now = std::time::Instant::now();
        if !force_update {
            if let Some(last) = last_time {
                if now.duration_since(last).as_secs() < 60 { // Reduced to 1m to get peer updates more often
                    return;
                }
            }
        }

        match current_id {
            Some(id) => {
                // Update
                let url = format!("{}/flights/{}", base_url, id);
                let body = serde_json::json!({
                    "arrival": summary.arrival.icao,
                    "statistics": summary
                });

                match self.client.put(&url).json(&body).send().await {
                    Ok(res) => {
                        if res.status().is_success() {
                            *self.last_update_time.lock().unwrap() = Some(now);
                            if let Ok(data) = res.json::<WebhookFlightResponse>().await {
                                if let Some(peers) = data.peers {
                                    if let Some(multiplayer) = app.try_state::<Arc<crate::multiplayer::MultiplayerManager>>() {
                                        multiplayer.update_peers(peers);
                                    }
                                }
                            }
                        } else {
                            crate::append_log(app, format!("[Webhook] Update failed (ID {}): {}", id, res.status()));
                        }
                    }
                    Err(e) => {
                        crate::append_log(app, format!("[Webhook] Update error: {}", e));
                    }
                }
            }
            None => {
                // Create
                let url = format!("{}/flights", base_url);
                let body = serde_json::json!({
                    "departure": summary.departure.icao,
                    "statistics": summary
                });

                match self.client.post(&url).json(&body).send().await {
                    Ok(res) => {
                        if res.status().is_success() {
                            if let Ok(data) = res.json::<WebhookFlightResponse>().await {
                                *self.current_remote_id.lock().unwrap() = Some(data.id);
                                *self.last_update_time.lock().unwrap() = Some(now);

                                if let Some(peers) = data.peers {
                                    if let Some(multiplayer) = app.try_state::<Arc<crate::multiplayer::MultiplayerManager>>() {
                                        multiplayer.update_peers(peers);
                                    }
                                }
                                
                                // 2. Persist new ID to DB
                                if !summary.log_path.is_empty() {
                                    if let Ok(conn) = Connection::open(&summary.log_path) {
                                        if let Err(e) = conn.execute(
                                            "INSERT OR REPLACE INTO summary (key, value) VALUES ('remote_id', ?1)",
                                            params![data.id.to_string()],
                                        ) {
                                            crate::append_log(app, format!("[Webhook] Error writing to DB: {}", e));
                                        }
                                    }
                                }
                                
                                crate::append_log(app, format!("[Webhook] Created remote flight ID: {}", data.id));
                            }
                        } else {
                            crate::append_log(app, format!("[Webhook] Create failed: {}", res.status()));
                        }
                    }
                    Err(e) => {
                        crate::append_log(app, format!("[Webhook] Create error: {}", e));
                    }
                }
            }
        }
    }
}
