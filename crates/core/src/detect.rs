//! Finds World of Warcraft installs, game flavors (Retail, Classic, …) and
//! accounts, including installs inside Wine/Proton prefixes on Linux.

use crate::addon;
use crate::model::DATA_FILE;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Install {
    pub root: PathBuf,
    pub flavors: Vec<Flavor>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Flavor {
    /// Folder name, e.g. `_retail_`.
    pub folder: String,
    pub name: String,
    pub path: PathBuf,
    /// Version of the Rosterbook add-on installed in this flavor, if any.
    pub addon_version: Option<String>,
    /// The add-on is installed but older than the app's copy, or doesn't list
    /// the installed game version (WoW would switch it off as out of date).
    pub addon_outdated: bool,
    /// Installed game version, e.g. `1.60.1.70450`.
    pub game_version: Option<String>,
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub name: String,
    pub saved_variables_dir: PathBuf,
    /// The add-on has saved data for this account.
    pub has_data: bool,
    /// When that data was last written (unix seconds).
    pub last_saved: Option<i64>,
    /// Stable ID used for this game in settings.
    pub game_id: String,
}

/// Human name for a flavor folder.
pub fn flavor_name(folder: &str, flavor_dir: &Path) -> String {
    if let Ok(info) = std::fs::read_to_string(flavor_dir.join(".flavor.info")) {
        if info.to_lowercase().contains("forever") {
            return "Forever".into();
        }
    }
    match folder {
        "_retail_" => "Retail".into(),
        "_ptr_" | "_xptr_" => "Retail PTR".into(),
        "_beta_" => "Retail Beta".into(),
        "_classic_" => "Classic".into(),
        "_classic_era_" => "Classic Era".into(),
        "_classic_ptr_" | "_classic_era_ptr_" => "Classic PTR".into(),
        "_classic_beta_" => "Forever".into(),
        "_anniversary_" => "Anniversary".into(),
        "_forever_" => "Forever".into(),
        other => {
            let t = other.trim_matches('_').replace('_', " ");
            let mut c = t.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => other.to_string(),
            }
        }
    }
}

/// Stable short ID for a SavedVariables folder (FNV-1a of the path).
pub fn game_id(saved_variables_dir: &Path) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in saved_variables_dir.to_string_lossy().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("g{:012x}", h & 0xffff_ffff_ffff)
}

/// Folders that may contain a `World of Warcraft` install.
pub fn candidate_roots(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let wow_in = |base: &Path| -> Vec<PathBuf> {
        ["Program Files (x86)", "Program Files", "Games", ""]
            .iter()
            .map(|p| if p.is_empty() { base.join("World of Warcraft") } else { base.join(p).join("World of Warcraft") })
            .collect()
    };

    for p in extra {
        out.push(p.clone());
        out.extend(wow_in(p));
        out.extend(wow_in(&p.join("drive_c")));
    }

    #[cfg(windows)]
    for drive in b'C'..=b'Z' {
        let base = PathBuf::from(format!("{}:\\", drive as char));
        if base.exists() {
            out.extend(wow_in(&base));
            out.push(base.join("Blizzard").join("World of Warcraft"));
            out.push(base.join("Battle.net").join("World of Warcraft"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        out.push(PathBuf::from("/Applications/World of Warcraft"));
        if let Some(home) = dirs::home_dir() {
            out.push(home.join("Applications/World of Warcraft"));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(home) = dirs::home_dir() {
        for prefix in wine_prefixes(&home) {
            out.extend(wow_in(&prefix.join("drive_c")));
        }
    }

    let mut seen = BTreeSet::new();
    out.into_iter()
        .filter(|p| p.join("WTF").exists() || has_flavor_dirs(p))
        .filter(|p| seen.insert(p.canonicalize().unwrap_or_else(|_| p.clone())))
        .collect()
}

/// Wine/Proton/Lutris/Bottles/Heroic/Faugus prefixes under the home folder.
#[cfg(all(unix, not(target_os = "macos")))]
fn wine_prefixes(home: &Path) -> Vec<PathBuf> {
    let mut out = vec![home.join(".wine")];
    let children = |dir: PathBuf| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect())
            .unwrap_or_default()
    };
    for dir in [
        home.join("Games"),
        home.join("Faugus"),
        home.join(".local/share/lutris/prefixes"),
        home.join(".local/share/bottles/bottles"),
        home.join(".var/app/com.usebottles.bottles/data/bottles/bottles"),
        home.join("Games/Heroic/Prefixes"),
        home.join("Games/Heroic/Prefixes/default"),
    ] {
        for child in children(dir) {
            out.extend(children(child.clone())); // e.g. ~/Games/battlenet/<prefix>
            out.push(child);
        }
    }
    for steam in [home.join(".steam/steam/steamapps/compatdata"), home.join(".local/share/Steam/steamapps/compatdata")] {
        for app in children(steam) {
            out.push(app.join("pfx"));
        }
    }
    out.into_iter().filter(|p| p.join("drive_c").is_dir()).collect()
}

fn has_flavor_dirs(root: &Path) -> bool {
    flavor_dirs(root).next().is_some()
}

fn flavor_dirs(root: &Path) -> impl Iterator<Item = (String, PathBuf)> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let path = e.path();
            // A game folder that has run has WTF; a freshly installed one at least has .flavor.info.
            let is_game = path.join("WTF").is_dir() || path.join(".flavor.info").is_file();
            (name.starts_with('_') && name.ends_with('_') && name.len() > 2 && is_game)
                .then_some((name, path))
        })
}

