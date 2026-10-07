//! Bink Audio ("1FCB", as stored by Miles in Titanfall 2's stream banks) to PCM.
//!
//! A Rust port of FFmpeg's Bink Audio DCT decoder (libavcodec/binkaudio.c) and binka demuxer
//! (libavformat/binka.c), LGPL-2.1-or-later, (c) 2007-2011 Peter Ross, Daniel Verkamp.

use anyhow::{bail, Result};
use rustdct::DctPlanner;

const MAX_CHANNELS_PER_BLOCK: usize = 2;
const MAX_DCT_CHANNELS: usize = 6;

const RLE_LENGTHS: [usize; 16] = [2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15, 16, 32, 64];

const CRITICAL_FREQS: [u32; 25] = [
    100, 200, 300, 400, 510, 630, 770, 920, 1080, 1270, 1480, 1720, 2000, 2320, 2700, 3150, 3700, 4400, 5300, 6400, 7700, 9500, 12000, 15500,
    24500,
];

/// Decoded audio: interleaved f32 samples.
pub struct Pcm {
    pub channels: usize,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

/// Little-endian bit reader (bits come out least significant first).
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn left(&self) -> isize {
        (self.data.len() * 8) as isize - self.pos as isize
    }
    fn get(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        for i in 0..n {
            let byte = self.data.get(self.pos >> 3).copied().unwrap_or(0);
            v |= (((byte >> (self.pos & 7)) & 1) as u32) << i;
            self.pos += 1;
        }
        v
    }
    fn float(&mut self) -> f32 {
        let power = self.get(5) as i32;
        let f = self.get(23) as f32 * 2f32.powi(power - 23);
        if self.get(1) == 1 { -f } else { f }
    }
    fn align32(&mut self) {
        let n = self.pos.wrapping_neg() & 31;
        self.pos += n;
    }
}

struct Decoder {
    frame_len: usize,
    overlap_len: usize,
    num_bands: usize,
    root: f32,
    bands: [usize; 26],
    quant_table: [f32; 96],
    previous: Vec<Vec<f32>>,
    first: bool,
    dct: std::sync::Arc<dyn rustdct::TransformType2And3<f32>>,
}

impl Decoder {
    fn new(sample_rate: u32, channels: usize) -> Self {
        let frame_len_bits = if sample_rate < 22050 { 9 } else if sample_rate < 44100 { 10 } else { 11 };
        let frame_len = 1usize << frame_len_bits;
        let overlap_len = frame_len / 16;
        let half_rate = sample_rate.div_ceil(2);
        let root = frame_len as f32 / ((frame_len as f32).sqrt() * 32768.0);
        let quant_table = std::array::from_fn(|i| (i as f32 * 0.152_891_65).exp() * root);
        let mut num_bands = 1;
        while num_bands < 25 && half_rate > CRITICAL_FREQS[num_bands - 1] {
            num_bands += 1;
        }
        let mut bands = [0usize; 26];
        bands[0] = 2;
        for i in 1..num_bands {
            bands[i] = (CRITICAL_FREQS[i - 1] as usize * frame_len / half_rate as usize) & !1;
        }
        bands[num_bands] = frame_len;
        let dct = DctPlanner::new().plan_dct3(frame_len);
        Self { frame_len, overlap_len, num_bands, root, bands, quant_table, previous: vec![vec![0.0; overlap_len]; channels], first: true, dct }
    }

