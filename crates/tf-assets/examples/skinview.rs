//! Skin an arms model with a weapon viewmodel's animation frame and report, per dominant bone,
//! the vertices in front of the eye inside the view cone:
//! `skinview <weapon.vpk> <weapon.mdl> <anim> <frame> <arms.vpk> <arms.mdl> [half_fov_deg]`
use tf_assets::{mdl::Model, vpk::Vpk};
type V3 = [f32; 3];
type Q = [f32; 4];
fn qmul(a: Q, b: Q) -> Q {
    [a[3]*b[0]+a[0]*b[3]+a[1]*b[2]-a[2]*b[1], a[3]*b[1]-a[0]*b[2]+a[1]*b[3]+a[2]*b[0], a[3]*b[2]+a[0]*b[1]-a[1]*b[0]+a[2]*b[3], a[3]*b[3]-a[0]*b[0]-a[1]*b[1]-a[2]*b[2]]
}
fn qrot(q: Q, v: V3) -> V3 {
    let r = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), [-q[0], -q[1], -q[2], q[3]]);
    [r[0], r[1], r[2]]
}
fn conj(q: Q) -> Q { [-q[0], -q[1], -q[2], q[3]] }
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let w = Model::parse(Vpk::open(&a[1])?.read(&a[2])?)?;
    let an = w.animation(a[3].parse()?)?;
    let f: usize = a[4].parse()?;
    let arms = Model::parse(Vpk::open(&a[5])?.read(&a[6])?)?;
    let half: f32 = a.get(7).and_then(|s| s.parse().ok()).unwrap_or(40.0f32);
    let fr = &an.frames[f.min(an.num_frames - 1)];
    // Arms bones: local from the weapon's frame by name, else the arms' rest.
    let mut world: Vec<(V3, Q)> = Vec::new();
    for b in &arms.bones {
        let (p, q) = match w.bones.iter().position(|x| x.name.eq_ignore_ascii_case(&b.name)) { Some(i) => fr[i], None => (b.pos, b.quat) };
        world.push(if b.parent < 0 { (p, q) } else { let (pp, pq) = world[b.parent as usize]; let r = qrot(pq, p); ([pp[0]+r[0], pp[1]+r[1], pp[2]+r[2]], qmul(pq, q)) });
    }
    let cam = arms.bones.iter().position(|b| b.name == "jx_c_camera").unwrap();
    let (cp, cq) = world[cam];
    let t = half.to_radians().tan();
    let mut by: std::collections::BTreeMap<usize, (usize, f32)> = Default::default();
    for md in arms.meshes(&[0; 8])? {
        let mat = &arms.textures[arms.skin_families[0][md.material] as usize];
        for (i, v) in md.positions.iter().enumerate() {
            let mut s = [0f32; 3];
            for k in 0..4 {
                let (wt, j) = (md.weights[i][k], md.joints[i][k] as usize);
                if wt <= 0.0 { continue; }
                let m = arms.bones[j].pose_to_bone;
                let local = [0, 1, 2].map(|r| m[r][0]*v[0] + m[r][1]*v[1] + m[r][2]*v[2] + m[r][3]);
                let (bp, bq) = world[j];
                let r = qrot(bq, local);
                for c in 0..3 { s[c] += wt * (bp[c] + r[c]); }
            }
            let c = qrot(conj(cq), [s[0]-cp[0], s[1]-cp[1], s[2]-cp[2]]);
            let (fwd, left, up) = (c[2], c[0], c[1]);
            if fwd > 0.0 && left.abs() < fwd * t * 1.4 && up.abs() < fwd * t {
                let k = (0..4).max_by(|&x, &y| md.weights[i][x].total_cmp(&md.weights[i][y])).unwrap();
                let e = by.entry(md.joints[i][k] as usize).or_insert((0, f32::MAX));
                e.0 += 1;
                e.1 = e.1.min(fwd);
                let _ = mat;
            }
        }
    }
    for (b, (n, near)) in by {
        println!("{:24} {n:5} nearest fwd {near:.1}", arms.bones[b].name);
    }
    Ok(())
}
