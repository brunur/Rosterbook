//! The character model shown by the app, built from the RosterBook add-on's
//! saved variable (`Rosterbook.lua` → `RosterbookDB`).

use crate::lua::parse_saved_variables;
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::Path;

pub const DATA_FILE: &str = "Rosterbook.lua";
/// Highest add-on data format this version understands.
pub const SUPPORTED_SCHEMA: i64 = 1;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GameData {
    pub flavor: Option<String>,
    pub build: Option<String>,
    pub characters: Vec<Character>,
    pub warband: Option<Warband>,
    pub totals: Totals,
    pub errors: Vec<String>,
    pub generated_at: i64,
}

impl GameData {
    /// Every item icon file ID used by any character, bank or warband bank.
    pub fn icon_ids(&self) -> std::collections::BTreeSet<u32> {
        let bags = self.characters.iter().flat_map(|c| c.bags.iter().chain(&c.bank));
        let warband = self.warband.iter().flat_map(|w| &w.tabs);
        let mut ids: std::collections::BTreeSet<u32> = bags
            .chain(warband)
            .flat_map(|b| &b.items)
            .chain(self.characters.iter().flat_map(|c| &c.equipment))
            .filter_map(|i| i.icon)
            .collect();
        ids.extend(self.characters.iter().flat_map(|c| &c.currencies).filter_map(|cur| cur.icon));
        ids.extend(self.characters.iter().flat_map(|c| &c.recipes).filter_map(|r| r.icon));
        ids.extend(self.characters.iter().flat_map(|c| &c.recipes).flat_map(|r| &r.reagents).filter_map(|rg| rg.icon));
        // 0 is never a real file ID — some "ByID" icon lookups return it for
        // an item whose data hasn't finished loading yet (reagents are the
        // case that hits this). One bad ID here fails the whole batch fetch,
        // taking every other icon down with it, so it's dropped rather than
        // sent.
        ids.remove(&0);
        ids
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub money: i64,
    pub characters: usize,
    pub played: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Warband {
    pub tabs: Vec<Bag>,
    pub scanned: Option<i64>,
    pub money: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    /// Stable identity (the character's GUID). Used for ordering, pinning and hiding.
    pub key: String,
    pub realm: String,
    pub name: String,
    pub first_name: String,
    pub surname: String,
    pub level: i64,
    pub class_id: i64,
    pub class_name: String,
    pub class_token: String,
    pub class_color: String,
    pub race_id: i64,
    pub race: String,
    pub gender: Option<String>,
    pub faction: String,
    pub money: i64,
    pub xp: Xp,
    pub played: i64,
    pub played_this_level: i64,
    pub zone: String,
    pub sub_zone: String,
    pub bind_location: String,
    pub last_logout: i64,
    pub health: Option<i64>,
    pub power: Option<Power>,
    pub item_level: Option<f64>,
    pub equipment: Vec<Item>,
    pub bags: Vec<Bag>,
    pub bank: Vec<Bag>,
    pub bank_visited: bool,
    pub bank_scanned: Option<i64>,
    pub professions: Vec<Profession>,
    pub recipes: Vec<Recipe>,
    pub recipes_scanned: Option<i64>,
    pub reputations: Vec<Reputation>,
    pub currencies: Vec<Currency>,
    pub mail_count: i64,
    pub mail_expires: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Xp {
    pub current: i64,
    /// 0 at max level (no XP bar).
    pub max: i64,
    pub disabled: bool,
    pub resting: bool,
    pub rested: Rested,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rested {
    /// Rested XP when the character logged out.
    pub saved: i64,
    /// Estimate including time rested since logout (8 h = 5 % of a level, ¼ speed outside an inn).
    pub now: i64,
    pub cap: i64,
    pub pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Power {
    #[serde(rename = "type")]
    pub kind: String,
    pub max: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bag {
    pub bag_id: i64,
    pub name: String,
    pub size: i64,
    pub free: i64,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub slot: i64,
    pub id: i64,
    pub count: i64,
    pub name: String,
    pub quality: i64,
    /// Item class / subclass (Enum.ItemClass) for the app's own category icons.
    pub class_id: Option<i64>,
    pub subclass_id: Option<i64>,
    pub equip_loc: Option<String>,
    /// The game's icon file ID, for showing pictures from the local install.
    pub icon: Option<u32>,
    /// Equipment slot name, for gear only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profession {
    pub name: String,
    pub rank: i64,
    pub max: i64,
    pub secondary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reagent {
    pub id: i64,
    pub name: String,
    pub count: i64,
    /// The game's icon file ID, for showing a picture from the local install.
    pub icon: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub id: i64,
    pub name: String,
    pub profession: Option<String>,
    /// The item this recipe produces, when the add-on could resolve one.
    pub output: Option<i64>,
    pub icon: Option<u32>,
    pub reagents: Vec<Reagent>,
    /// What a vendor pays for one output item, in copper — a floor price
    /// when there's no (or no better) auction house data for it.
    pub vendor_price: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reputation {
    pub id: i64,
    pub name: String,
    pub group: Option<String>,
    pub kind: String,
    pub label: String,
    pub color: String,
    /// Progress within the current level.
    pub value: i64,
    pub max: i64,
    pub pct: f64,
    /// Sort key: higher = more progress overall.
    pub rank: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Currency {
    pub id: i64,
    pub name: String,
    /// The game's icon file ID, for showing a picture from the local install.
    pub icon: Option<u32>,
    pub quantity: i64,
    /// Set only for currencies with a cap (weekly honor, seasonal tokens, …).
    pub max_quantity: Option<i64>,
    pub quality: i64,
}

// ---------------------------------------------------------------------------
// Static lookups
// ---------------------------------------------------------------------------

pub fn class_info(id: i64) -> (&'static str, &'static str, &'static str) {
    match id {
        1 => ("Warrior", "WARRIOR", "#C69B6D"),
        2 => ("Paladin", "PALADIN", "#F48CBA"),
        3 => ("Hunter", "HUNTER", "#AAD372"),
        4 => ("Rogue", "ROGUE", "#FFF468"),
        5 => ("Priest", "PRIEST", "#FFFFFF"),
        6 => ("Death Knight", "DEATHKNIGHT", "#C41E3A"),
        7 => ("Shaman", "SHAMAN", "#0070DD"),
        8 => ("Mage", "MAGE", "#3FC7EB"),
        9 => ("Warlock", "WARLOCK", "#8788EE"),
        10 => ("Monk", "MONK", "#00FF98"),
        11 => ("Druid", "DRUID", "#FF7C0A"),
        12 => ("Demon Hunter", "DEMONHUNTER", "#A330C9"),
        13 => ("Evoker", "EVOKER", "#33937F"),
        _ => ("Unknown", "UNKNOWN", "#999999"),
    }
}

fn slot_name(slot: i64) -> &'static str {
    match slot {
        1 => "Head", 2 => "Neck", 3 => "Shoulder", 4 => "Shirt", 5 => "Chest", 6 => "Waist",
        7 => "Legs", 8 => "Feet", 9 => "Wrist", 10 => "Hands", 11 => "Finger 1", 12 => "Finger 2",
        13 => "Trinket 1", 14 => "Trinket 2", 15 => "Back", 16 => "Main Hand", 17 => "Off Hand",
        18 => "Ranged", 19 => "Tabard", _ => "Slot",
    }
}

fn standing(id: i64) -> (&'static str, &'static str) {
    match id {
        1 => ("Hated", "#cc2222"),
        2 => ("Hostile", "#e03a3a"),
        3 => ("Unfriendly", "#e07a3a"),
        4 => ("Neutral", "#d9b43a"),
        5 => ("Friendly", "#4cc98a"),
        6 => ("Honored", "#4cc98a"),
        7 => ("Revered", "#4cc98a"),
        8 => ("Exalted", "#4cc98a"),
        _ => ("", "#d9a441"),
    }
}

// ---------------------------------------------------------------------------
// Value helpers (the add-on's tables may arrive as arrays or sparse objects)
// ---------------------------------------------------------------------------

fn list(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(a) => a.iter().filter(|x| !x.is_null()).collect(),
        Value::Object(o) => {
            let mut pairs: Vec<(i64, &Value)> =
                o.iter().map(|(k, v)| (k.parse::<i64>().unwrap_or(i64::MAX), v)).collect();
            pairs.sort_by_key(|(k, _)| *k);
            pairs.into_iter().map(|(_, v)| v).collect()
        }
        _ => Vec::new(),
    }
}

fn int(v: &Value, key: &str) -> Option<i64> {
    match v.get(key)? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64)),
        _ => None,
    }
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key)?.as_f64()
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(str::to_owned).filter(|s| !s.is_empty())
}

fn flag(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Building the model
// ---------------------------------------------------------------------------

/// Reads `Rosterbook.lua` from a SavedVariables folder. Missing file → empty data.
pub fn load_game_data(saved_variables_dir: &Path, now: i64) -> GameData {
    let file = saved_variables_dir.join(DATA_FILE);
    match std::fs::read_to_string(&file) {
        Ok(src) => match parse_saved_variables(&src) {
            Ok(vars) => build(vars.get("RosterbookDB").unwrap_or(&Value::Null), now),
            Err(e) => GameData { errors: vec![format!("{DATA_FILE}: {e}")], generated_at: now, ..Default::default() },
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => GameData { generated_at: now, ..Default::default() },
        Err(e) => GameData { errors: vec![format!("{DATA_FILE}: {e}")], generated_at: now, ..Default::default() },
    }
}

pub fn build(db: &Value, now: i64) -> GameData {
    let mut errors = Vec::new();
    if let Some(schema) = int(db, "schema") {
        if schema > SUPPORTED_SCHEMA {
            errors.push(format!("Saved data uses a newer format (schema {schema}). Update RosterBook."));
        }
    }

    let empty = Map::new();
    let chars_obj = db.get("characters").and_then(Value::as_object).unwrap_or(&empty);
    let mut raw: Vec<&Value> = chars_obj.values().filter(|c| text(c, "name").is_some()).collect();
    raw.sort_by_key(|c| int(c, "lastLogout").unwrap_or(0));

    let characters: Vec<Character> = raw.into_iter().map(|c| character(c, now)).collect();

    let warband = db.get("warband").and_then(|w| {
        let tabs: Vec<Bag> = list(w.get("tabs").unwrap_or(&Value::Null)).into_iter().map(bag).collect();
        (!tabs.is_empty()).then(|| Warband { tabs, scanned: int(w, "scanned"), money: int(w, "money").unwrap_or(0) })
    });

    let totals = Totals {
        money: characters.iter().map(|c| c.money).sum(),
        characters: characters.len(),
        played: characters.iter().map(|c| c.played).sum(),
    };

    GameData { flavor: text(db, "game"), build: text(db, "build"), characters, warband, totals, errors, generated_at: now }
}

fn rested(c: &Value, now: i64) -> Rested {
    let max = int(c, "xpMax").unwrap_or(0);
    if max <= 0 {
        return Rested { saved: 0, now: 0, cap: 0, pct: 0.0 };
    }
    let cap = max as f64 * 1.5;
    let saved = int(c, "restXP").unwrap_or(0);
    let since = (now - int(c, "lastLogout").unwrap_or(now)).max(0) as f64;
    let mut earned = since / 28_800.0 * (max as f64 / 20.0);
    if !flag(c, "resting") {
        earned /= 4.0;
    }
    let total = (saved as f64 + earned).min(cap);
    Rested { saved, now: total.round() as i64, cap: cap.round() as i64, pct: total / cap * 100.0 }
}

fn item(v: &Value) -> Item {
    let id = int(v, "id").unwrap_or(0);
    Item {
        slot: int(v, "slot").unwrap_or(0),
        id,
        count: int(v, "count").unwrap_or(1),
        name: text(v, "name").unwrap_or_else(|| format!("Item #{id}")),
        quality: int(v, "quality").unwrap_or(1),
        class_id: int(v, "classId"),
        subclass_id: int(v, "subclassId"),
        equip_loc: text(v, "equipLoc"),
        icon: int(v, "icon").and_then(|i| u32::try_from(i).ok()).filter(|&i| i > 0),
        slot_name: None,
    }
}

fn bag(v: &Value) -> Bag {
    let id = int(v, "id").unwrap_or(0);
    Bag {
        bag_id: id,
        name: if id == 0 { "Backpack".into() } else { text(v, "name").unwrap_or_else(|| format!("Bag {id}")) },
        size: int(v, "size").unwrap_or(0),
        free: int(v, "free").unwrap_or(0),
        items: list(v.get("items").unwrap_or(&Value::Null)).into_iter().map(item).collect(),
    }
}

fn reputation(r: &Value) -> Reputation {
    let kind = text(r, "kind").unwrap_or_else(|| "standing".into());
    let value = int(r, "value").unwrap_or(0);
    let min = int(r, "min").unwrap_or(0);
    let max = int(r, "max").unwrap_or(0);
    let span = max - min;
    let progress = (value - min).max(0);
    let (std_label, std_color) = standing(int(r, "standing").unwrap_or(0));
    let special = kind == "renown" || kind == "friendship";
    let id = int(r, "id").unwrap_or(0);
    Reputation {
        id,
        name: text(r, "name").unwrap_or_else(|| format!("Faction #{id}")),
        group: text(r, "group"),
        label: text(r, "label").unwrap_or_else(|| std_label.to_string()),
        color: match kind.as_str() {
            "renown" => "#d9a441".into(),
            "friendship" => "#6fb7d9".into(),
            _ => std_color.into(),
        },
        value: progress,
        max: span.max(0),
        pct: if span > 0 { (progress as f64 / span as f64 * 100.0).min(100.0) } else { 100.0 },
        rank: if special { 42_000 + value } else { value },
        kind,
    }
}

fn reagent(v: &Value) -> Reagent {
    let id = int(v, "id").unwrap_or(0);
    Reagent {
        id,
        name: text(v, "name").unwrap_or_else(|| format!("Item #{id}")),
        count: int(v, "count").unwrap_or(1),
        icon: int(v, "icon").map(|i| i as u32),
    }
}

fn recipe(v: &Value) -> Recipe {
    let id = int(v, "id").unwrap_or(0);
    Recipe {
        id,
        name: text(v, "name").unwrap_or_else(|| format!("Recipe #{id}")),
        profession: text(v, "profession"),
        output: int(v, "output"),
        icon: int(v, "icon").map(|i| i as u32),
        reagents: list(v.get("reagents").unwrap_or(&Value::Null)).into_iter().map(reagent).collect(),
        // A 0 sell price means the item isn't vendor-sellable at all, not
        // that it's worth nothing — treat the same as not having one.
        vendor_price: int(v, "vendorPrice").filter(|p| *p > 0),
    }
}

fn currency(v: &Value) -> Currency {
    let id = int(v, "id").unwrap_or(0);
    Currency {
        id,
        name: text(v, "name").unwrap_or_else(|| format!("Currency #{id}")),
        icon: int(v, "icon").map(|i| i as u32),
        quantity: int(v, "quantity").unwrap_or(0),
        max_quantity: int(v, "maxQuantity"),
        quality: int(v, "quality").unwrap_or(1),
    }
}

fn character(c: &Value, now: i64) -> Character {
    let class_id = int(c, "classId").unwrap_or(0);
    let (class_name, class_token, class_color) = class_info(class_id);
    let name = text(c, "name").unwrap_or_default();
    let mut parts = name.splitn(2, ' ');
    let first = parts.next().unwrap_or_default().to_string();
    let rest = parts.next().unwrap_or_default().to_string();
    let max_level = flag(c, "maxLevel");
    let mail = c.get("mail").cloned().unwrap_or(Value::Null);

    let mut equipment: Vec<Item> = list(c.get("equipment").unwrap_or(&Value::Null))
        .into_iter()
        .map(|e| {
            let mut it = item(e);
            it.count = 1;
            it.slot_name = Some(slot_name(it.slot).to_string());
            it
        })
        .collect();
    equipment.sort_by_key(|e| e.slot);

    let mut reputations: Vec<Reputation> =
        list(c.get("reputations").unwrap_or(&Value::Null)).into_iter().map(reputation).collect();
    reputations.sort_by_key(|r| std::cmp::Reverse(r.rank));

    let power = int(c, "powerMax").filter(|m| *m > 0).map(|max| Power {
        kind: match text(c, "powerToken").as_deref() {
            Some("MANA") => "Mana",
            Some("RAGE") => "Rage",
            Some("ENERGY") => "Energy",
            Some("FOCUS") => "Focus",
            Some("RUNIC_POWER") => "Runic Power",
            Some("FURY") => "Fury",
            Some("INSANITY") => "Insanity",
            Some("MAELSTROM") => "Maelstrom",
            Some("LUNAR_POWER") => "Astral Power",
            _ => "Power",
        }
        .to_string(),
        max,
    });

    Character {
        key: text(c, "guid").unwrap_or_else(|| name.clone()),
        realm: text(c, "realm").unwrap_or_default(),
        first_name: text(c, "firstName").unwrap_or(first),
        surname: text(c, "secondName").unwrap_or(rest),
        name,
        level: int(c, "level").unwrap_or(0),
        class_id,
        class_name: text(c, "class").unwrap_or_else(|| class_name.into()),
        class_token: text(c, "classToken").unwrap_or_else(|| class_token.into()),
        class_color: class_color.into(),
        race_id: int(c, "raceId").unwrap_or(0),
        race: text(c, "race").unwrap_or_else(|| "Unknown".into()),
        gender: match int(c, "sex") {
            Some(2) => Some("Male".into()),
            Some(3) => Some("Female".into()),
            _ => None,
        },
        faction: text(c, "faction").unwrap_or_else(|| "Unknown".into()),
        money: int(c, "money").unwrap_or(0),
        xp: Xp {
            current: int(c, "xp").unwrap_or(0),
            max: if max_level { 0 } else { int(c, "xpMax").unwrap_or(0) },
            disabled: flag(c, "xpDisabled"),
            resting: flag(c, "resting"),
            rested: rested(c, now),
        },
        played: int(c, "played").unwrap_or(0),
        played_this_level: int(c, "playedThisLevel").unwrap_or(0),
        zone: text(c, "zone").unwrap_or_default(),
        sub_zone: text(c, "subZone").unwrap_or_default(),
        bind_location: text(c, "bindLocation").unwrap_or_default(),
        last_logout: int(c, "lastLogout").or_else(|| int(c, "lastUpdate")).unwrap_or(0),
        health: int(c, "healthMax"),
        power,
        item_level: num(c, "itemLevel").filter(|v| *v > 0.0),
        equipment,
        bags: list(c.get("bags").unwrap_or(&Value::Null)).into_iter().map(bag).collect(),
        bank: list(c.get("bank").unwrap_or(&Value::Null)).into_iter().map(bag).collect(),
        bank_visited: int(c, "bankScanned").is_some(),
        bank_scanned: int(c, "bankScanned"),
        professions: list(c.get("professions").unwrap_or(&Value::Null))
            .into_iter()
            .map(|p| Profession {
                name: text(p, "name").unwrap_or_default(),
                rank: int(p, "rank").unwrap_or(0),
                max: int(p, "max").unwrap_or(0),
                secondary: text(p, "kind").as_deref() == Some("secondary"),
            })
            .collect(),
        recipes: list(c.get("recipes").unwrap_or(&Value::Null)).into_iter().map(recipe).collect(),
        recipes_scanned: int(c, "recipesScanned"),
        reputations,
        currencies: list(c.get("currencies").unwrap_or(&Value::Null)).into_iter().map(currency).collect(),
        mail_count: int(&mail, "count").unwrap_or(0),
        mail_expires: int(&mail, "soonestExpiry"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::parse_saved_variables;

    const FIXTURE: &str = include_str!("../tests/fixtures/Rosterbook.retail.lua");

    #[test]
    fn builds_model_from_addon_output() {
        let vars = parse_saved_variables(FIXTURE).unwrap();
        let g = build(&vars["RosterbookDB"], 1_792_600_000);
        assert!(g.errors.is_empty(), "{:?}", g.errors);
        assert_eq!(g.flavor.as_deref(), Some("retail"));
        assert_eq!(g.characters.len(), 1);
        let c = &g.characters[0];
        assert_eq!(c.first_name, "Norasonia");
        assert_eq!(c.class_name, "Paladin");
        assert_eq!(c.level, 80);
        assert_eq!(c.xp.max, 0, "max-level characters have no XP bar");
        assert_eq!(c.bank[0].name, "Mats");
        assert!(c.reputations.iter().any(|r| r.label == "Renown 22"));
        assert_eq!(c.currencies.len(), 2);
        assert!(c.currencies.iter().any(|cur| cur.name == "Merchant's Favor" && cur.max_quantity.is_none()));
        let honor = c.currencies.iter().find(|cur| cur.name == "Honor Points").expect("honor points");
        assert_eq!((honor.quantity, honor.max_quantity), (2450, Some(4000)));
        assert_eq!(c.recipes.len(), 1);
        let recipe = &c.recipes[0];
        assert_eq!(recipe.name, "Iron Buckler");
        assert_eq!(recipe.profession.as_deref(), Some("Blacksmithing"));
        assert_eq!(recipe.reagents[0].name, "Khorium Ore");
        assert_eq!(recipe.reagents[0].count, 2);
        assert_eq!(recipe.reagents[0].icon, Some(133_828));
        assert_eq!(recipe.icon, Some(136_241));
        assert_eq!(c.recipes_scanned, Some(1_792_593_450));
        assert!(g.icon_ids().contains(&136_241), "recipe icon fed into the icon-prefetch set");
        assert!(g.icon_ids().contains(&133_828), "reagent icon fed into the icon-prefetch set");
        assert_eq!(recipe.reagents[1].icon, Some(0), "the model keeps the raw 0, same as every other field");
        assert!(!g.icon_ids().contains(&0), "a 0 icon (not-yet-loaded reagent) must never reach the prefetch set");
        let w = g.warband.expect("warband bank");
        assert_eq!(w.tabs[0].items[0].count, 200);
    }

    #[test]
    fn rested_xp_grows_while_logged_out_and_caps() {
        let c: Value = serde_json::json!({ "xpMax": 1000, "restXP": 100, "lastLogout": 0, "resting": true });
        assert_eq!(rested(&c, 0).now, 100);
        assert_eq!(rested(&c, 28_800).now, 150); // 8 hours in an inn = 5 % of a level
        assert_eq!(rested(&c, 100 * 28_800).now, 1500); // capped at 150 %
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let g = load_game_data(dir.path(), 0);
        assert!(g.characters.is_empty() && g.errors.is_empty());
    }
}
