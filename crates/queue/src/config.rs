//! On-disk watcher configuration: where to capture from, how often, and
//! whether it's turned on at all.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "queue-watcher.json";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureRegion {
    /// Index into `xcap::Monitor::all()`, not a persistent monitor id —
    /// re-pick the region if the monitor layout changes.
    pub monitor_index: usize,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatcherConfig {
    /// The desktop app is expected not to launch this process at all when
    /// the user has queue tracking off; this is a defensive second switch
    /// in case it runs anyway (e.g. left over from a previous session).
    pub enabled: bool,
    pub position_region: Option<CaptureRegion>,
    pub time_region: Option<CaptureRegion>,
    pub active_interval_secs: u64,
    pub idle_interval_secs: u64,
    pub retention_days: u32,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            position_region: None,
            time_region: None,
            active_interval_secs: 5,
            idle_interval_secs: 30,
            retention_days: 30,
        }
    }
}

impl WatcherConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(src) => Ok(serde_json::from_str(&src)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    // Not called yet: the desktop app will use this once it has a capture-
    // region picker to write (step 6 of the handoff doc).
    #[allow(dead_code)]
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Both regions set and the watcher turned on — anything else and
    /// there's nothing sensible to capture.
    pub fn is_ready(&self) -> bool {
        self.enabled && self.position_region.is_some() && self.time_region.is_some()
    }
}

/// `<config dir>/RosterBook/queue`, alongside the main app's own settings.
pub fn default_dir() -> PathBuf {
    alt_ledger_core::settings::default_dir().join("queue")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_not_ready() {
        assert!(!WatcherConfig::default().is_ready());
    }

    #[test]
    fn ready_once_enabled_with_both_regions() {
        let region = CaptureRegion { monitor_index: 0, x: 0, y: 0, width: 10, height: 10 };
        let cfg = WatcherConfig {
            enabled: true,
            position_region: Some(region),
            time_region: Some(region),
            ..WatcherConfig::default()
        };
        assert!(cfg.is_ready());
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue-watcher.json");
        let region = CaptureRegion { monitor_index: 1, x: 5, y: 6, width: 200, height: 40 };
        let cfg = WatcherConfig {
            enabled: true,
            position_region: Some(region),
            time_region: Some(region),
            ..WatcherConfig::default()
        };
        cfg.save(&path).unwrap();
        let loaded = WatcherConfig::load(&path).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn missing_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = WatcherConfig::load(&dir.path().join("nope.json")).unwrap();
        assert_eq!(loaded, WatcherConfig::default());
    }
}
