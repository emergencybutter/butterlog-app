use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Runway {
    pub id: i64,
    pub airport_ref: i64,
    pub airport_ident: String,
    pub length_ft: Option<i32>,
    pub width_ft: Option<i32>,
    pub surface: Option<String>,
    pub lighted: Option<i32>,
    pub closed: Option<i32>,
    pub le_ident: Option<String>,
    pub le_latitude_deg: Option<f64>,
    pub le_longitude_deg: Option<f64>,
    pub le_elevation_ft: Option<i32>,
    #[serde(rename = "le_heading_degT")]
    pub le_heading_degt: Option<f64>,
    pub le_displaced_threshold_ft: Option<i32>,
    pub he_ident: Option<String>,
    pub he_latitude_deg: Option<f64>,
    pub he_longitude_deg: Option<f64>,
    pub he_elevation_ft: Option<i32>,
    #[serde(rename = "he_heading_degT")]
    pub he_heading_degt: Option<f64>,
    pub he_displaced_threshold_ft: Option<i32>,
}

pub struct RunwaysDatabase {
    pub runways: Vec<Runway>,
    pub by_airport_ident: HashMap<String, Vec<usize>>,
}

impl RunwaysDatabase {
    /// Loads the runways CSV and prints parsing stats.
    pub fn load_from_csv<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let start_time = Instant::now();
        let file = File::open(path)?;
        let mut rdr = csv::ReaderBuilder::new().from_reader(file);

        let mut runways = Vec::new();
        let mut by_airport_ident: HashMap<String, Vec<usize>> = HashMap::new();

        for result in rdr.deserialize() {
            let runway: Runway = result?;
            by_airport_ident
                .entry(runway.airport_ident.clone())
                .or_default()
                .push(runways.len());
            runways.push(runway);
        }

        println!(
            "RunwaysDatabase: Loaded {} runways in {:?}",
            runways.len(),
            start_time.elapsed()
        );

        Ok(RunwaysDatabase {
            runways,
            by_airport_ident,
        })
    }

    pub fn find_for_ident(&self, ident: &str) -> Vec<Runway> {
        if let Some(indices) = self.by_airport_ident.get(ident) {
            indices.iter().map(|&i| self.runways[i].clone()).collect()
        } else {
            Vec::new()
        }
    }
}
