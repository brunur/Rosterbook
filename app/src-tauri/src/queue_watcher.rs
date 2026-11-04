//! Starts and stops the separate `alt-ledger-queue-watcher` process based on
//! its own on-disk config (`alt_ledger_queue::WatcherConfig`).
//!
//! There's no region-picker UI yet to write that config (a later step), so
//! today this only makes the app honor a config file someone else wrote:
//! start the watcher once it's enabled with both regions set, stop it the
//! moment it isn't. Checked once at launch and every `CHECK_EVERY` after,
//! so a config file edited while the app is running still takes effect.

use alt_ledger_queue::config::CONFIG_FILE;
use alt_ledger_queue::WatcherConfig;
use std::path::PathBuf;
use std::process::Child;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const CHECK_EVERY: Duration = Duration::from_secs(30);

fn config_path() -> PathBuf {
    alt_ledger_queue::config::default_dir().join(CONFIG_FILE)
}

/// The watcher binary, expected right beside this one (true in dev, where
/// cargo puts every workspace binary in the same target dir; a release
/// build needs its own bundling step to put it there too — not done yet).
fn binary_path() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let name = if cfg!(windows) { "alt-ledger-queue-watcher.exe" } else { "alt-ledger-queue-watcher" };
    Some(dir.join(name))
}

fn should_run() -> bool {
    WatcherConfig::load(&config_path()).map(|c| c.is_ready()).unwrap_or(false)
}

fn is_running(child: &mut Option<Child>) -> bool {
    match child {
        Some(c) => matches!(c.try_wait(), Ok(None)),
        None => false,
    }
}

fn spawn() -> Option<Child> {
    let path = binary_path()?;
    if !path.exists() {
        eprintln!("queue watcher binary not found at {}", path.display());
        return None;
    }
    std::process::Command::new(path).spawn().map_err(|e| eprintln!("couldn't start the queue watcher: {e}")).ok()
}

pub fn stop(child: &mut Option<Child>) {
    if let Some(mut c) = child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// Starts the watcher if its config is ready and it isn't running yet,
/// stops it if it's running but no longer should be.
fn reconcile(child: &mut Option<Child>) {
    match (should_run(), is_running(child)) {
        (true, false) => *child = spawn(),
        (false, true) => stop(child),
        _ => {}
    }
}

pub fn spawn_watch_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            if let Some(shared) = app.try_state::<crate::Shared>() {
                reconcile(&mut shared.queue_watcher.lock().unwrap());
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}
