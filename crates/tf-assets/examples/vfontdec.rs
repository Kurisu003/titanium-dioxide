//! Decode a .vfont from a VPK: `vfontdec <vpk> <path> <out.ttf>`
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = tf_assets::vpk::Vpk::open(&a[1])?;
    let out = tf_assets::vfont::decode(&vpk.read(&a[2])?)?;
    eprintln!("{} bytes, magic {:02x?}", out.len(), &out[..4]);
    std::fs::write(&a[3], out)?;
    Ok(())
}
