//! Write a map's own collision (CM_PRIMITIVES brushes and tricoll meshes, no static props) as a
//! tf-sim collision dump (TFCW1: solid, player clips, titan clips) for comparing with the
//! movement collision the game builds: `cmdump <map vpk> <map> <out file>`
use tf_assets::bsp::contents::{BULLET, PLAYER, TITAN};
use tf_assets::{bsp::Bsp, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let map = &a[2];
    let data = vpk.read(&format!("maps/{map}.bsp"))?;
    let bsp = Bsp::parse(map, &data, |i| vpk.read(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok())?;
    let mut sets: [Vec<[[f32; 3]; 3]>; 3] = Default::default();
    for shape in bsp.collision_shapes() {
        let c = shape.contents;
        for t in &shape.tris {
            if c & BULLET != 0 && c & PLAYER != 0 {
                sets[0].push(*t);
            } else {
                if c & PLAYER != 0 {
                    sets[1].push(*t);
                }
                if c & TITAN != 0 {
                    sets[2].push(*t);
                }
            }
        }
    }
    let mut out = b"TFCW1".to_vec();
    for s in &sets {
        out.extend_from_slice(&(s.len() as u32).to_le_bytes());
        for t in s {
            for v in t {
                for x in v {
                    out.extend_from_slice(&x.to_le_bytes());
                }
            }
        }
    }
    std::fs::write(&a[3], out)?;
    println!("solid {} player clip {} titan clip {}", sets[0].len(), sets[1].len(), sets[2].len());
    Ok(())
}
