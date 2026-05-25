use crate::airports::AirportsDatabase;
use crate::flight_log_manager::{init_sqlite_db, insert_sqlite_row};
use crate::models::{AircraftInfo, FlightMetrics, WebhookFlightSummary, AirportInfo, ClosestAirportInfo};
use crate::sim_monitor::{calculate_distance, SimMonitor};
use crate::webhook_manager::WebhookManager;
use crate::runways::RunwaysDatabase;
use crate::aircraft_characteristics::{AircraftCharacteristic, CharacteristicsDatabase};
use chrono::Utc;
use rusqlite::{params, Connection};
use simplesimconnect::*;
use simplesimconnect_sys::*;
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub struct SimConnectMonitor {
    metrics: Arc<Mutex<FlightMetrics>>,
    aircraft_info: Arc<Mutex<AircraftInfo>>,
    current_flight_id: Arc<Mutex<String>>,
    running: Arc<Mutex<bool>>,
    connected: Arc<Mutex<bool>>,
    monitoring: Arc<Mutex<bool>>,
    remote_aircraft_sender: Arc<Mutex<Option<std::sync::mpsc::Sender<RemoteAircraftUpdate>>>>,
    available_aircraft: Arc<Mutex<Vec<String>>>,
    available_helicopters: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone)]
pub struct RemoteAircraftUpdate {
    pub id: String,
    pub title: String,
    pub atc_model: String,
    pub object_class: String,
    pub category: String,
    pub num_engines: i32,
    pub engine_type: String,
    pub metrics: FlightMetrics,
}

