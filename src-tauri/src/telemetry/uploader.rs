//! Sends the spool to the service (`docs/API.md`, Telemetry Sessions), oldest session
//! first, and drops what the service acknowledges.
//!
//! Every call is idempotent, so the uploader keeps no state worth persisting: after a
//! crash it opens each spooled session again and resends whatever is still spooled. A
//! session left by an earlier run is ended once its chunks are in, since the encoder that
//! fed it is gone.

use super::spool::Spool;
use super::Telemetry;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::collections::HashSet;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

/// How often the spool is sent: one chunk's worth of wall time.
const PASS_INTERVAL_SECS: u64 = 5;

/// Past this share of its bound, the spool is reported as growing.
const SPOOL_WARN_FRACTION: f64 = 0.5;

/// The service's answer to one call.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Reply {
    /// Accepted. Every chunk below `next_seq` is decoded on the service.
    Ok {
        next_seq: u32,
        status: String,
    },
    Unauthorized,
    /// No such session for this user.
    NotFound,
    /// The session is compacted and takes no more chunks.
    Gone,
    /// Sending the same bytes again will not help.
    Refused(String),
    /// Unreachable, or failing on its side: try again later.
    Unavailable(String),
}

/// The three calls, as a trait so the pass can be tested against a fake service.
pub(crate) trait Service {
    async fn open(&self, id: Uuid, body: Vec<u8>) -> Reply;
    async fn chunk(&self, id: Uuid, seq: u32, body: Vec<u8>) -> Reply;
    async fn end(&self, id: Uuid) -> Reply;
}

/// Why a pass stopped before the end of the spool.
#[derive(Debug, PartialEq)]
pub(crate) enum Halt {
    Unauthorized,
    Unavailable(String),
}

/// What a pass keeps between passes, for this run of the app only.
#[derive(Default)]
pub(crate) struct PassState {
    /// Opened since the app started.
    opened: HashSet<Uuid>,
    /// Refused by the service: left in the spool, and retried on the next start.
    parked: HashSet<Uuid>,
}

