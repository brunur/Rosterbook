//! BLTE: how CASC stores file contents, as a list of chunks that are each raw
//! (`N`), zlib-compressed (`Z`) or encrypted (`E`, which we don't read).
//!
//! `Reader` decodes as a stream, even inside a chunk: some of the game's
//! tables are stored as one chunk of a hundred megabytes or more, and this
//! never holds more than a small buffer of it in memory. Each chunk's output
//! is counted and must match the size its header declares.

use std::collections::VecDeque;
use std::io::{self, Read, Take};

use flate2::read::ZlibDecoder;

use crate::{bad, Error, Result};

const MAX_CHUNKS: u32 = 1 << 20;

#[derive(Clone, Copy)]
struct Chunk {
    encoded: u64,
    decoded: Option<u64>,
}

enum Stage<R> {
    /// Positioned at the start of the next chunk.
    Between(R),
    Raw(Take<R>, Chunk),
    Zlib(Box<ZlibDecoder<Take<R>>>, Chunk),
}

pub struct Reader<R> {
    stage: Option<Stage<R>>,
    chunks: VecDeque<Chunk>,
    produced: u64,
}

impl<R: Read> Reader<R> {
    /// `src` is positioned at the `BLTE` signature; `encoded_size` is the
    /// stored size from there on.
    pub fn new(mut src: R, encoded_size: u64) -> Result<Self> {
        let mut head = [0u8; 8];
        src.read_exact(&mut head)?;
        if &head[0..4] != b"BLTE" {
            return Err(bad("missing BLTE signature"));
        }
        let header_size = u32::from_be_bytes(head[4..8].try_into().unwrap());
        let mut chunks = VecDeque::new();
        if header_size == 0 {
            let encoded = encoded_size.checked_sub(8).filter(|&n| n > 0).ok_or_else(|| bad("bad BLTE size"))?;
            chunks.push_back(Chunk { encoded, decoded: None });
        } else {
            let mut info = [0u8; 4];
            src.read_exact(&mut info)?;
            let (flags, count) = (info[0], u32::from_be_bytes([0, info[1], info[2], info[3]]));
            if flags != 0x0f || count == 0 || count > MAX_CHUNKS || u64::from(header_size) != 12 + 24 * u64::from(count) {
                return Err(bad("unsupported BLTE chunk table"));
            }
            let mut total = u64::from(header_size);
            for _ in 0..count {
                let mut e = [0u8; 24];
                src.read_exact(&mut e)?;
                let encoded = u64::from(u32::from_be_bytes(e[0..4].try_into().unwrap()));
                let decoded = u64::from(u32::from_be_bytes(e[4..8].try_into().unwrap()));
                if encoded == 0 {
                    return Err(bad("empty BLTE chunk"));
                }
                total += encoded;
                chunks.push_back(Chunk { encoded, decoded: Some(decoded) });
            }
            if total > encoded_size {
                return Err(bad("BLTE chunks overrun the stored size"));
            }
        }
        Ok(Reader { stage: Some(Stage::Between(src)), chunks, produced: 0 })
    }

    fn read_inner(&mut self, out: &mut [u8]) -> Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            match self.stage.take() {
                None => return Err(bad("BLTE stream used after an error")),
                Some(Stage::Between(mut src)) => {
                    let Some(chunk) = self.chunks.pop_front() else {
                        self.stage = Some(Stage::Between(src));
                        return Ok(0);
                    };
                    let mut mode = [0u8; 1];
                    src.read_exact(&mut mode)?;
                    let body = src.take(chunk.encoded - 1);
                    self.produced = 0;
                    self.stage = Some(match mode[0] {
                        b'N' => Stage::Raw(body, chunk),
                        b'Z' => Stage::Zlib(Box::new(ZlibDecoder::new(body)), chunk),
                        b'E' => return Err(Error::Encrypted),
                        other => return Err(bad(format!("unsupported BLTE chunk type {:?}", other as char))),
                    });
                }
                Some(Stage::Raw(mut body, chunk)) => {
                    let n = body.read(out)?;
                    if n > 0 {
                        self.count(n, chunk)?;
                        self.stage = Some(Stage::Raw(body, chunk));
                        return Ok(n);
                    }
                    if body.limit() != 0 {
                        return Err(bad("BLTE data is truncated"));
                    }
                    self.finish(chunk)?;
                    self.stage = Some(Stage::Between(body.into_inner()));
                }
                Some(Stage::Zlib(mut z, chunk)) => {
                    let n = z.read(out).map_err(|e| bad(format!("zlib: {e}")))?;
                    if n > 0 {
                        self.count(n, chunk)?;
                        self.stage = Some(Stage::Zlib(z, chunk));
                        return Ok(n);
                    }
                    self.finish(chunk)?;
                    // Skip anything after the end of the zlib stream in this chunk.
                    let mut body = z.into_inner();
                    io::copy(&mut body, &mut io::sink())?;
                    if body.limit() != 0 {
                        return Err(bad("BLTE data is truncated"));
                    }
                    self.stage = Some(Stage::Between(body.into_inner()));
                }
            }
        }
    }

    fn count(&mut self, n: usize, chunk: Chunk) -> Result<()> {
        self.produced += n as u64;
        if chunk.decoded.is_some_and(|d| self.produced > d) {
            return Err(bad("BLTE chunk decoded to more than its stated size"));
        }
        Ok(())
    }

    fn finish(&self, chunk: Chunk) -> Result<()> {
        if chunk.decoded.is_some_and(|d| self.produced != d) {
            return Err(bad("BLTE chunk decoded to the wrong size"));
        }
        Ok(())
    }
}

