use serde::{Deserialize, Serialize};

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
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
    #[serde(rename = "GPSfix")]
    pub gps_fix_type: f64,
    #[serde(rename = "HAL")]
    pub horizontal_alarm_limit: f64,
    #[serde(rename = "VAL")]
    pub vertical_alarm_limit: f64,
    #[serde(rename = "HPLwas")]
    pub horizontal_protection_level_waas: f64,
    #[serde(rename = "HPLfd")]
    pub horizontal_protection_level_fd: f64,
    #[serde(rename = "VPLwas")]
    pub vertical_protection_level_waas: f64,
    #[serde(rename = "sim_on_ground")]
    pub is_on_ground: f64,

    // X-Plane 12 specific fields (Flat Columns)
    pub xp_agl: f64,
    pub xp_prop_rpm: f64,
    pub xp_gear_ratio: f64,
}

impl FlightMetrics {
    pub fn update_max(&mut self, other: &Self) {
        self.latitude = self.latitude.max(other.latitude);
        self.longitude = self.longitude.max(other.longitude);
        self.indicated_altitude = self.indicated_altitude.max(other.indicated_altitude);
        self.altimeter_setting = self.altimeter_setting.max(other.altimeter_setting);
        self.gps_altitude_msl = self.gps_altitude_msl.max(other.gps_altitude_msl);
        self.outside_air_temp = self.outside_air_temp.max(other.outside_air_temp);
        self.indicated_airspeed = self.indicated_airspeed.max(other.indicated_airspeed);
        self.ground_speed = self.ground_speed.max(other.ground_speed);
        self.vertical_speed = self.vertical_speed.max(other.vertical_speed);
        self.pitch_angle = self.pitch_angle.max(other.pitch_angle);
        self.roll_angle = self.roll_angle.max(other.roll_angle);
        self.lateral_acceleration = self.lateral_acceleration.max(other.lateral_acceleration);
        self.normal_acceleration = self.normal_acceleration.max(other.normal_acceleration);
        self.heading = self.heading.max(other.heading);
        self.track = self.track.max(other.track);
        self.volts_1 = self.volts_1.max(other.volts_1);
        self.volts_2 = self.volts_2.max(other.volts_2);
        self.amps_1 = self.amps_1.max(other.amps_1);
        self.fuel_quantity_left = self.fuel_quantity_left.max(other.fuel_quantity_left);
        self.fuel_quantity_right = self.fuel_quantity_right.max(other.fuel_quantity_right);
        self.engine_1_fuel_flow = self.engine_1_fuel_flow.max(other.engine_1_fuel_flow);
        self.engine_1_oil_temp = self.engine_1_oil_temp.max(other.engine_1_oil_temp);
        self.engine_1_oil_pressure = self.engine_1_oil_pressure.max(other.engine_1_oil_pressure);
        self.engine_1_manifold_pressure = self.engine_1_manifold_pressure.max(other.engine_1_manifold_pressure);
        self.engine_1_rpm = self.engine_1_rpm.max(other.engine_1_rpm);
        self.engine_1_percent_power = self.engine_1_percent_power.max(other.engine_1_percent_power);
        self.engine_1_cht_1 = self.engine_1_cht_1.max(other.engine_1_cht_1);
        self.engine_1_cht_2 = self.engine_1_cht_2.max(other.engine_1_cht_2);
        self.engine_1_cht_3 = self.engine_1_cht_3.max(other.engine_1_cht_3);
        self.engine_1_cht_4 = self.engine_1_cht_4.max(other.engine_1_cht_4);
        self.engine_1_cht_5 = self.engine_1_cht_5.max(other.engine_1_cht_5);
        self.engine_1_cht_6 = self.engine_1_cht_6.max(other.engine_1_cht_6);
        self.engine_1_egt_1 = self.engine_1_egt_1.max(other.engine_1_egt_1);
        self.engine_1_egt_2 = self.engine_1_egt_2.max(other.engine_1_egt_2);
        self.engine_1_egt_3 = self.engine_1_egt_3.max(other.engine_1_egt_3);
        self.engine_1_egt_4 = self.engine_1_egt_4.max(other.engine_1_egt_4);
        self.engine_1_egt_5 = self.engine_1_egt_5.max(other.engine_1_egt_5);
        self.engine_1_egt_6 = self.engine_1_egt_6.max(other.engine_1_egt_6);
        self.engine_1_tit_1 = self.engine_1_tit_1.max(other.engine_1_tit_1);
        self.engine_1_tit_2 = self.engine_1_tit_2.max(other.engine_1_tit_2);
        self.gps_altitude_wgs84 = self.gps_altitude_wgs84.max(other.gps_altitude_wgs84);
        self.true_airspeed = self.true_airspeed.max(other.true_airspeed);
        self.hsi_source = self.hsi_source.max(other.hsi_source);
        self.selected_course = self.selected_course.max(other.selected_course);
        self.nav_1_frequency = self.nav_1_frequency.max(other.nav_1_frequency);
        self.nav_2_frequency = self.nav_2_frequency.max(other.nav_2_frequency);
        self.com_1_frequency = self.com_1_frequency.max(other.com_1_frequency);
        self.com_2_frequency = self.com_2_frequency.max(other.com_2_frequency);
        self.horizontal_cdi = self.horizontal_cdi.max(other.horizontal_cdi);
        self.vertical_cdi = self.vertical_cdi.max(other.vertical_cdi);
        self.wind_speed = self.wind_speed.max(other.wind_speed);
        self.wind_direction = self.wind_direction.max(other.wind_direction);
        self.waypoint_distance = self.waypoint_distance.max(other.waypoint_distance);
        self.waypoint_bearing = self.waypoint_bearing.max(other.waypoint_bearing);
        self.magnetic_variation = self.magnetic_variation.max(other.magnetic_variation);
        self.autopilot_active = self.autopilot_active.max(other.autopilot_active);
        self.roll_mode = self.roll_mode.max(other.roll_mode);
        self.pitch_mode = self.pitch_mode.max(other.pitch_mode);
        self.roll_command = self.roll_command.max(other.roll_command);
        self.pitch_command = self.pitch_command.max(other.pitch_command);
        self.vertical_speed_target = self.vertical_speed_target.max(other.vertical_speed_target);
        self.gps_fix_type = self.gps_fix_type.max(other.gps_fix_type);
        self.horizontal_alarm_limit = self.horizontal_alarm_limit.max(other.horizontal_alarm_limit);
        self.vertical_alarm_limit = self.vertical_alarm_limit.max(other.vertical_alarm_limit);
        self.horizontal_protection_level_waas = self.horizontal_protection_level_waas.max(other.horizontal_protection_level_waas);
        self.horizontal_protection_level_fd = self.horizontal_protection_level_fd.max(other.horizontal_protection_level_fd);
        self.vertical_protection_level_waas = self.vertical_protection_level_waas.max(other.vertical_protection_level_waas);
        self.is_on_ground = self.is_on_ground.max(other.is_on_ground);
        self.xp_agl = self.xp_agl.max(other.xp_agl);
        self.xp_prop_rpm = self.xp_prop_rpm.max(other.xp_prop_rpm);
        self.xp_gear_ratio = self.xp_gear_ratio.max(other.xp_gear_ratio);
    }
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
pub struct AirportInfo {
    pub icao: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookFlightSummary {
    pub log_path: String,
    pub airframe_name: String,
    pub departure: AirportInfo,
    pub arrival: AirportInfo,
    pub takeoff_time: Option<String>,
    pub landing_time: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub takeoff_snapshot: Option<FlightMetrics>,
    pub landing_snapshot: Option<FlightMetrics>,
    pub current_snapshot: Option<FlightMetrics>,
    pub max_entries: Option<FlightMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlightEvent {
    pub timestamp: String,
    pub event_type: String, // "takeoff", "landing", "top_of_climb", "top_of_descent"
    pub latitude: f64,
    pub longitude: f64,
}
