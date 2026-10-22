//! Checks icon extraction against a real WoW install.
//!
//!   cargo run --release -p alt-ledger-casc --example probe -- OUT_DIR FLAVOR_DIR... [--ids 1,2,3]
//!
//! FLAVOR_DIR is a game folder such as `.../World of Warcraft/_retail_`.
//! Writes `<product>-<fileID>.png` files and `report.txt` to OUT_DIR.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use alt_ledger_casc::{blp_to_png, Install};

/// Common icons present in every version: question mark, coins, hearthstone,
/// linen cloth, copper ore, peacebloom, a sword, minor healing potion.
const DEFAULT_IDS: &[u32] = &[134400, 133784, 133785, 134414, 132889, 134566, 133939, 135274, 134830];

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut ids: BTreeSet<u32> = DEFAULT_IDS.iter().copied().collect();
    if let Some(i) = args.iter().position(|a| a == "--ids") {
        let list = args.get(i + 1).cloned().unwrap_or_default();
        ids.extend(list.split(',').filter_map(|s| s.trim().parse::<u32>().ok()));
        args.drain(i..(i + 2).min(args.len()));
    }
    if args.len() < 2 {
        eprintln!("usage: probe OUT_DIR FLAVOR_DIR... [--ids 1,2,3]");
        std::process::exit(2);
    }
    let out = PathBuf::from(&args[0]);
    std::fs::create_dir_all(&out).expect("can't create output folder");

    let mut report = String::new();
    for flavor in &args[1..] {
        let _ = writeln!(report, "== {flavor}");
        let started = Instant::now();
        let install = match Install::open_flavor(flavor.as_ref()) {
            Ok(i) => i,
            Err(e) => {
                let _ = writeln!(report, "   open failed: {e}\n");
                continue;
            }
        };
        let _ = writeln!(report, "   product {} version {} build {}", install.product, install.version, install.build_key);
        match install.read_files(&ids) {
            Ok(files) => {
                let _ = writeln!(report, "   read {} of {} files in {:.1}s", files.len(), ids.len(), started.elapsed().as_secs_f32());
                for id in &ids {
                    match files.get(id) {
                        None => {
                            let _ = writeln!(report, "   {id}: not found");
                        }
                        Some(data) => match blp_to_png(data) {
                            Ok(png) => {
                                let name = format!("{}-{id}.png", install.product);
                                std::fs::write(out.join(&name), &png).unwrap();
                                let _ = writeln!(report, "   {id}: ok ({} byte BLP, header {:?}) -> {name}", data.len(), blp_kind(data));
                            }
                            Err(e) => {
                                let _ = writeln!(report, "   {id}: {} bytes, not decoded: {e}, starts {:02x?}", data.len(), &data[..data.len().min(24)]);
                            }
                        },
                    }
                }
            }
            Err(e) => {
                let _ = writeln!(report, "   read failed after {:.1}s: {e}", started.elapsed().as_secs_f32());
            }
        }
        let _ = writeln!(report, "   peak memory so far: {}\n", peak_memory());
    }
    print!("{report}");
    std::fs::write(out.join("report.txt"), report).unwrap();
}

fn blp_kind(d: &[u8]) -> (u8, u8, u8) {
    if d.len() > 11 { (d[8], d[9], d[10]) } else { (0, 0, 0) }
}

fn peak_memory() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmHWM")).map(|l| l.split_whitespace().skip(1).collect::<Vec<_>>().join(" ")))
        .unwrap_or_else(|| "n/a".into())
}
