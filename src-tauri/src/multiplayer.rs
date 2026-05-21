use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use crate::models::FlightMetrics;
use crate::config::ConfigManager;
use crate::UnifiedMonitor;

pub struct MultiplayerManager {
    peers: Mutex<Vec<SocketAddr>>,
}

impl MultiplayerManager {
    pub fn new() -> Self {
        Self {
            peers: Mutex::new(Vec::new()),
        }
    }

    pub fn update_peers(&self, peer_strings: Vec<String>) {
        let mut peers = self.peers.lock().unwrap();
        let new_peers: Vec<SocketAddr> = peer_strings
            .into_iter()
            .filter_map(|s| s.parse().ok())
            .collect();
        *peers = new_peers;
    }

    pub fn start(&self, app: AppHandle) {
        let multiplayer = app.state::<Arc<MultiplayerManager>>().inner().clone();
        
        // Background thread for sending data
        std::thread::spawn(move || {
            let socket = match UdpSocket::bind("0.0.0.0:0") {
                Ok(s) => s,
                Err(e) => {
                    crate::append_log(&app, format!("[Multiplayer] Failed to bind UDP socket: {}", e));
                    return;
                }
            };
            
            let _ = socket.set_nonblocking(true);
            
            crate::append_log(&app, format!("[Multiplayer] UDP socket bound to: {:?}", socket.local_addr()));

            // Background thread for receiving data (UDP hole punching needs this to "open" the port)
            let recv_app = app.clone();
            let recv_socket = socket.try_clone().expect("Failed to clone socket");
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match recv_socket.recv_from(&mut buf) {
                        Ok((size, _addr)) => {
                            // For now we just log that we received something
                            // In a real scenario we'd parse the FlightMetrics
                            let _data = &buf[..size];
                            // crate::append_log(&recv_app, format!("[Multiplayer] Received {} bytes from {}", size, addr));
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => {
                            crate::append_log(&recv_app, format!("[Multiplayer] Receive error: {}", e));
                            break;
                        }
                    }
                }
            });

            loop {
                let config = app.state::<ConfigManager>().get_config();
                if config.enable_multiplayer_hubs {
                    let metrics = {
                        let monitor = app.state::<UnifiedMonitor>();
                        if let Some(m) = monitor.get_connected_monitor() {
                            m.get_metrics()
                        } else {
                            FlightMetrics::default()
                        }
                    };

                    let data = serde_json::to_vec(&metrics).unwrap_or_default();
                    if !data.is_empty() {
                        let peers = multiplayer.peers.lock().unwrap();
                        for peer in peers.iter() {
                            let _ = socket.send_to(&data, peer);
                        }
                    }
                }
                
                std::thread::sleep(Duration::from_millis(200));
            }
        });
    }
}
