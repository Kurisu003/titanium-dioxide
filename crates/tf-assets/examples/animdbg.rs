//! Debug an animation: `animdbg <dir.vpk> <model.mdl> <seq> [bone substring]`
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
    let filt = a.get(4).cloned().unwrap_or("arm".into());
    let mut owners = vec![];
    for inc in &m.include_models { if let Ok(d) = vpk.read(inc) { owners.push(Model::parse(d)?); } }
    let owner = std::iter::once(&m).chain(owners.iter()).find(|o| o.sequences.iter().any(|s| s.label == a[3])).expect("seq");
    let seq = owner.sequences.iter().find(|s| s.label == a[3]).unwrap();
    let an = owner.animation(seq.anims[0])?;
    println!("seq {} anim {} flags {:#x} frames {} ground speed {:.1} last movement {:?}", seq.label, an.name, an.flags, an.num_frames, an.ground_speed(), an.movement.last());
    // global transforms on BT skeleton
    let pose: Vec<([f32;3],[f32;4])> = m.bones.iter().map(|b| match owner.bone_index(&b.name) { Some(i) => an.frames[0][i], None => (b.pos, b.quat) }).collect();
    let mut g: Vec<([f32;3],[f32;4])> = vec![];
    for (i, b) in m.bones.iter().enumerate() {
        let (p, q) = pose[i];
        let gl = if b.parent < 0 { (p, q) } else { let (pp, pq) = g[b.parent as usize]; let r = qrot(pq, p); ([pp[0]+r[0], pp[1]+r[1], pp[2]+r[2]], qmul(pq, q)) };
        g.push(gl);
        if b.name.contains(&filt) || i < 6 {
            println!("{i:3} {:28} parent {:3} in_owner {:5} anim {:5} local p {:?} q {:?} -> global {:?}", b.name, b.parent, owner.bone_index(&b.name).is_some(), owner.bone_index(&b.name).map(|i| an.animated[i]).unwrap_or(false), p.map(|x| (x*10.0).round()/10.0), q.map(|x| (x*100.0).round()/100.0), gl.0.map(|x| x.round()));
        }
    }
    Ok(())
}
