//! List a model's texture (material) names and skin table: `mdltex <dir.vpk> <model.mdl>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    for (i, t) in m.textures.iter().enumerate() { println!("tex {i}: {t}"); }
    println!("cd: {:?}", m.cd_textures);
    for (i, f) in m.skin_families.iter().enumerate() { println!("skin {i}: {:?}", f); }
    for bp in &m.bodyparts { println!("bodypart {} models {}", bp.name, bp.num_models); }
    Ok(())
}
