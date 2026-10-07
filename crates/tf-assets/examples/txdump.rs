//! Decode every texture in an RPAK to raw RGBA files: `txdump <file.rpak> <outdir>` (writes
//! `<guid>_<w>x<h>.rgba`; convert with ImageMagick). For looking at assets only, never commit.
//! An optional third argument keeps only textures whose name contains it.
use tf_assets::texture::{self, TexFormat};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let pak = tf_assets::rpak::Rpak::open(&a[1])?;
    for asset in pak.assets.iter().filter(|x| x.kind_str() == "txtr") {
        let Ok(t) = texture::load(&pak, asset) else { continue };
        if let Some(f) = a.get(3) {
            if !t.info.name.as_deref().is_some_and(|n| n.to_ascii_lowercase().contains(&f.to_ascii_lowercase())) {
                continue;
            }
        }
        let (w, h) = (t.width as usize, t.height as usize);
        let mut px = vec![0u32; w * h];
        let ok = match t.info.format {
            TexFormat::Bc1 | TexFormat::Bc1Srgb => texture2ddecoder::decode_bc1(&t.data, w, h, &mut px).is_ok(),
            TexFormat::Bc3 | TexFormat::Bc3Srgb => texture2ddecoder::decode_bc3(&t.data, w, h, &mut px).is_ok(),
            TexFormat::Bc4 => texture2ddecoder::decode_bc4(&t.data, w, h, &mut px).is_ok(),
            TexFormat::Bc5 => texture2ddecoder::decode_bc5(&t.data, w, h, &mut px).is_ok(),
            TexFormat::Bc7 | TexFormat::Bc7Srgb => texture2ddecoder::decode_bc7(&t.data, w, h, &mut px).is_ok(),
            TexFormat::Rgba8 | TexFormat::Rgba8Srgb => {
                for (i, c) in t.data.chunks_exact(4).take(w * h).enumerate() {
                    px[i] = u32::from_le_bytes([c[2], c[1], c[0], c[3]]);
                }
                true
            }
            _ => false,
        };
        eprintln!("{:016x} {:?} {}x{} {:?} {}", t.info.guid, t.info.name, w, h, t.info.format, if ok { "" } else { "(skipped)" });
        if ok {
            // texture2ddecoder gives BGRA in a u32.
            let bytes: Vec<u8> = px.iter().flat_map(|p| { let b = p.to_le_bytes(); [b[2], b[1], b[0], b[3]] }).collect();
            std::fs::write(format!("{}/{:016x}_{w}x{h}.rgba", a[2], t.info.guid), bytes)?;
        }
    }
    Ok(())
}
