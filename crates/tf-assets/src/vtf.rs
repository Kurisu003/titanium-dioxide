//! Valve Texture Format (VTF 7.x) as used for Titanfall 2's particle textures
//! (`materials/particle/...` in the VPKs): header, resources (7.3+), the high-res image and the
//! sprite sheet (resource 0x10) that SpriteCard materials animate through.
//!
//! Layout (Valve's `vtf.h`): "VTF\0", version major/minor, header size, width/height (u16),
//! flags (u32), frames (u16), first frame, reflectivity (at 32), bumpscale (48), format (52),
//! mip count (56), low-res format (57) and size (61, 62), depth (63); 7.3+ resource count at 68
//! and 8-byte resource entries (3-byte tag, flags, u32 offset) from 80. The high-res data (tag
//! 0x30) stores mips smallest first, each holding every frame.

use anyhow::{bail, Result};

/// One sprite-sheet sequence: frames as (duration, [u0, v0, u1, v1]).
#[derive(Clone, Debug, Default)]
pub struct Sequence {
    pub clamp: bool,
    pub total: f32,
    pub frames: Vec<(f32, [f32; 4])>,
}

impl Sequence {
    /// The frame rectangle at `t` (0..1 through the sequence).
    pub fn rect_at(&self, t: f32) -> [f32; 4] {
        if self.frames.is_empty() {
            return [0.0, 0.0, 1.0, 1.0];
        }
        let mut at = t.clamp(0.0, 0.9999) * self.total.max(1e-6);
        for (d, r) in &self.frames {
            if at < *d {
                return *r;
            }
            at -= d;
        }
        self.frames.last().unwrap().1
    }
}

