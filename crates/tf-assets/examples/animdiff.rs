//! Bone-by-bone difference between two animation frames of a model:
//! `animdiff <dir.vpk> <model.mdl> <animA> <frameA|-1=last> <animB> <frameB|-1=last>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let get = |ai: &str, fi: &str| -> anyhow::Result<(String, Vec<([f32; 3], [f32; 4])>)> {
        let an = m.animation(ai.parse()?)?;
        let f: i64 = fi.parse()?;
        let f = if f < 0 { an.num_frames - 1 } else { (f as usize).min(an.num_frames - 1) };
        Ok((an.name.clone(), an.frames[f].clone()))
    };
    let (na, fa) = get(&a[3], &a[4])?;
    let (nb, fb) = get(&a[5], &a[6])?;
    println!("{na} vs {nb}");
    for (bi, ((pa, qa), (pb, qb))) in fa.iter().zip(&fb).enumerate() {
        let d = (qa[0]*qb[0]+qa[1]*qb[1]+qa[2]*qb[2]+qa[3]*qb[3]).abs().min(1.0);
        let r = 2.0 * d.acos().to_degrees();
        let t = ((pa[0]-pb[0]).powi(2)+(pa[1]-pb[1]).powi(2)+(pa[2]-pb[2]).powi(2)).sqrt();
        if r > 1.0 || t > 0.1 { println!("{bi:3} {:24} rot {r:.1} move {t:.2}", m.bones[bi].name); }
    }
    Ok(())
}
