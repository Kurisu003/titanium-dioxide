//! Write a map's cubemap faces as tone-mapped PPMs and summarise its light probes:
//! `cubedump <map vpk> <map> <outdir> [cubemap index]`. For looking at assets only, never commit.
use tf_assets::{bsp::Bsp, cubemap::CubemapSet, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let map = &a[2];
    let data = vpk.read(&format!("maps/{map}.bsp"))?;
    let bsp = Bsp::parse(map, &data, |i| vpk.read(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok())?;
    let probes = bsp.light_probes();
    let refs = bsp.light_probe_refs();
    println!("{} probes, {} refs, cubemaps at {:?}", probes.len(), refs.len(), bsp.cubemap_origins());
    let vtf = bsp.pakfile_entry("cubemaps.hdr.vtf").expect("no cubemaps.hdr.vtf");
    let set = CubemapSet::parse(vtf)?;
    println!("{} cubemaps, {}px, {} mips", set.count, set.size, set.mips);
    let frame: usize = a.get(4).and_then(|v| v.parse().ok()).unwrap_or(0);
    for face in 0..6 {
        let px = set.face(frame, face, 0);
        let s = set.size as usize;
        let mut out = format!("P6 {s} {s} 255\n").into_bytes();
        let mut max = 0f32;
        for p in &px {
            for &c in p {
                let v = half(c);
                max = max.max(v);
                let t = v / (1.0 + v);
                out.push((t.powf(1.0 / 2.2) * 255.0) as u8);
            }
        }
        println!("face {face}: max {max}");
        std::fs::write(format!("{}/{map}_{frame}_{face}.ppm", a[3]), out)?;
    }
    Ok(())
}

fn half(h: u16) -> f32 {
    let (s, e, m) = ((h >> 15) as u32, ((h >> 10) & 31) as i32, (h & 1023) as f32);
    let v = if e == 0 { m / 1024.0 * 2f32.powi(-14) } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) };
    if s == 1 { -v } else { v }
}
