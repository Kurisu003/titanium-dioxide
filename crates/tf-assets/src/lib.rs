//! Readers for Titanfall 2 asset formats, working directly on a game install.

mod guid;
pub mod bc6h;
pub mod binka;
pub mod cubemap;
pub mod bsp;
pub mod material;
pub mod miles;
pub mod pcf;
pub mod mdl;
pub mod navmesh;
pub mod rpak;
pub mod settings;
mod rpak_decompress;
mod rpak_lut;
pub mod texture;
pub mod uimg;
pub mod datatable;
pub mod vfont;
pub mod vpk;
pub mod vtf;

pub fn string_to_guid(s: &str) -> u64 {
    guid::string_to_guid(s.as_bytes())
}