pub struct Vtf {
    pub width: u32,
    pub height: u32,
    /// RGBA8 (straight alpha) of the decoded mip.
    pub rgba: Vec<u8>,
    pub sheet: Vec<Sequence>,
    /// The file's format code (8 = A8, 13 = DXT1, 15 = DXT5, ...).
    pub format: i32,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// (block width, bytes per block) of a format; uncompressed formats are 1-pixel "blocks".
fn layout(format: i32) -> Option<(u32, usize)> {
    Some(match format {
        0 | 1 | 11 | 12 | 16 => (1, 4),
        2 | 3 | 9 | 10 => (1, 3),
        4 | 17 | 18 | 19 | 6 => (1, 2),
        5 | 8 => (1, 1),
        13 | 20 => (4, 8),
        14 | 15 => (4, 16),
        _ => return None,
    })
}

fn mip_bytes(format: i32, w: u32, h: u32) -> usize {
    let (b, bytes) = layout(format).unwrap_or((1, 4));
    (w.div_ceil(b).max(1) * h.div_ceil(b).max(1)) as usize * bytes
}

/// Decode a VTF, picking the largest mip no bigger than `max_size` on either side.
pub fn decode(b: &[u8], max_size: u32) -> Result<Vtf> {
    if b.len() < 80 || &b[0..4] != b"VTF\0" {
        bail!("not a VTF");
    }
    let (major, minor) = (u32_at(b, 4), u32_at(b, 8));
    let header = u32_at(b, 12) as usize;
    let (w, h) = (u16_at(b, 16) as u32, u16_at(b, 18) as u32);
    let frames = u16_at(b, 24).max(1) as usize;
    let format = u32_at(b, 52) as i32;
    let mips = b[56] as u32;
    let low_format = u32_at(b, 57) as i32;
    let (low_w, low_h) = (b[61] as u32, b[62] as u32);
    let Some(_) = layout(format) else { bail!("unsupported VTF format {format}") };

    let mut hi = None;
    let mut sheet = Vec::new();
    if major == 7 && minor >= 3 {
        let n = u32_at(b, 68) as usize;
        for i in 0..n.min(32) {
            let e = 80 + i * 8;
            let tag = &b[e..e + 3];
            let off = u32_at(b, e + 4) as usize;
            match tag {
                [0x30, 0, 0] => hi = Some(off),
                [0x10, 0, 0] if off + 12 <= b.len() => sheet = parse_sheet(b, off + 4),
                _ => {}
            }
        }
    }
    let hi = hi.unwrap_or_else(|| header + if low_format >= 0 && low_w > 0 { mip_bytes(low_format, low_w, low_h) } else { 0 });

    let mut mip = 0;
    while mip + 1 < mips && (w >> mip).max(h >> mip) > max_size {
        mip += 1;
    }
    let size = |m: u32| mip_bytes(format, (w >> m).max(1), (h >> m).max(1));
    let skip: usize = (mip + 1..mips).map(|m| size(m) * frames).sum();
    let at = hi + skip;
    let (mw, mh) = ((w >> mip).max(1), (h >> mip).max(1));
    let data = b.get(at..at + size(mip)).ok_or_else(|| anyhow::anyhow!("VTF data out of range"))?;
    Ok(Vtf { width: mw, height: mh, rgba: to_rgba(format, data, mw, mh), sheet, format })
}

fn parse_sheet(b: &[u8], o: usize) -> Vec<Sequence> {
    let version = u32_at(b, o);
    let count = u32_at(b, o + 4) as usize;
    let coords = if version == 0 { 1 } else { 4 };
    let mut p = o + 8;
    let mut out = Vec::new();
    for _ in 0..count.min(256) {
        if p + 16 > b.len() {
            break;
        }
        let seq = u32_at(b, p) as usize;
        let clamp = u32_at(b, p + 4) != 0;
        let n = u32_at(b, p + 8) as usize;
        let total = f32_at(b, p + 12);
        p += 16;
        let mut frames = Vec::with_capacity(n);
        for _ in 0..n.min(1024) {
            if p + 4 + 16 * coords > b.len() {
                break;
            }
            let d = f32_at(b, p);
            let r = [f32_at(b, p + 4), f32_at(b, p + 8), f32_at(b, p + 12), f32_at(b, p + 16)];
            frames.push((d, r));
            p += 4 + 16 * coords;
        }
        if out.len() <= seq {
            out.resize(seq + 1, Sequence::default());
        }
        out[seq] = Sequence { clamp, total, frames };
    }
    out
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let bl = (c & 31) as u32;
    [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (bl * 255 / 31) as u8]
}

/// BC1 colour block into 16 RGBA pixels. `alpha` = whether the 3-colour mode is transparent.
fn bc1_block(blk: &[u8], out: &mut [[u8; 4]; 16], alpha: bool) {
    let (c0, c1) = (u16_at(blk, 0), u16_at(blk, 2));
    let (a, b) = (rgb565(c0), rgb565(c1));
    let mix = |x: u8, y: u8, wx: u32, wy: u32| ((x as u32 * wx + y as u32 * wy) / (wx + wy)) as u8;
    let mut pal = [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], [0; 4], [0; 4]];
    if c0 > c1 || !alpha {
        pal[2] = [mix(a[0], b[0], 2, 1), mix(a[1], b[1], 2, 1), mix(a[2], b[2], 2, 1), 255];
        pal[3] = [mix(a[0], b[0], 1, 2), mix(a[1], b[1], 1, 2), mix(a[2], b[2], 1, 2), 255];
    } else {
        pal[2] = [mix(a[0], b[0], 1, 1), mix(a[1], b[1], 1, 1), mix(a[2], b[2], 1, 1), 255];
        pal[3] = [0, 0, 0, 0];
    }
    let bits = u32_at(blk, 4);
    for (i, px) in out.iter_mut().enumerate() {
        *px = pal[((bits >> (i * 2)) & 3) as usize];
    }
}

