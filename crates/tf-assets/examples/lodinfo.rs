//! LOD switch points and triangle counts of models: `lodinfo <dir.vpk> <model.mdl>...`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    for p in &a[2..] {
        let m = Model::parse(vpk.read(p)?)?;
        let sw = m.lod_switch_points();
        let tris: Vec<usize> = (0..sw.len().max(1)).map(|l| m.meshes_lod(&[], l).map(|v| v.iter().map(|d| d.indices.len() / 3).sum()).unwrap_or(0)).collect();
        println!("{p}: switch {sw:?} tris {tris:?} hull {:?}", m.hull);
    }
    Ok(())
}