/// One pass over the spool. `log` gets a line per noteworthy outcome.
pub(crate) async fn upload_pass<S: Service>(
    spool: &Spool,
    service: &S,
    state: &mut PassState,
    log: &mut impl FnMut(String),
) -> Result<(), Halt> {
    for session in spool.sessions() {
        let id = session.id;
        if state.parked.contains(&id) {
            continue;
        }

        if !state.opened.contains(&id) {
            let body = match spool.read_open(id) {
                Ok(body) => body,
                Err(e) => {
                    log(format!("[Telemetry] Session {id} unreadable, parked: {e}"));
                    state.parked.insert(id);
                    continue;
                }
            };
            match service.open(id, body).await {
                Reply::Ok { next_seq, status } => {
                    if status == "compacted" {
                        log(format!(
                            "[Telemetry] Session {id} already compacted; dropped"
                        ));
                        let _ = spool.remove(id);
                        continue;
                    }
                    let _ = spool.ack(id, next_seq);
                    state.opened.insert(id);
                }
                Reply::Unauthorized => return Err(Halt::Unauthorized),
                Reply::Unavailable(e) => return Err(Halt::Unavailable(e)),
                Reply::Gone => {
                    let _ = spool.remove(id);
                    continue;
                }
                reply @ (Reply::NotFound | Reply::Refused(_)) => {
                    log(format!(
                        "[Telemetry] Session {id} refused, parked: {reply:?}"
                    ));
                    state.parked.insert(id);
                    continue;
                }
            }
        }

        let mut reopen = false;
        for seq in spool.chunks(id) {
            let body = match spool.read_chunk(id, seq) {
                Ok(body) => body,
                Err(e) => {
                    log(format!(
                        "[Telemetry] Chunk {seq} of {id} unreadable, parked: {e}"
                    ));
                    state.parked.insert(id);
                    break;
                }
            };
            match service.chunk(id, seq, body).await {
                Reply::Ok { next_seq, .. } => {
                    let _ = spool.ack(id, next_seq);
                    // Sent in order from the service's own cursor, so a chunk that does
                    // not move it failed to decode, and holds the session for good.
                    if next_seq <= seq {
                        log(format!(
                            "[Telemetry] Session {id} stuck at chunk {next_seq}; parked"
                        ));
                        state.parked.insert(id);
                        break;
                    }
                }
                Reply::Unauthorized => return Err(Halt::Unauthorized),
                Reply::Unavailable(e) => return Err(Halt::Unavailable(e)),
                Reply::NotFound => {
                    // Opened earlier in this run, yet unknown now: open it again.
                    state.opened.remove(&id);
                    reopen = true;
                    break;
                }
                Reply::Gone => {
                    log(format!(
                        "[Telemetry] Session {id} compacted before chunk {seq}; dropped"
                    ));
                    let _ = spool.remove(id);
                    break;
                }
                Reply::Refused(e) => {
                    log(format!(
                        "[Telemetry] Chunk {seq} of {id} refused, parked: {e}"
                    ));
                    state.parked.insert(id);
                    break;
                }
            }
        }
        if reopen || state.parked.contains(&id) || !spool.chunks(id).is_empty() {
            continue;
        }

        // `ended` was read before the chunks were listed, so every chunk of an ended
        // session is in by now.
        if session.ended {
            match service.end(id).await {
                Reply::Ok { .. } | Reply::NotFound | Reply::Gone => {
                    let _ = spool.remove(id);
                    state.opened.remove(&id);
                }
                Reply::Unauthorized => return Err(Halt::Unauthorized),
                Reply::Unavailable(e) => return Err(Halt::Unavailable(e)),
                Reply::Refused(e) => {
                    log(format!("[Telemetry] End of {id} refused, parked: {e}"));
                    state.parked.insert(id);
                }
            }
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ack {
    next_seq: i64,
    #[serde(default)]
    status: Option<String>,
}

/// The service over HTTP, with the app's Bearer token.
pub(crate) struct HttpService {
    pub client: Client,
    pub base_url: String,
    pub token: String,
}

impl HttpService {
    async fn send(&self, request: reqwest::RequestBuilder) -> Reply {
        let response = match request.bearer_auth(&self.token).send().await {
            Ok(r) => r,
            Err(e) => return Reply::Unavailable(e.to_string()),
        };
        let status = response.status();
        match status {
            StatusCode::OK => match response.json::<Ack>().await {
                Ok(ack) => Reply::Ok {
                    next_seq: ack.next_seq.max(0) as u32,
                    status: ack.status.unwrap_or_else(|| "open".to_string()),
                },
                Err(e) => Reply::Unavailable(format!("unreadable reply: {e}")),
            },
            StatusCode::UNAUTHORIZED => Reply::Unauthorized,
            StatusCode::NOT_FOUND => Reply::NotFound,
            StatusCode::GONE => Reply::Gone,
            StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS => {
                Reply::Unavailable(status.to_string())
            }
            s if s.is_server_error() => Reply::Unavailable(s.to_string()),
            s => Reply::Refused(format!(
                "{s}: {}",
                response.text().await.unwrap_or_default()
            )),
        }
    }
}

impl Service for HttpService {
    async fn open(&self, id: Uuid, body: Vec<u8>) -> Reply {
        let url = format!("{}/sessions/{id}", self.base_url);
        self.send(
            self.client
                .put(url)
                .header("Content-Type", "application/octet-stream")
                .body(body),
        )
        .await
    }

    async fn chunk(&self, id: Uuid, seq: u32, body: Vec<u8>) -> Reply {
        let url = format!("{}/sessions/{id}/chunks/{seq}", self.base_url);
        self.send(
            self.client
                .put(url)
                .header("Content-Type", "application/octet-stream")
                .body(body),
        )
        .await
    }

    async fn end(&self, id: Uuid) -> Reply {
        self.send(
            self.client
                .post(format!("{}/sessions/{id}/end", self.base_url)),
        )
        .await
    }
}

/// Starts the upload loop. Idle while the spool is empty or there is no token.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let client = Client::new();
        let mut state = PassState::default();
        let mut warned = false;
        let mut was_unavailable = false;
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(PASS_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let Some(telemetry) = app.try_state::<Telemetry>() else {
                continue;
            };
            let spool = telemetry.spool();

            let growing =
                spool.used_bytes() as f64 > spool.max_bytes() as f64 * SPOOL_WARN_FRACTION;
            if growing && !warned {
                crate::append_log(
                    &app,
                    format!(
                        "[Telemetry] Spool is growing: {} of {} MB unsent",
                        spool.used_bytes() / (1024 * 1024),
                        spool.max_bytes() / (1024 * 1024)
                    ),
                );
            }
            warned = growing;

            if spool.sessions().is_empty() {
                continue;
            }
            let config = app.state::<crate::config::ConfigManager>().get_config();
            let Some((base_url, token)) = config.api_auth() else {
                continue;
            };
            let service = HttpService {
                client: client.clone(),
                base_url,
                token,
            };
            let mut log = |line: String| crate::append_log(&app, line);
            match upload_pass(spool, &service, &mut state, &mut log).await {
                Ok(()) => {
                    if was_unavailable {
                        crate::append_log(&app, "[Telemetry] Service reachable again".to_string());
                    }
                    was_unavailable = false;
                }
                Err(Halt::Unauthorized) => {
                    crate::force_logout(&app, "service rejected token during telemetry upload");
                }
                Err(Halt::Unavailable(e)) => {
                    if !was_unavailable {
                        crate::append_log(&app, format!("[Telemetry] Upload paused: {e}"));
                    }
                    was_unavailable = true;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::tests::{open, ticks};
    use crate::telemetry::{temp_dir, Telemetry, SPOOL_MAX_BYTES};
    use butterlog_wire::{decompress, SessionDecoder, SessionOpen, Tick};
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Mutex;

    struct FakeSession {
        status: String,
        next_seq: u32,
        pending: BTreeMap<u32, Vec<u8>>,
        decoder: SessionDecoder,
        ticks: Vec<Tick>,
    }

    /// The service's semantics, in memory: chunks decode in order as they become
    /// contiguous, and everything is idempotent.
    #[derive(Default)]
    struct Fake {
        sessions: Mutex<HashMap<Uuid, FakeSession>>,
        /// Answers this many more calls, then is unreachable.
        calls_left: Mutex<Option<usize>>,
        refuse_opens: bool,
        chunk_calls: Mutex<usize>,
    }

    impl Fake {
        fn reachable(&self) -> bool {
            let mut left = self.calls_left.lock().unwrap();
            match left.as_mut() {
                Some(0) => false,
                Some(n) => {
                    *n -= 1;
                    true
                }
                None => true,
            }
        }

        fn ticks(&self, id: Uuid) -> Vec<Tick> {
            self.sessions.lock().unwrap()[&id].ticks.clone()
        }

        fn status(&self, id: Uuid) -> String {
            self.sessions.lock().unwrap()[&id].status.clone()
        }
    }

    impl Service for Fake {
        async fn open(&self, id: Uuid, body: Vec<u8>) -> Reply {
            if !self.reachable() {
                return Reply::Unavailable("down".into());
            }
            if self.refuse_opens {
                return Reply::Refused("422: protocol version".into());
            }
            let (opening, _) = SessionOpen::decode(&body).unwrap();
            let mut sessions = self.sessions.lock().unwrap();
            let s = sessions.entry(id).or_insert_with(|| FakeSession {
                status: "open".into(),
                next_seq: 0,
                pending: BTreeMap::new(),
                decoder: SessionDecoder::new(opening.channels),
                ticks: Vec::new(),
            });
            Reply::Ok {
                next_seq: s.next_seq,
                status: s.status.clone(),
            }
        }

        async fn chunk(&self, id: Uuid, seq: u32, body: Vec<u8>) -> Reply {
            if !self.reachable() {
                return Reply::Unavailable("down".into());
            }
            *self.chunk_calls.lock().unwrap() += 1;
            let mut sessions = self.sessions.lock().unwrap();
            let Some(s) = sessions.get_mut(&id) else {
                return Reply::NotFound;
            };
            if s.status == "compacted" {
                return Reply::Gone;
            }
            s.pending.insert(seq, body);
            while let Some(body) = s.pending.remove(&s.next_seq) {
                let decoded = s.decoder.decode(&decompress(&body).unwrap()).unwrap();
                s.ticks.extend(decoded.ticks);
                s.next_seq += 1;
            }
            Reply::Ok {
                next_seq: s.next_seq,
                status: s.status.clone(),
            }
        }

        async fn end(&self, id: Uuid) -> Reply {
            if !self.reachable() {
                return Reply::Unavailable("down".into());
            }
            let mut sessions = self.sessions.lock().unwrap();
            let s = sessions.get_mut(&id).unwrap();
            s.status = "ended".into();
            Reply::Ok {
                next_seq: s.next_seq,
                status: s.status.clone(),
            }
        }
    }

    fn recorder(dir: &std::path::Path) -> Telemetry {
        Telemetry::new(Spool::open(dir.to_path_buf(), SPOOL_MAX_BYTES).unwrap())
    }

    async fn pass(spool: &Spool, fake: &Fake, state: &mut PassState) -> Result<(), Halt> {
        upload_pass(spool, fake, state, &mut |_| {}).await
    }

    #[tokio::test]
    async fn sends_a_session_ends_it_and_empties_the_spool() {
        let dir = temp_dir("upload");
        let telemetry = recorder(&dir);
        let id = telemetry.start(&open()).unwrap();
        for tick in ticks(0, 23) {
            telemetry.record(tick).unwrap();
        }
        telemetry.stop().unwrap();

        let fake = Fake::default();
        pass(telemetry.spool(), &fake, &mut PassState::default())
            .await
            .unwrap();
        assert_eq!(fake.ticks(id), ticks(0, 23));
        assert_eq!(fake.status(id), "ended");
        assert!(telemetry.spool().sessions().is_empty());
        assert_eq!(telemetry.spool().used_bytes(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_live_session_is_streamed_but_not_ended() {
        let dir = temp_dir("upload-live");
        let telemetry = recorder(&dir);
        let id = telemetry.start(&open()).unwrap();
        let fake = Fake::default();
        let mut state = PassState::default();
        for tick in ticks(0, 12) {
            telemetry.record(tick).unwrap();
        }
        pass(telemetry.spool(), &fake, &mut state).await.unwrap();
        for tick in ticks(12, 6) {
            telemetry.record(tick).unwrap();
        }
        pass(telemetry.spool(), &fake, &mut state).await.unwrap();

        assert_eq!(fake.ticks(id), ticks(0, 18));
        assert_eq!(fake.status(id), "open");
        assert!(telemetry.spool().chunks(id).is_empty());
        assert_eq!(telemetry.spool().sessions().len(), 1);
        assert_eq!(*fake.chunk_calls.lock().unwrap(), 3, "nothing sent twice");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn an_outage_then_a_crash_loses_nothing_spooled() {
        let dir = temp_dir("upload-crash");
        let fake = Fake::default();
        let id = {
            let telemetry = recorder(&dir);
            let id = telemetry.start(&open()).unwrap();
            for tick in ticks(0, 30) {
                telemetry.record(tick).unwrap();
            }
            // The service answers the opening and two chunks, then goes away.
            *fake.calls_left.lock().unwrap() = Some(3);
            let halted = pass(telemetry.spool(), &fake, &mut PassState::default()).await;
            assert!(matches!(halted, Err(Halt::Unavailable(_))));
            assert_eq!(telemetry.spool().chunks(id), vec![2, 3, 4]);
            // The app dies here, mid-session, without ending it.
            id
        };

        *fake.calls_left.lock().unwrap() = None;
        let spool = Spool::open(dir.clone(), SPOOL_MAX_BYTES).unwrap();
        pass(&spool, &fake, &mut PassState::default())
            .await
            .unwrap();
        assert_eq!(fake.ticks(id), ticks(0, 30));
        assert_eq!(
            fake.status(id),
            "ended",
            "its encoder is gone, so the session is over"
        );
        assert!(spool.sessions().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_refused_session_is_parked_with_its_data() {
        let dir = temp_dir("upload-refused");
        let telemetry = recorder(&dir);
        let id = telemetry.start(&open()).unwrap();
        for tick in ticks(0, 12) {
            telemetry.record(tick).unwrap();
        }
        telemetry.stop().unwrap();

        let fake = Fake {
            refuse_opens: true,
            ..Default::default()
        };
        let mut state = PassState::default();
        pass(telemetry.spool(), &fake, &mut state).await.unwrap();
        pass(telemetry.spool(), &fake, &mut state).await.unwrap();
        assert_eq!(
            telemetry.spool().chunks(id).len(),
            2,
            "kept for the next start"
        );
        assert!(state.parked.contains(&id));
        assert_eq!(*fake.chunk_calls.lock().unwrap(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Against a real service, as an anonymous account: `BUTTERLOG_E2E_URL` is its API base,
    /// e.g. `http://127.0.0.1:18080/api/v0`. Run with `--ignored`.
    #[tokio::test]
    #[ignore]
    async fn end_to_end_against_a_real_service() {
        let base_url = std::env::var("BUTTERLOG_E2E_URL").expect("BUTTERLOG_E2E_URL");
        let client = Client::new();
        let account: serde_json::Value = client
            .post(format!("{base_url}/anonymous"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let service = HttpService {
            client: client.clone(),
            base_url: base_url.clone(),
            token: account["token"].as_str().unwrap().to_string(),
        };

        let dir = temp_dir("e2e");
        let telemetry = recorder(&dir);
        let id = telemetry.start(&open()).unwrap();
        for tick in ticks(0, 15) {
            telemetry.record(tick).unwrap();
        }
        let mut state = PassState::default();
        let mut log = |line: String| println!("{line}");
        upload_pass(telemetry.spool(), &service, &mut state, &mut log).await.unwrap();
        assert!(telemetry.spool().chunks(id).is_empty(), "the service decoded both chunks");

        // A crash: the rest is spooled, and a fresh run sends it and ends the session.
        for tick in ticks(15, 15) {
            telemetry.record(tick).unwrap();
        }
        drop(telemetry);
        let spool = Spool::open(dir.clone(), SPOOL_MAX_BYTES).unwrap();
        upload_pass(&spool, &service, &mut PassState::default(), &mut log).await.unwrap();
        assert!(spool.sessions().is_empty());

        let cursor: serde_json::Value = client
            .get(format!("{base_url}/sessions/{id}"))
            .bearer_auth(&service.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(cursor["status"], "ended");
        assert_eq!(cursor["nextSeq"], 5);
        assert_eq!(cursor["chunkCount"], 5);
        std::fs::remove_dir_all(dir).unwrap();
        println!("session {id} for user {}", account["userId"]);
    }

    #[tokio::test]
    async fn a_compacted_session_is_dropped() {
        let dir = temp_dir("upload-gone");
        let telemetry = recorder(&dir);
        let id = telemetry.start(&open()).unwrap();
        for tick in ticks(0, 12) {
            telemetry.record(tick).unwrap();
        }
        let fake = Fake::default();
        let mut state = PassState::default();
        pass(telemetry.spool(), &fake, &mut state).await.unwrap();
        fake.sessions.lock().unwrap().get_mut(&id).unwrap().status = "compacted".into();
        for tick in ticks(12, 6) {
            telemetry.record(tick).unwrap();
        }
        pass(telemetry.spool(), &fake, &mut state).await.unwrap();
        assert!(telemetry.spool().sessions().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