fn bc3_alpha(blk: &[u8]) -> [u8; 16] {
    let (a0, a1) = (blk[0] as u32, blk[1] as u32);
    let mut pal = [0u32; 8];
    pal[0] = a0;
    pal[1] = a1;
    if a0 > a1 {
        for i in 1..7 {
            pal[i + 1] = ((7 - i as u32) * a0 + i as u32 * a1) / 7;
        }
    } else {
        for i in 1..5 {
            pal[i + 1] = ((5 - i as u32) * a0 + i as u32 * a1) / 5;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    let mut bits = 0u64;
    for i in 0..6 {
        bits |= (blk[2 + i] as u64) << (8 * i);
    }
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = pal[((bits >> (3 * i)) & 7) as usize] as u8;
    }
    out
}

fn to_rgba(format: i32, d: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![0u8; w * h * 4];
    match format {
        13 | 20 | 14 | 15 => {
            let bytes = if format == 13 || format == 20 { 8 } else { 16 };
            let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
            for by in 0..bh {
                for bx in 0..bw {
                    let blk = &d[(by * bw + bx) * bytes..][..bytes];
                    let mut px = [[0u8; 4]; 16];
                    match format {
                        15 => {
                            bc1_block(&blk[8..], &mut px, false);
                            let a = bc3_alpha(blk);
                            for i in 0..16 {
                                px[i][3] = a[i];
                            }
                        }
                        14 => {
                            bc1_block(&blk[8..], &mut px, false);
                            for (i, p) in px.iter_mut().enumerate() {
                                let nib = (blk[i / 2] >> ((i % 2) * 4)) & 15;
                                p[3] = nib * 17;
                            }
                        }
                        _ => bc1_block(blk, &mut px, format == 20),
                    }
                    for (i, p) in px.iter().enumerate() {
                        let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                        if x < w && y < h {
                            out[(y * w + x) * 4..][..4].copy_from_slice(p);
                        }
                    }
                }
            }
        }
        _ => {
            for i in 0..w * h {
                let px: [u8; 4] = match format {
                    0 => [d[i * 4], d[i * 4 + 1], d[i * 4 + 2], d[i * 4 + 3]],
                    1 => [d[i * 4 + 3], d[i * 4 + 2], d[i * 4 + 1], d[i * 4]],
                    11 => [d[i * 4 + 1], d[i * 4 + 2], d[i * 4 + 3], d[i * 4]],
                    12 | 16 => [d[i * 4 + 2], d[i * 4 + 1], d[i * 4], d[i * 4 + 3]],
                    2 | 9 => [d[i * 3], d[i * 3 + 1], d[i * 3 + 2], 255],
                    3 | 10 => [d[i * 3 + 2], d[i * 3 + 1], d[i * 3], 255],
                    4 => {
                        let c = rgb565(u16_at(d, i * 2));
                        [c[0], c[1], c[2], 255]
                    }
                    17 => {
                        let c = rgb565(u16_at(d, i * 2));
                        [c[2], c[1], c[0], 255]
                    }
                    5 => [d[i], d[i], d[i], 255],
                    6 => [d[i * 2], d[i * 2], d[i * 2], d[i * 2 + 1]],
                    8 => [255, 255, 255, d[i]],
                    _ => [255, 0, 255, 255],
                };
                out[i * 4..i * 4 + 4].copy_from_slice(&px);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_rect_walks_frames() {
        let s = Sequence { clamp: true, total: 2.0, frames: vec![(1.0, [0.0, 0.0, 0.5, 1.0]), (1.0, [0.5, 0.0, 1.0, 1.0])] };
        assert_eq!(s.rect_at(0.2)[0], 0.0);
        assert_eq!(s.rect_at(0.7)[0], 0.5);
    }

    #[test]
    fn bc1_solid_block() {
        // c0 = c1 = pure red, all indices 0.
        let blk = [0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0];
        let mut px = [[0u8; 4]; 16];
        bc1_block(&blk, &mut px, false);
        assert_eq!(px[5], [255, 0, 0, 255]);
    }
}