impl<R: Read> Read for Reader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.read_inner(out).map_err(io::Error::other)
    }
}

/// Gets our own error back out of an `io::Error` raised while reading.
pub fn unwrap_io(e: io::Error) -> Error {
    match e.get_ref() {
        Some(inner) if inner.is::<Error>() => *e.into_inner().and_then(|i| i.downcast::<Error>().ok()).expect("checked above"),
        _ => Error::Io(e),
    }
}

pub fn read_exact(r: &mut impl Read, buf: &mut [u8]) -> Result<()> {
    r.read_exact(buf).map_err(unwrap_io)
}

/// Encodes data as BLTE with the given chunk splits (used by tests).
#[cfg(any(test, feature = "testing"))]
pub fn encode(data: &[u8], chunk_len: usize, zlib: bool) -> Vec<u8> {
    use std::io::Write;
    let parts: Vec<Vec<u8>> = data
        .chunks(chunk_len.max(1))
        .map(|c| {
            if zlib {
                let mut z = flate2::write::ZlibEncoder::new(vec![b'Z'], flate2::Compression::default());
                z.write_all(c).unwrap();
                z.finish().unwrap()
            } else {
                [&b"N"[..], c].concat()
            }
        })
        .collect();
    let mut out = b"BLTE".to_vec();
    out.extend_from_slice(&(12 + 24 * parts.len() as u32).to_be_bytes());
    out.push(0x0f);
    out.extend_from_slice(&(parts.len() as u32).to_be_bytes()[1..]);
    for (part, plain) in parts.iter().zip(data.chunks(chunk_len.max(1))) {
        out.extend_from_slice(&(part.len() as u32).to_be_bytes());
        out.extend_from_slice(&(plain.len() as u32).to_be_bytes());
        out.extend_from_slice(&[0; 16]);
    }
    for part in parts {
        out.extend_from_slice(&part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_raw_and_zlib_chunks() {
        let data: Vec<u8> = (0..10_000u32).map(|i| (i * 7 % 251) as u8).collect();
        for zlib in [false, true] {
            let enc = encode(&data, 3000, zlib);
            let mut out = Vec::new();
            Reader::new(&enc[..], enc.len() as u64).unwrap().read_to_end(&mut out).unwrap();
            assert_eq!(out, data);
        }
    }

    #[test]
    fn streams_a_large_chunk_and_checks_its_size() {
        let data: Vec<u8> = (0..6_000_000u32).map(|i| (i % 97) as u8).collect();
        let enc = encode(&data, data.len(), true);
        let mut out = Vec::new();
        Reader::new(&enc[..], enc.len() as u64).unwrap().read_to_end(&mut out).unwrap();
        assert_eq!(out.len(), data.len());

        // Header claims one byte more than the chunk holds.
        let mut lying = enc.clone();
        lying[16..20].copy_from_slice(&(data.len() as u32 + 1).to_be_bytes());
        let mut r = Reader::new(&lying[..], lying.len() as u64).unwrap();
        assert!(r.read_to_end(&mut Vec::new()).is_err());
    }

    #[test]
    fn single_chunk_without_table() {
        let mut enc = b"BLTE\0\0\0\0N".to_vec();
        enc.extend_from_slice(b"hello");
        let mut out = Vec::new();
        Reader::new(&enc[..], enc.len() as u64).unwrap().read_to_end(&mut out).unwrap();
        assert_eq!(out, b"hello");
    }

    #[test]
    fn rejects_encrypted_and_damaged_data() {
        let enc = b"BLTE\0\0\0\0Exxxx".to_vec();
        let mut r = Reader::new(&enc[..], enc.len() as u64).unwrap();
        assert!(matches!(r.read_to_end(&mut Vec::new()).map_err(unwrap_io), Err(Error::Encrypted)));
        assert!(Reader::new(&b"NOPE\0\0\0\0"[..], 8).is_err());
        // A chunk table claiming more data than is stored.
        let mut big = encode(&[1, 2, 3], 3, false);
        let n = big.len() as u64;
        big[12..16].copy_from_slice(&1_000_000u32.to_be_bytes());
        assert!(Reader::new(&big[..], n).is_err());
    }
}
