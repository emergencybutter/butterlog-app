//! The runways MSFS draws around a touchdown (`runways.rs`), from SimConnect's facilities:
//! the airports loaded around the aircraft, the nearest one's runways, then each runway's
//! thresholds. Each step answers in messages on the monitor's connection, which pass
//! through [`RunwayLookup::on_message`].

use super::runways::{distance_m, offset};
use butterlog_wire::{RunwayEnd, SimRunway, SimRunways};
use simplesimconnect::{
    DispatchMessage, SimConnect, DWORD, FACILITY_DATA_PAVEMENT, FACILITY_DATA_RUNWAY,
};
use std::time::{Duration, Instant};

/// Ids on the monitor's connection, clear of the monitor's and the sampler's.
pub const DEFINE_ID: DWORD = 21;
const LIST_REQUEST_ID: DWORD = 21;
const DATA_REQUEST_ID: DWORD = 22;

/// The nearest airport loaded must be within this of the touchdown.
const NEAR_M: f64 = 5_000.0;
/// A lookup that hasn't finished by then is dropped.
const TIMEOUT: Duration = Duration::from_secs(20);
const FT_PER_M: f64 = 3.280_84;

/// The facility definition: the airport, each runway's centre, heading, size and ends,
/// and the length of each end's displaced threshold. A runway's fields come packed in this
/// order: f64 latitude, f64 longitude, f32 heading, f32 length, f32 width, then i32 primary
/// number and designator, secondary number and designator. A threshold's: f32 length.
const FIELDS: &[&str] = &[
    "OPEN AIRPORT",
    "LATITUDE",
    "LONGITUDE",
    "OPEN RUNWAY",
    "LATITUDE",
    "LONGITUDE",
    "HEADING",
    "LENGTH",
    "WIDTH",
    "PRIMARY_NUMBER",
    "PRIMARY_DESIGNATOR",
    "SECONDARY_NUMBER",
    "SECONDARY_DESIGNATOR",
    "OPEN PRIMARY_THRESHOLD",
    "LENGTH",
    "CLOSE PRIMARY_THRESHOLD",
    "OPEN SECONDARY_THRESHOLD",
    "LENGTH",
    "CLOSE SECONDARY_THRESHOLD",
    "CLOSE RUNWAY",
    "CLOSE AIRPORT",
];

pub fn register(sc: &SimConnect) -> anyhow::Result<()> {
    for field in FIELDS {
        sc.add_to_facility_definition(DEFINE_ID, field)?;
    }
    Ok(())
}

/// A runway as the facility sends it, metres and degrees.
#[derive(Debug, Clone, PartialEq)]
struct Runway {
    unique_id: DWORD,
    centre: (f64, f64),
    heading: f64,
    length_m: f64,
    width_m: f64,
    primary: String,
    secondary: String,
    /// Displaced threshold lengths, primary then secondary, as they come.
    thresholds: Vec<f64>,
}

fn read<const N: usize>(data: &[u8], at: usize) -> Option<[u8; N]> {
    data.get(at..at + N)?.try_into().ok()
}

/// "09", "27L", "N"... from a facility's runway number and designator.
fn ident(number: i32, designator: i32) -> String {
    const POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    let base = match number {
        1..=36 => format!("{number:02}"),
        37..=44 => POINTS[(number - 37) as usize].to_string(),
        _ => "-".to_string(),
    };
    let suffix = match designator {
        1 => "L",
        2 => "R",
        3 => "C",
        4 => "W",
        5 => "A",
        6 => "B",
        _ => "",
    };
    format!("{base}{suffix}")
}

fn parse_runway(unique_id: DWORD, data: &[u8]) -> Option<Runway> {
    let f64_at = |at| read::<8>(data, at).map(f64::from_le_bytes);
    let f32_at = |at| read::<4>(data, at).map(|b| f32::from_le_bytes(b) as f64);
    let i32_at = |at| read::<4>(data, at).map(i32::from_le_bytes);
    Some(Runway {
        unique_id,
        centre: (f64_at(0)?, f64_at(8)?),
        heading: f32_at(16)?,
        length_m: f32_at(20)?,
        width_m: f32_at(24)?,
        primary: ident(i32_at(28)?, i32_at(32)?),
        secondary: ident(i32_at(36)?, i32_at(40)?),
        thresholds: Vec::new(),
    })
}

impl Runway {
    /// Its ends: the primary's threshold half its length back from the centre, pointing
    /// along its heading; the secondary's the other way.
    fn sim_runway(&self) -> SimRunway {
        let half = self.length_m / 2.0;
        let primary = offset(self.centre, self.heading + 180.0, half);
        let secondary = offset(self.centre, self.heading, half);
        let displaced = |i: usize| self.thresholds.get(i).copied().unwrap_or(0.0).max(0.0) * FT_PER_M;
        SimRunway {
            width_ft: self.width_m * FT_PER_M,
            length_ft: self.length_m * FT_PER_M,
            ends: [
                RunwayEnd {
                    ident: self.primary.clone(),
                    latitude: primary.0,
                    longitude: primary.1,
                    heading: self.heading.rem_euclid(360.0),
                    displaced_ft: displaced(0),
                },
                RunwayEnd {
                    ident: self.secondary.clone(),
                    latitude: secondary.0,
                    longitude: secondary.1,
                    heading: (self.heading + 180.0).rem_euclid(360.0),
                    displaced_ft: displaced(1),
                },
            ],
        }
    }
}

