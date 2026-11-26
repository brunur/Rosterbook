//! What to do with the junk already sitting in your bags and bank: sell to
//! a vendor, sell on the AH, or disenchant and sell the mats — whichever is
//! worth the most, shown only once we actually know a number for it.
//!
//! Scope note: this ranks items the player already owns, not the whole
//! auction house. Scanning the AH for *unowned* gear worth buying (to flip
//! or to disenchant) needs item quality/level/class data for items nobody's
//! looked at — this app has no such lookup (model data only covers items
//! the add-on has already seen client-side), so that's left for later
//! rather than guessed at here.

use crate::ah;
use crate::ah::history::{self, History};
use crate::ah::PriceTable;
use crate::disenchant::{self, DisenchantData};
use crate::features;
use crate::model::{Character, GameData, Item};
use serde::Serialize;
use std::collections::HashMap;

/// Enum.ItemClass numeric IDs, same fallback values `RosterbookDisenchant.lua` uses.
const CLASS_WEAPON: i64 = 2;
const CLASS_ARMOR: i64 = 4;

/// How far back a price trend looks.
const TREND_WINDOW_DAYS: i64 = 14;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisenchantOption {
    /// Expected mat value minus nothing (there's no "buy price" for an item
    /// you already own) — just the expected sale value of the mats.
    pub value: i64,
    pub confidence: &'static str,
    pub seen: i64,
}

/// How the current AH price compares to its own recent history.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceTrend {
    /// Negative means cheaper than its recent average (e.g. -22.0 = 22%
    /// below), positive means pricier.
    pub pct_vs_average: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDisposal {
    pub character: String,
    pub realm: String,
    pub item_id: i64,
    pub item_name: String,
    pub icon: Option<u32>,
    pub quality: i64,
    pub count: i64,
    /// For the UI's icon-glyph fallback when no game picture is cached —
    /// same fields `model::Item` carries for bags/gear.
    pub class_id: Option<i64>,
    pub subclass_id: Option<i64>,
    pub equip_loc: Option<String>,
    /// What a vendor pays, in copper. Always `None` today: the add-on only
    /// exports a vendor price for recipe outputs, not arbitrary bag items —
    /// showing a vendor option would mean guessing one, so it's left out
    /// rather than faked.
    pub vendor_price: Option<i64>,
    pub ah_price: Option<i64>,
    /// Withheld (not zero) until there's enough price history to mean
    /// anything — see `history::rolling_average`.
    pub price_trend: Option<PriceTrend>,
    pub disenchant: Option<DisenchantOption>,
    pub best_action: &'static str,
    pub best_value: i64,
}

/// Every weapon/armor item owned across every character, ranked by the best
/// known thing to do with it. Items with no priced option at all (no AH
/// price and no disenchant prediction) are left out — nothing to rank.
pub fn rank_disposals(
    game: &GameData,
    disenchant_data: &DisenchantData,
    prices_by_realm: &HashMap<String, PriceTable>,
    history: &History,
    now: i64,
) -> Vec<ItemDisposal> {
    let mut out = Vec::new();
    let empty = PriceTable::new();
    let since = now - TREND_WINDOW_DAYS * 86_400;
    for character in &game.characters {
        let (realm, prices) = ah::resolve_realm(prices_by_realm, &character.realm).unwrap_or((character.realm.as_str(), &empty));
        for item in owned_items(character) {
            let ah_price = prices.get(&item.id).map(|p| p.min_buyout);
            let price_trend = features::enabled("price_history")
                .then(|| {
                    ah_price.and_then(|price| {
                        let average = history::rolling_average(history, realm, item.id, since)?;
                        (average > 0).then(|| PriceTrend { pct_vs_average: (price - average) as f64 / average as f64 * 100.0 })
                    })
                })
                .flatten();

            let disenchant = is_disenchantable(&item).then(|| predict_value(&item, disenchant_data, prices)).flatten();

            let mut best_action = "";
            let mut best_value = i64::MIN;
            if let Some(p) = ah_price {
                best_action = "ah";
                best_value = p;
            }
            if let Some(d) = &disenchant {
                if d.value > best_value {
                    best_action = "disenchant";
                    best_value = d.value;
                }
            }
            if best_action.is_empty() {
                continue;
            }

            out.push(ItemDisposal {
                character: character.name.clone(),
                realm: character.realm.clone(),
                item_id: item.id,
                item_name: item.name.clone(),
                icon: item.icon,
                quality: item.quality,
                count: item.count,
                class_id: item.class_id,
                subclass_id: item.subclass_id,
                equip_loc: item.equip_loc.clone(),
                vendor_price: None,
                ah_price,
                price_trend,
                disenchant,
                best_action,
                best_value,
            });
        }
    }
    out.sort_by_key(|d| std::cmp::Reverse(d.best_value));
    out
}