impl SimConnectMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(FlightMetrics::default())),
            aircraft_info: Arc::new(Mutex::new(AircraftInfo::default())),
            current_flight_id: Arc::new(Mutex::new(String::new())),
            running: Arc::new(Mutex::new(false)),
            connected: Arc::new(Mutex::new(false)),
            monitoring: Arc::new(Mutex::new(false)),
            remote_aircraft_sender: Arc::new(Mutex::new(None)),
            available_aircraft: Arc::new(Mutex::new(Vec::new())),
            available_helicopters: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn run_monitor(
        app: &AppHandle,
        sc: SimConnect,
        metrics: &Arc<Mutex<FlightMetrics>>,
        aircraft_info_mutex: &Arc<Mutex<AircraftInfo>>,
        current_flight_id_mutex: &Arc<Mutex<String>>,
        running: &Arc<Mutex<bool>>,
        monitoring: &Arc<Mutex<bool>>,
        remote_receiver: std::sync::mpsc::Receiver<RemoteAircraftUpdate>,
        _requested_log_path: Option<&PathBuf>,
        available_aircraft: &Arc<Mutex<Vec<String>>>,
        available_helicopters: &Arc<Mutex<Vec<String>>>,
    ) -> anyhow::Result<()> {
        let define_id = 1;
        let aircraft_define_id = 2;
        let remote_define_id = 3;
        let request_id = 1;
        let aircraft_request_id = 2;
        let event_sim_start = 1;
        let event_sim_stop = 2;

        sc.subscribe_to_system_event(event_sim_start, "SimStart")?;
        sc.subscribe_to_system_event(event_sim_stop, "SimStop")?;

        // Enumerate local aircraft and liveries for multiplayer mapping
        let _ = sc.enumerate_sim_objects_and_liveries(9001, SIMCONNECT_SIMOBJECT_TYPE_SIMCONNECT_SIMOBJECT_TYPE_AIRCRAFT);
        let _ = sc.enumerate_sim_objects_and_liveries(9002, SIMCONNECT_SIMOBJECT_TYPE_SIMCONNECT_SIMOBJECT_TYPE_HELICOPTER);

        // Register all fields for user aircraft
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
        sc.add_to_data_definition::<f64>(define_id, "RECIP ENG TURBINE INLET TEMPERATURE:1", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "RECIP ENG TURBINE INLET TEMPERATURE:2", "farenheit")?;
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
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT BANK HOLD", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT PITCH HOLD", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT BANK HOLD REF", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT PITCH HOLD REF", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT VERTICAL HOLD VAR", "feet per minute")?;
        sc.add_to_data_definition::<f64>(define_id, "SIM ON GROUND", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "PLANE ALT ABOVE GROUND", "feet")?;
        sc.add_to_data_definition::<f64>(define_id, "G FORCE", "gforce")?;
        sc.add_to_data_definition::<f64>(define_id, "PRESSURE ALTITUDE", "feet")?;
        sc.add_to_data_definition::<f64>(define_id, "DENSITY ALTITUDE", "feet")?;
        sc.add_to_data_definition::<f64>(define_id, "PRESSURIZATION CABIN ALTITUDE", "feet")?;

        sc.add_to_data_definition::<f64>(define_id, "G FORCE", "gforce")?; // dummy for prop rpm
        sc.add_to_data_definition::<f64>(define_id, "G FORCE", "gforce")?; // dummy for gear ratio

        sc.add_string256_to_data_definition::<[u8; 256]>(aircraft_define_id, "TITLE")?;
        sc.add_string256_to_data_definition::<[u8; 256]>(aircraft_define_id, "ATC MODEL")?;
        sc.add_string256_to_data_definition::<[u8; 256]>(aircraft_define_id, "ATC ID")?;
        sc.add_string256_to_data_definition::<[u8; 256]>(aircraft_define_id, "AIRCRAFT OBJECT CLASS")?;
        sc.add_string256_to_data_definition::<[u8; 256]>(aircraft_define_id, "CATEGORY")?;
        sc.add_to_data_definition::<f64>(aircraft_define_id, "NUMBER OF ENGINES", "Number")?;
        sc.add_to_data_definition::<f64>(aircraft_define_id, "ENGINE TYPE", "Enum")?;

        // Define data structure for remote aircraft
        #[repr(C)]
        #[derive(Debug, Clone, Copy)]
        struct RemoteAircraftData {
            latitude: f64,
            longitude: f64,
            altitude: f64,
            pitch: f64,
            bank: f64,
            heading: f64,
        }

        sc.add_to_data_definition::<f64>(remote_define_id, "PLANE LATITUDE", "degrees")?;
        sc.add_to_data_definition::<f64>(remote_define_id, "PLANE LONGITUDE", "degrees")?;
        sc.add_to_data_definition::<f64>(remote_define_id, "PLANE ALTITUDE", "feet")?;
        sc.add_to_data_definition::<f64>(remote_define_id, "PLANE PITCH DEGREES", "degrees")?;
        sc.add_to_data_definition::<f64>(remote_define_id, "PLANE BANK DEGREES", "degrees")?;
        sc.add_to_data_definition::<f64>(remote_define_id, "PLANE HEADING DEGREES TRUE", "degrees")?;

        // Initial request for aircraft title
        sc.request_data_on_sim_object(aircraft_request_id, aircraft_define_id, OBJECT_ID_USER, SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_ONCE)?;
        sc.request_data_on_sim_object(request_id, define_id, OBJECT_ID_USER, SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_SECOND)?;

        let mut current_log_path: Option<PathBuf> = None;
        let mut db_conn: Option<Connection> = None;
        let mut aircraft_info = AircraftInfo::default();
        let mut analyzer = crate::flight_analyzer::FlightAnalyzer::new();
        let mut last_log_time = Utc::now();
        let mut flight_ongoing = false;

        let mut takeoff_snapshot: Option<FlightMetrics> = None;
        let mut landing_snapshot: Option<FlightMetrics> = None;
        let mut max_metrics: Option<FlightMetrics> = None;
        let mut takeoff_time: Option<String> = None;
        let mut landing_time: Option<String> = None;
        let mut start_time: Option<String> = None;

        let mut on_ground_since: Option<std::time::Instant> = None;
        let mut stationary_since: Option<std::time::Instant> = None;
        let mut last_agl = 0.0;
        let mut touchdown_time: Option<std::time::Instant> = None;
        let mut touchdown_update_done = false;
        let mut auto_finalized = false;

        let mut remote_aircraft: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let mut last_update_times: std::collections::HashMap<String, std::time::Instant> = std::collections::HashMap::new();
        let mut pending_requests: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
        let mut next_request_id: u32 = 1000;

        let webhook_manager = app.state::<WebhookManager>();
        webhook_manager.reset();

        {
            let mut m = monitoring.lock().unwrap();
            *m = false;
        }

        loop {
            if !*running.lock().unwrap() {
                break;
            }

            // Handle remote aircraft updates
            while let Ok(update) = remote_receiver.try_recv() {
                last_update_times.insert(update.id.clone(), std::time::Instant::now());
                if let Some(object_id) = remote_aircraft.get(&update.id) {
                    if *object_id != 0 {
                        let data = RemoteAircraftData {
                            latitude: update.metrics.latitude,
                            longitude: update.metrics.longitude,
                            altitude: update.metrics.gps_altitude_msl,
                            pitch: update.metrics.pitch_angle,
                            bank: update.metrics.roll_angle,
                            heading: update.metrics.heading,
                        };
                        
                        unsafe {
                            let handle: HANDLE = std::mem::transmute_copy(&sc);
                            let _ = SimConnect_SetDataOnSimObject(
                                handle,
                                remote_define_id,
                                *object_id,
                                0,
                                0,
                                std::mem::size_of::<RemoteAircraftData>() as DWORD,
                                std::mem::transmute(&data),
                            );
                        }
                    }
                } else {
                    // Create new AI aircraft
                    remote_aircraft.insert(update.id.clone(), 0); // Mark as pending
                    let request_id = next_request_id;
                    next_request_id += 1;
                    pending_requests.insert(request_id, update.id.clone());

                    let raw_title = if update.title.is_empty() { "Cessna Skyhawk G1000".to_string() } else { update.title };
                    let title = {
                        let ac_list = available_aircraft.lock().unwrap();
                        let hc_list = available_helicopters.lock().unwrap();
                        let empty_db = CharacteristicsDatabase { characteristics: std::collections::HashMap::new() };
                        let db_ref = app.try_state::<CharacteristicsDatabase>();
                        let db = db_ref.as_deref().unwrap_or(&empty_db);
                        let mapped = find_best_multiplayer_model(
                            &raw_title,
                            &update.atc_model,
                            &update.object_class,
                            &update.category,
                            update.num_engines,
                            &update.engine_type,
                            &ac_list,
                            &hc_list,
                            db,
                        );
                        if mapped != raw_title {
                            crate::append_log(app, format!("[MSFS] Mapping remote aircraft '{}' (ICAO: {}) to local model '{}'", raw_title, update.atc_model, mapped));
                        }
                        mapped
                    };
                    
                    let init_pos = InitPosition {
                        latitude: update.metrics.latitude,
                        longitude: update.metrics.longitude,
                        altitude: update.metrics.gps_altitude_msl,
                        pitch: update.metrics.pitch_angle,
                        bank: update.metrics.roll_angle,
                        heading: update.metrics.heading,
                        on_ground: if update.metrics.is_on_ground > 0.5 { 1 } else { 0 },
                        airspeed: update.metrics.indicated_airspeed as i32,
                    };
                    let _ = sc.ai_create_non_atc_aircraft(&title, "N-BUTTER", init_pos, request_id);
                }
            }

            while let Some(msg) = sc.get_next_dispatch()? {
                if msg.is_quit() {
                    return Ok(());
                }
                
                // Track assigned object IDs
                if let Some(assigned) = msg.as_assigned_object_id() {
                    if let Some(peer_id) = pending_requests.remove(&assigned.request_id) {
                        remote_aircraft.insert(peer_id, assigned.object_id);
                    }
                }

                // Parse available aircraft / helicopter lists from SimConnect
                if let Some(list) = msg.as_enumerate_simobject_and_livery_list() {
                    let request_id = list._base.dwRequestID;
                    let array_size = list._base.dwArraySize as usize;
                    let entry_number = list._base.dwEntryNumber;
                    let is_first = entry_number == 0;

                    let entries = unsafe {
                        std::slice::from_raw_parts(
                            list.rgData.as_ptr(),
                            array_size,
                        )
                    };

                    if request_id == 9001 {
                        let mut ac = available_aircraft.lock().unwrap();
                        if is_first {
                            ac.clear();
                        }
                        for entry in entries {
                            let title = c_char_array_to_string(&entry.AircraftTitle);
                            if !title.is_empty() && !ac.contains(&title) {
                                ac.push(title);
                            }
                        }
                    } else if request_id == 9002 {
                        let mut hc = available_helicopters.lock().unwrap();
                        if is_first {
                            hc.clear();
                        }
                        for entry in entries {
                            let title = c_char_array_to_string(&entry.AircraftTitle);
                            if !title.is_empty() && !hc.contains(&title) {
                                hc.push(title);
                            }
                        }
                    }
                }

                if msg.is_quit() {
                    return Ok(());
                }
                
                // ... (rest of the handle loop)

                if msg.is_quit() {
                    return Ok(());
                }
                if let Some(exception) = msg.as_exception() {
                    crate::append_log(app, format!("[BUG] SimConnectException:: {} {} {}", exception.exception, exception.send_id, exception.index));
                }

                if let Some(event) = msg.as_event() {
                    if event.event_id == event_sim_start {
                        crate::append_log(app, format!("[{}] Received SimStart event. Starting new flight log.", Utc::now().format("%H:%M:%S")));
                        flight_ongoing = true;
                        sc.request_data_on_sim_object(aircraft_request_id, aircraft_define_id, OBJECT_ID_USER, SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_ONCE)?;
                        {
                            let mut m = monitoring.lock().unwrap();
                            *m = true;
                        }
                        db_conn = None;
                        analyzer.reset();
                        aircraft_info = aircraft_info_mutex.lock().unwrap().clone();
                        webhook_manager.reset();
                        takeoff_snapshot = None;
                        landing_snapshot = None;
                        max_metrics = None;
                        takeoff_time = None;
                        landing_time = None;
                        start_time = Some(Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string());
                        
                        on_ground_since = None;
                        stationary_since = None;
                        last_agl = 0.0;
                        touchdown_time = None;
                        touchdown_update_done = false;

                        // Resumption check
                        let m_lock = metrics.lock().unwrap();
                        let current_m = *m_lock;
                        drop(m_lock);

                        let mut resumed_path = None;
                        if current_m.is_on_ground < 0.5 {
                            // Fetch aircraft title first if possible (SimConnect is async, so we might need a quick poll)
                            // But usually, it's safer to wait for movement detection if title is empty.
                            if !aircraft_info.title.is_empty() {
                                resumed_path = crate::flight_log_manager::try_find_resume_flight(app, &current_m, &aircraft_info.title);
                            }
                        }

                        let app_data_dir = app.path().app_data_dir().unwrap();
                        let internal_log_dir = app_data_dir.join("flightlogs");
                        let _ = create_dir_all(&internal_log_dir);
                        
                        let (path, filename) = if let Some(ref p) = resumed_path {
                            let f = p.file_name().unwrap().to_string_lossy().to_string();
                            crate::append_log(app, format!("[MSFS] Resuming existing flight log: {}", f));
                            (p.clone(), f)
                        } else {
                            let f = format!("butterlog_{}.db", Utc::now().format("%Y%m%d_%H%M%S"));
                            let p = internal_log_dir.join(&f);
                            (p, f)
                        };

                        current_log_path = Some(path.clone());
                        {
                            let mut fid = current_flight_id_mutex.lock().unwrap();
                            *fid = filename.replace(".db", "");
                        }

                        if let Ok(conn) = Connection::open(&path) {
                            if let Err(e) = init_sqlite_db(&conn) {
                                crate::append_log(app, format!("[MSFS] Error initializing DB: {}", e));
                            }

                            // Restore analyzer state if resuming
                            if current_log_path.as_ref().map(|p| p.to_string_lossy().contains("butterlog_")).unwrap_or(false) && resumed_path.is_some() {
                            if let Err(e) = analyzer.restore(&conn) {
                                crate::append_log(app, format!("[MSFS] Error restoring analyzer: {}", e));
                            } else {
                                start_time = analyzer.first_timestamp.clone();
                                takeoff_time = analyzer.takeoff_timestamp.clone();
                            }
                            }
                            // Set initial departure if on ground
                            let m_lock = metrics.lock().unwrap();
                            if m_lock.is_on_ground > 0.5 || m_lock.altitude_agl < 10.0 {
                                if let Some(db) = app.try_state::<AirportsDatabase>() {
                                    if let Some(nearest) = db.find_nearest(m_lock.latitude, m_lock.longitude, 1).first() {
                                        crate::append_log(app, format!("[MSFS] Identified departure: {} ({}) ({},{})", nearest.ident, nearest.name, m_lock.latitude, m_lock.longitude));
                                        if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('departure_icao', ?1)", params![nearest.ident]) {
                                            crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                        }
                                        if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('departure_name', ?1)", params![nearest.name]) {
                                            crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                        }
                                    }
                                }
                            }

                            // Set aircraft title if already known
                            if !aircraft_info.title.is_empty() {
                                crate::append_log(app, format!("[MSFS] Set aircraft title: {}", aircraft_info.title));
                                if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('aircraft_title', ?1)", params![aircraft_info.title]) {
                                    crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                }
                                if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('atc_model', ?1)", params![aircraft_info.atc_model]) {
                                    crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                }
                                if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('atc_id', ?1)", params![aircraft_info.atc_id]) {
                                    crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                }
                            }

                            db_conn = Some(conn);
                            let _ = app.emit("flight-logs-updated", ());
                        }
                    } else if event.event_id == event_sim_stop {
                        crate::append_log(app, format!("[{}] Received SimStop event. Closing and analyzing flight log.", Utc::now().format("%H:%M:%S")));
                        flight_ongoing = false;
                        {
                            let mut fid = current_flight_id_mutex.lock().unwrap();
                            fid.clear();
                        }
                        {
                            let mut m = monitoring.lock().unwrap();
                            *m = false;
                        }

                        if let Some(ref conn) = db_conn {
                            if let Some(db) = app.try_state::<AirportsDatabase>() {
                                // Advanced Landing Analysis
                                if let Some(r_db) = app.try_state::<RunwaysDatabase>() {
                                    analyzer.finalize_landing_performance(&db, &r_db, Some(conn));
                                }

                                 let start_icao = analyzer.find_start_icao(&db);
                                 let start_name = db.get_by_ident(&start_icao).map(|a| a.name.clone()).unwrap_or_default();
                                 let end_icao = analyzer.find_end_icao(&db);
                                 let end_name = db.get_by_ident(&end_icao).map(|a| a.name.clone()).unwrap_or_default();
                                 
                                 // Final Webhook Sync
                                 if takeoff_time.is_some() {
                                     let landing_event = analyzer.events.iter().find(|e| e.event_type == "landing");
                                     let current_snap = metrics.lock().map(|m| *m).ok();
                                     let closest_airport = if let Some(ref curr) = current_snap {
                                         let lat = curr.latitude;
                                         let lon = curr.longitude;
                                         let nearest = db.find_nearest(lat, lon, 1);
                                         nearest.first().map(|airport| {
                                             let dist = calculate_distance(lat, lon, airport.latitude_deg.unwrap_or(0.0), airport.longitude_deg.unwrap_or(0.0));
                                             ClosestAirportInfo {
                                                 icao: airport.ident.clone(),
                                                 name: airport.name.clone(),
                                                 distance: dist,
                                             }
                                         })
                                     } else {
                                         None
                                     };

                                     let summary = WebhookFlightSummary {
                                         log_path: current_log_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                                         airframe_name: aircraft_info.title.clone(),
                                         atc_model: aircraft_info.atc_model.clone(),
                                         atc_id: aircraft_info.atc_id.clone(),
                                         simulator: "MSFS".to_string(),
                                         simulator_version: "SimConnect".to_string(),
                                         departure: AirportInfo { icao: start_icao.clone(), name: start_name },
                                         arrival: AirportInfo { icao: end_icao.clone(), name: end_name },
                                         closest_airport,
                                         takeoff_time: takeoff_time.clone(),
                                         landing_time: landing_time.clone(),
                                         start_time: start_time.clone(),
                                         end_time: Some(Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()),
                                         takeoff_snapshot,
                                         landing_snapshot,
                                         current_snapshot: current_snap,
                                         max_entries: max_metrics,
                                         vs_variance: landing_event.and_then(|e| e.vs_variance),
                                         ias_variance: landing_event.and_then(|e| e.ias_variance),
                                         landing_score: landing_event.and_then(|e| e.calculate_landing_score()),
                                         landing_offset_percent: landing_event.and_then(|e| e.offset_percent),
                                         landing_threshold_dist_ft: landing_event.and_then(|e| e.threshold_dist_ft),
                                     };
                                    let app_c = app.clone();
                                    let sum_c = summary.clone();
                                    tauri::async_runtime::spawn(async move {
                                        app_c.state::<WebhookManager>().sync_flight(&app_c, &sum_c, true).await;
                                    });
                                    webhook_manager.reset();
                                }

                                let start_name = db.get_by_ident(&start_icao).map(|a| a.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                                let end_name = db.get_by_ident(&end_icao).map(|a| a.name.clone()).unwrap_or_else(|| "Unknown".to_string());

                                let mut summary_data = vec![
                                    ("departure_icao", start_icao.clone()),
                                    ("departure_name", start_name),
                                    ("arrival_icao", end_icao.clone()),
                                    ("arrival_name", end_name),
                                    ("aircraft_title", aircraft_info.title.clone()),
                                    ("atc_model", aircraft_info.atc_model.clone()),
                                    ("atc_id", aircraft_info.atc_id.clone()),
                                    ("max_altitude", analyzer.max_alt.to_string()),
                                    ("max_ground_speed", analyzer.max_gs.to_string()),
                                    ("fuel_consumed", (analyzer.initial_fuel - analyzer.final_fuel).to_string()),
                                ];

                                if let Some(landing) = analyzer.events.iter().find(|e| e.event_type == "landing") {
                                    if let Some(v) = landing.touchdown_fpm { summary_data.push(("touchdown_fpm", v.to_string())); }
                                    if let Some(v) = landing.landing_g { summary_data.push(("landing_g", v.to_string())); }
                                    if let Some(v) = landing.offset_percent { summary_data.push(("landing_offset_pct", v.to_string())); }
                                    if let Some(v) = landing.threshold_dist_ft { summary_data.push(("landing_dist_ft", v.to_string())); }
                                    if let Some(v) = landing.vs_variance { summary_data.push(("vs_variance", v.to_string())); }
                                    if let Some(v) = landing.ias_variance { summary_data.push(("ias_variance", v.to_string())); }
                                    if let Some(v) = landing.calculate_landing_score() { summary_data.push(("landing_score", v.to_string())); }
                                }

                                for (k, v) in summary_data {
                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params![k, v]) {
                                        crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                    }
                                }
                                crate::append_log(app, "[MSFS] Saved final flight summary to database.".to_string());

                                if let Ok(events_json) = serde_json::to_string(&analyzer.events) {
                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["flight_events", events_json]) {
                                        crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                    }
                                }

                                // Final update to analyzer to ensure duration and max values are accurate
                                let final_ts = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                                let m = metrics.lock().unwrap();
                                analyzer.update(&m, &final_ts);
                                drop(m);

                                let fuel_consumed = analyzer.initial_fuel - analyzer.final_fuel;
                                let duration_mins = analyzer.get_duration_minutes();
                                if let Err(e) = crate::flight_log_manager::update_aircraft_stats(app, &aircraft_info.title, duration_mins as f64, fuel_consumed, &end_icao, true) {
                                    crate::append_log(app, format!("[MSFS] Error updating aircraft stats: {}", e));
                                }

                                drop(db_conn.take());

                                // Cleanup short or empty flights
                                let duration_mins = analyzer.get_duration_minutes();
                                let has_movement = analyzer.max_gs > 5.0 || analyzer.max_alt > 50.0;
                                let is_very_short = duration_mins < 2;
                                
                                if is_very_short || !has_movement {
                                    if let Some(path) = current_log_path.take() {
                                        let _ = std::fs::remove_file(&path);
                                        crate::append_log(app, format!("[MSFS] Deleted short/empty flight log: {}", path.display()));
                                    }
                                }

                                let _ = app.emit("flight-logs-updated", ());
                            }
                        }
                        db_conn = None;
                    }
                }

                if msg.request_id() == Some(aircraft_request_id) {
                    //crate::append_log(app, format!("[MSFS] aircraft_request_id {}", aircraft_request_id));
                    #[repr(C)]
                    #[derive(Debug, Clone, Copy)]
                    struct SimConnectAircraftInfo {
                        title: [u8; 256],
                        atc_model: [u8; 256],
                        atc_id: [u8; 256],
                        object_class: [u8; 256],
                        category: [u8; 256],
                        number_of_engines: f64,
                        engine_type: f64,
                    }
                    if let Some(data) = msg.as_sim_object_data::<SimConnectAircraftInfo>() {
                        let title = String::from_utf8_lossy(&data.title).split('\0').next().unwrap_or("").trim().to_string();
                        let atc_model = String::from_utf8_lossy(&data.atc_model).split('\0').next().unwrap_or("").trim().to_string();
                        let atc_id = String::from_utf8_lossy(&data.atc_id).split('\0').next().unwrap_or("").trim().to_string();
                        let object_class = String::from_utf8_lossy(&data.object_class).split('\0').next().unwrap_or("").trim().to_string();
                        let category = String::from_utf8_lossy(&data.category).split('\0').next().unwrap_or("").trim().to_string();
                        let num_engines = data.number_of_engines as i32;
                        let engine_type = match data.engine_type as i32 {
                            0 => "piston".to_string(),
                            1 => "jet".to_string(),
                            5 => "turboprop".to_string(),
                            3 => "jet".to_string(), // Helo turbine
                            _ => "unknown".to_string(),
                        };

                        if !title.is_empty() {
                            aircraft_info.title = title.clone();
                            aircraft_info.atc_model = atc_model.clone();
                            aircraft_info.atc_id = atc_id.clone();
                            aircraft_info.object_class = object_class.clone();
                            aircraft_info.category = category.clone();
                            aircraft_info.num_engines = num_engines;
                            aircraft_info.engine_type = engine_type.clone();
                            
                            if let Some(ref conn) = db_conn {
                                crate::append_log(app, format!("[MSFS] Set aircraft title: {} [Model: {}, ID: {}]", title, atc_model, atc_id));
                                if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('aircraft_title', ?1)", params![title.clone()]) {
                                    crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                }
                                if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('atc_model', ?1)", params![atc_model.clone()]) {
                                    crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                }
                                if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('atc_id', ?1)", params![atc_id.clone()]) {
                                    crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                }
                            }

                            let mut info = aircraft_info_mutex.lock().unwrap();
                            info.title = title;
                            info.atc_model = atc_model;
                            info.atc_id = atc_id;
                            info.object_class = object_class;
                            info.category = category;
                            info.num_engines = num_engines;
                            info.engine_type = engine_type;
                        }
                    }
                }

                if let Some(data_ref) = msg.as_sim_object_data::<FlightMetrics>() {
                    let data_val = *data_ref;
                    //println!("Received data: {:?}", serde_json::to_string(&data_val).unwrap_or_else(|_| "Failed to serialize".into()) );
                    let data = &data_val;

                    if msg.request_id() == Some(request_id) {
                        {
                            let mut m = metrics.lock().unwrap();
                            *m = *data;
                        }

                        if data.latitude == 0.0 && data.longitude == 0.0 {
                            // Invalid GPS data, skip processing
                            continue;
                        }

                        if !flight_ongoing && data.ground_speed > 10.0 {
                            flight_ongoing = true;
                            
                            sc.request_data_on_sim_object(aircraft_request_id, aircraft_define_id, OBJECT_ID_USER, SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_ONCE)?;

                            { let mut m = monitoring.lock().unwrap(); *m = true; }
                            db_conn = None;
                            analyzer.reset();
                            aircraft_info = aircraft_info_mutex.lock().unwrap().clone();
                            webhook_manager.reset();
                            max_metrics = None;
                            start_time = Some(Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string());


                            
                            let mut resumed_path = None;
                            if !aircraft_info.title.is_empty() {
                                resumed_path = crate::flight_log_manager::try_find_resume_flight(app, data, &aircraft_info.title);
                            }
                            
                            
                            let app_data_dir = app.path().app_data_dir().unwrap();
                            let internal_log_dir = app_data_dir.join("flightlogs");
                            let _ = create_dir_all(&internal_log_dir);
                            
                            let (path, filename) = if let Some(ref p) = resumed_path {
                                let f = p.file_name().unwrap().to_string_lossy().to_string();
                                crate::append_log(app, format!("[MSFS] Resuming existing flight log: {}", f));
                                (p.clone(), f)
                            } else {
                                let f = format!("butterlog_{}.db", Utc::now().format("%Y%m%d_%H%M%S"));
                                let p = internal_log_dir.join(&f);
                                crate::append_log(app, format!("[MSFS] Aircraft movement detected (GS > 10.0). Starting fallback flight log: {}", p.display()));
                                (p, f)
                            };

                            current_log_path = Some(path.clone());
                            {
                                let mut fid = current_flight_id_mutex.lock().unwrap();
                                *fid = filename.replace(".db", "");
                            }
                            if let Ok(conn) = Connection::open(&path) {
                                if let Err(e) = init_sqlite_db(&conn) {
                                    crate::append_log(app, format!("[MSFS] Error initializing DB: {}", e));
                                }

                                // Set initial departure if on ground
                                if data.is_on_ground > 0.5 {
                                    if let Some(db) = app.try_state::<AirportsDatabase>() {
                                        if let Some(nearest) = db.find_nearest(data.latitude, data.longitude, 1).first() {
                                            if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('departure_icao', ?1)", params![nearest.ident]) {
                                                crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                            }
                                            if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('departure_name', ?1)", params![nearest.name]) {
                                                crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                            }
                                        }
                                    }
                                }

                                // Set aircraft title if already known
                                if !aircraft_info.title.is_empty() {
                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('aircraft_title', ?1)", params![aircraft_info.title]) {
                                        crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                    }
                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('atc_model', ?1)", params![aircraft_info.atc_model]) {
                                        crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                    }
                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('atc_id', ?1)", params![aircraft_info.atc_id]) {
                                        crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                    }
                                }

                                db_conn = Some(conn);
                                let _ = app.emit("flight-logs-updated", ());
                            }
                            
                            // Process first point immediately
                            let _ = analyzer.update(data, start_time.as_ref().unwrap());
                        }

                        if flight_ongoing {
                            // Update max metrics
                            match max_metrics {
                                Some(ref mut m) => m.update_max(data),
                                None => max_metrics = Some(*data),
                            }

                            let now = Utc::now();
                            let now_instant = std::time::Instant::now();
                            let mut sample_rate_ms = 1000;

                            let mut force_update = false;
                            if (last_agl < 100.0 && data.altitude_agl >= 100.0) || (last_agl > 100.0 && data.altitude_agl <= 100.0) {
                                force_update = true;
                            }
                            last_agl = data.altitude_agl;

                            if data.is_on_ground > 0.5 && analyzer.current_phase == crate::models::FlightPhase::Landing {
                                if touchdown_time.is_none() {
                                    touchdown_time = Some(now_instant);
                                } else if touchdown_time.unwrap().elapsed().as_secs() >= 5 && !touchdown_update_done {
                                    force_update = true;
                                    touchdown_update_done = true;
                                }
                            } else {
                                touchdown_time = None;
                                touchdown_update_done = false;
                            }

                            if data.is_on_ground < 0.5 {
                                if let Some(db) = app.try_state::<AirportsDatabase>() {
                                    if let Some(nearest) = db.find_nearest(data.latitude, data.longitude, 1).first() {
                                        let dist = calculate_distance(data.latitude, data.longitude, nearest.latitude_deg.unwrap_or(0.0), nearest.longitude_deg.unwrap_or(0.0));
                                        if dist <= 5.0 && (data.gps_altitude_msl - nearest.elevation_ft.unwrap_or(0) as f64) <= 500.0 {
                                            sample_rate_ms = 200;
                                        }
                                    }
                                }
                            }

                            if force_update || now.signed_duration_since(last_log_time) >= chrono::Duration::milliseconds(sample_rate_ms) {
                                last_log_time = now;
                                let now_str = now.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                                
                                let mut force_sync = false;
                                if force_update || data.ground_speed.abs() > 0.1 || data.vertical_speed.abs() > 10.0 {
                                    if let Some(new_phase) = analyzer.update(data, &now_str) {
                                        let _ = app.emit("flight-phase-change", new_phase);
                                        if new_phase == crate::models::FlightPhase::Takeoff {
                                            takeoff_snapshot = Some(*data);
                                            takeoff_time = Some(now_str.clone());
                                            force_sync = true;
                                            auto_finalized = false;

                                            // Immediate takeoff event in summary
                                            if let Some(ref conn) = db_conn {
                                                let takeoff_event = crate::models::FlightEvent {
                                                    timestamp: now_str.clone(),
                                                    event_type: "takeoff".to_string(),
                                                    latitude: data.latitude,
                                                    longitude: data.longitude,
                                                    touchdown_fpm: None,
                                                    landing_g: None,
                                                    offset_percent: None,
                                                    threshold_dist_ft: None,
                                                    vs_variance: None,
                                                    ias_variance: None,
                                                    heading: None,
                                                };
                                                if let Ok(event_json) = serde_json::to_string(&vec![takeoff_event]) {
                                                    let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('takeoff_event', ?1)", params![event_json]);
                                                }
                                            }
                                            let _ = app.emit("flight-logs-updated", ());
                                        } else if new_phase == crate::models::FlightPhase::Landing {
                                            landing_snapshot = Some(*data);
                                            landing_time = Some(now_str.clone());
                                            force_sync = true;

                                            // Immediate Arrival detection
                                            if let Some(ref takeoff_ts) = analyzer.takeoff_timestamp {
                                                if let (Some(t_start), Some(t_end)) = (analyzer.parse_timestamp(takeoff_ts), analyzer.parse_timestamp(&now_str)) {
                                                    if t_end - t_start > 60 {
                                                        if let Some(db) = app.try_state::<AirportsDatabase>() {
                                                            if let Some(nearest) = db.find_nearest(data.latitude, data.longitude, 1).first() {
                                                                if let Some(ref conn) = db_conn {
                                                                    crate::append_log(app, format!("[MSFS] Identified arrival: {} ({})", nearest.ident, nearest.name));
                                                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('arrival_icao', ?1)", params![nearest.ident]) {
                                                                        crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                                                    }
                                                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('arrival_name', ?1)", params![nearest.name]) {
                                                                        crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            let _ = app.emit("flight-logs-updated", ());
                                        }
                                    }

                                    if let Some(ref conn) = db_conn {
                                        if let Err(e) = insert_sqlite_row(conn, &now_str, data) {
                                            crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                        }

                                        // Update summary with live metrics
                                        let fuel_consumed = analyzer.initial_fuel - analyzer.final_fuel;
                                        let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('max_altitude', ?1)", params![analyzer.max_alt.to_string()]);
                                        let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('max_ground_speed', ?1)", params![analyzer.max_gs.to_string()]);
                                        let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('fuel_consumed', ?1)", params![fuel_consumed.to_string()]);

                                        if let Ok(events_json) = serde_json::to_string(&analyzer.events) {
                                            let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('flight_events', ?1)", params![events_json]);
                                        }

                                        if let Some(db) = app.try_state::<AirportsDatabase>() {
                                            let arrival_icao = analyzer.find_end_icao(&db);
                                            let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('arrival_icao', ?1)", params![arrival_icao]);
                                            if let Some(airport) = db.get_by_ident(&arrival_icao) {
                                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('arrival_name', ?1)", params![airport.name]);
                                            }
                                        }
                                    }
                                }
                                // Sync Webhook
                                if takeoff_time.is_some() {
                                    if let Some(db) = app.try_state::<AirportsDatabase>() {
                                        let closest_airport = {
                                            let lat = data.latitude;
                                            let lon = data.longitude;
                                            let nearest = db.find_nearest(lat, lon, 1);
                                            nearest.first().map(|airport| {
                                                let dist = calculate_distance(lat, lon, airport.latitude_deg.unwrap_or(0.0), airport.longitude_deg.unwrap_or(0.0));
                                                ClosestAirportInfo {
                                                    icao: airport.ident.clone(),
                                                    name: airport.name.clone(),
                                                    distance: dist,
                                                }
                                            })
                                        };

                                        let summary = WebhookFlightSummary {
                                            log_path: current_log_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                                            airframe_name: aircraft_info.title.clone(),
                                            atc_model: aircraft_info.atc_model.clone(),
                                            atc_id: aircraft_info.atc_id.clone(),
                                            simulator: "MSFS".to_string(),
                                            simulator_version: "SimConnect".to_string(),
                                            departure: AirportInfo { 
                                                icao: analyzer.find_start_icao(&db), 
                                                name: db.get_by_ident(&analyzer.find_start_icao(&db)).map(|a| a.name.clone()).unwrap_or_default()
                                            },
                                            arrival: AirportInfo { 
                                                icao: analyzer.find_end_icao(&db), 
                                                name: db.get_by_ident(&analyzer.find_end_icao(&db)).map(|a| a.name.clone()).unwrap_or_default()
                                            },
                                            closest_airport,
                                            takeoff_time: takeoff_time.clone(),
                                            landing_time: landing_time.clone(),
                                            start_time: start_time.clone(),
                                            end_time: Some(now_str),
                                            takeoff_snapshot,
                                            landing_snapshot,
                                            current_snapshot: Some(*data),
                                            max_entries: max_metrics,
                                            vs_variance: None,
                                            ias_variance: None,
                                            landing_score: None,
                                            landing_offset_percent: None,
                                            landing_threshold_dist_ft: None,
                                        };
                                        let app_c = app.clone();
                                        let sum_c = summary.clone();
                                        tauri::async_runtime::spawn(async move {
                                            app_c.state::<WebhookManager>().sync_flight(&app_c, &sum_c, force_sync).await;
                                        });
                                    }
                                }

                                // Auto-close logic: on ground > 30s or stationary > 10s
                                let now_instant = std::time::Instant::now();
                                if data.is_on_ground > 0.5 {
                                    if on_ground_since.is_none() { on_ground_since = Some(now_instant); }
                                } else {
                                    on_ground_since = None;
                                }

                                if data.ground_speed.abs() < 10.0 {
                                    if stationary_since.is_none() { stationary_since = Some(now_instant); }
                                } else {
                                    stationary_since = None;
                                }

                                let should_close = if takeoff_time.is_some() && landing_time.is_some() {
                                    (if let Some(t) = on_ground_since {
                                        t.elapsed().as_secs() > 30
                                    } else { false }) || (if let Some(t) = stationary_since {
                                        t.elapsed().as_secs() > 10
                                    } else { false })
                                } else { false };

                                if should_close && !auto_finalized {
                                    crate::append_log(app, "[MSFS] Aircraft stationary. Updating flight summary and stats.".to_string());
                                    auto_finalized = true;
                                    
                                    // Finalize logic manually
                                    if let Some(ref conn) = db_conn {
                                        if let Some(db) = app.try_state::<AirportsDatabase>() {
                                            if let Some(r_db) = app.try_state::<RunwaysDatabase>() {
                                                analyzer.finalize_landing_performance(&db, &r_db, Some(conn));
                                            }

                                            let start_icao = analyzer.find_start_icao(&db);
                                            let end_icao = analyzer.find_end_icao(&db);
                                            
                                            if takeoff_time.is_some() {
                                                let landing_event = analyzer.events.iter().find(|e| e.event_type == "landing");
                                                let closest_airport = {
                                                    let lat = data.latitude;
                                                    let lon = data.longitude;
                                                    let nearest = db.find_nearest(lat, lon, 1);
                                                    nearest.first().map(|airport| {
                                                        let dist = calculate_distance(lat, lon, airport.latitude_deg.unwrap_or(0.0), airport.longitude_deg.unwrap_or(0.0));
                                                        ClosestAirportInfo {
                                                            icao: airport.ident.clone(),
                                                            name: airport.name.clone(),
                                                            distance: dist,
                                                        }
                                                    })
                                                };

                                                let summary = WebhookFlightSummary {
                                                    log_path: current_log_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                                                    airframe_name: aircraft_info.title.clone(),
                                                    atc_model: aircraft_info.atc_model.clone(),
                                                    atc_id: aircraft_info.atc_id.clone(),
                                                    simulator: "MSFS".to_string(),
                                                    simulator_version: "SimConnect".to_string(),
                                                    departure: AirportInfo { 
                                                        icao: start_icao.clone(), 
                                                        name: db.get_by_ident(&start_icao).map(|a| a.name.clone()).unwrap_or_default() 
                                                    },
                                                    arrival: AirportInfo { 
                                                        icao: end_icao.clone(), 
                                                        name: db.get_by_ident(&end_icao).map(|a| a.name.clone()).unwrap_or_default() 
                                                    },
                                                    closest_airport,
                                                    takeoff_time: takeoff_time.clone(),
                                                    landing_time: landing_time.clone(),
                                                    start_time: start_time.clone(),
                                                    end_time: Some(Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()),
                                                    takeoff_snapshot: takeoff_snapshot.clone(),
                                                    landing_snapshot: landing_snapshot.clone(),
                                                    current_snapshot: Some(*data),
                                                    max_entries: max_metrics.clone(),
                                                    vs_variance: landing_event.and_then(|e| e.vs_variance),
                                                    ias_variance: landing_event.and_then(|e| e.ias_variance),
                                                    landing_score: landing_event.and_then(|e| e.calculate_landing_score()),
                                                    landing_offset_percent: landing_event.and_then(|e| e.offset_percent),
                                                    landing_threshold_dist_ft: landing_event.and_then(|e| e.threshold_dist_ft),
                                                };
                                                let app_c = app.clone();
                                                let sum_c = summary.clone();
                                                tauri::async_runtime::spawn(async move {
                                                    app_c.state::<WebhookManager>().sync_flight(&app_c, &sum_c, true).await;
                                                });
                                                webhook_manager.reset();
                                            }

                                            let start_name = db.get_by_ident(&start_icao).map(|a| a.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                                            let end_name = db.get_by_ident(&end_icao).map(|a| a.name.clone()).unwrap_or_else(|| "Unknown".to_string());

                                            let mut summary_data = vec![
                                                ("departure_icao", start_icao.clone()),
                                                ("departure_name", start_name),
                                                ("arrival_icao", end_icao.clone()),
                                                ("arrival_name", end_name),
                                                ("aircraft_title", aircraft_info.title.clone()),
                                                ("atc_model", aircraft_info.atc_model.clone()),
                                                ("atc_id", aircraft_info.atc_id.clone()),
                                                ("max_altitude", analyzer.max_alt.to_string()),
                                                ("max_ground_speed", analyzer.max_gs.to_string()),
                                                ("fuel_consumed", (analyzer.initial_fuel - analyzer.final_fuel).to_string()),
                                            ];

                                            if let Some(landing) = analyzer.events.iter().find(|e| e.event_type == "landing") {
                                                if let Some(v) = landing.touchdown_fpm { summary_data.push(("touchdown_fpm", v.to_string())); }
                                                if let Some(v) = landing.landing_g { summary_data.push(("landing_g", v.to_string())); }
                                                if let Some(v) = landing.offset_percent { summary_data.push(("landing_offset_pct", v.to_string())); }
                                                if let Some(v) = landing.threshold_dist_ft { summary_data.push(("landing_dist_ft", v.to_string())); }
                                                if let Some(v) = landing.vs_variance { summary_data.push(("vs_variance", v.to_string())); }
                                                if let Some(v) = landing.ias_variance { summary_data.push(("ias_variance", v.to_string())); }
                                                if let Some(v) = landing.calculate_landing_score() { summary_data.push(("landing_score", v.to_string())); }
                                            }

                                            for (k, v) in summary_data {
                                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params![k, v]);
                                            }

                                            if let Ok(events_json) = serde_json::to_string(&analyzer.events) {
                                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["flight_events", events_json]);
                                            }

                                            // Final update to analyzer to ensure duration and max values are accurate
                                            let final_ts = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                                            analyzer.update(data, &final_ts);

                                            let fuel_consumed = analyzer.initial_fuel - analyzer.final_fuel;
                                            let duration_mins = analyzer.get_duration_minutes();
                                            let _ = crate::flight_log_manager::update_aircraft_stats(app, &aircraft_info.title, duration_mins as f64, fuel_consumed, &end_icao, true);

                                            let _ = app.emit("flight-logs-updated", ());
                                        }
                                    }

                                    on_ground_since = None;
                                    stationary_since = None;
                                }
                            }
                        }
                    }
                }
            }

            // Check for timeout of remote aircraft (45 seconds)
            let now_instant = std::time::Instant::now();
            let mut to_remove = Vec::new();
            for (id, last_time) in &last_update_times {
                if now_instant.duration_since(*last_time) > std::time::Duration::from_secs(45) {
                    to_remove.push(id.clone());
                }
            }

            for id in to_remove {
                if let Some(object_id) = remote_aircraft.remove(&id) {
                    if object_id != 0 {
                        crate::append_log(app, format!("[MSFS] Removing remote aircraft '{}' due to timeout", id));
                        let _ = sc.ai_remove_object(object_id, 0);
                    }
                }
                last_update_times.remove(&id);
            }

            thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }
}

