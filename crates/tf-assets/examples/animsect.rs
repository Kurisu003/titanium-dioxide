//! Section layout of one animation: `animsect <dir.vpk> <model.mdl> <anim index>`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let raw = &m.raw;
    let i32_at = |o: usize| i32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
    let idx: usize = a[3].parse()?;
    let ad = i32_at(0xBC) as usize + idx * 0x5C;
    let nf = i32_at(ad + 0x10);
    let (ai, si, sf) = (i32_at(ad + 0x20), i32_at(ad + 0x34), i32_at(ad + 0x38));
    println!("frames {nf} animindex {ai:#x} sectionindex {si:#x} sectionframes {sf}");
    for w in 0..0x5C / 4 {
        print!("{:08x} ", i32_at(ad + w * 4));
    }
    println!();
    if sf > 0 {
        let n = (nf - 1) / sf + 2;
        for s in 0..n as usize {
            let d = i32_at(ad + si as usize + s * 4);
            print!("section {s}: {d:#x} bones:");
            if d != 0 {
                let mut p = ad + d as usize;
                for _ in 0..80 {
                    print!(" {}:{:x}", raw[p + 4], raw[p + 5]);
                    let nx = i32_at(p + 28);
                    if nx == 0 { break; }
                    p = (p as i64 + nx as i64) as usize;
                }
            }
            println!();
        }
    }
    Ok(())
}
