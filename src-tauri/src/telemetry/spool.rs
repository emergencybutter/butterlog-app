//! The crash-safe send buffer (design §3.5): every chunk is written here before it is
//! sent and deleted once the service has decoded it. Not an archive and not an offline
//! mode. It is bounded, and once full it refuses chunks rather than evicting unsent ones.
//!
//! One directory per session, named by its id:
//!
//! ```text
//! <spool>/<session id>/open.bin          the SessionOpen, as sent
//!                      0000000042.zst    chunk 42, zstd-compressed, as sent
//!                      end               present once the recorder ended the session
//! ```
//!
//! Every file is written to a `.tmp` and renamed, so a crash leaves either the whole file
//! or a `.tmp` that the next start deletes.
//!
//! What can't be written, because the disk is full or failing, waits in memory instead, up
//! to [`MEMORY_MAX_BYTES`], and is sent from there like the rest: the disk only makes the
//! buffer survive a crash, and losing that beats losing the flight. A full disk on
//! 2026-11-07 ended a session 75 minutes into a 9-hour flight, and the rest never reached
//! the service.

use parking_lot::Mutex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use uuid::Uuid;

/// How much may wait in memory when the disk refuses it. At ~140 kB per flight-hour, about
/// 230 hours.
pub const MEMORY_MAX_BYTES: u64 = 32 * 1024 * 1024;

const OPEN_FILE: &str = "open.bin";
const END_FILE: &str = "end";
const CHUNK_EXT: &str = "zst";
const TMP_EXT: &str = "tmp";

#[derive(Debug)]
pub enum SpoolError {
    /// Writing this would take the spool past its bound.
    Full,
    Io(io::Error),
}

impl std::fmt::Display for SpoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => write!(f, "the telemetry spool is full"),
            Self::Io(e) => write!(f, "telemetry spool: {e}"),
        }
    }
}

impl From<io::Error> for SpoolError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// A spooled session, as listed by [`Spool::sessions`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpooledSession {
    pub id: Uuid,
    /// No more chunks will be added: the recorder ended it, or it was left by an earlier
    /// run of the app, whose encoder state is gone.
    pub ended: bool,
}

pub struct Spool {
    dir: PathBuf,
    max_bytes: u64,
    used: Mutex<u64>,
    /// Sessions begun by this process. Any other is from an earlier run, so ended.
    begun_here: Mutex<HashSet<Uuid>>,
    /// What the disk refused, by session.
    memory: Mutex<Memory>,
}

#[derive(Default)]
struct Memory {
    sessions: HashMap<Uuid, InMemory>,
    bytes: u64,
}

struct InMemory {
    /// When it was begun, to list it among the others.
    begun: SystemTime,
    /// The opening, when the disk refused it: the whole session is then in memory.
    open: Option<Vec<u8>>,
    chunks: BTreeMap<u32, Vec<u8>>,
    /// The end marker could not be written.
    ended: bool,
}

impl InMemory {
    fn new() -> Self {
        Self { begun: SystemTime::now(), open: None, chunks: BTreeMap::new(), ended: false }
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension(TMP_EXT);
    let mut file = fs::File::create(&tmp)?;
    file.write_all(data)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)
}

fn chunk_seq(path: &Path) -> Option<u32> {
    if path.extension()? != CHUNK_EXT {
        return None;
    }
    path.file_stem()?.to_str()?.parse().ok()
}

