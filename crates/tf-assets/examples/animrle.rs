//! RLE tracks of one bone in one animation: `animrle <dir.vpk> <model.mdl> <anim index> <bone>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let raw = &m.raw;
    let i32_at = |o: usize| i32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
    let i16_at = |o: usize| i16::from_le_bytes(raw[o..o + 2].try_into().unwrap());
    let (idx, want): (usize, usize) = (a[3].parse()?, a[4].parse()?);
    let ad = i32_at(0xBC) as usize + idx * 0x5C;
    let mut p = ad + i32_at(ad + 0x20) as usize;
    loop {
        let (bone, flags) = (raw[p + 4] as usize, raw[p + 5]);
        if bone == want {
            println!(
                "bone {bone} flags {flags:#x} header {:02x?}",
                &raw[p..p + 32]
            );
            for (name, vp) in [("rot", p + 8), ("pos", p + 16)] {
                for k in 0..3 {
                    let off = i16_at(vp + k * 2);
                    print!("{name}{k} off {off}:");
                    if off > 0 {
                        let mut v = vp + off as usize;
                        let mut frames = 0;
                        while frames < 70 {
                            let (valid, total) = (raw[v] as usize, raw[v + 1] as usize);
                            let vals: Vec<i16> = (1..=valid).map(|j| i16_at(v + j * 2)).collect();
                            print!(" [{valid}/{total} {:?}]", vals);
                            if total == 0 {
                                break;
                            }
                            frames += total;
                            v += (valid + 1) * 2;
                        }
                    }
                    println!();
                }
            }
        }
        let nx = i32_at(p + 28);
        if nx == 0 {
            break;
        }
        p = (p as i64 + nx as i64) as usize;
    }
    Ok(())
}
