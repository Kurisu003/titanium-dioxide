//! For viewmodels, list each sequence's sound events and how many bank sources each resolves to:
//! `evsounds <dir.vpk> <r2/sound dir> <model.mdl>...`
use tf_assets::{mdl::Model, miles::MilesBank, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let bank = MilesBank::open(std::path::Path::new(&a[2]))?;
    let resolved = bank.sources.len();
    let _ = resolved;
    println!("{} events", bank.event_count());
    let (mut ok, mut missing) = (0, 0);
    for path in &a[3..] {
        let m = Model::parse(vpk.read(path)?)?;
        let mut all = vec![m];
        for inc in all[0].include_models.clone() {
            if let Ok(d) = vpk.read(&inc) {
                all.push(Model::parse(d)?);
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for m in &all {
            for s in &m.sequences {
                for e in s.events.iter().filter(|e| e.name == "AE_CL_PLAYSOUND" || e.name.contains("SOUND")) {
                    if seen.insert(e.options.clone()) {
                        let n = bank.event_variants(&e.options).len();
                        if n == 0 { missing += 1; println!("{path}: {} {} MISSING", s.label, e.options); } else { ok += 1; }
                    }
                }
            }
        }
    }
    println!("{ok} resolved, {missing} missing");
    Ok(())
}
