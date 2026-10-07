//! Search many rpaks for materials/textures whose name contains a string:
//! `findasset <substring> <rpak>...` (prints header patch info for each pak too)
use tf_assets::{material, rpak::Rpak, texture};
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let needle = args[1].to_ascii_lowercase();
    for p in &args[2..] {
        let raw = std::fs::read(p)?;
        let patch_count = u16::from_le_bytes([raw[0x3E], raw[0x3F]]);
        if patch_count != 0 && std::env::var_os("FIND_PATCHED").is_none() { println!("{p}: patches {patch_count} (skipped; FIND_PATCHED=1 reads its own pages)"); continue; }
        let pak = match Rpak::open(p) { Ok(p) => p, Err(e) => { println!("{p}: {e}"); continue } };
        for a in &pak.assets {
            let name = match &a.kind_str()[..] {
                "matl" => match material::load(&pak, a) { Ok(m) => m.name, Err(_) => continue },
                "txtr" => match texture::info(&pak, a) { Ok(t) => t.name.unwrap_or_default(), Err(_) => continue },
                _ => continue,
            };
            if name.to_ascii_lowercase().contains(&needle) {
                println!("{p}: {} {:016x} {}", a.kind_str(), a.guid, name);
            }
        }
    }
    Ok(())
}
