//! `txtr` (version 8) texture assets.

use crate::rpak::{Asset, Rpak};
use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexFormat {
    Bc1,
    Bc1Srgb,
    Bc2,
    Bc2Srgb,
    Bc3,
    Bc3Srgb,
    Bc4,
    Bc4Snorm,
    Bc5,
    Bc5Snorm,
    Bc6hUf16,
    Bc6hSf16,
    Bc7,
    Bc7Srgb,
    Rgba16Float,
    Rgba8,
    Rgba8Srgb,
    R8,
    Other(u16),
}

impl TexFormat {
    fn from_raw(v: u16) -> Self {
        use TexFormat::*;
        match v {
            0 => Bc1,
            1 => Bc1Srgb,
            2 => Bc2,
            3 => Bc2Srgb,
            4 => Bc3,
            5 => Bc3Srgb,
            6 => Bc4,
            7 => Bc4Snorm,
            8 => Bc5,
            9 => Bc5Snorm,
            10 => Bc6hUf16,
            11 => Bc6hSf16,
            12 => Bc7,
            13 => Bc7Srgb,
            20 => Rgba16Float,
            31 => Rgba8,
            32 => Rgba8Srgb,
            53 => R8,
            o => Other(o),
        }
    }

    /// (bytes per block, block width, block height)
    pub fn block(self) -> Option<(usize, usize, usize)> {
        use TexFormat::*;
        Some(match self {
            Bc1 | Bc1Srgb | Bc4 | Bc4Snorm => (8, 4, 4),
            Bc2 | Bc2Srgb | Bc3 | Bc3Srgb | Bc5 | Bc5Snorm | Bc6hUf16 | Bc6hSf16 | Bc7 | Bc7Srgb => (16, 4, 4),
            Rgba16Float => (8, 1, 1),
            Rgba8 | Rgba8Srgb => (4, 1, 1),
            R8 => (1, 1, 1),
            Other(_) => return None,
        })
    }

    pub fn mip_size(self, w: usize, h: usize) -> usize {
        let (b, bw, bh) = self.block().unwrap_or((4, 1, 1));
        w.div_ceil(bw) * h.div_ceil(bh) * b
    }
}

#[derive(Debug, Clone)]
pub struct TextureInfo {
    pub guid: u64,
    pub name: Option<String>,
    pub width: u32,
    pub height: u32,
    pub format: TexFormat,
    pub array_size: u32,
    pub permanent_mips: u32,
    pub streamed_mips: u32,
}

#[derive(Debug, Clone)]
pub struct Texture {
    pub info: TextureInfo,
    /// Mip chain, largest first, tightly packed (no alignment padding), `mips` levels.
    pub data: Vec<u8>,
    pub mips: u32,
    pub width: u32,
    pub height: u32,
}

const MIP_ALIGN: usize = 16;

pub fn info(pak: &Rpak, a: &Asset) -> Result<TextureInfo> {
    let h = a.head.context("texture has no header")?;
    Ok(TextureInfo {
        guid: pak.u64(h),
        name: pak.ptr(h + 8).map(|p| pak.cstr(p)),
        width: pak.u16(h + 0x10) as u32,
        height: pak.u16(h + 0x12) as u32,
        format: TexFormat::from_raw(pak.u16(h + 0x16)),
        array_size: (pak.u8(h + 0x1E) as u32).max(1),
        permanent_mips: pak.u8(h + 0x21) as u32,
        streamed_mips: pak.u8(h + 0x22) as u32,
    })
}

/// Load a texture's full mip chain (streamed mips come from the starpak).
/// If the starpak is unavailable, falls back to the permanent mips only.
pub fn load(pak: &Rpak, a: &Asset) -> Result<Texture> {
    load_inner(pak, a, true)
}

/// Load at most `max` texels wide/high: the starpak is only read when the permanent mips are
/// smaller than that, and larger levels are dropped (for small CPU-side copies).
pub fn load_max(pak: &Rpak, a: &Asset, max: u32) -> Result<Texture> {
    let i = info(pak, a)?;
    let permanent_top = (i.width >> i.streamed_mips).max(1);
    let mut t = load_inner(pak, a, permanent_top < max.min(i.width))?;
    while t.mips > 1 && (t.width > max || t.height > max) {
        let n = t.info.format.mip_size(t.width as usize, t.height as usize);
        t.data.drain(..n);
        t.mips -= 1;
        t.width = (t.width / 2).max(1);
        t.height = (t.height / 2).max(1);
    }
    Ok(t)
}

