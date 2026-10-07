//! Dump a model's raw hitbox sets: `hbdump <dir.vpk> <model.mdl> [stride]`
use tf_assets::vpk::Vpk;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let d = Vpk::open(&a[1])?.read(&a[2])?;
    let i32_at = |o: usize| i32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let f32_at = |o: usize| f32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let cstr = |o: usize| d[o..].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect::<String>();
    let (n, idx) = (i32_at(0xB0) as usize, i32_at(0xB4) as usize);
    let stride: usize = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(68);
    println!("{n} hitbox sets at {idx:#x}");
    for s in 0..n {
        let o = idx + s * 12;
        let (nm, nh, hi) = (i32_at(o), i32_at(o + 4) as usize, i32_at(o + 8) as usize);
        println!("set {s} '{}' {nh} boxes", cstr(o + nm as usize));
        for k in 0..nh.min(40) {
            let b = o + hi + k * stride;
            println!(
                "  bone {:3} group {:2} min [{:.1} {:.1} {:.1}] max [{:.1} {:.1} {:.1}] name '{}' extra {:?}",
                i32_at(b),
                i32_at(b + 4),
                f32_at(b + 8),
                f32_at(b + 12),
                f32_at(b + 16),
                f32_at(b + 20),
                f32_at(b + 24),
                f32_at(b + 28),
                cstr((b as i64 + i32_at(b + 36) as i64) as usize),
                (32..stride).step_by(4).map(|x| i32_at(b + x)).collect::<Vec<_>>()
            );
        }
    }
    Ok(())
}
