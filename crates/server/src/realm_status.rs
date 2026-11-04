//! TCP reachability check against Blizzard's regional login servers. One
//! login server per region serves retail, classic and classic era alike —
//! there's no way to tell those apart over a plain TCP connect, so this only
//! ever reports per-region up/down and latency.

use serde::Serialize;
use std::net::ToSocketAddrs;
use std::sync::RwLock;
use std::time::{Duration, Instant};

const LOGIN_PORT: u16 = 3724;
const PING_TIMEOUT: Duration = Duration::from_secs(3);
pub const CHECK_EVERY: Duration = Duration::from_secs(60);

const REGIONS: &[(&str, &str)] = &[
    ("US", "us.logon.worldofwarcraft.com"),
    ("EU", "eu.logon.worldofwarcraft.com"),
    ("KR", "kr.logon.worldofwarcraft.com"),
    ("TW", "tw.logon.worldofwarcraft.com"),
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionStatus {
    pub region: &'static str,
    pub host: &'static str,
    pub up: bool,
    pub latency_ms: Option<u64>,
    pub checked_at: i64,
}

pub struct RealmStatus {
    cache: RwLock<Vec<RegionStatus>>,
}

impl RealmStatus {
    pub fn new() -> Self {
        Self { cache: RwLock::new(Vec::new()) }
    }

    pub fn snapshot(&self) -> Vec<RegionStatus> {
        self.cache.read().unwrap().clone()
    }

    /// Pings every region, blocking the calling thread — call this from
    /// `spawn_blocking`.
    fn check_all() -> Vec<RegionStatus> {
        REGIONS.iter().map(|&(region, host)| ping(region, host)).collect()
    }

    pub async fn refresh(&self) {
        let found = tokio::task::spawn_blocking(Self::check_all).await.unwrap_or_default();
        *self.cache.write().unwrap() = found;
    }
}

fn ping(region: &'static str, host: &'static str) -> RegionStatus {
    let checked_at = crate::now();
    let addr = (host, LOGIN_PORT).to_socket_addrs().ok().and_then(|mut a| a.next());
    let Some(addr) = addr else {
        return RegionStatus { region, host, up: false, latency_ms: None, checked_at };
    };
    let start = Instant::now();
    match std::net::TcpStream::connect_timeout(&addr, PING_TIMEOUT) {
        Ok(_) => RegionStatus { region, host, up: true, latency_ms: Some(start.elapsed().as_millis() as u64), checked_at },
        Err(_) => RegionStatus { region, host, up: false, latency_ms: None, checked_at },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_host_is_down_not_an_error() {
        let s = ping("US", "this-host-does-not-exist.invalid");
        assert!(!s.up);
        assert_eq!(s.latency_ms, None);
    }

    #[tokio::test]
    async fn refresh_fills_one_entry_per_region() {
        let rs = RealmStatus::new();
        assert!(rs.snapshot().is_empty());
        rs.refresh().await;
        assert_eq!(rs.snapshot().len(), REGIONS.len());
    }
}
