//! Print flattened player settings: `setinfo <dir.vpk> scripts/players/mp/titan_buddy.set`
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = tf_assets::vpk::Vpk::open(&a[1])?;
    let s = tf_assets::settings::PlayerSettings::load(&a[2], true, &mut |p| vpk.read(p).ok().map(|b| String::from_utf8_lossy(&b).to_string())).expect("load");
    let mut v: Vec<_> = s.values.iter().collect();
    v.sort();
    for (k, val) in v { println!("{k} = {val}"); }
    Ok(())
}
