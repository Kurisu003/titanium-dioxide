//! Collision primitives of a map (CM_PRIMITIVES 0x59): counts by type and contents, and which
//! static props the game collides with versus their `solid` byte: `cmprims <map vpk> <map>`
use tf_assets::{bsp::Bsp, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let map = &a[2];
    let data = vpk.read(&format!("maps/{map}.bsp"))?;
    let bsp = Bsp::parse(map, &data, |i| {
        vpk.read(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok()
    })?;
    let uc: Vec<u32> = bsp
        .lump(0x5B)
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    let prims: Vec<u32> = bsp
        .lump(0x59)
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    let sp = bsp.static_props()?;
    let mut prop_prim = vec![false; sp.props.len()];
    let mut out_of_range = 0;
    for p in &prims {
        let (t, idx) = (p >> 24, (p >> 8) & 0xffff);
        if t == 96 {
            match prop_prim.get_mut(idx as usize) {
                Some(v) => *v = true,
                None => out_of_range += 1,
            }
        }
    }
    let _ = &uc;
    let mut table = std::collections::BTreeMap::new();
    for (i, p) in sp.props.iter().enumerate() {
        *table.entry((p.solid, prop_prim[i])).or_insert(0) += 1;
    }
    println!("props {} ; (solid byte, has collision primitive) -> count: {:?} ; prop prims out of range {out_of_range}", sp.props.len(), table);
    for (i, p) in sp.props.iter().enumerate() {
        if prop_prim[i] && p.solid == 0 {
            println!(
                "  game-solid prop with solid=0: {} at {:?} flags {:#x}",
                sp.model_names[p.model as usize], p.origin, p.flags
            );
        }
    }
    Ok(())
}
