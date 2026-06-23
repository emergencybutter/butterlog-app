use rstar::{PointDistance, RTree, RTreeObject, AABB};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::time::Instant;

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
#[derive(Debug, Clone, Copy, PartialEq)]
struct AirportLocation {
    index: usize,
    coords: [f64; 3],
}

impl RTreeObject for AirportLocation {
    type Envelope = AABB<[f64; 3]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_point(self.coords)
    }
}

impl PointDistance for AirportLocation {
    fn distance_2(&self, point: &[f64; 3]) -> f64 {
        let d0 = self.coords[0] - point[0];
        let d1 = self.coords[1] - point[1];
        let d2 = self.coords[2] - point[2];
        d0 * d0 + d1 * d1 + d2 * d2
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

/// When the closest airport to a fix is an uncoded placeholder, prefer a properly
/// coded airport within this distance instead of the placeholder.
const PREFER_CODED_WITHIN_KM: f64 = 10.0;

/// OurAirports gives minor fields with no official ICAO/IATA/local code a synthetic
/// `{ISO country}-{n}` ident (e.g. "RS-0011"). Real airport idents — ICAO codes like
/// "LYBE" or local codes — never have that 2-letter + hyphen shape, so this flags the
/// placeholders we should avoid surfacing as a flight's departure/arrival.
fn is_placeholder_ident(ident: &str) -> bool {
    let b = ident.as_bytes();
    b.len() >= 4
        && b[0].is_ascii_uppercase()
        && b[1].is_ascii_uppercase()
        && b[2] == b'-'
        && b[3..].iter().all(|c| c.is_ascii_alphanumeric())
}

/// Great-circle distance between two lat/lon points, in kilometers.
fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6371.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * a.sqrt().asin()
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

            // If the airport has valid coordinates and is NOT closed, add it to the list for spatial indexing.
            if let (Some(lat), Some(lon)) = (airport.latitude_deg, airport.longitude_deg) {
                if airport.airport_type != "closed" && airport.airport_type != "heliport" && airport.airport_type != "seaplane_base" {
                    airport_locations.push(AirportLocation {
                        index,
                        coords: lat_lon_to_cartesian(lat, lon),
                    });
                }
            }

            by_ident.insert(airport.ident.clone(), index);
            airports.push(airport);
        }

        // Bulk load the locations into the R-Tree for optimal performance.
        let tree = RTree::bulk_load(airport_locations);

        println!(
            "AirportsDatabase: Loaded {} airports and built spatial index in {:?}",
            airports.len(),
            start_time.elapsed()
        );

