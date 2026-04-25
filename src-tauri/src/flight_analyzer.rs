use std::collections::{VecDeque, HashMap};
use crate::airports::AirportsDatabase;
use crate::simconnect_monitor::FlightMetrics;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub enum FlightPhase {
    Parked,
    TaxiOut,
    Takeoff,
    Climb,
    Cruise,
    Descent,
    Approach,
    Landing,
    TaxiIn,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlightEvent {
    pub timestamp: String,
    pub event_type: String, // "takeoff", "landing", "top_of_climb", "top_of_descent"
    pub latitude: f64,
    pub longitude: f64,
}

pub struct FlightAnalyzer {
    start_coords: Vec<(f64, f64)>,
    end_coords: VecDeque<(f64, f64)>,
    pub current_phase: FlightPhase,
    last_phase_change: std::time::Instant,
    pub max_alt: f64,
    pub max_gs: f64,
    pub initial_fuel: f64,
    pub final_fuel: f64,
    pub events: Vec<FlightEvent>,
    landed: bool,
}

impl FlightAnalyzer {
    pub fn new() -> Self {
        Self {
            start_coords: Vec::with_capacity(300), // 5 minutes at 1Hz
            end_coords: VecDeque::with_capacity(300),
            current_phase: FlightPhase::Parked,
            last_phase_change: std::time::Instant::now(),
            max_alt: 0.0,
            max_gs: 0.0,
            initial_fuel: 0.0,
            final_fuel: 0.0,
            events: Vec::new(),
            landed: false,
        }
    }

    pub fn reset(&mut self) {
        self.start_coords.clear();
        self.end_coords.clear();
        self.current_phase = FlightPhase::Parked;
        self.last_phase_change = std::time::Instant::now();
        self.max_alt = 0.0;
        self.max_gs = 0.0;
        self.initial_fuel = 0.0;
        self.final_fuel = 0.0;
        self.events.clear();
        self.landed = false;
    }

    pub fn update(&mut self, metrics: &FlightMetrics) -> Option<FlightPhase> {
        let old_phase = self.current_phase;
        
        self.add_point(metrics.latitude, metrics.longitude);
        
        if metrics.alt_msl > self.max_alt {
            self.max_alt = metrics.alt_msl;
        }

        if metrics.gnd_spd > self.max_gs {
            self.max_gs = metrics.gnd_spd;
        }

        let current_fuel = metrics.f_qty_l + metrics.f_qty_r;
        if self.initial_fuel <= 0.0 && current_fuel > 0.0 {
            self.initial_fuel = current_fuel;
        }
        self.final_fuel = current_fuel;

        let on_ground = metrics.sim_on_ground > 0.5;
        let ground_speed = metrics.gnd_spd;
        let ias = metrics.ias;
        let v_spd = metrics.v_spd;

        match self.current_phase {
            FlightPhase::Parked => {
                if on_ground && ground_speed > 2.0 {
                    self.current_phase = FlightPhase::TaxiOut;
                } else if !on_ground {
                    self.current_phase = FlightPhase::Climb;
                }
            }
            FlightPhase::TaxiOut => {
                if on_ground && ias > 45.0 {
                    self.current_phase = FlightPhase::Takeoff;
                } else if !on_ground {
                    self.current_phase = FlightPhase::Climb;
                }
            }
            FlightPhase::Takeoff => {
                if !on_ground {
                    self.current_phase = FlightPhase::Climb;
                    self.add_event("takeoff", metrics.latitude, metrics.longitude);
                }
            }
            FlightPhase::Climb => {
                if !on_ground {
                    if v_spd.abs() < 200.0 && ias > 60.0 {
                        self.current_phase = FlightPhase::Cruise;
                        self.add_event("top_of_climb", metrics.latitude, metrics.longitude);
                    } else if v_spd < -400.0 {
                        self.current_phase = FlightPhase::Descent;
                        self.add_event("top_of_climb", metrics.latitude, metrics.longitude);
                        self.add_event("top_of_descent", metrics.latitude, metrics.longitude);
                    }
                } else {
                    self.current_phase = FlightPhase::Landing;
                    self.add_event("landing", metrics.latitude, metrics.longitude);
                }
            }
            FlightPhase::Cruise => {
                if !on_ground {
                    if v_spd < -500.0 {
                        self.current_phase = FlightPhase::Descent;
                        self.add_event("top_of_descent", metrics.latitude, metrics.longitude);
                    } else if v_spd > 500.0 {
                        self.current_phase = FlightPhase::Climb;
                    }
                } else {
                    self.current_phase = FlightPhase::Landing;
                    self.add_event("landing", metrics.latitude, metrics.longitude);
                }
            }
            FlightPhase::Descent => {
                if !on_ground {
                    if metrics.alt_msl < 3000.0 && v_spd < -200.0 { 
                         self.current_phase = FlightPhase::Approach;
                    } else if v_spd > 300.0 {
                        self.current_phase = FlightPhase::Climb;
                    } else if v_spd.abs() < 200.0 {
                        self.current_phase = FlightPhase::Cruise;
                    }
                } else {
                    self.current_phase = FlightPhase::Landing;
                    self.add_event("landing", metrics.latitude, metrics.longitude);
                }
            }
            FlightPhase::Approach => {
                if on_ground {
                    self.current_phase = FlightPhase::Landing;
                    self.add_event("landing", metrics.latitude, metrics.longitude);
                } else if v_spd > 500.0 {
                    self.current_phase = FlightPhase::Climb;
                }
            }
            FlightPhase::Landing => {
                if on_ground && ground_speed < 30.0 {
                    self.current_phase = FlightPhase::TaxiIn;
                    self.landed = true;
                } else if !on_ground {
                    self.current_phase = FlightPhase::Takeoff;
                }
            }
            FlightPhase::TaxiIn => {
                if ground_speed < 1.0 {
                    // Parked
                } else if !on_ground {
                    self.current_phase = FlightPhase::Climb;
                }
            }
        }

        if self.current_phase != old_phase {
            self.last_phase_change = std::time::Instant::now();
            Some(self.current_phase)
        } else {
            None
        }
    }

    fn add_event(&mut self, event_type: &str, lat: f64, lon: f64) {
        self.events.push(FlightEvent {
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            event_type: event_type.to_string(),
            latitude: lat,
            longitude: lon,
        });
    }

    fn add_point(&mut self, lat: f64, lon: f64) {
        if self.start_coords.len() < 300 {
            self.start_coords.push((lat, lon));
        }
        
        if self.end_coords.len() >= 300 {
            self.end_coords.pop_front();
        }
        self.end_coords.push_back((lat, lon));
    }

    pub fn find_start_icao(&self, db: &AirportsDatabase) -> String {
        self.find_dominant_icao(&self.start_coords, db)
    }

    pub fn find_end_icao(&self, db: &AirportsDatabase) -> String {
        let coords: Vec<(f64, f64)> = self.end_coords.iter().cloned().collect();
        self.find_dominant_icao(&coords, db)
    }

    fn find_dominant_icao(&self, coords: &[(f64, f64)], db: &AirportsDatabase) -> String {
        if coords.is_empty() { return "XXXX".to_string(); }
        
        let mut counts = HashMap::new();
        for (lat, lon) in coords {
            if let Some(airport) = db.find_nearest(*lat, *lon, 1).first() {
                let code = airport.ident.clone();
                *counts.entry(code).or_insert(0) += 1;
            }
        }
        
        counts.into_iter()
            .max_by_key(|&(_, count)| count)
            .map(|(icao, _)| icao)
            .unwrap_or_else(|| "XXXX".to_string())
    }
}
