//! Titanfall 2 BSP (`rBSP` version 37) reader: world render meshes and static props.
//!
//! Layouts follow snake-biscuits/bsp_tool (branches/respawn/titanfall{,2}.py).

use anyhow::{bail, Context, Result};

pub const NUM_LUMPS: usize = 128;

pub mod lump {
    pub const ENTITIES: usize = 0x00;
    pub const TEXTURE_DATA: usize = 0x02;
    pub const VERTICES: usize = 0x03;
    pub const MODELS: usize = 0x0E;
    pub const VERTEX_NORMALS: usize = 0x1E;
    pub const GAME_LUMP: usize = 0x23;
    pub const TEXTURE_DATA_STRING_DATA: usize = 0x2B;
    pub const TEXTURE_DATA_STRING_TABLE: usize = 0x2C;
    pub const VERTEX_UNLIT: usize = 0x47;
    pub const VERTEX_LIT_FLAT: usize = 0x48;
    pub const VERTEX_LIT_BUMP: usize = 0x49;
    pub const VERTEX_UNLIT_TS: usize = 0x4A;
    pub const MESH_INDICES: usize = 0x4F;
    pub const MESHES: usize = 0x50;
    pub const MATERIAL_SORTS: usize = 0x52;
    pub const LIGHTMAP_HEADERS: usize = 0x53;
    pub const LIGHTMAP_DATA_SKY: usize = 0x62;
    pub const PAKFILE: usize = 0x28;
    pub const CUBEMAPS: usize = 0x2A;
    pub const LIGHTPROBES: usize = 0x65;
    pub const LIGHTPROBE_REFERENCES: usize = 0x68;
}

pub mod mesh_flags {
    pub const SKY_2D: u32 = 0x2;
    pub const SKY: u32 = 0x4;
    pub const WARP: u32 = 0x8;
    pub const TRANSLUCENT: u32 = 0x10;
    pub const VERTEX_MASK: u32 = 0x600;
    pub const VERTEX_LIT_FLAT: u32 = 0x000;
    pub const VERTEX_LIT_BUMP: u32 = 0x200;
    pub const VERTEX_UNLIT: u32 = 0x400;
    pub const VERTEX_UNLIT_TS: u32 = 0x600;
    pub const SKIP: u32 = 0x20000;
    pub const TRIGGER: u32 = 0x40000;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LumpHeader {
    pub offset: u32,
    pub length: u32,
    pub version: u32,
}

/// A loaded map. Lumps are stored as raw byte vectors.
pub struct Bsp {
    pub name: String,
    pub version: u32,
    pub headers: [LumpHeader; NUM_LUMPS],
    pub lumps: Vec<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub first_index: u32,
    pub num_triangles: u32,
    pub material_sort: u16,
    pub flags: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct MaterialSort {
    pub texture_data: i16,
    /// Index into LIGHTMAP_HEADERS (-1: none).
    pub lightmap: i16,
    pub cubemap: i16,
    pub vertex_offset: i32,
}

/// One lightmap page: `sky_a` holds baked indirect/sky light (RGB), `sky_b` a light direction
/// (RGB) and sun visibility (A). Both are RGBA8, `width` x `height`.
#[derive(Debug, Clone)]
pub struct LightmapPage {
    pub width: u32,
    pub height: u32,
    pub sky_a: Vec<u8>,
    pub sky_b: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TextureData {
    pub name: String,
    pub size: [i32; 2],
    pub flags: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct BrushModel {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub first_mesh: u32,
    pub num_meshes: u32,
}

/// A render vertex resolved from one of the VERTEX_RESERVED_X lumps.
#[derive(Debug, Clone, Copy, Default)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [u8; 4],
    /// Lightmap UV (lit vertex formats only).
    pub lightmap_uv: Option<[f32; 2]>,
}

#[derive(Debug, Clone)]
pub struct StaticProp {
    pub origin: [f32; 3],
    /// pitch, yaw, roll in degrees (Source QAngle order).
    pub angles: [f32; 3],
    pub scale: f32,
    pub model: u16,
    pub solid: u8,
    pub flags: u8,
    pub skin: u16,
}

#[derive(Debug, Clone, Default)]
pub struct StaticProps {
    pub model_names: Vec<String>,
    pub props: Vec<StaticProp>,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn i16_at(b: &[u8], o: usize) -> i16 {
    i16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn vec3_at(b: &[u8], o: usize) -> [f32; 3] {
    [f32_at(b, o), f32_at(b, o + 4), f32_at(b, o + 8)]
}

impl Bsp {
    /// Parse a map. `read_external` is asked for `<name>.bsp.XXXX.bsp_lump` files for lumps
    /// that are not stored inline.
    /// Parse a BSP; lumps stored outside it (`.bsp_lump` files) are read with `read_external`,
    /// in parallel.
    pub fn parse(name: &str, data: &[u8], read_external: impl Fn(usize) -> Option<Vec<u8>> + Sync) -> Result<Self> {
        if data.len() < 16 + NUM_LUMPS * 16 || &data[0..4] != b"rBSP" {
            bail!("{name} is not a Respawn BSP");
        }
        let version = u32_at(data, 4);
        if version != 37 {
            bail!("{name}: BSP version {version} (expected 37, Titanfall 2)");
        }
        let mut headers = [LumpHeader::default(); NUM_LUMPS];
        for (i, h) in headers.iter_mut().enumerate() {
            let o = 16 + i * 16;
            *h = LumpHeader { offset: u32_at(data, o), length: u32_at(data, o + 4), version: u32_at(data, o + 8) };
        }
        let lumps: Vec<Vec<u8>> = std::thread::scope(|scope| {
            let jobs: Vec<_> = headers
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    let (s, e) = (h.offset as usize, h.offset as usize + h.length as usize);
                    let inline = h.length > 0 && e <= data.len();
                    let read = &read_external;
                    scope.spawn(move || if inline { data[s..e].to_vec() } else { read(i).unwrap_or_default() })
                })
                .collect();
            jobs.into_iter().map(|j| j.join().unwrap_or_default()).collect()
        });
        Ok(Self { name: name.to_string(), version, headers, lumps })
    }

