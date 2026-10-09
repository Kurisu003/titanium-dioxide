//! Static prop collision facts of a map: `propcoll <map vpk> <map> [model vpk...]`
//!
//! Lists the CM_PRIMITIVES prop entries (type 3) against the sprp lump's solid/flags, then per
//! model (first instance's CM contents): its convex collision hulls (what movement hits) next
//! to the render bounds, and its per-triangle mesh (what bullets hit) against LOD 0.
//! `PROPCOLL_FIND=<substring>` lists the instances of matching models instead.
use tf_assets::{bsp::Bsp, mdl::Model, vpk::Vpk};

fn bounds<'a>(pts: impl Iterator<Item = &'a [f32; 3]>) -> ([f32; 3], [f32; 3]) {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in pts {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let map = &a[2];
    let vpks: Vec<Vpk> = a[3..].iter().map(Vpk::open).collect::<anyhow::Result<_>>()?;
    let data = vpk.read(&format!("maps/{map}.bsp"))?;
    let bsp = Bsp::parse(map, &data, |i| vpk.read(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok())?;
    let sp = bsp.static_props()?;
    let in_cm = bsp.static_prop_contents();
    if let Ok(f) = std::env::var("PROPCOLL_FIND") {
        for (i, p) in sp.props.iter().enumerate() {
            let name = &sp.model_names[p.model as usize];
            if name.contains(&f) {
                println!("{name} #{i} at {:?} angles {:?} scale {} solid {} cm contents {:x?}", p.origin, p.angles, p.scale, p.solid, in_cm.get(&i));
            }
        }
        return Ok(());
    }
    println!("{} props, {} in CM_PRIMITIVES", sp.props.len(), in_cm.len());
    let mut hist = std::collections::BTreeMap::new();
    for (i, p) in sp.props.iter().enumerate() {
        *hist.entry((p.solid, p.flags, in_cm.get(&i).copied())).or_insert(0) += 1;
    }
    for ((solid, flags, c), n) in &hist {
        println!("solid {solid} flags {flags:#x} cm contents {c:x?}: {n}");
    }
    let mut seen = std::collections::BTreeSet::new();
    for (i, p) in sp.props.iter().enumerate() {
        let name = &sp.model_names[p.model as usize];
        if !seen.insert(p.model) {
            continue;
        }
        let Some(v) = vpks.iter().chain(std::iter::once(&vpk)).find(|v| v.contains(name)) else {
            println!("{name}: not found");
            continue;
        };
        let m = Model::parse(v.read(name)?)?;
        let hulls = m.collision_hulls();
        let ht = m.collision_hull_tris();
        let pt = m.collision_tris();
        let lod0 = m.meshes_lod(&[], 0).unwrap_or_default();
        let rv: Vec<[f32; 3]> = lod0.iter().flat_map(|md| md.positions.iter().copied()).collect();
        let rt: usize = lod0.iter().map(|md| md.indices.len() / 3).sum();
        let pv: Vec<[f32; 3]> = pt.iter().flat_map(|t| t.0).collect();
        let on = rv.iter().filter(|p| pv.iter().any(|q| (0..3).all(|k| (p[k] - q[k]).abs() < 0.5))).count();
        println!(
            "{name}: solid {} cm contents {:x?} model contents {:#x}; {} hulls ({} tris) bounds {:.1?}; render bounds {:.1?}; per-tri mesh {} tris, LOD 0 {} tris, {on}/{} LOD 0 vertices on it",
            p.solid,
            in_cm.get(&i),
            m.contents(),
            hulls.len(),
            ht.len(),
            bounds(ht.iter().flatten()),
            bounds(rv.iter()),
            pt.len(),
            rt,
            rv.len()
        );
    }
    Ok(())
}
