//! What the capture reads, as the window's Telemetry page shows it: every channel the
//! sampler records (`recorder.rs`), with its latest value, and the sim's recent events.
//!
//! The recorder publishes a copy here a few times a second; the page reads it while it is
//! open. Nothing is kept once the sim goes away.

use butterlog_wire::Channel;
use parking_lot::Mutex;
use serde::Serialize;

/// How many of the sim's events the page lists.
const RECENT_EVENTS: usize = 20;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LiveChannel {
    pub id: u16,
    /// SimVar or dataref, as the sim names it.
    pub name: String,
    /// In the sim's own unit.
    pub unit: String,
    /// `F64`, `Bool`, `Enum`, ...
    pub kind: String,
    /// How often it is read: `Frame`, `Fast`, `Slow`, `OnChange`.
    pub rate: String,
    /// Not read yet when absent.
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LiveEvent {
    #[ts(type = "number")]
    pub at_ms: i64,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LiveTelemetry {
    /// `msfs2024`, `xplane12`, ...: what the session header says, once known.
    pub sim: String,
    pub sim_version: String,
    /// `simconnect`, `plugin`, `webapi`.
    pub capture: String,
    pub aircraft: String,
    /// The session being recorded and sent, if any.
    pub session: Option<String>,
    /// Ticks sampled since the sim connected.
    #[ts(type = "number")]
    pub ticks: u64,
    /// Wall time of the last tick.
    #[ts(type = "number")]
    pub updated_ms: i64,
    pub channels: Vec<LiveChannel>,
    pub events: Vec<LiveEvent>,
}

impl LiveTelemetry {
    pub fn new(channels: &[Channel]) -> Self {
        Self {
            channels: channels
                .iter()
                .map(|c| LiveChannel {
                    id: c.id,
                    name: c.name.clone(),
                    unit: c.unit.clone(),
                    kind: format!("{:?}", c.kind),
                    rate: format!("{:?}", c.rate_class),
                    value: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    pub fn set(&mut self, channel: u16, value: f64) {
        if !value.is_finite() {
            return;
        }
        // Channel ids are dense and in order, so this is usually a direct hit.
        let hit = match self.channels.get_mut(channel as usize) {
            Some(c) if c.id == channel => Some(c),
            _ => self.channels.iter_mut().find(|c| c.id == channel),
        };
        if let Some(c) = hit {
            c.value = Some(value);
        }
    }

    pub fn event(&mut self, at_ms: i64, kind: String, detail: String) {
        if self.events.len() >= RECENT_EVENTS {
            self.events.remove(0);
        }
        self.events.push(LiveEvent { at_ms, kind, detail });
    }
}

/// Managed state: the last copy the recorder published, by sim.
#[derive(Default)]
pub struct LiveView(Mutex<Vec<(&'static str, LiveTelemetry)>>);

impl LiveView {
    pub fn publish(&self, monitor: &'static str, live: &LiveTelemetry) {
        let mut all = self.0.lock();
        match all.iter_mut().find(|(m, _)| *m == monitor) {
            Some((_, slot)) => *slot = live.clone(),
            None => all.push((monitor, live.clone())),
        }
    }

    pub fn clear(&self, monitor: &'static str) {
        self.0.lock().retain(|(m, _)| *m != monitor);
    }

    pub fn get(&self, monitor: &str) -> Option<LiveTelemetry> {
        self.0.lock().iter().find(|(m, _)| *m == monitor).map(|(_, l)| l.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use butterlog_wire::{DeltaOrder, Kind, RateClass};

    fn channel(id: u16, name: &str) -> Channel {
        Channel {
            id,
            name: name.into(),
            unit: "feet".into(),
            kind: Kind::F64,
            quantum: 1.0,
            rate_class: RateClass::Slow,
            delta: DeltaOrder::Delta,
        }
    }

    #[test]
    fn every_channel_is_listed_with_its_latest_value() {
        let mut live = LiveTelemetry::new(&[channel(0, "A"), channel(1, "B"), channel(7, "C")]);
        live.set(1, 12.5);
        live.set(7, 3.0);
        live.set(7, f64::NAN);
        live.set(9, 1.0);
        let values: Vec<_> = live.channels.iter().map(|c| (c.name.as_str(), c.value)).collect();
        assert_eq!(values, [("A", None), ("B", Some(12.5)), ("C", Some(3.0))]);
        assert_eq!(live.channels[0].rate, "Slow");
    }

    #[test]
    fn only_the_recent_events_are_kept() {
        let mut live = LiveTelemetry::new(&[]);
        for n in 0..25 {
            live.event(n, "Paused".into(), String::new());
        }
        assert_eq!(live.events.len(), RECENT_EVENTS);
        assert_eq!(live.events[0].at_ms, 5);
    }

    #[test]
    fn each_sim_has_its_own_view() {
        let view = LiveView::default();
        let mut live = LiveTelemetry::new(&[channel(0, "A")]);
        live.sim = "msfs2024".into();
        view.publish("msfs", &live);
        live.sim = "xplane12".into();
        view.publish("xplane", &live);
        assert_eq!(view.get("msfs").unwrap().sim, "msfs2024");
        view.clear("msfs");
        assert!(view.get("msfs").is_none());
        assert_eq!(view.get("xplane").unwrap().sim, "xplane12");
    }
}
