//! BLP2 textures (what WoW icons are) to PNG.
//!
//! Only the first (full size) mip level is decoded. Supported encodings:
//! palettized with 0/1/4/8-bit alpha, DXT1/3/5, and plain BGRA.

use crate::{bad, Result};

const HEADER: usize = 20 + 16 * 4 * 2 + 256 * 4;
const MAX_SIDE: u32 = 4096;

/// Decodes a BLP texture and returns it as a PNG file.
pub fn blp_to_png(data: &[u8]) -> Result<Vec<u8>> {
    let (w, h, rgba) = decode(data)?;
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::High);
    let mut writer = enc.write_header().map_err(|e| bad(format!("png: {e}")))?;
    writer.write_image_data(&rgba).map_err(|e| bad(format!("png: {e}")))?;
    writer.finish().map_err(|e| bad(format!("png: {e}")))?;
    Ok(out)
}

/// Returns width, height and RGBA pixels.
pub fn decode(d: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    if d.len() < HEADER || &d[0..4] != b"BLP2" {
        return Err(bad("not a BLP2 texture"));
    }
    let u32_at = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let (compression, alpha_depth, alpha_type) = (d[8], d[9], d[10]);
    let (w, h) = (u32_at(12), u32_at(16));
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(bad("BLP has an implausible size"));
    }
    let (offset, size) = (u32_at(20) as usize, u32_at(84) as usize);
    let mip = d.get(offset..offset.checked_add(size).ok_or_else(|| bad("BLP mip overflows"))?).ok_or_else(|| bad("BLP mip is outside the file"))?;
    let pixels = (w * h) as usize;
    let mut rgba = vec![0u8; pixels * 4];

    match compression {
        1 => {
            let palette = &d[148..148 + 1024];
            let alpha = &mip.get(pixels..).ok_or_else(|| bad("BLP palette data is short"))?;
            for i in 0..pixels {
                let p = usize::from(mip[i]) * 4;
                let a = match alpha_depth {
                    0 => 255,
                    1 => bit(alpha, i, 1) * 255,
                    4 => bit(alpha, i, 4) * 17,
                    8 => *alpha.get(i).ok_or_else(|| bad("BLP alpha data is short"))?,
                    _ => return Err(bad("unsupported BLP alpha depth")),
                };
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[palette[p + 2], palette[p + 1], palette[p], a]);
            }
        }
        2 => {
            let block_bytes = if alpha_type == 0 { 8 } else { 16 };
            let (bw, bh) = (w.div_ceil(4) as usize, h.div_ceil(4) as usize);
            if mip.len() < bw * bh * block_bytes {
                return Err(bad("BLP DXT data is short"));
            }
            for by in 0..bh {
                for bx in 0..bw {
                    let block = &mip[(by * bw + bx) * block_bytes..][..block_bytes];
                    let texels = match alpha_type {
                        0 => dxt1(block, if alpha_depth > 0 { Dxt1::PunchThrough } else { Dxt1::Opaque }),
                        1 => with_alpha(dxt1(&block[8..], Dxt1::FourColor), explicit_alpha(&block[..8])),
                        7 => with_alpha(dxt1(&block[8..], Dxt1::FourColor), interpolated_alpha(&block[..8])),
                        _ => return Err(bad("unsupported BLP DXT type")),
                    };
                    for (t, px) in texels.iter().enumerate() {
                        let (x, y) = (bx * 4 + t % 4, by * 4 + t / 4);
                        if x < w as usize && y < h as usize {
                            let o = (y * w as usize + x) * 4;
                            rgba[o..o + 4].copy_from_slice(px);
                        }
                    }
                }
            }
        }
        3 => {
            if mip.len() < pixels * 4 {
                return Err(bad("BLP pixel data is short"));
            }
            for (o, px) in rgba.chunks_exact_mut(4).zip(mip.chunks_exact(4)) {
                o.copy_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }
        _ => return Err(bad("unsupported BLP compression")),
    }
    Ok((w, h, rgba))
}

fn bit(data: &[u8], i: usize, bits: usize) -> u8 {
    let pos = i * bits;
    let byte = data.get(pos / 8).copied().unwrap_or(0);
    (byte >> (pos % 8)) & ((1u16 << bits) - 1) as u8
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8]
}