    pub fn lump(&self, i: usize) -> &[u8] {
        &self.lumps[i]
    }

    pub fn entities(&self) -> String {
        String::from_utf8_lossy(self.lump(lump::ENTITIES)).to_string()
    }

    pub fn meshes(&self) -> Vec<Mesh> {
        let b = self.lump(lump::MESHES);
        (0..b.len() / 28)
            .map(|i| {
                let o = i * 28;
                Mesh {
                    first_index: u32_at(b, o),
                    num_triangles: u16_at(b, o + 4) as u32,
                    material_sort: u16_at(b, o + 22),
                    flags: u32_at(b, o + 24),
                }
            })
            .collect()
    }

    pub fn material_sorts(&self) -> Vec<MaterialSort> {
        let b = self.lump(lump::MATERIAL_SORTS);
        (0..b.len() / 12)
            .map(|i| {
                let o = i * 12;
                MaterialSort { texture_data: i16_at(b, o), lightmap: i16_at(b, o + 2), cubemap: i16_at(b, o + 4), vertex_offset: i32_at(b, o + 8) }
            })
            .collect()
    }

    pub fn texture_data(&self) -> Vec<TextureData> {
        let b = self.lump(lump::TEXTURE_DATA);
        let table = self.lump(lump::TEXTURE_DATA_STRING_TABLE);
        let strings = self.lump(lump::TEXTURE_DATA_STRING_DATA);
        (0..b.len() / 36)
            .map(|i| {
                let o = i * 36;
                let name_index = i32_at(b, o + 12) as usize;
                let so = if name_index * 4 + 4 <= table.len() { i32_at(table, name_index * 4) as usize } else { usize::MAX };
                let name = if so < strings.len() {
                    let end = strings[so..].iter().position(|&c| c == 0).unwrap_or(0);
                    String::from_utf8_lossy(&strings[so..so + end]).to_string()
                } else {
                    String::new()
                };
                TextureData {
                    name,
                    size: [i32_at(b, o + 16), i32_at(b, o + 20)],
                    flags: u32_at(b, o + 32),
                }
            })
            .collect()
    }

    pub fn models(&self) -> Vec<BrushModel> {
        let b = self.lump(lump::MODELS);
        (0..b.len() / 32)
            .map(|i| {
                let o = i * 32;
                BrushModel {
                    mins: vec3_at(b, o),
                    maxs: vec3_at(b, o + 12),
                    first_mesh: u32_at(b, o + 24),
                    num_meshes: u32_at(b, o + 28),
                }
            })
            .collect()
    }

    pub fn mesh_indices(&self) -> Vec<u16> {
        self.lump(lump::MESH_INDICES).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
    }

