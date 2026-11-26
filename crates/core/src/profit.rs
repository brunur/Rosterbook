//! Ranks known recipes by expected auction-house profit.
//!
//! Pure arithmetic: reagent costs and output price come straight from a
//! [`crate::ah::PriceTable`] (or, lacking that, a vendor sell price), nothing
//! here guesses or asks an LLM for a number. A recipe only appears once its
//! output has *some* known price (AH or vendor) — but a recipe with one or
//! more unpriced reagents still appears, with `cost`/`profit` left `None`
//! and those reagents named in `missing_reagents`, rather than being hidden
//! outright: the user can then go scan exactly what's missing.

use crate::ah::PriceTable;
use crate::model::GameData;
use serde::Serialize;
use std::collections::HashMap;

/// Cut the auction house takes off a sale (5%, same on every realm).
const AH_CUT: f64 = 0.95;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftSuggestion {
    pub character: String,
    pub recipe_id: i64,
    pub recipe_name: String,
    pub output_item: i64,
    pub icon: Option<u32>,
    pub profession: Option<String>,
    /// Total reagent cost, in copper — `None` when one or more reagents
    /// have no known price (see `missing_reagents`).
    pub cost: Option<i64>,
    /// Expected sale price, in copper: the AH price minus its cut, or a
    /// vendor sell price when there's no AH data for this item.
    pub revenue: i64,
    pub revenue_source: &'static str,
    /// What a vendor pays for the output item, in copper — shown
    /// alongside AH revenue regardless of which one `revenue` used.
    pub vendor_sell: Option<i64>,
    /// `None` exactly when `cost` is `None`.
    pub profit: Option<i64>,
    /// True when this character already holds every reagent, in bags or
    /// bank, in the quantity the recipe needs — no AH buying required.
    pub craftable: bool,
    /// Reagent names with no known price. Empty when `cost` is known.
    pub missing_reagents: Vec<String>,
}

/// Every known recipe across every character whose output has a known
/// price, sorted most profitable first (recipes with an unpriced reagent —
/// no `profit` to sort by — sort last). `prices_by_realm` is keyed by realm
/// name, since one account's characters can span several — but falls back
/// to the only realm present when a character's realm name doesn't match
/// any of them (the add-on and RosterBook can disagree on a private
/// server's realm name; e.g. Auctionator may key a realm by its
/// faction-specific AH name rather than the one the game API reports).
pub fn rank_recipes(game: &GameData, prices_by_realm: &HashMap<String, PriceTable>) -> Vec<CraftSuggestion> {
    let mut out = Vec::new();
    let empty = PriceTable::new();
    for character in &game.characters {
        let (_, prices) = crate::ah::resolve_realm(prices_by_realm, &character.realm).unwrap_or((character.realm.as_str(), &empty));
        let owned = owned_counts(character);
        for recipe in &character.recipes {
            let Some(output) = recipe.output else { continue };
            if recipe.reagents.is_empty() {
                continue;
            }
            let (revenue, revenue_source) = match prices.get(&output) {
                Some(sell) => ((sell.min_buyout as f64 * AH_CUT) as i64, "ah"),
                None => match recipe.vendor_price {
                    Some(p) => (p, "vendor"),
                    None => continue,
                },
            };

            let mut cost = 0i64;
            let mut missing_reagents = Vec::new();
            for reagent in &recipe.reagents {
                match prices.get(&reagent.id) {
                    Some(p) => cost = cost.saturating_add(p.min_buyout.saturating_mul(reagent.count)),
                    None => missing_reagents.push(reagent.name.clone()),
                }
            }
            let cost = missing_reagents.is_empty().then_some(cost);

            let craftable = recipe.reagents.iter().all(|r| owned.get(&r.id).copied().unwrap_or(0) >= r.count);
            out.push(CraftSuggestion {
                character: character.name.clone(),
                recipe_id: recipe.id,
                recipe_name: recipe.name.clone(),
                output_item: output,
                icon: recipe.icon,
                profession: recipe.profession.clone(),
                cost,
                revenue,
                revenue_source,
                vendor_sell: recipe.vendor_price,
                profit: cost.map(|c| revenue - c),
                craftable,
                missing_reagents,
            });
        }
    }
    out.sort_by_key(|r| std::cmp::Reverse(r.profit.unwrap_or(i64::MIN)));
    out
}

