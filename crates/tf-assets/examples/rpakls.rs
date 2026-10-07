//! List assets in an RPAK: `rpakls <file.rpak> [type]`
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    let pak = tf_assets::rpak::Rpak::open(&args[1])?;
    eprintln!("loaded {} bytes, {} assets, {} pages in {:?}; starpaks {:?}", pak.data.len(), pak.assets.len(), pak.pages.len(), t.elapsed(), pak.starpaks);
    let mut counts = std::collections::BTreeMap::new();
    for a in &pak.assets {
        *counts.entry((a.kind_str(), a.version, a.header_size)).or_insert(0) += 1;
    }
    for (k, v) in &counts {
        eprintln!("{:>6} {} v{} hdr {:#x}", v, k.0, k.1, k.2);
    }
    if let Some(kind) = args.get(2) {
        for a in pak.assets.iter().filter(|a| &a.kind_str() == kind) {
            println!("{:016x} head={:?} cpu={:?} star={:x} deps={}", a.guid, a.head, a.cpu, a.starpak_offset, a.dependencies.len());
        }
    }
    Ok(())
}
