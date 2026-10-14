//! Where a delta clip moves the gun in view space under different conventions:
//! `adsfire <dir.vpk> <model.mdl> <base anim> <delta anim>` (base at its last frame)
use tf_assets::{mdl::Model, vpk::Vpk};
type Q = [f32; 4];
type V = [f32; 3];
fn qmul(a: Q, b: Q) -> Q {
    [a[3]*b[0] + a[0]*b[3] + a[1]*b[2] - a[2]*b[1], a[3]*b[1] - a[0]*b[2] + a[1]*b[3] + a[2]*b[0], a[3]*b[2] + a[0]*b[1] - a[1]*b[0] + a[2]*b[3], a[3]*b[3] - a[0]*b[0] - a[1]*b[1] - a[2]*b[2]]
}
fn conj(q: Q) -> Q { [-q[0], -q[1], -q[2], q[3]] }
fn qrot(q: Q, v: V) -> V { let p = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), conj(q)); [p[0], p[1], p[2]] }
fn sub(a: V, b: V) -> V { [a[0]-b[0], a[1]-b[1], a[2]-b[2]] }
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let m = Model::parse(Vpk::open(&a[1])?.read(&a[2])?)?;
    let base = m.animation(a[3].parse()?)?;
    let delta = m.animation(a[4].parse()?)?;
    let bf = &base.frames[base.num_frames - 1];
    let idx = |n: &str| m.bones.iter().position(|b| b.name == n).unwrap();
    let (cam, muz, wb) = (idx("jx_c_camera"), idx("muzzle_flash"), idx("weapon_bone"));
    for conv in ["none", "post b*d", "pre d*b", "post b*conj(d)"] {
        println!("== {conv}");
        for f in [0usize, 1, 2, 3, 4, 6] {
            let df = &delta.frames[f.min(delta.num_frames - 1)];
            let mut g: Vec<(V, Q)> = Vec::new();
            for (bi, b) in m.bones.iter().enumerate() {
                let (mut p, mut q) = bf[bi];
                let (dp, dq) = df[bi];
                match conv {
                    "none" => {}
                    "post b*d" => { q = qmul(q, dq); p = [p[0]+dp[0], p[1]+dp[1], p[2]+dp[2]]; }
                    "pre d*b" => { q = qmul(dq, q); p = [p[0]+dp[0], p[1]+dp[1], p[2]+dp[2]]; }
                    _ => { q = qmul(q, conj(dq)); p = [p[0]+dp[0], p[1]+dp[1], p[2]+dp[2]]; }
                }
                let w = if b.parent >= 0 { let (pp, pq) = g[b.parent as usize]; let r = qrot(pq, p); ([pp[0]+r[0], pp[1]+r[1], pp[2]+r[2]], qmul(pq, q)) } else { (p, q) };
                g.push(w);
            }
            // View frame: the CAMERA attachment (x forward = bone z, y left = bone x, z up = bone y).
            let (cp, cq) = g[cam];
            let to_view = |v: V| { let l = qrot(conj(cq), v); [l[2], l[0], l[1]] };
            let mz = to_view(sub(g[muz].0, cp));
            let w = to_view(sub(g[wb].0, cp));
            let bar = sub(mz, w);
            let deg = |y: f32, x: f32| y.atan2(x).to_degrees();
            println!("f{f}: muzzle fwd {:6.1} left {:5.1} up {:5.1} (elev {:5.1} az {:5.1}) | barrel elev {:5.1} az {:5.1} | weapon_bone up {:5.1} fwd {:5.1}",
                mz[0], mz[1], mz[2], deg(mz[2], mz[0]), deg(mz[1], mz[0]), deg(bar[2], bar[0]), deg(bar[1], bar[0]), w[2], w[0]);
        }
    }
    Ok(())
}
