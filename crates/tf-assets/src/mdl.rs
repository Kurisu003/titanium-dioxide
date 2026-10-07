//! Titanfall 2 studio models (`IDST` version 53). Geometry (VTX + VVD) is embedded in the
//! .mdl file; animations are RLE-compressed bone tracks, often stored in "include" models.
//!
//! Struct layouts follow r-ex/rsx (studio_r2.h, optimize.h). All values here are in the
//! game's coordinate system: Z up, inches.

use anyhow::{bail, Result};
use std::collections::HashMap;

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn i32(&self, o: usize) -> i32 {
        i32::from_le_bytes(self.0[o..o + 4].try_into().unwrap())
    }
    fn u16(&self, o: usize) -> u16 {
        u16::from_le_bytes(self.0[o..o + 2].try_into().unwrap())
    }
    fn i16(&self, o: usize) -> i16 {
        self.u16(o) as i16
    }
    fn f32(&self, o: usize) -> f32 {
        f32::from_le_bytes(self.0[o..o + 4].try_into().unwrap())
    }
    fn v3(&self, o: usize) -> [f32; 3] {
        [self.f32(o), self.f32(o + 4), self.f32(o + 8)]
    }
    fn v4(&self, o: usize) -> [f32; 4] {
        [self.f32(o), self.f32(o + 4), self.f32(o + 8), self.f32(o + 12)]
    }
    fn cstr(&self, o: usize) -> String {
        if o >= self.0.len() {
            return String::new();
        }
        let end = self.0[o..].iter().position(|&c| c == 0).unwrap_or(0);
        String::from_utf8_lossy(&self.0[o..o + end]).to_string()
    }
}

#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    pub parent: i32,
    pub pos: [f32; 3],
    /// x, y, z, w
    pub quat: [f32; 4],
    pub rot: [f32; 3],
    pub rot_scale: [f32; 3],
    /// Rest scale, and the quantisation scale of animated scale tracks.
    pub scale: [f32; 3],
    pub scale_scale: [f32; 3],
    /// Row-major 3x4 matrix taking model-space positions into bone space (inverse bind).
    pub pose_to_bone: [[f32; 4]; 3],
    pub q_alignment: [f32; 4],
    pub flags: i32,
}

