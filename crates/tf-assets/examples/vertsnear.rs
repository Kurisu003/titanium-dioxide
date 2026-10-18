//! Bind-space vertices near a point given in a bone's space (socket checks):
//! `vertsnear <dir.vpk> <model.mdl> <bone> <x> <y> <z> <radius>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let bi = m
        .bones
        .iter()
        .position(|b| b.name.eq_ignore_ascii_case(&a[3]))
        .ok_or_else(|| anyhow::anyhow!("no bone {}", a[3]))?;
    let off: Vec<f32> = a[4..7].iter().map(|s| s.parse().unwrap()).collect();
    let r: f32 = a[7].parse()?;
    // bind world = inverse(pose_to_bone): rotation part is orthonormal.
    let p2b = m.bones[bi].pose_to_bone;
    let rot = [[p2b[0][0], p2b[0][1], p2b[0][2]], [p2b[1][0], p2b[1][1], p2b[1][2]], [p2b[2][0], p2b[2][1], p2b[2][2]]];
    let t = [p2b[0][3], p2b[1][3], p2b[2][3]];
    // world = R^T * (bone - t)
    let bone = off;
    let d = [bone[0] - t[0], bone[1] - t[1], bone[2] - t[2]];
    let c = [
        rot[0][0] * d[0] + rot[1][0] * d[1] + rot[2][0] * d[2],
        rot[0][1] * d[0] + rot[1][1] * d[1] + rot[2][1] * d[2],
        rot[0][2] * d[0] + rot[1][2] * d[1] + rot[2][2] * d[2],
    ];
    println!("bone {} bind origin {:?}; point {:?}", a[3], [-(rot[0][0]*t[0]+rot[1][0]*t[1]+rot[2][0]*t[2]), -(rot[0][1]*t[0]+rot[1][1]*t[1]+rot[2][1]*t[2]), -(rot[0][2]*t[0]+rot[1][2]*t[1]+rot[2][2]*t[2])], c);
    let mut n = 0;
    for md in m.meshes(&[0; 16])? {
        for (i, p) in md.positions.iter().enumerate() {
            let dd = ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt();
            if dd <= r {
                let (j, w) = (md.joints[i], md.weights[i]);
                let bones: Vec<String> = (0..4)
                    .filter(|&k| w[k] > 0.001)
                    .map(|k| format!("{}:{:.2}", m.bones[j[k] as usize].name, w[k]))
                    .collect();
                println!("{:8.2} {:8.2} {:8.2}  rel {:6.2} {:6.2} {:6.2}  {}", p[0], p[1], p[2], p[0]-c[0], p[1]-c[1], p[2]-c[2], bones.join(" "));
                n += 1;
            }
        }
    }
    println!("{n} vertices within {r}");
    Ok(())
}
