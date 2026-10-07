//! Cut named images out of an rpak's UI atlases as raw RGBA: `uimgdump <ui.rpak> <outdir> <rui path>...`
//! (writes `<last path part>_<w>x<h>.rgba`). For looking at assets only, never commit.
use tf_assets::texture::{self, TexFormat};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let pak = tf_assets::rpak::Rpak::open(&a[1])?;
    let want: Vec<(u32, &str)> = a[3..].iter().map(|p| (tf_assets::uimg::path_hash(p), p.rsplit('/').next().unwrap_or(p))).collect();
    for asset in pak.assets.iter().filter(|x| x.kind_str() == "uimg") {
        let Ok(atlas) = tf_assets::uimg::load(&pak, asset) else { continue };
        let Some(ta) = pak.assets.iter().find(|t| t.guid == atlas.texture) else { continue };
        let Ok(t) = texture::load(&pak, ta) else { continue };
        let (w, h) = (t.width as usize, t.height as usize);
        let mut px = vec![0u32; w * h];
        let ok = match t.info.format {
            TexFormat::Bc1 | TexFormat::Bc1Srgb => texture2ddecoder::decode_bc1(&t.data, w, h, &mut px).is_ok(),
            TexFormat::Bc3 | TexFormat::Bc3Srgb => texture2ddecoder::decode_bc3(&t.data, w, h, &mut px).is_ok(),
            TexFormat::Bc7 | TexFormat::Bc7Srgb => texture2ddecoder::decode_bc7(&t.data, w, h, &mut px).is_ok(),
            TexFormat::Rgba8 | TexFormat::Rgba8Srgb => {
                for (i, c) in t.data.chunks_exact(4).take(w * h).enumerate() {
                    px[i] = u32::from_le_bytes([c[2], c[1], c[0], c[3]]);
                }
                true
            }
            _ => false,
        };
        if !ok {
            continue;
        }
        for img in &atlas.images {
            let Some((_, name)) = want.iter().find(|(hh, _)| *hh == img.hash) else { continue };
            let mut out = Vec::with_capacity((img.w * img.h * 4) as usize);
            for y in img.y..img.y + img.h {
                for x in img.x..img.x + img.w {
                    let b = px[(y as usize).min(h - 1) * w + (x as usize).min(w - 1)].to_le_bytes();
                    out.extend_from_slice(&[b[2], b[1], b[0], b[3]]);
                }
            }
            std::fs::write(format!("{}/{name}_{}x{}.rgba", a[2], img.w, img.h), out)?;
            eprintln!("{name}: {}x{}", img.w, img.h);
        }
    }
    Ok(())
}
