//! Normalized auction-house price data, regardless of which addon it came from.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceRecord {
    /// Last seen minimum buyout, in copper.
    pub min_buyout: i64,
    pub source: &'static str,
}

/// Item ID -> last known price.
pub type PriceTable = HashMap<i64, PriceRecord>;
