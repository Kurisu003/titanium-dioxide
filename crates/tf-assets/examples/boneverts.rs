//! Vertices per dominant bone, with their bounds: `boneverts <dir.vpk> <model.mdl>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let mut by: std::collections::BTreeMap<usize, (usize, [f32; 3], [f32; 3])> = Default::default();
    for md in m.meshes(&[0; 16])? {
        for (i, p) in md.positions.iter().enumerate() {
            let (j, w) = (md.joints[i], md.weights[i]);
            let k = (0..4).max_by(|&x, &y| w[x].total_cmp(&w[y])).unwrap();
            let e = by.entry(j[k] as usize).or_insert((0, [f32::MAX; 3], [f32::MIN; 3]));
            e.0 += 1;
            for c in 0..3 {
                e.1[c] = e.1[c].min(p[c]);
                e.2[c] = e.2[c].max(p[c]);
            }
        }
    }
    for (b, (n, lo, hi)) in by {
        println!("{b:3} {:24} {n:6} {:?}..{:?}", m.bones[b].name, lo.map(|x| x.round()), hi.map(|x| x.round()));
    }
    Ok(())
}
