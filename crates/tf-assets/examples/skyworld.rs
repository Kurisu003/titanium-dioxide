//! World meshes near a point (e.g. the sky camera), by material: `skyworld <vpk> <map> <x> <y> <z> [radius]`
use tf_assets::{bsp::Bsp, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let map = &a[2];
    let data = vpk.read(&format!("maps/{map}.bsp"))?;
    let bsp = Bsp::parse(map, &data, |i| vpk.read(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok())?;
    let c: Vec<f32> = a[3..6].iter().map(|s| s.parse().unwrap()).collect();
    let r: f32 = a.get(6).and_then(|s| s.parse().ok()).unwrap_or(9000.0);
    let (meshes, sorts, td, idx) = (bsp.meshes(), bsp.material_sorts(), bsp.texture_data(), bsp.mesh_indices());
    let world = bsp.models()[0];
    let mut by: std::collections::BTreeMap<String, (usize, [f32; 3], [f32; 3])> = Default::default();
    for m in &meshes[world.first_mesh as usize..(world.first_mesh + world.num_meshes) as usize] {
        let sort = sorts[m.material_sort as usize];
        let i0 = idx[m.first_index as usize] as usize + sort.vertex_offset as usize;
        let Some(v) = bsp.vertex(m.flags, i0) else { continue };
        let d = ((v.pos[0] - c[0]).powi(2) + (v.pos[1] - c[1]).powi(2) + (v.pos[2] - c[2]).powi(2)).sqrt();
        if d > r { continue; }
        let e = by.entry(format!("{} flags {:#x}", td[sort.texture_data as usize].name, m.flags)).or_insert((0, [f32::MAX; 3], [f32::MIN; 3]));
        e.0 += m.num_triangles as usize;
        for k in 0..3 { e.1[k] = e.1[k].min(v.pos[k]); e.2[k] = e.2[k].max(v.pos[k]); }
    }
    for (k, (n, lo, hi)) in by { println!("{n:6} {k} {:?}..{:?}", lo.map(|x| x.round()), hi.map(|x| x.round())); }
    Ok(())
}
