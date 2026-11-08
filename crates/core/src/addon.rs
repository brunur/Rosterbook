//! The Rosterbook add-on, embedded in the app so it can be installed or
//! updated into any game flavor with one click.

use std::path::{Path, PathBuf};

pub const FOLDER: &str = "Rosterbook";

/// (file name, contents) of every file in the add-on folder.
pub const FILES: &[(&str, &str)] = &[
    ("Rosterbook.toc", include_str!("../../../addon/Rosterbook/Rosterbook.toc")),
    ("Rosterbook.lua", include_str!("../../../addon/Rosterbook/Rosterbook.lua")),
    ("LICENSE.txt", include_str!("../../../addon/Rosterbook/LICENSE.txt")),
];

/// Version of the add-on bundled with this build of the app.
pub fn bundled_version() -> String {
    toc_version(FILES[0].1).unwrap_or_else(|| "unknown".into())
}

fn toc_version(toc: &str) -> Option<String> {
    toc.lines()
        .find_map(|l| l.trim().strip_prefix("## Version:"))
        .map(|v| v.trim().to_string())
}

/// `## Interface:` numbers a TOC declares (e.g. 120105 for 12.1.5).
pub fn toc_interfaces(toc: &str) -> Vec<u32> {
    toc.lines()
        .find_map(|l| l.trim().strip_prefix("## Interface:"))
        .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_default()
}

/// Interface number for a game version: "1.60.1.70450" -> 16001.
pub fn interface_for(version: &str) -> Option<u32> {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().ok());
    let (major, minor, patch) = (parts.next()??, parts.next()??, parts.next()??);
    (minor < 100 && patch < 100).then(|| major * 10_000 + minor * 100 + patch)
}

/// The installed game version of a flavor folder, from `.flavor.info` (its
/// product name) and the install's `.build.info`.
pub fn game_version(flavor_dir: &Path) -> Option<String> {
    let info = std::fs::read_to_string(flavor_dir.join(".flavor.info")).ok()?;
    let product = info.lines().skip(1).map(str::trim).find(|l| !l.is_empty())?.to_string();
    let build_info = std::fs::read_to_string(flavor_dir.parent()?.join(".build.info")).ok()?;
    let mut lines = build_info.lines().filter(|l| !l.trim().is_empty());
    let names: Vec<&str> = lines.next()?.split('|').map(|h| h.split('!').next().unwrap_or("")).collect();
    let col = |n: &str| names.iter().position(|x| *x == n);
    let (p, v, a) = (col("Product")?, col("Version")?, col("Active"));
    lines.map(|l| l.split('|').collect::<Vec<_>>()).find_map(|row| {
        let active = a.and_then(|a| row.get(a)).is_none_or(|x| *x != "0");
        (active && row.get(p) == Some(&product.as_str())).then(|| row.get(v).map(|s| s.to_string())).flatten()
    })
}

/// Whether the add-on installed in this flavor is current: the bundled
/// version, and declared compatible with the installed game version (WoW turns
/// off add-ons that don't list the game's version as "out of date").
pub fn installed_is_current(flavor_dir: &Path) -> bool {
    let Ok(toc) = std::fs::read_to_string(addon_dir(flavor_dir).join("Rosterbook.toc")) else { return false };
    let version_ok = toc_version(&toc).as_deref() == Some(bundled_version().as_str());
    let interface_ok = match game_version(flavor_dir).as_deref().and_then(interface_for) {
        Some(i) => toc_interfaces(&toc).contains(&i),
        None => true,
    };
    version_ok && interface_ok
}

pub fn addon_dir(flavor_dir: &Path) -> PathBuf {
    flavor_dir.join("Interface").join("AddOns").join(FOLDER)
}

/// Version installed in a game flavor folder (e.g. `.../_retail_`), if any.
pub fn installed_version(flavor_dir: &Path) -> Option<String> {
    let toc = std::fs::read_to_string(addon_dir(flavor_dir).join("Rosterbook.toc")).ok()?;
    toc_version(&toc)
}

