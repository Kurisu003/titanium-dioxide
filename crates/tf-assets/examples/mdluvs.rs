//! Print each LOD-0 mesh's position and UV bounds: `mdluvs <dir.vpk> <model.mdl>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let model = Model::parse(vpk.read(&a[2])?)?;
    for (i, m) in model.meshes(&[])?.iter().enumerate() {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for uv in &m.uvs {
            for k in 0..2 {
                lo[k] = lo[k].min(uv[k]);
                hi[k] = hi[k].max(uv[k]);
            }
        }
        println!("mesh {i}: {} verts, {} tris, uv {lo:?}..{hi:?}", m.positions.len(), m.indices.len() / 3);
        for j in (0..m.positions.len()).step_by((m.positions.len() / 12).max(1)) {
            println!("  pos {:?} uv {:?}", m.positions[j], m.uvs[j]);
        }
    }
    Ok(())
}