impl Spool {
    /// Opens the spool at `dir`, creating it if needed, and tidies what a crash left:
    /// `.tmp` files and session directories without an opening.
    pub fn open(dir: PathBuf, max_bytes: u64) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let mut used = 0;
        for entry in fs::read_dir(&dir)? {
            let session = entry?.path();
            if !session.is_dir() {
                continue;
            }
            if !session.join(OPEN_FILE).is_file() {
                fs::remove_dir_all(&session)?;
                continue;
            }
            for file in fs::read_dir(&session)? {
                let file = file?.path();
                if file.extension().is_some_and(|e| e == TMP_EXT) {
                    fs::remove_file(&file)?;
                } else {
                    used += fs::metadata(&file)?.len();
                }
            }
        }
        Ok(Self {
            dir,
            max_bytes,
            used: Mutex::new(used),
            begun_here: Mutex::new(HashSet::new()),
            memory: Mutex::new(Memory::default()),
        })
    }

    fn session_dir(&self, id: Uuid) -> PathBuf {
        self.dir.join(id.to_string())
    }

    fn chunk_path(&self, id: Uuid, seq: u32) -> PathBuf {
        self.session_dir(id).join(format!("{seq:010}.{CHUNK_EXT}"))
    }

    /// Writes `data` at `path`, counting it against the bound.
    fn write_counted(&self, path: &Path, data: &[u8]) -> Result<(), SpoolError> {
        let len = data.len() as u64;
        {
            let mut used = self.used.lock();
            if *used + len > self.max_bytes {
                return Err(SpoolError::Full);
            }
            *used += len;
        }
        write_atomic(path, data).map_err(|e| {
            *self.used.lock() -= len;
            SpoolError::Io(e)
        })
    }

    /// Keeps `data` in memory once the disk refused it with `on_disk`. The spool's own
    /// bound still holds: [`SpoolError::Full`] is not the disk's to excuse.
    fn keep_in_memory(
        &self,
        id: Uuid,
        on_disk: SpoolError,
        put: impl FnOnce(&mut InMemory, Vec<u8>),
        data: &[u8],
    ) -> Result<(), SpoolError> {
        if matches!(on_disk, SpoolError::Full) {
            return Err(on_disk);
        }
        let mut memory = self.memory.lock();
        let len = data.len() as u64;
        if memory.bytes + len > MEMORY_MAX_BYTES {
            return Err(on_disk);
        }
        memory.bytes += len;
        put(memory.sessions.entry(id).or_insert_with(InMemory::new), data.to_vec());
        Ok(())
    }

    /// The session's whole opening is in memory, so its chunks go there too.
    fn opened_in_memory(&self, id: Uuid) -> bool {
        self.memory.lock().sessions.get(&id).is_some_and(|s| s.open.is_some())
    }

    /// Drops what memory holds of the session below `next_seq`, or all of it.
    fn forget(&self, id: Uuid, next_seq: Option<u32>) {
        let mut memory = self.memory.lock();
        let Some(session) = memory.sessions.get_mut(&id) else { return };
        let freed: u64 = match next_seq {
            Some(next) => {
                let kept = session.chunks.split_off(&next);
                let dropped = std::mem::replace(&mut session.chunks, kept);
                dropped.values().map(|c| c.len() as u64).sum()
            }
            None => {
                let session = memory.sessions.remove(&id).expect("just found");
                session.chunks.values().map(|c| c.len() as u64).sum::<u64>()
                    + session.open.map_or(0, |o| o.len() as u64)
            }
        };
        memory.bytes = memory.bytes.saturating_sub(freed);
    }

    fn release(&self, len: u64) {
        let mut used = self.used.lock();
        *used = used.saturating_sub(len);
    }

    pub fn begin(&self, id: Uuid, open: &[u8]) -> Result<(), SpoolError> {
        self.begun_here.lock().insert(id);
        let written = fs::create_dir_all(self.session_dir(id))
            .map_err(SpoolError::Io)
            .and_then(|()| self.write_counted(&self.session_dir(id).join(OPEN_FILE), open));
        match written {
            Ok(()) => Ok(()),
            Err(e) => {
                // A directory left without its opening would be tidied as a crash's.
                let _ = fs::remove_dir(self.session_dir(id));
                self.keep_in_memory(id, e, |s, d| s.open = Some(d), open)
            }
        }
    }

    pub fn append(&self, id: Uuid, seq: u32, data: &[u8]) -> Result<(), SpoolError> {
        let written = if self.opened_in_memory(id) {
            Err(SpoolError::Io(io::Error::other("the session is in memory")))
        } else {
            self.write_counted(&self.chunk_path(id, seq), data)
        };
        match written {
            Ok(()) => Ok(()),
            Err(e) => self.keep_in_memory(id, e, |s, d| { s.chunks.insert(seq, d); }, data),
        }
    }

    /// Marks the session ended. Write its last chunk first: a lister that sees the marker
    /// must also see every chunk.
    pub fn end(&self, id: Uuid) -> io::Result<()> {
        let written = if self.opened_in_memory(id) {
            Err(io::Error::other("the session is in memory"))
        } else {
            write_atomic(&self.session_dir(id).join(END_FILE), b"")
        };
        if let Err(e) = written {
            if !self.begun_here.lock().contains(&id) {
                return Err(e);
            }
            self.memory.lock().sessions.entry(id).or_insert_with(InMemory::new).ended = true;
        }
        Ok(())
    }

    /// Spooled sessions, oldest first.
    pub fn sessions(&self) -> Vec<SpooledSession> {
        // An unreadable directory still leaves what waits in memory.
        let entries = fs::read_dir(&self.dir).into_iter().flatten();
        let begun_here = self.begun_here.lock().clone();
        let memory = self.memory.lock();
        let mut sessions: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let id = Uuid::parse_str(e.file_name().to_str()?).ok()?;
                let open = fs::metadata(e.path().join(OPEN_FILE)).ok()?;
                let ended = !begun_here.contains(&id)
                    || e.path().join(END_FILE).exists()
                    || memory.sessions.get(&id).is_some_and(|s| s.ended);
                Some((open.modified().ok(), SpooledSession { id, ended }))
            })
            .collect();
        sessions.extend(
            memory
                .sessions
                .iter()
                .filter(|(_, s)| s.open.is_some())
                .map(|(&id, s)| (Some(s.begun), SpooledSession { id, ended: s.ended })),
        );
        sessions.sort_by_key(|(t, s)| (*t, s.id));
        sessions.into_iter().map(|(_, s)| s).collect()
    }

    /// The session's spooled chunks, in order.
    pub fn chunks(&self, id: Uuid) -> Vec<u32> {
        let mut seqs: Vec<u32> = fs::read_dir(self.session_dir(id))
            .map(|entries| entries.filter_map(|e| chunk_seq(&e.ok()?.path())).collect())
            .unwrap_or_default();
        if let Some(session) = self.memory.lock().sessions.get(&id) {
            seqs.extend(session.chunks.keys());
        }
        seqs.sort_unstable();
        seqs.dedup();
        seqs
    }

    pub fn read_open(&self, id: Uuid) -> io::Result<Vec<u8>> {
        if let Some(open) = self.memory.lock().sessions.get(&id).and_then(|s| s.open.clone()) {
            return Ok(open);
        }
        fs::read(self.session_dir(id).join(OPEN_FILE))
    }

    pub fn read_chunk(&self, id: Uuid, seq: u32) -> io::Result<Vec<u8>> {
        let kept = self.memory.lock().sessions.get(&id).and_then(|s| s.chunks.get(&seq).cloned());
        match kept {
            Some(chunk) => Ok(chunk),
            None => fs::read(self.chunk_path(id, seq)),
        }
    }

    /// Drops every chunk below `next_seq`: the service has decoded them.
    pub fn ack(&self, id: Uuid, next_seq: u32) -> io::Result<()> {
        self.forget(id, Some(next_seq));
        for seq in self.chunks(id).into_iter().take_while(|&s| s < next_seq) {
            let path = self.chunk_path(id, seq);
            let len = fs::metadata(&path)?.len();
            fs::remove_file(&path)?;
            self.release(len);
        }
        Ok(())
    }

    /// Drops the whole session.
    pub fn remove(&self, id: Uuid) -> io::Result<()> {
        self.forget(id, None);
        let dir = self.session_dir(id);
        if dir.exists() {
            let len: u64 = fs::read_dir(&dir)?
                .filter_map(|e| e.ok()?.metadata().ok())
                .map(|m| m.len())
                .sum();
            fs::remove_dir_all(&dir)?;
            self.release(len);
        }
        self.begun_here.lock().remove(&id);
        Ok(())
    }

    /// What it holds, on disk and in memory.
    pub fn used_bytes(&self) -> u64 {
        *self.used.lock() + self.memory.lock().bytes
    }

    /// What waits in memory because the disk refused it.
    pub fn memory_bytes(&self) -> u64 {
        self.memory.lock().bytes
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::temp_dir;

    #[test]
    fn counts_what_it_holds_and_releases_on_ack() {
        let dir = temp_dir("count");
        let spool = Spool::open(dir.clone(), 1000).unwrap();
        let id = Uuid::new_v4();
        spool.begin(id, &[0; 100]).unwrap();
        for seq in 0..4 {
            spool.append(id, seq, &[seq as u8; 50]).unwrap();
        }
        assert_eq!(spool.used_bytes(), 300);
        assert_eq!(spool.chunks(id), vec![0, 1, 2, 3]);
        assert_eq!(spool.read_chunk(id, 2).unwrap(), vec![2; 50]);

        spool.ack(id, 3).unwrap();
        assert_eq!(spool.chunks(id), vec![3]);
        assert_eq!(spool.used_bytes(), 150);

        spool.remove(id).unwrap();
        assert_eq!(spool.used_bytes(), 0);
        assert!(spool.sessions().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refuses_rather_than_evicts_when_full() {
        let dir = temp_dir("full");
        let spool = Spool::open(dir.clone(), 250).unwrap();
        let id = Uuid::new_v4();
        spool.begin(id, &[0; 100]).unwrap();
        spool.append(id, 0, &[0; 100]).unwrap();
        assert!(matches!(
            spool.append(id, 1, &[0; 100]),
            Err(SpoolError::Full)
        ));
        assert_eq!(
            spool.chunks(id),
            vec![0],
            "nothing evicted, nothing half-written"
        );
        assert_eq!(spool.used_bytes(), 200);

        spool.ack(id, 1).unwrap();
        spool.append(id, 1, &[0; 100]).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    /// Makes the disk refuse chunk `seq` of the session: its `.tmp` can't be created.
    fn refuse(dir: &Path, id: Uuid, seq: u32) {
        fs::create_dir_all(dir.join(id.to_string()).join(format!("{seq:010}.{TMP_EXT}"))).unwrap();
    }

    #[test]
    fn chunks_the_disk_refuses_wait_in_memory() {
        let dir = temp_dir("refused");
        let spool = Spool::open(dir.clone(), 10_000).unwrap();
        let id = Uuid::new_v4();
        spool.begin(id, &[0; 10]).unwrap();
        spool.append(id, 0, &[0; 20]).unwrap();
        refuse(&dir, id, 1);
        refuse(&dir, id, 2);
        spool.append(id, 1, &[1; 30]).unwrap();
        spool.append(id, 2, &[2; 40]).unwrap();
        spool.append(id, 3, &[3; 50]).unwrap();
        assert_eq!(spool.chunks(id), vec![0, 1, 2, 3]);
        assert_eq!(spool.read_chunk(id, 2).unwrap(), vec![2; 40]);
        assert_eq!(spool.memory_bytes(), 70);
        assert_eq!(spool.used_bytes(), 10 + 20 + 70 + 50);

        // The end marker refused too: ended all the same.
        fs::create_dir_all(dir.join(id.to_string()).join(format!("{END_FILE}.{TMP_EXT}"))).unwrap();
        spool.end(id).unwrap();
        assert_eq!(spool.sessions(), vec![SpooledSession { id, ended: true }]);

        spool.ack(id, 2).unwrap();
        assert_eq!(spool.chunks(id), vec![2, 3]);
        assert_eq!(spool.memory_bytes(), 40);
        spool.ack(id, 4).unwrap();
        assert_eq!(spool.memory_bytes(), 0);
        spool.remove(id).unwrap();
        assert!(spool.sessions().is_empty());
        assert_eq!(spool.used_bytes(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_session_the_disk_refuses_from_the_start_lives_in_memory() {
        let dir = temp_dir("refused-open");
        let spool = Spool::open(dir.clone(), 10_000).unwrap();
        // The spool's directory is gone and a file stands in its place: nothing can be
        // written under it.
        fs::remove_dir_all(&dir).unwrap();
        fs::write(&dir, b"").unwrap();
        let id = Uuid::new_v4();
        spool.begin(id, &[7; 10]).unwrap();
        spool.append(id, 0, &[0; 20]).unwrap();
        spool.append(id, 1, &[1; 20]).unwrap();
        assert_eq!(spool.sessions(), vec![SpooledSession { id, ended: false }]);
        assert_eq!(spool.read_open(id).unwrap(), vec![7; 10]);
        assert_eq!(spool.chunks(id), vec![0, 1]);
        spool.end(id).unwrap();
        assert!(spool.sessions()[0].ended);
        spool.remove(id).unwrap();
        assert!(spool.sessions().is_empty());
        assert_eq!(spool.memory_bytes(), 0);
        fs::remove_file(&dir).unwrap();
    }

    #[test]
    fn memory_is_bounded_too() {
        let dir = temp_dir("refused-bound");
        let spool = Spool::open(dir.clone(), u64::MAX).unwrap();
        let id = Uuid::new_v4();
        spool.begin(id, &[0; 10]).unwrap();
        refuse(&dir, id, 0);
        refuse(&dir, id, 1);
        let half = vec![0; (MEMORY_MAX_BYTES / 2 + 1) as usize];
        spool.append(id, 0, &half).unwrap();
        assert!(matches!(spool.append(id, 1, &half), Err(SpoolError::Io(_))));
        assert_eq!(spool.chunks(id), vec![0]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_restart_recounts_tidies_and_ends_what_it_finds() {
        let dir = temp_dir("restart");
        let (live, ended) = (Uuid::new_v4(), Uuid::new_v4());
        {
            let spool = Spool::open(dir.clone(), 10_000).unwrap();
            spool.begin(ended, &[0; 10]).unwrap();
            spool.append(ended, 0, &[0; 20]).unwrap();
            spool.end(ended).unwrap();
            spool.begin(live, &[0; 10]).unwrap();
            spool.append(live, 0, &[0; 30]).unwrap();
            let listed = spool.sessions();
            assert_eq!(listed.len(), 2);
            assert!(listed.iter().any(|s| s.id == live && !s.ended));
        }
        // What a crash leaves: a half-written chunk, and a session that never got its
        // opening written.
        fs::write(dir.join(live.to_string()).join("0000000001.tmp"), [0; 99]).unwrap();
        fs::create_dir_all(dir.join(Uuid::new_v4().to_string())).unwrap();

        let spool = Spool::open(dir.clone(), 10_000).unwrap();
        assert_eq!(spool.used_bytes(), 70);
        let listed = spool.sessions();
        assert_eq!(listed.len(), 2);
        assert!(
            listed.iter().all(|s| s.ended),
            "the encoder that fed them is gone"
        );
        assert_eq!(spool.chunks(live), vec![0]);
        fs::remove_dir_all(dir).unwrap();
    }
}