enum Step {
    /// Collecting the loaded airports: the nearest so far, with its distance.
    Listing { at: (f64, f64), nearest: Option<(f64, String, String)> },
    Fetching { airport: String, runways: Vec<Runway> },
}

/// One lookup at a time; a touchdown during one is skipped.
#[derive(Default)]
pub struct RunwayLookup {
    step: Option<(Step, Instant)>,
}

impl RunwayLookup {
    /// Starts looking up the runways around a touchdown.
    pub fn start(&mut self, sc: &SimConnect, at: (f64, f64)) -> anyhow::Result<()> {
        if self.step.as_ref().is_some_and(|(_, t)| t.elapsed() < TIMEOUT) {
            return Ok(());
        }
        self.step = None;
        sc.request_airport_list(LIST_REQUEST_ID)?;
        self.step = Some((Step::Listing { at, nearest: None }, Instant::now()));
        Ok(())
    }

    /// A message from the connection: the runways, once they are all in. `Err` when the
    /// lookup fails; either way it is over.
    pub fn on_message(&mut self, sc: &SimConnect, msg: &DispatchMessage) -> Option<Result<SimRunways, String>> {
        let (step, _) = self.step.as_mut()?;
        match step {
            Step::Listing { at, nearest } => {
                let list = msg.as_airport_list().filter(|l| l.request_id == LIST_REQUEST_ID)?;
                for a in list.airports {
                    let d = distance_m(*at, (a.latitude, a.longitude));
                    if nearest.as_ref().is_none_or(|(n, ..)| d < *n) {
                        *nearest = Some((d, a.ident, a.region));
                    }
                }
                if list.entry + 1 < list.out_of {
                    return None;
                }
                let found = nearest.take().filter(|(d, ..)| *d <= NEAR_M);
                let Some((_, ident, region)) = found else {
                    self.step = None;
                    return Some(Err("no airport loaded near the touchdown".into()));
                };
                if let Err(e) = sc.request_facility_data(DEFINE_ID, DATA_REQUEST_ID, &ident, &region) {
                    self.step = None;
                    return Some(Err(e.to_string()));
                }
                *step = Step::Fetching { airport: ident, runways: Vec::new() };
                None
            }
            Step::Fetching { airport, runways } => {
                if let Some(data) = msg.as_facility_data().filter(|d| d.request_id == DATA_REQUEST_ID) {
                    match data.kind {
                        FACILITY_DATA_RUNWAY => runways.extend(parse_runway(data.unique_id, data.data)),
                        FACILITY_DATA_PAVEMENT => {
                            let length = read::<4>(data.data, 0).map(|b| f32::from_le_bytes(b) as f64);
                            if let (Some(r), Some(length)) =
                                (runways.iter_mut().find(|r| r.unique_id == data.parent_id), length)
                            {
                                r.thresholds.push(length);
                            }
                        }
                        _ => {}
                    }
                    return None;
                }
                if msg.as_facility_data_end() != Some(DATA_REQUEST_ID) {
                    return None;
                }
                let found = SimRunways {
                    airport: airport.clone(),
                    runways: runways.iter().filter(|r| r.length_m > 0.0).map(Runway::sim_runway).collect(),
                };
                self.step = None;
                Some(if found.runways.is_empty() { Err(format!("no runways at {}", found.airport)) } else { Ok(found) })
            }
        }
    }

    /// A lookup that never finished, such as one the sim answered with an exception, is
    /// dropped so the next touchdown can look again.
    pub fn expire(&mut self) {
        if self.step.as_ref().is_some_and(|(_, t)| t.elapsed() >= TIMEOUT) {
            self.step = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::runways::{bearing, distance_m};

    #[test]
    fn runway_idents_read_as_painted() {
        assert_eq!(ident(2, 0), "02");
        assert_eq!(ident(27, 1), "27L");
        assert_eq!(ident(9, 3), "09C");
        assert_eq!(ident(37, 0), "N");
        assert_eq!(ident(0, 0), "-");
    }

    #[test]
    fn a_runway_is_read_in_the_definitions_order_and_its_ends_placed() {
        let mut data = Vec::new();
        data.extend(20.963_f64.to_le_bytes());
        data.extend((-156.673_f64).to_le_bytes());
        for v in [28.0_f32, 914.0, 30.0] {
            data.extend(v.to_le_bytes());
        }
        for v in [2_i32, 0, 20, 0] {
            data.extend(v.to_le_bytes());
        }
        let mut r = parse_runway(7, &data).unwrap();
        assert_eq!((r.primary.as_str(), r.secondary.as_str()), ("02", "20"));
        assert!(parse_runway(7, &data[..40]).is_none());
        r.thresholds = vec![0.0, 30.0];
        let s = r.sim_runway();
        let (a, b) = ((s.ends[0].latitude, s.ends[0].longitude), (s.ends[1].latitude, s.ends[1].longitude));
        assert!((distance_m(a, b) - 914.0).abs() < 1.0);
        assert!((bearing(a, b) - 28.0).abs() < 0.1);
        assert!((s.ends[1].heading - 208.0).abs() < 1e-6);
        assert!((s.ends[1].displaced_ft - 98.4).abs() < 0.1);
        assert!((s.width_ft - 98.4).abs() < 0.1);
    }
}
