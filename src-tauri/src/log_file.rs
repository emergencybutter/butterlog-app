//! The backend log, written to a file as well as the in-memory buffer the log view shows,
//! so a run can be read after the fact.
//!
//! One file a day, `butterlog-YYYY-MM-DD.log` under `<app data>/logs` (`logs-dev` with
//! `--dev`). Files older than `KEEP_DAYS` are deleted when a new day's file opens. A day's
//! file stops growing at `MAX_FILE_BYTES`, with a line saying so, so a runaway loop can't
//! fill the disk.

use chrono::{Local, NaiveDate};
use parking_lot::Mutex;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const KEEP_DAYS: i64 = 7;
const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
const PREFIX: &str = "butterlog-";
const SUFFIX: &str = ".log";

pub fn dir_name_from_args(args: &[String]) -> &'static str {
    if args.iter().any(|a| a == "--dev") { "logs-dev" } else { "logs" }
}

struct Open {
    day: NaiveDate,
    file: File,
    written: u64,
    full: bool,
}

pub struct LogFile {
    dir: PathBuf,
    open: Mutex<Option<Open>>,
}

fn file_name(day: NaiveDate) -> String {
    format!("{PREFIX}{}{SUFFIX}", day.format("%Y-%m-%d"))
}

/// The day a log file belongs to, from its name.
fn day_of(name: &str) -> Option<NaiveDate> {
    let date = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

/// Deletes the log files more than `KEEP_DAYS` older than `today`.
fn prune(dir: &Path, today: NaiveDate) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(day) = name.to_str().and_then(day_of) else { continue };
        if (today - day).num_days() >= KEEP_DAYS {
            let _ = fs::remove_file(entry.path());
        }
    }
}

impl LogFile {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, open: Mutex::new(None) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Today's file, which may not exist yet.
    pub fn current(&self) -> PathBuf {
        self.dir.join(file_name(Local::now().date_naive()))
    }

    /// Appends a line (a timestamped log message). Failures are dropped: logging must never
    /// take the app down, and there's nowhere to report them but the log.
    pub fn write(&self, line: &str) {
        self.write_on(Local::now().date_naive(), line);
    }

    fn write_on(&self, today: NaiveDate, line: &str) {
        let mut open = self.open.lock();
        if open.as_ref().is_none_or(|o| o.day != today) {
            *open = None;
            if fs::create_dir_all(&self.dir).is_err() {
                return;
            }
            prune(&self.dir, today);
            let path = self.dir.join(file_name(today));
            let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) else { return };
            let written = file.metadata().map(|m| m.len()).unwrap_or(0);
            *open = Some(Open { day: today, file, written, full: false });
        }
        let Some(o) = open.as_mut() else { return };
        if o.full {
            return;
        }
        let bytes = line.len() as u64 + 1;
        if o.written + bytes > MAX_FILE_BYTES {
            o.full = true;
            let _ = writeln!(o.file, "[log file reached {} MB; nothing more is written today]", MAX_FILE_BYTES >> 20);
            return;
        }
        if writeln!(o.file, "{line}").is_ok() {
            o.written += bytes;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("butterlog-log-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn lines_go_to_the_day_file_and_old_files_are_pruned() {
        let dir = temp_dir("rotate");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("butterlog-2026-10-20.log"), "old\n").unwrap();
        fs::write(dir.join("butterlog-2026-10-27.log"), "kept\n").unwrap();
        fs::write(dir.join("notes.txt"), "not a log\n").unwrap();

        let log = LogFile::new(dir.clone());
        log.write_on(day("2026-11-01"), "first");
        log.write_on(day("2026-11-01"), "second");
        log.write_on(day("2026-11-02"), "next day");

        assert_eq!(fs::read_to_string(dir.join("butterlog-2026-11-01.log")).unwrap(), "first\nsecond\n");
        assert_eq!(fs::read_to_string(dir.join("butterlog-2026-11-02.log")).unwrap(), "next day\n");
        assert!(!dir.join("butterlog-2026-10-20.log").exists());
        assert!(!dir.join("butterlog-2026-10-26.log").exists());
        assert!(dir.join("butterlog-2026-10-27.log").exists());
        assert!(dir.join("notes.txt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_restart_appends() {
        let dir = temp_dir("append");
        LogFile::new(dir.clone()).write_on(day("2026-11-01"), "before");
        LogFile::new(dir.clone()).write_on(day("2026-11-01"), "after");
        assert_eq!(fs::read_to_string(dir.join("butterlog-2026-11-01.log")).unwrap(), "before\nafter\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_full_file_stops_growing() {
        let dir = temp_dir("full");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("butterlog-2026-11-01.log");
        fs::write(&path, vec![b'x'; (MAX_FILE_BYTES - 10) as usize]).unwrap();
        let log = LogFile::new(dir.clone());
        log.write_on(day("2026-11-01"), "this line does not fit");
        log.write_on(day("2026-11-01"), "nor this");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.ends_with("nothing more is written today]\n"));
        assert!(!text.contains("nor this"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dev_runs_log_apart() {
        assert_eq!(dir_name_from_args(&["app".into(), "--dev".into()]), "logs-dev");
        assert_eq!(dir_name_from_args(&["app".into()]), "logs");
    }
}
