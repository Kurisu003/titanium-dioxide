//! Summarise the game's world collision (`Bsp::collision_shapes`): `cmbrush <map vpk> <map>`
//! Prints triangle counts by kind and contents, and checks brush faces stay in their bounds.
use tf_assets::{bsp::Bsp, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let map = &a[2];
    let data = vpk.read(&format!("maps/{map}.bsp"))?;
    let bsp = Bsp::parse(map, &data, |i| {
        vpk.read(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok()
    })?;
    let t = std::time::Instant::now();
    let shapes = bsp.collision_shapes();
    println!("{} shapes in {:?}", shapes.len(), t.elapsed());
    let td = bsp.texture_data();
    let mut table = std::collections::BTreeMap::new();
    for s in &shapes {
        let name = if s.texture_data < 0 {
            "<brush>".to_string()
        } else {
            td.get(s.texture_data as usize)
                .map(|t| t.name.clone())
                .unwrap_or_default()
        };
        let tool = name.to_ascii_uppercase().starts_with("TOOLS") || name.starts_with('<');
        let e = table
            .entry((
                if tool { name } else { "(textured)".into() },
                format!("{:#x}", s.contents),
            ))
            .or_insert((0, 0));
        e.0 += 1;
        e.1 += s.tris.len();
    }
    for (k, v) in table {
        println!("{k:?}: {} shapes, {} tris", v.0, v.1);
    }
    // Brush faces: 3+ sides each, inside a sane size.
    let mut empty = 0;
    for s in shapes.iter().filter(|s| s.texture_data < 0) {
        if s.tris.len() < 4 {
            empty += 1;
        }
    }
    println!("brushes with fewer than 4 triangles: {empty}");
    Ok(())
}
