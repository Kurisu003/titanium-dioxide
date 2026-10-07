// SPDX-License-Identifier: AGPL-3.0-only
// Port of the RPAK decompression routine published by r-ex/rsx (https://github.com/r-ex/rsx), AGPLv3.
//! The RTech LZ-style decompressor used by Titanfall 2 RPAK files (header flag 0x100).
//!
//! This is a straight port of the reverse-engineered routine from the game (as documented
//! by the r-ex/rsx project). Variable names follow the decompiled original where a better
//! name isn't known; the control flow is restructured into a loop, without gotos.

use crate::rpak_lut::LUT;
use anyhow::{bail, Result};

fn lut_u32(idx: usize) -> u32 {
    u32::from_le_bytes(LUT[idx * 4..idx * 4 + 4].try_into().unwrap())
}

/// Bit-level decoder state (`PakDecompressContext_t` in rsx).
struct Ctx {
    decomp_size: u64,
    input_inv_mask: u64,
    output_inv_mask: u64,
    header_offset: u32,
    file_pos: u64,
    decomp_pos: u64,
    buffer_size_needed: u64,
    current_byte: u64,
    current_bit: u32,
    dword6c: u32,
    qword70: u64,
    compressed_stream_size: u64,
    decomp_stream_size: u64,
}

struct Input<'a>(&'a [u8]);
impl Input<'_> {
    #[inline(always)]
    fn u64(&self, pos: u64) -> u64 {
        let p = pos as usize;
        if p + 8 <= self.0.len() {
            u64::from_le_bytes(self.0[p..p + 8].try_into().unwrap())
        } else {
            let mut b = [0u8; 8];
            for (i, v) in b.iter_mut().enumerate() {
                *v = *self.0.get(p + i).unwrap_or(&0);
            }
            u64::from_le_bytes(b)
        }
    }
}

fn low_mask(bits: u64) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

fn init(input: &Input, data_size: u64, header_size: u64) -> Ctx {
    let data_offset = 0u64;
    let mut v7 = input.u64(data_offset + header_size);
    let unk_pos = data_offset + header_size + 8;
    let mut v8 = (v7 & 0x3F) as u32;
    v7 >>= 6;
    let decomp_size = (v7 & low_mask(v8 as u64)) | (1u64 << v8);
    let v9 = (v7 >> v8) | input.u64(unk_pos).wrapping_shl(64 - (v8 + 6));
    let v10 = unk_pos + ((v8 + 6) >> 3) as u64;
    v8 = (v8 + 6) & 7;
    let v11 = v8 + 13;
    let v12 = (u64::MAX >> v8) & v9;
    let v13 = ((((v12 as u8) as u32).wrapping_sub(1)) & 0x3F) + 1;
    let v14 = u64::MAX.wrapping_shr(64 - v13);
    let input_inv_mask = if v13 == 64 { u64::MAX } else { v14 };
    let v15 = v10 + (v11 >> 3) as u64;
    let output_inv_mask = u64::MAX >> (63 - (((v12 >> 6).wrapping_sub(1)) & 0x3F));
    let v16 = (v12 >> 13) | input.u64(v10).wrapping_shl(64 - v11);
    let v17 = v11 & 7;
    let v18 = (u64::MAX >> v17) & v16;
    let mut file_pos = v15;
    let (header_offset, v21) = if input_inv_mask == u64::MAX {
        (0u32, data_size)
    } else {
        let v19 = v13 >> 3;
        let v20 = input.u64(v15);
        file_pos = v15 + v19 as u64 + 1;
        (v19 + 1, v20 & low_mask(8 * (v19 as u64 + 1)))
    };
    let mut ctx = Ctx {
        decomp_size,
        input_inv_mask,
        output_inv_mask,
        header_offset,
        file_pos,
        decomp_pos: header_size,
        buffer_size_needed: v21 + data_offset,
        current_byte: v18,
        current_bit: v17,
        dword6c: 0,
        qword70: input_inv_mask.wrapping_add(data_offset).wrapping_sub(6),
        compressed_stream_size: v21 + data_offset,
        decomp_stream_size: decomp_size,
    };
    if decomp_size - 1 > output_inv_mask {
        ctx.decomp_stream_size = output_inv_mask + 1;
        ctx.compressed_stream_size = v21 + data_offset - header_offset as u64;
    }
    ctx
}