/// Decode the top level of a BC1 image to RGB bytes (values as stored, no sRGB conversion).
pub fn decode_bc1(data: &[u8], w: usize, h: usize) -> Vec<[u8; 3]> {
    let mut out = vec![[0u8; 3]; w * h];
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let rgb = |c: u16| [((c >> 11) & 31) as u32 * 255 / 31, ((c >> 5) & 63) as u32 * 255 / 63, (c & 31) as u32 * 255 / 31];
    for by in 0..bh {
        for bx in 0..bw {
            let o = (by * bw + bx) * 8;
            let Some(b) = data.get(o..o + 8) else { return out };
            let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
            let (p0, p1) = (rgb(c0), rgb(c1));
            let mix = |a: u32, b: u32, n: u32, d: u32| ((a * (d - n) + b * n) / d) as u8;
            let pal: [[u8; 3]; 4] = if c0 > c1 {
                [p0.map(|v| v as u8), p1.map(|v| v as u8), std::array::from_fn(|i| mix(p0[i], p1[i], 1, 3)), std::array::from_fn(|i| mix(p0[i], p1[i], 2, 3))]
            } else {
                [p0.map(|v| v as u8), p1.map(|v| v as u8), std::array::from_fn(|i| mix(p0[i], p1[i], 1, 2)), [0; 3]]
            };
            let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            for k in 0..16 {
                let (x, y) = (bx * 4 + k % 4, by * 4 + k / 4);
                if x < w && y < h {
                    out[y * w + x] = pal[((bits >> (2 * k)) & 3) as usize];
                }
            }
        }
    }
    out
}

/// Decode the top level of a BC4 image to one byte per texel.
pub fn decode_bc4(data: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    for by in 0..bh {
        for bx in 0..bw {
            let o = (by * bw + bx) * 8;
            let Some(b) = data.get(o..o + 8) else { return out };
            let (r0, r1) = (b[0] as u32, b[1] as u32);
            let mut pal = [0u8; 8];
            pal[0] = r0 as u8;
            pal[1] = r1 as u8;
            for i in 1..7u32 {
                pal[i as usize + 1] = if r0 > r1 {
                    ((r0 * (7 - i) + r1 * i) / 7) as u8
                } else if i < 5 {
                    ((r0 * (5 - i) + r1 * i) / 5) as u8
                } else if i == 5 {
                    0
                } else {
                    255
                };
            }
            let bits = u64::from_le_bytes([b[2], b[3], b[4], b[5], b[6], b[7], 0, 0]);
            for k in 0..16 {
                let (x, y) = (bx * 4 + k % 4, by * 4 + k / 4);
                if x < w && y < h {
                    out[y * w + x] = pal[((bits >> (3 * k)) & 7) as usize];
                }
            }
        }
    }
    out
}

/// Decode the top level of a BC5 (unsigned) image to two bytes per texel: two BC4 channels
/// per 16-byte block.
pub fn decode_bc5(data: &[u8], w: usize, h: usize) -> Vec<[u8; 2]> {
    let blocks = w.div_ceil(4) * h.div_ceil(4);
    let (mut r, mut g) = (Vec::with_capacity(blocks * 8), Vec::with_capacity(blocks * 8));
    for b in data.chunks_exact(16).take(blocks) {
        r.extend_from_slice(&b[..8]);
        g.extend_from_slice(&b[8..]);
    }
    decode_bc4(&r, w, h).into_iter().zip(decode_bc4(&g, w, h)).map(|(r, g)| [r, g]).collect()
}

fn load_inner(pak: &Rpak, a: &Asset, starpak: bool) -> Result<Texture> {
    let info = info(pak, a)?;
    if info.format.block().is_none() {
        bail!("unsupported texture format {:?}", info.format);
    }
    let total = info.permanent_mips + info.streamed_mips;
    let level_size = |i: u32| {
        let w = (info.width >> i).max(1) as usize;
        let h = (info.height >> i).max(1) as usize;
        info.format.mip_size(w, h)
    };
    let aligned = |n: usize| n.div_ceil(MIP_ALIGN) * MIP_ALIGN;
    let arr = info.array_size as usize;

    // Mips are stored smallest first, each padded to 16 bytes (times the array size).
    let mut levels: Vec<Option<Vec<u8>>> = vec![None; total as usize];
    if let Some(cpu) = a.cpu {
        let mut off = cpu;
        for i in (info.streamed_mips..total).rev() {
            let n = level_size(i);
            levels[i as usize] = Some(pak.data[off..off + n].to_vec());
            off += aligned(n) * arr;
        }
    }
    if starpak && info.streamed_mips > 0 && a.starpak_offset >= 0 {
        let sizes: Vec<usize> = (0..info.streamed_mips).map(|i| aligned(level_size(i)) * arr).collect();
        let total_size: usize = sizes.iter().sum();
        if let Ok(buf) = pak.read_starpak(a.starpak_offset, total_size) {
            let mut off = 0;
            for i in (0..info.streamed_mips).rev() {
                let n = level_size(i);
                levels[i as usize] = Some(buf[off..off + n].to_vec());
                off += sizes[i as usize];
            }
        }
    }

    let first = levels.iter().position(|l| l.is_some()).context("texture has no mip data")?;
    let mut data = Vec::new();
    let mut mips = 0;
    for l in &levels[first..] {
        match l {
            Some(d) => {
                data.extend_from_slice(d);
                mips += 1;
            }
            None => break,
        }
    }
    // GPU block-compressed textures need the top level to be at least one block; drop
    // trailing levels smaller than that only if the format requires it (wgpu accepts them).
    Ok(Texture {
        width: (info.width >> first).max(1),
        height: (info.height >> first).max(1),
        info,
        data,
        mips,
    })
}
