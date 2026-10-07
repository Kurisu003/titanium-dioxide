//! Inspect RUI ("ui" v30) asset headers: `ruidump <ui.rpak> [count]` — prints each header's
//! pointer fields resolved to strings where they point at text, for format research.
use tf_assets::rpak::Rpak;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let pak = Rpak::open(&a[1])?;
    let n: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(5);
    for asset in pak.assets.iter().filter(|x| x.kind_str() == "ui").take(n) {
        let Some(h) = asset.head else { continue };
        print!("{:016x}:", asset.guid);
        for o in (0..asset.header_size as usize).step_by(8) {
            let v = pak.u64(h + o);
            match pak.ptr(h + o).filter(|p| *p < pak.data.len()) {
                Some(p) => {
                    let s = pak.cstr(p);
                    if s.len() > 2 && s.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
                        print!(" +{o:#x}=\"{s}\"");
                    } else {
                        print!(" +{o:#x}=ptr[{}]", (0..4).map(|k| format!("{:08x}", pak.u32(p + k * 4))).collect::<Vec<_>>().join(" "));
                    }
                }
                None => print!(" +{o:#x}=({:#x},{:#x})", v as u32, v >> 32),
            }
        }
        println!();
    }
    Ok(())
}
