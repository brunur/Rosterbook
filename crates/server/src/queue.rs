//! Reads what `alt-ledger-queue-watcher` wrote to disk and serves it to the
//! UI. No OCR, parsing, or session logic here — the watcher already wrote
//! camelCase JSON, so this just reads it back.

use serde_json::Value;
use std::path::Path;

const SAMPLES_FILE: &str = "queue-samples.jsonl";
const STATUS_FILE: &str = "watcher-status.json";

pub fn read_status(dir: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(dir.join(STATUS_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The most recent `limit` samples, oldest first. Every session the watcher
/// has ever run lands in the same file for now; grouping by session is a
/// later step the UI doesn't need yet.
pub fn read_recent_samples(dir: &Path, limit: usize) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(dir.join(SAMPLES_FILE)) else { return Vec::new() };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(limit);
    lines[start..].iter().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_files_are_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_status(dir.path()), None);
        assert_eq!(read_recent_samples(dir.path(), 10), Vec::<Value>::new());
    }

    #[test]
    fn reads_status_written_by_the_watcher() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(STATUS_FILE), r#"{"updatedAt":1000,"cadence":"active","error":null}"#).unwrap();
        let status = read_status(dir.path()).unwrap();
        assert_eq!(status["cadence"], "active");
    }

    #[test]
    fn caps_sample_count_and_keeps_the_most_recent() {
        let dir = tempfile::tempdir().unwrap();
        let lines: String = (0..5).map(|i| format!(r#"{{"id":"{i}"}}"#)).collect::<Vec<_>>().join("\n");
        std::fs::write(dir.path().join(SAMPLES_FILE), lines + "\n").unwrap();
        let samples = read_recent_samples(dir.path(), 2);
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0]["id"], "3");
        assert_eq!(samples[1]["id"], "4");
    }
}
