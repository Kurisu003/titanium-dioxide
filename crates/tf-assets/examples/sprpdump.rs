//! Dump raw static prop records (sprp v13, 64 bytes) to work out the fields past the skin:
//! `sprpdump <map dir.vpk> <map> [count]`
use tf_assets::{bsp::Bsp, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let map = &a[2];
    let n: usize = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(12);
    let data = vpk.read(&format!("maps/{map}.bsp"))?;
    let bsp = Bsp::parse(map, &data, |i| vpk.read(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok())?;
    let sp = bsp.static_props()?;
    let raw = bsp.static_prop_records()?;
    for (i, r) in raw.iter().take(n).enumerate() {
        let f = |o: usize| f32::from_le_bytes(r[o..o + 4].try_into().unwrap());
        let h = |o: usize| i16::from_le_bytes(r[o..o + 2].try_into().unwrap());
        println!(
            "{i:4} {:40} | 34:{} 36:{:.1} 40:{:.1} 44:{:.1} 48:{:.1} 52:{:?} 56:{:?} 60:{:?}",
            sp.model_names[sp.props[i].model as usize].rsplit('/').next().unwrap_or(""),
            h(34),
            f(36),
            f(40),
            f(44),
            f(48),
            &r[52..56],
            &r[56..60],
            &r[60..64]
        );
    }
    Ok(())
}
