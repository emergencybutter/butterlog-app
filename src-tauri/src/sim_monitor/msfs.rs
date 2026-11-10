use crate::models::{AircraftInfo, FlightMetrics};
use crate::sim_monitor::SimMonitor;
use crate::aircraft_characteristics::{AircraftCharacteristic, CharacteristicsDatabase};
use simplesimconnect::*;
use simplesimconnect_sys::*;
use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Simvars in the injected-aircraft (`remote_define_id`) data definition, in
/// registration order. Used to label SimConnect exceptions by datum name.
/// MUST stay in sync with the `add_to_data_definition` calls below.
const REMOTE_DATUM_NAMES: &[&str] = &[
    "PLANE LATITUDE",
    "PLANE LONGITUDE",
    "PLANE ALTITUDE",
    "PLANE PITCH DEGREES",
    "PLANE BANK DEGREES",
    "PLANE HEADING DEGREES TRUE",
];

pub struct SimConnectMonitor {
    metrics: Arc<Mutex<FlightMetrics>>,
    aircraft_info: Arc<Mutex<AircraftInfo>>,
    running: Arc<Mutex<bool>>,
    connected: Arc<Mutex<bool>>,
    remote_aircraft_sender: Arc<Mutex<Option<std::sync::mpsc::Sender<RemoteAircraftUpdate>>>>,
    available_aircraft: Arc<Mutex<Vec<String>>>,
    available_helicopters: Arc<Mutex<Vec<String>>>,
    /// Memoized title -> characteristics index for multiplayer model matching, rebuilt only
    /// when the installed library or characteristics DB changes (see `ModelMatchIndex`).
    model_match_index: Arc<Mutex<Option<ModelMatchIndex>>>,
}

/// Cached title -> resolved-characteristics map plus the input sizes it was built from, so a
/// burst of peers resolving against the same installed library reuses one scan instead of
/// re-resolving every local title per peer. Lengths are a cheap staleness key: the available
/// lists and the DB only grow as they load, then stay fixed for the session.
struct ModelMatchIndex {
    aircraft_len: usize,
    helicopter_len: usize,
    db_len: usize,
    title_chars: Arc<std::collections::HashMap<String, Option<AircraftCharacteristic>>>,
}

#[derive(Clone)]
pub struct RemoteAircraftUpdate {
    pub id: String,
    pub identity: crate::sim_monitor::RemoteAircraftIdentity,
    pub metrics: FlightMetrics,
    /// Local model the multiplayer layer resolved for this peer, or `None` while
    /// resolution is still pending. The dispatch loop spawns with this directly rather
    /// than re-scanning the installed library on the SimConnect thread.
    pub chosen_model: Option<String>,
}

