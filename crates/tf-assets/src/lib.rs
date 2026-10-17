//! Readers for Titanfall 2 asset formats, working directly on a game install.

pub mod bc6h;
pub mod binka;
pub mod bsp;
mod bytes;
pub mod cubemap;
pub mod datatable;
mod guid;
pub mod material;
pub mod mdl;
pub mod miles;
pub mod navmesh;
pub mod pcf;
pub mod rpak;
mod rpak_decompress;
mod rpak_lut;
pub mod settings;
pub mod texture;
pub mod uimg;
pub mod vfont;
pub mod vpk;
pub mod vtf;

/// The GUID an asset is looked up by from its name (`RTech::StringToGuid`; case-insensitive,
/// `\` and `/` alike), e.g. `datatable/titan_properties.rpak`.
pub fn string_to_guid(s: &str) -> u64 {
    guid::string_to_guid(s.as_bytes())
}
