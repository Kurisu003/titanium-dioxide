//! Titanfall 2 RPAK (version 7) reader.
//!
//! An RPAK is a header, some tables, then "pages" of data. Assets point into pages with
//! (page index, offset) pairs; the pointer table lists every such pair stored *inside*
//! page data, which the game rewrites into real pointers on load. We do the same, but
//! rewrite them into absolute offsets into our decompressed buffer, so asset structs can
//! be read with plain offset arithmetic.
//!
//! Patch chains (`name(01).rpak` ...) are not applied as deltas; a patch pak's own pages can be
//! read on their own (the viewer indexes new assets from `common(NN).rpak` that way).

use crate::rpak_decompress;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const HEADER_SIZE_V7: usize = 0x58;
const ASSET_SIZE_V7: usize = 0x48;

#[derive(Debug, Clone, Copy)]
pub struct Segment {
    pub flags: u32,
    pub align: u32,
    pub size: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct Page {
    pub segment: u32,
    pub align: u32,
    pub size: u32,
    /// Offset of the page's data in [`Rpak::data`].
    pub start: usize,
}

#[derive(Debug, Clone)]
pub struct Asset {
    pub guid: u64,
    /// Absolute offset of the asset header in [`Rpak::data`], if any.
    pub head: Option<usize>,
    /// Absolute offset of the asset's CPU data, if any.
    pub cpu: Option<usize>,
    pub starpak_offset: i64,
    pub header_size: u32,
    pub version: u32,
    pub kind: [u8; 4],
    pub dependencies: Vec<u64>,
}

impl Asset {
    pub fn kind_str(&self) -> String {
        String::from_utf8_lossy(&self.kind).trim_end_matches('\0').to_string()
    }
}

pub struct Rpak {
    pub path: PathBuf,
    pub version: u16,
    pub flags: u16,
    pub data: Vec<u8>,
    pub starpaks: Vec<String>,
    pub pages: Vec<Page>,
    pub segments: Vec<Segment>,
    pub assets: Vec<Asset>,
    pub by_guid: HashMap<u64, usize>,
}

fn rd_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn rd_u64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

impl Rpak {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let raw = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        if raw.len() < HEADER_SIZE_V7 || &raw[0..4] != b"RPak" {
            bail!("{} is not an RPAK", path.display());
        }
        let version = rd_u16(&raw, 4);
        if version != 7 {
            bail!("{}: rpak version {version} unsupported (Titanfall 2 uses 7)", path.display());
        }
        let flags = rd_u16(&raw, 6);
        let data = if rpak_decompress::is_rtech_compressed(flags) {
            rpak_decompress::decompress_pak(&raw, HEADER_SIZE_V7)?
        } else {
            raw
        };
        let dcmp_size = rd_u64(&data, 0x28) as usize;
        if data.len() != dcmp_size {
            bail!("rpak: decompressed {} bytes, header says {}", data.len(), dcmp_size);
        }
        Self::parse(path, data)
    }

