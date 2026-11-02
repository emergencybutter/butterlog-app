//! The traffic packets peers exchange (`multiplayer.rs`), and their conversion to and from
//! what the monitors and injectors use.
//!
//! Two formats are on the wire while older apps are around (design §6.1 in the service):
//!
//! - **`TrafficState`** (`butterlog-wire`): about 100 bytes, sim-neutral, versioned.
//! - **The JSON packet** older apps send and understand: the full `FlightMetrics` (about
//!   3 kB) and the identity. Apps that speak `TrafficState` add `"traffic_state": <version>`
//!   to it, which older apps ignore.
//!
//! A peer gets `TrafficState` once it has shown it speaks it (either way), JSON until then,
//! so old and new apps keep seeing each other. The JSON packet goes once no client older
//! than this one is left (rollout phase 10).

use crate::models::{AircraftInfo, FlightMetrics};
use crate::sim_monitor::RemoteAircraftIdentity;
use butterlog_wire::{lights, TrafficIdentity, TrafficState, TRAFFIC_VERSION};

const M_PER_FT: f64 = 0.3048;

/// A peer's update, whichever format it came in.
#[derive(Debug, Clone)]
pub struct Update {
    pub identity: RemoteAircraftIdentity,
    pub metrics: FlightMetrics,
    /// The peer can be sent `TrafficState`.
    pub speaks_traffic_state: bool,
}

/// Our aircraft as a `TrafficState`. `FlightMetrics` attitude carries the MSFS signs
/// (positive pitch nose down, positive bank left wing down); `TrafficState` carries the
/// usual ones.
pub fn to_state(m: &FlightMetrics, a: &AircraftInfo) -> TrafficState {
    let on = |v: f64, bit: u8| if v > 0.5 { bit } else { 0 };
    TrafficState {
        version: TRAFFIC_VERSION,
        latitude_deg: m.latitude,
        longitude_deg: m.longitude,
        altitude_msl_m: (m.gps_altitude_msl * M_PER_FT) as f32,
        heading_deg: m.heading as f32,
        track_deg: m.track as f32,
        pitch_deg: -m.pitch_angle as f32,
        bank_deg: -m.roll_angle as f32,
        ground_speed_kt: m.ground_speed as f32,
        indicated_airspeed_kt: m.indicated_airspeed as f32,
        on_ground: m.is_on_ground > 0.5,
        gear: m.xp_gear_ratio as f32,
        lights: on(m.nav_lights, lights::NAV)
            | on(m.beacon_lights, lights::BEACON)
            | on(m.strobe_lights, lights::STROBE)
            | on(m.taxi_lights, lights::TAXI)
            | on(m.landing_lights, lights::LANDING),
        identity: TrafficIdentity {
            title: a.title.clone(),
            atc_model: a.atc_model.clone(),
            icao: a.resolved_icao.clone(),
            airline_icao: a.resolved_airline_icao.clone(),
            livery: a.livery.clone(),
            object_class: a.object_class.clone(),
            category: a.category.clone(),
            engine_type: a.engine_type.clone(),
            engine_count: a.num_engines.clamp(0, 255) as u8,
        },
    }
}

/// A peer's `TrafficState` as the injectors take it: the fields they draw from, the rest
/// left at zero.
pub fn from_state(s: &TrafficState) -> Update {
    let bit = |b: u8| if s.lights & b != 0 { 1.0 } else { 0.0 };
    let altitude_ft = f64::from(s.altitude_msl_m) / M_PER_FT;
    let metrics = FlightMetrics {
        latitude: s.latitude_deg,
        longitude: s.longitude_deg,
        gps_altitude_msl: altitude_ft,
        // Shown in the multiplayer tab; the injectors place traffic by MSL altitude.
        indicated_altitude: altitude_ft,
        heading: s.heading_deg.into(),
        track: s.track_deg.into(),
        pitch_angle: -f64::from(s.pitch_deg),
        roll_angle: -f64::from(s.bank_deg),
        ground_speed: s.ground_speed_kt.into(),
        indicated_airspeed: s.indicated_airspeed_kt.into(),
        is_on_ground: if s.on_ground { 1.0 } else { 0.0 },
        xp_gear_ratio: s.gear.into(),
        nav_lights: bit(lights::NAV),
        beacon_lights: bit(lights::BEACON),
        strobe_lights: bit(lights::STROBE),
        taxi_lights: bit(lights::TAXI),
        landing_lights: bit(lights::LANDING),
        ..Default::default()
    };
    let id = &s.identity;
    Update {
        identity: identity(
            &id.title,
            &id.atc_model,
            &id.icao,
            &id.airline_icao,
            &id.livery,
            &id.object_class,
            &id.category,
            id.engine_count.into(),
            &id.engine_type,
        ),
        metrics,
        speaks_traffic_state: true,
    }
}

