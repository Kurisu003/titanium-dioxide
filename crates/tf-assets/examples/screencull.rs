//! How much of a cockpit model lies behind its main screen (`*_int_screen`) seen from
//! jx_c_camera in a sequence's first frame: `screencull <dir.vpk> <model.mdl> <sequence>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let seq = m.sequences.iter().find(|s| s.label.eq_ignore_ascii_case(&a[3])).ok_or_else(|| anyhow::anyhow!("no sequence"))?;
    let pose = tf_assets::mdl::screen_cull::pose(&m, seq.anims[0])?;
    let cam = m.bone_index("jx_c_camera").ok_or_else(|| anyhow::anyhow!("no camera bone"))?;
    println!("camera at {:?}", pose[cam].1);
    let meshes = m.meshes(&[0; 16])?;
    let mats: Vec<String> = meshes
        .iter()
        .map(|md| m.skin_families.first().and_then(|f| f.get(md.material)).map(|&t| m.textures.get(t as usize).cloned().unwrap_or_default()).unwrap_or_default())
        .collect();
    let hidden = tf_assets::mdl::screen_cull::hidden_triangles(&m, &meshes, &mats, &pose, cam);
    for (md, (mat, h)) in meshes.iter().zip(mats.iter().zip(&hidden)) {
        println!("{mat}: {} of {} triangles behind the screen", h.iter().filter(|&&x| x).count(), md.indices.len() / 3);
    }
    // Bounds in the camera's view: forward distance and the angles right/up of forward (X).
    let e = pose[cam].1;
    for (md, mat) in meshes.iter().zip(&mats) {
        let mut hist = std::collections::BTreeMap::new();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for i in 0..md.positions.len() {
            let p = tf_assets::mdl::screen_cull::skin(&m, &pose, md, i);
            let d = [p[0] - e[0], p[1] - e[1], p[2] - e[2]];
            let v = [d[0], (d[1] / d[0]).atan().to_degrees(), (d[2] / d[0]).atan().to_degrees()];
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
            if d[0] > 0.0 && v[1].abs() < 20.0 && v[2].abs() < 15.0 {
                *hist.entry((d[0] / 4.0) as i32 * 4).or_insert(0) += 1;
            }
        }
        println!("{mat}: fwd {:.1}..{:.1} left {:.0}..{:.0} deg up {:.0}..{:.0} deg; central verts by fwd: {hist:?}", lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]);
    }
    // Where the console's central vertices sit relative to the screen along their view rays
    // (1 = on it, below 1 = in front).
    let mut screen = Vec::new();
    for (md, mat) in meshes.iter().zip(&mats) {
        if mat.contains("_int_screen") {
            for t in md.indices.chunks_exact(3) {
                screen.push([0, 1, 2].map(|k| tf_assets::mdl::screen_cull::skin(&m, &pose, md, t[k] as usize)));
            }
        }
    }
    for (md, mat) in meshes.iter().zip(&mats) {
        if !mat.contains("console") {
            continue;
        }
        let mut hist = std::collections::BTreeMap::new();
        for i in 0..md.positions.len() {
            let p = tf_assets::mdl::screen_cull::skin(&m, &pose, md, i);
            if let Some(t) = tf_assets::mdl::screen_cull::screen_hit(&screen, e, p) {
                *hist.entry(((1.0 / t) * 100.0) as i32).or_insert(0) += 1;
            }
        }
        println!("{mat}: vertex distance / screen distance (percent): {hist:?}");
    }
    Ok(())
}
