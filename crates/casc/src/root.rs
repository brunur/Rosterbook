//! The WoW root file: file data ID -> content key.
//!
//! It's a series of blocks, each listing files that share locale and content
//! flags. File IDs are delta-encoded. We stream through every block once and
//! keep only the IDs we were asked for, preferring the English or
//! all-locale copy and skipping low-violence variants when there's a choice.

use std::collections::{BTreeSet, HashMap};
use std::io::Read;

use crate::blte::{read_exact, unwrap_io};
use crate::{bad, Key, Result};

const NO_NAME_HASH: u32 = 0x1000_0000;
const LOW_VIOLENCE: u32 = 0x80;
const EN_US: u32 = 0x2;
const ALL_LOCALES: u32 = 0xFFFF_FFFF;
const MAX_RECORDS: u32 = 1 << 24;

pub fn lookup(mut r: impl Read, wanted: &BTreeSet<u32>) -> Result<HashMap<u32, Key>> {
    let mut best: HashMap<u32, (u8, Key)> = HashMap::new();

    let mut first = [0u8; 4];
    if !read_or_eof(&mut r, &mut first)? {
        return Ok(HashMap::new());
    }
    // "TSFM" marks the 8.2+ manifest format; older roots start with a block.
    let (modern, version, allow_unnamed, mut pending) = if &first == b"TSFM" {
        let a = u32_le(&mut r)?;
        let b = u32_le(&mut r)?;
        if a == 0x18 && (b == 1 || b == 2) {
            let total = u32_le(&mut r)?;
            let named = u32_le(&mut r)?;
            u32_le(&mut r)?; // padding
            (true, b, total != named, None)
        } else {
            // 8.2 layout: header is just the two file counts.
            (true, 0, a != b, None)
        }
    } else {
        (false, 0, false, Some(u32::from_le_bytes(first)))
    };

    loop {
        let count = match pending.take() {
            Some(c) => c,
            None => {
                let mut c = [0u8; 4];
                if !read_or_eof(&mut r, &mut c)? {
                    break;
                }
                u32::from_le_bytes(c)
            }
        };
        if count > MAX_RECORDS {
            return Err(bad("root block is implausibly large"));
        }
        let (content, locale) = if version == 2 {
            let locale = u32_le(&mut r)?;
            let a = u32_le(&mut r)?;
            let b = u32_le(&mut r)?;
            let mut c = [0u8; 1];
            read_exact(&mut r, &mut c)?;
            (a | b | (u32::from(c[0]) << 17), locale)
        } else {
            (u32_le(&mut r)?, u32_le(&mut r)?)
        };

        let mut ids = Vec::with_capacity(count as usize);
        let mut id: i64 = -1;
        for _ in 0..count {
            id += 1 + i64::from(i32_le(&mut r)?);
            if !(0..=i64::from(u32::MAX)).contains(&id) {
                return Err(bad("root file ID out of range"));
            }
            ids.push(id as u32);
        }

        let rank = rank(content, locale);
        let mut key = [0u8; 16];
        if modern {
            for &id in &ids {
                read_exact(&mut r, &mut key)?;
                consider(&mut best, wanted, id, rank, key);
            }
            if !(allow_unnamed && content & NO_NAME_HASH != 0) {
                skip(&mut r, u64::from(count) * 8)?;
            }
        } else {
            for &id in &ids {
                read_exact(&mut r, &mut key)?;
                skip(&mut r, 8)?;
                consider(&mut best, wanted, id, rank, key);
            }
        }
    }
    Ok(best.into_iter().map(|(id, (_, k))| (id, k)).collect())
}

/// Higher is better: English or every locale, and not the low-violence copy.
fn rank(content: u32, locale: u32) -> u8 {
    let lang = u8::from(locale == ALL_LOCALES || locale & EN_US != 0);
    let normal = u8::from(content & LOW_VIOLENCE == 0);
    lang * 2 + normal + 1
}

fn consider(best: &mut HashMap<u32, (u8, Key)>, wanted: &BTreeSet<u32>, id: u32, rank: u8, key: Key) {
    if wanted.contains(&id) && best.get(&id).is_none_or(|(r, _)| rank > *r) {
        best.insert(id, (rank, key));
    }
}

fn read_or_eof(r: &mut impl Read, buf: &mut [u8; 4]) -> Result<bool> {
    let mut got = 0;
    while got < buf.len() {
        let n = r.read(&mut buf[got..]).map_err(unwrap_io)?;
        if n == 0 {
            return if got == 0 { Ok(false) } else { Err(bad("root file is truncated")) };
        }
        got += n;
    }
    Ok(true)
}

fn u32_le(r: &mut impl Read) -> Result<u32> {
    let mut b = [0u8; 4];
    read_exact(r, &mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn i32_le(r: &mut impl Read) -> Result<i32> {
    Ok(u32_le(r)? as i32)
}

fn skip(r: &mut impl Read, n: u64) -> Result<()> {
    let copied = std::io::copy(&mut r.take(n), &mut std::io::sink()).map_err(unwrap_io)?;
    if copied != n {
        return Err(bad("root file is truncated"));
    }
    Ok(())
}

/// (content flags, locale flags, files) for one test root block.
#[cfg(any(test, feature = "testing"))]
pub type TestBlock = (u32, u32, Vec<(u32, Key)>);

/// Builds a version-2 root file from blocks (tests).
#[cfg(any(test, feature = "testing"))]
pub fn build(blocks: &[TestBlock]) -> Vec<u8> {
    let total: usize = blocks.iter().map(|b| b.2.len()).sum();
    let mut out = b"TSFM".to_vec();
    for v in [0x18u32, 2, total as u32, 0, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for (content, locale, files) in blocks {
        out.extend_from_slice(&(files.len() as u32).to_le_bytes());
        out.extend_from_slice(&locale.to_le_bytes());
        out.extend_from_slice(&content.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.push(0);
        let mut prev: i64 = -1;
        for (id, _) in files {
            out.extend_from_slice(&((i64::from(*id) - prev - 1) as i32).to_le_bytes());
            prev = i64::from(*id);
        }
        for (_, k) in files {
            out.extend_from_slice(k);
        }
        if content & NO_NAME_HASH == 0 {
            out.extend_from_slice(&vec![0xAB; files.len() * 8]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_ids_and_prefers_english_normal_copies() {
        let k = |n: u8| [n; 16];
        let root = build(&[
            (NO_NAME_HASH, 0x4, vec![(134400, k(1)), (134401, k(2))]), // koKR only
            (0, ALL_LOCALES, vec![(100, k(3)), (134400, k(4)), (200_000, k(5))]),
            (NO_NAME_HASH | LOW_VIOLENCE, ALL_LOCALES, vec![(134401, k(6))]),
        ]);
        let wanted: BTreeSet<u32> = [134400, 134401, 200_000, 999].into();
        let found = lookup(&root[..], &wanted).unwrap();
        assert_eq!(found.len(), 3);
        assert_eq!(found[&134400], k(4), "all-locale beats koKR");
        assert_eq!(found[&134401], k(6), "an all-locale low-violence copy beats koKR");
        assert_eq!(found[&200_000], k(5));
    }

    #[test]
    fn truncated_root_is_an_error() {
        let root = build(&[(0, ALL_LOCALES, vec![(1, [1; 16])])]);
        assert!(lookup(&root[..root.len() - 3], &[1].into()).is_err());
    }
}
