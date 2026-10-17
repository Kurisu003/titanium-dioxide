//! Model-space bone positions in one animation frame: `bonepos <dir.vpk> <model.mdl> <anim index> [frame] [name filter]`
use tf_assets::{mdl::Model, vpk::Vpk};
fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
fn qrot(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let p = qmul(
        qmul(q, [v[0], v[1], v[2], 0.0]),
        [-q[0], -q[1], -q[2], q[3]],
    );
    [p[0], p[1], p[2]]
}
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let m = Model::parse(Vpk::open(&a[1])?.read(&a[2])?)?;
    let an = m.animation(a[3].parse()?)?;
    let f: usize = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let filt = a.get(5).cloned().unwrap_or_default();
    let fr = &an.frames[f.min(an.num_frames - 1)];
    let mut g: Vec<([f32; 3], [f32; 4])> = Vec::new();
    for (bi, b) in m.bones.iter().enumerate() {
        let (p, q) = fr[bi];
        let w = if b.parent >= 0 {
            let (pp, pq) = g[b.parent as usize];
            let r = qrot(pq, p);
            ([pp[0] + r[0], pp[1] + r[1], pp[2] + r[2]], qmul(pq, q))
        } else {
            (p, q)
        };
        g.push(w);
        if b.name.contains(&filt) {
            println!(
                "{bi:3} {:24} {:7.1} {:7.1} {:7.1}",
                b.name, w.0[0], w.0[1], w.0[2]
            );
        }
    }
    Ok(())
}
