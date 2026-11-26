//! Disenchant yields learned in-game by the `RosterbookDisenchant` add-on
//! (`RosterbookDisenchant.lua` → `RosterbookDisenchantDB`), mirrored here so
//! the desktop app can show the same profit numbers the in-game tooltip
//! does, for the same reason: nothing here guesses a yield from a hardcoded
//! table, since WoW's own disenchant results are a hidden server-side loot
//! table we have no verified source for on any given game version.
//!
//! Three levels of data, most to least specific — matches the add-on's own
//! `onTooltipSetItemImpl` precedence exactly, so desktop and in-game never
//! disagree about the same item:
//!   1. `yields[itemId]`      — this exact item, personally disenchanted.
//!   2. `buckets[quality:ilvl-range]` — other items of the same quality and
//!      item-level bracket (WoW's yield tiers are driven by these, not by
//!      the specific item).
//!   3. `qualityBuckets[quality]` — any item of the same quality, any level.
//!      Rougher (item level shifts which dust/essence/shard tier within a
//!      quality), but still better than nothing.

use crate::lua::parse_saved_variables;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

pub const DATA_FILE: &str = "RosterbookDisenchant.lua";
const VAR_NAME: &str = "RosterbookDisenchantDB";

/// A learned (or rolled-up) disenchant result: how many times it's been
/// seen, and the average count of each mat per disenchant.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisenchantYield {
    pub n: i64,
    /// (mat item ID, average count per disenchant).
    pub mats: Vec<(i64, f64)>,
}

#[derive(Debug, Clone, Default)]
pub struct DisenchantData {
    pub yields: HashMap<i64, DisenchantYield>,
    pub buckets: HashMap<String, DisenchantYield>,
    pub quality_buckets: HashMap<i64, DisenchantYield>,
}

/// How a prediction was sourced, worst to best fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Exact,
    Bucket,
    Quality,
}

impl Confidence {
    pub fn label(self) -> &'static str {
        match self {
            Confidence::Exact => "exact",
            Confidence::Bucket => "bucket",
            Confidence::Quality => "quality",
        }
    }
}

/// Reads `RosterbookDisenchant.lua` from a SavedVariables folder. Missing
/// file (add-on never installed, or nothing learned yet) → empty data, not
/// an error.
pub fn load_disenchant_data(saved_variables_dir: &Path) -> DisenchantData {
    let file = saved_variables_dir.join(DATA_FILE);
    let Ok(src) = std::fs::read_to_string(&file) else { return DisenchantData::default() };
    let Ok(vars) = parse_saved_variables(&src) else { return DisenchantData::default() };
    let Some(db) = vars.get(VAR_NAME) else { return DisenchantData::default() };
    build(db)
}

/// Item-level bucket boundaries, matching `RosterbookDisenchant.lua`'s own
/// `levelBucket()` exactly.
fn level_bucket(ilvl: f64) -> &'static str {
    if ilvl <= 15.0 {
        "1-15"
    } else if ilvl <= 25.0 {
        "16-25"
    } else if ilvl <= 35.0 {
        "26-35"
    } else if ilvl <= 45.0 {
        "36-45"
    } else if ilvl <= 55.0 {
        "46-55"
    } else {
        "56+"
    }
}

pub fn bucket_key(quality: i64, ilvl: f64) -> String {
    format!("{quality}:{}", level_bucket(ilvl))
}

/// Exact match first, then the quality+level bucket (only possible when the
/// caller actually knows this item's level — e.g. not for a bag item, since
/// `model::Item` has no per-item level, only a character-wide average),
/// then quality alone.
pub fn predict(item_id: i64, quality: i64, ilvl: Option<f64>, data: &DisenchantData) -> Option<(&DisenchantYield, Confidence)> {
    if let Some(y) = data.yields.get(&item_id).filter(|y| y.n > 0) {
        return Some((y, Confidence::Exact));
    }
    if let Some(ilvl) = ilvl {
        if let Some(y) = data.buckets.get(&bucket_key(quality, ilvl)).filter(|y| y.n > 0) {
            return Some((y, Confidence::Bucket));
        }
    }
    if let Some(y) = data.quality_buckets.get(&quality).filter(|y| y.n > 0) {
        return Some((y, Confidence::Quality));
    }
    None
}

// ---------------------------------------------------------------------------
// Building from the parsed Lua table
// ---------------------------------------------------------------------------

