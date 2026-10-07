//! Print material flags + texture formats: `matflags <rpak> <name filter>...`
use tf_assets::{material, rpak::Rpak, texture};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let pak = Rpak::open(&a[1])?;
    for asset in pak.assets.iter().filter(|x| &x.kind == b"matl") {
        let m = material::load(&pak, asset)?;
        let n = m.name.to_lowercase();
        if !a[2..].iter().any(|f| n.contains(f.as_str())) { continue; }
        let h = asset.head.unwrap();
        let dx: Vec<String> = (0..2).map(|i| format!("{:08x}/{:04x}/{:04x}", pak.u32(h + 0x50 + i * 0x20 + 16), pak.u16(h + 0x50 + i * 0x20 + 20), pak.u16(h + 0x50 + i * 0x20 + 22))).collect();
        let fmts: Vec<String> = m.textures.iter().filter(|&&g| g != 0).map(|&g| pak.asset(g).and_then(|t| texture::info(&pak, t).ok()).map(|i| format!("{:?} {}", i.format, i.name.unwrap_or_default())).unwrap_or(format!("ext {g:016x}"))).collect();
        println!("{:60} glue {:08x} {:08x} dx {:?} shds {:016x} {:?}", m.name, m.glue_flags, m.glue_flags2, dx, m.shader_set, fmts);
    }
    Ok(())
}
