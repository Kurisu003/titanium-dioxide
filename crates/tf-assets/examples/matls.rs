//! Dump materials and their textures: `matls <file.rpak> [name filter]`
use tf_assets::{material, rpak::Rpak, texture};
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let pak = Rpak::open(&args[1])?;
    let filter = args.get(2).cloned().unwrap_or_default();
    for a in pak.assets.iter().filter(|a| a.kind_str() == "matl") {
        let m = material::load(&pak, a)?;
        if !m.name.contains(&filter) { continue; }
        println!("{:016x} {} surf={} shds={:016x} guid_check={}", a.guid, m.name, m.surface_prop, m.shader_set, tf_assets::string_to_guid(&m.name) == a.guid);
        for (i, t) in m.textures.iter().enumerate() {
            if *t == 0 { continue; }
            match pak.asset(*t) {
                Some(ta) => { let ti = texture::info(&pak, ta)?; println!("   [{i}] {:016x} {:?} {}x{} {:?} perm={} stream={}", t, ti.name, ti.width, ti.height, ti.format, ti.permanent_mips, ti.streamed_mips); }
                None => println!("   [{i}] {:016x} (external)", t),
            }
        }
    }
    Ok(())
}
