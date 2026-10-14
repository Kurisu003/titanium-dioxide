//! Print every value of a player settings file: `setdump <dir.vpk> <scripts/players/mp/x.set>`
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = tf_assets::vpk::Vpk::open(&a[1])?;
    let mut read = |p: &str| vpk.read(p).ok().map(|b| String::from_utf8_lossy(&b).to_string());
    let s = tf_assets::settings::PlayerSettings::load(&a[2], true, &mut read).ok_or_else(|| anyhow::anyhow!("no {}", a[2]))?;
    let mut v: Vec<_> = s.values.iter().collect();
    v.sort();
    for (k, val) in v {
        println!("{k}\t{val}");
    }
    Ok(())
}
