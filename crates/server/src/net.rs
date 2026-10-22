//! Network helpers: which address other devices should use, and QR codes.

use qrcode::render::{svg, unicode};
use qrcode::QrCode;
use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Clone, Serialize)]
pub struct LanAddress {
    pub adapter: String,
    pub ip: IpAddr,
    pub likely: bool,
}

/// Virtual adapters (VMware, Docker, VPNs …) that other devices can't usually reach.
fn is_virtual(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["lo", "vmnet", "vmbr", "docker", "br-", "virbr", "veth", "vboxnet", "vnet", "tun", "tap", "wg",
     "zt", "tailscale", "utun", "cni", "flannel", "podman", "lxc", "lxd", "vethernet", "vmware", "virtualbox", "hyper-v"]
        .iter()
        .any(|p| n.starts_with(p) || n.contains(&format!("({p}")))
}

fn is_private(ip: &Ipv4Addr) -> bool {
    ip.is_private()
}

/// IPv4 addresses of this machine, best guess for "home network" first.
pub fn lan_addresses() -> Vec<LanAddress> {
    let mut found: Vec<(i32, LanAddress)> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(v4) if !v4.is_link_local() => Some((i.name.clone(), v4)),
            _ => None,
        })
        .map(|(name, v4)| {
            let mut score = 0;
            if !is_virtual(&name) {
                score += 10;
            }
            let n = name.to_ascii_lowercase();
            if n.starts_with("en") || n.starts_with("eth") || n.starts_with("wl") || n.contains("wi-fi") || n.contains("ethernet") {
                score += 5;
            }
            if v4.octets()[0] == 192 && v4.octets()[1] == 168 {
                score += 3;
            } else if is_private(&v4) {
                score += 1;
            }
            (score, LanAddress { adapter: name, ip: IpAddr::V4(v4), likely: false })
        })
        .collect();
    found.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    let mut out: Vec<LanAddress> = found.into_iter().map(|(_, a)| a).collect();
    if let Some(first) = out.first_mut() {
        first.likely = !is_virtual(&first.adapter);
    }
    out
}

pub fn qr_svg(text: &str) -> String {
    match QrCode::new(text.as_bytes()) {
        Ok(code) => code
            .render::<svg::Color>()
            .min_dimensions(240, 240)
            .quiet_zone(true)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
        Err(_) => String::new(),
    }
}

/// QR code drawn with half-block characters, for the terminal.
pub fn qr_terminal(text: &str) -> String {
    match QrCode::new(text.as_bytes()) {
        Ok(code) => code
            .render::<unicode::Dense1x2>()
            .dark_color(unicode::Dense1x2::Light)
            .light_color(unicode::Dense1x2::Dark)
            .quiet_zone(true)
            .build(),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_adapters_are_recognised() {
        assert!(is_virtual("vmnet8"));
        assert!(is_virtual("docker0"));
        assert!(is_virtual("vEthernet (WSL)"));
        assert!(!is_virtual("enp5s0"));
        assert!(!is_virtual("wlan0"));
        assert!(!is_virtual("Wi-Fi"));
    }

    #[test]
    fn qr_codes_render() {
        assert!(qr_svg("http://192.168.1.5:8787/pair?code=123456").starts_with("<?xml"));
        assert!(!qr_terminal("http://x").is_empty());
    }
}