fn c_char_array_to_string(arr: &[std::os::raw::c_char; 256]) -> String {
    let bytes: Vec<u8> = arr.iter().map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes)
        .split('\0')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

fn is_helicopter_title(title: &str) -> bool {
    let lower = title.to_lowercase();
    lower.contains("helicopter")
        || lower.contains("heli")
        || lower.contains("rotor")
        || lower.contains("bell")
        || lower.contains("cabri")
        || lower.contains("robinson")
        || lower.contains("h125")
        || lower.contains("h135")
        || lower.contains("h145")
        || lower.contains("chinook")
        || lower.contains("sikorsky")
        || lower.contains("g2 cabri")
        || lower.contains("coptr")
}

fn share_significant_keyword(a: &str, b: &str) -> bool {
    let a_words: Vec<String> = a.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 2 && w != "helicopter" && w != "heli" && w != "aircraft" && w != "plane")
        .collect();

    for w in b.split(|c: char| !c.is_alphanumeric()) {
        let wl = w.to_lowercase();
        if a_words.contains(&wl) {
            return true;
        }
    }
    false
}

fn matches_generic_profile(
    title: &str,
    engine_type: &str,
    num_engines: i32,
) -> bool {
    let lower = title.to_lowercase();
    match engine_type {
        "jet" => {
            if num_engines >= 2 {
                lower.contains("boeing")
                    || lower.contains("airbus")
                    || lower.contains("737")
                    || lower.contains("747")
                    || lower.contains("777")
                    || lower.contains("787")
                    || lower.contains("a32")
                    || lower.contains("a33")
                    || lower.contains("a35")
                    || lower.contains("a38")
                    || lower.contains("crj")
                    || lower.contains("erj")
                    || lower.contains("md8")
                    || lower.contains("md9")
                    || lower.contains("embraer")
                    || lower.contains("citation")
                    || lower.contains("learjet")
            } else {
                lower.contains("f-16")
                    || lower.contains("f16")
                    || lower.contains("f-18")
                    || lower.contains("f18")
                    || lower.contains("hornet")
                    || lower.contains("vision")
                    || lower.contains("sf50")
                    || lower.contains("l39")
            }
        }
        "turboprop" => {
            if num_engines >= 2 {
                lower.contains("king air")
                    || lower.contains("kingair")
                    || lower.contains("beechcraft")
                    || lower.contains("atr")
                    || lower.contains("dhc-8")
                    || lower.contains("q400")
                    || lower.contains("dash 8")
                    || lower.contains("casa")
                    || lower.contains("twin otter")
                    || lower.contains("twinotter")
            } else {
                lower.contains("caravan")
                    || lower.contains("c208")
                    || lower.contains("tbm")
                    || lower.contains("pilatus")
                    || lower.contains("pc12")
                    || lower.contains("pc-12")
            }
        }
        "piston" => {
            if num_engines >= 2 {
                lower.contains("baron")
                    || lower.contains("seneca")
                    || lower.contains("seminole")
                    || lower.contains("duchess")
                    || lower.contains("beech 58")
                    || lower.contains("cessna 310")
            } else {
                lower.contains("cessna")
                    || lower.contains("172")
                    || lower.contains("152")
                    || lower.contains("piper")
                    || lower.contains("pa28")
                    || lower.contains("pa-28")
                    || lower.contains("archer")
                    || lower.contains("warrior")
                    || lower.contains("cub")
                    || lower.contains("bonanza")
                    || lower.contains("cirrus")
                    || lower.contains("sr22")
                    || lower.contains("sr20")
                    || lower.contains("mooney")
            }
        }
        _ => false,
    }
}


