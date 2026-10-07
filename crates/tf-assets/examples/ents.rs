//! Print a map's entities of one class: `ents <vpk> <map> <classname substring>`
use tf_assets::{bsp::Bsp, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let map = &a[2];
    let data = vpk.read(&format!("maps/{map}.bsp"))?;
    let bsp = Bsp::parse(map, &data, |i| vpk.read(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok())?;
    for e in tf_assets::bsp::parse_entities(&bsp.entities()) {
        if e.iter().any(|(k, v)| k == "classname" && v.contains(a[3].as_str())) {
            println!("{e:?}");
        }
    }
    Ok(())
}
