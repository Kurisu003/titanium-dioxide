//! List (and optionally extract one file from) a Respawn VPK: `vpkls <dir.vpk> [filter] [--extract path out]`
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let vpk = tf_assets::vpk::Vpk::open(&args[1])?;
    if args.get(3).map(|s| s.as_str()) == Some("--extract") {
        let data = vpk.read(&args[4])?;
        std::fs::write(&args[5], data)?;
        return Ok(());
    }
    let filter = args.get(2).cloned().unwrap_or_default();
    let mut names: Vec<_> = vpk.entries.iter().filter(|(k, _)| k.contains(&filter)).collect();
    names.sort_by(|a, b| a.0.cmp(b.0));
    for (k, e) in names {
        println!("{:>10} {}", e.size(), k);
    }
    Ok(())
}
