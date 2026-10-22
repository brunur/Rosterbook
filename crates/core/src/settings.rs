//! App settings, stored as JSON in the user's config folder
//! (`~/.config/AltLedger`, `%APPDATA%\AltLedger`, `~/Library/Application Support/AltLedger`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Games shown in the app: one entry per game flavor + WoW account.
    pub games: Vec<GameConfig>,
    pub port: u16,
    /// Let other devices on the home network connect (after pairing).
    pub lan: bool,
    /// "system", "light" or "dark".
    pub theme: String,
    /// Per-game view preferences, keyed by `GameConfig::id`.
    pub views: HashMap<String, ViewPrefs>,
    /// Names for factions the add-on couldn't name, keyed by faction ID.
    pub faction_names: HashMap<String, String>,
    /// Devices allowed to connect over the network.
    pub devices: Vec<Device>,
    /// Extra folders to search for WoW installs.
    pub extra_install_paths: Vec<PathBuf>,
    /// Show item pictures read from the local WoW install instead of Alt
    /// Ledger's own drawings. Off by default.
    pub game_icons: bool,
    /// Games (by `GameConfig::id`) the owner chose not to add when offered.
    pub dismissed_games: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            games: Vec::new(),
            port: 8787,
            lan: true,
            theme: "system".into(),
            views: HashMap::new(),
            faction_names: HashMap::new(),
            devices: Vec::new(),
            extra_install_paths: Vec::new(),
            game_icons: false,
            dismissed_games: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameConfig {
    pub id: String,
    pub name: String,
    pub saved_variables_dir: PathBuf,
    /// The game flavor folder (e.g. `.../_retail_`), used for add-on installs.
    #[serde(default)]
    pub flavor_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewPrefs {
    /// "custom", "level", "name", "gold" or "recent".
    pub sort: String,
    /// Character keys in the user's custom order.
    pub order: Vec<String>,
    pub pinned: Vec<String>,
    pub hidden: Vec<String>,
    pub filters: Filters,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Filters {
    pub realms: Vec<String>,
    pub factions: Vec<String>,
    pub races: Vec<String>,
    pub classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    /// SHA-256 of the device's secret token (the token itself is never stored).
    pub token_hash: String,
    pub created: i64,
    pub last_seen: i64,
}

pub fn default_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("AltLedger")
}

/// Where rebuildable files (such as item pictures) are kept; not backed up or roamed.
pub fn default_cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(default_dir).join("AltLedger")
}

impl Settings {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes atomically (temp file + rename) so a crash never leaves a half-written file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn view(&self, game_id: &str) -> ViewPrefs {
        let mut v = self.views.get(game_id).cloned().unwrap_or_default();
        if v.sort.is_empty() {
            v.sort = "custom".into();
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_tolerates_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(Settings::load(&path).unwrap().port, 8787);

        let mut s = Settings { theme: "dark".into(), ..Default::default() };
        s.views.insert("g1".into(), ViewPrefs { sort: "level".into(), order: vec!["a".into()], ..Default::default() });
        s.save(&path).unwrap();
        let back = Settings::load(&path).unwrap();
        assert_eq!(back.theme, "dark");
        assert_eq!(back.view("g1").sort, "level");
        assert_eq!(back.view("missing").sort, "custom");

        std::fs::write(&path, r#"{"theme":"light"}"#).unwrap();
        let partial = Settings::load(&path).unwrap();
        assert_eq!(partial.theme, "light");
        assert_eq!(partial.port, 8787);
    }
}
