//! Magnitude of each animation of a sequence: `animmag <dir.vpk> <model.mdl> <seq>...`
//! Prints, per blend anim, the delta flag and the largest translation/rotation of any bone
//! over all frames (for deltas: from identity; otherwise from the first frame).
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let mut owners = vec![m];
    for inc in owners[0].include_models.clone() { if let Ok(d) = vpk.read(&inc) { owners.push(Model::parse(d)?); } }
    for name in &a[3..] {
        let Some((o, s)) = owners.iter().find_map(|o| o.sequences.iter().find(|s| &s.label == name).map(|s| (o, s))) else { println!("{name}: missing"); continue };
        for &ai in &s.anims {
            let an = o.animation(ai)?;
            let delta = an.flags & 4 != 0;
            let (mut mt, mut mr, mut bt, mut br) = (0f32, 0f32, String::new(), String::new());
            for f in &an.frames {
                for (bi, (p, q)) in f.iter().enumerate() {
                    if !an.animated[bi] || o.bones[bi].name.contains("propHand") { continue; }
                    let (p0, q0) = if delta { ([0.0; 3], [0.0, 0.0, 0.0, 1.0]) } else { an.frames[0][bi] };
                    let t = ((p[0]-p0[0]).powi(2)+(p[1]-p0[1]).powi(2)+(p[2]-p0[2]).powi(2)).sqrt();
                    let d = (q[0]*q0[0]+q[1]*q0[1]+q[2]*q0[2]+q[3]*q0[3]).abs().min(1.0);
                    let r = 2.0 * d.acos().to_degrees();
                    if t > mt { mt = t; bt = o.bones[bi].name.clone(); }
                    if r > mr { mr = r; br = o.bones[bi].name.clone(); }
                }
            }
            println!("{name} anim {ai} {} delta={delta} frames={} max move {mt:.2} ({bt}) max rot {mr:.1} deg ({br})", an.name, an.num_frames);
        }
    }
    Ok(())
}
