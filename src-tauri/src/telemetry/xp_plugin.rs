//! The X-Plane plugin's telemetry link (design §4.2, rollout phase 4): `butterlog_xp_plugin`
//! listens on `127.0.0.1:PORT` and, once subscribed, sends the datarefs the X-Plane sampler
//! records on every sim frame, stamped with the sim clock. The sampler then records with
//! `capture: "xp_plugin"` instead of from the Web API's ~8 updates a second.
//!
//! The plugin only reads datarefs; quantising and sampling stay here, in the sampler shared
//! with MSFS. Without the plugin, or with one that speaks another protocol, the Web API
//! remains the capture path.
//!
//! The protocol is described in the plugin's `src/link.rs`. Both sides test the same golden
//! messages.

use super::xplane::XplaneSampler;
use parking_lot::Mutex;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

pub const PORT: u16 = 49021;
pub const PROTOCOL: u16 = 1;

const HELLO: u8 = 1;
const SUBSCRIBED: u8 = 2;
const FRAME: u8 = 3;
const EVENT: u8 = 4;
const SUBSCRIBE: u8 = 1;

pub const EVENT_AIRCRAFT_LOADED: u8 = 1;
pub const EVENT_AIRPORT_LOADED: u8 = 2;

const SCALAR: u16 = u16::MAX;
const MAX_MESSAGE: usize = 1 << 20;

#[derive(Debug, PartialEq)]
pub struct Hello {
    pub protocol: u16,
    pub plugin_version: String,
    pub xplane_version: i32,
}

#[derive(Debug, PartialEq)]
pub enum Message {
    Hello(Hello),
    Subscribed(Vec<bool>),
    Frame { sim_time: f64, wall_ms: i64, dropped: u32, values: Vec<f64> },
    Event { kind: u8, detail: String },
    /// A type this version doesn't know; skipped.
    Other(u8),
}

struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

/// A message's type byte and payload.
pub fn decode(kind: u8, payload: &[u8]) -> Option<Message> {
    let mut c = Cursor(payload);
    let message = match kind {
        HELLO => Message::Hello(Hello { protocol: c.u16()?, plugin_version: c.str()?, xplane_version: c.i32()? }),
        SUBSCRIBED => {
            let n = c.u16()? as usize;
            Message::Subscribed((0..n).map(|_| c.u8().map(|f| f != 0)).collect::<Option<_>>()?)
        }
        FRAME => {
            let (sim_time, wall_ms, dropped) = (c.f64()?, c.i64()?, c.u32()?);
            let n = c.u16()? as usize;
            let values = (0..n).map(|_| c.f64()).collect::<Option<_>>()?;
            Message::Frame { sim_time, wall_ms, dropped, values }
        }
        EVENT => Message::Event { kind: c.u8()?, detail: c.str()? },
        other => return Some(Message::Other(other)),
    };
    c.0.is_empty().then_some(message)
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u16).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// The subscription: dataref and element (`None` for a scalar), in channel order.
pub fn subscribe(entries: &[(&str, Option<usize>)]) -> Vec<u8> {
    let mut p = (entries.len() as u16).to_le_bytes().to_vec();
    for (name, element) in entries {
        put_str(&mut p, name);
        p.extend_from_slice(&element.map_or(SCALAR, |e| e as u16).to_le_bytes());
    }
    let mut out = (p.len() as u32 + 1).to_le_bytes().to_vec();
    out.push(SUBSCRIBE);
    out.extend_from_slice(&p);
    out
}

/// Reads one whole message, waiting for it (up to the stream's read timeout).
fn read_message(stream: &mut TcpStream) -> std::io::Result<Message> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len == 0 || len > MAX_MESSAGE {
        return Err(std::io::Error::new(ErrorKind::InvalidData, format!("message of {len} bytes")));
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body)?;
    decode(body[0], &body[1..]).ok_or_else(|| std::io::Error::new(ErrorKind::InvalidData, format!("bad message of type {}", body[0])))
}