/// A Lua table's entries with their original (integer) keys — our own
/// SavedVariables parser collapses a table into a JSON array when its keys
/// are exactly `1..n`, which would otherwise lose which item/quality ID an
/// entry belongs to.
fn keyed_int(v: &Value) -> Vec<(i64, &Value)> {
    match v {
        Value::Object(o) => o.iter().filter_map(|(k, v)| k.parse::<i64>().ok().map(|k| (k, v))).collect(),
        Value::Array(a) => a.iter().enumerate().filter(|(_, x)| !x.is_null()).map(|(i, v)| (i as i64 + 1, v)).collect(),
        _ => Vec::new(),
    }
}

fn int(v: &Value, key: &str) -> Option<i64> {
    match v.get(key)? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64)),
        _ => None,
    }
}

fn disenchant_yield(v: &Value) -> DisenchantYield {
    let n = int(v, "n").unwrap_or(0);
    let mut mats: Vec<(i64, f64)> = Vec::new();
    if n > 0 {
        for (mat_id, m) in keyed_int(v.get("mats").unwrap_or(&Value::Null)) {
            let count = int(m, "count").unwrap_or(0);
            if count > 0 {
                mats.push((mat_id, count as f64 / n as f64));
            }
        }
    }
    DisenchantYield { n, mats }
}

fn build(db: &Value) -> DisenchantData {
    let yields = keyed_int(db.get("yields").unwrap_or(&Value::Null))
        .into_iter()
        .map(|(id, v)| (id, disenchant_yield(v)))
        .collect();

    let buckets = db
        .get("buckets")
        .and_then(Value::as_object)
        .map(|o| o.iter().map(|(k, v)| (k.clone(), disenchant_yield(v))).collect())
        .unwrap_or_default();

    let quality_buckets = keyed_int(db.get("qualityBuckets").unwrap_or(&Value::Null))
        .into_iter()
        .map(|(q, v)| (q, disenchant_yield(v)))
        .collect();

    DisenchantData { yields, buckets, quality_buckets }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let data = load_disenchant_data(dir.path());
        assert!(data.yields.is_empty() && data.buckets.is_empty() && data.quality_buckets.is_empty());
    }

    #[test]
    fn parses_yields_buckets_and_quality_buckets() {
        let src = r#"
RosterbookDisenchantDB = {
  yields = {
    [2851] = { n = 2, mats = { [10938] = { count = 3, name = "Lesser Magic Essence" } } },
  },
  buckets = {
    ["2:1-15"] = { n = 2, mats = { [10938] = { count = 3, name = "Lesser Magic Essence" } } },
  },
  qualityBuckets = {
    [2] = { n = 2, mats = { [10938] = { count = 3, name = "Lesser Magic Essence" } } },
  },
}
"#;
        let vars = parse_saved_variables(src).unwrap();
        let data = build(&vars[VAR_NAME]);

        let y = data.yields.get(&2851).expect("exact yield");
        assert_eq!(y.n, 2);
        assert_eq!(y.mats, vec![(10938, 1.5)]);

        assert_eq!(data.buckets.get("2:1-15").expect("bucket").n, 2);
        assert_eq!(data.quality_buckets.get(&2).expect("quality bucket").n, 2);
    }

    #[test]
    fn predict_prefers_exact_then_bucket_then_quality() {
        let mut data = DisenchantData::default();
        data.yields.insert(2851, DisenchantYield { n: 2, mats: vec![(10938, 1.5)] });
        data.buckets.insert("2:1-15".into(), DisenchantYield { n: 5, mats: vec![(10938, 2.0)] });
        data.quality_buckets.insert(2, DisenchantYield { n: 9, mats: vec![(10938, 3.0)] });

        // Exact item: wins over its own bucket/quality data.
        let (y, c) = predict(2851, 2, Some(6.0), &data).unwrap();
        assert_eq!(c, Confidence::Exact);
        assert_eq!(y.n, 2);

        // Different item, same quality+level bucket: falls back to the bucket.
        let (y, c) = predict(9999, 2, Some(6.0), &data).unwrap();
        assert_eq!(c, Confidence::Bucket);
        assert_eq!(y.n, 5);

        // Different item, same quality but a level outside that bucket: falls
        // back further, to the quality-wide rollup.
        let (y, c) = predict(9999, 2, Some(40.0), &data).unwrap();
        assert_eq!(c, Confidence::Quality);
        assert_eq!(y.n, 9);

        // No item level known at all (e.g. a bag item): skips the bucket
        // step entirely and goes straight to the quality-wide rollup.
        let (y, c) = predict(9999, 2, None, &data).unwrap();
        assert_eq!(c, Confidence::Quality);
        assert_eq!(y.n, 9);

        // Different quality entirely with no data anywhere: nothing shown.
        assert!(predict(9999, 4, Some(6.0), &data).is_none());
    }
}
