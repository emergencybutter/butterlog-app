//! Sim-native telemetry (butterlog-service `docs/sim-native-telemetry.md`): the app
//! encodes what the sim exposes into chunks, spools them, and uploads them to the service,
//! which stores and analyses them.
//!
//! The recorder here takes ticks from a sampler and writes a chunk to the spool every
//! [`CHUNK_MS`]; the uploader sends the spool. No sampler feeds it yet: until one does,
//! the spool stays empty and the uploader idles. Runs beside `live_track.rs`.

// The recording side has no caller until the SimConnect sampler (rollout phase 2).
#![cfg_attr(not(test), allow(dead_code))]

pub mod spool;
pub mod uploader;

use butterlog_wire::{compress, EncodeError, SessionEncoder, SessionOpen, Tick};
use parking_lot::Mutex;
use spool::{Spool, SpoolError};
use uuid::Uuid;

/// Wall time per chunk: the live latency budget. 2 s chunks measured ~40% larger.
pub const CHUNK_MS: i64 = 5_000;

/// How often a chunk restates every channel. 120 s measured only ~4% smaller.
pub const KEYFRAME_MS: i64 = 60_000;

/// zstd level for chunks as sent. Levels 1–9 measured within a few percent.
const CHUNK_LEVEL: i32 = 3;

/// The spool's bound. At ~140 kB per flight-hour, about 1,800 hours unsent.
pub const SPOOL_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// The spool directory, under the app's data directory.
pub const SPOOL_DIR_NAME: &str = "telemetry_spool";

#[derive(Debug)]
pub enum RecordError {
    /// The tick was dropped; the session goes on.
    Encode(EncodeError),
    /// The chunk could not be spooled, so the session has ended.
    Spool(SpoolError),
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(e) => write!(f, "tick dropped: {e}"),
            Self::Spool(e) => write!(f, "recording stopped: {e}"),
        }
    }
}

struct Recording {
    id: Uuid,
    encoder: SessionEncoder,
}

/// Owns the spool and the session being recorded, if any. Managed as Tauri state.
pub struct Telemetry {
    spool: Spool,
    live: Mutex<Option<Recording>>,
}

impl Telemetry {
    pub fn new(spool: Spool) -> Self {
        Self {
            spool,
            live: Mutex::new(None),
        }
    }

    pub fn spool(&self) -> &Spool {
        &self.spool
    }

    /// Starts a session, ending the one being recorded first.
    pub fn start(&self, open: &SessionOpen) -> Result<Uuid, SpoolError> {
        let mut live = self.live.lock();
        if let Some(recording) = live.take() {
            self.finish(recording)?;
        }
        let id = Uuid::new_v4();
        self.spool.begin(id, &open.encode())?;
        *live = Some(Recording {
            id,
            encoder: SessionEncoder::new(open.channels.clone(), KEYFRAME_MS),
        });
        Ok(id)
    }

    /// Records one tick, spooling a chunk once [`CHUNK_MS`] of ticks are buffered. Without
    /// a session, the tick is ignored.
    pub fn record(&self, tick: Tick) -> Result<(), RecordError> {
        let mut live = self.live.lock();
        let Some(recording) = live.as_mut() else {
            return Ok(());
        };
        recording.encoder.push(tick).map_err(RecordError::Encode)?;
        if recording.encoder.buffered_span_ms() < CHUNK_MS {
            return Ok(());
        }
        let Some(chunk) = recording.encoder.flush() else {
            return Ok(());
        };
        let id = recording.id;
        if let Err(e) = self
            .spool
            .append(id, chunk.seq, &compress(&chunk.payload, CHUNK_LEVEL))
        {
            // Chunks continue each other, so the session cannot skip one. What is spooled
            // still goes up, as a session that ends here.
            *live = None;
            let _ = self.spool.end(id);
            return Err(RecordError::Spool(e));
        }
        Ok(())
    }

    /// Spools what is buffered and ends the session.
    pub fn stop(&self) -> Result<(), SpoolError> {
        match self.live.lock().take() {
            Some(recording) => self.finish(recording),
            None => Ok(()),
        }
    }