/// One drawable piece of geometry: one VTX mesh at LOD 0 of the selected body groups.
#[derive(Debug, Clone, Default)]
pub struct MeshData {
    /// Index into the skin reference table (map through [`Model::skin_families`]).
    pub material: usize,
    pub bodypart: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct BodyPart {
    pub name: String,
    pub num_models: usize,
}

/// A decoded animation: per-frame local transforms for each bone of the model that owns it.
#[derive(Debug, Clone)]
pub struct Animation {
    pub name: String,
    pub fps: f32,
    pub flags: i32,
    pub num_frames: usize,
    /// frames[frame][bone] = (position, quaternion xyzw), bones of the owning model.
    pub frames: Vec<Vec<([f32; 3], [f32; 4])>>,
    /// Bones that have a track in at least one frame (others hold the rest pose).
    pub animated: Vec<bool>,
    /// Root motion per frame (x, y, z, yaw degrees), from the frame-movement block.
    pub movement: Vec<[f32; 4]>,
}

impl Animation {
    /// Average ground speed of the root motion, in units per second.
    pub fn ground_speed(&self) -> f32 {
        match (self.movement.first(), self.movement.last()) {
            (Some(a), Some(b)) if self.num_frames > 1 => {
                let d = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                d * self.fps / (self.num_frames - 1) as f32
            }
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Sequence {
    pub label: String,
    pub activity: String,
    pub flags: i32,
    /// Indices into the owning model's animation descriptors (blend grid, row-major).
    pub anims: Vec<usize>,
    pub group_size: [i32; 2],
    /// Pose parameter index driving each grid axis (-1 = none) and the parameter range the
    /// axis spans (`paramindex`, `paramstart`, `paramend`).
    pub param_index: [i32; 2],
    pub param_start: [f32; 2],
    pub param_end: [f32; 2],
    /// Animation events (sounds, effects) in cycle order.
    pub events: Vec<Event>,
    /// Per-bone blend weights (`weightlistindex`): 0 leaves a bone out of the sequence.
    pub bone_weights: Vec<f32>,
}

/// mstudioevent_t (v53, 0x50 bytes: cycle, event, type, options[64], name index): fires `name` with `options` when the cycle passes `cycle`.
#[derive(Debug, Clone)]
pub struct Event {
    pub cycle: f32,
    pub event: i32,
    pub name: String,
    pub options: String,
}

pub struct Model {
    pub name: String,
    pub raw: Vec<u8>,
    pub bones: Vec<Bone>,
    pub textures: Vec<String>,
    pub cd_textures: Vec<String>,
    /// skin_families[family][skinref] -> texture index
    pub skin_families: Vec<Vec<u16>>,
    pub bodyparts: Vec<BodyPart>,
    pub include_models: Vec<String>,
    pub sequences: Vec<Sequence>,
    pub num_anims: usize,
    pub surface_prop: String,
    pub hull: ([f32; 3], [f32; 3]),
    /// The default hitbox set (set 0).
    pub hitboxes: Vec<Hitbox>,
    /// Local pose parameters (mstudioposeparamdesc_t: name, flags, start, end, loop).
    pub pose_params: Vec<PoseParam>,
    /// Attachments (mstudioattachment_t, 0x5C bytes: name, flags, bone, 3x4 local matrix).
    pub attachments: Vec<Attachment>,
    /// RUI meshes: the screens weapon scripts draw their `UiData` RUIs on.
    pub rui_meshes: Vec<RuiMesh>,
}

/// An RUI mesh (v53 header 0x128/0x12C: a table of {name hash, offset from the entry}).
/// Each mesh is {numparents, numvertices, numfaces, parentindex, vertexindex, vertmapindex,
/// facedataindex, ?} (ints, offsets from the mesh) then its name; parents are bone indices
/// (shorts), vertices {parent slot, position in that bone's space} (16 bytes), and each face
/// three corner vertices (shorts) of a parallelogram, the fourth implied, with face data
/// {u of the four corners, v of the four corners} (32 bytes). Corner order for the UVs:
/// the three listed corners, then the implied one (c0 + c2 - c1).
#[derive(Debug, Clone)]
pub struct RuiMesh {
    pub name: String,
    pub faces: Vec<RuiFace>,
}

#[derive(Debug, Clone)]
pub struct RuiFace {
    /// Bone of the first corner.
    pub bone: usize,
    /// The four corners in that bone's space.
    pub corners: [[f32; 3]; 4],
    pub uvs: [[f32; 2]; 4],
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub name: String,
    pub bone: usize,
    /// Row-major 3x4 matrix in the bone's space (translation in column 3).
    pub local: [[f32; 4]; 3],
}

#[derive(Debug, Clone)]
pub struct PoseParam {
    pub name: String,
    pub start: f32,
    pub end: f32,
    pub looping: f32,
}

/// mstudiobbox_t (v53, 0x44 bytes): a box in a bone's space with its hit group (Source
/// HITGROUP_*: 1 head, 2 chest, 3 stomach, 4/5 arms, 6/7 legs) and Respawn's crit flag
/// (`critShotOverride` at 0x24; set on Titans' cockpit hatch boxes).
#[derive(Debug, Clone, Copy)]
pub struct Hitbox {
    pub bone: usize,
    pub group: i32,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub crit: bool,
}

const HDR_SIZE: usize = 0x2CC;
const BONE_SIZE: usize = 0xF4;
const ANIMDESC_SIZE: usize = 0x5C;
pub const SEQDESC_SIZE: usize = 0xE8;
const VERTEX_SIZE: usize = 48;

impl Model {
    fn rui_meshes(r: &R, raw_len: usize) -> Vec<RuiMesh> {
        let (n, table) = (r.i32(0x128).clamp(0, 64) as usize, r.i32(0x12C).max(0) as usize);
        let mut out = Vec::new();
        for k in 0..n {
            let e = table + k * 8;
            if e + 8 > raw_len {
                break;
            }
            let m = (e as i64 + r.i32(e + 4) as i64) as usize;
            if m + 32 > raw_len {
                continue;
            }
            let field = |i: usize| r.i32(m + i * 4).max(0) as usize;
            let (np, nv, nf, pi, vi, mi, fi) = (field(0).min(64), field(1).min(4096), field(2).min(4096), field(3), field(4), field(5), field(6));
            if m + pi + np * 2 > raw_len || m + vi + nv * 16 > raw_len || m + mi + nf * 6 > raw_len || m + fi + nf * 32 > raw_len {
                continue;
            }
            let parents: Vec<usize> = (0..np).map(|p| r.i16(m + pi + p * 2).max(0) as usize).collect();
            let vert = |v: usize| {
                let o = m + vi + v.min(nv.saturating_sub(1)) * 16;
                (r.i32(o).max(0) as usize, r.v3(o + 4))
            };
            let faces = (0..nf)
                .map(|f| {
                    let c = |i: usize| vert(r.u16(m + mi + f * 6 + i * 2) as usize);
                    let ((p0, a), (_, b), (_, c2)) = (c(0), c(1), c(2));
                    let d = [a[0] + c2[0] - b[0], a[1] + c2[1] - b[1], a[2] + c2[2] - b[2]];
                    let o = m + fi + f * 32;
                    let uv = |i: usize| [r.f32(o + i * 4), r.f32(o + 16 + i * 4)];
                    RuiFace { bone: parents.get(p0).copied().unwrap_or(0), corners: [a, b, c2, d], uvs: [uv(0), uv(1), uv(2), uv(3)] }
                })
                .collect();
            out.push(RuiMesh { name: r.cstr(m + 32), faces });
        }
        out
    }

    pub fn parse(raw: Vec<u8>) -> Result<Self> {
        if raw.len() < HDR_SIZE || &raw[0..4] != b"IDST" {
            bail!("not a studio model");
        }
        let r = R(&raw);
        let raw_len = raw.len();
        let version = r.i32(4);
        if version != 53 {
            bail!("mdl version {version} unsupported (Titanfall 2 uses 53)");
        }
        let name = r.cstr(r.i32(0xC) as usize);

        let (num_bones, bone_index) = (r.i32(0xA0) as usize, r.i32(0xA4) as usize);
        let mut bones = Vec::with_capacity(num_bones);
        for i in 0..num_bones {
            let o = bone_index + i * BONE_SIZE;
            let m = |row: usize| r.v4(o + 0x78 + row * 16);
            bones.push(Bone {
                name: r.cstr(o + r.i32(o) as usize),
                parent: r.i32(o + 4),
                pos: r.v3(o + 0x20),
                quat: r.v4(o + 0x2C),
                rot: r.v3(o + 0x3C),
                rot_scale: r.v3(o + 0x60),
                scale: r.v3(o + 0x48),
                scale_scale: r.v3(o + 0x6C),
                pose_to_bone: [m(0), m(1), m(2)],
                q_alignment: r.v4(o + 0xA8),
                flags: r.i32(o + 0xB8),
            });
        }

        let (num_tex, tex_index) = (r.i32(0xD0) as usize, r.i32(0xD4) as usize);
        let textures = (0..num_tex)
            .map(|i| {
                let o = tex_index + i * 0x2C;
                r.cstr(o + r.i32(o) as usize)
            })
            .collect();
        let (num_cd, cd_index) = (r.i32(0xD8) as usize, r.i32(0xDC) as usize);
        let cd_textures = (0..num_cd).map(|i| r.cstr(r.i32(cd_index + i * 4) as usize)).collect();

        let (num_skinref, num_families, skin_index) = (r.i32(0xE0) as usize, r.i32(0xE4) as usize, r.i32(0xE8) as usize);
        let skin_families = (0..num_families)
            .map(|f| (0..num_skinref).map(|s| r.u16(skin_index + (f * num_skinref + s) * 2)).collect())
            .collect();

        let (num_bp, bp_index) = (r.i32(0xEC) as usize, r.i32(0xF0) as usize);
        let bodyparts = (0..num_bp)
            .map(|i| {
                let o = bp_index + i * 16;
                BodyPart { name: r.cstr(o + r.i32(o) as usize), num_models: r.i32(o + 4) as usize }
            })
            .collect();

        let (num_inc, inc_index) = (r.i32(0x154) as usize, r.i32(0x158) as usize);
        let include_models = (0..num_inc)
            .map(|i| {
                let o = inc_index + i * 8;
                r.cstr(o + r.i32(o + 4) as usize)
            })
            .collect();

        let (num_seq, seq_index) = (r.i32(0xC0) as usize, r.i32(0xC4) as usize);
        let mut sequences = Vec::with_capacity(num_seq);
        for i in 0..num_seq {
            let o = seq_index + i * SEQDESC_SIZE;
            let group_size = [r.i32(o + 0x44), r.i32(o + 0x48)];
            let n = (group_size[0] * group_size[1]).max(0) as usize;
            let ai = r.i32(o + 0x3C) as usize;
            sequences.push(Sequence {
                label: r.cstr(o + r.i32(o + 4) as usize),
                activity: r.cstr(o + r.i32(o + 8) as usize),
                flags: r.i32(o + 0xC),
                anims: (0..n).map(|k| r.i16(o + ai + k * 2) as usize).collect(),
                group_size,
                param_index: [r.i32(o + 0x4C), r.i32(o + 0x50)],
                param_start: [r.f32(o + 0x54), r.f32(o + 0x58)],
                param_end: [r.f32(o + 0x5C), r.f32(o + 0x60)],
                bone_weights: {
                    let wi = r.i32(o + 0x9C) as usize;
                    (0..num_bones).map(|b| if wi == 0 { 1.0 } else { r.f32(o + wi + b * 4) }).collect()
                },
                events: {
                    let (ne, ei) = (r.i32(o + 0x18).clamp(0, 256) as usize, r.i32(o + 0x1C) as usize);
                    (0..ne)
                        .map(|k| {
                            let e = o + ei + k * 0x50;
                            Event {
                                cycle: r.f32(e),
                                event: r.i32(e + 4),
                                options: r.cstr(e + 12),
                                name: r.cstr(e + r.i32(e + 0x4C) as usize),
                            }
                        })
                        .collect()
                },
            });
        }

        let hitboxes = {
            let (num_sets, set_index) = (r.i32(0xB0).max(0) as usize, r.i32(0xB4) as usize);
            if num_sets == 0 || set_index + 12 > raw_len {
                Vec::new()
            } else {
                let (n, hi) = (r.i32(set_index + 4).clamp(0, 512) as usize, r.i32(set_index + 8) as usize);
                (0..n)
                    .filter_map(|k| {
                        let b = set_index + hi + k * 0x44;
                        (b + 0x44 <= raw_len).then(|| Hitbox { bone: r.i32(b).max(0) as usize, group: r.i32(b + 4), min: r.v3(b + 8), max: r.v3(b + 20), crit: r.i32(b + 0x24) != 0 })
                    })
                    .collect()
            }
        };
        let surface_prop = r.cstr(r.i32(0x138) as usize);
        let hull = (r.v3(0x6C), r.v3(0x78));
        let num_anims = r.i32(0xB8) as usize;
        Ok(Self {
            name,
            bones,
            textures,
            cd_textures,
            skin_families,
            bodyparts,
            include_models,
            sequences,
            num_anims,
            surface_prop,
            hull,
            hitboxes,
            attachments: {
                let (na, ai) = (r.i32(0xF4).clamp(0, 512) as usize, r.i32(0xF8) as usize);
                (0..na)
                    .filter(|k| ai + k * 0x5C + 0x5C <= raw_len)
                    .map(|k| {
                        let o = ai + k * 0x5C;
                        let m = |i: usize| r.f32(o + 12 + i * 4);
                        Attachment {
                            name: r.cstr(o + r.i32(o) as usize),
                            bone: r.i32(o + 8) as usize,
                            local: [[m(0), m(1), m(2), m(3)], [m(4), m(5), m(6), m(7)], [m(8), m(9), m(10), m(11)]],
                        }
                    })
                    .collect()
            },
            rui_meshes: Self::rui_meshes(&r, raw_len),
            pose_params: {
                let (np, pi) = (r.i32(0x130).clamp(0, 64) as usize, r.i32(0x134) as usize);
                (0..np)
                    .map(|k| {
                        let o = pi + k * 20;
                        PoseParam { name: r.cstr(o + r.i32(o) as usize), start: r.f32(o + 8), end: r.f32(o + 12), looping: r.f32(o + 16) }
                    })
                    .collect()
            },
            raw,
        })
    }

    pub fn bone_index(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name.eq_ignore_ascii_case(name))
    }

    /// Build LOD 0 geometry. `body` selects the model index within each body part
    /// (missing entries default to 0).
    pub fn meshes(&self, body: &[usize]) -> Result<Vec<MeshData>> {
        self.meshes_lod(body, 0)
    }

    /// The model's LOD switch points (VTX `switchPoint`, first body part's first model), one
    /// per LOD; empty if the model has no VTX.
    pub fn lod_switch_points(&self) -> Vec<f32> {
        let r = R(&self.raw);
        let vtx_off = r.i32(0x1AC) as usize;
        if r.i32(0x1BC) == 0 || self.bodyparts.is_empty() || r.i32(vtx_off + 28) == 0 {
            return Vec::new();
        }
        let vtx_bp_off = vtx_off + r.i32(vtx_off + 32) as usize;
        let vmodel = vtx_bp_off + r.i32(vtx_bp_off + 4) as usize;
        let n = r.i32(vmodel).clamp(0, 8) as usize;
        let vlod0 = vmodel + r.i32(vmodel + 4) as usize;
        (0..n).map(|k| r.f32(vlod0 + k * 12 + 8)).collect()
    }

    /// Geometry of one LOD (0 = full detail). LODs past the model's last one use its last.
    pub fn meshes_lod(&self, body: &[usize], lod: usize) -> Result<Vec<MeshData>> {
        let r = R(&self.raw);
        let vtx_off = r.i32(0x1AC) as usize;
        let vvd_off = r.i32(0x1B0) as usize;
        let (vtx_size, vvd_size) = (r.i32(0x1BC) as usize, r.i32(0x1C0) as usize);
        if vtx_size == 0 || vvd_size == 0 {
            return Ok(Vec::new());
        }
        let lod = lod.min(self.lod_switch_points().len().saturating_sub(1));
        let vertices = self.lod_vertices(vvd_off, lod)?;

        let bp_index = r.i32(0xF0) as usize;
        let vtx_bp_count = r.i32(vtx_off + 28) as usize;
        let vtx_bp_off = vtx_off + r.i32(vtx_off + 32) as usize;
        // With VVD fixups, a lower LOD's vertex buffer holds only that LOD's vertices, so each
        // mesh starts at the running sum of the meshes' `numLODVertexes[lod]` (mesh + 0x34),
        // in body part / model / mesh order. Checked against the buffer size; on mismatch use
        // LOD 0.
        let fixups = r.i32(vvd_off + 48) > 0;
        let lod_starts: Option<HashMap<(usize, usize, usize), usize>> = if lod > 0 && fixups {
            let mut starts = HashMap::new();
            let mut acc = 0usize;
            let mut ok = true;
            for (bpi, bp) in self.bodyparts.iter().enumerate() {
                let mdl_bp = bp_index + bpi * 16;
                for mo in 0..bp.num_models {
                    let mdl_model = mdl_bp + r.i32(mdl_bp + 12) as usize + mo * 0x94;
                    let nm = r.i32(mdl_model + 0x48).max(0) as usize;
                    let m0 = mdl_model + r.i32(mdl_model + 0x4C) as usize;
                    for mi in 0..nm {
                        let mesh = m0 + mi * 0x74;
                        ok &= r.i32(mesh + 0x34) == r.i32(mesh + 8);
                        starts.insert((bpi, mo, mi), acc);
                        acc += r.i32(mesh + 0x34 + lod * 4).max(0) as usize;
                    }
                }
            }
            (ok && acc == vertices.len()).then_some(starts)
        } else {
            None
        };
        if lod > 0 && fixups && lod_starts.is_none() {
            return self.meshes_lod(body, 0);
        }
        let mut out = Vec::new();
        for (bpi, bp) in self.bodyparts.iter().enumerate() {
            if bpi >= vtx_bp_count {
                break;
            }
            let model_idx = body.get(bpi).copied().unwrap_or(0).min(bp.num_models.saturating_sub(1));
            let mdl_bp = bp_index + bpi * 16;
            let mdl_model = mdl_bp + r.i32(mdl_bp + 12) as usize + model_idx * 0x94;
            let model_vertex_base = r.i32(mdl_model + 0x54) as usize / VERTEX_SIZE;
            let num_meshes = r.i32(mdl_model + 0x48) as usize;
            let mesh_index = mdl_model + r.i32(mdl_model + 0x4C) as usize;

            let vbp = vtx_bp_off + bpi * 8;
            let vmodel = vbp + r.i32(vbp + 4) as usize + model_idx * 8;
            if r.i32(vmodel) == 0 {
                continue; // no LODs: empty body group choice
            }
            let num_lods = r.i32(vmodel) as usize;
            let vlod = vmodel + r.i32(vmodel + 4) as usize + lod.min(num_lods - 1) * 12;
            let vlod_meshes = r.i32(vlod) as usize;
            let vmesh_base = vlod + r.i32(vlod + 4) as usize;

            for mi in 0..num_meshes.min(vlod_meshes) {
                let mesh = mesh_index + mi * 0x74;
                let material = r.i32(mesh) as usize;
                let mesh_vertex_base = match &lod_starts {
                    Some(s) => s.get(&(bpi, model_idx, mi)).copied().unwrap_or(0),
                    None => model_vertex_base + r.i32(mesh + 0xC) as usize,
                };

                let vmesh = vmesh_base + mi * 9;
                let num_sg = r.i32(vmesh) as usize;
                let sg_base = vmesh + r.i32(vmesh + 4) as usize;
                let mut md = MeshData { material, bodypart: bpi, ..Default::default() };
                let mut remap: HashMap<usize, u32> = HashMap::new();
                for sgi in 0..num_sg {
                    let sg = sg_base + sgi * 33;
                    let sg_verts = sg + r.i32(sg + 4) as usize;
                    let sg_indices = sg + r.i32(sg + 12) as usize;
                    let num_strips = r.i32(sg + 16) as usize;
                    let strips = sg + r.i32(sg + 20) as usize;
                    for si in 0..num_strips {
                        let st = strips + si * 35;
                        let n_idx = r.i32(st) as usize;
                        let idx_off = r.i32(st + 4) as usize;
                        let flags = self.raw[st + 18];
                        if flags & 1 == 0 {
                            continue; // only triangle lists exist in practice
                        }
                        for k in 0..n_idx {
                            let vi = r.u16(sg_indices + (idx_off + k) * 2) as usize;
                            let vtx_vert = sg_verts + vi * 9;
                            let orig = r.u16(vtx_vert + 4) as usize;
                            let global = mesh_vertex_base + orig;
                            let next = md.positions.len() as u32;
                            let id = *remap.entry(global).or_insert_with(|| {
                                let v = &vertices[global.min(vertices.len() - 1)];
                                md.positions.push(v.pos);
                                md.normals.push(v.normal);
                                md.uvs.push(v.uv);
                                md.joints.push(v.joints);
                                md.weights.push(v.weights);
                                next
                            });
                            md.indices.push(id);
                        }
                    }
                }
                if !md.indices.is_empty() {
                    out.push(md);
                }
            }
        }
        Ok(out)
    }

    /// The VVD vertices one LOD uses: with fixups, the blocks whose LOD is at least `lod`.
    fn lod_vertices(&self, vvd: usize, lod: usize) -> Result<Vec<Vert>> {
        let r = R(&self.raw);
        if &self.raw[vvd..vvd + 4] != b"IDSV" {
            bail!("missing embedded VVD");
        }
        let num_lod0 = r.i32(vvd + 16) as usize;
        let num_fixups = r.i32(vvd + 48) as usize;
        let fixup_start = vvd + r.i32(vvd + 52) as usize;
        let vertex_start = vvd + r.i32(vvd + 56) as usize;
        let read = |i: usize| {
            let o = vertex_start + i * VERTEX_SIZE;
            let n = (self.raw[o + 15] as usize).clamp(1, 3);
            let mut joints = [0u16; 4];
            let mut weights = [0f32; 4];
            for k in 0..n {
                joints[k] = self.raw[o + 12 + k] as u16;
                weights[k] = r.f32(o + k * 4);
            }
            let sum: f32 = weights.iter().sum();
            if sum > 0.0 {
                weights.iter_mut().for_each(|w| *w /= sum);
            } else {
                weights = [1.0, 0.0, 0.0, 0.0];
            }
            Vert { pos: r.v3(o + 16), normal: r.v3(o + 28), uv: [r.f32(o + 40), r.f32(o + 44)], joints, weights }
        };
        let mut out = Vec::with_capacity(num_lod0);
        if num_fixups == 0 {
            for i in 0..num_lod0 {
                out.push(read(i));
            }
        } else {
            for f in 0..num_fixups {
                let o = fixup_start + f * 12;
                // A fixup block is used by every LOD up to and including its own.
                if (r.i32(o) as usize) < lod {
                    continue;
                }
                let (src, n) = (r.i32(o + 4) as usize, r.i32(o + 8) as usize);
                for i in src..src + n {
                    out.push(read(i));
                }
            }
        }
        if out.is_empty() {
            bail!("model has no vertices");
        }
        Ok(out)
    }

    /// Decode animation descriptor `index` of this model at every integer frame.
    pub fn animation(&self, index: usize) -> Result<Animation> {
        let r = R(&self.raw);
        let num = r.i32(0xB8) as usize;
        if index >= num {
            bail!("animation index out of range");
        }
        let ad = r.i32(0xBC) as usize + index * ANIMDESC_SIZE;
        let name = r.cstr(ad + r.i32(ad + 4) as usize);
        let fps = r.f32(ad + 8);
        let flags = r.i32(ad + 0xC);
        let num_frames = r.i32(ad + 0x10).max(1) as usize;
        let anim_index = r.i32(ad + 0x20) as usize;
        let section_index = r.i32(ad + 0x34) as usize;
        let section_frames = r.i32(ad + 0x38) as usize;
        let delta = flags & 0x4 != 0;

        let framemovement = r.i32(ad + 0x1C) as usize;
        let mut movement = Vec::new();
        if framemovement != 0 && flags & 0x40000 != 0 {
            let fm = ad + framemovement;
            for frame in 0..num_frames {
                let mut v = [0f32; 4];
                for (k, vk) in v.iter_mut().enumerate() {
                    let off = r.i16(fm + 16 + k * 2);
                    if off > 0 {
                        *vk = self.anim_track(fm + off as usize, frame, r.f32(fm + k * 4));
                    }
                }
                movement.push(v);
            }
        }
        let mut frames = Vec::with_capacity(num_frames);
        let mut animated = vec![false; self.bones.len()];
        for frame in 0..num_frames {
            // Locate the RLE block for this frame (pAnimdataNoStall).
            let mut local = frame;
            let mut data = anim_index;
            if section_frames != 0 {
                let section = if num_frames > section_frames && frame == num_frames - 1 {
                    local = 0;
                    (num_frames - 1) / section_frames + 1
                } else {
                    let s = frame / section_frames;
                    local = frame - s * section_frames;
                    s
                };
                data = r.i32(ad + section_index + section * 4) as usize;
            }
            let mut pose: Vec<([f32; 3], [f32; 4])> = self
                .bones
                .iter()
                .map(|b| if delta { ([0.0; 3], [0.0, 0.0, 0.0, 1.0]) } else { (b.pos, b.quat) })
                .collect();
            if data != 0 {
                let mut p = ad + data;
                loop {
                    if p + 32 > self.raw.len() {
                        break;
                    }
                    let bone = self.raw[p + 4] as usize;
                    let bflags = self.raw[p + 5];
                    if bone < self.bones.len() {
                        pose[bone] = self.decode_bone(p, bone, bflags, local, delta);
                        animated[bone] = true;
                    }
                    let next = r.i32(p + 28);
                    if next == 0 {
                        break;
                    }
                    p = (p as i64 + next as i64) as usize;
                }
            }
            frames.push(pose);
        }
        Ok(Animation { name, fps, flags, num_frames, frames, animated, movement })
    }

    fn decode_bone(&self, p: usize, bone: usize, flags: u8, frame: usize, delta: bool) -> ([f32; 3], [f32; 4]) {
        const RAWPOS: u8 = 0x02;
        const RAWROT: u8 = 0x04;
        const NOROT: u8 = 0x10;
        let r = R(&self.raw);
        let b = &self.bones[bone];
        let posscale = r.f32(p);

        let q = if flags & RAWROT != 0 {
            quat64(u64::from_le_bytes(self.raw[p + 8..p + 16].try_into().unwrap()))
        } else if flags & NOROT != 0 {
            if delta { [0.0, 0.0, 0.0, 1.0] } else { b.quat }
        } else {
            let mut e = [0f32; 3];
            for (k, ek) in e.iter_mut().enumerate() {
                *ek = self.anim_value(p + 8, k, frame, b.rot_scale[k]);
                if !delta {
                    *ek += b.rot[k];
                }
            }
            let mut q = angle_quaternion(e);
            if !delta && (b.flags & 0x100000) != 0 {
                let a = b.q_alignment;
                if a[0] * q[0] + a[1] * q[1] + a[2] * q[2] + a[3] * q[3] < 0.0 {
                    q = [-q[0], -q[1], -q[2], -q[3]];
                }
            }
            q
        };

        let pos = if flags & RAWPOS != 0 {
            [half(r.u16(p + 16)), half(r.u16(p + 18)), half(r.u16(p + 20))]
        } else {
            let mut v = [0f32; 3];
            for (k, vk) in v.iter_mut().enumerate() {
                *vk = self.anim_value(p + 16, k, frame, posscale);
                if !delta {
                    *vk += b.pos[k];
                }
            }
            v
        };
        (pos, q)
    }

    /// `ExtractAnimValue` for one axis of an `mstudioanim_valueptr_t` (offsets are relative
    /// to the value pointer itself).
    fn anim_value(&self, valueptr: usize, axis: usize, frame: usize, scale: f32) -> f32 {
        let off = R(&self.raw).i16(valueptr + axis * 2);
        if off <= 0 {
            return 0.0;
        }
        self.anim_track(valueptr + off as usize, frame, scale)
    }

    /// Read frame `frame` of an RLE-compressed i16 track (no interpolation).
    fn anim_track(&self, track: usize, frame: usize, scale: f32) -> f32 {
        let r = R(&self.raw);
        let mut v = track;
        let mut k = frame as i32;
        loop {
            if v + 2 > self.raw.len() {
                return 0.0;
            }
            let valid = self.raw[v] as i32;
            let total = self.raw[v + 1] as i32;
            if total == 0 {
                return 0.0;
            }
            if total > k {
                let idx = if valid > k { k + 1 } else { valid };
                return r.i16(v + idx as usize * 2) as f32 * scale;
            }
            k -= total;
            v += (valid as usize + 1) * 2;
        }
    }
}

struct Vert {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    joints: [u16; 4],
    weights: [f32; 4],
}

/// RadianEuler (x = roll, y = pitch, z = yaw) to quaternion, Source convention.
pub fn angle_quaternion(a: [f32; 3]) -> [f32; 4] {
    let (sy, cy) = (a[2] * 0.5).sin_cos();
    let (sp, cp) = (a[1] * 0.5).sin_cos();
    let (sr, cr) = (a[0] * 0.5).sin_cos();
    let (sr_cp, cr_sp) = (sr * cp, cr * sp);
    let (cr_cp, sr_sp) = (cr * cp, sr * sp);
    [
        sr_cp * cy - cr_sp * sy,
        cr_sp * cy + sr_cp * sy,
        cr_cp * sy - sr_sp * cy,
        cr_cp * cy + sr_sp * sy,
    ]
}

fn quat64(v: u64) -> [f32; 4] {
    let comp = |bits: u64| (bits as i64 - 1_048_576) as f32 * (1.0 / 1_048_576.5);
    let x = comp(v & 0x1F_FFFF);
    let y = comp((v >> 21) & 0x1F_FFFF);
    let z = comp((v >> 42) & 0x1F_FFFF);
    let mut w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
    if v >> 63 != 0 {
        w = -w;
    }
    [x, y, z, w]
}

fn half(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1F) as i32;
    let m = (h & 0x3FF) as f32;
    s * match e {
        0 => m * 2f32.powi(-24),
        31 => {
            if m == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

/// A cockpit's main screen (`*_int_screen`) shows a camera feed in the game, hiding whatever
/// of the cockpit lies behind it. Drawn without the screen (so the world shows through), that
/// geometry would show: these find it, seen from the camera bone.
pub mod screen_cull {
    use super::{MeshData, Model};
    use anyhow::Result;

    type V3 = [f32; 3];
    type Q = [f32; 4];

    /// A vertex this close in front of the screen (as a fraction of the way from the eye) counts
    /// as on it: BT's console has a frame of thin cables lying on the screen surface, a few
    /// percent either side of it.
    const SURFACE: f32 = 1.0 / 0.97;

    fn qmul(a: Q, b: Q) -> Q {
        [
            a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
            a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
            a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
            a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
        ]
    }
    fn qrot(q: Q, v: V3) -> V3 {
        let r = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), [-q[0], -q[1], -q[2], q[3]]);
        [r[0], r[1], r[2]]
    }
    fn add(a: V3, b: V3) -> V3 {
        [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
    }
    fn sub(a: V3, b: V3) -> V3 {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }
    fn cross(a: V3, b: V3) -> V3 {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }
    fn dot(a: V3, b: V3) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// Model-space (rotation, position) of every bone in an animation's first frame.
    pub fn pose(m: &Model, anim: usize) -> Result<Vec<(Q, V3)>> {
        let an = m.animation(anim)?;
        anyhow::ensure!(!an.frames.is_empty(), "empty animation");
        Ok(pose_frame(m, &an, 0))
    }

    /// Model-space (rotation, position) of every bone in one frame of an animation.
    pub fn pose_frame(m: &Model, an: &super::Animation, frame: usize) -> Vec<(Q, V3)> {
        let fr = &an.frames[frame.min(an.frames.len().saturating_sub(1))];
        let mut w: Vec<(Q, V3)> = Vec::with_capacity(m.bones.len());
        for (i, b) in m.bones.iter().enumerate() {
            let (p, q) = fr[i];
            w.push(if b.parent < 0 {
                (q, p)
            } else {
                let (pq, pp) = w[b.parent as usize];
                (qmul(pq, q), add(pp, qrot(pq, p)))
            });
        }
        w
    }

    /// A vertex where the pose puts it (by its heaviest bone).
    pub fn skin(m: &Model, pose: &[(Q, V3)], md: &MeshData, i: usize) -> V3 {
        let (j, w) = (md.joints[i], md.weights[i]);
        let k = (0..4).max_by(|&x, &y| w[x].total_cmp(&w[y])).unwrap_or(0);
        let bone = j[k] as usize;
        let (Some(b), Some(&(q, p))) = (m.bones.get(bone), pose.get(bone)) else { return md.positions[i] };
        let v = md.positions[i];
        let t = &b.pose_to_bone;
        let local = [0, 1, 2].map(|r| t[r][0] * v[0] + t[r][1] * v[1] + t[r][2] * v[2] + t[r][3]);
        add(p, qrot(q, local))
    }

    /// Möller-Trumbore: the nearest fraction of the segment eye -> p at which it crosses the
    /// screen's triangles.
    pub fn screen_hit(screen: &[[V3; 3]], eye: V3, p: V3) -> Option<f32> {
        let d = sub(p, eye);
        screen
            .iter()
            .filter_map(|tri| {
                let (e1, e2) = (sub(tri[1], tri[0]), sub(tri[2], tri[0]));
                let h = cross(d, e2);
                let det = dot(e1, h);
                if det.abs() < 1e-9 {
                    return None;
                }
                let s = sub(eye, tri[0]);
                let u = dot(s, h) / det;
                if !(0.0..=1.0).contains(&u) {
                    return None;
                }
                let q = cross(s, e1);
                let v = dot(d, q) / det;
                if v < 0.0 || u + v > 1.0 {
                    return None;
                }
                let t = dot(e2, q) / det;
                (t > 0.0).then_some(t)
            })
            .min_by(f32::total_cmp)
    }

    /// Each vertex's connected piece of the mesh (a representative vertex), by shared
    /// positions so split normals and UV seams don't break a piece apart.
    fn components(md: &MeshData) -> Vec<usize> {
        fn find(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        let n = md.positions.len();
        let mut parent: Vec<usize> = (0..n).collect();
        let mut by_pos: std::collections::HashMap<[u32; 3], usize> = Default::default();
        for i in 0..n {
            let key = md.positions[i].map(f32::to_bits);
            let j = *by_pos.entry(key).or_insert(i);
            let (a, b) = (find(&mut parent, i), find(&mut parent, j));
            parent[a] = b;
        }
        for t in md.indices.chunks_exact(3) {
            for k in 1..3 {
                let (a, b) = (find(&mut parent, t[0] as usize), find(&mut parent, t[k] as usize));
                parent[a] = b;
            }
        }
        (0..n).map(|i| find(&mut parent, i)).collect()
    }

    /// Per mesh, per triangle: whether it belongs to a piece of the mesh lying mostly behind
    /// (or on) an `_int_screen` mesh seen from bone `eye` (screen meshes are never marked).
    pub fn hidden_triangles(m: &Model, meshes: &[MeshData], mats: &[String], pose: &[(Q, V3)], eye: usize) -> Vec<Vec<bool>> {
        let is_screen = |mat: &String| mat.to_ascii_lowercase().contains("_int_screen");
        let eye = pose[eye].1;
        let mut screen: Vec<[V3; 3]> = Vec::new();
        for (md, mat) in meshes.iter().zip(mats) {
            if is_screen(mat) {
                for t in md.indices.chunks_exact(3) {
                    screen.push([0, 1, 2].map(|k| skin(m, pose, md, t[k] as usize)));
                }
            }
        }
        let behind = |p: V3| screen_hit(&screen, eye, p).is_some_and(|t| t < SURFACE);
        meshes
            .iter()
            .zip(mats)
            .map(|(md, mat)| {
                if is_screen(mat) || screen.is_empty() {
                    return vec![false; md.indices.len() / 3];
                }
                let hid: Vec<bool> = (0..md.positions.len()).map(|i| behind(skin(m, pose, md, i))).collect();
                // Whole connected pieces go when most of their vertices are hidden: the cables
                // cross the screen surface back and forth.
                let comp = components(md);
                let mut count: std::collections::HashMap<usize, (usize, usize)> = Default::default();
                for (i, &c) in comp.iter().enumerate() {
                    let e = count.entry(c).or_default();
                    e.0 += hid[i] as usize;
                    e.1 += 1;
                }
                md.indices.chunks_exact(3).map(|t| count.get(&comp[t[0] as usize]).is_some_and(|&(h, n)| h * 2 > n)).collect()
            })
            .collect()
    }
}
