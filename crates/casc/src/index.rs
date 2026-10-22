//! Local index files (`Data/data/BBVVVVVVVV.idx`): encoded key -> archive location.
//!
//! There are 16 buckets; each key lives in the bucket given by a hash of its
//! first 9 bytes, and the newest file per bucket wins. Entries are fixed-size
//! and sorted, so a lookup is a binary search done with file seeks: nothing is
//! loaded into memory.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::{bad, Key, Result};

const KEY_LEN: usize = 9;
const ENTRY_LEN: u64 = 18; // 9-byte key, 5-byte location, 4-byte size

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub archive: u32,
    pub offset: u64,
    pub size: u32,
}

pub struct Indexes {
    files: [Option<PathBuf>; 16],
}

impl Indexes {
    pub fn open(dir: &Path) -> Result<Self> {
        let mut best: [Option<(u32, PathBuf)>; 16] = Default::default();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            let Some(stem) = name.strip_suffix(".idx") else { continue };
            if stem.len() != 10 {
                continue;
            }
            let (Ok(bucket), Ok(version)) = (u8::from_str_radix(&stem[..2], 16), u32::from_str_radix(&stem[2..], 16))
            else {
                continue;
            };
            let slot = &mut best[usize::from(bucket & 0x0f)];
            if slot.as_ref().is_none_or(|(v, _)| version > *v) {
                *slot = Some((version, path));
            }
        }
        if best.iter().all(Option::is_none) {
            return Err(bad("no index files in Data/data"));
        }
        Ok(Indexes { files: best.map(|b| b.map(|(_, p)| p)) })
    }

    pub fn find(&self, ekey: &Key) -> Result<Option<Location>> {
        let b = bucket(&ekey[..KEY_LEN]);
        // A few entries are filed one bucket over in current installs.
        for bucket in [b, (b + 1) % 16] {
            if let Some(path) = &self.files[usize::from(bucket)] {
                if let Some(loc) = search(path, &ekey[..KEY_LEN])? {
                    return Ok(Some(loc));
                }
            }
        }
        Ok(None)
    }
}

pub fn bucket(key: &[u8]) -> u8 {
    let x = key.iter().fold(0u8, |a, b| a ^ b);
    (x & 0x0f) ^ (x >> 4)
}

fn search(path: &Path, key: &[u8]) -> Result<Option<Location>> {
    let mut f = File::open(path)?;
    let file_len = f.metadata()?.len();

    let mut head = [0u8; 8];
    f.read_exact(&mut head)?;
    let header_len = u32::from_le_bytes(head[0..4].try_into().unwrap());
    if !(8..=64).contains(&header_len) {
        return Err(bad("index header has an unexpected size"));
    }
    let mut header = vec![0u8; header_len as usize];
    f.read_exact(&mut header)?;
    let version = u16::from_le_bytes([header[0], header[1]]);
    let (size_len, offset_len, key_len, offset_bits) = (header[4], header[5], header[6], header[7]);
    if version != 7 || size_len != 4 || offset_len != 5 || usize::from(key_len) != KEY_LEN || offset_bits != 30 {
        return Err(bad(format!("index version {version} isn't supported")));
    }

    let entries_at = (8 + u64::from(header_len) + 15) & !15;
    f.seek(SeekFrom::Start(entries_at))?;
    let mut block = [0u8; 8];
    f.read_exact(&mut block)?;
    let entries_len = u64::from(u32::from_le_bytes(block[0..4].try_into().unwrap()));
    let first = entries_at + 8;
    if first + entries_len > file_len {
        return Err(bad("index file is truncated"));
    }

    let (mut lo, mut hi) = (0u64, entries_len / ENTRY_LEN);
    let mut entry = [0u8; ENTRY_LEN as usize];
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        f.seek(SeekFrom::Start(first + mid * ENTRY_LEN))?;
        f.read_exact(&mut entry)?;
        match entry[..KEY_LEN].cmp(key) {
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
            std::cmp::Ordering::Equal => {
                let packed = u64::from_be_bytes([0, 0, 0, entry[9], entry[10], entry[11], entry[12], entry[13]]);
                return Ok(Some(Location {
                    archive: (packed >> 30) as u32,
                    offset: packed & ((1 << 30) - 1),
                    size: u32::from_le_bytes(entry[14..18].try_into().unwrap()),
                }));
            }
        }
    }
    Ok(None)
}

/// Writes an index file in the real layout (used by tests).
#[cfg(any(test, feature = "testing"))]
pub fn write_index(path: &Path, mut entries: Vec<([u8; 9], Location)>) {
    entries.sort_by_key(|e| e.0);
    let mut b = Vec::new();
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&7u16.to_le_bytes());
    b.extend_from_slice(&[0, 0, 4, 5, 9, 30]);
    b.extend_from_slice(&(1u64 << 40).to_le_bytes());
    while b.len() % 16 != 0 {
        b.push(0);
    }
    b.extend_from_slice(&((entries.len() * 18) as u32).to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    for (k, loc) in entries {
        b.extend_from_slice(&k);
        let packed = (u64::from(loc.archive) << 30) | loc.offset;
        b.extend_from_slice(&packed.to_be_bytes()[3..]);
        b.extend_from_slice(&loc.size.to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_entries_and_prefers_newest_file() {
        let dir = tempfile::tempdir().unwrap();
        let key: Key = [7; 16];
        let b = bucket(&key[..9]);
        let loc = Location { archive: 41, offset: 1_062_875_165, size: 7957 };
        let others: Vec<_> = (0..50u8).map(|i| ([i, 1, 2, 3, 4, 5, 6, 7, b ^ i], Location { archive: 1, offset: 0, size: 1 })).collect();
        let mut entries = others.clone();
        entries.push((key[..9].try_into().unwrap(), loc));
        write_index(&dir.path().join(format!("{b:02x}00000002.idx")), entries);
        write_index(&dir.path().join(format!("{b:02x}00000001.idx")), others);

        let idx = Indexes::open(dir.path()).unwrap();
        assert_eq!(idx.find(&key).unwrap(), Some(loc));
        assert_eq!(idx.find(&[9; 16]).unwrap(), None);
    }
}
