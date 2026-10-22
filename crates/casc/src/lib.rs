//! Read-only access to item icons in a local World of Warcraft install.
//!
//! WoW keeps its game files in CASC storage under `<install>/Data`. This crate
//! reads just enough of it to turn icon file IDs (which the add-on records)
//! into PNG images:
//!
//! 1. `.build.info` and the build config say which encoding and root files the
//!    installed product (`wow`, `wow_classic_beta`, ...) uses.
//! 2. The root file maps file IDs to content keys, the encoding file maps
//!    content keys to encoded keys, and the local `.idx` files map encoded keys
//!    to a place in a `data.NNN` archive.
//! 3. Files are stored BLTE-encoded (raw or zlib chunks); icons are BLP
//!    textures, decoded to PNG here.
//!
//! Design constraints: it never writes to the install, never touches the
//! network, and streams the large tables (encoding, root) chunk by chunk so a
//! lookup uses a few megabytes of memory rather than hundreds. Every size read
//! from disk is bounds-checked before allocating, and extracted files are
//! verified against their MD5 content key.

mod blp;
mod blte;
mod encoding;
mod index;
mod root;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};

pub use blp::blp_to_png;

pub type Key = [u8; 16];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not a supported WoW install: {0}")]
    Install(String),
    #[error("damaged or unsupported game data: {0}")]
    Format(String),
    #[error("this file is encrypted")]
    Encrypted,
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn bad(msg: impl Into<String>) -> Error {
    Error::Format(msg.into())
}

/// One installed product (a game flavor) inside a WoW install.
#[derive(Debug, Clone)]
pub struct Install {
    data_dir: PathBuf,
    /// e.g. `wow`, `wow_classic_beta`.
    pub product: String,
    /// e.g. `12.1.5.70448`.
    pub version: String,
    /// Changes with every patch; used to key caches.
    pub build_key: String,
    root_ckey: Key,
    encoding_ekey: Key,
}

impl Install {
    /// Opens the product installed in `flavor_dir` (e.g. `.../World of Warcraft/_retail_`),
    /// reading its name from `.flavor.info`.
    pub fn open_flavor(flavor_dir: &Path) -> Result<Self> {
        let info = std::fs::read_to_string(flavor_dir.join(".flavor.info"))
            .map_err(|e| Error::Install(format!("no .flavor.info in {}: {e}", flavor_dir.display())))?;
        let product = info
            .lines()
            .skip(1)
            .map(str::trim)
            .find(|l| !l.is_empty())
            .ok_or_else(|| Error::Install(".flavor.info is empty".into()))?
            .to_string();
        let root = flavor_dir.parent().ok_or_else(|| Error::Install("flavor folder has no parent".into()))?;
        Self::open(root, &product)
    }

    /// Opens `product` in the install at `install_root` (the folder holding `.build.info`).
    pub fn open(install_root: &Path, product: &str) -> Result<Self> {
        let build_info = std::fs::read_to_string(install_root.join(".build.info"))
            .map_err(|e| Error::Install(format!("no .build.info: {e}")))?;
        let row = parse_build_info(&build_info)
            .into_iter()
            .find(|r| r.get("Product").map(String::as_str) == Some(product) && r.get("Active").map(String::as_str) != Some("0"))
            .ok_or_else(|| Error::Install(format!("{product} is not installed here")))?;
        let build_key = row.get("Build Key").filter(|k| is_hex_key(k)).cloned().ok_or_else(|| Error::Install("no build key".into()))?;
        let version = row.get("Version").cloned().unwrap_or_default();

        let data_dir = install_root.join("Data");
        let config_path = data_dir.join("config").join(&build_key[0..2]).join(&build_key[2..4]).join(&build_key);
        let config = std::fs::read_to_string(&config_path)
            .map_err(|e| Error::Install(format!("build config missing ({e})")))?;
        let mut root_ckey = None;
        let mut encoding_ekey = None;
        for line in config.lines() {
            let Some((name, value)) = line.split_once('=') else { continue };
            let values: Vec<&str> = value.split_whitespace().collect();
            match name.trim() {
                "root" => root_ckey = values.first().and_then(|v| parse_key(v)),
                // "encoding = <content key> <encoded key>"
                "encoding" => encoding_ekey = values.get(1).and_then(|v| parse_key(v)),
                _ => {}
            }
        }
        Ok(Install {
            data_dir,
            product: product.to_string(),
            version,
            build_key,
            root_ckey: root_ckey.ok_or_else(|| bad("build config has no root"))?,
            encoding_ekey: encoding_ekey.ok_or_else(|| bad("build config has no encoding key"))?,
        })
    }

