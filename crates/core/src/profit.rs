//! Ranks known recipes by expected auction-house profit.
//!
//! Pure arithmetic: reagent costs and output price come straight from a
//! [`crate::ah::PriceTable`], nothing here guesses or asks an LLM for a
//! number. A recipe is only ranked once every reagent and the output have a
//! known price — a recipe missing any of that data is left out rather than
//! given a cost that's too low (and so a profit that's too good).

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
    /// Total reagent cost, in copper.
    pub cost: i64,
    /// Expected sale price after the AH cut, in copper.
    pub revenue: i64,
    pub profit: i64,
}

/// Every known recipe across every character whose mats and output both
/// have a current price, sorted most profitable first. `prices_by_realm` is
/// keyed by realm name, since one account's characters can span several.
pub fn rank_recipes(game: &GameData, prices_by_realm: &HashMap<String, PriceTable>) -> Vec<CraftSuggestion> {
    let mut out = Vec::new();
    let empty = PriceTable::new();
    for character in &game.characters {
        let prices = prices_by_realm.get(&character.realm).unwrap_or(&empty);
        for recipe in &character.recipes {
            let Some(output) = recipe.output else { continue };
            let Some(sell) = prices.get(&output) else { continue };
            if recipe.reagents.is_empty() {
                continue;
            }

            let mut cost = 0i64;
            let mut complete = true;
            for reagent in &recipe.reagents {
                match prices.get(&reagent.id) {
                    Some(p) => cost += p.min_buyout * reagent.count,
                    None => {
                        complete = false;
                        break;
                    }
                }
            }
            if !complete {
                continue;
            }

            let revenue = (sell.min_buyout as f64 * AH_CUT) as i64;
            out.push(CraftSuggestion {
                character: character.name.clone(),
                recipe_id: recipe.id,
                recipe_name: recipe.name.clone(),
                output_item: output,
                icon: recipe.icon,
                cost,
                revenue,
                profit: revenue - cost,
            });
        }
    }
    out.sort_by(|a, b| b.profit.cmp(&a.profit));
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
        };
        let pricey = Recipe {
            id: 2,
            name: "Pricey elixir".into(),
            profession: Some("Alchemy".into()),
            output: Some(200),
            icon: None,
            reagents: vec![Reagent { id: 10, name: "Herb".into(), count: 1, icon: None }],
        };
        let unpriced_mat = Recipe {
            id: 3,
            name: "Needs rare mat".into(),
            profession: Some("Alchemy".into()),
            output: Some(300),
            icon: None,
            reagents: vec![Reagent { id: 999, name: "Mystery".into(), count: 1, icon: None }],
        };
        let game = GameData {
            characters: vec![character("Faashri", vec![cheap, pricey, unpriced_mat])],
            ..Default::default()
        };

        let mut prices: PriceTable = HashMap::new();
        prices.insert(10, price(100)); // herb
        prices.insert(100, price(1000)); // cheap potion sells for 1000
        prices.insert(200, price(10000)); // pricey elixir sells for 10000
        // 300 and 999 intentionally have no price.
        let prices_by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        let ranked = rank_recipes(&game, &prices_by_realm);
        assert_eq!(ranked.len(), 2, "the recipe needing an unpriced mat is left out");
        assert_eq!(ranked[0].recipe_id, 2, "the more profitable recipe sorts first");
        assert!(ranked[0].profit > ranked[1].profit);
        assert_eq!(ranked[1].cost, 200); // 2 herbs * 100 copper
        assert_eq!(ranked[1].revenue, (1000.0 * AH_CUT) as i64);
    }

    #[test]
    fn skips_recipes_with_no_known_output_price() {
        let recipe = Recipe {
            id: 1,
            name: "No sale data".into(),
            profession: Some("Alchemy".into()),
            output: Some(999),
            icon: None,
            reagents: vec![Reagent { id: 10, name: "Herb".into(), count: 1, icon: None }],
        };
        let game = GameData { characters: vec![character("Faashri", vec![recipe])], ..Default::default() };
        let mut prices = PriceTable::new();
        prices.insert(10, price(100));
        let prices_by_realm = HashMap::from([("Area 52".to_string(), prices)]);

        assert!(rank_recipes(&game, &prices_by_realm).is_empty());
    }
}
