//! Item pictures read from the local WoW install (optional, off by default).
//!
//! When enabled, after a game's data loads we work out which icon file IDs its
//! characters use, extract only the ones not already cached, and store them as
//! PNGs under `<cache>/icons/<product>-<build>/`. A new game patch gets a fresh
//! folder and the old one is removed. IDs the build doesn't have are noted so
//! they aren't looked up again. Extraction runs one game at a time on a
//! blocking thread and streams the game's tables, so it stays light on memory.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use alt_ledger_casc::{blp_to_png, Install};

const MISSING_FILE: &str = "missing.txt";

#[derive(Debug, Clone, Default)]
pub struct GameIcons {
    dir: Option<PathBuf>,
    /// Icon IDs with a cached picture.
    pub ready: BTreeSet<u32>,
    /// Game version the pictures came from.
    pub version: Option<String>,
    pub error: Option<String>,
    pub working: bool,
}

pub struct IconCache {
    root: PathBuf,
    games: Mutex<HashMap<String, GameIcons>>,
    work: tokio::sync::Mutex<()>,
}

impl IconCache {
    pub fn new(cache_dir: &Path) -> Self {
        IconCache { root: cache_dir.join("icons"), games: Mutex::new(HashMap::new()), work: tokio::sync::Mutex::new(()) }
    }

    pub fn status(&self, game: &str) -> GameIcons {
        self.games.lock().unwrap().get(game).cloned().unwrap_or_default()
    }

    pub fn forget_others(&self, keep: &[String]) {
        self.games.lock().unwrap().retain(|k, _| keep.contains(k));
    }

    /// Path of a cached picture, if this game has one for `id`.
    pub fn file(&self, game: &str, id: u32) -> Option<PathBuf> {
        let games = self.games.lock().unwrap();
        let g = games.get(game)?;
        g.ready.contains(&id).then(|| g.dir.as_ref().map(|d| d.join(format!("{id}.png")))).flatten()
    }

    /// Brings `game`'s cache up to date with the icons it uses. Waits for any
    /// other extraction to finish first.
    pub async fn refresh(&self, game: &str, flavor_dir: Option<PathBuf>, wanted: BTreeSet<u32>) {
        let _turn = self.work.lock().await;
        self.games.lock().unwrap().entry(game.to_string()).or_default().working = true;
        let root = self.root.clone();
        let result = match flavor_dir {
            None => Err("Choose this game again in Settings → Games so Rosterbook knows where it's installed.".to_string()),
            Some(flavor) => tokio::task::spawn_blocking(move || update(&root, &flavor, &wanted))
                .await
                .unwrap_or_else(|e| Err(format!("Reading pictures stopped unexpectedly: {e}"))),
        };
        let mut games = self.games.lock().unwrap();
        let entry = games.entry(game.to_string()).or_default();
        entry.working = false;
        match result {
            Ok((dir, ready, version)) => {
                *entry = GameIcons { dir: Some(dir), ready, version: Some(version), error: None, working: false };
            }
            Err(e) => {
                eprintln!("item pictures for {game}: {e}");
                entry.error = Some(e);
            }
        }
    }
}

/// Returns the cache folder, the IDs it holds, and the game version.
fn update(root: &Path, flavor: &Path, wanted: &BTreeSet<u32>) -> Result<(PathBuf, BTreeSet<u32>, String), String> {
    let install = Install::open_flavor(flavor).map_err(|e| {
        eprintln!("item pictures: {e}");
        match e {
            alt_ledger_casc::Error::Install(_) => {
                "Couldn't find this game's files. If you moved or reinstalled it, choose it again above.".to_string()
            }
            other => format!("Couldn't read this game's files ({other})."),
        }
    })?;
    let product: String = install.product.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    let folder = format!("{product}-{}", install.build_key);
    let dir = root.join(&folder);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;

    // Drop pictures from older builds of this product.
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name != folder && name.starts_with(&format!("{product}-")) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }

    let mut have: BTreeSet<u32> = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(id) = name.strip_suffix(".png").and_then(|s| s.parse().ok()) {
                have.insert(id);
            }
        }
    }
    let mut missing: BTreeSet<u32> = std::fs::read_to_string(dir.join(MISSING_FILE))
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();

    let needed: BTreeSet<u32> = wanted.iter().filter(|id| !have.contains(id) && !missing.contains(id)).copied().collect();
    if !needed.is_empty() {
        let files = install.read_files(&needed).map_err(|e| format!("Couldn't read this game's files ({e})."))?;
        for id in &needed {
            let png = files.get(id).and_then(|blp| blp_to_png(blp).ok());
            match png {
                Some(png) => {
                    let tmp = dir.join(format!("{id}.tmp"));
                    std::fs::write(&tmp, &png)
                        .and_then(|_| std::fs::rename(&tmp, dir.join(format!("{id}.png"))))
                        .map_err(|e| format!("Can't save pictures in {}: {e}", dir.display()))?;
                    have.insert(*id);
                }
                None => {
                    missing.insert(*id);
                }
            }
        }
        let list: Vec<String> = missing.iter().map(u32::to_string).collect();
        let _ = std::fs::write(dir.join(MISSING_FILE), list.join("\n"));
    }
    Ok((dir, have, install.version))
}
