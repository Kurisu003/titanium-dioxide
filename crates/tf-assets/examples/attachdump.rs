//! Dump a model's attachments (name, bone, local position): `attachdump <dir.vpk> <model.mdl>`
use tf_assets::vpk::Vpk;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let raw = vpk.read(&a[2])?;
    let i32at = |o: usize| i32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
    let f32at = |o: usize| f32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
    let cstr = |o: usize| { let e = raw[o..].iter().position(|&b| b == 0).unwrap_or(0); String::from_utf8_lossy(&raw[o..o + e]).into_owned() };
    let (n, idx) = (i32at(0xF4) as usize, i32at(0xF8) as usize);
    let (nb, bi) = (i32at(0xA0) as usize, i32at(0xA4) as usize);
    println!("{n} attachments at {idx:#x}");
    for i in 0..n {
        let o = idx + i * 0x5C;
        let name = cstr(o + i32at(o) as usize);
        let bone = i32at(o + 8) as usize;
        let m: Vec<f32> = (0..12).map(|k| f32at(o + 12 + k * 4)).collect();
        let bname = if bone < nb { let bo = bi + bone * 0xF4; cstr(bo + i32at(bo) as usize) } else { "?".into() };
        println!("{name}: bone {bone} ({bname}) pos [{:.1} {:.1} {:.1}] x [{:.2} {:.2} {:.2}] y [{:.2} {:.2} {:.2}] z [{:.2} {:.2} {:.2}]", m[3], m[7], m[11], m[0], m[4], m[8], m[1], m[5], m[9], m[2], m[6], m[10]);
    }
    Ok(())
}
