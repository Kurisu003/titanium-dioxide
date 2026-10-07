//! Bone rest scales and per-bone anim flags of one animation: `bonescale <dir.vpk> <model.mdl> [anim]`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    for (i, b) in m.bones.iter().enumerate() {
        println!("{i:3} {:24} scale {:?} ss {:?}", b.name, b.scale, b.scale_scale);
    }
    Ok(())
}