/// Reads one install folder.
pub fn inspect(root: &Path) -> Install {
    let mut flavors: Vec<Flavor> = flavor_dirs(root)
        .map(|(folder, path)| {
            let mut accounts: Vec<Account> = std::fs::read_dir(path.join("WTF").join("Account"))
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name() != "SavedVariables" && e.path().join("SavedVariables").is_dir())
                .map(|e| {
                    let sv = e.path().join("SavedVariables");
                    let data = sv.join(DATA_FILE);
                    let last_saved = std::fs::metadata(&data)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64);
                    Account {
                        name: e.file_name().to_string_lossy().to_string(),
                        game_id: game_id(&sv),
                        has_data: data.is_file(),
                        last_saved,
                        saved_variables_dir: sv,
                    }
                })
                .collect();
            // Accounts with data first, most recently saved first.
            accounts.sort_by(|a, b| b.last_saved.cmp(&a.last_saved).then(a.name.cmp(&b.name)));
            Flavor {
                name: flavor_name(&folder, &path),
                addon_version: addon::installed_version(&path),
                addon_outdated: addon::installed_version(&path).is_some() && !addon::installed_is_current(&path),
                game_version: addon::game_version(&path),
                folder,
                path,
                accounts,
            }
        })
        .collect();
    flavors.sort_by(|a, b| a.name.cmp(&b.name));
    Install { root: root.to_path_buf(), flavors }
}

pub fn detect(extra: &[PathBuf]) -> Vec<Install> {
    candidate_roots(extra).iter().map(|r| inspect(r)).filter(|i| !i.flavors.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_flavors_and_accounts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("World of Warcraft");
        let sv = root.join("_retail_/WTF/Account/MYACCOUNT/SavedVariables");
        std::fs::create_dir_all(&sv).unwrap();
        std::fs::create_dir_all(root.join("_retail_/WTF/Account/SavedVariables")).unwrap();
        std::fs::write(sv.join(DATA_FILE), "RosterbookDB = {}").unwrap();
        std::fs::create_dir_all(root.join("_classic_era_/WTF/Account/OTHER/SavedVariables")).unwrap();

        let found = detect(&[tmp.path().to_path_buf()]);
        assert_eq!(found.len(), 1);
        let names: Vec<_> = found[0].flavors.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Classic Era", "Retail"]);
        let retail = &found[0].flavors[1];
        assert_eq!(retail.accounts.len(), 1, "the shared SavedVariables folder is not an account");
        assert!(retail.accounts[0].has_data);
        assert_eq!(retail.accounts[0].game_id, game_id(&sv));
    }

    #[test]
    fn names_unknown_flavors_readably() {
        assert_eq!(flavor_name("_retail_", Path::new("/nope")), "Retail");
        assert_eq!(flavor_name("_new_thing_", Path::new("/nope")), "New thing");
    }
}
