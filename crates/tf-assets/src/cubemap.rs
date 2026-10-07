//! A map's baked cubemaps: `materials/maps/<map>/cubemaps.hdr.vtf` in the BSP's PAKFILE.
//!
//! Titanfall 2 stores them as one VTF 7.5 with a frame per cubemap (in CUBEMAPS lump order) and
//! six faces per frame, in BC6H (VTF format 66, which stock Source doesn't have). Face order and
//! orientation are Direct3D's (+X, -X, +Y, -Y, +Z, -Z) indexed by game-space directions.
//! Image data runs from the smallest mip to the largest; within a mip, frame then face.

use crate::bc6h;
use anyhow::{bail, Context, Result};

pub struct CubemapSet {
    /// Face width/height of mip 0.
    pub size: u32,
    pub mips: u32,
    pub count: usize,
    data: Vec<u8>,
}

const VTF_FORMAT_BC6H: i32 = 66;

fn face_bytes(size: u32, mip: u32) -> usize {
    let s = (size >> mip).max(1) as usize;
    s.div_ceil(4) * s.div_ceil(4) * 16
}

impl CubemapSet {
    pub fn parse(vtf: &[u8]) -> Result<Self> {
        if vtf.len() < 80 || &vtf[0..4] != b"VTF\0" {
            bail!("not a VTF");
        }
        let u32_at = |o: usize| u32::from_le_bytes(vtf[o..o + 4].try_into().unwrap());
        let u16_at = |o: usize| u16::from_le_bytes([vtf[o], vtf[o + 1]]);
        let minor = u32_at(8);
        let header = u32_at(12) as usize;
        let (w, h) = (u16_at(16) as u32, u16_at(18) as u32);
        let frames = u16_at(24) as usize;
        let format = i32::from_le_bytes(vtf[52..56].try_into().unwrap());
        let mips = vtf[56] as u32;
        if format != VTF_FORMAT_BC6H || w != h || w == 0 {
            bail!("unsupported cubemap VTF (format {format}, {w}x{h})");
        }
        // 7.3+: a resource directory after the header names the image data's offset.
        let mut offset = header;
        if minor >= 3 {
            let n = u32_at(68) as usize;
            for i in 0..n {
                let e = 80 + i * 8;
                if e + 8 <= vtf.len() && vtf[e..e + 3] == [0x30, 0, 0] {
                    offset = u32_at(e + 4) as usize;
                }
            }
        }
        let total: usize = (0..mips).map(|m| face_bytes(w, m) * 6 * frames).sum();
        let data = vtf.get(offset..offset + total).context("cubemap VTF is truncated")?.to_vec();
        Ok(Self { size: w, mips, count: frames, data })
    }

    /// One face of one cubemap at `mip` (0 = largest), as half-float RGB texels, row-major.
    pub fn face(&self, frame: usize, face: usize, mip: u32) -> Vec<[u16; 3]> {
        let mut o = 0usize;
        for m in (mip + 1..self.mips).rev() {
            o += face_bytes(self.size, m) * 6 * self.count;
        }
        let fb = face_bytes(self.size, mip);
        o += (frame * 6 + face) * fb;
        let s = (self.size >> mip).max(1) as usize;
        let bw = s.div_ceil(4);
        let mut out = vec![[0u16; 3]; s * s];
        let mut block = [[0u16; 3]; 16];
        for (bi, chunk) in self.data[o..o + fb].chunks_exact(16).enumerate() {
            bc6h::decode_block(chunk, &mut block, false);
            let (bx, by) = (bi % bw * 4, bi / bw * 4);
            for k in 0..16 {
                let (x, y) = (bx + k % 4, by + k / 4);
                if x < s && y < s {
                    out[y * s + x] = block[k];
                }
            }
        }
        out
    }
}
