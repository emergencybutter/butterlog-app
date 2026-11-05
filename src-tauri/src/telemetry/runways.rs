//! The runways the aircraft touched down among, as the sim draws them, sent in the
//! session as an [`EventKind::Runways`](butterlog_wire::EventKind::Runways) event. The
//! service measures the touchdown against them: a public database's thresholds can sit
//! tens of feet from the sim's scenery, which reads as landing off the centreline.
//!
//! [`TouchdownWatch`] notices the touchdown from the recorded channels; each sim finds its
//! runways its own way: MSFS through SimConnect's facilities (`msfs_runways.rs`), X-Plane
//! from the `apt.dat` files of its scenery, here.

use butterlog_wire::{RunwayEnd, SimRunway, SimRunways};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Airborne at least this long before a touchdown counts, so a bounce or a ground
/// flicker does not.
const AIRBORNE_MS: i64 = 10_000;
/// At most one lookup this often.
const LOOKUP_EVERY_MS: i64 = 60_000;
/// Airports with a runway end within this of the touchdown are looked at.
const NEAR_M: f64 = 5_000.0;
const FT_PER_M: f64 = 3.280_84;
const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Notices touchdowns from the channels as they are recorded.
pub struct TouchdownWatch {
    on_ground: u16,
    latitude: u16,
    longitude: u16,
    position: (Option<f64>, Option<f64>),
    ground: Option<bool>,
    airborne_since: Option<i64>,
    last_lookup: Option<i64>,
    touchdown: Option<(f64, f64)>,
}

impl TouchdownWatch {
    /// The channel ids of the on-ground flag and the position, in degrees.
    pub fn new(on_ground: u16, latitude: u16, longitude: u16) -> Self {
        Self {
            on_ground,
            latitude,
            longitude,
            position: (None, None),
            ground: None,
            airborne_since: None,
            last_lookup: None,
            touchdown: None,
        }
    }

    pub fn set(&mut self, channel: u16, value: f64, now_ms: i64) {
        if channel == self.latitude {
            self.position.0 = Some(value);
        } else if channel == self.longitude {
            self.position.1 = Some(value);
        } else if channel == self.on_ground {
            let ground = value > 0.5;
            match (self.ground, ground) {
                (Some(false), true) => {
                    let flew = self.airborne_since.is_some_and(|t| now_ms - t >= AIRBORNE_MS);
                    let due = self.last_lookup.is_none_or(|t| now_ms - t >= LOOKUP_EVERY_MS);
                    if let (true, true, (Some(lat), Some(lon))) = (flew, due, self.position) {
                        self.touchdown = Some((lat, lon));
                        self.last_lookup = Some(now_ms);
                    }
                }
                (Some(false), false) => {}
                (_, false) => self.airborne_since = Some(now_ms),
                (_, true) => {}
            }
            self.ground = Some(ground);
        }
    }

    /// Where the aircraft touched down, once per touchdown.
    pub fn take(&mut self) -> Option<(f64, f64)> {
        self.touchdown.take()
    }
}

/// Metres from one point to another, degrees.
pub fn distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.0.to_radians(), b.0.to_radians());
    let dla = la2 - la1;
    let dlo = (b.1 - a.1).to_radians();
    let h = (dla / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().asin()
}

/// True bearing from one point to another, degrees.
pub fn bearing(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.0.to_radians(), b.0.to_radians());
    let dlo = (b.1 - a.1).to_radians();
    let y = dlo.sin() * la2.cos();
    let x = la1.cos() * la2.sin() - la1.sin() * la2.cos() * dlo.cos();
    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

/// The point `metres` from `from` on a true `bearing`, flat-earth at runway scale.
pub fn offset(from: (f64, f64), bearing: f64, metres: f64) -> (f64, f64) {
    let b = bearing.to_radians();
    let dlat = metres * b.cos() / EARTH_RADIUS_M;
    let dlon = metres * b.sin() / (EARTH_RADIUS_M * from.0.to_radians().cos());
    (from.0 + dlat.to_degrees(), from.1 + dlon.to_degrees())
}

