//! Static props near a point: `propsnear <vpk> <map> "x y z" radius`
use tf_assets::{bsp::Bsp, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let map = &a[2];
    let p: Vec<f32> = a[3].split_whitespace().map(|x| x.parse().unwrap()).collect();
    let r: f32 = a[4].parse()?;
    let data = vpk.read(&format!("maps/{map}.bsp"))?;
    let bsp = Bsp::parse(map, &data, |i| vpk.read(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok())?;
    let sp = bsp.static_props()?;
    let mut counts = std::collections::BTreeMap::new();
    for prop in &sp.props {
        let d = ((prop.origin[0] - p[0]).powi(2) + (prop.origin[1] - p[1]).powi(2) + (prop.origin[2] - p[2]).powi(2)).sqrt();
        if d < r {
            *counts.entry(sp.model_names[prop.model as usize].clone()).or_insert((0, prop.scale)) = (counts.get(&sp.model_names[prop.model as usize]).map(|c: &(i32, f32)| c.0).unwrap_or(0) + 1, prop.scale);
        }
    }
    for (k, v) in counts { println!("{:4} x scale {:.2} {k}", v.0, v.1); }
    // World meshes near the point, by material.
    let models = bsp.models();
    println!("{} brush models; first bounds {:?}", models.len(), models.iter().take(4).collect::<Vec<_>>());
    Ok(())
}
