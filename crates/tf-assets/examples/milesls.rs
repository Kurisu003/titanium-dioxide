//! List Miles audio sources, or dump one as .binka: `milesls <r2/sound dir> [filter] [--dump name out.binka]`
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let bank = tf_assets::miles::MilesBank::open(std::path::Path::new(&a[1]))?;
    if a.get(3).map(String::as_str) == Some("--dump") {
        let s = bank.find(&a[4]).expect("no such source");
        std::fs::write(&a[5], bank.read_binka(s)?)?;
        return Ok(());
    }
    eprintln!("mbnk v{}, {} sources", bank.version, bank.sources.len());
    let filter = a.get(2).cloned().unwrap_or_default();
    for s in bank.sources.iter().filter(|s| s.name.contains(&filter)) {
        println!("{:<60} {:>5} Hz {}ch lang {:>2} patch {} {}+{} bytes", s.name, s.sample_rate, s.channels, s.language, s.patch, s.header_size, s.data_size);
    }
    Ok(())
}