/// Returns true if the RPAK header flags say the body is RTech-compressed.
pub fn is_rtech_compressed(flags: u16) -> bool {
    flags & 0x100 != 0
}

/// Decompress a whole RPAK file. `header_size` bytes at the start are copied verbatim.
pub fn decompress_pak(file: &[u8], header_size: usize) -> Result<Vec<u8>> {
    let input = Input(file);
    let mut c = init(&input, file.len() as u64, header_size as u64);
    let in_len = file.len() as u64;
    let out_len = c.decomp_size;
    if in_len < c.buffer_size_needed {
        bail!("rpak: compressed stream larger than file");
    }
    // Writes happen in 8/16 byte chunks and may overrun the logical end slightly.
    let mut out = vec![0u8; out_len as usize + 64];

    let mut v5 = c.decomp_pos;
    let mut v7 = c.current_bit;
    let mut v9 = c.file_pos;
    let mut v10 = c.qword70.min(c.compressed_stream_size);
    let mut v71 = c.dword6c;
    let mut v8;
    let mut v19: u64;

    if v7 != 0 {
        let v13 = input.u64(v9).wrapping_shl(64 - v7) | c.current_byte;
        let i = v7;
        v7 &= 7;
        v9 += (i >> 3) as u64;
        v8 = (u64::MAX >> v7) & v13;
    } else {
        v8 = c.current_byte;
    }

    loop {
        // LABEL_11: decode one token.
        let v12 = v71;
        let v15 = (v12 as usize) << 8;
        let v16 = v12 as usize;
        let v17 = LUT[(v8 as u8) as usize + v15 + 512] as u32;
        let v18 = (v8 as u8) as usize + v15;
        v7 += v17;
        v19 = v8 >> v17;
        let token = LUT[v18] as i8;
        if token < 0 {
            // Literal run.
            let mut v56 = (-(token as i32)) as u32;
            v71 = 1;
            if v56 == LUT[v16 + 1248] as u32 {
                if (!v9 & c.input_inv_mask) < 0xF
                    || (c.output_inv_mask & !v5) < 15
                    || c.decomp_size - v5 < 0x10
                {
                    v56 = 1;
                }
                let v60 = v19 >> 3;
                let v61 = (v19 as u8 & 7) as usize;
                let mut v62 = v60;
                let (v63, v64);
                if v61 != 0 {
                    v63 = LUT[v61 + 1232] as u32;
                    v64 = LUT[v61 + 1240] as u32;
                } else {
                    v62 = v60 >> 4;
                    let v65 = (v60 & 0xF) as usize;
                    v7 += 4;
                    v63 = lut_u32(v65 + 288);
                    v64 = LUT[v65 + 1216] as u32;
                }
                v7 += v64 + 3;
                v19 = v62 >> v64;
                let v66 = v63 + (v62 & low_mask(v64 as u64)) as u32 + v56;
                let (s, d, n) = (v9 as usize, v5 as usize, v66 as usize);
                copy_in(&mut out, d, file, s, n);
                v9 += v66 as u64;
                v5 += v66 as u64;
            } else {
                copy_in(&mut out, v5 as usize, file, v9 as usize, 16);
                v9 += v56 as u64;
                v5 += v56 as u64;
            }
        } else {
            // Back-reference.
            let v20 = token as u32;
            let v21 = (v19 & 0xF) as u32;
            v71 = 0;
            let s = ((v21.wrapping_sub(31)) >> 3) & 6;
            let v22 = (((v19 as u32) as u64) >> s) & 0x3F;
            let t = ((v19 >> 4) & (((24 * (((v21.wrapping_sub(31)) >> 3) & 2)) >> 4) as u64)) as u32;
            let v23 = 1u32 << (v21 + t);
            let lut_a = LUT[v22 as usize + 1088] as u32;
            v7 += s + lut_a + v21 + t;
            let v25 = 16u32.wrapping_mul(
                v23.wrapping_add(((v23 - 1) as u64 & (v19 >> (s + lut_a))) as u32),
            );
            v19 >>= s + lut_a + v21 + t;
            let v26 = v25.wrapping_add(LUT[v22 as usize + 1024] as u32).wrapping_sub(16);
            let src = v5.wrapping_sub(v26 as u64) as usize;
            let dst = v5 as usize;
            if v20 == 17 {
                let v41 = v19 >> 3;
                let v42 = (v19 as u8 & 7) as usize;
                let mut v43 = v41;
                let (v44, v45);
                if v42 != 0 {
                    v44 = LUT[v42 + 1232] as u32;
                    v45 = LUT[v42 + 1240] as u32;
                } else {
                    v7 += 4;
                    let v46 = (v41 & 0xF) as usize;
                    v43 = v41 >> 4;
                    v44 = lut_u32(v46 + 288);
                    v45 = LUT[v46 + 1216] as u32;
                    if v7 + v45 >= 61 {
                        v43 |= (*file.get(v9 as usize).unwrap_or(&0) as u64) << (61 - v7);
                        v9 += 1;
                        v7 -= 8;
                    }
                }
                v7 += v45 + 3;
                v19 = v43 >> v45;
                let v48 = ((v43 as u32) & (low_mask(v45 as u64) as u32)) as u64 + v44 as u64 + 17;
                v5 += v48;
                if v26 < 8 {
                    let v50 = (v48 - 13) as usize;
                    v5 -= 13;
                    if v26 == 1 {
                        let b = out[src];
                        let n = v50.div_ceil(8) * 8;
                        out[dst..dst + n].fill(b);
                    } else {
                        for k in 0..v50 {
                            out[dst + k] = out[src + k];
                        }
                    }
                } else {
                    let mut l = 0usize;
                    while (l as u64) < v48 {
                        out.copy_within(src + l..src + l + 8, dst + l);
                        l += 8;
                    }
                }
            } else {
                v5 += v20 as u64;
                out.copy_within(src..src + 8, dst);
                out.copy_within(src + 8..src + 16, dst + 8);
            }
        }

        if v9 >= v10 {
            // End of a block (or of the stream).
            let mut go_label25 = true;
            if v5 == c.decomp_stream_size {
                let v30 = c.decomp_size;
                if v5 == v30 {
                    break;
                }
                let v31 = c.input_inv_mask;
                let v32 = c.header_offset as u64;
                let v33 = v31 & (v9 as i64).wrapping_neg() as u64;
                v19 >>= 1;
                v7 += 1;
                if v32 > v33 {
                    v9 += v33;
                    let v34 = c.qword70;
                    if v9 > v34 {
                        c.qword70 = v31.wrapping_add(v34).wrapping_add(1);
                    }
                }
                let v35 = v9;
                v9 += v32;
                let mut v36 = v5 + c.output_inv_mask + 1;
                let v37 = input.u64(v35) & low_mask(8 * v32);
                let v38 = v37 + c.buffer_size_needed;
                let v39 = v37 + c.compressed_stream_size;
                c.buffer_size_needed = v38;
                c.compressed_stream_size = v39;
                if v36 >= v30 {
                    v36 = v30;
                    c.compressed_stream_size = v32 + v39;
                }
                c.decomp_stream_size = v36;
                if !(in_len >= v38 && out_len >= v36) {
                    go_label25 = false;
                }
            }
            if !go_label25 {
                bail!("rpak: decompressor ran out of input (corrupt or truncated file)");
            }
            // LABEL_25
            v10 = c.qword70;
            if v9 >= v10 {
                v9 = !c.input_inv_mask & (v9 + 7);
                v10 = v10.wrapping_add(c.input_inv_mask).wrapping_add(1);
                c.qword70 = v10;
            }
            if c.compressed_stream_size < v10 {
                v10 = c.compressed_stream_size;
            }
        }

        // LABEL_29: refill the bit buffer.
        let v13 = input.u64(v9).wrapping_shl(64 - v7) | v19;
        let i = v7;
        v7 &= 7;
        v9 += (i >> 3) as u64;
        v8 = (u64::MAX >> v7) & v13;
    }

    let _ = v19;
    out.truncate(out_len as usize);
    out[..header_size].copy_from_slice(&file[..header_size]);
    Ok(out)
}

fn copy_in(out: &mut [u8], dst: usize, src: &[u8], s: usize, n: usize) {
    let avail = src.len().saturating_sub(s).min(n);
    out[dst..dst + avail].copy_from_slice(&src[s..s + avail]);
    out[dst + avail..dst + n].fill(0);
}
