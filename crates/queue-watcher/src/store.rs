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

/// Drops sample rows older than `retention_days`, rewriting the file in
/// place. A line that fails to parse is dropped too, rather than left to
/// jam every later prune — it's one stray sample, not worth keeping over
/// a growing, never-cleaned file.
pub fn prune_samples(dir: &Path, now: i64, retention_days: u32) -> anyhow::Result<()> {
    let path = dir.join(SAMPLES_FILE);
    let Ok(contents) = std::fs::read_to_string(&path) else { return Ok(()) };
    let cutoff = now - retention_days as i64 * 86_400;
    let kept: Vec<&str> = contents
        .lines()
        .filter(|line| serde_json::from_str::<QueueSample>(line).is_ok_and(|s| s.ts >= cutoff))
        .collect();
    if kept.len() == contents.lines().count() {
        return Ok(()); // nothing to drop, don't touch the file
    }
    let mut body = kept.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    std::fs::write(&path, body)?;
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
    fn prune_drops_samples_older_than_retention() {
        let dir = tempfile::tempdir().unwrap();
        const DAY: i64 = 86_400;
        let old = QueueSample::new("old", "sess1", 1000, ParsedQueueText { position: Some(1), displayed_time_text: None });
        let recent = QueueSample::new(
            "recent",
            "sess1",
            1000 + 10 * DAY,
            ParsedQueueText { position: Some(2), displayed_time_text: None },
        );
        append_sample(dir.path(), &old).unwrap();
        append_sample(dir.path(), &recent).unwrap();

        prune_samples(dir.path(), 1000 + 10 * DAY, 5).unwrap();

        let contents = std::fs::read_to_string(dir.path().join(SAMPLES_FILE)).unwrap();
        let kept: Vec<QueueSample> = contents.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "recent");
    }

    #[test]
    fn prune_leaves_file_alone_when_nothing_is_old() {
        let dir = tempfile::tempdir().unwrap();
        let sample = QueueSample::new("s1", "sess1", 1000, ParsedQueueText { position: Some(1), displayed_time_text: None });
        append_sample(dir.path(), &sample).unwrap();

        prune_samples(dir.path(), 1000, 30).unwrap();

        let contents = std::fs::read_to_string(dir.path().join(SAMPLES_FILE)).unwrap();
        assert_eq!(contents.lines().count(), 1);
    }

    #[test]
    fn prune_on_missing_file_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        prune_samples(dir.path(), 1000, 30).unwrap();
        assert!(!dir.path().join(SAMPLES_FILE).exists());
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