    /// Reads the files with these file data IDs. IDs that aren't in this build
    /// (or are encrypted) are left out of the result, so a caller can show a
    /// fallback for them; only problems with the storage itself are errors.
    pub fn read_files(&self, file_ids: &BTreeSet<u32>) -> Result<BTreeMap<u32, Vec<u8>>> {
        let mut out = BTreeMap::new();
        if file_ids.is_empty() {
            return Ok(out);
        }
        let indexes = index::Indexes::open(&self.data_dir.join("data"))?;

        // root file: content key -> encoded key, via the encoding file.
        let enc_loc = indexes.find(&self.encoding_ekey)?.ok_or_else(|| bad("encoding file not found in local storage"))?;
        let root_ekey = {
            let reader = self.open_entry(&enc_loc).map_err(ctx("opening the encoding table"))?;
            let wanted = BTreeSet::from([self.root_ckey]);
            *encoding::lookup(reader, &wanted)
                .map_err(ctx("reading the encoding table"))?
                .get(&self.root_ckey)
                .ok_or_else(|| bad("root file missing from encoding"))?
        };
        let root_loc = indexes.find(&root_ekey)?.ok_or_else(|| bad("root file not found in local storage"))?;
        let root_reader = self.open_entry(&root_loc).map_err(ctx("opening the root table"))?;
        let ckeys: HashMap<u32, Key> = root::lookup(root_reader, file_ids).map_err(ctx("reading the root table"))?;

        // encoded keys for every requested file, in one more pass over encoding.
        let wanted: BTreeSet<Key> = ckeys.values().copied().collect();
        let ekeys = encoding::lookup(self.open_entry(&enc_loc)?, &wanted).map_err(ctx("reading the encoding table"))?;

        for (&id, ckey) in &ckeys {
            let Some(ekey) = ekeys.get(ckey) else { continue };
            let Some(loc) = indexes.find(ekey)? else { continue };
            let mut data = Vec::new();
            match self.open_entry(&loc).and_then(|mut r| {
                (&mut r).take(MAX_FILE).read_to_end(&mut data).map_err(blte::unwrap_io)
            }) {
                Ok(_) => {}
                Err(Error::Encrypted) => continue,
                Err(e) => return Err(e),
            }
            if Md5::digest(&data).as_slice() != ckey {
                return Err(bad(format!("file {id} failed its checksum")));
            }
            out.insert(id, data);
        }
        Ok(out)
    }

    fn open_entry(&self, loc: &index::Location) -> Result<blte::Reader<BufReader<File>>> {
        let path = self.data_dir.join("data").join(format!("data.{:03}", loc.archive));
        let mut f = BufReader::new(File::open(&path)?);
        f.seek(SeekFrom::Start(loc.offset))?;
        // Each archive entry starts with a 30-byte header: the encoded key
        // (byte-reversed), the entry size, flags and two checksums.
        let mut header = [0u8; 30];
        f.read_exact(&mut header)?;
        let size = u32::from_le_bytes(header[16..20].try_into().unwrap());
        if size != loc.size || size < 30 + 8 {
            return Err(bad("archive entry size doesn't match its index"));
        }
        blte::Reader::new(f, u64::from(size) - 30)
    }
}

/// Adds which step failed to a format error.
fn ctx(stage: &'static str) -> impl Fn(Error) -> Error {
    move |e| match e {
        Error::Format(m) => Error::Format(format!("{stage}: {m}")),
        other => other,
    }
}

/// Largest single file we'll extract (icons are a few kilobytes).
const MAX_FILE: u64 = 16 << 20;

fn parse_build_info(text: &str) -> Vec<HashMap<String, String>> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#'));
    let Some(header) = lines.next() else { return vec![] };
    let names: Vec<String> = header.split('|').map(|h| h.split('!').next().unwrap_or("").to_string()).collect();
    lines
        .map(|l| names.iter().cloned().zip(l.split('|').map(str::to_string)).collect())
        .collect()
}