/// A subscribed link, not yet streaming into a sampler.
pub struct Connected {
    stream: TcpStream,
    pub hello: Hello,
    /// Per subscribed value, whether X-Plane has the dataref.
    pub found: Vec<bool>,
}

/// How long the plugin may take to answer a subscription: X-Plane loading a flight.
const SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(120);

/// Connects to the plugin and subscribes. `None`, logged, when there is no plugin, or it
/// isn't one this app understands; the Web API is used then. Blocking: a missing plugin
/// fails within the 500 ms connect timeout and a silent one within 3 s, but a plugin that
/// has said hello is waited on while X-Plane loads, up to [`SUBSCRIBE_TIMEOUT`].
pub fn connect(app: &AppHandle, entries: &[(&str, Option<usize>)]) -> Option<Connected> {
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    let mut stream = match TcpStream::connect_timeout(&addr, Duration::from_millis(500)) {
        Ok(s) => s,
        Err(e) => {
            crate::append_log(app, format!("[Telemetry] No X-Plane plugin link ({e}); recording through the Web API."));
            return None;
        }
    };
    let handshake = (|| -> std::io::Result<Connected> {
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_nodelay(true)?;
        let Message::Hello(hello) = read_message(&mut stream)? else {
            return Err(std::io::Error::new(ErrorKind::InvalidData, "no hello"));
        };
        if hello.protocol != PROTOCOL {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                format!("plugin {} speaks protocol {}, this app {PROTOCOL}", hello.plugin_version, hello.protocol),
            ));
        }
        stream.write_all(&subscribe(entries))?;
        // HELLO comes from the plugin's network thread, SUBSCRIBED from X-Plane's flight
        // loop, which doesn't run while X-Plane is still loading. The Web API answers well
        // before then, so this can wait for the loading to end.
        stream.set_read_timeout(Some(SUBSCRIBE_TIMEOUT))?;
        let found = loop {
            match read_message(&mut stream)? {
                Message::Subscribed(found) if found.len() == entries.len() => break found,
                Message::Subscribed(found) => {
                    return Err(std::io::Error::new(
                        ErrorKind::InvalidData,
                        format!("subscribed to {} of {} values", found.len(), entries.len()),
                    ))
                }
                _ => {}
            }
        };
        Ok(Connected { stream, hello, found })
    })();
    match handshake {
        Ok(c) => {
            let missing: Vec<_> =
                entries.iter().zip(&c.found).filter(|(_, f)| !**f).map(|((name, _), _)| *name).collect();
            crate::append_log(app, format!(
                "[Telemetry] X-Plane plugin {} link up: {} of {} values found{}.",
                c.hello.plugin_version,
                c.found.iter().filter(|f| **f).count(),
                entries.len(),
                if missing.is_empty() { String::new() } else { format!(" (missing: {})", missing.join(", ")) }
            ));
            Some(c)
        }
        Err(e) => {
            crate::append_log(app, format!("[Telemetry] X-Plane plugin link failed ({e}); recording through the Web API."));
            None
        }
    }
}

/// Streams frames into the sampler until dropped. Dropping ends the thread without telling
/// the sampler the link was lost: the connection it belongs to is ending anyway.
pub struct Running {
    stream: TcpStream,
    stop: Arc<AtomicBool>,
}

