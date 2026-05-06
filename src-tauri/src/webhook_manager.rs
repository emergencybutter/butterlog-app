use crate::config::ConfigManager;
use crate::models::WebhookFlightSummary;
use reqwest::blocking::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookFlightResponse {
    pub id: i64,
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
        let config = app.state::<ConfigManager>().get_config();
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

    pub fn sync_flight(
        &self, 
        app: &AppHandle, 
        summary: &WebhookFlightSummary,
        db_conn: Option<&Connection>,
        force_update: bool
    ) {
        let base_url = match self.get_base_url(app) {
            Some(url) => url,
            None => return,
        };

        let mut current_id = self.current_remote_id.lock().unwrap();
        let mut last_time = self.last_update_time.lock().unwrap();

        // 1. Try to recover ID from DB if memory is empty
        if current_id.is_none() {
            if let Some(conn) = db_conn {
                let existing: Option<String> = conn.query_row(
                    "SELECT value FROM summary WHERE key = 'remote_id'",
                    [],
                    |r| r.get(0)
                ).map_err(|e| {
                    if !matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                        crate::append_log(app, format!("[Webhook] Database error (read remote_id): {}", e));
                    }
                    e
                }).optional().unwrap_or(None);

                if let Some(id_str) = existing {
                    if let Ok(id) = id_str.parse::<i64>() {
                        *current_id = Some(id);
                    }
                }
            }
        }

        let now = std::time::Instant::now();
        if !force_update {
            if let Some(last) = *last_time {
                if now.duration_since(last).as_secs() < 60 {
                    return;
                }
            }
        }

        match *current_id {
            Some(id) => {
                // Update
                let url = format!("{}/flights/{}", base_url, id);
                let body = serde_json::json!({
                    "arrival": summary.arrival.icao,
                    "statistics": summary
                });

                match self.client.put(&url).json(&body).send() {
                    Ok(res) => {
                        if res.status().is_success() {
                            *last_time = Some(now);
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

                match self.client.post(&url).json(&body).send() {
                    Ok(res) => {
                        if res.status().is_success() {
                            if let Ok(data) = res.json::<WebhookFlightResponse>() {
                                *current_id = Some(data.id);
                                *last_time = Some(now);
                                
                                // 2. Persist new ID to DB
                                if let Some(conn) = db_conn {
                                    if let Err(e) = conn.execute(
                                        "INSERT OR REPLACE INTO summary (key, value) VALUES ('remote_id', ?1)",
                                        params![data.id.to_string()],
                                    ) {
                                        crate::append_log(app, format!("[Webhook] Error writing to DB: {}", e));
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
