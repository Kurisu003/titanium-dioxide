//! List a model's RUI meshes (weapon screens): `ruimeshes <dir.vpk> <model.mdl>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let model = Model::parse(vpk.read(&a[2])?)?;
    for m in &model.rui_meshes {
        println!("{} ({} faces)", m.name, m.faces.len());
        for f in &m.faces {
            let bone = model.bones.get(f.bone).map(|b| b.name.as_str()).unwrap_or("?");
            println!("  {bone}: corners {:?} uvs {:?}", f.corners, f.uvs);
        }
    }
    Ok(())
}
