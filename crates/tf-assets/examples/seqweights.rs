//! A sequence's per-bone weights: `seqweights <dir.vpk> <model.mdl> <sequence label>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let m = Model::parse(Vpk::open(&a[1])?.read(&a[2])?)?;
    for s in m.sequences.iter().filter(|s| s.label == a[3]) {
        for (b, w) in s.bone_weights.iter().enumerate() {
            println!("{b:3} {:24} {w}", m.bones[b].name);
        }
    }
    Ok(())
}
