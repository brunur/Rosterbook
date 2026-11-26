//! Auction-house price data read from other add-ons' SavedVariables.
//!
//! Each supported add-on gets its own module that knows its SavedVariables
//! file name and how to pull a normalized [`price::PriceTable`] out of it.
//! Adding another add-on later means adding one more module here, not
//! touching the recommendation engine that consumes [`price::PriceTable`].

pub mod auctionator;
pub mod history;
mod lua_raw;
pub mod price;

pub use price::{PriceRecord, PriceTable};

/// One supported AH add-on, named for detection and settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Auctionator,
}

impl Source {
    pub const ALL: &'static [Source] = &[Source::Auctionator];

    pub fn id(&self) -> &'static str {
        match self {
            Source::Auctionator => "auctionator",
        }
    }

    pub fn data_file(&self) -> &'static str {
        match self {
            Source::Auctionator => auctionator::DATA_FILE,
        }
    }
}

/// Which realm's price table to use for a character, and the realm key that
/// table is actually stored under: an exact match, or — when there's only
/// one realm's worth of data — that one, since the add-on and an AH source
/// can disagree on a private server's realm name (e.g. Auctionator may key a
/// realm by its faction-specific AH name rather than the one the game API
/// reports). Returns `None` when neither applies (no prices for this
/// character at all). Callers that need a second lookup keyed by realm
/// (e.g. price history) should use the returned key, not `realm`, to stay
/// in sync with whichever realm actually supplied the price.
pub fn resolve_realm<'a>(
    prices_by_realm: &'a std::collections::HashMap<String, PriceTable>,
    realm: &str,
) -> Option<(&'a str, &'a PriceTable)> {
    if let Some((k, v)) = prices_by_realm.get_key_value(realm) {
        return Some((k.as_str(), v));
    }
    match prices_by_realm.len() {
        1 => prices_by_realm.iter().next().map(|(k, v)| (k.as_str(), v)),
        _ => None,
    }
}

/// Reads the given add-on's price tables, keyed by realm name (an account's
/// SavedVariables cover every realm it has played on).
pub fn load_all(saved_variables_dir: &std::path::Path, source: Source) -> std::collections::HashMap<String, PriceTable> {
    // Read as bytes, not a `String`: these files can hold binary data (an
    // unescaped CBOR blob) inside an otherwise-text Lua literal, so the file
    // as a whole isn't reliably valid UTF-8.
    let Ok(src) = std::fs::read(saved_variables_dir.join(source.data_file())) else {
        return std::collections::HashMap::new();
    };
    match source {
        Source::Auctionator => auctionator::parse_all(&src),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ah::price::PriceRecord;

    fn table(prices: &[(i64, i64)]) -> PriceTable {
        prices.iter().map(|&(id, copper)| (id, PriceRecord { min_buyout: copper, source: "test" })).collect()
    }

    #[test]
    fn resolve_realm_matches_exactly_when_present() {
        let by_realm = std::collections::HashMap::from([
            ("Area 52".to_string(), table(&[(1, 100)])),
            ("Stormrage".to_string(), table(&[(2, 200)])),
        ]);
        let (realm, prices) = resolve_realm(&by_realm, "Stormrage").unwrap();
        assert_eq!(realm, "Stormrage");
        assert_eq!(prices[&2].min_buyout, 200);
    }

    #[test]
    fn resolve_realm_falls_back_to_the_only_realm_present() {
        let by_realm = std::collections::HashMap::from([("Stormrage".to_string(), table(&[(999, 500)]))]);
        let (realm, prices) = resolve_realm(&by_realm, "Some Other Realm Name").unwrap();
        assert_eq!(realm, "Stormrage");
        assert_eq!(prices[&999].min_buyout, 500);
    }

    #[test]
    fn resolve_realm_is_none_without_an_exact_match_or_a_sole_realm() {
        let by_realm = std::collections::HashMap::from([
            ("Area 52".to_string(), table(&[(1, 100)])),
            ("Stormrage".to_string(), table(&[(2, 200)])),
        ]);
        assert!(resolve_realm(&by_realm, "Some Other Realm Name").is_none());
    }
}
