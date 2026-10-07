//! Time reading and parsing a model: `readtime <dir.vpk> <model.mdl>...`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    for p in &a[2..] {
        let t = std::time::Instant::now();
        let d = vpk.read(p)?;
        let r = t.elapsed();
        let n = d.len();
        let m = Model::parse(d)?;
        let pt = t.elapsed() - r;
        let t2 = std::time::Instant::now();
        let meshes = m.meshes(&[])?;
        println!("{p}: {n} bytes, read {r:?}, parse {pt:?}, meshes {:?} ({} meshes)", t2.elapsed(), meshes.len());
    }
    Ok(())
}
