//! List every body part model's LOD-0 meshes with their material: `mdlmeshes <dir.vpk> <model.mdl>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    for (bpi, bp) in m.bodyparts.iter().enumerate() {
        for mi in 0..bp.num_models {
            let mut body = vec![0usize; m.bodyparts.len()];
            body[bpi] = mi;
            let meshes = m.meshes(&body)?;
            for md in meshes.iter().filter(|md| md.bodypart == bpi) {
                let tex = m.skin_families.first().and_then(|f| f.get(md.material)).map(|&t| m.textures.get(t as usize).cloned().unwrap_or_default()).unwrap_or_default();
                println!("bodypart {} ({}) model {mi}: mat {} -> {tex} tris {}", bp.name, bpi, md.material, md.indices.len() / 3);
            }
        }
    }
    Ok(())
}