/// The JSON packet, for peers that haven't shown they speak `TrafficState`.
pub fn json_packet(m: &FlightMetrics, a: &AircraftInfo) -> serde_json::Value {
    serde_json::json!({
        "aircraft": a.title,
        "atc_model": a.atc_model,
        // The airline goes as a portable ICAO code; the receiver maps it back to a name
        // against its own table (names and livery paths aren't portable).
        "resolved_icao": a.resolved_icao,
        "airline_icao": a.resolved_airline_icao,
        "livery": a.livery,
        "object_class": a.object_class,
        "category": a.category,
        "num_engines": a.num_engines,
        "engine_type": a.engine_type,
        "metrics": m,
        "traffic_state": TRAFFIC_VERSION,
    })
}

/// A datagram from a peer: `Ok(None)` for one that is neither format, `Err` for one that
/// claims to be and isn't.
pub fn parse(data: &[u8]) -> Result<Option<Update>, String> {
    if TrafficState::is_traffic_state(data) {
        return TrafficState::decode(data)
            .map(|s| Some(from_state(&s)))
            .map_err(|e| format!("bad TrafficState: {e:?}"));
    }
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(data) else {
        return Ok(None);
    };
    let (Some(title), Some(metrics)) = (payload["aircraft"].as_str(), payload.get("metrics")) else {
        return Ok(None);
    };
    let metrics = serde_json::from_value::<FlightMetrics>(metrics.clone())
        .map_err(|e| format!("bad FlightMetrics: {e}"))?;
    let text = |k: &str| payload.get(k).and_then(|v| v.as_str()).unwrap_or("");
    Ok(Some(Update {
        identity: identity(
            title,
            text("atc_model"),
            // Absent from peers older than identity resolution.
            text("resolved_icao"),
            text("airline_icao"),
            text("livery"),
            text("object_class"),
            text("category"),
            payload.get("num_engines").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            text("engine_type"),
        ),
        metrics,
        speaks_traffic_state: payload.get("traffic_state").and_then(|v| v.as_u64()).unwrap_or(0) >= 1,
    }))
}

