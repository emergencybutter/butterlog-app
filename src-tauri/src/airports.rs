use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::time::Instant;
use rstar::{Point, RTree};

#[derive(Debug, Deserialize, Serialize, Clone)]
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

/// A wrapper for an airport's location to be used with rstar.
/// It stores the 3D cartesian coordinates and the index of the airport in the main Vec.
#[derive(Debug, Clone, Copy)]
struct AirportLocation {
    index: usize,
    coords: [f64; 3],
}

impl Point for AirportLocation {
    type Scalar = f64;
    const DIMENSIONS: usize = 3;

    fn generate(generator: impl Fn(usize) -> Self::Scalar) -> Self {
        AirportLocation {
            index: 0, // This won't be used directly by rstar's generation logic for our use case.
            coords: [generator(0), generator(1), generator(2)],
        }
    }

    fn nth(&self, index: usize) -> Self::Scalar {
        self.coords[index]
    }
}

/// Converts latitude and longitude (in degrees) to 3D Cartesian coordinates on a unit sphere.
fn lat_lon_to_cartesian(lat_deg: f64, lon_deg: f64) -> [f64; 3] {
    let lat_rad = lat_deg.to_radians();
    let lon_rad = lon_deg.to_radians();
    let x = lat_rad.cos() * lon_rad.cos();
    let y = lat_rad.cos() * lon_rad.sin();
    let z = lat_rad.sin();
    [x, y, z]
}

pub struct AirportsDatabase {
    pub airports: Vec<Airport>,
    pub by_ident: HashMap<String, usize>,
    tree: RTree<AirportLocation>,
}

impl AirportsDatabase {
    /// Loads the airports CSV and prints parsing stats.
    pub fn load_from_csv<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let start_time = Instant::now();
        let file = File::open(path)?;
        let mut rdr = csv::ReaderBuilder::new().from_reader(file);
        
        let mut airports = Vec::new();
        let mut by_ident = HashMap::new();
        let mut airport_locations = Vec::new();

        for (index, result) in rdr.deserialize().enumerate() {
            let airport: Airport = result?;

            // If the airport has valid coordinates, add it to the list for spatial indexing.
            if let (Some(lat), Some(lon)) = (airport.latitude_deg, airport.longitude_deg) {
                airport_locations.push(AirportLocation {
                    index,
                    coords: lat_lon_to_cartesian(lat, lon),
                });
            }

            by_ident.insert(airport.ident.clone(), index);
            airports.push(airport);
        }

        // Bulk load the locations into the R-Tree for optimal performance.
        let tree = RTree::bulk_load(airport_locations);

        println!("AirportsDatabase: Loaded {} airports and built spatial index in {:?}", airports.len(), start_time.elapsed());

        Ok(AirportsDatabase { airports, by_ident, tree })
    }

    /// Fetch an airport by its identifier (e.g., ICAO code, local code)
    pub fn get_by_ident(&self, ident: &str) -> Option<&Airport> {
        self.by_ident.get(ident).map(|&idx| &self.airports[idx])
    }

    /// Finds the `count` nearest airports to a given latitude and longitude.
    pub fn find_nearest(&self, lat: f64, lon: f64, count: usize) -> Vec<&Airport> {
        let search_point = lat_lon_to_cartesian(lat, lon);
        self.tree.nearest_neighbor_iter(&search_point)
            .take(count)
            .map(|loc| &self.airports[loc.index])
            .collect()
    }
}