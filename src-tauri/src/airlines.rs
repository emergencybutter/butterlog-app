//! Loader for `airlines.csv` (`icao,airline,callsign`) — the airline reference table
//! used to identify an operator from a free string (sim title / livery).

use std::error::Error;
use std::fs::File;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Airline {
    pub icao: String,
    pub name: String,
    pub callsign: String,
}

pub struct AirlinesDatabase {
    pub airlines: Vec<Airline>,
}

impl AirlinesDatabase {
    pub fn load_from_csv<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let file = File::open(path)?;
        let mut reader = csv::Reader::from_reader(file);
        let headers = reader.headers()?.clone();

        let icao_idx = headers.iter().position(|h| h == "icao");
        let name_idx = headers.iter().position(|h| h == "airline");
        let callsign_idx = headers.iter().position(|h| h == "callsign");

        let mut airlines = Vec::new();
        if let (Some(ii), Some(ni), Some(ci)) = (icao_idx, name_idx, callsign_idx) {
            for record in reader.records().flatten() {
                let icao = record.get(ii).unwrap_or("").trim().to_uppercase();
                let name = record.get(ni).unwrap_or("").trim().to_string();
                let callsign = record.get(ci).unwrap_or("").trim().to_string();
                if !icao.is_empty() && !name.is_empty() {
                    airlines.push(Airline { icao, name, callsign });
                }
            }
        }
        Ok(Self { airlines })
    }
}
