//! Minimal persistence for the watcher: appends every `QueueSample` to a
//! newline-delimited JSON file, and keeps a small status file so later UI
//! code can show what the watcher is doing right now. Session lifecycle
//! (start/end detection, trend, ETA) is a later step's job, not this one's
//! — every sample here is tagged with one session id for the whole process
//! run, as a placeholder.

use crate::sampler::{Cadence, WatcherStatus};
use alt_ledger_queue::QueueSample;
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

pub const SAMPLES_FILE: &str = "queue-samples.jsonl";
pub const STATUS_FILE: &str = "watcher-status.json";

pub fn append_sample(dir: &Path, sample: &QueueSample) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut file = OpenOptions::new().create(true).append(true).open(dir.join(SAMPLES_FILE))?;
    writeln!(file, "{}", serde_json::to_string(sample)?)?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusFile<'a> {
    updated_at: i64,
    cadence: &'static str,
    error: Option<&'a str>,
}

pub fn write_status(dir: &Path, now: i64, cadence: Cadence, status: &WatcherStatus) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let error = match status {
        WatcherStatus::Ok => None,
        WatcherStatus::Error(message) => Some(message.as_str()),
    };
    let cadence = match cadence {
        Cadence::Active => "active",
        Cadence::Idle => "idle",
    };
    let body = StatusFile { updated_at: now, cadence, error };
    std::fs::write(dir.join(STATUS_FILE), serde_json::to_string_pretty(&body)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alt_ledger_queue::ParsedQueueText;

    #[test]
    fn appends_one_line_per_sample() {
        let dir = tempfile::tempdir().unwrap();
        let sample =
            QueueSample::new("s1", "sess1", 1000, ParsedQueueText { position: Some(5), displayed_time_text: None });
        append_sample(dir.path(), &sample).unwrap();
        append_sample(dir.path(), &sample).unwrap();
        let contents = std::fs::read_to_string(dir.path().join(SAMPLES_FILE)).unwrap();
        assert_eq!(contents.lines().count(), 2);
    }

    #[test]
    fn status_file_reports_error_message_and_cadence() {
        let dir = tempfile::tempdir().unwrap();
        write_status(dir.path(), 1000, Cadence::Idle, &WatcherStatus::Error("boom".into())).unwrap();
        let contents = std::fs::read_to_string(dir.path().join(STATUS_FILE)).unwrap();
        assert!(contents.contains("boom"));
        assert!(contents.contains("idle"));
    }

    #[test]
    fn status_file_has_no_error_field_when_ok() {
        let dir = tempfile::tempdir().unwrap();
        write_status(dir.path(), 1000, Cadence::Active, &WatcherStatus::Ok).unwrap();
        let contents = std::fs::read_to_string(dir.path().join(STATUS_FILE)).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed["error"], serde_json::Value::Null);
        assert_eq!(parsed["cadence"], "active");
    }
}
