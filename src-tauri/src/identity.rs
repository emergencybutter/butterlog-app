//! The aircraft identity (ICAO type and airline) the service resolved, which traffic peers
//! get in place of the local one (docs/sim-native-telemetry.md §6.1 in the service).
//!
//! Both sides resolve with the same crate (`butterlog-identity`), so they agree unless one
//! has a newer copy. The service's answer wins: an index fix then reaches every peer with a
//! service deploy. Until a session opens, or when it never does (telemetry off, service
//! down), the monitors' local resolution is what peers get.
//!
//! Answers are keyed by aircraft title and livery, so one for an earlier session (a spool
//! replayed after a restart) never applies to a different aircraft.

use crate::models::AircraftInfo;
use parking_lot::Mutex;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The `identity` in the service's reply to opening a session.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceIdentity {
    #[serde(default)]
    pub icao: String,
    #[serde(default)]
    pub airline_icao: String,
    #[serde(default)]
    pub airline: String,
}

struct Answer {
    identity: ServiceIdentity,
    /// The difference from the local resolution was logged.
    compared: bool,
}

fn answers() -> &'static Mutex<HashMap<(String, String), Answer>> {
    static ANSWERS: OnceLock<Mutex<HashMap<(String, String), Answer>>> = OnceLock::new();
    ANSWERS.get_or_init(Default::default)
}

/// Records the service's answer for an aircraft.
pub fn record(title: &str, livery: &str, identity: ServiceIdentity) {
    answers()
        .lock()
        .insert((title.to_string(), livery.to_string()), Answer { identity, compared: false });
}

/// `info` as peers should see it: the service's type and airline when it has answered for
/// this aircraft, the local ones otherwise. `log` gets one line per aircraft the first time
/// the two disagree.
pub fn for_peers(info: &AircraftInfo, log: impl FnOnce(String)) -> AircraftInfo {
    let mut answers = answers().lock();
    let Some(answer) = answers.get_mut(&(info.title.clone(), info.livery.clone())) else {
        return info.clone();
    };
    let service = &answer.identity;
    if !answer.compared {
        answer.compared = true;
        if service.icao != info.resolved_icao || service.airline_icao != info.resolved_airline_icao {
            log(format!(
                "[Identity] '{}' [{}]: service says {} {}, app said {} {}; peers get the service's",
                info.title,
                info.livery,
                or_none(&service.icao),
                or_none(&service.airline_icao),
                or_none(&info.resolved_icao),
                or_none(&info.resolved_airline_icao),
            ));
        }
    }
    AircraftInfo {
        resolved_icao: service.icao.clone(),
        resolved_airline_icao: service.airline_icao.clone(),
        resolved_airline: service.airline.clone(),
        ..info.clone()
    }
}

fn or_none(s: &str) -> &str {
    if s.is_empty() {
        "(none)"
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(title: &str) -> AircraftInfo {
        AircraftInfo {
            title: title.into(),
            livery: "Blue".into(),
            resolved_icao: "B737".into(),
            ..Default::default()
        }
    }

    #[test]
    fn peers_get_the_local_identity_until_the_service_answers() {
        let local = info("identity test: unanswered");
        assert_eq!(for_peers(&local, |_| {}).resolved_icao, "B737");
    }

    #[test]
    fn the_service_answer_wins_and_a_disagreement_is_logged_once() {
        let local = info("identity test: answered");
        record(&local.title, "Blue", ServiceIdentity {
            icao: "B738".into(),
            airline_icao: "RYR".into(),
            airline: "Ryanair".into(),
        });
        let mut lines = Vec::new();
        let sent = for_peers(&local, |l| lines.push(l));
        assert_eq!(
            (sent.resolved_icao.as_str(), sent.resolved_airline_icao.as_str(), sent.resolved_airline.as_str()),
            ("B738", "RYR", "Ryanair")
        );
        assert_eq!(sent.title, local.title);
        for_peers(&local, |l| lines.push(l));
        assert_eq!(lines.len(), 1, "{lines:?}");
    }

    #[test]
    fn an_answer_for_another_livery_does_not_apply() {
        let local = info("identity test: other livery");
        record(&local.title, "Red", ServiceIdentity { icao: "A320".into(), ..Default::default() });
        assert_eq!(for_peers(&local, |_| {}).resolved_icao, "B737");
    }
}