fn is_disenchantable(item: &Item) -> bool {
    let Some(equip_loc) = item.equip_loc.as_deref().filter(|s| !s.is_empty()) else { return false };
    if matches!(equip_loc, "INVTYPE_BAG" | "INVTYPE_QUIVER" | "INVTYPE_AMMO") {
        return false;
    }
    matches!(item.class_id, Some(CLASS_WEAPON) | Some(CLASS_ARMOR))
}

/// No per-item level in `model::Item` (only a character-wide average), so
/// this can only ever reach exact or quality-wide predictions, never the
/// quality+level bucket — see `disenchant::predict`.
fn predict_value(item: &Item, data: &DisenchantData, prices: &PriceTable) -> Option<DisenchantOption> {
    let (yield_, confidence) = disenchant::predict(item.id, item.quality, None, data)?;
    let mut value = 0i64;
    for &(mat_id, avg_count) in &yield_.mats {
        let price = prices.get(&mat_id)?.min_buyout;
        value += (avg_count * price as f64).round() as i64;
    }
    Some(DisenchantOption { value, confidence: confidence.label(), seen: yield_.n })
}

/// Every item ID owned by any character, across bags and bank. Used to
/// bound price history to items that are actually ranked here, instead of
/// snapshotting every item the AH add-on has ever cached a price for (which
/// can be the whole auction house).
pub fn owned_item_ids(game: &GameData) -> std::collections::HashSet<i64> {
    game.characters.iter().flat_map(|c| c.bags.iter().chain(&c.bank)).flat_map(|bag| &bag.items).map(|item| item.id).collect()
}

