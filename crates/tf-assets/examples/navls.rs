//! Inspect a map's navmeshes: `navls <dir.vpk> <map> [size]` (size: large, medium, med_short,
//! small; default large). Prints tile/poly/link counts, the walkable sizes and a few polys.
use tf_assets::{navmesh, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let size = a.get(3).map(|s| s.as_str()).unwrap_or("large");
    let path = format!("maps/navmesh/{}_{size}.nm", a[2]);
    let t = std::time::Instant::now();
    let m = navmesh::parse(&vpk.read(&path)?)?;
    let links: usize = m.polys.iter().map(|p| p.links.len()).sum();
    let isolated = m.polys.iter().filter(|p| p.links.is_empty()).count();
    println!(
        "{path}: {} tiles, {} polys ({} isolated, {} off-mesh skipped), {links} links, walkable height {} radius {} climb {} in {:?}",
        m.tiles,
        m.polys.len(),
        isolated,
        m.off_mesh,
        m.walkable_height,
        m.walkable_radius,
        m.walkable_climb,
        t.elapsed()
    );
    for (i, p) in m.polys.iter().enumerate().take(5) {
        println!("  poly {i}: centre {:?} verts {} links {:?} flags {:#x} area {}", p.center, p.verts.len(), p.links, p.flags, p.area);
    }
    Ok(())
}
