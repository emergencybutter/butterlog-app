use std::collections::{VecDeque, HashMap};
use crate::airports::AirportsDatabase;

pub struct FlightAnalyzer {
    start_coords: Vec<(f64, f64)>,
    end_coords: VecDeque<(f64, f64)>,
}

impl FlightAnalyzer {
    pub fn new() -> Self {
        Self {
            start_coords: Vec::with_capacity(300), // 5 minutes at 1Hz
            end_coords: VecDeque::with_capacity(300),
        }
    }

    pub fn reset(&mut self) {
        self.start_coords.clear();
        self.end_coords.clear();
    }

    pub fn add_point(&mut self, lat: f64, lon: f64) {
        if self.start_coords.len() < 300 {
            self.start_coords.push((lat, lon));
        }
        
        if self.end_coords.len() >= 300 {
            self.end_coords.pop_front();
        }
        self.end_coords.push_back((lat, lon));
    }

    pub fn find_start_icao(&self, db: &AirportsDatabase) -> String {
        self.find_most_frequent_closest_icao(db, &self.start_coords)
    }

    pub fn find_end_icao(&self, db: &AirportsDatabase) -> String {
        let coords: Vec<(f64, f64)> = self.end_coords.iter().cloned().collect();
        self.find_most_frequent_closest_icao(db, &coords)
    }

    fn find_most_frequent_closest_icao(&self, db: &AirportsDatabase, coords: &[(f64, f64)]) -> String {
        if coords.is_empty() {
            return "XXXX".to_string();
        }
        
        let mut counts = HashMap::new();
        for &(lat, lon) in coords {
            let nearest = db.find_nearest(lat, lon, 1);
            if let Some(airport) = nearest.first() {
                // Use icao_code if not empty, otherwise fallback to ident
                let code = if !airport.icao_code.is_empty() {
                    airport.icao_code.clone()
                } else {
                    airport.ident.clone()
                };
                *counts.entry(code).or_insert(0) += 1;
            }
        }
        
        counts.into_iter()
            .max_by_key(|&(_, count)| count)
            .map(|(icao, _)| icao)
            .unwrap_or_else(|| "XXXX".to_string())
    }
}
