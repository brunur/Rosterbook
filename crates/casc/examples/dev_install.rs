//! Development helper: builds a miniature WoW install (Forever product) whose
//! icons are the given PNG files, for trying the item-pictures feature without
//! the game.
//!
//!   cargo run -p alt-ledger-casc --features testing --example dev_install -- INSTALL_DIR ID=FILE.png...

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("usage: dev_install INSTALL_DIR ID=FILE.png..."));
    let files: Vec<(u32, Vec<u8>)> = args
        .map(|a| {
            let (id, path) = a.split_once('=').expect("arguments look like ID=FILE.png");
            (id.parse().expect("ID is a number"), png_to_blp(&std::fs::read(path).expect("can't read PNG")))
        })
        .collect();
    let flavor = alt_ledger_casc::testing::fake_install(&dir, &files);
    println!("{} icons in {}", files.len(), flavor.display());
}

/// Uncompressed BGRA BLP2 from an RGBA PNG.
fn png_to_blp(png_bytes: &[u8]) -> Vec<u8> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png_bytes)).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba, "expects RGBA PNGs");
    let bgra: Vec<u8> = buf[..info.buffer_size()].chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    alt_ledger_casc::testing::blp(3, 8, 0, info.width, info.height, &bgra)
}
