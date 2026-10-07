//! List sequences of a model and its includes matching a filter: `seqls <dir.vpk> <model.mdl> [filter]`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let f = a.get(3).cloned().unwrap_or_default();
    let mut all = vec![(a[2].clone(), m)];
    for inc in all[0].1.include_models.clone() { if let Ok(d) = vpk.read(&inc) { all.push((inc.clone(), Model::parse(d)?)); } }
    for (name, m) in &all {
        for (i, p) in m.pose_params.iter().enumerate() { println!("{name}: pose {i} {} {}..{} loop {}", p.name, p.start, p.end, p.looping); }
        for s in m.sequences.iter().filter(|s| s.label.to_lowercase().contains(&f) || s.activity.to_lowercase().contains(&f)) {
            println!("{name}: {} act={} flags={:#x} grid={:?} params={:?} {:?}..{:?} anims={:?}", s.label, s.activity, s.flags, s.group_size, s.param_index, s.param_start, s.param_end, &s.anims[..s.anims.len().min(4)]);
            for e in &s.events {
                println!("    {:.3} {} {} {:?}", e.cycle, e.event, e.name, e.options);
            }
        }
    }
    Ok(())
}
