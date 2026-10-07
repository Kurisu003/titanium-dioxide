//! Extract every file under a path prefix from one or more VPKs, for reading scripts:
//! `vpkdump <outdir> <prefix> <dir.vpk>...` (writes outside the repo; never commit the output)
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (out, prefix) = (std::path::Path::new(&args[1]), &args[2]);
    let mut n = 0;
    for path in &args[3..] {
        let vpk = tf_assets::vpk::Vpk::open(path)?;
        for name in vpk.entries.keys().filter(|k| k.starts_with(prefix.as_str())) {
            let dest = out.join(name);
            if dest.exists() {
                continue;
            }
            std::fs::create_dir_all(dest.parent().unwrap())?;
            std::fs::write(&dest, vpk.read(name)?)?;
            n += 1;
        }
    }
    eprintln!("extracted {n} files");
    Ok(())
}
