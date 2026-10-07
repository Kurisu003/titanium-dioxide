//! Write raw BSP lumps to files for inspection: `lumpdump <map vpk> <map> <outdir> <lump hex>...`
//! For looking at assets only, never commit the output.
use tf_assets::{bsp::Bsp, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let map = &a[2];
    let data = vpk.read(&format!("maps/{map}.bsp"))?;
    let bsp = Bsp::parse(map, &data, |i| vpk.read(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok())?;
    for l in &a[4..] {
        let i = usize::from_str_radix(l, 16)?;
        std::fs::write(format!("{}/{map}.{i:02x}.lump", a[3]), bsp.lump(i))?;
    }
    Ok(())
}
