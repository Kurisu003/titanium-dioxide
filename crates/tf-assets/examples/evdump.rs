//! Hex-dump a sequence's raw event block: `evdump <dir.vpk> <model.mdl> <sequence> [bytes]`
use tf_assets::vpk::Vpk;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let d = Vpk::open(&a[1])?.read(&a[2])?;
    let i32_at = |o: usize| i32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let (n, si) = (i32_at(0xC0) as usize, i32_at(0xC4) as usize);
    let size = (i32_at(si + 0x1C + 0) as usize, 0);
    let _ = size;
    for i in 0..n {
        let o = si + i * tf_assets::mdl::SEQDESC_SIZE;
        let lo = o + i32_at(o + 4) as usize;
        let label: String = d[lo..].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
        if label != a[3] { continue; }
        let (ne, ei) = (i32_at(o + 0x18), i32_at(o + 0x1C) as usize);
        println!("{label}: {ne} events at +{ei:#x}");
        let len: usize = a.get(4).and_then(|v| v.parse().ok()).unwrap_or(0x300);
        for (row, chunk) in d[o + ei..o + ei + len].chunks(16).enumerate() {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            let txt: String = chunk.iter().map(|&b| if (32..127).contains(&b) { b as char } else { '.' }).collect();
            println!("{:04x}: {} {}", row * 16, hex.join(" "), txt);
        }
    }
    Ok(())
}
