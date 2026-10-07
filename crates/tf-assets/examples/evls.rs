//! Print the sources each named sound event plays: `evls <r2/sound dir> <event>...`, or the
//! event names containing a substring: `evls <r2/sound dir> --grep <text>`
use tf_assets::miles::MilesBank;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let bank = MilesBank::open(std::path::Path::new(&a[1]))?;
    if a.get(2).map(String::as_str) == Some("--grep") {
        let f = a.get(3).map(|s| s.to_ascii_lowercase()).unwrap_or_default();
        for n in bank.event_names().into_iter().filter(|n| n.contains(&f)) {
            println!("{n}");
        }
        return Ok(());
    }
    for e in &a[2..] {
        let names: Vec<&str> = bank.event_variants(e).into_iter().map(|i| bank.sources[i].name.as_str()).collect();
        let looped = bank.event_info(e).map(|i| i.looped);
        println!("{e} (looped {looped:?}): {names:?}");
    }
    Ok(())
}