impl SimConnectMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(FlightMetrics::default())),
            aircraft_info: Arc::new(Mutex::new(AircraftInfo::default())),
            running: Arc::new(Mutex::new(false)),
            connected: Arc::new(Mutex::new(false)),
            remote_aircraft_sender: Arc::new(Mutex::new(None)),
            available_aircraft: Arc::new(Mutex::new(Vec::new())),
            available_helicopters: Arc::new(Mutex::new(Vec::new())),
            model_match_index: Arc::new(Mutex::new(None)),
        }
    }

    /// Return the memoized title -> characteristics index, rebuilding it only when the
    /// installed library or DB has changed. The expensive scan runs at most once per
    /// snapshot, so a startup burst of peers shares a single build.
    fn title_characteristics_index(
        &self,
        ac_list: &[String],
        hc_list: &[String],
        db: &CharacteristicsDatabase,
    ) -> Arc<std::collections::HashMap<String, Option<AircraftCharacteristic>>> {
        let mut cache = self.model_match_index.lock();
        let stale = match cache.as_ref() {
            Some(idx) => {
                idx.aircraft_len != ac_list.len()
                    || idx.helicopter_len != hc_list.len()
                    || idx.db_len != db.characteristics.len()
            }
            None => true,
        };
        if stale {
            let mut titles: Vec<String> = Vec::with_capacity(ac_list.len() + hc_list.len());
            titles.extend_from_slice(ac_list);
            titles.extend_from_slice(hc_list);
            let title_chars = Arc::new(db.build_title_index(&titles));
            *cache = Some(ModelMatchIndex {
                aircraft_len: ac_list.len(),
                helicopter_len: hc_list.len(),
                db_len: db.characteristics.len(),
                title_chars,
            });
        }
        cache.as_ref().unwrap().title_chars.clone()
    }

    fn run_monitor(
        app: &AppHandle,
        sc: SimConnect,
        metrics: &Arc<Mutex<FlightMetrics>>,
        aircraft_info_mutex: &Arc<Mutex<AircraftInfo>>,
        running: &Arc<Mutex<bool>>,
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

        sc.add_to_data_definition::<f64>(define_id, "G FORCE", "gforce")?; // dummy for xp_prop_rpm
        // Real landing-gear extension (0..1) into the xp_gear_ratio slot so it
        // propagates to peers. In "percent", not "percent over 100": the variable is
        // already a fraction, and "percent over 100" read 0.01 with the gear down, which
        // X-Plane peers drew as up.
        sc.add_to_data_definition::<f64>(define_id, "GEAR TOTAL PCT EXTENDED", "percent")?;
        sc.add_to_data_definition::<f64>(define_id, "BRAKE PARKING POSITION", "bool")?;
        // Exterior light states (match the trailing FlightMetrics fields).
        sc.add_to_data_definition::<f64>(define_id, "LIGHT NAV", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "LIGHT BEACON", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "LIGHT STROBE", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "LIGHT TAXI", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "LIGHT LANDING", "bool")?;

        sc.add_string256_to_data_definition::<[u8; 256]>(aircraft_define_id, "TITLE")?;
        sc.add_string256_to_data_definition::<[u8; 256]>(aircraft_define_id, "LIVERY NAME")?;
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
        // NOTE: We propagate gear positions and light states to injected AI.
        // We write to individual gear positions (GEAR LEFT/RIGHT/CENTER POSITION)
        // instead of GEAR HANDLE POSITION to avoid the re-seating/levitation bug.
        // We also call SimConnect_AIReleaseControl after spawning to let MSFS manage
        // ground clamping physics. Gear and lights are also propagated to X-Plane peers.

        // Initial request for aircraft title
        sc.request_data_on_sim_object(aircraft_request_id, aircraft_define_id, OBJECT_ID_USER, SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_ONCE)?;
        sc.request_data_on_sim_object(request_id, define_id, OBJECT_ID_USER, SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_SECOND)?;

        // Sim-native telemetry, in shadow mode beside the flight log. Its failure never
        // stops the monitor. Dropped with the connection, which ends its session.
        let mut telemetry_sampler = crate::telemetry::msfs::MsfsSampler::register(app, &sc)
            .unwrap_or_else(|e| {
                crate::append_log(app, format!("[Telemetry] MSFS telemetry unavailable: {}", e));
                None
            });

        let mut aircraft_info = AircraftInfo::default();
        // Seen moving since the flight loaded: the aircraft is then asked for again.
        let mut moving = false;

        // Type and airline resolution, as the service does it (`butterlog-identity`). The
        // service's answer replaces this one for traffic peers once the session opens.
        let resolver = butterlog_identity::Resolver::embedded();

        let mut remote_aircraft: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let mut last_update_times: std::collections::HashMap<String, std::time::Instant> = std::collections::HashMap::new();
        let mut last_ai_log: std::collections::HashMap<String, std::time::Instant> = std::collections::HashMap::new();
        // Maps a SetDataOnSimObject packet's send id -> peer id, so SimConnect
        // exceptions can be attributed to the exact inject call + datum.
        let mut inject_sends: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
        let mut pending_requests: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
        let mut next_request_id: u32 = 1000;
        let mut last_msfs_update = std::time::Instant::now();
        // K events mapped so far, name -> client event id. SimConnect wants each
        // event mapped once per connection before it can be transmitted.
        let mut mapped_events: std::collections::HashMap<&'static str, u32> =
            std::collections::HashMap::new();

        loop {
            if !*running.lock() {
                break;
            }

            // Web-issued commands. Executed here rather than from the poller
            // task because SimConnect calls belong to the thread that owns the
            // connection.
            drain_sim_commands(app, &sc, &mut mapped_events);

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
                            // Correlate this packet's send id back to the peer so a
                            // SimConnect exception can be attributed to this call.
                            let mut send_id: DWORD = 0;
                            if SimConnect_GetLastSentPacketID(handle, &mut send_id) >= 0 {
                                if inject_sends.len() >= 4096 {
                                    inject_sends.clear();
                                }
                                inject_sends.insert(send_id, update.id.clone());
                            }
                        }

                        let should_log = last_ai_log.get(&update.id)
                            .map(|t| t.elapsed().as_secs() >= 60)
                            .unwrap_or(true);
                        if should_log {
                            crate::append_log(app, format!(
                                "[MSFS AI] Updating '{}' (obj {}) at Lat={:.4}, Lon={:.4}, Alt={:.0} ft, Hdg={:.0}",
                                update.identity.title, object_id, update.metrics.latitude, update.metrics.longitude,
                                update.metrics.gps_altitude_msl, update.metrics.heading
                            ));
                            last_ai_log.insert(update.id.clone(), std::time::Instant::now());
                        }
                    }
                } else {
                    // Create new AI aircraft. The local model was already resolved off the
                    // SimConnect thread by the multiplayer resolver pool; scanning the
                    // installed library here would stall dispatch and serialize the startup
                    // burst of peers. If it isn't ready yet, skip — a fresh update arrives
                    // every 50ms, so we'll spawn on a later tick once a model is available.
                    let title = match update.chosen_model.as_deref() {
                        Some(t) if !t.is_empty() => t.to_string(),
                        _ => continue,
                    };

                    remote_aircraft.insert(update.id.clone(), 0); // Mark as pending
                    let request_id = next_request_id;
                    next_request_id += 1;
                    pending_requests.insert(request_id, update.id.clone());

                    crate::append_log(app, format!(
                        "[MSFS AI] Spawning '{}' for peer {} at Lat={:.4}, Lon={:.4}, Alt={:.0} ft",
                        title, update.id, update.metrics.latitude, update.metrics.longitude, update.metrics.gps_altitude_msl
                    ));
                    last_ai_log.insert(update.id.clone(), std::time::Instant::now());

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

            while let Some(msg) = get_next_dispatch_with_retry(&sc, app)? {
                if msg.is_quit() {
                    return Err(anyhow::anyhow!("SimConnect connection lost: simulator exited (received QUIT message)"));
                }

                if let Some(ts) = telemetry_sampler.as_mut() {
                    if let Some(info) = msg.as_open() {
                        ts.on_open(info);
                    }
                    // Before anything reads the message as a `FlightMetrics`, which these
                    // shorter messages aren't.
                    if msg.request_id() == Some(crate::telemetry::msfs::REQUEST_ID) {
                        if let Some(data) = msg.as_tagged_f64() {
                            ts.on_data(&sc, &data);
                        }
                        continue;
                    }
                    if ts.on_message(&sc, &msg) {
                        continue;
                    }
                    if let Some(event) = msg.as_event() {
                        ts.on_event(event.event_id, event.data, None);
                    }
                    if let Some((event_id, file)) = msg.as_event_filename() {
                        ts.on_event(event_id, 0, Some(file));
                    }
                }
                
                // Track assigned object IDs
                if let Some(assigned) = msg.as_assigned_object_id() {
                    if let Some(peer_id) = pending_requests.remove(&assigned.request_id) {
                        remote_aircraft.insert(peer_id, assigned.object_id);
                        let _ = sc.ai_release_control(assigned.object_id, assigned.request_id);
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
                        let mut ac = available_aircraft.lock();
                        if is_first {
                            ac.clear();
                        }
                        for entry in entries {
                            let title = c_char_array_to_string(&entry.AircraftTitle);
                            let livery = c_char_array_to_string(&entry.LiveryName);
                            if !title.is_empty() && !ac.contains(&title) {
                                log_icao_index_match(app, Some(&resolver.types), "aircraft", &title);
                                log_airline_index_match(app, Some(&resolver.airlines), "aircraft", &title, &livery, false);
                                ac.push(title);
                            }
                        }
                    } else if request_id == 9002 {
                        let mut hc = available_helicopters.lock();
                        if is_first {
                            hc.clear();
                        }
                        for entry in entries {
                            let title = c_char_array_to_string(&entry.AircraftTitle);
                            let livery = c_char_array_to_string(&entry.LiveryName);
                            if !title.is_empty() && !hc.contains(&title) {
                                log_icao_index_match(app, Some(&resolver.types), "helicopter", &title);
                                log_airline_index_match(app, Some(&resolver.airlines), "helicopter", &title, &livery, false);
                                hc.push(title);
                            }
                        }
                    }
                }
                if let Some(exception) = msg.as_exception() {
                    // If this exception belongs to an inject SetDataOnSimObject call,
                    // attribute it to the peer and the specific datum that failed.
                    let detail = match inject_sends.get(&exception.send_id) {
                        Some(peer) => {
                            let datum = REMOTE_DATUM_NAMES
                                .get(exception.index as usize)
                                .copied()
                                .unwrap_or("<out-of-range>");
                            format!("  (inject peer={} datum[{}]={})", peer, exception.index, datum)
                        }
                        None => String::new(),
                    };
                    crate::append_log(app, format!("[BUG] SimConnectException:: {} {} {}{}", exception.exception, exception.send_id, exception.index, detail));
                }

                if let Some(event) = msg.as_event() {
                    if event.event_id == event_sim_start {
                        // A flight loaded: the aircraft may have changed.
                        crate::append_log(app, "Received SimStart event.".to_string());
                        moving = false;
                        sc.request_data_on_sim_object(aircraft_request_id, aircraft_define_id, OBJECT_ID_USER, SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_ONCE)?;
                    } else if event.event_id == event_sim_stop {
                        crate::append_log(app, "Received SimStop event.".to_string());
                        moving = false;
                    }
                }

                if msg.request_id() == Some(aircraft_request_id) {
                    //crate::append_log(app, format!("[MSFS] aircraft_request_id {}", aircraft_request_id));
                    #[repr(C)]
                    #[derive(Debug, Clone, Copy)]
                    struct SimConnectAircraftInfo {
                        title: [u8; 256],
                        livery: [u8; 256],
                        atc_model: [u8; 256],
                        atc_id: [u8; 256],
                        object_class: [u8; 256],
                        category: [u8; 256],
                        number_of_engines: f64,
                        engine_type: f64,
                    }
                    if let Some(data) = msg.as_sim_object_data::<SimConnectAircraftInfo>() {
                        let title = String::from_utf8_lossy(&data.title).split('\0').next().unwrap_or("").trim().to_string();
                        let livery = String::from_utf8_lossy(&data.livery).split('\0').next().unwrap_or("").trim().to_string();
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
                            aircraft_info.livery = livery.clone();
                            aircraft_info.atc_model = atc_model.clone();
                            aircraft_info.atc_id = atc_id.clone();
                            aircraft_info.object_class = object_class.clone();
                            aircraft_info.category = category.clone();
                            aircraft_info.num_engines = num_engines;
                            aircraft_info.engine_type = engine_type.clone();

                            // Resolve an ICAO type from the title + livery via the word index.
                            let r = resolver.resolve(&title, &livery);
                            match &r.icao {
                                Some(m) => crate::append_log(app, format!("[MSFS Index] '{}' [{}] -> {} (score {:.2})", title, livery, m.icao, m.score)),
                                None => crate::append_log(app, format!("[MSFS Index] '{}' [{}] -> no ICAO match", title, livery)),
                            }
                            let resolved_icao = r.icao_code().to_string();
                            aircraft_info.resolved_icao = resolved_icao.clone();
                            match &r.airline {
                                Some(m) => crate::append_log(app, format!("[MSFS Airline] current '{}' [{}] -> {} {} (score {:.2})", title, livery, m.icao, m.name, m.score)),
                                None if r.airline_skipped => crate::append_log(app, format!("[MSFS Airline] current '{}' -> skipped, {} is a piston/light turboprop", title, resolved_icao)),
                                None => {}
                            }
                            let (resolved_airline_icao, resolved_airline) =
                                (r.airline_icao().to_string(), r.airline_name().to_string());
                            aircraft_info.resolved_airline = resolved_airline.clone();
                            aircraft_info.resolved_airline_icao = resolved_airline_icao.clone();


                            if let Some(ts) = telemetry_sampler.as_mut() {
                                ts.aircraft_loaded(&aircraft_info);
                            }

                            let mut info = aircraft_info_mutex.lock();
                            info.title = title;
                            info.livery = livery;
                            info.resolved_icao = resolved_icao;
                            info.resolved_airline = resolved_airline;
                            info.resolved_airline_icao = resolved_airline_icao;
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
                        last_msfs_update = std::time::Instant::now();
                        {
                            let mut m = metrics.lock();
                            *m = *data;
                        }

                        if !data.has_position() {
                            // Invalid GPS data, skip processing
                            continue;
                        }

                        // Moving under its own power: the aircraft is loaded, so ask for it again.
                        let has_aircraft_title = !aircraft_info_mutex.lock().title.is_empty();
                        if !moving && data.ground_speed > 10.0 && has_aircraft_title {
                            moving = true;
                            sc.request_data_on_sim_object(aircraft_request_id, aircraft_define_id, OBJECT_ID_USER, SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_ONCE)?;
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
                // Drop the per-peer bookkeeping too, otherwise these maps grow
                // unbounded as peers come and go over a long-running session: the
                // log-throttle timestamp, and any spawn request still awaiting an
                // assigned-object-id ack (the aircraft timed out before it spawned).
                last_ai_log.remove(&id);
                pending_requests.retain(|_, peer| peer != &id);
            }

            // Check if we haven't received telemetry updates from MSFS for 60 seconds
            if last_msfs_update.elapsed() > std::time::Duration::from_secs(60) {
                return Err(anyhow::anyhow!("No updates received from MSFS for 60 seconds. Assuming disconnected."));
            }

            thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }
}

/// Resolve an enumerated AI-aircraft title against the ICAO word index. Used at startup to
/// validate the index against the locally installed models; only unresolved titles are
/// logged, since resolved ones are the expected (and noisy) common case.
fn log_icao_index_match(
    app: &AppHandle,
    index: Option<&butterlog_identity::icao_index::IcaoIndex>,
    kind: &str,
    title: &str,
) {
    let Some(index) = index else { return };
    if index.find(title).is_none() {
        crate::append_log(
            app,
            format!("[MSFS Index] {} '{}' -> no ICAO match", kind, title),
        );
    }
}

/// Identify the operating airline from a title + livery via the airline word index, returning
/// the matched `(airline_icao, airline_name)` (both empty when none). With `log_on_match`
/// false only unresolved queries are logged — used for the AI-aircraft enumeration to avoid
/// flooding the log with the (expected) resolved entries; the current aircraft passes true to
/// log its match.
fn log_airline_index_match(
    app: &AppHandle,
    index: Option<&butterlog_identity::airline_index::AirlineIndex>,
    kind: &str,
    title: &str,
    livery: &str,
    log_on_match: bool,
) -> (String, String) {
    let Some(index) = index else { return (String::new(), String::new()) };
    let query = format!("{} {}", title, livery);
    match index.find(&query) {
        Some(m) => {
            if log_on_match {
                crate::append_log(
                    app,
                    format!("[MSFS Airline] {} '{}' [{}] -> {} {} (score {:.2})", kind, title, livery, m.icao, m.name, m.score),
                );
            }
            (m.icao, m.name)
        }
        None => (String::new(), String::new()),
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


/// Split an airline name / livery string into distinctive lowercase tokens usable for
/// matching against a local MSFS title (which encodes the livery, e.g.
/// "Boeing 737-800 Ryanair"). Generic aviation filler is dropped so it can't match on
/// a bare model title.
fn airline_match_tokens(text: &str) -> Vec<String> {
    const FILLER: &[&str] = &[
        "air", "airline", "airlines", "airway", "airways", "aviation", "aero", "cargo",
        "international", "intl", "express", "group", "the", "and",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .filter(|t| !FILLER.contains(t))
        .map(|t| t.to_string())
        .collect()
}

fn find_best_multiplayer_model(
    identity: &crate::sim_monitor::RemoteAircraftIdentity,
    available_aircraft: &[String],
    available_helicopters: &[String],
    // Title -> resolved characteristics, prebuilt once per installed-library snapshot so we
    // don't re-scan/re-sort the whole characteristics DB for every local title on every peer.
    title_chars: &std::collections::HashMap<String, Option<AircraftCharacteristic>>,
    db: &CharacteristicsDatabase,
) -> String {
    let remote_title = identity.title.as_str();
    let remote_object_class = identity.object_class.as_str();
    let remote_category = identity.category.as_str();
    let remote_num_engines = identity.num_engines;
    let remote_engine_type = identity.engine_type.as_str();
    // Prefer the sender's deduced ICAO type over its raw `ATC MODEL` claim, which is
    // frequently empty or wrong on add-on aircraft.
    let type_icao = if !identity.resolved_icao.is_empty() {
        identity.resolved_icao.as_str()
    } else {
        identity.atc_model.as_str()
    };
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

    // Helicopter gate: decide early so the airline-aware fixed-wing pass below never
    // represents a rotorcraft with an airliner that happens to share an ICAO code.
    let is_hc = remote_object_class.to_lowercase() == "helicopter"
        || remote_category.to_lowercase() == "helicopter"
        || is_helicopter_title(remote_title);

    // 3. Airline-aware exact-type match (fixed-wing): among local titles that resolve to
    //    the *same* ICAO type, prefer one whose title mentions the operating airline —
    //    MSFS ships liveries as separate titles ("Boeing 737-800 Ryanair"), so this is how
    //    we pick the right livery. Falls back to the first same-type title otherwise.
    if !is_hc && !type_icao.is_empty() {
        let type_upper = type_icao.to_uppercase();
        let mut type_matches: Vec<&String> = Vec::new();
        for ac in available_aircraft {
            if let Some(Some(c)) = title_chars.get(ac) {
                if c.icao_code.eq_ignore_ascii_case(&type_upper) {
                    type_matches.push(ac);
                }
            }
        }
        if !type_matches.is_empty() {
            let airline_tokens =
                airline_match_tokens(&format!("{} {}", identity.resolved_airline, identity.livery));
            if !airline_tokens.is_empty() {
                for ac in &type_matches {
                    let low = ac.to_lowercase();
                    if airline_tokens.iter().any(|t| low.contains(t)) {
                        return (*ac).clone();
                    }
                }
            }
            return type_matches[0].clone();
        }
    }

    // 4. Try to match by ICAO type as a substring in aircraft/helicopter titles
    if !type_icao.is_empty() {
        let atc_lower = type_icao.to_lowercase();
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
    let remote_char = if !type_icao.is_empty() {
        db.characteristics.get(&type_icao.to_uppercase()).cloned()
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
                wingspan: 0.0,
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
            if let Some(Some(l_char)) = title_chars.get(ac) {
                let score = db.calculate_similarity_score(&r_char, l_char);
                if score > best_score {
                    best_score = score;
                    best_match = Some(ac.clone());
                }
            }
        }
        
        if let Some(m) = best_match {
            return m;
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

/// True when the MSFS SimConnect server pipe is present in the named-pipe
/// namespace. A cheap, allocation-free gate in front of `simconnect_available`
/// so we don't spawn a probe process every couple of seconds while the sim is
/// plainly not running.
///
/// The pipe is named `Microsoft Flight Simulator\SimConnect` (the default pipe
/// SimConnect_Open connects to); we match on "flight simulator" specifically so
/// an unrelated `Custom\SimConnect` pipe from another app doesn't read as MSFS.
fn simconnect_pipe_present() -> bool {
    match std::fs::read_dir(r"\.\pipe\") {
        Ok(entries) => entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .contains("flight simulator")
        }),
        // If the pipe namespace can't be read, fall back to probing rather than
        // never connecting.
        Err(_) => true,
    }
}

/// Hidden CLI flag that turns a launch of our own executable into a one-shot
/// SimConnect availability probe. See [`run_probe_child`].
pub const PROBE_FLAG: &str = "--probe-simconnect";

/// Exit codes the probe child uses to report its answer.
const PROBE_AVAILABLE: i32 = 0;
const PROBE_UNAVAILABLE: i32 = 1;

/// Upper bound on how long we wait for a probe child. SimConnect_Open against a
/// live sim answers in milliseconds; anything beyond this is a wedged SDK call,
/// and killing the child is exactly the cleanup we spawned it for.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Body of the probe child process: open a SimConnect client, close it, and let
/// the exit code carry the answer back to the parent. Never returns.
///
/// A failed `SimConnect_Open` leaks resources inside the MSFS SDK — a socket per
/// attempt — that closing the handle does not reclaim, so the process that made
/// the attempt exits immediately and the OS reclaims them. `lib::run` routes
/// here before any other startup work when it sees [`PROBE_FLAG`].
pub fn run_probe_child() -> ! {
    let code = match SimConnect::open("ButterLogProbe") {
        Ok(sc) => {
            drop(sc);
            PROBE_AVAILABLE
        }
        Err(_) => PROBE_UNAVAILABLE,
    };
    std::process::exit(code);
}

/// True when MSFS will accept a SimConnect connection right now.
///
/// The attempt is made in a short-lived child process (our own executable run
/// with [`PROBE_FLAG`]) so the SDK's per-attempt leak dies with that process
/// instead of accumulating in the app across hours of polling. Only once the
/// child reports success does the caller open a connection in-process, where
/// the open is expected to succeed and so leaks nothing.
fn simconnect_available() -> bool {
    if !simconnect_pipe_present() {
        return false;
    }

    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        // Can't locate ourselves to spawn the probe: fall back to attempting the
        // connection in-process rather than never connecting.
        Err(_) => return true,
    };

    let mut cmd = std::process::Command::new(exe);
    cmd.arg(PROBE_FLAG)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flash on debug builds.
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(_) => return true,
    };

    let deadline = std::time::Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code() == Some(PROBE_AVAILABLE),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return true;
            }
        }
    }
}

impl SimMonitor for SimConnectMonitor {
    fn id(&self) -> &'static str { "msfs" }
    fn start(&self, app: AppHandle, log_path: Option<PathBuf>) -> anyhow::Result<()> {
        let mut running = self.running.lock();
        if *running { return Ok(()); }
        *running = true;
        
        let metrics = self.metrics.clone();
        let aircraft_info = self.aircraft_info.clone();
        let running_clone = self.running.clone();
        let connected_clone = self.connected.clone();
        let remote_aircraft_sender = self.remote_aircraft_sender.clone();
        let available_aircraft = self.available_aircraft.clone();
        let available_helicopters = self.available_helicopters.clone();
        
        thread::spawn({
            let app = app.clone();
            let available_aircraft = available_aircraft.clone();
            let available_helicopters = available_helicopters.clone();
            move || {
                // Each failed SimConnect_Open leaks resources inside the MSFS SDK
                // (a socket per attempt) that we can't reclaim, so we never make a
                // speculative open() here: `simconnect_available` does the trial
                // connection in a child process that exits, and we only open
                // in-process once it says the sim is answering.
                let max_backoff = Duration::from_secs(30);
                let mut backoff = Duration::from_secs(1);
                loop {
                if !*running_clone.lock() { break; }
                if !simconnect_available() {
                    // Sim not up (or not answering yet): poll again shortly.
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
                match SimConnect::open("ButterLogV2") {
                    Ok(sc) => {
                        backoff = Duration::from_secs(1);
                        crate::append_log(&app, "Successfully connected to MSFS.".to_string());
                        { let mut connected = connected_clone.lock(); *connected = true; }
                        
                        let (tx, rx) = std::sync::mpsc::channel();
                        {
                             let mut sender = remote_aircraft_sender.lock();
                             *sender = Some(tx);
                        }

                        match Self::run_monitor(
                            &app, 
                            sc, 
                            &metrics, 
                            &aircraft_info, 
                            &running_clone, 
                            rx, 
                            log_path.as_ref(),
                            &available_aircraft,
                            &available_helicopters,
                        ) {
                            Ok(_) => {
                                crate::append_log(&app, "MSFS monitor session closed normally.".to_string());
                            }
                            Err(e) => {
                                crate::append_log(&app, format!("MSFS monitor disconnected with error: {}", e));
                            }
                        }
                        { let mut connected = connected_clone.lock(); *connected = false; }
                        { let mut m = metrics.lock(); *m = FlightMetrics::default(); }
                        { let mut info = aircraft_info.lock(); *info = AircraftInfo::default(); }
                    }
                    Err(_) => {
                        backoff = (backoff * 2).min(max_backoff);
                    }
                }
                thread::sleep(backoff);
                }
            }
        });
        Ok(())
    }
    fn stop(&self) { let mut running = self.running.lock(); *running = false; }
    fn get_metrics(&self) -> FlightMetrics { *self.metrics.lock() }
    fn get_aircraft_info(&self) -> crate::models::AircraftInfo { self.aircraft_info.lock().clone() }
    fn is_connected(&self) -> bool { *self.connected.lock() }
    fn update_remote_aircraft(
        &self,
        id: &str,
        identity: &crate::sim_monitor::RemoteAircraftIdentity,
        metrics: &FlightMetrics,
        chosen_model: Option<&str>,
    ) {
        let sender = self.remote_aircraft_sender.lock();
        if let Some(tx) = sender.as_ref() {
            let _ = tx.send(RemoteAircraftUpdate {
                id: id.to_string(),
                identity: identity.clone(),
                metrics: *metrics,
                chosen_model: chosen_model.map(|s| s.to_string()),
            });
        }
    }

    fn choose_remote_model(
        &self,
        identity: &crate::sim_monitor::RemoteAircraftIdentity,
        db: &CharacteristicsDatabase,
    ) -> Option<String> {
        let mut identity = identity.clone();
        if identity.title.is_empty() {
            identity.title = "Cessna Skyhawk G1000".to_string();
        }
        let ac_list = self.available_aircraft.lock();
        let hc_list = self.available_helicopters.lock();
        let title_chars = self.title_characteristics_index(&ac_list, &hc_list, db);
        Some(find_best_multiplayer_model(&identity, &ac_list, &hc_list, &title_chars, db))
    }
}

/// Client event ids for mapped K events. Well above the request/define ids used
/// elsewhere in this file so the ranges cannot collide.
const COMMAND_EVENT_ID_BASE: u32 = 50_000;

/// Execute any queued web-issued commands against the sim.
///
/// A `false` return from the mapping means the sim cannot service the command
/// with those parameters — reported as `unsupported` rather than swallowed, so
/// the web UI can stop offering a control that does nothing here.
fn drain_sim_commands(
    app: &AppHandle,
    sc: &SimConnect,
    mapped: &mut std::collections::HashMap<&'static str, u32>,
) {
    let Some(queue) = app.try_state::<crate::sim_commands::CommandQueue>() else {
        return;
    };
    let pending = queue.drain();
    if pending.is_empty() {
        return;
    }

    let config = app.state::<crate::config::ConfigManager>().get_config();
    let flight_id = crate::service::live_flight_id(app);

    for cmd in pending {
        let outcome = match crate::sim_commands::allowed_by_settings(
            &cmd.kind,
            config.is_logged_in(),
            config.is_logged_in(),
        ) {
            Err(why) => crate::sim_commands::Outcome::Rejected(why),
            Ok(()) => match crate::sim_commands::msfs_event(&cmd.kind, &cmd.params) {
                None => crate::sim_commands::Outcome::Unsupported(format!(
                    "MSFS has no binding for `{}` with these parameters",
                    cmd.kind
                )),
                Some((event_name, data)) => {
                    let next_id = COMMAND_EVENT_ID_BASE + mapped.len() as u32;
                    let event_id = *mapped.entry(event_name).or_insert(next_id);
                    let mapped_ok = if event_id == next_id {
                        sc.map_client_event_to_sim_event(event_id, event_name).is_ok()
                    } else {
                        true
                    };
                    if !mapped_ok {
                        mapped.remove(event_name);
                        crate::sim_commands::Outcome::Unsupported(format!(
                            "MSFS did not accept the event `{}`",
                            event_name
                        ))
                    } else {
                        match sc.transmit_client_event(event_id, data) {
                            Ok(()) => {
                                crate::append_log(
                                    app,
                                    format!("[Command] Sent {} ({}) to MSFS", event_name, data),
                                );
                                crate::sim_commands::Outcome::Applied
                            }
                            Err(e) => crate::sim_commands::Outcome::Rejected(format!(
                                "MSFS rejected {}: {}",
                                event_name, e
                            )),
                        }
                    }
                }
            },
        };

        if let Some(flight_id) = flight_id {
            let app_c = app.clone();
            let id = cmd.id.clone();
            tauri::async_runtime::spawn(async move {
                crate::sim_commands::ack(&app_c, flight_id, &id, outcome).await;
            });
        }
    }
}

fn get_next_dispatch_with_retry<'a>(
    sc: &'a SimConnect,
    app: &AppHandle,
) -> anyhow::Result<Option<DispatchMessage<'a>>> {
    let mut consecutive_errors = 0;
    loop {
        match sc.get_next_dispatch() {
            Ok(None) => {thread::sleep(Duration::from_millis(200)); return Ok(None)},
            Ok(msg) => return Ok(msg),
            Err(e) => {
                consecutive_errors += 1;
                if consecutive_errors >= 5 {
                    return Err(anyhow::anyhow!("SimConnect connection lost after 5 consecutive dispatch errors. Last error: {}", e));
                }
                crate::append_log(
                    app,
                    format!(
                        "[{}] Warning: SimConnect dispatch error (attempt {}/5): {}. Retrying in 50ms...",
                        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
                        consecutive_errors,
                        e
                    ),
                );
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim_monitor::RemoteAircraftIdentity;

    /// Legacy positional shim around `find_best_multiplayer_model` so existing cases stay
    /// terse; new behavior (deduced ICAO / airline) is exercised via dedicated tests.
    fn fbm(
        title: &str,
        atc_model: &str,
        object_class: &str,
        category: &str,
        num_engines: i32,
        engine_type: &str,
        available_aircraft: &[String],
        available_helicopters: &[String],
        db: &CharacteristicsDatabase,
    ) -> String {
        let identity = RemoteAircraftIdentity {
            title: title.to_string(),
            atc_model: atc_model.to_string(),
            object_class: object_class.to_string(),
            category: category.to_string(),
            num_engines,
            engine_type: engine_type.to_string(),
            ..Default::default()
        };
        fbm_id(&identity, available_aircraft, available_helicopters, db)
    }

    /// Build the title index the way production does, then resolve — so tests exercise the
    /// same prebuilt-index path as `choose_remote_model`.
    fn fbm_id(
        identity: &RemoteAircraftIdentity,
        available_aircraft: &[String],
        available_helicopters: &[String],
        db: &CharacteristicsDatabase,
    ) -> String {
        let mut titles = available_aircraft.to_vec();
        titles.extend_from_slice(available_helicopters);
        let title_chars = db.build_title_index(&titles);
        find_best_multiplayer_model(identity, available_aircraft, available_helicopters, &title_chars, db)
    }

    #[test]
    fn matches_gulfstream_g650() {
        let db = CharacteristicsDatabase::parse(butterlog_identity::aircraft_characteristics::CSV)
            .expect("Failed to load characteristics DB for tests");

        let available_aircraft = vec![
            "Cessna Skyhawk G1000".to_string(),
            "Gulfstream G650".to_string(),
        ];

        // Exact-title match wins outright — the common case where the peer flies the same
        // stock title we have installed. (The deduced ICAO is along for the ride.)
        let exact = RemoteAircraftIdentity {
            title: "Gulfstream G650".to_string(),
            resolved_icao: "GLF6".to_string(),
            ..Default::default()
        };
        assert_eq!(fbm_id(&exact, &available_aircraft, &[], &db), "Gulfstream G650");

        // Case-insensitive exact match.
        let lower = RemoteAircraftIdentity {
            title: "gulfstream g650".to_string(),
            resolved_icao: "GLF6".to_string(),
            ..Default::default()
        };
        assert_eq!(fbm_id(&lower, &available_aircraft, &[], &db), "Gulfstream G650");

        // When the peer didn't deduce an ICAO type, a livery title we don't have verbatim
        // still maps to the installed G650 by title overlap rather than a generic aircraft.
        let livery_only = vec![
            "Cessna Skyhawk G1000".to_string(),
            "Gulfstream G650 NetJets".to_string(),
        ];
        let no_icao = RemoteAircraftIdentity {
            title: "Gulfstream G650".to_string(),
            ..Default::default()
        };
        assert_eq!(fbm_id(&no_icao, &livery_only, &[], &db), "Gulfstream G650 NetJets");

        // Deduced ICAO drives selection even when the peer's raw title is an unhelpful add-on
        // name: "Gulfstream G650" resolves to GLF6 (via the brand + model-name match), so the
        // GLF6 type maps to it rather than falling back to a generic jet or the Cessna default.
        let deduced = RemoteAircraftIdentity {
            title: "PMDG-style custom bizjet livery".to_string(),
            resolved_icao: "GLF6".to_string(),
            ..Default::default()
        };
        assert_eq!(fbm_id(&deduced, &available_aircraft, &[], &db), "Gulfstream G650");

        // Airline-aware livery selection: among installed G650 liveries (all resolving to GLF6),
        // pick the one whose title mentions the operating airline.
        let liveries = vec![
            "Gulfstream G650 NetJets".to_string(),
            "Gulfstream G650 Qatar Executive".to_string(),
        ];
        let qatar = RemoteAircraftIdentity {
            title: "Gulfstream G650".to_string(),
            resolved_icao: "GLF6".to_string(),
            resolved_airline: "Qatar Executive".to_string(),
            ..Default::default()
        };
        assert_eq!(fbm_id(&qatar, &liveries, &[], &db), "Gulfstream G650 Qatar Executive");
    }

    #[test]
    fn test_find_best_multiplayer_model() {
        let db = CharacteristicsDatabase::parse(butterlog_identity::aircraft_characteristics::CSV)
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
            fbm("Boeing 737-800", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Boeing 737-800"
        );

        // Case-insensitive match
        assert_eq!(
            fbm("boeing 737-800", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Boeing 737-800"
        );

        // Helicopter exact match
        assert_eq!(
            fbm("Bell 407", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Bell 407"
        );

        // Helicopter keyword fallback
        assert_eq!(
            fbm("Bell 206 Helicopter", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Bell 407"
        );

        // Helicopter category fallback (forces helicopter matching even with a non-helo title)
        assert_eq!(
            fbm("MyCrazyRotorcraft", "", "helicopter", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Cabri G2"
        );

        // Helicopter generic fallback
        assert_eq!(
            fbm("Rotorway Heli", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Cabri G2"
        );

        // Substring aircraft match
        assert_eq!(
            fbm("A320", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Airbus A320neo"
        );

        // ICAO (atc_model) match
        assert_eq!(
            fbm("Some Weird Livery Name", "A320", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Airbus A320neo"
        );

        // Generic profile matching (twin jet -> Boeing 737-800 because it is the first matching jet in the list)
        assert_eq!(
            fbm("Weird Twin Jet 5000", "", "", "", 2, "jet", &available_aircraft, &available_helicopters, &db),
            "Boeing 737-800"
        );

        // Generic profile matching (single piston -> Cessna Skyhawk G1000)
        assert_eq!(
            fbm("Heavy Single Piston 3000", "", "", "", 1, "piston", &available_aircraft, &available_helicopters, &db),
            "Cessna Skyhawk G1000"
        );

        // CSV database similarity match (remote ICAO A19N matches Airbus A320neo because both are twin jets in same wake class)
        assert_eq!(
            fbm("Airbus A319 Neo", "A19N", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Airbus A320neo"
        );

        // Sensible aircraft default fallback
        assert_eq!(
            fbm("F-18 Hornet", "", "", "", 0, "", &available_aircraft, &available_helicopters, &db),
            "Cessna Skyhawk G1000"
        );

        // Ultimate fallback
        let empty_ac = vec![];
        let empty_hc = vec![];
        assert_eq!(
            fbm("F-18 Hornet", "", "", "", 0, "", &empty_ac, &empty_hc, &db),
            "Cessna Skyhawk G1000"
        );
    }

    #[test]
    fn prefers_deduced_icao_over_atc_model() {
        let db = CharacteristicsDatabase::parse(butterlog_identity::aircraft_characteristics::CSV)
            .expect("Failed to load characteristics DB for tests");
        let available_aircraft = vec![
            "Cessna Skyhawk G1000".to_string(),
            "Boeing 737-800".to_string(),
            "Airbus A320neo".to_string(),
        ];

        // The sim's own ATC MODEL is blank (typical for add-ons) but the sender deduced the
        // ICAO type from the title; the deduced type drives selection.
        let identity = RemoteAircraftIdentity {
            title: "PMDG 737 in some custom livery".to_string(),
            resolved_icao: "B738".to_string(),
            ..Default::default()
        };
        assert_eq!(
            fbm_id(&identity, &available_aircraft, &[], &db),
            "Boeing 737-800"
        );
    }

    #[test]
    fn airline_aware_livery_selection() {
        let db = CharacteristicsDatabase::parse(butterlog_identity::aircraft_characteristics::CSV)
            .expect("Failed to load characteristics DB for tests");
        // Two installed liveries of the same type, encoded as separate titles.
        let available_aircraft = vec![
            "Cessna Skyhawk G1000".to_string(),
            "Boeing 737-800 United".to_string(),
            "Boeing 737-800 Ryanair".to_string(),
        ];

        // Same type, different deduced airline -> pick the matching livery title.
        let ryr = RemoteAircraftIdentity {
            title: "Boeing 737-800".to_string(),
            resolved_icao: "B738".to_string(),
            resolved_airline: "Ryanair".to_string(),
            ..Default::default()
        };
        assert_eq!(
            fbm_id(&ryr, &available_aircraft, &[], &db),
            "Boeing 737-800 Ryanair"
        );

        let ual = RemoteAircraftIdentity {
            title: "Boeing 737-800".to_string(),
            resolved_icao: "B738".to_string(),
            resolved_airline: "United Airlines".to_string(),
            ..Default::default()
        };
        assert_eq!(
            fbm_id(&ual, &available_aircraft, &[], &db),
            "Boeing 737-800 United"
        );

        // No deduced airline -> first same-type title, not a generic fallback.
        let none = RemoteAircraftIdentity {
            title: "Boeing 737-800".to_string(),
            resolved_icao: "B738".to_string(),
            ..Default::default()
        };
        assert_eq!(
            fbm_id(&none, &available_aircraft, &[], &db),
            "Boeing 737-800 United"
        );
    }
}

