//! List shader sets (`shds`) with their names, and which materials use each:
//! `shdsls <file.rpak> [material name filter]`
use tf_assets::{material, rpak::Rpak};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let pak = Rpak::open(&a[1])?;
    let filter = a.get(2).cloned().unwrap_or_default();
    let mut names = std::collections::HashMap::new();
    for s in pak.assets.iter().filter(|x| &x.kind == b"shds") {
        let h = s.head.unwrap();
        // Try the pointer slots of the header for a name string.
        let mut name = String::new();
        for off in (0..0x20).step_by(8) {
            if let Some(p) = pak.ptr(h + off) {
                let c = pak.cstr(p);
                if c.len() > 2 && c.chars().all(|ch| ch.is_ascii_graphic()) {
                    name = format!("@{off:#x} {c}");
                    break;
                }
            }
        }
        names.insert(s.guid, name);
    }
    let mut uses: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for m in pak.assets.iter().filter(|x| &x.kind == b"matl") {
        let mat = material::load(&pak, m)?;
        if !mat.name.to_lowercase().contains(&filter.to_lowercase()) {
            continue;
        }
        let n = names.get(&mat.shader_set).cloned().unwrap_or_else(|| format!("{:016x} (external)", mat.shader_set));
        uses.entry(n).or_default().push(mat.name);
    }
    for (s, ms) in &uses {
        println!("{s}: {} materials, e.g. {:?}", ms.len(), &ms[..ms.len().min(3)]);
    }
    Ok(())
}