/// A runway from its two ends as X-Plane gives them: ident, position, displaced threshold
/// in metres.
fn runway_from_ends(width_m: f64, a: (&str, f64, f64, f64), b: (&str, f64, f64, f64)) -> SimRunway {
    let (pa, pb) = ((a.1, a.2), (b.1, b.2));
    let end = |e: (&str, f64, f64, f64), heading: f64| RunwayEnd {
        ident: e.0.to_string(),
        latitude: e.1,
        longitude: e.2,
        heading,
        displaced_ft: e.3 * FT_PER_M,
    };
    SimRunway {
        width_ft: width_m * FT_PER_M,
        length_ft: distance_m(pa, pb) * FT_PER_M,
        ends: [end(a, bearing(pa, pb)), end(b, bearing(pb, pa))],
    }
}

/// The land runways (row 100) of the airports in an `apt.dat` that have an end within
/// [`NEAR_M`] of `at`: (airport ident, its runways), in file order.
pub fn near_in_apt_dat(reader: impl BufRead, at: (f64, f64)) -> Vec<SimRunways> {
    let mut found: Vec<SimRunways> = Vec::new();
    let mut airport: Option<String> = None;
    let mut taken = false;
    // A runway end can be no further than this, in degrees of latitude.
    let near_deg = NEAR_M / EARTH_RADIUS_M * 180.0 / std::f64::consts::PI;
    for line in reader.split(b'\n').map_while(Result::ok) {
        let line = String::from_utf8_lossy(&line);
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("1" | "16" | "17") => {
                airport = fields.nth(3).map(str::to_string);
                taken = false;
            }
            Some("100") => {
                let f: Vec<&str> = fields.collect();
                if f.len() < 20 {
                    continue;
                }
                let num = |i: usize| f[i].parse::<f64>().ok();
                let (Some(width), Some(la), Some(lo), Some(da), Some(lb), Some(lob), Some(db)) =
                    (num(0), num(8), num(9), num(10), num(17), num(18), num(19))
                else {
                    continue;
                };
                if (la - at.0).abs() > near_deg * 2.0 || distance_m((la, lo), at).min(distance_m((lb, lob), at)) > NEAR_M {
                    continue;
                }
                let Some(ident) = &airport else { continue };
                let runway = runway_from_ends(width, (f[7], la, lo, da), (f[16], lb, lob, db));
                if !taken {
                    found.push(SimRunways { airport: ident.clone(), runways: Vec::new() });
                    taken = true;
                }
                found.last_mut().expect("pushed").runways.push(runway);
            }
            _ => {}
        }
    }
    found
}

/// The `apt.dat` files of an X-Plane install in the order X-Plane gives them priority:
/// the scenery packs in `scenery_packs.ini`, the global airports where it lists them (or
/// last).
pub fn apt_dats(xplane: &Path) -> Vec<PathBuf> {
    let custom = xplane.join("Custom Scenery");
    let global = xplane.join("Global Scenery").join("Global Airports").join("Earth nav data").join("apt.dat");
    let mut files = Vec::new();
    let mut global_listed = false;
    let ini = std::fs::read_to_string(custom.join("scenery_packs.ini")).unwrap_or_default();
    for line in ini.lines() {
        let Some(pack) = line.trim().strip_prefix("SCENERY_PACK ") else { continue };
        let pack = pack.trim();
        if pack == "*GLOBAL_AIRPORTS*" {
            files.push(global.clone());
            global_listed = true;
            continue;
        }
        let dir = Path::new(pack);
        let dir = if dir.is_absolute() { dir.to_path_buf() } else { xplane.join(dir) };
        files.push(dir.join("Earth nav data").join("apt.dat"));
    }
    if !global_listed {
        files.push(global);
    }
    files.retain(|f| f.is_file());
    files
}

