//! Bone positions of one animation frame relative to jx_c_camera (x fwd, y left, z up after the
//! delta bone): `posecam <dir.vpk> <model.mdl> <anim> [frame]`
type Vec3 = [f32; 3];
fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[3]*b[0]+a[0]*b[3]+a[1]*b[2]-a[2]*b[1], a[3]*b[1]-a[0]*b[2]+a[1]*b[3]+a[2]*b[0], a[3]*b[2]+a[0]*b[1]-a[1]*b[0]+a[2]*b[3], a[3]*b[3]-a[0]*b[0]-a[1]*b[1]-a[2]*b[2]]
}
fn qrot(q: [f32; 4], v: Vec3) -> Vec3 {
    let r = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), [-q[0], -q[1], -q[2], q[3]]);
    [r[0], r[1], r[2]]
}
fn conj(q: [f32; 4]) -> [f32; 4] { [-q[0], -q[1], -q[2], q[3]] }
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let an = m.animation(a[3].parse()?)?;
    let f: usize = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let fr = &an.frames[f.min(an.num_frames - 1)];
    let mut w: Vec<(Vec3, [f32; 4])> = Vec::new();
    for (i, b) in m.bones.iter().enumerate() {
        let (p, q) = fr[i];
        w.push(if b.parent < 0 { (p, q) } else { let (pp, pq) = w[b.parent as usize]; let r = qrot(pq, p); ([pp[0]+r[0], pp[1]+r[1], pp[2]+r[2]], qmul(pq, q)) });
    }
    let cam = m.bones.iter().position(|b| b.name == "jx_c_camera").unwrap();
    let (cp, cq) = w[cam];
    println!("camera {cp:?} fwd(Z) {:?} up(Y) {:?}", qrot(cq, [0.0, 0.0, 1.0]), qrot(cq, [0.0, 1.0, 0.0]));
    for (i, b) in m.bones.iter().enumerate() {
        // Camera frame: forward = bone Z, left = bone X, up = bone Y.
        let d = w[i].0;
        let p = qrot(conj(cq), [d[0]-cp[0], d[1]-cp[1], d[2]-cp[2]]);
        println!("{i:3} {:24} fwd {:7.2} left {:7.2} up {:7.2}", b.name, p[2], p[0], p[1]);
    }
    Ok(())
}
