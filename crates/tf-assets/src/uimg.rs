//! `uimg` (version 10) UI image atlases: named sub-images of one atlas texture.
//! Layout from rsx (r-ex/rsx, `ui_image_atlas.h/.cpp`).

use crate::rpak::{Asset, Rpak};
use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct UiImage {
    /// RTech::StringToUIMGHash of the image path (e.g. "rui/menu/main_menu/title").
    pub hash: u32,
    /// Pixel rectangle in the atlas texture.
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone)]
pub struct UiAtlas {
    pub guid: u64,
    pub texture: u64,
    pub width: u32,
    pub height: u32,
    pub images: Vec<UiImage>,
}

/// The hash UI image paths are looked up by: the asset GUID's halves XORed.
pub fn path_hash(path: &str) -> u32 {
    let g = crate::guid::string_to_guid(path.as_bytes());
    (g as u32) ^ ((g >> 32) as u32)
}

pub fn load(pak: &Rpak, a: &Asset) -> Result<UiAtlas> {
    let h = a.head.context("uimg without header")?;
    let width = pak.u16(h + 8) as u32;
    let height = pak.u16(h + 10) as u32;
    let count = pak.u16(h + 12) as usize;
    let hashes = pak.ptr(h + 40).context("uimg without hashes")?;
    let texture = pak.u64(h + 56);
    let bounds = a.cpu.context("uimg without bounds")?;
    let f = |o: usize| f32::from_bits(pak.u32(o));
    let images = (0..count)
        .map(|i| {
            let b = bounds + i * 16;
            let (minx, miny, sx, sy) = (f(b), f(b + 4), f(b + 8), f(b + 12));
            UiImage {
                hash: pak.u32(hashes + i * 8),
                x: (minx * width as f32) as u32,
                y: (miny * height as f32) as u32,
                w: (sx * width as f32 + 0.5) as u32,
                h: (sy * height as f32 + 0.5) as u32,
            }
        })
        .collect();
    Ok(UiAtlas { guid: a.guid, texture, width, height, images })
}
