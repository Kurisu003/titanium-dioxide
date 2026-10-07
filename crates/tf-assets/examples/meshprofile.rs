//! Radius profile of a body part's meshes along an axis: `meshprofile <dir.vpk> <model.mdl> <body,list> <part index>`
//! Prints, per material, vertex counts and min/max radius (around the axis) per 0.5-unit slice of z.
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let body: Vec<usize> = a[3].split(',').filter_map(|x| x.parse().ok()).collect();
    let part: usize = a[4].parse()?;
    for md in m.meshes(&body)?.iter().filter(|md| md.bodypart == part) {
        println!("{}", m.textures[m.skin_families[0][md.material] as usize]);
        let mut bins: std::collections::BTreeMap<i32, (usize, f32, f32)> = Default::default();
        for p in &md.positions {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            let e = bins.entry((p[2] * 2.0).floor() as i32).or_insert((0, f32::MAX, 0.0));
            e.0 += 1;
            e.1 = e.1.min(r);
            e.2 = e.2.max(r);
        }
        for (z, (n, lo, hi)) in bins {
            println!("  z {:5.1}..{:5.1}: {n:5} verts, radius {lo:.2}..{hi:.2}", z as f32 / 2.0, z as f32 / 2.0 + 0.5);
        }
    }
    Ok(())
}
