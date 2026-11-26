//! A record of AH prices over time, so a current price can be judged against
//! what it's actually been recently rather than shown in isolation.
//!
//! Storage mirrors `alt-ledger-queue-watcher`'s `store.rs`: one newline-
//! delimited JSON file per game, appended to on every capture and pruned of
//! anything older than `RETENTION_DAYS`.

use super::PriceTable;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

const FILE: &str = "ah-history.jsonl";
const RETENTION_DAYS: i64 = 90;
/// Below this many samples, an average is just noise, not a trend.
const MIN_SAMPLES: usize = 3;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PriceSample {
    ts: i64,
    item_id: i64,
    min_buyout: i64,
}

/// `(realm, item id)` -> every known `(ts, min_buyout)` sample, newest last.
pub type History = HashMap<(String, i64), Vec<(i64, i64)>>;

fn file_path(history_dir: &Path, game_id: &str) -> std::path::PathBuf {
    history_dir.join(game_id).join(FILE)
}

/// Appends one row per (realm, item) in `prices_by_realm`, then drops rows
/// older than `RETENTION_DAYS`. A realm's file is a flat JSONL list with the
/// realm name as a JSON object key per line (not per-row, to avoid repeating
/// it on every sample) — see `Row` below.
pub fn capture(history_dir: &Path, game_id: &str, prices_by_realm: &HashMap<String, PriceTable>, now: i64) -> anyhow::Result<()> {
    let dir = history_dir.join(game_id);
    std::fs::create_dir_all(&dir)?;
    let mut file = OpenOptions::new().create(true).append(true).open(dir.join(FILE))?;
    for (realm, table) in prices_by_realm {
        for (item_id, record) in table {
            let row = Row { realm: realm.clone(), sample: PriceSample { ts: now, item_id: *item_id, min_buyout: record.min_buyout } };
            writeln!(file, "{}", serde_json::to_string(&row)?)?;
        }
    }
    drop(file);
    prune(history_dir, game_id, now);
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Row {
    realm: String,
    #[serde(flatten)]
    sample: PriceSample,
}

/// Drops rows older than `RETENTION_DAYS`, rewriting the file in place. A
/// line that fails to parse is dropped too, same as `queue-watcher`'s
/// `prune_samples` — one stray row isn't worth jamming every later prune.
fn prune(history_dir: &Path, game_id: &str, now: i64) {
    let path = file_path(history_dir, game_id);
    let Ok(contents) = std::fs::read_to_string(&path) else { return };
    let cutoff = now - RETENTION_DAYS * 86_400;
    let kept: Vec<&str> = contents.lines().filter(|line| serde_json::from_str::<Row>(line).is_ok_and(|r| r.sample.ts >= cutoff)).collect();
    if kept.len() == contents.lines().count() {
        return; // nothing to drop, don't touch the file
    }
    let mut body = kept.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    let _ = std::fs::write(&path, body);
}

/// Reads a game's whole history once, so computing trends for many items
/// doesn't mean re-reading the file per item (same shape as
/// `disenchant::load_disenchant_data`). A missing file is an empty history,
/// not an error — the add-on may not be installed, or no snapshot taken yet.
pub fn load(history_dir: &Path, game_id: &str) -> History {
    let Ok(contents) = std::fs::read_to_string(file_path(history_dir, game_id)) else { return History::new() };
    let mut out = History::new();
    for row in contents.lines().filter_map(|line| serde_json::from_str::<Row>(line).ok()) {
        out.entry((row.realm, row.sample.item_id)).or_default().push((row.sample.ts, row.sample.min_buyout));
    }
    out
}

/// Average `min_buyout` for `(realm, item_id)` since `since`, or `None` if
/// there aren't enough samples in that window to mean anything.
pub fn rolling_average(history: &History, realm: &str, item_id: i64, since: i64) -> Option<i64> {
    let samples = history.get(&(realm.to_string(), item_id))?;
    let recent: Vec<i64> = samples.iter().filter(|(ts, _)| *ts >= since).map(|(_, price)| *price).collect();
    if recent.len() < MIN_SAMPLES {
        return None;
    }
    Some(recent.iter().sum::<i64>() / recent.len() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ah::price::PriceRecord;

    fn prices(entries: &[(i64, i64)]) -> HashMap<String, PriceTable> {
        let mut table = PriceTable::new();
        for &(item_id, min_buyout) in entries {
            table.insert(item_id, PriceRecord { min_buyout, source: "auctionator" });
        }
        HashMap::from([("Area 52".to_string(), table)])
    }

    #[test]
    fn appends_one_row_per_item_per_capture() {
        let dir = tempfile::tempdir().unwrap();
        capture(dir.path(), "g1", &prices(&[(100, 50), (200, 75)]), 1000).unwrap();
        let history = load(dir.path(), "g1");
        assert_eq!(history.get(&("Area 52".to_string(), 100)), Some(&vec![(1000, 50)]));
        assert_eq!(history.get(&("Area 52".to_string(), 200)), Some(&vec![(1000, 75)]));
    }

    #[test]
    fn repeated_captures_accumulate_samples() {
        let dir = tempfile::tempdir().unwrap();
        capture(dir.path(), "g1", &prices(&[(100, 50)]), 1000).unwrap();
        capture(dir.path(), "g1", &prices(&[(100, 60)]), 2000).unwrap();
        let history = load(dir.path(), "g1");
        assert_eq!(history.get(&("Area 52".to_string(), 100)), Some(&vec![(1000, 50), (2000, 60)]));
    }

    #[test]
    fn missing_file_loads_as_empty_history() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path(), "nope").is_empty());
    }

    #[test]
    fn prune_drops_samples_older_than_retention() {
        let dir = tempfile::tempdir().unwrap();
        const DAY: i64 = 86_400;
        capture(dir.path(), "g1", &prices(&[(100, 10)]), 1000).unwrap();
        capture(dir.path(), "g1", &prices(&[(100, 20)]), 1000 + 100 * DAY).unwrap();
        let history = load(dir.path(), "g1");
        assert_eq!(history.get(&("Area 52".to_string(), 100)), Some(&vec![(1000 + 100 * DAY, 20)]));
    }

    #[test]
    fn rolling_average_ignores_samples_outside_the_window() {
        let dir = tempfile::tempdir().unwrap();
        capture(dir.path(), "g1", &prices(&[(100, 100)]), 0).unwrap();
        capture(dir.path(), "g1", &prices(&[(100, 200)]), 10).unwrap();
        capture(dir.path(), "g1", &prices(&[(100, 300)]), 20).unwrap();
        let history = load(dir.path(), "g1");
        // All three in-window: average of 100/200/300.
        assert_eq!(rolling_average(&history, "Area 52", 100, 0), Some(200));
        // Only the last two in-window: average of 200/300 -- but that's only
        // 2 samples, below the floor.
        assert_eq!(rolling_average(&history, "Area 52", 100, 15), None);
    }

    #[test]
    fn rolling_average_withholds_below_the_sample_floor() {
        let dir = tempfile::tempdir().unwrap();
        capture(dir.path(), "g1", &prices(&[(100, 50)]), 1000).unwrap();
        capture(dir.path(), "g1", &prices(&[(100, 60)]), 2000).unwrap();
        let history = load(dir.path(), "g1");
        assert_eq!(rolling_average(&history, "Area 52", 100, 0), None, "only 2 samples, below the floor of 3");
    }

    #[test]
    fn rolling_average_is_none_for_an_unknown_item() {
        let history = History::new();
        assert_eq!(rolling_average(&history, "Area 52", 999, 0), None);
    }
}