    /// Resolve vertex `index` of the vertex lump selected by mesh `flags`.
    pub fn vertex(&self, flags: u32, index: usize) -> Option<Vertex> {
        let (lump_id, stride) = match flags & mesh_flags::VERTEX_MASK {
            mesh_flags::VERTEX_LIT_FLAT => (lump::VERTEX_LIT_FLAT, 36),
            mesh_flags::VERTEX_LIT_BUMP => (lump::VERTEX_LIT_BUMP, 44),
            mesh_flags::VERTEX_UNLIT => (lump::VERTEX_UNLIT, 20),
            _ => (lump::VERTEX_UNLIT_TS, 28),
        };
        let b = self.lump(lump_id);
        let o = index * stride;
        if o + stride > b.len() {
            return None;
        }
        let pi = u32_at(b, o) as usize;
        let ni = u32_at(b, o + 4) as usize;
        let verts = self.lump(lump::VERTICES);
        let norms = self.lump(lump::VERTEX_NORMALS);
        Some(Vertex {
            pos: if pi * 12 + 12 <= verts.len() { vec3_at(verts, pi * 12) } else { [0.0; 3] },
            normal: if ni * 12 + 12 <= norms.len() { vec3_at(norms, ni * 12) } else { [0.0, 0.0, 1.0] },
            uv: [f32_at(b, o + 8), f32_at(b, o + 12)],
            color: b[o + 16..o + 20].try_into().unwrap(),
            lightmap_uv: matches!(flags & mesh_flags::VERTEX_MASK, mesh_flags::VERTEX_LIT_FLAT | mesh_flags::VERTEX_LIT_BUMP)
                .then(|| [f32_at(b, o + 20), f32_at(b, o + 24)]),
        })
    }

    /// Lightmap pages (LIGHTMAP_HEADERS + LIGHTMAP_DATA_SKY; per page: sky A then sky B).
    pub fn lightmaps(&self) -> Vec<LightmapPage> {
        let h = self.lump(lump::LIGHTMAP_HEADERS);
        let sky = self.lump(lump::LIGHTMAP_DATA_SKY);
        let mut out = Vec::new();
        let mut o = 0usize;
        for c in h.chunks_exact(8) {
            let (w, hh) = (u16_at(c, 4) as usize, u16_at(c, 6) as usize);
            let n = w * hh * 4;
            if o + 2 * n > sky.len() {
                break;
            }
            out.push(LightmapPage { width: w as u32, height: hh as u32, sky_a: sky[o..o + n].to_vec(), sky_b: sky[o + n..o + 2 * n].to_vec() });
            o += 2 * n;
        }
        out
    }

    /// Baked light probes (LIGHTPROBES, 48 bytes each): per colour channel an L1 spherical
    /// harmonic `[x, y, z, dc]` (int16). Irradiance towards normal n is `dc - (x, y, z)·n`
    /// (the sign checked against each probe's sky direction).
    pub fn light_probes(&self) -> Vec<[[i16; 4]; 3]> {
        self.lump(lump::LIGHTPROBES)
            .chunks_exact(48)
            .map(|c| std::array::from_fn(|ch| std::array::from_fn(|k| i16::from_le_bytes([c[ch * 8 + k * 2], c[ch * 8 + k * 2 + 1]]))))
            .collect()
    }

    /// Where the probes sit (LIGHTPROBE_REFERENCES, 20 bytes: origin, probe index, unknown).
    pub fn light_probe_refs(&self) -> Vec<([f32; 3], u32)> {
        self.lump(lump::LIGHTPROBE_REFERENCES)
            .chunks_exact(20)
            .map(|c| {
                let f = |o: usize| f32::from_le_bytes([c[o], c[o + 1], c[o + 2], c[o + 3]]);
                ([f(0), f(4), f(8)], u32::from_le_bytes([c[12], c[13], c[14], c[15]]))
            })
            .collect()
    }

    /// Cubemap sample origins (CUBEMAPS, 16 bytes: int origin, unknown), in the order of the
    /// frames of `materials/maps/<map>/cubemaps.hdr.vtf`.
    pub fn cubemap_origins(&self) -> Vec<[f32; 3]> {
        self.lump(lump::CUBEMAPS)
            .chunks_exact(16)
            .map(|c| std::array::from_fn(|k| i32::from_le_bytes([c[k * 4], c[k * 4 + 1], c[k * 4 + 2], c[k * 4 + 3]]) as f32))
            .collect()
    }

    /// A stored (uncompressed) file from the map's PAKFILE zip.
    pub fn pakfile_entry(&self, name_suffix: &str) -> Option<&[u8]> {
        let z = self.lump(lump::PAKFILE);
        let mut o = 0usize;
        while o + 30 <= z.len() && z[o..o + 4] == [0x50, 0x4B, 0x03, 0x04] {
            let u16_ = |p: usize| u16::from_le_bytes([z[p], z[p + 1]]) as usize;
            let method = u16_(o + 8);
            let size = u32::from_le_bytes([z[o + 18], z[o + 19], z[o + 20], z[o + 21]]) as usize;
            let (n, e) = (u16_(o + 26), u16_(o + 28));
            let name = String::from_utf8_lossy(&z[o + 30..(o + 30 + n).min(z.len())]).to_ascii_lowercase();
            let data = o + 30 + n + e;
            if method == 0 && name.ends_with(name_suffix) {
                return z.get(data..data + size);
            }
            o = data + size;
        }
        None
    }

