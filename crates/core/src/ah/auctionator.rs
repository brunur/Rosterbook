//! Reads price data out of Auctionator's `AUCTIONATOR_PRICE_DATABASE`.
//!
//! Each item's record is itself a Lua string holding a CBOR-serialized blob
//! (`{ l = {...}, h = {...}, a = {...}, m = <last min buyout> }`, see
//! Auctionator's `Source/Database/Mixin.lua`), so reading it needs a CBOR
//! decode on top of the Lua literal parse. Item keys are plain stringified
//! item IDs for ordinary items (`Source/API/v1/GetAuctionPrice.lua`); keys
//! for pets, keystones etc. aren't, and are skipped.

use super::lua_raw::{parse_saved_variables, Key, RawValue};
use super::price::{PriceRecord, PriceTable};
use std::collections::HashMap;

pub const DATA_FILE: &str = "Auctionator.lua";
const VAR_NAME: &str = "AUCTIONATOR_PRICE_DATABASE";
const SOURCE: &str = "auctionator";

/// Realm names present in the saved file (for picking one when the account
/// plays on several, or when the caller's realm name doesn't match exactly).
pub fn realms(src: &[u8]) -> Vec<String> {
    let Some(db) = find_db(src) else { return Vec::new() };
    db.as_table()
        .map(|t| {
            t.iter()
                .filter_map(|(k, _)| match k {
                    Key::Str(s) if s != "__dbversion" => Some(s.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Prices for one realm. Falls back to the only realm in the file if `realm`
/// isn't an exact match (connected-realm names drift, e.g. the root name
/// Blizzard reports can differ from the one the add-on last normalized to).
pub fn parse(src: &[u8], realm: &str) -> PriceTable {
    let out = PriceTable::new();
    let Some(db) = find_db(src) else { return out };
    let Some(table) = db.as_table() else { return out };

    let realm_value = table
        .iter()
        .find_map(|(k, v)| matches!(k, Key::Str(s) if s == realm).then_some(v))
        .or_else(|| {
            let mut realms = table.iter().filter(|(k, _)| !matches!(k, Key::Str(s) if s == "__dbversion"));
            let only = realms.next()?;
            realms.next().is_none().then_some(&only.1)
        });

    prices_from_realm_value(realm_value)
}

/// Prices for every realm in the file, keyed by realm name. Accounts that
/// play on several realms need this instead of `parse`, which only looks at
/// one.
pub fn parse_all(src: &[u8]) -> HashMap<String, PriceTable> {
    let Some(db) = find_db(src) else { return HashMap::new() };
    let Some(table) = db.as_table() else { return HashMap::new() };
    table
        .iter()
        .filter_map(|(k, v)| match k {
            Key::Str(s) if s != "__dbversion" => Some((s.clone(), v)),
            _ => None,
        })
        .map(|(realm, v)| (realm, prices_from_realm_value(Some(v))))
        .collect()
}

/// A realm's prices, in whichever of the two shapes Auctionator left it in:
/// still a plain Lua table (the realm the player is currently on, kept
/// readable for fast logins), or the whole realm re-packed into one CBOR
/// blob (every other realm — `Auctionator.Variables.InitializeDatabase`
/// serializes realms other than the current one in the background so they
/// don't bloat every future save). Either way each item inside is itself a
/// CBOR-serialized string (`_SetPrice` always stores items that way), so the
/// packed case needs a CBOR decode just to reach the per-item blobs the
/// unpacked case already has as native Lua strings.
fn prices_from_realm_value(value: Option<&RawValue>) -> PriceTable {
    match value {
        Some(RawValue::Table(t)) => {
            let mut out = PriceTable::new();
            for (key, value) in t {
                let Key::Str(item_id) = key else { continue };
                let Ok(item_id) = item_id.parse::<i64>() else { continue };
                let Some(blob) = value.as_bytes() else { continue };
                let Some(min_buyout) = min_buyout_from_item_blob(blob) else { continue };
                out.insert(item_id, PriceRecord { min_buyout, source: SOURCE });
            }
            out
        }
        Some(RawValue::Bytes(blob)) => prices_from_packed_realm(blob),
        _ => PriceTable::new(),
    }
}

/// LibCBOR encodes the whole realm table as one native CBOR map — item keys
/// (as a CBOR byte string of ASCII digits, not text; LibCBOR doesn't know a
/// Lua string is "really" text) mapping straight to each item's `{l,h,a,m}`
/// map, not to another serialized blob. One CBOR decode reaches everything.
fn prices_from_packed_realm(blob: &[u8]) -> PriceTable {
    let mut out = PriceTable::new();
    let Ok(decoded) = ciborium::de::from_reader::<ciborium::value::Value, _>(blob) else { return out };
    let Some(map) = decoded.as_map() else { return out };
    for (key, value) in map {
        let Some(item_id) = cbor_item_id(key) else { continue };
        let Some(min_buyout) = min_buyout_from_cbor_value(value) else { continue };
        out.insert(item_id, PriceRecord { min_buyout, source: SOURCE });
    }
    out
}

fn cbor_item_id(key: &ciborium::value::Value) -> Option<i64> {
    match key {
        ciborium::value::Value::Text(s) => s.parse().ok(),
        ciborium::value::Value::Bytes(b) => std::str::from_utf8(b).ok()?.parse().ok(),
        ciborium::value::Value::Integer(i) => i64::try_from(*i).ok(),
        _ => None,
    }
}

fn find_db(src: &[u8]) -> Option<RawValue> {
    let vars = parse_saved_variables(src).ok()?;
    vars.into_iter().find(|(name, _)| name == VAR_NAME).map(|(_, v)| v)
}

/// A CBOR-serialized `{ l = {...}, h = {...}, a = {...}, m = <price> }` blob
/// (the shape of one item's record when it's still a Lua string, as in the
/// per-realm-table case — `Auctionator.DatabaseMixin:_SetPrice`).
fn min_buyout_from_item_blob(blob: &[u8]) -> Option<i64> {
    let decoded: ciborium::value::Value = ciborium::de::from_reader(blob).ok()?;
    min_buyout_from_cbor_value(&decoded)
}

fn min_buyout_from_cbor_value(value: &ciborium::value::Value) -> Option<i64> {
    let map = value.as_map()?;
    // LibCBOR has no notion of Lua's string/text distinction, so map keys
    // that started life as Lua strings (every key here) can come back as
    // either CBOR type depending on their bytes — matching both is needed,
    // not just `Value::Text`.
    let (_, m) = map.iter().find(|(k, _)| cbor_key_is(k, "m"))?;
    m.as_integer().and_then(|i| i64::try_from(i).ok())
}

fn cbor_key_is(key: &ciborium::value::Value, s: &str) -> bool {
    match key {
        ciborium::value::Value::Text(t) => t == s,
        ciborium::value::Value::Bytes(b) => b.as_slice() == s.as_bytes(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cbor_record_value(m: i64) -> ciborium::value::Value {
        ciborium::value::Value::Map(vec![
            (ciborium::value::Value::Text("l".into()), ciborium::value::Value::Map(vec![])),
            (ciborium::value::Value::Text("h".into()), ciborium::value::Value::Map(vec![])),
            (ciborium::value::Value::Text("m".into()), ciborium::value::Value::Integer(m.into())),
        ])
    }

    fn cbor_record(m: i64) -> Vec<u8> {
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&cbor_record_value(m), &mut buf).unwrap();
        buf
    }

    /// Same record, but with CBOR byte-string keys instead of text-string
    /// keys — how LibCBOR actually encodes a real scan (`m` comes back as
    /// `Value::Bytes(b"m")`, not `Value::Text("m")`).
    fn cbor_record_value_real_shape(m: i64) -> ciborium::value::Value {
        ciborium::value::Value::Map(vec![
            (ciborium::value::Value::Bytes(b"l".to_vec()), ciborium::value::Value::Array(vec![])),
            (ciborium::value::Value::Bytes(b"h".to_vec()), ciborium::value::Value::Map(vec![])),
            (ciborium::value::Value::Bytes(b"m".to_vec()), ciborium::value::Value::Integer(m.into())),
        ])
    }

    #[test]
    fn reads_cbor_packed_prices() {
        let blob = cbor_record(1234);
        let escaped: String = blob.iter().map(|b| format!("\\{b}")).collect();
        let src = format!(r#"AUCTIONATOR_PRICE_DATABASE = {{ __dbversion = 8, ["Area 52"] = {{ ["12345"] = "{escaped}" }} }}"#);

        let prices = parse(src.as_bytes(), "Area 52");
        assert_eq!(prices[&12345].min_buyout, 1234);
        assert_eq!(prices[&12345].source, "auctionator");
    }

    #[test]
    fn falls_back_to_the_only_realm_present() {
        let blob = cbor_record(500);
        let escaped: String = blob.iter().map(|b| format!("\\{b}")).collect();
        let src = format!(r#"AUCTIONATOR_PRICE_DATABASE = {{ __dbversion = 8, ["Stormrage"] = {{ ["999"] = "{escaped}" }} }}"#);

        let prices = parse(src.as_bytes(), "Some Other Realm Name");
        assert_eq!(prices[&999].min_buyout, 500);
    }

    #[test]
    fn skips_non_numeric_item_keys() {
        let src = r#"AUCTIONATOR_PRICE_DATABASE = { ["Area 52"] = { ["p:1:2"] = "whatever" } }"#;
        assert!(parse(src.as_bytes(), "Area 52").is_empty());
    }

    #[test]
    fn parses_every_realm_at_once() {
        let a = cbor_record(111);
        let b = cbor_record(222);
        let (ea, eb): (String, String) = (a.iter().map(|b| format!("\\{b}")).collect(), b.iter().map(|b| format!("\\{b}")).collect());
        let src = format!(
            r#"AUCTIONATOR_PRICE_DATABASE = {{ __dbversion = 8, ["Area 52"] = {{ ["1"] = "{ea}" }}, ["Stormrage"] = {{ ["2"] = "{eb}" }} }}"#
        );

        let all = parse_all(src.as_bytes());
        assert_eq!(all["Area 52"][&1].min_buyout, 111);
        assert_eq!(all["Stormrage"][&2].min_buyout, 222);
    }

    #[test]
    fn lists_realms() {
        let src = r#"AUCTIONATOR_PRICE_DATABASE = { __dbversion = 8, ["Area 52"] = {}, ["Stormrage"] = {} }"#;
        let mut r = realms(src.as_bytes());
        r.sort();
        assert_eq!(r, vec!["Area 52".to_string(), "Stormrage".to_string()]);
    }

    #[test]
    fn raw_non_utf8_bytes_in_a_string_literal_dont_break_parsing() {
        // Real Auctionator files write the CBOR blob's bytes straight into the
        // quoted string, unescaped, rather than as \ddd escapes — so the file
        // as a whole usually isn't valid UTF-8 even though this crate used to
        // assume it was.
        let blob = cbor_record(42);
        let mut src = b"AUCTIONATOR_PRICE_DATABASE = { [\"Area 52\"] = { [\"7\"] = \"".to_vec();
        src.extend_from_slice(&blob);
        src.extend_from_slice(b"\" } }");
        assert!(std::str::from_utf8(&src).is_err(), "the fixture should actually exercise the non-UTF8 path");

        let prices = parse(&src, "Area 52");
        assert_eq!(prices[&7].min_buyout, 42);
    }

    /// Auctionator re-packs every realm but the current one into a single
    /// CBOR blob at login (`Auctionator.Variables.InitializeDatabase`), so an
    /// account that was last played on a different realm has its price data
    /// in this shape, not the per-item-table shape the other tests cover.
    /// Confirmed against a real scanned `Auctionator.lua`: the item key AND
    /// every key inside its `{l,h,a,m}` record are CBOR *byte* strings, not
    /// text strings (LibCBOR doesn't know a Lua string is "really" text) —
    /// and the value is that map directly, no second layer of serialization.
    #[test]
    fn reads_a_whole_realm_thats_been_packed_into_one_cbor_blob() {
        let realm_map = ciborium::value::Value::Map(vec![(
            ciborium::value::Value::Bytes(b"54321".to_vec()),
            cbor_record_value_real_shape(777),
        )]);
        let mut realm_blob = Vec::new();
        ciborium::ser::into_writer(&realm_map, &mut realm_blob).unwrap();
        let escaped: String = realm_blob.iter().map(|b| format!("\\{b}")).collect();
        let src = format!(r#"AUCTIONATOR_PRICE_DATABASE = {{ __dbversion = 8, ["Area 52"] = "{escaped}" }}"#);

        let prices = parse(src.as_bytes(), "Area 52");
        assert_eq!(prices[&54321].min_buyout, 777);
    }
}
