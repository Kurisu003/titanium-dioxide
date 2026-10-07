//! Per-bone values of one animation frame: `animbones <dir.vpk> <model.mdl> <anim index> [frame]`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let an = m.animation(a[3].parse()?)?;
    let f: usize = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    println!("{} flags {:#x} frames {}", an.name, an.flags, an.num_frames);
    for (bi, (p, q)) in an.frames[f.min(an.num_frames - 1)].iter().enumerate() {
        if !an.animated[bi] { continue; }
        let r = 2.0 * q[3].abs().min(1.0).acos().to_degrees();
        let t = (p[0]*p[0]+p[1]*p[1]+p[2]*p[2]).sqrt();
        if std::env::var("ALL").is_ok() || r > 0.5 || t > 0.05 { println!("{bi:3} {:24} p {:?} q {:?} rot {r:.1} move {t:.2}", m.bones[bi].name, p.map(|x| (x*100.0).round()/100.0), q.map(|x| (x*1000.0).round()/1000.0)); }
    }
    Ok(())
}