/// The runways X-Plane draws near `at`: from the first scenery pack, in priority order,
/// that has each airport.
pub fn xplane_near(installs: &[PathBuf], at: (f64, f64)) -> Vec<SimRunways> {
    let mut found: Vec<SimRunways> = Vec::new();
    for xplane in installs {
        for file in apt_dats(xplane) {
            let Ok(f) = File::open(&file) else { continue };
            for airport in near_in_apt_dat(BufReader::new(f), at) {
                if !found.iter().any(|a| a.airport == airport.airport) {
                    found.push(airport);
                }
            }
        }
        if !found.is_empty() {
            break;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_touchdown_counts_once_after_flying() {
        let mut w = TouchdownWatch::new(2, 0, 1);
        w.set(0, 20.96, 0);
        w.set(1, -156.67, 0);
        w.set(2, 1.0, 0);
        w.set(2, 0.0, 1_000);
        // A bounce: airborne for less than ten seconds after the first contact.
        w.set(2, 1.0, 5_000);
        assert_eq!(w.take(), None);
        w.set(2, 0.0, 6_000);
        w.set(2, 1.0, 100_000);
        assert_eq!(w.take(), Some((20.96, -156.67)));
        assert_eq!(w.take(), None);
        // A touch-and-go a minute later counts again; one sooner doesn't.
        w.set(2, 0.0, 101_000);
        w.set(2, 1.0, 130_000);
        assert_eq!(w.take(), None);
        w.set(2, 0.0, 131_000);
        w.set(2, 1.0, 170_000);
        assert!(w.take().is_some());
    }

    const APT: &str = "\
I
1200 Generated by WorldEditor

1     10 0 0 PHJH Kapalua
100 30.48 1 0 0.25 0 2 0 02 20.95930000 -156.67500000 0 0 3 0 0 0 20 20.96660000 -156.67100000 45.72 0 3 0 0 0
1     95 0 0 PHOG Kahului
100 45.72 1 0 0.25 1 2 1 02 20.88000000 -156.44000000 0 0 3 0 0 0 20 20.90000000 -156.43000000 0 0 3 0 0 0
99
";

    #[test]
    fn finds_the_runways_near_the_touchdown_in_an_apt_dat() {
        let found = near_in_apt_dat(APT.as_bytes(), (20.9600, -156.6745));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].airport, "PHJH");
        let r = &found[0].runways[0];
        assert!((r.width_ft - 100.0).abs() < 0.1);
        assert_eq!(r.ends[0].ident, "02");
        assert!((r.ends[0].heading - 28.0).abs() < 2.0, "{}", r.ends[0].heading);
        assert!((r.ends[1].heading - r.ends[0].heading - 180.0).abs() < 0.1);
        assert!((r.ends[1].displaced_ft - 150.0).abs() < 0.1);
        assert!((r.length_ft - 2900.0).abs() < 200.0, "{}", r.length_ft);
        assert!(near_in_apt_dat(APT.as_bytes(), (0.0, 0.0)).is_empty());
    }

    #[test]
    fn offsets_and_bearings_agree() {
        let a = (46.15, -123.87);
        let b = offset(a, 334.0, 1500.0);
        assert!((distance_m(a, b) - 1500.0).abs() < 1.0);
        assert!((bearing(a, b) - 334.0).abs() < 0.1);
    }
}

#[cfg(test)]
mod against_the_install {
    /// Against the X-Plane installed here: `cargo test -- --ignored scenery`.
    #[test]
    #[ignore]
    fn scenery() {
        let installs = crate::xp_plugin_install::installs();
        let t = std::time::Instant::now();
        let found = super::xplane_near(&installs, (46.1527572, -123.876316));
        println!("{installs:?} in {:?}", t.elapsed());
        for a in &found {
            println!("{}", a.encode());
        }
        assert!(!found.is_empty());
    }
}