impl Connected {
    pub fn run(self, app: AppHandle, sampler: Arc<Mutex<XplaneSampler>>) -> Option<Running> {
        let stop = Arc::new(AtomicBool::new(false));
        let control = self.stream.try_clone().ok()?;
        let mut stream = self.stream;
        let found = self.found;
        let thread_stop = stop.clone();
        let spawned = std::thread::Builder::new().name("xp-plugin-link".into()).spawn(move || {
            // Frames come every sim frame; a long silence means X-Plane hung or is loading.
            // That's not a lost link, so only errors and EOF end it.
            let _ = stream.set_read_timeout(None);
            let mut reported_drops = 0;
            let error = loop {
                match read_message(&mut stream) {
                    Ok(Message::Frame { sim_time, wall_ms, dropped, values }) => {
                        if dropped >= reported_drops + 100 {
                            reported_drops = dropped;
                            crate::append_log(&app, format!("[Telemetry] The X-Plane plugin dropped {dropped} frames so far: this app fell behind."));
                        }
                        sampler.lock().on_frame(sim_time, wall_ms, &values, &found);
                    }
                    Ok(Message::Event { kind, detail }) => sampler.lock().on_plugin_event(kind, detail),
                    Ok(_) => {}
                    Err(e) => break e,
                }
            };
            if !thread_stop.load(Ordering::Relaxed) {
                crate::append_log(&app, format!("[Telemetry] X-Plane plugin link lost ({error})."));
                sampler.lock().plugin_lost();
            }
        });
        spawned.ok().map(|_| Running { stream: control, stop })
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plugin's `link.rs` encodes these same bytes.
    #[test]
    fn golden_messages() {
        let hello = [1, 0, 5, 0, b'0', b'.', b'3', b'.', b'0', 0x38, 0xDA, 0x01, 0x00];
        assert_eq!(
            decode(HELLO, &hello),
            Some(Message::Hello(Hello { protocol: 1, plugin_version: "0.3.0".into(), xplane_version: 121_400 }))
        );
        let mut frame = Vec::new();
        frame.extend_from_slice(&2.0f64.to_le_bytes());
        frame.extend_from_slice(&3i64.to_le_bytes());
        frame.extend_from_slice(&[4, 0, 0, 0, 1, 0]);
        frame.extend_from_slice(&1.5f64.to_le_bytes());
        assert_eq!(decode(FRAME, &frame), Some(Message::Frame { sim_time: 2.0, wall_ms: 3, dropped: 4, values: vec![1.5] }));
        assert_eq!(
            decode(EVENT, &[1, 5, 0, b'a', b'.', b'a', b'c', b'f']),
            Some(Message::Event { kind: EVENT_AIRCRAFT_LOADED, detail: "a.acf".into() })
        );
        assert_eq!(decode(SUBSCRIBED, &[2, 0, 1, 0]), Some(Message::Subscribed(vec![true, false])));
        assert_eq!(decode(FRAME, &frame[..frame.len() - 1]), None, "truncated");
        assert_eq!(decode(99, &[1, 2]), Some(Message::Other(99)));
    }

    #[test]
    fn subscriptions_encode_as_the_plugin_decodes() {
        let mut expected = vec![0, 0, 0, 0, SUBSCRIBE, 2, 0];
        expected.extend_from_slice(&[5, 0]);
        expected.extend_from_slice(b"sim/a");
        expected.extend_from_slice(&SCALAR.to_le_bytes());
        expected.extend_from_slice(&[5, 0]);
        expected.extend_from_slice(b"sim/b");
        expected.extend_from_slice(&[1, 0]);
        expected[0] = (expected.len() - 4) as u8;
        assert_eq!(subscribe(&[("sim/a", None), ("sim/b", Some(1))]), expected);
    }

    /// The whole exchange against a fake plugin on a loopback port.
    #[test]
    fn handshake_and_frames_over_tcp() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let plugin = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let hello = [14u32.to_le_bytes().as_slice(), &[HELLO, 1, 0, 5, 0], b"0.3.0", &121_400i32.to_le_bytes()].concat();
            s.write_all(&hello).unwrap();
            let mut len = [0u8; 4];
            s.read_exact(&mut len).unwrap();
            let mut body = vec![0u8; u32::from_le_bytes(len) as usize];
            s.read_exact(&mut body).unwrap();
            assert_eq!(body[0], SUBSCRIBE);
            s.write_all(&[5, 0, 0, 0, SUBSCRIBED, 2, 0, 1, 1]).unwrap();
        });
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let Message::Hello(hello) = read_message(&mut stream).unwrap() else { panic!() };
        assert_eq!(hello.protocol, PROTOCOL);
        stream.write_all(&subscribe(&[("sim/a", None), ("sim/b", Some(0))])).unwrap();
        assert_eq!(read_message(&mut stream).unwrap(), Message::Subscribed(vec![true, true]));
        plugin.join().unwrap();
    }
}