/// Every item owned across bags and bank, counts summed by item ID (the
/// same items can be split across several bag slots).
fn owned_items(character: &Character) -> Vec<Item> {
    let mut out: HashMap<i64, Item> = HashMap::new();
    for item in character.bags.iter().chain(&character.bank).flat_map(|bag| &bag.items) {
        out.entry(item.id).and_modify(|existing| existing.count += item.count).or_insert_with(|| item.clone());
    }
    out.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ah::PriceRecord;
    use crate::disenchant::DisenchantYield;
    use crate::model::{Bag, Xp};

    fn price(copper: i64) -> PriceRecord {
        PriceRecord { min_buyout: copper, source: "test" }
    }

    fn item(id: i64, quality: i64, class_id: Option<i64>, equip_loc: Option<&str>) -> Item {
        Item {
            slot: 0,
            id,
            count: 1,
            name: format!("Item #{id}"),
            quality,
            class_id,
            subclass_id: None,
            equip_loc: equip_loc.map(str::to_string),
            icon: None,
            slot_name: None,
        }
    }

    fn character(name: &str, items: Vec<Item>) -> Character {
        Character {
            key: name.into(),
            realm: "Area 52".into(),
            name: name.into(),
            first_name: name.into(),
            surname: String::new(),
            level: 60,
            class_id: 1,
            class_name: "Warrior".into(),
            class_token: "WARRIOR".into(),
            class_color: "#fff".into(),
            race_id: 1,
            race: "Human".into(),
            gender: None,
            faction: "Alliance".into(),
            money: 0,
            xp: Xp { current: 0, max: 0, disabled: true, resting: false, rested: crate::model::Rested { saved: 0, now: 0, cap: 0, pct: 0.0 } },
            played: 0,
            played_this_level: 0,
            zone: String::new(),
            sub_zone: String::new(),
            bind_location: String::new(),
            last_logout: 0,
            health: None,
            power: None,
            item_level: None,
            equipment: Vec::new(),
            bags: vec![Bag { bag_id: 0, name: "Backpack".into(), size: 16, free: 0, items }],
            bank: Vec::new(),
            bank_visited: false,
            bank_scanned: None,
            professions: Vec::new(),
            recipes: Vec::new(),
            recipes_scanned: None,
            reputations: Vec::new(),
            currencies: Vec::new(),
            mail_count: 0,
            mail_expires: None,
        }
    }

    #[test]
    fn ranks_by_ah_price_when_theres_no_disenchant_data() {
        let game = GameData {
            characters: vec![character("Faashri", vec![item(100, 2, Some(CLASS_ARMOR), Some("INVTYPE_WAIST"))])],
            ..Default::default()
        };
        let mut prices = PriceTable::new();
        prices.insert(100, price(500));
        let by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let ranked = rank_disposals(&game, &DisenchantData::default(), &by_realm, &History::new(), 0);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].best_action, "ah");
        assert_eq!(ranked[0].best_value, 500);
        assert!(ranked[0].disenchant.is_none());
    }

    #[test]
    fn disenchant_wins_when_its_mat_value_beats_the_ah_price() {
        let game = GameData {
            characters: vec![character("Faashri", vec![item(2851, 2, Some(CLASS_ARMOR), Some("INVTYPE_WAIST"))])],
            ..Default::default()
        };
        let mut prices = PriceTable::new();
        prices.insert(2851, price(50)); // worth little on the AH as-is
        prices.insert(10938, price(1000)); // but its mat is worth a lot
        let by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let mut dd = DisenchantData::default();
        dd.yields.insert(2851, DisenchantYield { n: 2, mats: vec![(10938, 1.5)] });

        let ranked = rank_disposals(&game, &dd, &by_realm, &History::new(), 0);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].best_action, "disenchant");
        assert_eq!(ranked[0].best_value, 1500);
        let d = ranked[0].disenchant.as_ref().unwrap();
        assert_eq!(d.confidence, "exact");
        assert_eq!(d.seen, 2);
    }

    #[test]
    fn skips_an_item_with_no_priced_option_at_all() {
        let game = GameData {
            characters: vec![character("Faashri", vec![item(999, 2, Some(CLASS_ARMOR), Some("INVTYPE_WAIST"))])],
            ..Default::default()
        };
        let ranked = rank_disposals(&game, &DisenchantData::default(), &HashMap::new(), &History::new(), 0);
        assert!(ranked.is_empty(), "nothing known about this item, so nothing to show");
    }

    #[test]
    fn withholds_disenchant_value_when_a_mat_has_no_price() {
        let game = GameData {
            characters: vec![character("Faashri", vec![item(2851, 2, Some(CLASS_ARMOR), Some("INVTYPE_WAIST"))])],
            ..Default::default()
        };
        let mut prices = PriceTable::new();
        prices.insert(2851, price(50));
        // 10938 (the mat) intentionally left unpriced.
        let by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let mut dd = DisenchantData::default();
        dd.yields.insert(2851, DisenchantYield { n: 2, mats: vec![(10938, 1.5)] });

        let ranked = rank_disposals(&game, &dd, &by_realm, &History::new(), 0);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].best_action, "ah", "falls back to the AH price since disenchant value is unknown");
        assert!(ranked[0].disenchant.is_none());
    }

    #[test]
    fn ignores_non_weapon_armor_items_for_disenchant() {
        let game = GameData {
            characters: vec![character("Faashri", vec![item(2320, 1, None, None)])], // e.g. a cloth reagent
            ..Default::default()
        };
        let mut prices = PriceTable::new();
        prices.insert(2320, price(10));
        let by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let mut dd = DisenchantData::default();
        dd.quality_buckets.insert(1, DisenchantYield { n: 5, mats: vec![(10938, 1.0)] });

        let ranked = rank_disposals(&game, &dd, &by_realm, &History::new(), 0);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].best_action, "ah");
        assert!(ranked[0].disenchant.is_none());
    }

    #[test]
    fn sums_the_same_item_split_across_bag_slots() {
        let mut c = character("Faashri", vec![item(2851, 2, Some(CLASS_ARMOR), Some("INVTYPE_WAIST"))]);
        c.bags.push(Bag { bag_id: 1, name: "Bag".into(), size: 16, free: 0, items: vec![item(2851, 2, Some(CLASS_ARMOR), Some("INVTYPE_WAIST"))] });
        let game = GameData { characters: vec![c], ..Default::default() };
        let mut prices = PriceTable::new();
        prices.insert(2851, price(500));
        let by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let ranked = rank_disposals(&game, &DisenchantData::default(), &by_realm, &History::new(), 0);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].count, 2);
    }

    #[test]
    fn reports_price_trend_against_its_recent_average() {
        let game = GameData {
            characters: vec![character("Faashri", vec![item(100, 2, Some(CLASS_ARMOR), Some("INVTYPE_WAIST"))])],
            ..Default::default()
        };
        let mut prices = PriceTable::new();
        prices.insert(100, price(80)); // currently cheap...
        let by_realm = HashMap::from([("Area 52".to_string(), prices)]);
        // ...against a recent average of 100.
        let history = History::from([(("Area 52".to_string(), 100), vec![(0, 100), (1, 100), (2, 100)])]);

        let ranked = rank_disposals(&game, &DisenchantData::default(), &by_realm, &history, 1_000_000);
        assert_eq!(ranked.len(), 1);
        let trend = ranked[0].price_trend.expect("enough history for a trend");
        assert_eq!(trend.pct_vs_average, -20.0);
    }

    #[test]
    fn realm_is_carried_through_to_the_row() {
        let game = GameData {
            characters: vec![character("Faashri", vec![item(100, 2, Some(CLASS_ARMOR), Some("INVTYPE_WAIST"))])],
            ..Default::default()
        };
        let mut prices = PriceTable::new();
        prices.insert(100, price(500));
        let by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let ranked = rank_disposals(&game, &DisenchantData::default(), &by_realm, &History::new(), 0);
        assert_eq!(ranked[0].realm, "Area 52");
    }

    #[test]
    fn withholds_price_trend_without_enough_history() {
        let game = GameData {
            characters: vec![character("Faashri", vec![item(100, 2, Some(CLASS_ARMOR), Some("INVTYPE_WAIST"))])],
            ..Default::default()
        };
        let mut prices = PriceTable::new();
        prices.insert(100, price(80));
        let by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let ranked = rank_disposals(&game, &DisenchantData::default(), &by_realm, &History::new(), 1_000_000);
        assert_eq!(ranked.len(), 1);
        assert!(ranked[0].price_trend.is_none());
    }
}
