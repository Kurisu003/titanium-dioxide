//! Decode a .binka to raw interleaved f32: `binkadec in.binka out.f32`
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    let pcm = tf_assets::binka::decode(&std::fs::read(&a[1])?)?;
    eprintln!("{} ch, {} Hz, {} frames in {:?}", pcm.channels, pcm.sample_rate, pcm.samples.len() / pcm.channels, t.elapsed());
    let bytes: Vec<u8> = pcm.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    std::fs::write(&a[2], bytes)?;
    Ok(())
}
