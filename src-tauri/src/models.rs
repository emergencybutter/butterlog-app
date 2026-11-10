use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(default)]
pub struct FlightMetrics {
    #[serde(rename = "Latitude")]
    pub latitude: f64,
    #[serde(rename = "Longitude")]
    pub longitude: f64,
    #[serde(rename = "AltB")]
    pub indicated_altitude: f64,
    #[serde(rename = "BaroA")]
    pub altimeter_setting: f64,
    #[serde(rename = "AltMSL")]
    pub gps_altitude_msl: f64,
    #[serde(rename = "OAT")]
    pub outside_air_temp: f64,
    #[serde(rename = "IAS")]
    pub indicated_airspeed: f64,
    #[serde(rename = "GndSpd")]
    pub ground_speed: f64,
    #[serde(rename = "VSpd")]
    pub vertical_speed: f64,
    #[serde(rename = "Pitch")]
    pub pitch_angle: f64,
    #[serde(rename = "Roll")]
    pub roll_angle: f64,
    #[serde(rename = "LatAc")]
    pub lateral_acceleration: f64,
    #[serde(rename = "NormAc")]
    pub normal_acceleration: f64,
    #[serde(rename = "HDG")]
    pub heading: f64,
    #[serde(rename = "TRK")]
    pub track: f64,
    #[serde(rename = "volt1")]
    pub volts_1: f64,
    #[serde(rename = "volt2")]
    pub volts_2: f64,
    #[serde(rename = "amp1")]
    pub amps_1: f64,
    #[serde(rename = "FQtyL")]
    pub fuel_quantity_left: f64,
    #[serde(rename = "FQtyR")]
    pub fuel_quantity_right: f64,
    #[serde(rename = "E1 FFlow")]
    pub engine_1_fuel_flow: f64,
    #[serde(rename = "E1 OilT")]
    pub engine_1_oil_temp: f64,
    #[serde(rename = "E1 OilP")]
    pub engine_1_oil_pressure: f64,
    #[serde(rename = "E1 MAP")]
    pub engine_1_manifold_pressure: f64,
    #[serde(rename = "E1 RPM")]
    pub engine_1_rpm: f64,
    #[serde(rename = "E1 %Pwr")]
    pub engine_1_percent_power: f64,
    #[serde(rename = "E1 CHT1")]
    pub engine_1_cht_1: f64,
    #[serde(rename = "E1 CHT2")]
    pub engine_1_cht_2: f64,
    #[serde(rename = "E1 CHT3")]
    pub engine_1_cht_3: f64,
    #[serde(rename = "E1 CHT4")]
    pub engine_1_cht_4: f64,
    #[serde(rename = "E1 CHT5")]
    pub engine_1_cht_5: f64,
    #[serde(rename = "E1 CHT6")]
    pub engine_1_cht_6: f64,
    #[serde(rename = "E1 EGT1")]
    pub engine_1_egt_1: f64,
    #[serde(rename = "E1 EGT2")]
    pub engine_1_egt_2: f64,
    #[serde(rename = "E1 EGT3")]
    pub engine_1_egt_3: f64,
    #[serde(rename = "E1 EGT4")]
    pub engine_1_egt_4: f64,
    #[serde(rename = "E1 EGT5")]
    pub engine_1_egt_5: f64,
    #[serde(rename = "E1 EGT6")]
    pub engine_1_egt_6: f64,
    #[serde(rename = "E1 TIT1")]
    pub engine_1_tit_1: f64,
    #[serde(rename = "E1 TIT2")]
    pub engine_1_tit_2: f64,
    #[serde(rename = "AltGPS")]
    pub gps_altitude_wgs84: f64,
    #[serde(rename = "TAS")]
    pub true_airspeed: f64,
    #[serde(rename = "HSIS")]
    pub hsi_source: f64,
    #[serde(rename = "CRS")]
    pub selected_course: f64,
    #[serde(rename = "NAV1")]
    pub nav_1_frequency: f64,
    #[serde(rename = "NAV2")]
    pub nav_2_frequency: f64,
    #[serde(rename = "COM1")]
    pub com_1_frequency: f64,
    #[serde(rename = "COM2")]
    pub com_2_frequency: f64,
    #[serde(rename = "HCDI")]
    pub horizontal_cdi: f64,
    #[serde(rename = "VCDI")]
    pub vertical_cdi: f64,
    #[serde(rename = "WndSpd")]
    pub wind_speed: f64,
    #[serde(rename = "WndDr")]
    pub wind_direction: f64,
    #[serde(rename = "WptDst")]
    pub waypoint_distance: f64,
    #[serde(rename = "WptBrg")]
    pub waypoint_bearing: f64,
    #[serde(rename = "MagVar")]
    pub magnetic_variation: f64,
    #[serde(rename = "AfcsOn")]
    pub autopilot_active: f64,
    #[serde(rename = "RollM")]
    pub roll_mode: f64,
    #[serde(rename = "PitchM")]
    pub pitch_mode: f64,
    #[serde(rename = "RollC")]
    pub roll_command: f64,
    #[serde(rename = "PichC")]
    pub pitch_command: f64,
    #[serde(rename = "VSpdG")]
    pub vertical_speed_target: f64,
    #[serde(rename = "sim_on_ground")]
    pub is_on_ground: f64,
    pub altitude_agl: f64,
    pub gforce: f64,
    pub pressure_altitude: f64,
    pub density_altitude: f64,
    pub pressurization_cabin_altitude: f64,