    fn parse(path: PathBuf, mut data: Vec<u8>) -> Result<Self> {
        let h = &data;
        let version = rd_u16(h, 4);
        let flags = rd_u16(h, 6);
        let starpak_buf = rd_u16(h, 0x38) as usize;
        let num_segments = rd_u16(h, 0x3A) as usize;
        let num_pages = rd_u16(h, 0x3C) as usize;
        let patch_count = rd_u16(h, 0x3E) as usize;
        let num_pointers = rd_u32(h, 0x40) as usize;
        let num_assets = rd_u32(h, 0x44) as usize;
        let num_guid_refs = rd_u32(h, 0x48) as usize;
        let num_deps = rd_u32(h, 0x4C) as usize;
        let num_ext_refs = rd_u32(h, 0x50) as usize;
        let ext_refs_size = rd_u32(h, 0x54) as usize;

        let mut off = HEADER_SIZE_V7;
        let mut first_page = 0usize;
        let mut patch_stream_size = 0usize;
        if patch_count != 0 {
            patch_stream_size = rd_u32(h, off) as usize;
            first_page = rd_u32(h, off + 4) as usize;
            off += 8 + 16 * patch_count + 2 * patch_count;
        }

        let mut starpaks = Vec::new();
        for s in h[off..off + starpak_buf].split(|&c| c == 0) {
            if !s.is_empty() {
                starpaks.push(String::from_utf8_lossy(s).to_string());
            }
        }
        off += starpak_buf;

        let mut segments = Vec::with_capacity(num_segments);
        for i in 0..num_segments {
            let o = off + i * 16;
            segments.push(Segment { flags: rd_u32(h, o), align: rd_u32(h, o + 4), size: rd_u64(h, o + 8) });
        }
        off += num_segments * 16;

        let mut pages = Vec::with_capacity(num_pages);
        for i in 0..num_pages {
            let o = off + i * 12;
            pages.push(Page { segment: rd_u32(h, o), align: rd_u32(h, o + 4), size: rd_u32(h, o + 8), start: usize::MAX });
        }
        off += num_pages * 12;

        let pointers_off = off;
        off += num_pointers * 8;
        let assets_off = off;
        off += num_assets * ASSET_SIZE_V7;
        let guid_refs_off = off;
        off += num_guid_refs * 8;
        off += num_deps * 4;
        if num_ext_refs != 0 {
            off += num_ext_refs * 4 + ext_refs_size;
        }
        off += patch_stream_size;

        // Pages below `first_page` live in older paks of a patch chain; we don't have them.
        for p in pages.iter_mut().skip(first_page) {
            p.start = off;
            off += p.size as usize;
        }
        if off > data.len() {
            bail!("rpak: page data runs past end of file ({off} > {})", data.len());
        }

        let page_addr = |pages: &[Page], idx: u32, offset: u32| -> Option<usize> {
            let p = pages.get(idx as usize)?;
            if p.start == usize::MAX {
                return None;
            }
            Some(p.start + offset as usize)
        };

        // Rewrite every in-page pointer into an absolute buffer offset.
        for i in 0..num_pointers {
            let o = pointers_off + i * 8;
            let (pi, po) = (rd_u32(&data, o), rd_u32(&data, o + 4));
            let Some(slot) = page_addr(&pages, pi, po) else { continue };
            let (ti, to) = (rd_u32(&data, slot), rd_u32(&data, slot + 4));
            let abs = page_addr(&pages, ti, to).map(|a| a as u64).unwrap_or(0);
            data[slot..slot + 8].copy_from_slice(&abs.to_le_bytes());
        }

        let mut assets = Vec::with_capacity(num_assets);
        let mut by_guid = HashMap::new();
        for i in 0..num_assets {
            let o = assets_off + i * ASSET_SIZE_V7;
            let ptr = |o: usize| {
                let (idx, off) = (rd_u32(&data, o), rd_u32(&data, o + 4));
                if idx == u32::MAX {
                    None
                } else {
                    page_addr(&pages, idx, off)
                }
            };
            let deps_index = rd_u32(&data, o + 0x30) as usize;
            let deps_count = rd_u32(&data, o + 0x38) as usize;
            let mut dependencies = Vec::with_capacity(deps_count);
            for d in 0..deps_count {
                let r = guid_refs_off + (deps_index + d) * 8;
                if let Some(slot) = page_addr(&pages, rd_u32(&data, r), rd_u32(&data, r + 4)) {
                    dependencies.push(rd_u64(&data, slot));
                }
            }
            let asset = Asset {
                guid: rd_u64(&data, o),
                head: ptr(o + 0x10),
                cpu: ptr(o + 0x18),
                starpak_offset: rd_u64(&data, o + 0x20) as i64,
                header_size: rd_u32(&data, o + 0x3C),
                version: rd_u32(&data, o + 0x40),
                kind: data[o + 0x44..o + 0x48].try_into().unwrap(),
                dependencies,
            };
            by_guid.insert(asset.guid, i);
            assets.push(asset);
        }

        Ok(Self { path, version, flags, data, starpaks, pages, segments, assets, by_guid })
    }

    pub fn asset(&self, guid: u64) -> Option<&Asset> {
        self.by_guid.get(&guid).map(|&i| &self.assets[i])
    }

    pub fn u8(&self, o: usize) -> u8 {
        self.data[o]
    }
    pub fn u16(&self, o: usize) -> u16 {
        rd_u16(&self.data, o)
    }
    pub fn u32(&self, o: usize) -> u32 {
        rd_u32(&self.data, o)
    }
    pub fn u64(&self, o: usize) -> u64 {
        rd_u64(&self.data, o)
    }
    /// Read a pointer slot that was rewritten at load (0 = null).
    pub fn ptr(&self, o: usize) -> Option<usize> {
        match rd_u64(&self.data, o) {
            0 => None,
            v => Some(v as usize),
        }
    }
    pub fn cstr(&self, o: usize) -> String {
        let end = self.data[o..].iter().position(|&c| c == 0).unwrap_or(0);
        String::from_utf8_lossy(&self.data[o..o + end]).to_string()
    }

    /// Directory holding this pak and its starpaks.
    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap()
    }

    /// Read `size` bytes of streamed data for an asset from its starpak.
    pub fn read_starpak(&self, packed_offset: i64, size: usize) -> Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        if packed_offset < 0 {
            bail!("asset has no streamed data");
        }
        let idx = (packed_offset & 0xFFF) as usize;
        let offset = (packed_offset as u64) & !0xFFF;
        let name = self.starpaks.get(idx).context("starpak index out of range")?;
        let file_name = name.rsplit(['/', '\\']).next().unwrap();
        let path = self.dir().join(file_name);
        let mut f = std::fs::File::open(&path).with_context(|| format!("open {}", path.display()))?;
        f.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; size];
        f.read_exact(&mut buf)?;
        Ok(buf)
    }
}


/// Find the self-contained pak for `name` (e.g. "mp_rise.rpak") in `dir`: the plain file if it
/// exists, else the earliest `name(NN).rpak` that isn't a patch (DLC maps ship that way).
pub fn resolve(dir: &Path, file: &str) -> Option<std::path::PathBuf> {
    let plain = dir.join(file);
    if plain.exists() {
        return Some(plain);
    }
    let stem = file.strip_suffix(".rpak")?;
    for n in 1..=99 {
        let p = dir.join(format!("{stem}({n:02}).rpak"));
        let Ok(mut f) = std::fs::File::open(&p) else { continue };
        let mut h = [0u8; 0x40];
        if std::io::Read::read_exact(&mut f, &mut h).is_ok() && u16::from_le_bytes([h[0x3E], h[0x3F]]) == 0 {
            return Some(p);
        }
    }
    None
}
