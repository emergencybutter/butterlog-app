use simplesimconnect::*;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use chrono::{Local, DateTime};
use std::fs::{File, create_dir_all};
use std::io::{Write, BufWriter};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, serde::Serialize)]
pub struct FlightMetrics {
    pub latitude: f64,
    pub longitude: f64,
    pub alt_b: f64,
    pub baro_a: f64,
    pub alt_msl: f64,
    pub oat: f64,
    pub ias: f64,
    pub gnd_spd: f64,
    pub v_spd: f64,
    pub pitch: f64,
    pub roll: f64,
    pub lat_ac: f64,
    pub norm_ac: f64,
    pub hdg: f64,
    pub trk: f64,
    pub volt1: f64,
    pub volt2: f64,
    pub amp1: f64,
    pub f_qty_l: f64,
    pub f_qty_r: f64,
    pub e1_fflow: f64,
    pub e1_oil_t: f64,
    pub e1_oil_p: f64,
    pub e1_map: f64,
    pub e1_rpm: f64,
    pub e1_pwr: f64,
    pub e1_cht1: f64,
    pub e1_cht2: f64,
    pub e1_cht3: f64,
    pub e1_cht4: f64,
    pub e1_cht5: f64,
    pub e1_cht6: f64,
    pub e1_egt1: f64,
    pub e1_egt2: f64,
    pub e1_egt3: f64,
    pub e1_egt4: f64,
    pub e1_egt5: f64,
    pub e1_egt6: f64,
    pub e1_tit1: f64,
    pub e1_tit2: f64,
    pub alt_gps: f64,
    pub tas: f64,
    pub hsis: f64,
    pub crs: f64,
    pub nav1: f64,
    pub nav2: f64,
    pub com1: f64,
    pub com2: f64,
    pub hcdi: f64,
    pub vcdi: f64,
    pub wnd_spd: f64,
    pub wnd_dr: f64,
    pub wpt_dst: f64,
    pub wpt_brg: f64,
    pub mag_var: f64,
    pub afcs_on: f64,
    pub roll_m: f64,
    pub pitch_m: f64,
    pub roll_c: f64,
    pub pitch_c: f64,
    pub v_spd_g: f64,
    pub gps_fix: f64,
    pub hal: f64,
    pub val: f64,
    pub hpl_was: f64,
    pub hpl_fd: f64,
    pub vpl_was: f64,
}

pub struct SimConnectMonitor {
    metrics: Arc<Mutex<FlightMetrics>>,
    running: Arc<Mutex<bool>>,
    connected: Arc<Mutex<bool>>,
}