    pub xp_prop_rpm: f64,
    pub xp_gear_ratio: f64,
    pub parking_brake: f64,

    // Exterior light states (0.0 off / 1.0 on), propagated to peers so injected
    // traffic shows the right lights. Appended last to keep the MSFS SimConnect
    // direct-cast field offsets stable.
    pub nav_lights: f64,
    pub beacon_lights: f64,
    pub strobe_lights: f64,
    pub taxi_lights: f64,
    pub landing_lights: f64,
}

/// See [`FlightMetrics::has_position`].
pub fn has_position(latitude: f64, longitude: f64) -> bool {
    const NULL_ISLAND_DEG: f64 = 0.01;
    !(latitude.abs() < NULL_ISLAND_DEG && longitude.abs() < NULL_ISLAND_DEG)
}

impl FlightMetrics {
    /// Whether the sim has given us a real position yet. While MSFS loads a
    /// flight it parks the user aircraft at "null island" - 0,0 at ~54,000 ft -
    /// and the coordinates are only *nearly* zero (a longitude of 1e-5 has been
    /// seen), so an exact `== 0.0` test lets those samples through. No airport
    /// is within 0.01 degrees (~1 km) of 0,0, so nothing real is lost.
    ///
    /// Loading samples are not always at null island: 0,90 with an indicated
    /// altitude of -9,404,658 ft has been seen. No real aircraft indicates far
    /// below the Dead Sea (-1,400 ft), so such an altitude marks the whole
    /// sample as garbage.
    pub fn has_position(&self) -> bool {
        const MIN_INDICATED_ALTITUDE_FT: f64 = -5000.0;
        has_position(self.latitude, self.longitude)
            && self.indicated_altitude > MIN_INDICATED_ALTITUDE_FT
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AircraftInfo {
    pub title: String,
    /// Livery name (MSFS `LIVERY NAME` SimVar / X-Plane `acf_livery_path`). May be empty.
    pub livery: String,
    /// ICAO type designator resolved from the title + livery via the word index.
    /// May be empty when nothing matched. Distinct from `atc_model` (the sim's own claim).
    pub resolved_icao: String,
    /// Operating airline name identified from the title + livery via the airline index.
    /// May be empty when no operator was inferred.
    pub resolved_airline: String,
    /// ICAO designator of the identified operating airline (e.g. `UAL`). This is the
    /// portable form sent over the multiplayer wire; the name above is for display only.
    /// May be empty when no operator was inferred.
    pub resolved_airline_icao: String,
    pub atc_model: String,
    pub atc_id: String,
    pub object_class: String,
    pub category: String,
    pub num_engines: i32,
    pub engine_type: String,
}

#[cfg(test)]
mod position_tests {
    use super::{has_position, FlightMetrics};

    #[test]
    fn loading_garbage_is_not_a_position() {
        // Seen from MSFS while a flight loads: off null island, but the
        // indicated altitude gives it away.
        let mut m = FlightMetrics::default();
        m.latitude = -2.1556893808986427e-07;
        m.longitude = 90.00032277330374;
        m.indicated_altitude = -9404658.0;
        assert!(!m.has_position());

        m.indicated_altitude = -1400.0; // Dead Sea
        assert!(m.has_position());
    }

    #[test]
    fn null_island_is_not_a_position() {
        assert!(!has_position(0.0, 0.0));
        // Seen from MSFS while a flight loads.
        assert!(!has_position(0.0, 1.139614354883633e-05));
    }

    #[test]
    fn real_positions_are_kept() {
        assert!(has_position(39.723220567447314, -86.30493499662506)); // KIND
        assert!(has_position(4.896, -1.775)); // DGTK, the closest real airport to 0,0
        assert!(has_position(0.0, 0.5));
    }
}
