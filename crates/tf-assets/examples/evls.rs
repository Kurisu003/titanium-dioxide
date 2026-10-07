//! Print the sources each named sound event plays: `evls <r2/sound dir> <event>...`
use tf_assets::miles::MilesBank;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let bank = MilesBank::open(std::path::Path::new(&a[1]))?;
    for e in &a[2..] {
        let names: Vec<&str> = bank.event_variants(e).into_iter().map(|i| bank.sources[i].name.as_str()).collect();
        let looped = bank.event_info(e).map(|i| i.looped);
        println!("{e} (looped {looped:?}): {names:?}");
    }
    Ok(())
}
