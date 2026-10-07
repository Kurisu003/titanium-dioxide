//! Valve's obfuscated fonts (`.vfont`, "VFONT1"): decode to the original TrueType/OpenType file.
//! Algorithm from ValveResourceFormat (ValveFont.cs, MIT).

use anyhow::{bail, Result};

const MAGIC: &[u8] = b"VFONT1";
const TRICK: u32 = 167;

pub fn decode(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < MAGIC.len() + 1 || &data[data.len() - MAGIC.len()..] != MAGIC {
        bail!("not a VFONT1 file");
    }
    let salt_len = data[data.len() - MAGIC.len() - 1] as usize;
    let out_len = data.len().checked_sub(MAGIC.len() + salt_len).filter(|&n| n > 0);
    let Some(out_len) = out_len else { bail!("bad vfont salt") };
    // The salt (all but its length byte) seeds the key.
    let salt_start = data.len() - MAGIC.len() - 1 - (salt_len - 1);
    let mut key = TRICK;
    for &b in &data[salt_start..salt_start + salt_len - 1] {
        key ^= (b as u32 + TRICK) % 256;
    }
    let mut out = Vec::with_capacity(out_len);
    for &b in &data[..out_len] {
        out.push(b ^ key as u8);
        key = (b as u32 + TRICK) % 256;
    }
    Ok(out)
}
