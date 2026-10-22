//! The encoding file: content key -> encoded key.
//!
//! Layout (big-endian): a 22-byte header, a block of encoding specs to skip,
//! a page index holding the first content key of every page, then the pages.
//! We stream through the pages and only parse the ones whose key range could
//! hold a key we want, so memory stays at one page plus the index.

use std::collections::{BTreeSet, HashMap};
use std::io::{self, Read};

use crate::blte::read_exact;
use crate::{bad, Key, Result};

const MAX_PAGES: u64 = 1 << 20;
const MAX_PAGE: usize = 1 << 20;

pub fn lookup(mut r: impl Read, wanted: &BTreeSet<Key>) -> Result<HashMap<Key, Key>> {
    let mut found = HashMap::new();
    if wanted.is_empty() {
        return Ok(found);
    }
    let mut h = [0u8; 22];
    read_exact(&mut r, &mut h)?;
    if &h[0..2] != b"EN" || h[2] != 1 {
        return Err(bad("unrecognised encoding file"));
    }
    let (ckey_len, ekey_len) = (usize::from(h[3]), usize::from(h[4]));
    if ckey_len != 16 || !(9..=16).contains(&ekey_len) {
        return Err(bad("unsupported key sizes in encoding file"));
    }
    let page_size = usize::from(u16::from_be_bytes([h[5], h[6]])) * 1024;
    let page_count = u64::from(u32::from_be_bytes(h[9..13].try_into().unwrap()));
    let spec_block = u64::from(u32::from_be_bytes(h[18..22].try_into().unwrap()));
    if page_size == 0 || page_size > MAX_PAGE || page_count > MAX_PAGES {
        return Err(bad("implausible encoding page table"));
    }
    skip(&mut r, spec_block)?;

    // Page index: first content key and checksum of each page.
    let mut firsts = Vec::with_capacity(page_count as usize);
    let mut entry = [0u8; 32];
    for _ in 0..page_count {
        read_exact(&mut r, &mut entry)?;
        firsts.push(Key::try_from(&entry[..16]).unwrap());
    }

    let mut page = vec![0u8; page_size];
    for (i, first) in firsts.iter().enumerate() {
        read_exact(&mut r, &mut page)?;
        let candidates: Vec<&Key> = match firsts.get(i + 1) {
            Some(next) if next <= first => return Err(bad("encoding page index is out of order")),
            Some(next) => wanted.range(*first..*next).collect(),
            None => wanted.range(*first..).collect(),
        };
        if candidates.is_empty() {
            continue;
        }
        let mut at = 0;
        while at + 6 + ckey_len <= page.len() {
            let key_count = usize::from(page[at]);
            if key_count == 0 {
                break; // padding to the end of the page
            }
            let ckey = Key::try_from(&page[at + 6..at + 22]).unwrap();
            let ekeys_at = at + 6 + ckey_len;
            let next = ekeys_at + key_count * ekey_len;
            if next > page.len() {
                return Err(bad("encoding entry crosses a page"));
            }
            if candidates.contains(&&ckey) {
                let mut ekey = [0u8; 16];
                ekey[..ekey_len].copy_from_slice(&page[ekeys_at..ekeys_at + ekey_len]);
                found.insert(ckey, ekey);
            }
            at = next;
        }
        if found.len() == wanted.len() {
            break;
        }
    }
    Ok(found)
}

fn skip(r: &mut impl Read, n: u64) -> Result<()> {
    let copied = io::copy(&mut r.take(n), &mut io::sink()).map_err(crate::blte::unwrap_io)?;
    if copied != n {
        return Err(bad("encoding file is truncated"));
    }
    Ok(())
}

/// Builds an encoding file mapping each content key to an encoded key (tests).
#[cfg(any(test, feature = "testing"))]
pub fn build(mut pairs: Vec<(Key, Key)>, page_size_kb: u16) -> Vec<u8> {
    pairs.sort();
    let page_size = usize::from(page_size_kb) * 1024;
    let mut pages: Vec<Vec<u8>> = vec![];
    let mut firsts = vec![];
    for (ckey, ekey) in pairs {
        let entry: Vec<u8> = [&[1u8, 0, 0, 0, 0x10, 0][..], &ckey, &ekey].concat();
        if pages.last().is_none_or(|p| p.len() + entry.len() > page_size) {
            pages.push(vec![]);
            firsts.push(ckey);
        }
        pages.last_mut().unwrap().extend_from_slice(&entry);
    }
    let spec = b"z\0".to_vec();
    let mut out = b"EN".to_vec();
    out.extend_from_slice(&[1, 16, 16]);
    out.extend_from_slice(&page_size_kb.to_be_bytes());
    out.extend_from_slice(&page_size_kb.to_be_bytes());
    out.extend_from_slice(&(pages.len() as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.push(0);
    out.extend_from_slice(&(spec.len() as u32).to_be_bytes());
    out.extend_from_slice(&spec);
    for f in &firsts {
        out.extend_from_slice(f);
        out.extend_from_slice(&[0; 16]);
    }
    for mut p in pages {
        p.resize(page_size, 0);
        out.extend_from_slice(&p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u32) -> Key {
        let mut k = [0u8; 16];
        k[..4].copy_from_slice(&n.to_be_bytes());
        k[15] = (n % 251) as u8;
        k
    }

    #[test]
    fn finds_keys_across_pages() {
        let pairs: Vec<_> = (0..500u32).map(|n| (key(n * 3), key(n * 3 + 1_000_000))).collect();
        let file = build(pairs, 1); // 1 KB pages: many pages
        let wanted: BTreeSet<Key> = [key(0), key(3 * 250), key(3 * 499), key(7)].into();
        let found = lookup(&file[..], &wanted).unwrap();
        assert_eq!(found.len(), 3, "key(7) isn't in the file");
        assert_eq!(found[&key(750)], key(750 + 1_000_000));
        assert_eq!(found[&key(1497)], key(1497 + 1_000_000));
    }
}
