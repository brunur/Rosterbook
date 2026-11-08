//! Watches each game's SavedVariables folder and reloads that game when WoW
//! writes `Rosterbook.lua` (on logout or /reload). WoW rewrites many files at
//! once, so changes are debounced.

use crate::AppState;
use alt_ledger_core::model::DATA_FILE;
use notify::{RecursiveMode, Watcher as _};
use std::path::PathBuf;
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(1500);

pub struct Watcher {
    _inner: notify::RecommendedWatcher,
    _task: tokio::task::JoinHandle<()>,
}

impl Watcher {
    pub fn start(state: AppState, dirs: Vec<(String, PathBuf)>) -> notify::Result<Self> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let lookup = dirs.clone();
        let mut inner = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            for path in &event.paths {
                // WoW writes via a temp file + rename, so match on the final name.
                if path.file_name().map(|n| n == DATA_FILE).unwrap_or(false) {
                    if let Some((id, _)) = lookup.iter().find(|(_, d)| path.parent() == Some(d.as_path())) {
                        let _ = tx.send(id.clone());
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
            let mut pending: std::collections::HashSet<String> = Default::default();
            loop {
                let first = if pending.is_empty() {
                    match rx.recv().await {
                        Some(id) => Some(id),
                        None => break,
                    }
                } else {
                    match tokio::time::timeout(DEBOUNCE, rx.recv()).await {
                        Ok(Some(id)) => Some(id),
                        Ok(None) => break,
                        Err(_) => None, // quiet period over: reload
                    }
                };
                match first {
                    Some(id) => {
                        pending.insert(id);
                    }
                    None => {
                        for id in pending.drain() {
                            state.reload_game(&id).await;
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
