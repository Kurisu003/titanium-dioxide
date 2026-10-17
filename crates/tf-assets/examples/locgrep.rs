//! Print localisation lines matching a pattern: `locgrep <needle>`.
fn main() {
    let needle = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    let root = std::path::Path::new("/mnt/4TB/SteamLibrary/steamapps/common/Titanfall2");
    for v in ["englishclient_frontend", "englishclient_mp_common"] {
        let Ok(vpk) = tf_assets::vpk::Vpk::open(root.join(format!("vpk/{v}.bsp.pak000_dir.vpk")))
        else {
            continue;
        };
        let Ok(raw) = vpk.read("resource/r1_english.txt") else {
            continue;
        };
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let text = String::from_utf16_lossy(&units);
        for line in text.lines() {
            if line.to_lowercase().contains(&needle) {
                println!("{line}");
            }
        }
    }
}