/// Item ID -> how many a character has on hand, bags and bank combined.
fn owned_counts(character: &crate::model::Character) -> HashMap<i64, i64> {
    let mut out = HashMap::new();
    for item in character.bags.iter().chain(&character.bank).flat_map(|bag| &bag.items) {
        *out.entry(item.id).or_insert(0) += item.count;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ah::PriceRecord;
    use crate::model::{Character, Profession, Reagent, Recipe};
    use std::collections::HashMap;

    fn character(name: &str, recipes: Vec<Recipe>) -> Character {
        Character {
            key: name.to_string(),
            realm: "Area 52".into(),
            name: name.to_string(),
            first_name: name.to_string(),
            surname: String::new(),
            level: 80,
            class_id: 1,
            class_name: "Warrior".into(),
            class_token: "WARRIOR".into(),
            class_color: "#fff".into(),
            race_id: 1,
            race: "Human".into(),
            gender: None,
            faction: "Alliance".into(),
            money: 0,
            xp: crate::model::Xp {
                current: 0,
                max: 0,
                disabled: true,
                resting: false,
                rested: crate::model::Rested { saved: 0, now: 0, cap: 0, pct: 0.0 },
            },
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
            bags: Vec::new(),
            bank: Vec::new(),
            bank_visited: false,
            bank_scanned: None,
            professions: vec![Profession { name: "Alchemy".into(), rank: 100, max: 100, secondary: false }],
            recipes,
            recipes_scanned: None,
            reputations: Vec::new(),
            currencies: Vec::new(),
            mail_count: 0,
            mail_expires: None,
        }
    }

    fn price(copper: i64) -> PriceRecord {
        PriceRecord { min_buyout: copper, source: "test" }
    }

    #[test]
    fn ranks_recipes_with_full_price_data_by_profit() {
        let cheap = Recipe {
            id: 1,
            name: "Cheap potion".into(),
            profession: Some("Alchemy".into()),
            output: Some(100),
            icon: None,
            reagents: vec![Reagent { id: 10, name: "Herb".into(), count: 2, icon: None }],
            vendor_price: None,
        };
        let pricey = Recipe {
            id: 2,
            name: "Pricey elixir".into(),
            profession: Some("Alchemy".into()),
            output: Some(200),
            icon: None,
            reagents: vec![Reagent { id: 10, name: "Herb".into(), count: 1, icon: None }],
            vendor_price: None,
        };
        let unpriced_mat = Recipe {
            id: 3,
            name: "Needs rare mat".into(),
            profession: Some("Alchemy".into()),
            output: Some(300),
            icon: None,
            reagents: vec![Reagent { id: 999, name: "Mystery".into(), count: 1, icon: None }],
            vendor_price: None,
        };
        let game = GameData {
            characters: vec![character("Faashri", vec![cheap, pricey, unpriced_mat])],
            ..Default::default()
        };

        let mut prices: PriceTable = HashMap::new();
        prices.insert(10, price(100)); // herb
        prices.insert(100, price(1000)); // cheap potion sells for 1000
        prices.insert(200, price(10000)); // pricey elixir sells for 10000
        // 300 and 999 intentionally have no price, and recipe 3 has no vendor_price either.
        let prices_by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let ranked = rank_recipes(&game, &prices_by_realm);
        assert_eq!(ranked.len(), 2, "the recipe with no price for its output at all is left out");
        assert_eq!(ranked[0].recipe_id, 2, "the more profitable recipe sorts first");
        assert!(ranked[0].profit > ranked[1].profit);
        assert_eq!(ranked[1].cost, Some(200)); // 2 herbs * 100 copper
        assert_eq!(ranked[1].revenue, (1000.0 * AH_CUT) as i64);
        assert_eq!(ranked[1].revenue_source, "ah");
    }

    #[test]
    fn skips_recipes_with_no_known_output_price_at_all() {
        let recipe = Recipe {
            id: 1,
            name: "No sale data".into(),
            profession: Some("Alchemy".into()),
            output: Some(999),
            icon: None,
            reagents: vec![Reagent { id: 10, name: "Herb".into(), count: 1, icon: None }],
            vendor_price: None,
        };
        let game = GameData { characters: vec![character("Faashri", vec![recipe])], ..Default::default() };
        let mut prices = PriceTable::new();
        prices.insert(10, price(100));
        let prices_by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        assert!(rank_recipes(&game, &prices_by_realm).is_empty());
    }

    #[test]
    fn falls_back_to_vendor_price_when_the_ah_has_none() {
        let recipe = Recipe {
            id: 1,
            name: "Linen Cloth Bolt".into(),
            profession: Some("Tailoring".into()),
            output: Some(999), // not in the AH price table below
            icon: None,
            reagents: vec![Reagent { id: 10, name: "Linen".into(), count: 2, icon: None }],
            vendor_price: Some(40),
        };
        let game = GameData { characters: vec![character("Faashri", vec![recipe])], ..Default::default() };
        let mut prices = PriceTable::new();
        prices.insert(10, price(5));
        let prices_by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let ranked = rank_recipes(&game, &prices_by_realm);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].revenue, 40, "vendor sell price, no AH cut taken off it");
        assert_eq!(ranked[0].revenue_source, "vendor");
        assert_eq!(ranked[0].cost, Some(10));
        assert_eq!(ranked[0].profit, Some(30));
    }

    #[test]
    fn ah_price_wins_over_vendor_price_when_both_are_known() {
        let recipe = Recipe {
            id: 1,
            name: "Linen Cloth Bolt".into(),
            profession: Some("Tailoring".into()),
            output: Some(100),
            icon: None,
            reagents: vec![Reagent { id: 10, name: "Linen".into(), count: 1, icon: None }],
            vendor_price: Some(1), // deliberately far below the AH price
        };
        let game = GameData { characters: vec![character("Faashri", vec![recipe])], ..Default::default() };
        let mut prices = PriceTable::new();
        prices.insert(10, price(5));
        prices.insert(100, price(1000));
        let prices_by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let ranked = rank_recipes(&game, &prices_by_realm);
        assert_eq!(ranked[0].revenue_source, "ah");
        assert_eq!(ranked[0].revenue, (1000.0 * AH_CUT) as i64);
    }

    #[test]
    fn flags_missing_reagent_prices_instead_of_hiding_the_recipe() {
        let recipe = Recipe {
            id: 1,
            name: "Needs rare mat".into(),
            profession: Some("Alchemy".into()),
            output: Some(100),
            icon: None,
            reagents: vec![
                Reagent { id: 10, name: "Herb".into(), count: 1, icon: None },
                Reagent { id: 999, name: "Mystery Dust".into(), count: 1, icon: None },
            ],
            vendor_price: None,
        };
        let game = GameData { characters: vec![character("Faashri", vec![recipe])], ..Default::default() };
        let mut prices = PriceTable::new();
        prices.insert(10, price(50));
        prices.insert(100, price(1000));
        // 999 intentionally left unpriced.
        let prices_by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let ranked = rank_recipes(&game, &prices_by_realm);
        assert_eq!(ranked.len(), 1, "still shown, just without a cost/profit");
        assert_eq!(ranked[0].cost, None);
        assert_eq!(ranked[0].profit, None);
        assert_eq!(ranked[0].missing_reagents, vec!["Mystery Dust".to_string()]);
    }

    #[test]
    fn recipes_with_unknown_profit_sort_after_priced_ones() {
        let priced = Recipe {
            id: 1,
            name: "Cheap potion".into(),
            profession: Some("Alchemy".into()),
            output: Some(100),
            icon: None,
            reagents: vec![Reagent { id: 10, name: "Herb".into(), count: 1, icon: None }],
            vendor_price: None,
        };
        let unpriced_reagent = Recipe {
            id: 2,
            name: "Needs rare mat".into(),
            profession: Some("Alchemy".into()),
            output: Some(200),
            icon: None,
            reagents: vec![Reagent { id: 999, name: "Mystery".into(), count: 1, icon: None }],
            vendor_price: None,
        };
        let game = GameData {
            characters: vec![character("Faashri", vec![unpriced_reagent, priced])],
            ..Default::default()
        };
        let mut prices = PriceTable::new();
        prices.insert(10, price(50));
        prices.insert(100, price(1000));
        prices.insert(200, price(2000)); // output priced; only the reagent (999) is missing
        let prices_by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let ranked = rank_recipes(&game, &prices_by_realm);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].recipe_id, 1, "the fully-priced recipe sorts first");
        assert_eq!(ranked[1].recipe_id, 2);
    }

    #[test]
    fn marks_craftable_when_bags_and_bank_cover_every_reagent() {
        let recipe = Recipe {
            id: 1,
            name: "Cheap potion".into(),
            profession: Some("Alchemy".into()),
            output: Some(100),
            icon: None,
            reagents: vec![
                Reagent { id: 10, name: "Herb".into(), count: 2, icon: None },
                Reagent { id: 11, name: "Vial".into(), count: 1, icon: None },
            ],
            vendor_price: None,
        };
        let owned_item = |id: i64| crate::model::Item {
            slot: 1,
            id,
            count: 1,
            name: String::new(),
            quality: 1,
            class_id: None,
            subclass_id: None,
            equip_loc: None,
            icon: None,
            slot_name: None,
        };
        let mut c = character("Faashri", vec![recipe]);
        c.bags = vec![crate::model::Bag { bag_id: 0, name: "Backpack".into(), size: 16, free: 14, items: vec![owned_item(10)] }];
        c.bank = vec![crate::model::Bag { bag_id: -1, name: "Bank".into(), size: 28, free: 27, items: vec![owned_item(10)] }];
        let game = GameData { characters: vec![c], ..Default::default() };
        let mut prices = PriceTable::new();
        prices.insert(10, price(50));
        prices.insert(11, price(20));
        prices.insert(100, price(1000));
        let prices_by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let ranked = rank_recipes(&game, &prices_by_realm);
        assert_eq!(ranked.len(), 1);
        // 2 herbs owned (1 bag + 1 bank) but 0 vials: not craftable.
        assert!(!ranked[0].craftable);
    }

    #[test]
    fn falls_back_to_the_only_realm_when_the_names_dont_match() {
        // The character's realm name comes from RosterBook's own add-on data
        // (the game's connected-realm name); the price table's key here is
        // whatever a different realm name Auctionator used. On a private
        // server these routinely disagree even though it's really the same
        // (and only) realm in play.
        let recipe = Recipe {
            id: 1,
            name: "Cheap potion".into(),
            profession: Some("Alchemy".into()),
            output: Some(100),
            icon: None,
            reagents: vec![Reagent { id: 10, name: "Herb".into(), count: 1, icon: None }],
            vendor_price: None,
        };
        let game = GameData { characters: vec![character("Faashri", vec![recipe])], ..Default::default() };
        let mut prices = PriceTable::new();
        prices.insert(10, price(50));
        prices.insert(100, price(500));
        let prices_by_realm = HashMap::from([("PvE Alliance".to_string(), prices)]);

        let ranked = rank_recipes(&game, &prices_by_realm);
        assert_eq!(ranked.len(), 1, "falls back since this is the only realm present");
        assert_eq!(ranked[0].cost, Some(50));
    }
}
