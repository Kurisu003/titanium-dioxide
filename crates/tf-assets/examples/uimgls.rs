//! List UI atlas images, naming those whose path is in a list: `uimgls <ui.rpak> [names.txt]`
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let pak = tf_assets::rpak::Rpak::open(&a[1])?;
    let names: std::collections::HashMap<u32, String> = a
        .get(2)
        .map(|p| std::fs::read_to_string(p).unwrap_or_default())
        .unwrap_or_default()
        .lines()
        .map(|l| (tf_assets::uimg::path_hash(l.trim()), l.trim().to_string()))
        .collect();
    for asset in pak.assets.iter().filter(|x| x.kind_str() == "uimg") {
        let at = tf_assets::uimg::load(&pak, asset)?;
        let named = at.images.iter().filter(|i| names.contains_key(&i.hash)).count();
        println!("atlas {:016x} texture {:016x} {}x{} images {} named {}", at.guid, at.texture, at.width, at.height, at.images.len(), named);
        for i in &at.images {
            println!("  {:08x} {:5} {:5} {:5}x{:<5} {}", i.hash, i.x, i.y, i.w, i.h, names.get(&i.hash).map(String::as_str).unwrap_or("?"));
        }
    }
    Ok(())
}
