//! Headless Rosterbook server, for running without the desktop app
//! (development, or a PC you only reach from other devices).
//!
//!   alt-ledger-server [--settings FILE] [--port N] [--ui DIR]

use alt_ledger_server::{net, start, Options};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut settings_path = alt_ledger_core::settings::default_dir().join("settings.json");
    let mut port = None;
    let mut ui_dir = None;
    let mut queue_dir = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--settings" => settings_path = PathBuf::from(args.next().expect("--settings needs a file")),
            "--port" => port = Some(args.next().expect("--port needs a number").parse()?),
            "--ui" => ui_dir = Some(PathBuf::from(args.next().expect("--ui needs a folder"))),
            "--queue" => queue_dir = Some(PathBuf::from(args.next().expect("--queue needs a folder"))),
            "-h" | "--help" => {
                println!("alt-ledger-server [--settings FILE] [--port N] [--ui DIR] [--queue DIR]");
                return Ok(());
            }
            other => anyhow::bail!("unknown option {other}"),
        }
    }

    let mut running =
        start(Options { settings_path: settings_path.clone(), ui_dir, port, cache_dir: None, queue_dir }).await?;
    println!("\nRosterbook {} is running.", alt_ledger_server::VERSION);
    println!("  Settings:    {}", settings_path.display());
    println!("  On this PC:  http://localhost:{}", running.port);
    if let Some(best) = net::lan_addresses().first() {
        println!("  Network:     http://{}:{}  ({})", best.ip, running.port, best.adapter);
    }
    println!("\nOpen it on this PC to choose your games and to connect a tablet or phone.\n");

    tokio::signal::ctrl_c().await?;
    running.stop();
    let _ = running.task.await;
    Ok(())
}
