//! Watches each game's SavedVariables folder and reloads that game when WoW
//! writes `Rosterbook.lua`, or captures a price snapshot when it writes a
//! supported AH add-on's file (e.g. `Auctionator.lua`) — on logout or
//! /reload. WoW rewrites many files at once, so changes are debounced.

use crate::AppState;
use alt_ledger_core::{ah, model::DATA_FILE};
use notify::{RecursiveMode, Watcher as _};
use std::path::PathBuf;
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    GameData,
    Prices,
}

pub struct Watcher {
    _inner: notify::RecommendedWatcher,
    _task: tokio::task::JoinHandle<()>,
}

impl Watcher {
    pub fn start(state: AppState, dirs: Vec<(String, PathBuf)>) -> notify::Result<Self> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(String, Kind)>();
        let lookup = dirs.clone();
        let mut inner = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            for path in &event.paths {
                // WoW writes via a temp file + rename, so match on the final name.
                let Some(name) = path.file_name() else { continue };
                let kind = if name == DATA_FILE {
                    Some(Kind::GameData)
                } else if ah::Source::ALL.iter().any(|s| name == s.data_file()) {
                    Some(Kind::Prices)
                } else {
                    None
                };
                if let Some(kind) = kind {
                    if let Some((id, _)) = lookup.iter().find(|(_, d)| path.parent() == Some(d.as_path())) {
                        let _ = tx.send((id.clone(), kind));
                    }
                }
            }
        })?;
        for (_, dir) in &dirs {
            if dir.is_dir() {
                if let Err(e) = inner.watch(dir, RecursiveMode::NonRecursive) {
                    eprintln!("Can't watch {}: {e}", dir.display());
                }
            }
        }

        let task = tokio::spawn(async move {
            let mut pending: std::collections::HashSet<(String, Kind)> = Default::default();
            loop {
                let first = if pending.is_empty() {
                    match rx.recv().await {
                        Some(msg) => Some(msg),
                        None => break,
                    }
                } else {
                    match tokio::time::timeout(DEBOUNCE, rx.recv()).await {
                        Ok(Some(msg)) => Some(msg),
                        Ok(None) => break,
                        Err(_) => None, // quiet period over: reload
                    }
                };
                match first {
                    Some(msg) => {
                        pending.insert(msg);
                    }
                    None => {
                        for (id, kind) in pending.drain() {
                            match kind {
                                Kind::GameData => state.reload_game(&id).await,
                                Kind::Prices => state.capture_prices(&id).await,
                            }
                            // Either kind of change can change the ranking.
                            state.sync_opportunities_addon(&id).await;
                        }
                    }
                }
            }
        });

        Ok(Self { _inner: inner, _task: task })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self._task.abort();
    }
}