    /// Static props from the `sprp` game lump (version 13).
    /// The static prop records as raw 64-byte slices (for inspecting unparsed fields).
    pub fn static_prop_records(&self) -> Result<Vec<Vec<u8>>> {
        let Some(d) = self.sprp_data()? else { return Ok(Vec::new()) };
        let n_names = u32_at(d, 0) as usize;
        let p = 4 + n_names * 128;
        let n_props = u32_at(d, p) as usize;
        let p = p + 12;
        Ok((0..n_props).map(|i| d[p + i * 64..p + i * 64 + 64].to_vec()).collect())
    }

    /// The `sprp` game lump's data, if the map has one.
    fn sprp_data(&self) -> Result<Option<&[u8]>> {
        let g = self.lump(lump::GAME_LUMP);
        if g.len() < 4 {
            return Ok(None);
        }
        let count = i32_at(g, 0) as usize;
        for i in 0..count {
            let o = 4 + i * 16;
            let id = &g[o..o + 4];
            let version = u16_at(g, o + 6);
            let fileofs = i32_at(g, o + 8) as usize;
            let filelen = i32_at(g, o + 12) as usize;
            if id != b"prps" && id != b"sprp" {
                continue;
            }
            if version != 13 {
                bail!("static prop lump version {version} unsupported");
            }
            let base = self.headers[lump::GAME_LUMP].offset as usize;
            let s = fileofs.checked_sub(base).context("game lump offset before lump start")?;
            return Ok(Some(g.get(s..s + filelen).context("static prop lump out of range")?));
        }
        Ok(None)
    }

    pub fn static_props(&self) -> Result<StaticProps> {
        let g = self.lump(lump::GAME_LUMP);
        if g.len() < 4 {
            return Ok(StaticProps::default());
        }
        let count = i32_at(g, 0) as usize;
        for i in 0..count {
            let o = 4 + i * 16;
            let id = &g[o..o + 4];
            let version = u16_at(g, o + 6);
            let fileofs = i32_at(g, o + 8) as usize;
            let filelen = i32_at(g, o + 12) as usize;
            if id != b"prps" && id != b"sprp" {
                continue;
            }
            if version != 13 {
                bail!("static prop lump version {version} unsupported");
            }
            // Game lump offsets are relative to the start of the .bsp file.
            let base = self.headers[lump::GAME_LUMP].offset as usize;
            let s = fileofs.checked_sub(base).context("game lump offset before lump start")?;
            let d = g.get(s..s + filelen).context("static prop lump out of range")?;
            let n_names = u32_at(d, 0) as usize;
            let mut p = 4;
            let mut model_names = Vec::with_capacity(n_names);
            for _ in 0..n_names {
                let raw = &d[p..p + 128];
                let end = raw.iter().position(|&c| c == 0).unwrap_or(128);
                model_names.push(String::from_utf8_lossy(&raw[..end]).to_string());
                p += 128;
            }
            let n_props = u32_at(d, p) as usize;
            p += 12;
            let mut props = Vec::with_capacity(n_props);
            for _ in 0..n_props {
                props.push(StaticProp {
                    origin: vec3_at(d, p),
                    angles: vec3_at(d, p + 12),
                    scale: f32_at(d, p + 24),
                    model: u16_at(d, p + 28),
                    solid: d[p + 30],
                    flags: d[p + 31],
                    skin: u16_at(d, p + 32),
                });
                p += 64;
            }
            return Ok(StaticProps { model_names, props });
        }
        Ok(StaticProps::default())
    }
}

/// Parse the entity lump text into a list of key/value maps.
pub fn parse_entities(text: &str) -> Vec<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut cur: Option<Vec<(String, String)>> = None;
    let mut chars = text.chars().peekable();
    let mut pending_key: Option<String> = None;
    while let Some(c) = chars.next() {
        match c {
            '{' => cur = Some(Vec::new()),
            '}' => {
                if let Some(e) = cur.take() {
                    out.push(e);
                }
            }
            '"' => {
                let mut s = String::new();
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                    s.push(c);
                }
                match pending_key.take() {
                    None => pending_key = Some(s),
                    Some(k) => {
                        if let Some(e) = cur.as_mut() {
                            e.push((k, s));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}
