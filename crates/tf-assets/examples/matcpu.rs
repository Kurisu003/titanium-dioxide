//! Dump a material's CPU data (its shader constant buffer) as floats:
//! `matcpu <file.rpak> <material name filter>`
use tf_assets::{material, rpak::Rpak};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let pak = Rpak::open(&a[1])?;
    for asset in pak.assets.iter().filter(|x| &x.kind == b"matl") {
        let m = material::load(&pak, asset)?;
        if !m.name.to_lowercase().contains(&a[2].to_lowercase()) || m.name.ends_with("_colpass") {
            continue;
        }
        let Some(c) = asset.cpu else {
            println!("{}: no cpu data", m.name);
            continue;
        };
        // MaterialCPUHeader: data pointer, data size, version.
        let ptr = pak.ptr(c);
        let size = pak.u32(c + 8);
        println!("{} cpu@{c:#x} ptr {:?} size {size}", m.name, ptr);
        if let Some(p) = ptr {
            let n = (size as usize / 4).min(160);
            for row in 0..n.div_ceil(8) {
                let vals: Vec<String> = (0..8).filter(|k| row * 8 + k < n).map(|k| format!("{:9.4}", f32::from_bits(pak.u32(p + (row * 8 + k) * 4)))).collect();
                println!("  {:#05x}: {}", row * 32, vals.join(" "));
            }
        }
    }
    Ok(())
}
