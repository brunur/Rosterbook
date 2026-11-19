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
pub fn realms(src: &str) -> Vec<String> {
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
pub fn parse(src: &str, realm: &str) -> PriceTable {
    let out = PriceTable::new();
    let Some(db) = find_db(src) else { return out };
    let Some(table) = db.as_table() else { return out };

    let realm_table = table
        .iter()
        .find_map(|(k, v)| matches!(k, Key::Str(s) if s == realm).then_some(v))
        .or_else(|| {
            let mut realms = table.iter().filter(|(k, _)| !matches!(k, Key::Str(s) if s == "__dbversion"));
            let only = realms.next()?;
            realms.next().is_none().then_some(&only.1)
        });

    let Some(realm_table) = realm_table.and_then(RawValue::as_table) else { return out };
    prices_from_realm_table(realm_table)
}

/// Prices for every realm in the file, keyed by realm name. Accounts that
/// play on several realms need this instead of `parse`, which only looks at
/// one.
pub fn parse_all(src: &str) -> HashMap<String, PriceTable> {
    let Some(db) = find_db(src) else { return HashMap::new() };
    let Some(table) = db.as_table() else { return HashMap::new() };
    table
        .iter()
        .filter_map(|(k, v)| match k {
            Key::Str(s) if s != "__dbversion" => Some((s.clone(), v)),
            _ => None,
        })
        .filter_map(|(realm, v)| Some((realm, prices_from_realm_table(v.as_table()?))))
        .collect()
}

fn prices_from_realm_table(realm_table: &[(Key, RawValue)]) -> PriceTable {
    let mut out = PriceTable::new();
    for (key, value) in realm_table {
        let Key::Str(item_id) = key else { continue };
        let Ok(item_id) = item_id.parse::<i64>() else { continue };
        let Some(min_buyout) = read_min_buyout(value) else { continue };
        out.insert(item_id, PriceRecord { min_buyout, source: SOURCE });
    }
    out
}

fn find_db(src: &str) -> Option<RawValue> {
    let vars = parse_saved_variables(src).ok()?;
    vars.into_iter().find(|(name, _)| name == VAR_NAME).map(|(_, v)| v)
}

/// A price record is either still a plain table (freshly written, not yet
/// re-packed into CBOR) or a CBOR-serialized string; either way it has an
/// `m` field holding the last seen minimum buyout.
fn read_min_buyout(value: &RawValue) -> Option<i64> {
    match value {
        RawValue::Table(_) => value.get("m")?.as_int(),
        RawValue::Bytes(blob) => {
            let decoded: ciborium::value::Value = ciborium::de::from_reader(&blob[..]).ok()?;
            let map = decoded.as_map()?;
            let (_, m) = map.iter().find(|(k, _)| k.as_text() == Some("m"))?;
            m.as_integer().and_then(|i| i64::try_from(i).ok())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cbor_record(m: i64) -> Vec<u8> {
        let value = ciborium::value::Value::Map(vec![
            (ciborium::value::Value::Text("l".into()), ciborium::value::Value::Map(vec![])),
            (ciborium::value::Value::Text("h".into()), ciborium::value::Value::Map(vec![])),
            (ciborium::value::Value::Text("m".into()), ciborium::value::Value::Integer(m.into())),
        ]);
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&value, &mut buf).unwrap();
        buf
    }

    #[test]
    fn reads_cbor_packed_prices() {
        let blob = cbor_record(1234);
        let escaped: String = blob.iter().map(|b| format!("\\{b}")).collect();
        let src = format!(r#"AUCTIONATOR_PRICE_DATABASE = {{ __dbversion = 8, ["Area 52"] = {{ ["12345"] = "{escaped}" }} }}"#);

        let prices = parse(&src, "Area 52");
        assert_eq!(prices[&12345].min_buyout, 1234);
        assert_eq!(prices[&12345].source, "auctionator");
    }

    #[test]
    fn falls_back_to_the_only_realm_present() {
        let blob = cbor_record(500);
        let escaped: String = blob.iter().map(|b| format!("\\{b}")).collect();
        let src = format!(r#"AUCTIONATOR_PRICE_DATABASE = {{ __dbversion = 8, ["Stormrage"] = {{ ["999"] = "{escaped}" }} }}"#);

        let prices = parse(&src, "Some Other Realm Name");
        assert_eq!(prices[&999].min_buyout, 500);
    }

    #[test]
    fn skips_non_numeric_item_keys() {
        let src = r#"AUCTIONATOR_PRICE_DATABASE = { ["Area 52"] = { ["p:1:2"] = "whatever" } }"#;
        assert!(parse(src, "Area 52").is_empty());
    }

    #[test]
    fn parses_every_realm_at_once() {
        let a = cbor_record(111);
        let b = cbor_record(222);
        let (ea, eb): (String, String) = (a.iter().map(|b| format!("\\{b}")).collect(), b.iter().map(|b| format!("\\{b}")).collect());
        let src = format!(
            r#"AUCTIONATOR_PRICE_DATABASE = {{ __dbversion = 8, ["Area 52"] = {{ ["1"] = "{ea}" }}, ["Stormrage"] = {{ ["2"] = "{eb}" }} }}"#
        );

        let all = parse_all(&src);
        assert_eq!(all["Area 52"][&1].min_buyout, 111);
        assert_eq!(all["Stormrage"][&2].min_buyout, 222);
    }

    #[test]
    fn lists_realms() {
        let src = r#"AUCTIONATOR_PRICE_DATABASE = { __dbversion = 8, ["Area 52"] = {}, ["Stormrage"] = {} }"#;
        let mut r = realms(src);
        r.sort();
        assert_eq!(r, vec!["Area 52".to_string(), "Stormrage".to_string()]);
    }
}