    /// One block for `count` channels starting at `offset`; writes frame_len samples each.
    fn block(&mut self, gb: &mut Bits, out: &mut [Vec<f32>], offset: usize, count: usize) -> Result<()> {
        gb.get(2);
        let n = self.frame_len;
        for ch in 0..count {
            if gb.left() < 58 {
                bail!("truncated block");
            }
            let mut coeffs = vec![0f32; n];
            coeffs[0] = gb.float() * self.root;
            coeffs[1] = gb.float() * self.root;
            if gb.left() < self.num_bands as isize * 8 {
                bail!("truncated block");
            }
            let mut quant = [0f32; 25];
            for q in quant.iter_mut().take(self.num_bands) {
                *q = self.quant_table[(gb.get(8) as usize).min(95)];
            }
            let mut k = 0;
            let mut q = quant[0];
            let mut i = 2;
            while i < n {
                let j = if gb.get(1) == 1 { i + RLE_LENGTHS[gb.get(4) as usize] * 8 } else { i + 8 }.min(n);
                let width = gb.get(4);
                if width == 0 {
                    i = j;
                    while self.bands[k] < i {
                        q = quant[k];
                        k += 1;
                    }
                } else {
                    while i < j {
                        if self.bands[k] == i {
                            q = quant[k];
                            k += 1;
                        }
                        let c = gb.get(width);
                        if c != 0 {
                            coeffs[i] = if gb.get(1) == 1 { -q * c as f32 } else { q * c as f32 };
                        }
                        i += 1;
                    }
                }
            }
            // FFmpeg: coeffs[0] /= 0.5, then an inverse DCT (DCT-III). rustdct's unnormalized
            // DCT-III times 2/frame_len matches FFmpeg's output.
            coeffs[0] /= 0.5;
            self.dct.process_dct3(&mut coeffs);
            let scale = 2.0 / n as f32;
            let o = &mut out[offset + ch];
            for (dst, c) in o.iter_mut().zip(&coeffs) {
                *dst = c * scale;
            }
        }
        for ch in 0..count {
            let c = offset + ch;
            let total = self.overlap_len * count;
            if !self.first {
                let mut j = ch;
                for i in 0..self.overlap_len {
                    out[c][i] = (self.previous[c][i] * (total - j) as f32 + out[c][i] * j as f32) / total as f32;
                    j += count;
                }
            }
            self.previous[c].copy_from_slice(&out[c][n - self.overlap_len..]);
        }
        self.first = false;
        Ok(())
    }
}

/// Decode a whole "1FCB" Bink Audio file.
pub fn decode(file: &[u8]) -> Result<Pcm> {
    if file.len() < 24 || &file[0..4] != b"1FCB" {
        bail!("not a Bink Audio file");
    }
    let channels = file[5] as usize;
    let sample_rate = u16::from_le_bytes([file[6], file[7]]) as u32;
    let total_samples = u32::from_le_bytes(file[8..12].try_into().unwrap()) as usize;
    if channels == 0 || channels > MAX_DCT_CHANNELS || sample_rate == 0 {
        bail!("unsupported Bink Audio: {channels} channels at {sample_rate} Hz");
    }
    let entries = u16::from_le_bytes([file[20], file[21]]) as usize;
    let mut pos = 22 + entries * 2 + 2;
    let mut dec = Decoder::new(sample_rate, channels);
    let keep = dec.frame_len - dec.overlap_len;
    let mut out = vec![vec![0f32; dec.frame_len]; channels];
    let mut samples = Vec::with_capacity(total_samples * channels);
    while pos + 4 <= file.len() {
        let size = u16::from_le_bytes([file[pos + 2], file[pos + 3]]) as usize;
        if size == 0 {
            break;
        }
        let start = pos + 4;
        let end = (start + size).min(file.len());
        pos = start + size;
        let mut gb = Bits { data: &file[start..end], pos: 0 };
        // A packet can hold several frames; each frame is one block per channel pair.
        while gb.left() > 0 {
            let mut offset = 0;
            while offset < channels {
                let count = MAX_CHANNELS_PER_BLOCK.min(channels - offset);
                if dec.block(&mut gb, &mut out, offset, count).is_err() {
                    gb.pos = gb.data.len() * 8;
                    break;
                }
                gb.align32();
                offset += MAX_CHANNELS_PER_BLOCK;
            }
            if offset < channels {
                break;
            }
            for i in 0..keep {
                for ch in out.iter() {
                    samples.push(ch[i]);
                }
            }
        }
    }
    samples.truncate(total_samples * channels);
    Ok(Pcm { channels, sample_rate, samples })
}
