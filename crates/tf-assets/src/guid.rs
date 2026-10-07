//! Respawn's asset-name hash (`RTech::StringToGuid`), processing the string 4 bytes at a
//! time. Case-insensitive and treats '\\' like '/'.

pub fn string_to_guid(s: &[u8]) -> u64 {
    let word = |i: usize| -> u32 {
        let mut b = [0u8; 4];
        for (k, v) in b.iter_mut().enumerate() {
            *v = *s.get(i + k).unwrap_or(&0);
        }
        // Bytes after the terminator are irrelevant (masked out below), so stop at the first NUL.
        if let Some(n) = b.iter().position(|&c| c == 0) {
            b[n..].fill(0);
        }
        u32::from_le_bytes(b)
    };
    let mut v1: u64 = 0;
    let mut i: u32 = 0;
    loop {
        let w = word(i as usize);
        let v4 = !w & w.wrapping_sub(0x0101_0101) & 0x8080_8080;
        let v5 = v4 ^ v4.wrapping_sub(1);
        let v6 = (v5 & w) ^ 0x5C5C_5C5C;
        let v7 = !v6 & v6.wrapping_sub(0x0101_0101) & 0x8080_8080;
        let mut v8 = v7 & v7.wrapping_neg();
        if v7 != v8 {
            let mut v9: u32 = 0xFF00_0000;
            loop {
                let v10 = v9;
                if v9 & v6 == 0 {
                    v8 |= v9 & 0x8080_8080;
                }
                v9 >>= 8;
                if v10 < 0x100 {
                    break;
                }
            }
        }
        let v11 = 0x633D5F1u64.wrapping_mul(v1);
        let folded = ((v5 & w).wrapping_sub(45u32.wrapping_mul(v8 >> 7))) & 0xDFDF_DFDF;
        let v12 = 0xFB8C4D96501u64.wrapping_mul(folded as u64) >> 24;
        if v4 != 0 {
            let v13 = 31 - v5.leading_zeros() as i32;
            let n = (i as i32 + v13 / 8) as u32;
            return v12.wrapping_add(v11).wrapping_sub(0xAE502812AA7333u64.wrapping_mul(n as u64));
        }
        i += 4;
        let sum = v11.wrapping_add(v12);
        v1 = (sum >> 61) ^ sum;
    }
}
