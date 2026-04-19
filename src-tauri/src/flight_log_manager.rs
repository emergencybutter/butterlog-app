use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use chrono::{Duration, NaiveDateTime};
use crate::config::ConfigManager;
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FlightSummary {
    pub filename: String,
    pub start_icao: String,
    pub end_icao: String,
    pub start_time: String,
    pub end_time: String,
    pub duration_minutes: i64,
    pub file_size_bytes: u64,
}

pub fn scan_logs(app: AppHandle) -> Result<Vec<FlightSummary>, String> {
    let config = app.state::<ConfigManager>().get_config();
    let log_dir = config.log_directory.unwrap_or_else(|| {
        app.path().app_data_dir().unwrap().join("logs")
    });

    if !log_dir.exists() {
        return Ok(Vec::new());
    }

    let mut summaries = Vec::new();
    let entries = fs::read_dir(log_dir).map_err(|e| e.to_string())?;

    for entry in entries {
        if let Ok(entry) = entry {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("csv") {
                if let Some(summary) = parse_log_file(&path) {
                    summaries.push(summary);
                }
            }
        }
    }

    // Sort by start time descending
    summaries.sort_by(|a, b| b.start_time.cmp(&a.start_time));

    Ok(summaries)
}

fn parse_log_file(path: &PathBuf) -> Option<FlightSummary> {
    let filename = path.file_name()?.to_str()?.to_string();
    let metadata = fs::metadata(path).ok()?;
    
    // Attempt to extract ICAOs from filename: butterlog_START_END_timestamp.csv
    // actually we changed it to butterlog_{start_icao}_{end_icao}_{timestamp}.csv
    let parts: Vec<&str> = filename.split('_').collect();
    let (start_icao, end_icao) = if parts.len() >= 4 && parts[0] == "butterlog" {
        (parts[1].to_string(), parts[2].to_string())
    } else {
        ("XXXX".to_string(), "XXXX".to_string())
    };

    // Read the file to get start/end times
    let content = fs::read_to_string(path).ok()?;
    let mut lines = content.lines().filter(|l| !l.starts_with('#') && !l.starts_with("Lcl Date"));
    
    let first_line = lines.next()?;
    let last_line = lines.last().or(Some(first_line))?;

    let start_dt = parse_line_datetime(first_line)?;
    let end_dt = parse_line_datetime(last_line)?;
    
    let duration = end_dt.signed_duration_since(start_dt);

    Some(FlightSummary {
        filename,
        start_icao,
        end_icao,
        start_time: start_dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        end_time: end_dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        duration_minutes: duration.num_minutes(),
        file_size_bytes: metadata.len(),
    })
}

fn parse_line_datetime(line: &str) -> Option<NaiveDateTime> {
    // Row format: 2026-04-19, 14:30:00, +02:00, ...
    let cols: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if cols.len() < 2 { return None; }
    
    let datetime_str = format!("{} {}", cols[0], cols[1]);
    NaiveDateTime::parse_from_str(&datetime_str, "%Y-%m-%d %H:%M:%S").ok()
}
