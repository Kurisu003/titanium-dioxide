//! Decode VTFs from a VPK to raw RGBA for inspection: `vtfdec <dir.vpk> <outdir> <path.vtf>...`
//! (writes `<name>_<w>x<h>.rgba` outside the repo; never commit the output)
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = tf_assets::vpk::Vpk::open(&a[1])?;
    for p in &a[3..] {
        let t = tf_assets::vtf::decode(&vpk.read(p)?, 512)?;
        let name = p.rsplit('/').next().unwrap().trim_end_matches(".vtf");
        std::fs::write(format!("{}/{name}_{}x{}.rgba", a[2], t.width, t.height), &t.rgba)?;
        println!("{p}: {}x{} fmt {} sheet {} seqs", t.width, t.height, t.format, t.sheet.len());
    }
    Ok(())
}