impl SimConnectMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(FlightMetrics::default())),
            running: Arc::new(Mutex::new(false)),
            connected: Arc::new(Mutex::new(false)),
        }
    }

    pub fn start(&self, app: AppHandle, log_path: Option<PathBuf>) -> anyhow::Result<()> {
        let mut running = self.running.lock().unwrap();
        if *running {
            return Ok(());
        }
        *running = true;

        let metrics = self.metrics.clone();
        let running_clone = self.running.clone();
        let connected_clone = self.connected.clone();

        thread::spawn(move || {
            crate::append_log(&app, format!("[{}] SimConnect monitor thread started.", Local::now().format("%Y-%m-%d %H:%M:%S")));
            loop {
                if !*running_clone.lock().unwrap() {
                    break;
                }

                match SimConnect::open("ButterLogV2") {
                    Ok(sc) => {
                        crate::append_log(&app, format!("[{}] Successfully connected to SimConnect.", Local::now().format("%Y-%m-%d %H:%M:%S")));
                        {
                            let mut connected = connected_clone.lock().unwrap();
                            *connected = true;
                        }

                        if let Err(e) = Self::run_monitor(&app, sc, &metrics, &running_clone, log_path.as_ref()) {
                            crate::append_log(&app, format!("[{}] Monitor error: {}", Local::now().format("%Y-%m-%d %H:%M:%S"), e));
                        }

                        {
                            let mut connected = connected_clone.lock().unwrap();
                            *connected = false;
                        }
                        crate::append_log(&app, format!("[{}] Disconnected from SimConnect.", Local::now().format("%Y-%m-%d %H:%M:%S")));
                    }
                    Err(_) => {
                        // Silently retry every second if SimConnect is not available
                    }
                }

                thread::sleep(Duration::from_secs(1));
            }
            crate::append_log(&app, format!("[{}] SimConnect monitor thread exiting.", Local::now().format("%Y-%m-%d %H:%M:%S")));
        });

        Ok(())
    }

    pub fn stop(&self) {
        let mut running = self.running.lock().unwrap();
        *running = false;
    }

    pub fn get_metrics(&self) -> FlightMetrics {
        *self.metrics.lock().unwrap()
    }

    pub fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }

    fn run_monitor(
        app: &AppHandle,
        sc: SimConnect,
        metrics: &Arc<Mutex<FlightMetrics>>,
        running: &Arc<Mutex<bool>>,
        requested_log_path: Option<&PathBuf>,
    ) -> anyhow::Result<()> {
        let define_id = 1;
        let request_id = 1;
        let event_sim_start = 1;
        let event_sim_stop = 2;

        sc.subscribe_to_system_event(event_sim_start, "SimStart")?;
        sc.subscribe_to_system_event(event_sim_stop, "SimStop")?;

        // Register all fields in the exact order they appear in FlightMetrics struct
        sc.add_to_data_definition::<f64>(define_id, "PLANE LATITUDE", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "PLANE LONGITUDE", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "INDICATED ALTITUDE", "feet")?;
        sc.add_to_data_definition::<f64>(define_id, "KOHLSMAN SETTING HG", "inHg")?;
        sc.add_to_data_definition::<f64>(define_id, "PLANE ALTITUDE", "feet")?;
        sc.add_to_data_definition::<f64>(define_id, "AMBIENT TEMPERATURE", "celsius")?;
        sc.add_to_data_definition::<f64>(define_id, "AIRSPEED INDICATED", "knots")?;
        sc.add_to_data_definition::<f64>(define_id, "GROUND VELOCITY", "knots")?;
        sc.add_to_data_definition::<f64>(define_id, "VERTICAL SPEED", "feet per minute")?;
        sc.add_to_data_definition::<f64>(define_id, "PLANE PITCH DEGREES", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "PLANE BANK DEGREES", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "ACCELERATION BODY X", "G force")?;
        sc.add_to_data_definition::<f64>(define_id, "ACCELERATION BODY Z", "G force")?;
        sc.add_to_data_definition::<f64>(define_id, "PLANE HEADING DEGREES TRUE", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS GROUND TRUE TRACK", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "ELECTRICAL MAIN BUS VOLTAGE:1", "volts")?;
        sc.add_to_data_definition::<f64>(define_id, "ELECTRICAL MAIN BUS VOLTAGE:2", "volts")?;
        sc.add_to_data_definition::<f64>(define_id, "ELECTRICAL MAIN BUS AMPS:1", "amps")?;
        sc.add_to_data_definition::<f64>(define_id, "FUEL LEFT QUANTITY", "gallons")?;
        sc.add_to_data_definition::<f64>(define_id, "FUEL RIGHT QUANTITY", "gallons")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG FUEL FLOW GPH:1", "gallons per hour")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG OIL TEMPERATURE:1", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG OIL PRESSURE:1", "psi")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG MANIFOLD PRESSURE:1", "inHg")?;
        sc.add_to_data_definition::<f64>(define_id, "GENERAL ENG RPM:1", "rpm")?;
        sc.add_to_data_definition::<f64>(define_id, "GENERAL ENG PCT MAX RPM:1", "percent")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG CYLINDER HEAD TEMPERATURE:1", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG CYLINDER HEAD TEMPERATURE:2", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG CYLINDER HEAD TEMPERATURE:3", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG CYLINDER HEAD TEMPERATURE:4", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG CYLINDER HEAD TEMPERATURE:5", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG CYLINDER HEAD TEMPERATURE:6", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG EXHAUST GAS TEMPERATURE:1", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG EXHAUST GAS TEMPERATURE:2", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG EXHAUST GAS TEMPERATURE:3", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG EXHAUST GAS TEMPERATURE:4", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG EXHAUST GAS TEMPERATURE:5", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG EXHAUST GAS TEMPERATURE:6", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG TURBINE INLET TEMPERATURE:1", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG TURBINE INLET TEMPERATURE:2", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS POSITION ALT", "feet")?;
        sc.add_to_data_definition::<f64>(define_id, "AIRSPEED TRUE", "knots")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS DRIVES NAV1", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "NAV OBS:1", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "NAV ACTIVE FREQUENCY:1", "MHz")?;
        sc.add_to_data_definition::<f64>(define_id, "NAV ACTIVE FREQUENCY:2", "MHz")?;
        sc.add_to_data_definition::<f64>(define_id, "COM ACTIVE FREQUENCY:1", "MHz")?;
        sc.add_to_data_definition::<f64>(define_id, "COM ACTIVE FREQUENCY:2", "MHz")?;
        sc.add_to_data_definition::<f64>(define_id, "NAV CDI:1", "number")?;
        sc.add_to_data_definition::<f64>(define_id, "NAV GSI:1", "number")?;
        sc.add_to_data_definition::<f64>(define_id, "AMBIENT WIND VELOCITY", "knots")?;
        sc.add_to_data_definition::<f64>(define_id, "AMBIENT WIND DIRECTION", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS WP DISTANCE", "nautical miles")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS WP BEARING", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "MAGVAR", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT MASTER", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT ROLL HOLD", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT PITCH HOLD", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT BANK HOLD ANGLE", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT PITCH HOLD ANGLE", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT VERTICAL HOLD VAR", "feet per minute")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS FIX TYPE", "enum")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS HORIZONTAL ERROR", "meters")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS VERTICAL ERROR", "meters")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS WP DISTANCE", "meters")?; // Placeholder for HPLwas
        sc.add_to_data_definition::<f64>(define_id, "GPS WP DISTANCE", "meters")?; // Placeholder for HPLfd
        sc.add_to_data_definition::<f64>(define_id, "GPS WP DISTANCE", "meters")?; // Placeholder for VPLwas

        sc.request_data_on_sim_object(
            request_id,
            define_id,
            OBJECT_ID_USER,
            PERIOD_VISUAL_FRAME,
        )?;

        let mut current_log_path: Option<PathBuf> = requested_log_path.cloned();
        let mut writer: Option<BufWriter<File>> = if let Some(path) = requested_log_path {
            let file = File::create(path)?;
            let mut w = BufWriter::new(file);
            Self::write_header(&mut w)?;
            Some(w)
        } else {
            None
        };

        let mut analyzer = crate::flight_analyzer::FlightAnalyzer::new();
        let mut last_log_time = Local::now();

        loop {
            if !*running.lock().unwrap() {
                break;
            }

            while let Some(msg) = sc.get_next_dispatch()? {
                if msg.is_quit() {
                    return Ok(());
                }

                if let Some(event) = msg.as_event() {
                    if event.event_id == event_sim_start {
                        crate::append_log(app, format!("[{}] Received SimStart event. Starting new flight log.", Local::now().format("%H:%M:%S")));
                        
                        // Close existing writer if any
                        if let Some(mut w) = writer.take() {
                            let _ = w.flush();
                        }
                        analyzer.reset();

                        // Create new log file
                        let log_dir = app.path().app_data_dir()?.join("logs");
                        create_dir_all(&log_dir)?;
                        let filename = format!("butterlog_{}.csv", Local::now().format("%Y%m%d_%H%M%S"));
                        let path = log_dir.join(filename);
                        current_log_path = Some(path.clone());
                        
                        match File::create(&path) {
                            Ok(file) => {
                                let mut w = BufWriter::new(file);
                                if let Err(e) = Self::write_header(&mut w) {
                                    crate::append_log(app, format!("Failed to write header to new log: {}", e));
                                } else {
                                    writer = Some(w);
                                    crate::append_log(app, format!("New flight log created at: {:?}", path));
                                }
                            }
                            Err(e) => {
                                crate::append_log(app, format!("Failed to create new log file: {}", e));
                            }
                        }
                    } else if event.event_id == event_sim_stop {
                        crate::append_log(app, format!("[{}] Received SimStop event. Closing and analyzing flight log.", Local::now().format("%H:%M:%S")));
                        
                        // Close existing writer
                        if let Some(mut w) = writer.take() {
                            let _ = w.flush();
                        }

                        // Perform analysis and rename
                        if let (Some(path), Some(db)) = (current_log_path.take(), app.try_state::<crate::airports::AirportsDatabase>()) {
                            let start_icao = analyzer.find_start_icao(&db);
                            let end_icao = analyzer.find_end_icao(&db);
                            
                            if let Some(old_filename) = path.file_name().and_then(|f| f.to_str()) {
                                let new_filename = old_filename.replace("butterlog_", &format!("butterlog_{}_{}_", start_icao, end_icao));
                                let new_path = path.with_file_name(new_filename);
                                
                                match std::fs::rename(&path, &new_path) {
                                    Ok(_) => {
                                        crate::append_log(app, format!("Flight log renamed to: {:?}", new_path.file_name().unwrap()));
                                    }
                                    Err(e) => {
                                        crate::append_log(app, format!("Failed to rename log file: {}", e));
                                    }
                                }
                            }
                        } else {
                            crate::append_log(app, "Could not rename log file: path or database not available.".to_string());
                        }
                    }
                }

                if let Some(data) = msg.as_sim_object_data::<FlightMetrics>() {
                    if msg.request_id() == Some(request_id) {
                        let mut m = metrics.lock().unwrap();
                        *m = *data;

                        // Log to CSV every second
                        let now = Local::now();
                        if now.signed_duration_since(last_log_time) >= chrono::Duration::seconds(1) {
                            last_log_time = now;
                            
                            analyzer.add_point(data.latitude, data.longitude);

                            if let Some(ref mut w) = writer {
                                if let Err(e) = Self::write_csv_row(w, &now, data) {
                                    crate::append_log(app, format!("Failed to write CSV row: {}", e));
                                }
                            }
                        }
                    }
                }
            }

            thread::sleep(Duration::from_millis(50));
        }

        if let Some(mut w) = writer {
            w.flush()?;
        }

        Ok(())
    }

    fn write_header<W: Write>(w: &mut W) -> std::io::Result<()> {
        writeln!(w, "#airframe_info, log_version=\"1.00\", airframe_name=\"Simulated Aircraft\", unit_software_part_number=\"006-BXXX9-DE\", unit_software_version=\"15.24\", system_software_part_number=\"006-BXXXX-37\", system_id=\"25XXXX67\", mode=NORMAL, simulator_id=\"ButterLogV2\",")?;
        writeln!(w, "#yyy-mm-dd, hh:mm:ss,   hh:mm,  ident,      degrees,      degrees, ft Baro,  inch,  ft msl, deg C,     kt,     kt,     fpm,    deg,    deg,      G,      G,   deg,   deg, volts, volts,  amps,   gals,   gals,      gph,   deg F,     psi,     Hg,    rpm,       %,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,  ft wgs,  kt, enum,    deg,    MHz,    MHz,     MHz,     MHz,    fsd,    fsd,     kt,   deg,     nm,    deg,    deg,   bool,  enum,   enum,   deg,   deg,   fpm,   enum,   mt,    mt,     mt,    mt,     mt")?;
        writeln!(w, "Lcl Date, Lcl Time, UTCOfst, AtvWpt,     Latitude,    Longitude,    AltB, BaroA,  AltMSL,   OAT,    IAS, GndSpd,    VSpd,  Pitch,   Roll,  LatAc, NormAc,   HDG,   TRK, volt1, volt2,  amp1,  FQtyL,  FQtyR, E1 FFlow, E1 OilT, E1 OilP, E1 MAP, E1 RPM, E1 %Pwr, E1 CHT1, E1 CHT2, E1 CHT3, E1 CHT4, E1 CHT5, E1 CHT6, E1 EGT1, E1 EGT2, E1 EGT3, E1 EGT4, E1 EGT5, E1 EGT6, E1 TIT1, E1 TIT2,  AltGPS, TAS, HSIS,    CRS,   NAV1,   NAV2,    COM1,    COM2,   HCDI,   VCDI,WndSpd,WndDr, WptDst, WptBrg, MagVar, AfcsOn, RollM, PitchM, RollC, PichC, VSpdG, GPSfix,  HAL,   VAL, HPLwas, HPLfd, VPLwas")?;
        Ok(())
    }

    fn write_csv_row<W: Write>(w: &mut W, now: &DateTime<Local>, m: &FlightMetrics) -> std::io::Result<()> {
        let lcl_date = now.format("%Y-%m-%d").to_string();
        let lcl_time = now.format("%H:%M:%S").to_string();
        let utc_ofst = now.offset().to_string();

        write!(w, "{}, {}, {}, {}, ", lcl_date, lcl_time, utc_ofst, "")?; // Empty AtvWpt
        write!(w, "{:.6}, {:.6}, {:.1}, {:.2}, {:.1}, {:.1}, ", m.latitude, m.longitude, m.alt_b, m.baro_a, m.alt_msl, m.oat)?;
        write!(w, "{:.1}, {:.1}, {:.1}, {:.1}, {:.1}, ", m.ias, m.gnd_spd, m.v_spd, m.pitch, m.roll)?;
        write!(w, "{:.3}, {:.3}, {:.1}, {:.1}, ", m.lat_ac, m.norm_ac, m.hdg, m.trk)?;
        write!(w, "{:.1}, {:.1}, {:.1}, ", m.volt1, m.volt2, m.amp1)?;
        write!(w, "{:.2}, {:.2}, {:.1}, ", m.f_qty_l, m.f_qty_r, m.e1_fflow)?;
        write!(w, "{:.1}, {:.1}, {:.2}, {:.1}, {:.1}, ", m.e1_oil_t, m.e1_oil_p, m.e1_map, m.e1_rpm, m.e1_pwr)?;
        write!(w, "{:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, ", m.e1_cht1, m.e1_cht2, m.e1_cht3, m.e1_cht4, m.e1_cht5, m.e1_cht6)?;
        write!(w, "{:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, ", m.e1_egt1, m.e1_egt2, m.e1_egt3, m.e1_egt4, m.e1_egt5, m.e1_egt6)?;
        write!(w, "{:.1}, {:.1}, ", m.e1_tit1, m.e1_tit2)?;
        write!(w, "{:.1}, {:.1}, {}, {:.0}, ", m.alt_gps, m.tas, "GPS", m.crs)?;
        write!(w, "{:.3}, {:.3}, {:.3}, {:.3}, ", m.nav1, m.nav2, m.com1, m.com2)?;
        write!(w, "{:.2}, {:.2}, {:.1}, {:.1}, ", m.hcdi, m.vcdi, m.wnd_spd, m.wnd_dr)?;
        write!(w, "{:.2}, {:.1}, {:.2}, {}, ", m.wpt_dst, m.wpt_brg, m.mag_var, if m.afcs_on > 0.5 { "1" } else { "0" })?;
        write!(w, "{}, {}, {:.1}, {:.1}, {:.1}, ", "NONE", "NONE", m.roll_c, m.pitch_c, m.v_spd_g)?;
        writeln!(w, "{}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}", "3DDiff", m.hal, m.val, m.hpl_was, m.hpl_fd, m.vpl_was)
    }
}
