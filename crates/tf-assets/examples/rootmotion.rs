//! Print root-bone and camera-bone motion of a sequence over time:
//! `rootmotion <dir.vpk> <model.mdl> <seq> [bone...]`
use tf_assets::{mdl::Model, vpk::Vpk};
fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (ax, ay, az, aw) = (a[0], a[1], a[2], a[3]);
    let (bx, by, bz, bw) = (b[0], b[1], b[2], b[3]);
    [aw*bx + ax*bw + ay*bz - az*by, aw*by - ax*bz + ay*bw + az*bx, aw*bz + ax*by - ay*bx + az*bw, aw*bw - ax*bx - ay*by - az*bz]
}
fn qrot(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let p = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), [-q[0], -q[1], -q[2], q[3]]);
    [p[0], p[1], p[2]]
}
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let mut owners = vec![];
    for inc in &m.include_models { if let Ok(d) = vpk.read(inc) { owners.push(Model::parse(d)?); } }
    let owner = std::iter::once(&m).chain(owners.iter()).find(|o| o.sequences.iter().any(|s| s.label == a[3])).expect("seq");
    let seq = owner.sequences.iter().find(|s| s.label == a[3]).unwrap();
    let an = owner.animation(seq.anims[0])?;
    let bones: Vec<String> = if a.len() > 4 { a[4..].to_vec() } else { vec!["jx_c_delta".into(), "def_c_hip".into(), "jx_c_camera".into()] };
    println!("{} frames {} fps {} duration {:.2}s", a[3], an.num_frames, an.fps, (an.num_frames - 1) as f32 / an.fps);
    if !an.movement.is_empty() {
        let n = an.movement.len();
        println!("  movement ({n}): first {:?} mid {:?} last {:?}", an.movement[0], an.movement[n / 2], an.movement[n - 1]);
    }
    let step = if std::env::var_os("ALL_FRAMES").is_some() { 1 } else { (an.num_frames / 8).max(1) };
    for f in (0..an.num_frames).step_by(step).chain(std::iter::once(an.num_frames - 1)) {
        let mut g: Vec<([f32; 3], [f32; 4])> = vec![];
        for (i, b) in owner.bones.iter().enumerate() {
            let (p, q) = an.frames[f][i];
            let gl = if b.parent < 0 { (p, q) } else { let (pp, pq) = g[b.parent as usize]; let r = qrot(pq, p); ([pp[0]+r[0], pp[1]+r[1], pp[2]+r[2]], qmul(pq, q)) };
            g.push(gl);
        }
        if let Some(i) = owner.bones.iter().position(|b| b.name == "jx_c_start") {
            let (p, q) = g[i];
            let yaw = (2.0 * (q[3] * q[2] + q[0] * q[1])).atan2(1.0 - 2.0 * (q[1] * q[1] + q[2] * q[2])).to_degrees();
            let mv = an.movement.get(f).copied().unwrap_or([0.0; 4]);
            println!("  f {f:3}: jx_c_start pos {:?} yaw {yaw:.1}  movement {:?}", p.map(|v| v.round()), mv.map(|v| (v * 10.0).round() / 10.0));
        }
        let line: Vec<String> = bones.iter().filter_map(|n| owner.bone_index(n).map(|i| format!("{n} {:?}", g[i].0.map(|x| x.round())))).collect();
        println!("  f{f:4}: {}", line.join("  "));
    }
    Ok(())
}
