//! Durations of sequences matching a filter in a model and its includes:
//! `seqlen <dir.vpk> <model.mdl> <filter>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let f = a[3].to_lowercase();
    let mut all = vec![(a[2].clone(), m)];
    for inc in all[0].1.include_models.clone() {
        if let Ok(d) = vpk.read(&inc) {
            all.push((inc.clone(), Model::parse(d)?));
        }
    }
    for (name, m) in &all {
        for s in m.sequences.iter().filter(|s| s.label.to_lowercase().contains(&f)) {
            if let Some(&ai) = s.anims.first() {
                if let Ok(an) = m.animation(ai) {
                    println!("{name}: {} {:.3}s ({} frames @ {} fps)", s.label, (an.num_frames.max(2) - 1) as f32 / an.fps.max(1.0), an.num_frames, an.fps);
                }
            }
        }
    }
    Ok(())
}