fn find_best_multiplayer_model(
    remote_title: &str,
    remote_atc_model: &str,
    remote_object_class: &str,
    remote_category: &str,
    remote_num_engines: i32,
    remote_engine_type: &str,
    available_aircraft: &[String],
    available_helicopters: &[String],
    db: &CharacteristicsDatabase,
) -> String {
    let remote_lower = remote_title.to_lowercase();

    // 1. Check for case-insensitive exact match in aircraft
    for ac in available_aircraft {
        if ac.to_lowercase() == remote_lower {
            return ac.clone();
        }
    }

    // 2. Check for case-insensitive exact match in helicopters
    for hc in available_helicopters {
        if hc.to_lowercase() == remote_lower {
            return hc.clone();
        }
    }

    // 3. Try to match by ICAO (atc_model) as a substring in aircraft/helicopter titles
    if !remote_atc_model.is_empty() {
        let atc_lower = remote_atc_model.to_lowercase();
        for ac in available_aircraft {
            if ac.to_lowercase().contains(&atc_lower) {
                return ac.clone();
            }
        }
        for hc in available_helicopters {
            if hc.to_lowercase().contains(&atc_lower) {
                return hc.clone();
            }
        }
    }

    // 4. Helicopter matching heuristics (Category/ObjectClass or Title keywords)
    let is_hc = remote_object_class.to_lowercase() == "helicopter"
        || remote_category.to_lowercase() == "helicopter"
        || is_helicopter_title(remote_title);

    if is_hc && !available_helicopters.is_empty() {
        for hc in available_helicopters {
            let hc_lower = hc.to_lowercase();
            if hc_lower.contains(&remote_lower) || remote_lower.contains(&hc_lower) {
                return hc.clone();
            }
        }
        for hc in available_helicopters {
            if share_significant_keyword(remote_title, hc) {
                return hc.clone();
            }
        }
        return available_helicopters[0].clone();
    }

    // 5. Try to match using database characteristics (WTC, engine type, etc.)
    let remote_char = if !remote_atc_model.is_empty() {
        db.characteristics.get(&remote_atc_model.to_uppercase()).cloned()
    } else {
        if !remote_engine_type.is_empty() {
            Some(AircraftCharacteristic {
                icao_code: "".to_string(),
                manufacturer: "".to_string(),
                model_faa: "".to_string(),
                model_bada: "".to_string(),
                engine_type: remote_engine_type.to_lowercase(),
                num_engines: remote_num_engines,
                wtc: if remote_num_engines >= 2 && remote_engine_type.to_lowercase() == "jet" { "Medium".to_string() } else { "Light".to_string() },
                class: "Fixed-wing".to_string(),
            })
        } else {
            None
        }
    };

    if let Some(r_char) = remote_char {
        let list_to_search = if is_hc { available_helicopters } else { available_aircraft };
        let mut best_score = -1;
        let mut best_match = None;
        
        for ac in list_to_search {
            if let Some(l_char) = db.resolve_title_characteristics(ac) {
                let score = db.calculate_similarity_score(&r_char, &l_char);
                if score > best_score {
                    best_score = score;
                    best_match = Some(ac.clone());
                }
            }
        }
        
        if let Some(m) = best_match {
            if best_score >= 100 {
                return m;
            }
        }
    }

    // 6. Try keyword substring matches against available aircraft
    if !available_aircraft.is_empty() {
        for ac in available_aircraft {
            let ac_lower = ac.to_lowercase();
            if ac_lower.contains(&remote_lower) || remote_lower.contains(&ac_lower) {
                return ac.clone();
            }
        }
        for ac in available_aircraft {
            if share_significant_keyword(remote_title, ac) {
                return ac.clone();
            }
        }
    }

    // 7. Generic profile matching (engine type and count fallback)
    if !available_aircraft.is_empty() && !remote_engine_type.is_empty() {
        for ac in available_aircraft {
            if matches_generic_profile(ac, remote_engine_type, remote_num_engines) {
                return ac.clone();
            }
        }
    }

    // 7. Fallback cascade
    if !available_aircraft.is_empty() {
        for ac in available_aircraft {
            let ac_low = ac.to_lowercase();
            if ac_low.contains("cessna skyhawk") || ac_low.contains("172") {
                return ac.clone();
            }
        }
        return available_aircraft[0].clone();
    }

    if !available_helicopters.is_empty() {
        return available_helicopters[0].clone();
    }

    "Cessna Skyhawk G1000".to_string()
}

