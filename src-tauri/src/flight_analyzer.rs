use crate::airports::AirportsDatabase;
use crate::models::{FlightEvent, FlightMetrics, FlightPhase};
use crate::runways::{Runway, RunwaysDatabase};
use std::collections::{HashMap, VecDeque};

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
    airborne_start: bool,
    pub last_on_ground: bool,
    last_autopilot_active: bool,
    last_v_spd: f64,
    pub takeoff_timestamp: Option<String>,
    first_timestamp: Option<String>,
    last_timestamp: Option<String>,

    // Stability check state
    pending_toc_ts: Option<String>,
    pending_toc_coords: (f64, f64),
    pending_tod_ts: Option<String>,
    pending_tod_coords: (f64, f64),

    // Landing performance tracking
    pub max_landing_g: f64,
    pub touchdown_fpm: f64,
}

impl FlightAnalyzer {
    pub fn new() -> Self {
        Self {
            start_coords: Vec::with_capacity(300), 
            end_coords: VecDeque::with_capacity(300),
            current_phase: FlightPhase::Parked,
            last_phase_change: std::time::Instant::now(),
            max_alt: 0.0,
            max_gs: 0.0,
            initial_fuel: 0.0,
            final_fuel: 0.0,
            events: Vec::new(),
            landed: false,
            airborne_start: false,
            last_on_ground: true,
            last_autopilot_active: false,
            last_v_spd: 0.0,
            takeoff_timestamp: None,
            first_timestamp: None,
            last_timestamp: None,
            pending_toc_ts: None,
            pending_toc_coords: (0.0, 0.0),
            pending_tod_ts: None,
            pending_tod_coords: (0.0, 0.0),
            max_landing_g: 0.0,
            touchdown_fpm: 0.0,
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
        self.airborne_start = false;
        self.last_on_ground = true;
        self.last_autopilot_active = false;
        self.last_v_spd = 0.0;
        self.takeoff_timestamp = None;
        self.first_timestamp = None;
        self.last_timestamp = None;
        self.max_landing_g = 0.0;
        self.touchdown_fpm = 0.0;
        self.pending_toc_ts = None;
        self.pending_toc_coords = (0.0, 0.0);
        self.pending_tod_ts = None;
        self.pending_tod_coords = (0.0, 0.0);
    }

    pub fn parse_timestamp(&self, ts: &str) -> Option<i64> {
        chrono::NaiveDateTime::parse_from_str(
            ts.split('.').next().unwrap_or(ts),
            "%Y-%m-%d %H:%M:%S",
        ).ok().map(|dt| dt.and_utc().timestamp())
    }

    pub fn get_duration_minutes(&self) -> i64 {
        if let (Some(start), Some(end)) = (&self.first_timestamp, &self.last_timestamp) {
            if let (Ok(start_dt), Ok(end_dt)) = (
                chrono::NaiveDateTime::parse_from_str(
                    start.split('.').next().unwrap_or(start),
                    "%Y-%m-%d %H:%M:%S",
                ),
                chrono::NaiveDateTime::parse_from_str(
                    end.split('.').next().unwrap_or(end),
                    "%Y-%m-%d %H:%M:%S",
                ),
            ) {
                return end_dt.signed_duration_since(start_dt).num_minutes();
            }
        }
        0
    }

    pub fn update(&mut self, metrics: &FlightMetrics, timestamp: &str) -> Option<FlightPhase> {
        if self.first_timestamp.is_none() {
            self.first_timestamp = Some(timestamp.to_string());
        }
        self.last_timestamp = Some(timestamp.to_string());

        let old_phase = self.current_phase;

        self.add_point(metrics.latitude, metrics.longitude);

        if metrics.gps_altitude_msl > self.max_alt {
            self.max_alt = metrics.gps_altitude_msl;
        }

        if metrics.ground_speed > self.max_gs {
            self.max_gs = metrics.ground_speed;
        }

        let current_fuel = metrics.fuel_quantity_left + metrics.fuel_quantity_right;
        if self.initial_fuel <= 0.0 && current_fuel > 0.0 {
            self.initial_fuel = current_fuel;
        }
        self.final_fuel = current_fuel;

        let on_ground = metrics.is_on_ground > 0.5;

        // Peak G tracking during landing phase
        if self.current_phase == FlightPhase::Landing && on_ground {
            self.max_landing_g = self.max_landing_g.max(metrics.normal_acceleration);
        }

        // Track Autopilot
        let ap_active = metrics.autopilot_active > 0.5;
        if ap_active != self.last_autopilot_active {
            if ap_active {
                self.add_event("autopilot_on", metrics.latitude, metrics.longitude, timestamp, None, None, None, None);
            } else {
                self.add_event("autopilot_off", metrics.latitude, metrics.longitude, timestamp, None, None, None, None);
            }
            self.last_autopilot_active = ap_active;
        }

        let ground_speed = metrics.ground_speed;
        let ias = metrics.indicated_airspeed;
        let v_spd = metrics.vertical_speed;

        match self.current_phase {
            FlightPhase::Parked => {
                if on_ground && ground_speed > 2.0 {
                    self.current_phase = FlightPhase::TaxiOut;
                } else if !on_ground {
                    self.current_phase = FlightPhase::Climb;
                    self.takeoff_timestamp = Some(timestamp.to_string());
                    self.airborne_start = true;
                }
            }
            FlightPhase::TaxiOut => {
                if on_ground && ias > 45.0 {
                    self.current_phase = FlightPhase::Takeoff;
                } else if !on_ground {
                    self.current_phase = FlightPhase::Climb;
                    self.takeoff_timestamp = Some(timestamp.to_string());
                }
            }
            FlightPhase::Takeoff => {
                if !on_ground {
                    self.current_phase = FlightPhase::Climb;
                    self.takeoff_timestamp = Some(timestamp.to_string());
                    self.add_event("takeoff", metrics.latitude, metrics.longitude, timestamp, None, None, None, None);
                }
            }
            FlightPhase::Climb => {
                if !on_ground {
                    if v_spd.abs() < 200.0 && ias > 60.0 {
                        // Potential TOC
                        let mut trigger_toc = None;
                        if let Some(start_ts) = &self.pending_toc_ts {
                            if let (Some(s), Some(e)) = (self.parse_timestamp(start_ts), self.parse_timestamp(timestamp)) {
                                if e - s >= 10 {
                                    trigger_toc = Some(start_ts.clone());
                                }
                            }
                        } else {
                            self.pending_toc_ts = Some(timestamp.to_string());
                            self.pending_toc_coords = (metrics.latitude, metrics.longitude);
                        }

                        if let Some(start_ts) = trigger_toc {
                            self.current_phase = FlightPhase::Cruise;
                            self.add_event("top_of_climb", self.pending_toc_coords.0, self.pending_toc_coords.1, &start_ts, None, None, None, None);
                            self.pending_toc_ts = None;
                        }
                    } else if v_spd < -400.0 {
                        // Direct Climb -> Descent (skip Cruise)
                        self.current_phase = FlightPhase::Descent;
                        self.add_event("top_of_climb", metrics.latitude, metrics.longitude, timestamp, None, None, None, None);
                        self.add_event("top_of_descent", metrics.latitude, metrics.longitude, timestamp, None, None, None, None);
                        self.pending_toc_ts = None;
                    } else {
                        self.pending_toc_ts = None;
                    }
                } else {
                    self.current_phase = FlightPhase::Landing;
                    self.touchdown_fpm = self.last_v_spd;
                    self.max_landing_g = metrics.normal_acceleration;
                    self.pending_toc_ts = None;
                    self.pending_tod_ts = None;
                    self.add_event("landing", metrics.latitude, metrics.longitude, timestamp, Some(self.last_v_spd), Some(metrics.normal_acceleration), None, None);
                }
            }
            FlightPhase::Cruise => {
                if !on_ground {
                    if v_spd < -500.0 {
                        // Potential TOD
                        let mut trigger_tod = None;
                        if let Some(start_ts) = &self.pending_tod_ts {
                            if let (Some(s), Some(e)) = (self.parse_timestamp(start_ts), self.parse_timestamp(timestamp)) {
                                if e - s >= 10 {
                                    trigger_tod = Some(start_ts.clone());
                                }
                            }
                        } else {
                            self.pending_tod_ts = Some(timestamp.to_string());
                            self.pending_tod_coords = (metrics.latitude, metrics.longitude);
                        }

                        if let Some(start_ts) = trigger_tod {
                            self.current_phase = FlightPhase::Descent;
                            self.add_event("top_of_descent", self.pending_tod_coords.0, self.pending_tod_coords.1, &start_ts, None, None, None, None);
                            self.pending_tod_ts = None;
                        }
                    } else if v_spd > 500.0 {
                        self.current_phase = FlightPhase::Climb;
                        self.pending_tod_ts = None;
                    } else {
                        self.pending_tod_ts = None;
                    }
                } else {
                    self.current_phase = FlightPhase::Landing;
                    self.touchdown_fpm = self.last_v_spd;
                    self.max_landing_g = metrics.normal_acceleration;
                    self.pending_toc_ts = None;
                    self.pending_tod_ts = None;
                    self.add_event("landing", metrics.latitude, metrics.longitude, timestamp, Some(self.last_v_spd), Some(metrics.normal_acceleration), None, None);
                }
            }
            FlightPhase::Descent => {
                if !on_ground {
                    if metrics.gps_altitude_msl < 3000.0 && v_spd < -200.0 {
                        self.current_phase = FlightPhase::Approach;
                    } else if v_spd > 300.0 {
                        self.current_phase = FlightPhase::Climb;
                    } else if v_spd.abs() < 200.0 {
                        self.current_phase = FlightPhase::Cruise;
                    }
                    self.pending_toc_ts = None;
                    self.pending_tod_ts = None;
                } else {
                    self.current_phase = FlightPhase::Landing;
                    self.touchdown_fpm = self.last_v_spd;
                    self.max_landing_g = metrics.normal_acceleration;
                    self.pending_toc_ts = None;
                    self.pending_tod_ts = None;
                    self.add_event("landing", metrics.latitude, metrics.longitude, timestamp, Some(self.last_v_spd), Some(metrics.normal_acceleration), None, None);
                }
            }
            FlightPhase::Approach => {
                if on_ground {
                    self.current_phase = FlightPhase::Landing;
                    self.touchdown_fpm = self.last_v_spd;
                    self.max_landing_g = metrics.normal_acceleration;
                    self.pending_toc_ts = None;
                    self.pending_tod_ts = None;
                    self.add_event("landing", metrics.latitude, metrics.longitude, timestamp, Some(self.last_v_spd), Some(metrics.normal_acceleration), None, None);
                } else if v_spd > 500.0 {
                    self.current_phase = FlightPhase::Climb;
                    self.pending_toc_ts = None;
                    self.pending_tod_ts = None;
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

        self.last_on_ground = on_ground;
        self.last_v_spd = v_spd;

        if self.current_phase != old_phase {
            self.last_phase_change = std::time::Instant::now();
            Some(self.current_phase)
        } else {
            None
        }
    }

    fn add_event(&mut self, event_type: &str, lat: f64, lon: f64, timestamp: &str, fpm: Option<f64>, g: Option<f64>, offset: Option<f64>, dist: Option<f64>) {
        self.events.push(FlightEvent {
            timestamp: timestamp.to_string(),
            event_type: event_type.to_string(),
            latitude: lat,
            longitude: lon,
            touchdown_fpm: fpm,
            landing_g: g,
            offset_percent: offset,
            threshold_dist_ft: dist,
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
        if self.airborne_start {
            return "Airborne".to_string();
        }
        self.find_dominant_icao(&self.start_coords, db)
    }

    pub fn find_end_icao(&self, db: &AirportsDatabase) -> String {
        if !self.last_on_ground && !self.landed && self.current_phase != FlightPhase::Parked && self.current_phase != FlightPhase::TaxiOut {
            return "Airborne".to_string();
        }
        let coords: Vec<(f64, f64)> = self.end_coords.iter().cloned().collect();
        self.find_dominant_icao(&coords, db)
    }

    fn find_dominant_icao(&self, coords: &[(f64, f64)], db: &AirportsDatabase) -> String {
        if coords.is_empty() {
            return "XXXX".to_string();
        }

        let mut counts = HashMap::new();
        for (lat, lon) in coords {
            if let Some(airport) = db.find_nearest(*lat, *lon, 1).first() {
                let code = airport.ident.clone();
                *counts.entry(code).or_insert(0) += 1;
            }
        }

        counts
            .into_iter()
            .max_by_key(|&(_, count)| count)
            .map(|(icao, _)| icao)
            .unwrap_or_else(|| "XXXX".to_string())
    }

    // Refactored to be called by the monitor which has all DBs
    pub fn finalize_landing_performance(&mut self, airports_db: &AirportsDatabase, runways_db: &RunwaysDatabase) {
        let end_icao = self.find_end_icao(airports_db);
        if end_icao == "XXXX" || end_icao == "Airborne" { return; }

        if let Some(idx) = self.events.iter().position(|e| e.event_type == "landing") {
            let lat = self.events[idx].latitude;
            let lon = self.events[idx].longitude;
            
            let runways = runways_db.find_for_ident(&end_icao);
            if let Some((_runway, dist_to_threshold, offset_pct)) = find_best_runway_match(lat, lon, &runways) {
                let landing = &mut self.events[idx];
                landing.threshold_dist_ft = Some(dist_to_threshold);
                landing.offset_percent = Some(offset_pct);
            }
        }
    }
}

fn find_best_runway_match(lat: f64, lon: f64, runways: &[Runway]) -> Option<(Runway, f64, f64)> {
    let mut best_match = None;
    let mut min_dist = f64::MAX;

    for rw in runways {
        // Low end threshold
        if let (Some(le_lat), Some(le_lon), Some(le_hdg)) = (rw.le_latitude_deg, rw.le_longitude_deg, rw.le_heading_degt) {
            let (dist, offset, total_dist) = project_onto_runway(lat, lon, le_lat, le_lon, le_hdg, rw.le_displaced_threshold_ft.unwrap_or(0) as f64);
            if dist.abs() < 200.0 && total_dist > -500.0 && total_dist < (rw.length_ft.unwrap_or(0) as f64 + 500.0) {
                let width = rw.width_ft.unwrap_or(150) as f64;
                let offset_pct = (offset / (width / 2.0)) * 100.0;
                if dist.abs() < min_dist {
                    min_dist = dist.abs();
                    best_match = Some((rw.clone(), total_dist, offset_pct));
                }
            }
        }
        // High end threshold
        if let (Some(he_lat), Some(he_lon), Some(he_hdg)) = (rw.he_latitude_deg, rw.he_longitude_deg, rw.he_heading_degt) {
            let (dist, offset, total_dist) = project_onto_runway(lat, lon, he_lat, he_lon, he_hdg, rw.he_displaced_threshold_ft.unwrap_or(0) as f64);
            if dist.abs() < 200.0 && total_dist > -500.0 && total_dist < (rw.length_ft.unwrap_or(0) as f64 + 500.0) {
                let width = rw.width_ft.unwrap_or(150) as f64;
                let offset_pct = (offset / (width / 2.0)) * 100.0;
                if dist.abs() < min_dist {
                    min_dist = dist.abs();
                    best_match = Some((rw.clone(), total_dist, offset_pct));
                }
            }
        }
    }

    best_match
}

fn project_onto_runway(lat: f64, lon: f64, thr_lat: f64, thr_lon: f64, hdg: f64, displaced: f64) -> (f64, f64, f64) {
    let r = 20925646.3; // Earth radius in feet
    
    let d_lat = (lat - thr_lat).to_radians();
    let d_lon = (lon - thr_lon).to_radians();
    
    // Approximate local flat projection for short distances (runway scale)
    let y = d_lat * r;
    let x = d_lon * r * thr_lat.to_radians().cos();
    
    let angle = (90.0 - hdg).to_radians();
    
    // Rotate coordinates to runway axis
    let run_x = x * angle.cos() + y * angle.sin();
    let run_y = -x * angle.sin() + y * angle.cos();
    
    // run_x is distance along runway axis
    // run_y is lateral offset
    
    (run_y, run_y, run_x - displaced)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_metrics() -> FlightMetrics {
        FlightMetrics {
            latitude: 0.0,
            longitude: 0.0,
            indicated_altitude: 1000.0,
            altimeter_setting: 29.92,
            gps_altitude_msl: 1000.0,
            outside_air_temp: 15.0,
            indicated_airspeed: 0.0,
            ground_speed: 0.0,
            vertical_speed: 0.0,
            pitch_angle: 0.0,
            roll_angle: 0.0,
            lateral_acceleration: 0.0,
            normal_acceleration: 1.0,
            heading: 0.0,
            track: 0.0,
            volts_1: 28.0,
            volts_2: 28.0,
            amps_1: 0.0,
            fuel_quantity_left: 20.0,
            fuel_quantity_right: 20.0,
            engine_1_fuel_flow: 0.0,
            engine_1_oil_temp: 180.0,
            engine_1_oil_pressure: 60.0,
            engine_1_manifold_pressure: 10.0,
            engine_1_rpm: 0.0,
            engine_1_percent_power: 0.0,
            engine_1_cht_1: 0.0,
            engine_1_cht_2: 0.0,
            engine_1_cht_3: 0.0,
            engine_1_cht_4: 0.0,
            engine_1_cht_5: 0.0,
            engine_1_cht_6: 0.0,
            engine_1_egt_1: 0.0,
            engine_1_egt_2: 0.0,
            engine_1_egt_3: 0.0,
            engine_1_egt_4: 0.0,
            engine_1_egt_5: 0.0,
            engine_1_egt_6: 0.0,
            engine_1_tit_1: 0.0,
            engine_1_tit_2: 0.0,
            gps_altitude_wgs84: 1000.0,
            true_airspeed: 0.0,
            hsi_source: 0.0,
            selected_course: 0.0,
            nav_1_frequency: 110.5,
            nav_2_frequency: 110.5,
            com_1_frequency: 121.5,
            com_2_frequency: 121.5,
            horizontal_cdi: 0.0,
            vertical_cdi: 0.0,
            wind_speed: 0.0,
            wind_direction: 0.0,
            waypoint_distance: 0.0,
            waypoint_bearing: 0.0,
            magnetic_variation: 0.0,
            autopilot_active: 0.0,
            roll_mode: 0.0,
            pitch_mode: 0.0,
            roll_command: 0.0,
            pitch_command: 0.0,
            vertical_speed_target: 0.0,
            gps_fix_type: 3.0,
            horizontal_alarm_limit: 0.0,
            vertical_alarm_limit: 0.0,
            horizontal_protection_level_waas: 0.0,
            horizontal_protection_level_fd: 0.0,
            vertical_protection_level_waas: 0.0,
            is_on_ground: 1.0,
            xp_agl: 0.0,
            xp_prop_rpm: 0.0,
            xp_gear_ratio: 0.0,
        }
    }

    #[test]
    fn test_duration_calculation() {
        let mut analyzer = FlightAnalyzer::new();
        analyzer.update(&mock_metrics(), "2026-04-30 12:00:00");
        analyzer.update(&mock_metrics(), "2026-04-30 12:45:30");

        assert_eq!(analyzer.get_duration_minutes(), 45);
    }

    #[test]
    fn test_phase_transitions() {
        let mut analyzer = FlightAnalyzer::new();
        let mut metrics = mock_metrics();

        // Initial state: Parked
        assert_eq!(analyzer.current_phase, FlightPhase::Parked);

        // Taxi Out
        metrics.ground_speed = 5.0;
        analyzer.update(&metrics, "2026-04-30 12:00:01");
        assert_eq!(analyzer.current_phase, FlightPhase::TaxiOut);

        // Takeoff
        metrics.indicated_airspeed = 50.0;
        analyzer.update(&metrics, "2026-04-30 12:00:02");
        assert_eq!(analyzer.current_phase, FlightPhase::Takeoff);

        // Climb
        metrics.is_on_ground = 0.0;
        metrics.vertical_speed = 500.0;
        analyzer.update(&metrics, "2026-04-30 12:00:03");
        assert_eq!(analyzer.current_phase, FlightPhase::Climb);
        assert_eq!(analyzer.events.len(), 1);
        assert_eq!(analyzer.events[0].event_type, "takeoff");

        // Cruise (requires 10s stability)
        metrics.vertical_speed = 50.0;
        metrics.indicated_airspeed = 100.0;
        analyzer.update(&metrics, "2026-04-30 12:00:04");
        assert_eq!(analyzer.current_phase, FlightPhase::Climb); // Still pending
        analyzer.update(&metrics, "2026-04-30 12:00:14");
        assert_eq!(analyzer.current_phase, FlightPhase::Cruise);
        assert_eq!(analyzer.events.len(), 2);
        assert_eq!(analyzer.events[1].event_type, "top_of_climb");

        // Descent (requires 10s stability)
        metrics.vertical_speed = -600.0;
        analyzer.update(&metrics, "2026-04-30 12:00:15");
        assert_eq!(analyzer.current_phase, FlightPhase::Cruise); // Still pending
        analyzer.update(&metrics, "2026-04-30 12:00:25");
        assert_eq!(analyzer.current_phase, FlightPhase::Descent);
        assert_eq!(analyzer.events.len(), 3);
        assert_eq!(analyzer.events[2].event_type, "top_of_descent");

        // Approach
        metrics.gps_altitude_msl = 2500.0;
        metrics.vertical_speed = -300.0;
        analyzer.update(&metrics, "2026-04-30 12:00:26");
        assert_eq!(analyzer.current_phase, FlightPhase::Approach);

        // Landing
        metrics.is_on_ground = 1.0;
        analyzer.update(&metrics, "2026-04-30 12:00:27");
        assert_eq!(analyzer.current_phase, FlightPhase::Landing);
        assert_eq!(analyzer.events.len(), 4);
        assert_eq!(analyzer.events[3].event_type, "landing");

        // Taxi In
        metrics.ground_speed = 10.0;
        analyzer.update(&metrics, "2026-04-30 12:00:28");
        assert_eq!(analyzer.current_phase, FlightPhase::TaxiIn);
    }

    #[test]
    fn test_autopilot_events() {
        let mut analyzer = FlightAnalyzer::new();
        let mut metrics = mock_metrics();

        metrics.autopilot_active = 1.0;
        analyzer.update(&metrics, "2026-04-30 12:00:00");
        assert_eq!(analyzer.events.len(), 1);
        assert_eq!(analyzer.events[0].event_type, "autopilot_on");

        metrics.autopilot_active = 0.0;
        analyzer.update(&metrics, "2026-04-30 12:00:01");
        assert_eq!(analyzer.events.len(), 2);
        assert_eq!(analyzer.events[1].event_type, "autopilot_off");
    }
}