        Ok(AirportsDatabase {
            airports,
            by_ident,
            tree,
        })
    }

    /// Fetch an airport by its identifier (e.g., ICAO code, local code)
    pub fn get_by_ident(&self, ident: &str) -> Option<&Airport> {
        self.by_ident.get(ident).map(|&idx| &self.airports[idx])
    }

    /// Finds the `count` nearest airports to a given latitude and longitude.
    pub fn find_nearest(&self, lat: f64, lon: f64, count: usize) -> Vec<&Airport> {
        let search_point = lat_lon_to_cartesian(lat, lon);
        self.tree
            .nearest_neighbor_iter(&search_point)
            .take(count)
            .map(|loc| &self.airports[loc.index])
            .collect()
    }

    /// Resolve the airport at a position for *naming a flight's endpoint*. Like
    /// [`Self::find_nearest`] with `count == 1`, except when the closest airport is an
    /// uncoded placeholder field (e.g. "RS-0011") it prefers the nearest properly coded
    /// airport within [`PREFER_CODED_WITHIN_KM`] (e.g. "LYBE"). This stops a co-located
    /// minor airstrip from masking the real airport a flight actually departed/arrived at.
    pub fn find_best_airport(&self, lat: f64, lon: f64) -> Option<&Airport> {
        let candidates = self.find_nearest(lat, lon, 10);
        let closest = *candidates.first()?;
        if !is_placeholder_ident(&closest.ident) {
            return Some(closest);
        }
        if let Some(coded) = candidates.iter().copied().find(|a| !is_placeholder_ident(&a.ident)) {
            if let (Some(clat), Some(clon)) = (coded.latitude_deg, coded.longitude_deg) {
                if haversine_km(lat, lon, clat, clon) <= PREFER_CODED_WITHIN_KM {
                    return Some(coded);
                }
            }
        }
        Some(closest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to quickly create a mock airport for testing
    fn mock_airport(id: i64, ident: &str, lat: f64, lon: f64) -> Airport {
        Airport {
            id,
            ident: ident.to_string(),
            airport_type: "small_airport".to_string(),
            name: format!("Airport {}", ident),
            latitude_deg: Some(lat),
            longitude_deg: Some(lon),
            elevation_ft: Some(0),
            continent: "EU".to_string(),
            iso_country: "GB".to_string(),
            iso_region: "GB-ENG".to_string(),
            municipality: "London".to_string(),
            scheduled_service: "no".to_string(),
            gps_code: ident.to_string(),
            icao_code: ident.to_string(),
            iata_code: "".to_string(),
            local_code: "".to_string(),
            home_link: "".to_string(),
            wikipedia_link: "".to_string(),
            keywords: "".to_string(),
        }
    }

    /// Helper function to create an in-memory database with some mock data
    fn create_mock_db() -> AirportsDatabase {
        let airports = vec![
            mock_airport(1, "A1", 0.0, 0.0),
            mock_airport(2, "A2", 10.0, 0.0),
            mock_airport(3, "A3", 0.0, 10.0),
        ];

        let mut by_ident = HashMap::new();
        let mut airport_locations = Vec::new();

        for (index, airport) in airports.iter().enumerate() {
            by_ident.insert(airport.ident.clone(), index);
            if let (Some(lat), Some(lon)) = (airport.latitude_deg, airport.longitude_deg) {
                airport_locations.push(AirportLocation {
                    index,
                    coords: lat_lon_to_cartesian(lat, lon),
                });
            }
        }

        let tree = RTree::bulk_load(airport_locations);
        AirportsDatabase {
            airports,
            by_ident,
            tree,
        }
    }

    #[test]
    fn test_lat_lon_to_cartesian() {
        // Equator / Prime Meridian
        let [x, y, z] = lat_lon_to_cartesian(0.0, 0.0);
        assert!((x - 1.0).abs() < 1e-9);
        assert!((y - 0.0).abs() < 1e-9);
        assert!((z - 0.0).abs() < 1e-9);

        // North Pole
        let [x2, y2, z2] = lat_lon_to_cartesian(90.0, 0.0);
        assert!((x2 - 0.0).abs() < 1e-9);
        assert!((y2 - 0.0).abs() < 1e-9);
        assert!((z2 - 1.0).abs() < 1e-9);

        // Equator / 90 degrees East
        let [x3, y3, z3] = lat_lon_to_cartesian(0.0, 90.0);
        assert!((x3 - 0.0).abs() < 1e-9);
        assert!((y3 - 1.0).abs() < 1e-9);
        assert!((z3 - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_get_by_ident() {
        let db = create_mock_db();

        assert!(db.get_by_ident("A1").is_some());
        assert_eq!(db.get_by_ident("A1").unwrap().name, "Airport A1");

        assert!(db.get_by_ident("UNKNOWN").is_none());
    }

    #[test]
    fn test_find_nearest() {
        let db = create_mock_db();

        // Search near 9.0, 0.0, which should be closest to A2 (10.0, 0.0)
        let nearest = db.find_nearest(9.0, 0.0, 1);
        assert_eq!(nearest.len(), 1);
        assert_eq!(nearest[0].ident, "A2");

        // Requesting 2 nearest from 1.0, 0.0 -> Should return A1 then A2
        let nearest_two = db.find_nearest(1.0, 0.0, 2);
        assert_eq!(nearest_two.len(), 2);
        assert_eq!(nearest_two[0].ident, "A1");
        assert_eq!(nearest_two[1].ident, "A2");
    }

    #[test]
    fn test_is_placeholder_ident() {
        assert!(is_placeholder_ident("RS-0011"));
        assert!(is_placeholder_ident("US-1234"));
        assert!(is_placeholder_ident("GB-0AB1"));
        assert!(!is_placeholder_ident("LYBE"));
        assert!(!is_placeholder_ident("KJFK"));
        assert!(!is_placeholder_ident("X51"));
        assert!(!is_placeholder_ident("1A2"));
    }

    fn db_from(airports: Vec<Airport>) -> AirportsDatabase {
        let mut by_ident = HashMap::new();
        let mut locs = Vec::new();
        for (i, a) in airports.iter().enumerate() {
            by_ident.insert(a.ident.clone(), i);
            if let (Some(lat), Some(lon)) = (a.latitude_deg, a.longitude_deg) {
                locs.push(AirportLocation { index: i, coords: lat_lon_to_cartesian(lat, lon) });
            }
        }
        AirportsDatabase { airports, by_ident, tree: RTree::bulk_load(locs) }
    }

    #[test]
    fn find_best_airport_prefers_coded_over_colocated_placeholder() {
        // Belgrade Nikola Tesla (LYBE, coded) with the co-located "Surčin Airstrip"
        // placeholder (RS-0011) ~5.5 km south — the real-world case this fixes.
        let db = db_from(vec![
            mock_airport(1, "LYBE", 44.8184, 20.3091),
            mock_airport(2, "RS-0011", 44.7689, 20.3047),
        ]);
        // The raw nearest at the airstrip is the placeholder...
        assert_eq!(db.find_nearest(44.7689, 20.3047, 1)[0].ident, "RS-0011");
        // ...but endpoint resolution prefers the nearby real airport.
        assert_eq!(db.find_best_airport(44.7689, 20.3047).unwrap().ident, "LYBE");
    }

    #[test]
    fn find_best_airport_keeps_placeholder_when_no_coded_airport_is_near() {
        // A placeholder strip with the only coded airport ~450 km away stays itself.
        let db = db_from(vec![
            mock_airport(1, "RS-0011", 10.0, 10.0),
            mock_airport(2, "LYBE", 13.0, 13.0),
        ]);
        assert_eq!(db.find_best_airport(10.0, 10.0).unwrap().ident, "RS-0011");
    }
}
