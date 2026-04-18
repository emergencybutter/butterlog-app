use serde::Deserialize;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Deserialize, Clone)]
pub struct Airport {
    pub id: i64,
    pub ident: String,
    // `type` is a reserved keyword in Rust, so we rename it during deserialization
    #[serde(rename = "type")]
    pub airport_type: String,
    pub name: String,
    pub latitude_deg: Option<f64>,
    pub longitude_deg: Option<f64>,
    pub elevation_ft: Option<i32>,
    pub continent: String,
    pub iso_country: String,
    pub iso_region: String,
    pub municipality: String,
    pub scheduled_service: String,
    pub gps_code: String,
    pub icao_code: String,
    pub iata_code: String,
    pub local_code: String,
    pub home_link: String,
    pub wikipedia_link: String,
    pub keywords: String,
}

pub struct AirportsDatabase {
    pub airports: Vec<Airport>,
    pub by_ident: HashMap<String, usize>,
}

impl AirportsDatabase {
    /// Loads the airports CSV and prints parsing stats.
    pub fn load_from_csv<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let start_time = Instant::now();
        let file = File::open(path)?;
        let mut rdr = csv::ReaderBuilder::new().from_reader(file);
        
        let mut airports = Vec::new();
        let mut by_ident = HashMap::new();

        for result in rdr.deserialize() {
            let airport: Airport = result?;
            by_ident.insert(airport.ident.clone(), airports.len());
            airports.push(airport);
        }

        println!("AirportsDatabase: Loaded {} airports in {:?}", airports.len(), start_time.elapsed());

        Ok(AirportsDatabase { airports, by_ident })
    }

    /// Fetch an airport by its identifier (e.g., ICAO code, local code)
    pub fn get_by_ident(&self, ident: &str) -> Option<&Airport> {
        self.by_ident.get(ident).map(|&idx| &self.airports[idx])
    }
}