fn mix(a: [u8; 3], b: [u8; 3], wa: u32, wb: u32) -> [u8; 3] {
    let f = |x: u8, y: u8| ((u32::from(x) * wa + u32::from(y) * wb) / (wa + wb)) as u8;
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

#[derive(Clone, Copy, PartialEq)]
enum Dxt1 {
    /// DXT1 without alpha: the 3-colour mode's fourth entry is opaque black.
    Opaque,
    /// DXT1 with 1-bit alpha: that entry is transparent.
    PunchThrough,
    /// The colour half of DXT3/DXT5 blocks always uses four colours.
    FourColor,
}

/// Decodes a DXT1 colour block into 16 RGBA texels.
fn dxt1(b: &[u8], mode: Dxt1) -> [[u8; 4]; 16] {
    let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let colors: [[u8; 4]; 4] = if c0 > c1 || mode == Dxt1::FourColor {
        let (p2, p3) = (mix(p0, p1, 2, 1), mix(p0, p1, 1, 2));
        [rgba(p0, 255), rgba(p1, 255), rgba(p2, 255), rgba(p3, 255)]
    } else {
        let last = if mode == Dxt1::PunchThrough { [0, 0, 0, 0] } else { [0, 0, 0, 255] };
        [rgba(p0, 255), rgba(p1, 255), rgba(mix(p0, p1, 1, 1), 255), last]
    };
    let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    std::array::from_fn(|i| colors[((bits >> (i * 2)) & 3) as usize])
}

fn rgba(c: [u8; 3], a: u8) -> [u8; 4] {
    [c[0], c[1], c[2], a]
}

fn explicit_alpha(b: &[u8]) -> [u8; 16] {
    let bits = u64::from_le_bytes(b[..8].try_into().unwrap());
    std::array::from_fn(|i| ((bits >> (i * 4)) & 15) as u8 * 17)
}

fn interpolated_alpha(b: &[u8]) -> [u8; 16] {
    let (a0, a1) = (u32::from(b[0]), u32::from(b[1]));
    let mut table = [0u8; 8];
    table[0] = a0 as u8;
    table[1] = a1 as u8;
    for i in 2..8u32 {
        table[i as usize] = if a0 > a1 {
            ((a0 * (8 - i) + a1 * (i - 1)) / 7) as u8
        } else if i < 6 {
            ((a0 * (6 - i) + a1 * (i - 1)) / 5) as u8
        } else if i == 6 {
            0
        } else {
            255
        };
    }
    let mut bits = 0u64;
    for (i, byte) in b[2..8].iter().enumerate() {
        bits |= u64::from(*byte) << (8 * i);
    }
    std::array::from_fn(|i| table[((bits >> (i * 3)) & 7) as usize])
}

fn with_alpha(mut texels: [[u8; 4]; 16], alpha: [u8; 16]) -> [[u8; 4]; 16] {
    for (t, a) in texels.iter_mut().zip(alpha) {
        t[3] = a;
    }
    texels
}

/// Builds a BLP2 header + one mip (tests).
#[cfg(any(test, feature = "testing"))]
pub fn build(compression: u8, alpha_depth: u8, alpha_type: u8, w: u32, h: u32, palette: &[[u8; 4]], mip: &[u8]) -> Vec<u8> {
    let mut d = b"BLP2".to_vec();
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(&[compression, alpha_depth, alpha_type, 0]);
    d.extend_from_slice(&w.to_le_bytes());
    d.extend_from_slice(&h.to_le_bytes());
    let mut offsets = [0u32; 16];
    let mut sizes = [0u32; 16];
    offsets[0] = HEADER as u32;
    sizes[0] = mip.len() as u32;
    offsets.iter().chain(sizes.iter()).for_each(|v| d.extend_from_slice(&v.to_le_bytes()));
    let mut pal = vec![0u8; 1024];
    for (i, c) in palette.iter().enumerate() {
        pal[i * 4..i * 4 + 4].copy_from_slice(c); // stored BGRA
    }
    d.extend_from_slice(&pal);
    d.extend_from_slice(mip);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_dxt1_block() {
        // Pure red (0xF800) and pure blue (0x001F); texel 0 uses colour 0, texel 1 colour 1,
        // texel 2 the 2/3 red mix.
        let block = [0x00, 0xF8, 0x1F, 0x00, 0b0010_0100, 0, 0, 0];
        let blp = build(2, 0, 0, 4, 4, &[], &block);
        let (w, h, px) = decode(&blp).unwrap();
        assert_eq!((w, h), (4, 4));
        assert_eq!(&px[0..4], &[255, 0, 0, 255]);
        assert_eq!(&px[4..8], &[0, 0, 255, 255]);
        assert_eq!(&px[8..12], &[170, 0, 85, 255]);
    }

    #[test]
    fn decodes_dxt5_alpha() {
        let mut block = vec![255, 0, 0b0000_1000, 0, 0, 0, 0, 0]; // texel 0 -> a0, texel 1 -> a1
        block.extend_from_slice(&[0xFF, 0xFF, 0, 0, 0, 0, 0, 0]); // white
        let (_, _, px) = decode(&build(2, 8, 7, 4, 4, &[], &block)).unwrap();
        assert_eq!(px[3], 255);
        assert_eq!(px[7], 0);
        assert_eq!(&px[0..3], &[255, 255, 255]);
    }

    #[test]
    fn decodes_palette_with_8bit_alpha_and_writes_png() {
        // 2x1: palette 0 = green, 1 = blue (stored BGRA), then two alpha bytes.
        let blp = build(1, 8, 0, 2, 1, &[[0, 255, 0, 0], [255, 0, 0, 0]], &[0, 1, 128, 255]);
        let (_, _, px) = decode(&blp).unwrap();
        assert_eq!(px, vec![0, 255, 0, 128, 0, 0, 255, 255]);
        let png = blp_to_png(&blp).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn rejects_bad_textures() {
        assert!(decode(b"BLP2").is_err());
        assert!(decode(&build(2, 0, 0, 64, 64, &[], &[0; 8])).is_err(), "too little DXT data");
        assert!(decode(&build(2, 0, 0, 100_000, 1, &[], &[0; 8])).is_err());
    }
}
