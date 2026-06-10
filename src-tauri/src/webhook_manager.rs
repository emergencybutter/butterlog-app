use crate::models::WebhookFlightSummary;
use reqwest::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookFlightResponse {
    pub id: i64,
    pub peers: Option<Vec<String>>,
}

struct SyncGuard<'a>(&'a Mutex<bool>);

impl<'a> Drop for SyncGuard<'a> {
    fn drop(&mut self) {
        let mut syncing = self.0.lock();
        *syncing = false;
    }
}

pub struct WebhookManager {
    client: Client,
    current_remote_id: Mutex<Option<i64>>,
    last_update_time: Mutex<Option<std::time::Instant>>,
    is_syncing: Mutex<bool>,
}

impl WebhookManager {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            current_remote_id: Mutex::new(None),
            last_update_time: Mutex::new(None),
            is_syncing: Mutex::new(false),
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
        
        if let Some(custom_url) = crate::get_custom_service_url() {
            url = url.replace("https://butterlog.flyvoyager.net", &custom_url);
        }
        
        Some(url)
    }

    pub fn reset(&self) {
        let mut id = self.current_remote_id.lock();
        *id = None;
        let mut time = self.last_update_time.lock();
        *time = None;
        let mut syncing = self.is_syncing.lock();
        *syncing = false;
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

        {
            let mut syncing = self.is_syncing.lock();
            if *syncing {
                return;
            }
            *syncing = true;
        }
        let _guard = SyncGuard(&self.is_syncing);

        let config = app.state::<crate::config::ConfigManager>().get_config();
        let multiplayer_enabled = config.enable_multiplayer_hubs;
        let inject_traffic = config.inject_butterlog_traffic;

        let udp_address = if let Some(multiplayer) = app.try_state::<Arc<crate::multiplayer::MultiplayerManager>>() {
            multiplayer.get_public_address().map(|addr| addr.to_string())
        } else {
            None
        };

        let mut current_id = self.current_remote_id.lock().clone();
        let last_time = self.last_update_time.lock().clone();

        // 1. Try to recover ID from DB if memory is empty (blocking SQLite work
        // runs off the async runtime threads)
        if current_id.is_none() && !summary.log_path.is_empty() {
            let log_path = summary.log_path.clone();
            let recovered = tauri::async_runtime::spawn_blocking(move || {
                let conn = Connection::open(&log_path).ok()?;
                let existing: Option<String> = conn.query_row(
                    "SELECT value FROM summary WHERE key = 'remote_id'",
                    [],
                    |r| r.get(0)
                ).optional().unwrap_or(None);
                existing.and_then(|id_str| id_str.parse::<i64>().ok())
            })
            .await
            .ok()
            .flatten();

            if let Some(id) = recovered {
                current_id = Some(id);
                *self.current_remote_id.lock() = Some(id);
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

        if let Some(ref addr) = udp_address {
            crate::append_log(app, format!("[Webhook Sync] Publishing public UDP address: {} to service", addr));
        }

        match current_id {
            Some(id) => {
                // Update
                let url = format!("{}/flights/{}", base_url, id);
                let body = serde_json::json!({
                    "arrival": summary.arrival.icao,
                    "statistics": summary,
                    "multiplayer_enabled": multiplayer_enabled || inject_traffic,
                    "udp_address": udp_address
                });

                match self.client.put(&url).json(&body).send().await {
                    Ok(res) => {
                        if res.status().is_success() {
                            *self.last_update_time.lock() = Some(now);
                            if let Ok(data) = res.json::<WebhookFlightResponse>().await {
                                if let Some(peers) = data.peers {
                                    crate::append_log(app, format!("[Webhook Sync] Received {} peers from service: {:?}", peers.len(), peers));
                                    if let Some(multiplayer) = app.try_state::<Arc<crate::multiplayer::MultiplayerManager>>() {
                                        multiplayer.update_peers(peers);
                                    }
                                }
                            }
                        } else if res.status().as_u16() == 401 {
                            crate::force_logout(app, "service rejected token during flight sync");
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
                    "statistics": summary,
                    "multiplayer_enabled": multiplayer_enabled || inject_traffic,
                    "udp_address": udp_address
                });

                match self.client.post(&url).json(&body).send().await {
                    Ok(res) => {
                        if res.status().is_success() {
                            if let Ok(data) = res.json::<WebhookFlightResponse>().await {
                                *self.current_remote_id.lock() = Some(data.id);
                                *self.last_update_time.lock() = Some(now);

                                if let Some(peers) = data.peers {
                                    crate::append_log(app, format!("[Webhook Sync] Received {} peers from service: {:?}", peers.len(), peers));
                                    if let Some(multiplayer) = app.try_state::<Arc<crate::multiplayer::MultiplayerManager>>() {
                                        multiplayer.update_peers(peers);
                                    }
                                }
                                
                                // 2. Persist new ID to DB (off the async runtime)
                                if !summary.log_path.is_empty() {
                                    let log_path = summary.log_path.clone();
                                    let id_str = data.id.to_string();
                                    let persist = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
                                        let conn = Connection::open(&log_path).map_err(|e| e.to_string())?;
                                        conn.execute(
                                            "INSERT OR REPLACE INTO summary (key, value) VALUES ('remote_id', ?1)",
                                            params![id_str],
                                        ).map_err(|e| e.to_string())?;
                                        Ok(())
                                    })
                                    .await;
                                    if let Ok(Err(e)) | Err(e) = persist.map_err(|e| e.to_string()) {
                                        crate::append_log(app, format!("[Webhook] Error writing to DB: {}", e));
                                    }
                                }
                                
                                crate::append_log(app, format!("[Webhook] Created remote flight ID: {}", data.id));
                            }
                        } else if res.status().as_u16() == 401 {
                            crate::force_logout(app, "service rejected token during flight sync");
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