/// Installs or updates the add-on. Only ever writes inside `Interface/AddOns/Rosterbook`.
///
/// The installed copy also declares the game version actually installed in
/// that folder, so a game patch doesn't get it switched off as out of date.
pub fn install(flavor_dir: &Path) -> anyhow::Result<String> {
    anyhow::ensure!(
        flavor_dir.join("WTF").is_dir() || flavor_dir.join("Interface").is_dir() || flavor_dir.join(".flavor.info").is_file(),
        "{} doesn't look like a WoW game folder",
        flavor_dir.display()
    );
    let interface = game_version(flavor_dir).as_deref().and_then(interface_for);
    let dir = addon_dir(flavor_dir);
    std::fs::create_dir_all(&dir)?;
    for (name, contents) in FILES {
        let contents = if name.ends_with(".toc") { with_interface(contents, interface) } else { contents.to_string() };
        let tmp = dir.join(format!("{name}.tmp"));
        std::fs::write(&tmp, contents)?;
        std::fs::rename(&tmp, dir.join(name))?;
    }
    Ok(bundled_version())
}

/// Adds `interface` to the TOC's `## Interface:` list if it's missing.
fn with_interface(toc: &str, interface: Option<u32>) -> String {
    let Some(i) = interface.filter(|i| !toc_interfaces(toc).contains(i)) else { return toc.to_string() };
    toc.lines()
        .map(|l| if l.trim().starts_with("## Interface:") { format!("{}, {i}", l.trim_end()) } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_and_reports_version() {
        let tmp = tempfile::tempdir().unwrap();
        let flavor = tmp.path().join("_retail_");
        std::fs::create_dir_all(flavor.join("WTF")).unwrap();
        assert_eq!(installed_version(&flavor), None);
        let v = install(&flavor).unwrap();
        assert_eq!(installed_version(&flavor), Some(v.clone()));
        assert_eq!(v, bundled_version());
        assert!(addon_dir(&flavor).join("Rosterbook.lua").is_file());
    }

    /// A game folder at a version the bundled TOC doesn't list (e.g. after a patch).
    fn patched_flavor(dir: &Path, version: &str) -> PathBuf {
        let flavor = dir.join("_forever_");
        std::fs::create_dir_all(&flavor).unwrap();
        std::fs::write(flavor.join(".flavor.info"), "Product Flavor!STRING:0\nwow_forever\n").unwrap();
        std::fs::write(
            dir.join(".build.info"),
            format!("Branch!STRING:0|Active!DEC:1|Version!STRING:0|Product!STRING:0\nus|1|12.1.5.70448|wow\nus|1|{version}|wow_forever\n"),
        )
        .unwrap();
        flavor
    }

    #[test]
    fn reads_versions_and_interfaces() {
        assert_eq!(interface_for("1.60.1.70450"), Some(16001));
        assert_eq!(interface_for("12.1.5.70448"), Some(120105));
        assert_eq!(interface_for("12.1"), None);
        assert!(toc_interfaces(FILES[0].1).contains(&120105));
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(game_version(&patched_flavor(tmp.path(), "1.61.0.71000")).as_deref(), Some("1.61.0.71000"));
    }

    #[test]
    fn declares_the_installed_game_version() {
        let tmp = tempfile::tempdir().unwrap();
        // A brand-new game folder (no WTF yet) at a version the add-on doesn't list.
        let flavor = patched_flavor(tmp.path(), "1.61.0.71000");
        assert!(!installed_is_current(&flavor));
        install(&flavor).unwrap();
        let toc = std::fs::read_to_string(addon_dir(&flavor).join("Rosterbook.toc")).unwrap();
        assert!(toc_interfaces(&toc).contains(&16100));
        assert!(toc_interfaces(&toc).contains(&120105), "keeps the versions it already listed");
        assert!(installed_is_current(&flavor));

        // The game patches: the add-on needs updating, and reinstalling fixes it.
        patched_flavor(tmp.path(), "1.61.1.71200");
        assert!(!installed_is_current(&flavor));
        install(&flavor).unwrap();
        assert!(installed_is_current(&flavor));
    }

    #[test]
    fn refuses_folders_that_are_not_wow() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(install(tmp.path()).is_err());
    }
}
