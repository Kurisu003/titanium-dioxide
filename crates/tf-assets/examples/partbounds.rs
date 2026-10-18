//! Per-bodypart mesh bounds and joint ranges of a model: `partbounds <dir.vpk> <model.mdl>`.
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = tf_assets::vpk::Vpk::open(&a[1])?;
    let model = tf_assets::mdl::Model::parse(vpk.read(&a[2])?)?;
    for md in model.meshes(&[])? {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &md.positions {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        let maxj = md
            .joints
            .iter()
            .flat_map(|j| j.iter().copied())
            .max()
            .unwrap_or(0);
        let maxw: f32 = md
            .weights
            .iter()
            .map(|w| w.iter().sum::<f32>())
            .fold(0.0, f32::max);
        let minw: f32 = md
            .weights
            .iter()
            .map(|w| w.iter().sum::<f32>())
            .fold(9.0, f32::min);
        if md.bodypart == 6 {
            let mut hist = std::collections::HashMap::<u16, usize>::new();
            for (j, w) in md.joints.iter().zip(&md.weights) {
                let (mut bj, mut bw) = (0u16, -1.0f32);
                for k in 0..4 {
                    if w[k] > bw {
                        bw = w[k];
                        bj = j[k];
                    }
                }
                *hist.entry(bj).or_default() += 1;
            }
            let mut v: Vec<_> = hist.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            for (j, n) in v.iter().take(12) {
                println!(
                    "  joint {j} ({}): {n} verts",
                    model
                        .bones
                        .get(*j as usize)
                        .map(|b| b.name.as_str())
                        .unwrap_or("?")
                );
            }
        }
        println!(
            "bodypart {} mat {}: {} verts, {} tris, bounds {:?}..{:?}, max joint {maxj} of {} bones, weight sums {minw:.2}..{maxw:.2}",
            md.bodypart,
            md.material,
            md.positions.len(),
            md.indices.len() / 3,
            lo,
            hi,
            model.bones.len()
        );
    }
    Ok(())
}
