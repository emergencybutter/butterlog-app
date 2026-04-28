use serde::{Deserialize, Serialize};

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct FlightMetrics {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(rename = "alt_b")]
    pub indicated_altitude: f64,
    #[serde(rename = "baro_a")]
    pub altimeter_setting: f64,
    #[serde(rename = "alt_msl")]
    pub gps_altitude_msl: f64,
    #[serde(rename = "oat")]
    pub outside_air_temp: f64,
    #[serde(rename = "ias")]
    pub indicated_airspeed: f64,
    #[serde(rename = "gnd_spd")]
    pub ground_speed: f64,
    #[serde(rename = "v_spd")]
    pub vertical_speed: f64,
    #[serde(rename = "pitch")]
    pub pitch_angle: f64,
    #[serde(rename = "roll")]
    pub roll_angle: f64,
    #[serde(rename = "lat_ac")]
    pub lateral_acceleration: f64,
    #[serde(rename = "norm_ac")]
    pub normal_acceleration: f64,
    #[serde(rename = "hdg")]
    pub heading: f64,
    #[serde(rename = "trk")]
    pub track: f64,
    #[serde(rename = "volt1")]
    pub volts_1: f64,
    #[serde(rename = "volt2")]
    pub volts_2: f64,
    #[serde(rename = "amp1")]
    pub amps_1: f64,
    #[serde(rename = "f_qty_l")]
    pub fuel_quantity_left: f64,
    #[serde(rename = "f_qty_r")]
    pub fuel_quantity_right: f64,
    #[serde(rename = "e1_fflow")]
    pub engine_1_fuel_flow: f64,
    #[serde(rename = "e1_oil_t")]
    pub engine_1_oil_temp: f64,
    #[serde(rename = "e1_oil_p")]
    pub engine_1_oil_pressure: f64,
    #[serde(rename = "e1_map")]
    pub engine_1_manifold_pressure: f64,
    #[serde(rename = "e1_rpm")]
    pub engine_1_rpm: f64,
    #[serde(rename = "e1_pwr")]
    pub engine_1_percent_power: f64,
    #[serde(rename = "e1_cht1")]
    pub engine_1_cht_1: f64,
    #[serde(rename = "e1_cht2")]
    pub engine_1_cht_2: f64,
    #[serde(rename = "e1_cht3")]
    pub engine_1_cht_3: f64,
    #[serde(rename = "e1_cht4")]
    pub engine_1_cht_4: f64,
    #[serde(rename = "e1_cht5")]
    pub engine_1_cht_5: f64,
    #[serde(rename = "e1_cht6")]
    pub engine_1_cht_6: f64,
    #[serde(rename = "e1_egt1")]
    pub engine_1_egt_1: f64,
    #[serde(rename = "e1_egt2")]
    pub engine_1_egt_2: f64,
    #[serde(rename = "e1_egt3")]
    pub engine_1_egt_3: f64,
    #[serde(rename = "e1_egt4")]
    pub engine_1_egt_4: f64,
    #[serde(rename = "e1_egt5")]
    pub engine_1_egt_5: f64,
    #[serde(rename = "e1_egt6")]
    pub engine_1_egt_6: f64,
    #[serde(rename = "e1_tit1")]
    pub engine_1_tit_1: f64,
    #[serde(rename = "e1_tit2")]
    pub engine_1_tit_2: f64,
    #[serde(rename = "alt_gps")]
    pub gps_altitude_wgs84: f64,
    #[serde(rename = "tas")]
    pub true_airspeed: f64,
    #[serde(rename = "hsis")]
    pub hsi_source: f64,
    #[serde(rename = "crs")]
    pub selected_course: f64,
    #[serde(rename = "nav1")]
    pub nav_1_frequency: f64,
    #[serde(rename = "nav2")]
    pub nav_2_frequency: f64,
    #[serde(rename = "com1")]
    pub com_1_frequency: f64,
    #[serde(rename = "com2")]
    pub com_2_frequency: f64,
    #[serde(rename = "hcdi")]
    pub horizontal_cdi: f64,
    #[serde(rename = "vcdi")]
    pub vertical_cdi: f64,
    #[serde(rename = "wnd_spd")]
    pub wind_speed: f64,
    #[serde(rename = "wnd_dr")]
    pub wind_direction: f64,
    #[serde(rename = "wpt_dst")]
    pub waypoint_distance: f64,
    #[serde(rename = "wpt_brg")]
    pub waypoint_bearing: f64,
    #[serde(rename = "mag_var")]
    pub magnetic_variation: f64,
    #[serde(rename = "afcs_on")]
    pub autopilot_active: f64,
    #[serde(rename = "roll_m")]
    pub roll_mode: f64,
    #[serde(rename = "pitch_m")]
    pub pitch_mode: f64,
    #[serde(rename = "roll_c")]
    pub roll_command: f64,
    #[serde(rename = "pitch_c")]
    pub pitch_command: f64,
    #[serde(rename = "v_spd_g")]
    pub vertical_speed_target: f64,
    #[serde(rename = "gps_fix")]
    pub gps_fix_type: f64,
    #[serde(rename = "hal")]
    pub horizontal_alarm_limit: f64,
    #[serde(rename = "val")]
    pub vertical_alarm_limit: f64,
    #[serde(rename = "hpl_was")]
    pub horizontal_protection_level_waas: f64,
    #[serde(rename = "hpl_fd")]
    pub horizontal_protection_level_fd: f64,
    #[serde(rename = "vpl_was")]
    pub vertical_protection_level_waas: f64,
    #[serde(rename = "sim_on_ground")]
    pub is_on_ground: f64,

    // X-Plane 12 specific fields (Flat Columns)
    pub xp_agl: f64,
    pub xp_prop_rpm: f64,
    pub xp_gear_ratio: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AircraftInfo {
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FlightPhase {
    Parked,
    TaxiOut,
    Takeoff,
    Climb,
    Cruise,
    Descent,
    Approach,
    Landing,
    TaxiIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlightEvent {
    pub timestamp: String,
    pub event_type: String, // "takeoff", "landing", "top_of_climb", "top_of_descent"
    pub latitude: f64,
    pub longitude: f64,
}