impl SimMonitor for SimConnectMonitor {
    fn id(&self) -> &'static str { "msfs" }
    fn start(&self, app: AppHandle, log_path: Option<PathBuf>) -> anyhow::Result<()> {
        let mut running = self.running.lock().unwrap();
        if *running { return Ok(()); }
        *running = true;
        
        let metrics = self.metrics.clone();
        let aircraft_info = self.aircraft_info.clone();
        let current_flight_id = self.current_flight_id.clone();
        let running_clone = self.running.clone();
        let connected_clone = self.connected.clone();
        let monitoring_clone = self.monitoring.clone();
        let remote_aircraft_sender = self.remote_aircraft_sender.clone();
        let available_aircraft = self.available_aircraft.clone();
        let available_helicopters = self.available_helicopters.clone();
        
        thread::spawn({
            let app = app.clone();
            let available_aircraft = available_aircraft.clone();
            let available_helicopters = available_helicopters.clone();
            move || loop {
                if !*running_clone.lock().unwrap() { break; }
                match SimConnect::open("ButterLogV2") {
                    Ok(sc) => {
                        crate::append_log(&app, format!("[{}] Successfully connected to MSFS.", Utc::now().format("%Y-%m-%d %H:%M:%S")));
                        { let mut connected = connected_clone.lock().unwrap(); *connected = true; }
                        
                        let (tx, rx) = std::sync::mpsc::channel();
                        {
                             let mut sender = remote_aircraft_sender.lock().unwrap();
                             *sender = Some(tx);
                        }

                        let _ = Self::run_monitor(
                            &app, 
                            sc, 
                            &metrics, 
                            &aircraft_info, 
                            &current_flight_id, 
                            &running_clone, 
                            &monitoring_clone, 
                            rx, 
                            log_path.as_ref(),
                            &available_aircraft,
                            &available_helicopters,
                        );
                        { let mut connected = connected_clone.lock().unwrap(); *connected = false; }
                        { let mut monitoring = monitoring_clone.lock().unwrap(); *monitoring = false; }
                        { let mut m = metrics.lock().unwrap(); *m = FlightMetrics::default(); }
                        { let mut info = aircraft_info.lock().unwrap(); *info = AircraftInfo::default(); }
                        { let mut fid = current_flight_id.lock().unwrap(); *fid = "".to_string(); }
                    }
                    Err(_) => {}
                }
                thread::sleep(Duration::from_secs(1));
            }
        });
        Ok(())
    }
    fn stop(&self) { let mut running = self.running.lock().unwrap(); *running = false; }
    fn get_metrics(&self) -> FlightMetrics { *self.metrics.lock().unwrap() }
    fn get_aircraft_info(&self) -> crate::models::AircraftInfo { self.aircraft_info.lock().unwrap().clone() }
    fn get_current_flight_id(&self) -> String { self.current_flight_id.lock().unwrap().clone() }
    fn is_connected(&self) -> bool { *self.connected.lock().unwrap() }
    fn is_monitoring(&self) -> bool { *self.monitoring.lock().unwrap() }
    fn update_remote_aircraft(
        &self,
        id: &str,
        title: &str,
        atc_model: &str,
        object_class: &str,
        category: &str,
        num_engines: i32,
        engine_type: &str,
        metrics: &FlightMetrics,
    ) {
        let sender = self.remote_aircraft_sender.lock().unwrap();
        if let Some(tx) = sender.as_ref() {
            let _ = tx.send(RemoteAircraftUpdate {
                id: id.to_string(),
                title: title.to_string(),
                atc_model: atc_model.to_string(),
                object_class: object_class.to_string(),
                category: category.to_string(),
                num_engines,
                engine_type: engine_type.to_string(),
                metrics: *metrics,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_best_multiplayer_model() {
        let db = CharacteristicsDatabase::load_from_csv("../public/aircraft-characteristics.csv")
            .expect("Failed to load characteristics DB for tests");

        let available_aircraft = vec![
            "Cessna Skyhawk G1000".to_string(),
            "Boeing 737-800".to_string(),
            "Airbus A320neo".to_string(),
        ];
        let available_helicopters = vec![
            "Cabri G2".to_string(),
            "Bell 407".to_string(),
        ];

        // Exact match
        assert_eq!(
            find_best_multiplayer_model("Boeing 737-800", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Boeing 737-800"
        );

        // Case-insensitive match
        assert_eq!(
            find_best_multiplayer_model("boeing 737-800", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Boeing 737-800"
        );

        // Helicopter exact match
        assert_eq!(
            find_best_multiplayer_model("Bell 407", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Bell 407"
        );

        // Helicopter keyword fallback
        assert_eq!(
            find_best_multiplayer_model("Bell 206 Helicopter", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Bell 407"
        );

        // Helicopter category fallback (forces helicopter matching even with a non-helo title)
        assert_eq!(
            find_best_multiplayer_model("MyCrazyRotorcraft", "", "helicopter", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Cabri G2"
        );

        // Helicopter generic fallback
        assert_eq!(
            find_best_multiplayer_model("Rotorway Heli", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Cabri G2"
        );

        // Substring aircraft match
        assert_eq!(
            find_best_multiplayer_model("A320", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Airbus A320neo"
        );

        // ICAO (atc_model) match
        assert_eq!(
            find_best_multiplayer_model("Some Weird Livery Name", "A320", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Airbus A320neo"
        );

        // Generic profile matching (twin jet -> Boeing 737-800 because it is the first matching jet in the list)
        assert_eq!(
            find_best_multiplayer_model("Weird Twin Jet 5000", "", "", "", 2, "jet", &available_aircraft, &available_helicopters, &db),
            "Boeing 737-800"
        );

        // Generic profile matching (single piston -> Cessna Skyhawk G1000)
        assert_eq!(
            find_best_multiplayer_model("Heavy Single Piston 3000", "", "", "", 1, "piston", &available_aircraft, &available_helicopters, &db),
            "Cessna Skyhawk G1000"
        );

        // CSV database similarity match (remote ICAO A19N matches Airbus A320neo because both are twin jets in same wake class)
        assert_eq!(
            find_best_multiplayer_model("Airbus A319 Neo", "A19N", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Airbus A320neo"
        );

        // Sensible aircraft default fallback
        assert_eq!(
            find_best_multiplayer_model("F-18 Hornet", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Cessna Skyhawk G1000"
        );

        // Ultimate fallback
        let empty_ac = vec![];
        let empty_hc = vec![];
        assert_eq!(
            find_best_multiplayer_model("F-18 Hornet", "", "", "", 0, "", &empty_ac, &empty_hc, &db),
            "Cessna Skyhawk G1000"
        );
    }
}