    fn finish(&self, mut recording: Recording) -> Result<(), SpoolError> {
        let flushed = match recording.encoder.flush() {
            Some(chunk) => self.spool.append(
                recording.id,
                chunk.seq,
                &compress(&chunk.payload, CHUNK_LEVEL),
            ),
            None => Ok(()),
        };
        // Ended even when the last chunk did not fit: the session cannot go on without it.
        self.spool.end(recording.id)?;
        flushed
    }
}

#[cfg(test)]
pub(crate) fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("butterlog-telemetry-{name}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use butterlog_wire::{
        decompress, Aircraft, Channel, DeltaOrder, Kind, RateClass, SessionDecoder, SessionHeader,
        Value, PROTOCOL_VERSION,
    };

    pub(crate) fn open() -> SessionOpen {
        SessionOpen {
            header: SessionHeader {
                protocol_version: PROTOCOL_VERSION,
                sim: "msfs2024".into(),
                sim_version: "1.4".into(),
                capture: "simconnect".into(),
                client_version: "test".into(),
                aircraft: Aircraft {
                    title: "Test".into(),
                    ..Default::default()
                },
            },
            channels: vec![Channel {
                id: 1,
                name: "PLANE ALTITUDE".into(),
                unit: "feet".into(),
                kind: Kind::F64,
                quantum: 1.0,
                rate_class: RateClass::Slow,
                delta: DeltaOrder::Delta,
            }],
        }
    }

    /// One tick a second from `from`, climbing a foot a second.
    pub(crate) fn ticks(from: i64, n: i64) -> Vec<Tick> {
        (from..from + n)
            .map(|s| Tick {
                sim_time: s as f64,
                wall_time_ms: 1_700_000_000_000 + s * 1000,
                flags: 0,
                values: vec![(1, Value::Int(s))],
            })
            .collect()
    }

    /// Decodes the session's spooled chunks, in order.
    pub(crate) fn decode_spooled(spool: &Spool, id: Uuid) -> Vec<Tick> {
        let mut decoder = SessionDecoder::new(open().channels);
        spool
            .chunks(id)
            .into_iter()
            .flat_map(|seq| {
                let payload = decompress(&spool.read_chunk(id, seq).unwrap()).unwrap();
                decoder.decode(&payload).unwrap().ticks
            })
            .collect()
    }

    #[test]
    fn spools_a_chunk_every_five_seconds_and_the_rest_on_stop() {
        let dir = temp_dir("record");
        let telemetry = Telemetry::new(Spool::open(dir.clone(), SPOOL_MAX_BYTES).unwrap());
        let id = telemetry.start(&open()).unwrap();
        for tick in ticks(0, 23) {
            telemetry.record(tick).unwrap();
        }
        assert_eq!(telemetry.spool().chunks(id).len(), 3);
        assert!(!telemetry.spool().sessions()[0].ended);

        telemetry.stop().unwrap();
        assert_eq!(telemetry.spool().chunks(id).len(), 4);
        assert!(telemetry.spool().sessions()[0].ended);
        assert_eq!(decode_spooled(telemetry.spool(), id), ticks(0, 23));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_full_spool_ends_the_session_and_keeps_what_it_has() {
        let dir = temp_dir("record-full");
        let opening = open().encode().len() as u64;
        let telemetry = Telemetry::new(Spool::open(dir.clone(), opening + 60).unwrap());
        let id = telemetry.start(&open()).unwrap();
        let mut stopped = None;
        for (n, tick) in ticks(0, 600).into_iter().enumerate() {
            if let Err(e) = telemetry.record(tick) {
                stopped = Some((n, e));
                break;
            }
        }
        assert!(matches!(
            stopped,
            Some((_, RecordError::Spool(SpoolError::Full)))
        ));
        assert!(telemetry.spool().sessions()[0].ended);
        let kept = decode_spooled(telemetry.spool(), id);
        assert!(!kept.is_empty());
        assert_eq!(kept, ticks(0, kept.len() as i64));

        // Ticks after the session ended are ignored, not an error.
        telemetry.record(ticks(600, 1).remove(0)).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