fn is_hex_key(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn parse_key(s: &str) -> Option<Key> {
    if !is_hex_key(s) {
        return None;
    }
    let mut k = [0u8; 16];
    for (i, byte) in k.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(k)
}

/// Builds miniature WoW installs for tests (also used by the server's tests).
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;

    /// Writes a complete miniature install: .build.info, flavor folder, build
    /// config, index files and one archive holding encoding, root and files.
    pub fn fake_install(dir: &Path, files: &[(u32, Vec<u8>)]) -> PathBuf {
        let md5 = |d: &[u8]| -> Key { Md5::digest(d).into() };
        let ekey_of = |d: &[u8]| -> Key { let mut k = md5(d); k[15] ^= 0x5a; k };

        let mut archive = Vec::new();
        let mut index_entries: Vec<([u8; 9], index::Location)> = vec![];
        let mut store = |plain: &[u8], ekey: Key| {
            let blte = blte::encode(plain, 700, true);
            let offset = archive.len() as u64;
            let size = (30 + blte.len()) as u32;
            let mut rev = ekey;
            rev.reverse();
            archive.extend_from_slice(&rev);
            archive.extend_from_slice(&size.to_le_bytes());
            archive.extend_from_slice(&[0; 10]);
            archive.extend_from_slice(&blte);
            index_entries.push((ekey[..9].try_into().unwrap(), index::Location { archive: 0, offset, size }));
        };

        let mut pairs = vec![];
        let mut root_files = vec![];
        for (id, data) in files {
            let (c, e) = (md5(data), ekey_of(data));
            store(data, e);
            pairs.push((c, e));
            root_files.push((*id, c));
        }
        root_files.sort();
        let root = root::build(&[(0, 0xFFFF_FFFF, root_files)]);
        let (root_c, root_e) = (md5(&root), ekey_of(&root));
        store(&root, root_e);
        pairs.push((root_c, root_e));
        let encoding = encoding::build(pairs, 4);
        let enc_e = ekey_of(&encoding);
        store(&encoding, enc_e);

        let data = dir.join("Data/data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("data.000"), &archive).unwrap();
        let mut by_bucket: HashMap<u8, Vec<_>> = HashMap::new();
        for e in index_entries {
            by_bucket.entry(index::bucket(&e.0)).or_default().push(e);
        }
        for (b, entries) in by_bucket {
            index::write_index(&data.join(format!("{b:02x}00000001.idx")), entries);
        }

        let hex = |k: &Key| k.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let build_key = "84bea4233331e390fda8724c6f91ffa5";
        let cfg = dir.join("Data/config/84/be");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join(build_key),
            format!("# Build Configuration\n\nroot = {}\nencoding = {} {}\n", hex(&root_c), hex(&md5(&encoding)), hex(&enc_e)),
        )
        .unwrap();
        std::fs::write(
            dir.join(".build.info"),
            format!("Branch!STRING:0|Active!DEC:1|Build Key!HEX:16|Version!STRING:0|Product!STRING:0\nus|1|{build_key}|1.60.1.70450|wow_classic_beta\n"),
        )
        .unwrap();
        let flavor = dir.join("_classic_beta_");
        std::fs::create_dir_all(&flavor).unwrap();
        std::fs::write(flavor.join(".flavor.info"), "Product Flavor!STRING:0\nwow_classic_beta\n").unwrap();
        flavor
    }

    /// A BLP2 texture with one mip level (compression 1 palette, 2 DXT, 3 BGRA).
    pub fn blp(compression: u8, alpha_depth: u8, alpha_type: u8, w: u32, h: u32, mip: &[u8]) -> Vec<u8> {
        blp::build(compression, alpha_depth, alpha_type, w, h, &[], mip)
    }

    /// A 4x4 icon: red top-left texel, blue next to it.
    pub fn sample_icon() -> Vec<u8> {
        blp::build(2, 0, 0, 4, 4, &[], &[0x00, 0xF8, 0x1F, 0x00, 0b0000_0100, 0, 0, 0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testing::fake_install;

    #[test]
    fn extracts_icons_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let icon = blp::build(2, 0, 0, 4, 4, &[], &[0x00, 0xF8, 0x1F, 0x00, 0, 0, 0, 0]);
        let filler: Vec<(u32, Vec<u8>)> = (0..300u32).map(|i| (1000 + i * 7, format!("file {i}").repeat(40).into_bytes())).collect();
        let mut files = filler.clone();
        files.push((134400, icon.clone()));
        let flavor = fake_install(dir.path(), &files);

        let install = Install::open_flavor(&flavor).unwrap();
        assert_eq!(install.product, "wow_classic_beta");
        assert_eq!(install.version, "1.60.1.70450");

        let wanted: BTreeSet<u32> = [134400, 1000 + 7 * 150, 42].into();
        let got = install.read_files(&wanted).unwrap();
        assert_eq!(got.len(), 2, "unknown IDs are skipped");
        assert_eq!(got[&134400], icon);
        assert_eq!(got[&(1000 + 7 * 150)], filler[150].1);
        let png = blp_to_png(&got[&134400]).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn reports_missing_products() {
        let dir = tempfile::tempdir().unwrap();
        fake_install(dir.path(), &[]);
        assert!(matches!(Install::open(dir.path(), "wow"), Err(Error::Install(_))));
    }

    #[test]
    fn reads_build_info_rows() {
        let text = "Branch!STRING:0|Active!DEC:1|Build Key!HEX:16|Version!STRING:0|Product!STRING:0\n\
                    us|1|88d5e8ef05ec5a8446cdec93d142d7c6|12.1.5.70448|wow\n\
                    us|1|84bea4233331e390fda8724c6f91ffa5|1.60.1.70450|wow_classic_beta\n";
        let rows = parse_build_info(text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1]["Product"], "wow_classic_beta");
        assert_eq!(rows[1]["Build Key"], "84bea4233331e390fda8724c6f91ffa5");
        assert_eq!(parse_key(&rows[0]["Build Key"]).unwrap()[0], 0x88);
    }
}
