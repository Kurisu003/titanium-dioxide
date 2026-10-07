//! Dump a map's lightmap pages as raw RGBA: `lmdump <vpk> <map> <outdir>`
use tf_assets::{bsp::Bsp, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let map = &a[2];
    let data = vpk.read(&format!("maps/{map}.bsp"))?;
    let bsp = Bsp::parse(map, &data, |i| vpk.read(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok())?;
    let h = bsp.lump(0x53);
    let sky = bsp.lump(0x62);
    let rtl = bsp.lump(0x69);
    eprintln!("headers {} bytes, sky {} bytes, rtl {} bytes", h.len(), sky.len(), rtl.len());
    let (mut so, mut ro) = (0usize, 0usize);
    for (i, c) in h.chunks_exact(8).enumerate() {
        let ty = u32::from_le_bytes(c[0..4].try_into()?);
        let (w, hh) = (u16::from_le_bytes([c[4], c[5]]) as usize, u16::from_le_bytes([c[6], c[7]]) as usize);
        eprintln!("page {i}: type {ty} {w}x{hh}");
        for ab in ["a", "b"] {
            let n = w * hh * 4;
            if so + n <= sky.len() { std::fs::write(format!("{}/sky{i}{ab}_{w}x{hh}.rgba", a[3]), &sky[so..so + n])?; }
            so += n;
            if ro + n <= rtl.len() { std::fs::write(format!("{}/rtl{i}{ab}_{w}x{hh}.rgba", a[3]), &rtl[ro..ro + n])?; }
            ro += n;
        }
        if rtl.len() > ro { ro += w * hh; }
    }
    eprintln!("sky used {so}/{}, rtl used {ro}/{}", sky.len(), rtl.len());
    Ok(())
}