#[allow(clippy::too_many_arguments)]
fn identity(
    title: &str,
    atc_model: &str,
    icao: &str,
    airline_icao: &str,
    livery: &str,
    object_class: &str,
    category: &str,
    num_engines: i32,
    engine_type: &str,
) -> RemoteAircraftIdentity {
    RemoteAircraftIdentity {
        title: title.to_string(),
        atc_model: atc_model.to_string(),
        resolved_icao: icao.to_string(),
        // Our own name for the airline, to match against our installed livery titles.
        resolved_airline: crate::airlines::AirlinesDatabase::embedded()
            .name_for_icao(airline_icao)
            .unwrap_or_default()
            .to_string(),
        resolved_airline_icao: airline_icao.to_string(),
        livery: livery.to_string(),
        object_class: object_class.to_string(),
        category: category.to_string(),
        num_engines,
        engine_type: engine_type.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flying() -> (FlightMetrics, AircraftInfo) {
        let m = FlightMetrics {
            latitude: 47.4490123,
            longitude: -122.3098765,
            gps_altitude_msl: 4_050.0,
            indicated_altitude: 4_020.0,
            heading: 163.2,
            track: 161.0,
            pitch_angle: -7.5, // nose up, MSFS sign
            roll_angle: 12.0,  // left wing down, MSFS sign
            ground_speed: 182.0,
            indicated_airspeed: 170.0,
            is_on_ground: 0.0,
            xp_gear_ratio: 1.0,
            nav_lights: 1.0,
            beacon_lights: 1.0,
            landing_lights: 1.0,
            vertical_speed: 1_200.0, // not sent
            ..Default::default()
        };
        let a = AircraftInfo {
            title: "Boeing 737-800 Ryanair".into(),
            atc_model: "B738".into(),
            resolved_icao: "B738".into(),
            resolved_airline_icao: "RYR".into(),
            livery: "Ryanair".into(),
            object_class: "airplane".into(),
            num_engines: 2,
            engine_type: "jet".into(),
            ..Default::default()
        };
        (m, a)
    }

    #[test]
    fn our_aircraft_reaches_a_peer_as_the_injectors_need_it() {
        let (m, a) = flying();
        let state = to_state(&m, &a);
        assert_eq!((state.pitch_deg, state.bank_deg), (7.5, -12.0));

        let got = parse(&state.encode()).unwrap().unwrap();
        assert!(got.speaks_traffic_state);
        let r = &got.metrics;
        assert_eq!((r.latitude, r.longitude), (m.latitude, m.longitude));
        assert!((r.gps_altitude_msl - m.gps_altitude_msl).abs() < 0.01, "{}", r.gps_altitude_msl);
        assert!((r.heading - m.heading).abs() < 1e-4);
        assert!((r.pitch_angle - m.pitch_angle).abs() < 1e-4);
        assert!((r.roll_angle - m.roll_angle).abs() < 1e-4);
        assert!((r.indicated_airspeed - m.indicated_airspeed).abs() < 1e-4);
        assert_eq!((r.is_on_ground, r.xp_gear_ratio), (0.0, 1.0));
        assert_eq!(
            (r.nav_lights, r.beacon_lights, r.strobe_lights, r.taxi_lights, r.landing_lights),
            (1.0, 1.0, 0.0, 0.0, 1.0)
        );
        assert_eq!(r.vertical_speed, 0.0);

        let id = &got.identity;
        assert_eq!((id.title.as_str(), id.resolved_icao.as_str()), ("Boeing 737-800 Ryanair", "B738"));
        assert_eq!((id.resolved_airline_icao.as_str(), id.resolved_airline.as_str()), ("RYR", "Ryanair"));
        assert_eq!(id.num_engines, 2);
    }

    #[test]
    fn the_json_packet_says_we_speak_traffic_state() {
        let (m, a) = flying();
        let json = serde_json::to_vec(&json_packet(&m, &a)).unwrap();
        let got = parse(&json).unwrap().unwrap();
        assert!(got.speaks_traffic_state);
        assert_eq!(got.metrics.vertical_speed, 1_200.0);
        assert_eq!(got.identity.resolved_airline, "Ryanair");
    }

    /// Older apps parse every datagram as JSON and drop what fails, so a `TrafficState`
    /// that reaches one (before it is known not to speak it) is ignored, not misread.
    #[test]
    fn an_older_app_drops_a_traffic_state() {
        let (m, a) = flying();
        assert!(serde_json::from_slice::<serde_json::Value>(&to_state(&m, &a).encode()).is_err());
    }

    #[test]
    fn an_older_app_s_json_packet_still_parses() {
        let json = br#"{"aircraft":"Cessna Skyhawk","atc_model":"C172","metrics":{"Latitude":47.5}}"#;
        let got = parse(json).unwrap().unwrap();
        assert!(!got.speaks_traffic_state);
        assert_eq!((got.identity.title.as_str(), got.metrics.latitude), ("Cessna Skyhawk", 47.5));
    }

    #[test]
    fn other_datagrams_are_not_updates() {
        assert!(parse(b"hello").unwrap().is_none());
        assert!(parse(br#"{"ping":1}"#).unwrap().is_none());
        assert!(parse(b"BLT1 broken").is_err());
    }
}
