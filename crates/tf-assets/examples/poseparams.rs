//! Pose parameters of a model: `poseparams <dir.vpk> <model.mdl>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    for (i, p) in m.pose_params.iter().enumerate() {
        println!("{i} {p:?}");
    }
    Ok(())
}
