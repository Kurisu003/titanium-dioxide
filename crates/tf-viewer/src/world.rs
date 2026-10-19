//! Map geometry: BSP render meshes and static props.

use crate::convert::{self, Cache};
use crate::gamedata::GameData;
use bevy::prelude::*;
use std::collections::HashMap;
use tf_assets::bsp::{mesh_flags, Bsp};
use tf_assets::mdl::Model;
use tf_sim::collision::CollisionWorld;
use tf_sim::glam::Vec3 as SVec3;

/// Spawn the world model (brush model 0) as one mesh per material.
pub fn spawn_world(
    commands: &mut Commands,
    root: Entity,
    gd: &GameData,
    bsp: &Bsp,
    cache: &mut Cache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    blends: &mut Assets<convert::BlendMaterial>,
    collision: &mut CollisionWorld,
    sky_light: Vec3,
) {
    let t = std::time::Instant::now();
    let bsp_meshes = bsp.meshes();
    let sorts = bsp.material_sorts();
    let texdata = bsp.texture_data();
    let indices = bsp.mesh_indices();
    let Some(world) = bsp.models().first().copied() else {
        return;
    };

    struct Batch {
        pos: Vec<[f32; 3]>,
        nrm: Vec<[f32; 3]>,
        uv: Vec<[f32; 2]>,
        lm: Vec<[f32; 2]>,
        col: Vec<[f32; 4]>,
        idx: Vec<u32>,
        blend: bool,
    }
    // Baked lighting: one image per lightmap page, made after the meshes are gathered (the
    // sky term needs their normals).
    let raw_pages = bsp.lightmaps();
    let mut batches: HashMap<(i16, bool, i16), Batch> = HashMap::new();
    let skip = mesh_flags::SKY | mesh_flags::SKY_2D | mesh_flags::SKIP | mesh_flags::TRIGGER;
    let mut tris = 0usize;
    let mut sky_names = std::collections::BTreeSet::new();
    for m in &bsp_meshes[world.first_mesh as usize..(world.first_mesh + world.num_meshes) as usize]
    {
        if m.flags & skip != 0 {
            if m.flags & (mesh_flags::SKY | mesh_flags::SKY_2D) != 0 {
                let sort = sorts[m.material_sort as usize];
                sky_names.insert(texdata[sort.texture_data as usize].name.clone());
            }
            continue;
        }
        let sort = sorts[m.material_sort as usize];
        let td = &texdata[sort.texture_data as usize];
        let lname = td.name.to_ascii_lowercase();
        // Tool brushes are not drawn. (Fog and godray cards are, with their own shader: uber.rs.)
        if lname.starts_with("tools")
            || (lname.starts_with("world\\atmosphere") && !crate::uber::enabled())
        {
            continue;
        }
        // TF_HIDE=substr: debug toggle that leaves out world materials whose name contains it.
        if std::env::var("TF_HIDE")
            .is_ok_and(|h| !h.is_empty() && lname.contains(&h.to_ascii_lowercase()))
        {
            continue;
        }
        let blend = m.flags & mesh_flags::TRANSLUCENT != 0;
        let page = if (sort.lightmap as usize) < raw_pages.len() && sort.lightmap >= 0 {
            sort.lightmap
        } else {
            -1
        };
        let b = batches
            .entry((sort.texture_data, blend, page))
            .or_insert_with(|| Batch {
                pos: Vec::new(),
                nrm: Vec::new(),
                uv: Vec::new(),
                lm: Vec::new(),
                col: Vec::new(),
                idx: Vec::new(),
                blend,
            });
        let start = m.first_index as usize;
        let mut remap: HashMap<usize, u32> = HashMap::new();
        for &i in &indices[start..start + m.num_triangles as usize * 3] {
            let vi = sort.vertex_offset as usize + i as usize;
            let next = b.pos.len() as u32;
            let id = *remap.entry(vi).or_insert_with(|| {
                let v = bsp.vertex(m.flags, vi).unwrap_or_default();
                b.pos.push(v.pos);
                b.nrm.push(v.normal);
                b.uv.push(v.uv);
                b.lm.push(v.lightmap_uv.unwrap_or([0.0, 0.0]));
                b.col.push(v.color.map(|c| c as f32 / 255.0));
                next
            });
            b.idx.push(id);
        }
        tris += m.num_triangles as usize;
    }
    let n = batches.len();
    let pages: Vec<Handle<Image>> = {
        let tl = std::time::Instant::now();
        let mut tris: Vec<Vec<[([f32; 2], Vec3); 3]>> = vec![Vec::new(); raw_pages.len()];
        for ((_, _, page), b) in &batches {
            if *page < 0 {
                continue;
            }
            for tri in b.idx.chunks_exact(3) {
                tris[*page as usize].push(std::array::from_fn(|k| {
                    let i = tri[k] as usize;
                    (b.lm[i], Vec3::from(b.nrm[i]))
                }));
            }
        }
        let jobs: Vec<(&tf_assets::bsp::LightmapPage, &Vec<[([f32; 2], Vec3); 3]>)> =
            raw_pages.iter().zip(&tris).collect();
        let baked = parallel_map(&jobs, |(p, t)| bake_lightmap(p, t, sky_light));
        let pages = baked
            .into_iter()
            .zip(&raw_pages)
            .map(|(data, p)| {
                let mut img = Image::new(
                    bevy::render::render_resource::Extent3d {
                        width: p.width,
                        height: p.height,
                        depth_or_array_layers: 1,
                    },
                    bevy::render::render_resource::TextureDimension::D2,
                    data.unwrap_or_else(|| vec![0; (p.width * p.height * 8) as usize]),
                    bevy::render::render_resource::TextureFormat::Rgba16Float,
                    bevy::asset::RenderAssetUsages::RENDER_WORLD,
                );
                img.sampler = bevy::image::ImageSampler::linear();
                images.add(img)
            })
            .collect::<Vec<_>>();
        log::info!("{} lightmap pages baked in {:?}", pages.len(), tl.elapsed());
        pages
    };
    // In key order, so the collision triangles (and with them the BVH, the nav grids' probes
    // and anything else that breaks ties by triangle order) are the same on every run: a
    // HashMap's order changes from run to run.
    let mut batches: Vec<_> = batches.into_iter().collect();
    batches.sort_unstable_by_key(|(k, _)| *k);
    // Collision on this thread, then the meshes (tangent generation is most of the work) on
    // every core.
    let batches: Vec<((i16, bool, i16), std::sync::Mutex<Option<Batch>>)> = batches
        .into_iter()
        .map(|(k, b)| {
            log::debug!(
                "batch {:?} flags {:#x} blend {} tris {}",
                texdata[k.0 as usize].name,
                texdata[k.0 as usize].flags,
                b.blend,
                b.idx.len() / 3
            );
            if !b.blend {
                for tri in b.idx.chunks_exact(3) {
                    let v = |i: u32| SVec3::from(b.pos[i as usize]);
                    collision.add(v(tri[0]), v(tri[1]), v(tri[2]));
                }
            }
            (k, std::sync::Mutex::new(Some(b)))
        })
        .collect();
    let two_layer_names = |td: i16, blend: bool| {
        let name = &texdata[td as usize].name;
        !blend
            && name.to_ascii_lowercase().ends_with("_bm")
            && std::env::var_os("TF_NO_BM").is_none()
    };
    let prep: &Cache = cache;
    let built = parallel_map(&batches, |((td, _, page), b)| {
        let b = b.lock().ok()?.take()?;
        let blend = b.blend;
        // The material's textures decode here too (the main thread only adds them).
        convert::prefetch_material(gd, prep, &texdata[*td as usize].name, blend);
        let mut mesh = convert::build_mesh(b.pos, b.nrm, b.uv, b.idx, None);
        if *page >= 0 {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, b.lm);
        }
        let effect_vcol = crate::uber::info(gd, &texdata[*td as usize].name)
            .is_some_and(|i| i.flags & (crate::uber::VCOLT | crate::uber::VCOLA) != 0);
        if two_layer_names(*td, blend) || effect_vcol {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, b.col);
        }
        Some((mesh, blend))
    });
    for (((td, _, page), _), r) in batches.iter().zip(built) {
        let (td, page) = (*td, *page);
        let Some(Some((mesh, blend))) = r else {
            continue;
        };
        let name = &texdata[td as usize].name;
        // Two-layer materials blend by vertex alpha in their own shader.
        let two_layer = if two_layer_names(td, blend) {
            convert::blend_material(gd, cache, images, materials, blends, name)
        } else {
            None
        };
        let mesh = meshes.add(mesh);
        let e = match two_layer {
            Some(m) => commands
                .spawn((Mesh3d(mesh), MeshMaterial3d(m), Transform::IDENTITY))
                .id(),
            None => {
                let mat = convert::material(gd, cache, images, materials, name, blend);
                commands
                    .spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY))
                    .id()
            }
        };
        if page >= 0 {
            commands.entity(e).insert(bevy::pbr::Lightmap {
                image: pages[page as usize].clone(),
                uv_rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                bicubic_sampling: false,
            });
        }
        if blend {
            // Translucent world surfaces (dirt and grime overlays on the ground, decals, glass)
            // draw before every other translucent thing (see `convert::TransparentOrder`).
            commands.entity(e).insert(convert::TransparentOrder::WORLD);
        }
        // Translucent surfaces and effect cards (fog sheets, godrays) cast no sun shadow: the
        // atmosphere cards hanging over Forwardbase Kodai shaded the whole play area.
        if blend || crate::uber::info(gd, name).is_some() {
            commands.entity(e).insert(bevy::light::NotShadowCaster);
        }
        commands.entity(root).add_child(e);
    }
    log::info!("sky materials: {sky_names:?}");
    log::info!(
        "world: {tris} triangles in {n} batches, built in {:?}",
        t.elapsed()
    );
}

/// Build an unanimated model as (mesh, material) pairs in model space, plus its triangles.
/// A static model's meshes (with their material names) and model-space triangles, without
/// touching any Bevy assets, so it can run on worker threads.
fn static_model_data(gd: &GameData, path: &str) -> anyhow::Result<PropModel> {
    let model = Model::parse(gd.read_file(path)?)?;
    let mut tris = Vec::new();
    let mut radius = 0.0f32;
    let mut lods = Vec::new();
    // The model's own LODs (VTX switch points); the extra ones only when they really are
    // lighter, at most MAX_PROP_LODS.
    let switches = model.lod_switch_points();
    let mut last_tris = usize::MAX;
    for (l, &switch) in switches
        .iter()
        .enumerate()
        .take(MAX_PROP_LODS)
        .chain((switches.is_empty()).then_some((0, &0.0)))
    {
        let mds = model.meshes_lod(&[], l)?;
        let n: usize = mds.iter().map(|m| m.indices.len() / 3).sum();
        if l > 0 && n * 10 >= last_tris * 9 {
            continue; // not meaningfully lighter than the previous LOD
        }
        last_tris = n;
        let mut parts = Vec::new();
        for md in mds {
            let tex = model
                .skin_families
                .first()
                .and_then(|f| f.get(md.material))
                .copied()
                .unwrap_or(0) as usize;
            let mat_name = model.textures.get(tex).cloned().unwrap_or_default();
            if l == 0 {
                for &i in &md.indices {
                    radius = radius.max(Vec3::from(md.positions[i as usize]).length());
                }
            }
            parts.push((
                mat_name,
                convert::build_mesh(md.positions, md.normals, md.uvs, md.indices, None),
            ));
        }
        lods.push((if l == 0 { 0.0 } else { switch }, parts));
    }
    tris.extend(model.collision_tris().into_iter().flat_map(|(t, _)| t));
    Ok(PropModel {
        lods,
        tris,
        hull: model.collision_hull_tris(),
        radius,
    })
}

/// At most this many LODs per prop model are built.
const MAX_PROP_LODS: usize = 3;
/// Source's LOD metric is 100 / (screen size of a unit sphere), about 0.31 x distance at the
/// reference 640-wide, 90-degree view, so a switch point is about 3.2 x its distance in units.
const LOD_METRIC_TO_UNITS: f32 = 3.2;
/// Props are drawn while their bounding sphere covers more than about 5 pixels at 1080p and
/// 70 degrees: distance = radius x this (the game fades small props with distance too).
const PROP_DRAW_RADII: f32 = 150.0;
/// Props this big (radius, units) are drawn at any distance.
const PROP_ALWAYS_DRAWN: f32 = 400.0;
/// Props smaller than this (radius, units) don't cast real-time shadows: their shadows on the
/// world are already baked into the lightmaps.
const PROP_SHADOW_MIN_RADIUS: f32 = 150.0;

/// Prop rendering switches, for A/B measurements: TF_PROP_LOD=0 (LOD 0 only), TF_PROP_CULL=0
/// (no draw distance), TF_PROP_SHADOWS=all (every prop casts), TF_PROP_RADII=N (draw distance
/// in bounding radii).
struct PropOptions {
    lod: bool,
    cull: bool,
    small_no_shadow: bool,
    draw_radii: f32,
}

impl PropOptions {
    fn from_env() -> Self {
        let off = |k: &str| std::env::var(k).is_ok_and(|v| v == "0");
        Self {
            lod: !off("TF_PROP_LOD"),
            cull: !off("TF_PROP_CULL"),
            small_no_shadow: std::env::var("TF_PROP_SHADOWS").map_or(true, |v| v != "all"),
            draw_radii: std::env::var("TF_PROP_RADII")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(PROP_DRAW_RADII),
        }
    }
}

/// A static prop model ready to instance: LODs as (switch point, parts), its model-space
/// triangles for collision, and its bounding radius about the origin.
struct PropModel {
    lods: Vec<(f32, Vec<(String, Mesh)>)>,
    /// The per-triangle collision mesh (`Model::collision_tris`), three corners each: LOD 0's
    /// triangles less foliage cards. Only shots trace it.
    tris: Vec<[f32; 3]>,
    /// The convex collision hulls as triangles (`Model::collision_hull_tris`): what movement hits.
    hull: Vec<[[f32; 3]; 3]>,
    radius: f32,
}

/// One lightmap page in the game's linear light (RGBA16F), the indirect term the world shader
/// (uber*_wld ps_5_0) adds up before the sun:
/// - lightmap A (`lightmapTexture0`, created sRGB: materialsystem_dx11.dll 0x18006c360 passes
///   flag 0x20000, which turns DXGI 28 into 28's _SRGB 29 at 0x18002847f) times its alpha
///   scale `a * 3.984375 + 0.015625`, times `N . N_vertex` (1 here: no normal maps);
/// - plus `c_skyColor` times the sky term of lightmap B (`lightmapTexture1`, linear UNORM):
///   the sky visibility vector `rgb * 2.00787 - 1` (see `env::sky_term`), for the surface's
///   normal. That needs each texel's normal, so the page's triangles are rasterized in
///   lightmap space (vertex normals, interpolated); texels no triangle centre covers (chart
///   borders, read by bilinear filtering) take their neighbours'.
fn bake_lightmap(
    p: &tf_assets::bsp::LightmapPage,
    tris: &[[([f32; 2], Vec3); 3]],
    sky: Vec3,
) -> Vec<u8> {
    let (w, h) = (p.width as usize, p.height as usize);
    let mut nrm = vec![Vec3::ZERO; w * h];
    for t in tris {
        let pt = t.map(|(uv, _)| Vec2::new(uv[0] * w as f32 - 0.5, uv[1] * h as f32 - 0.5));
        let area = (pt[1] - pt[0]).perp_dot(pt[2] - pt[0]);
        if area.abs() < 1e-8 {
            continue;
        }
        let lo = pt[0].min(pt[1]).min(pt[2]).floor().max(Vec2::ZERO);
        let hi = pt[0]
            .max(pt[1])
            .max(pt[2])
            .ceil()
            .min(Vec2::new(w as f32 - 1.0, h as f32 - 1.0));
        for y in lo.y as usize..=hi.y.max(lo.y) as usize {
            for x in lo.x as usize..=hi.x.max(lo.x) as usize {
                let c = Vec2::new(x as f32, y as f32);
                let b = [
                    (pt[2] - pt[1]).perp_dot(c - pt[1]) / area,
                    (pt[0] - pt[2]).perp_dot(c - pt[2]) / area,
                    (pt[1] - pt[0]).perp_dot(c - pt[0]) / area,
                ];
                if b.iter().any(|&v| v < -1e-4) {
                    continue;
                }
                nrm[y * w + x] =
                    (t[0].1 * b[0] + t[1].1 * b[1] + t[2].1 * b[2]).normalize_or_zero();
            }
        }
    }
    // Spread normals into the uncovered border texels.
    for _ in 0..4 {
        let prev = nrm.clone();
        for y in 0..h {
            for x in 0..w {
                if prev[y * w + x] != Vec3::ZERO {
                    continue;
                }
                let mut sum = Vec3::ZERO;
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                        sum += prev[ny as usize * w + nx as usize];
                    }
                }
                nrm[y * w + x] = sum.normalize_or_zero();
            }
        }
    }
    let srgb: [f32; 256] = std::array::from_fn(|i| {
        let c = i as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    let mut out = Vec::with_capacity(w * h * 8);
    for i in 0..w * h {
        let a = &p.sky_a[i * 4..i * 4 + 4];
        let b = &p.sky_b[i * 4..i * 4 + 4];
        let scale = a[3] as f32 / 255.0 * 3.984375 + 0.015625;
        let indirect = Vec3::new(
            srgb[a[0] as usize],
            srgb[a[1] as usize],
            srgb[a[2] as usize],
        ) * scale;
        let v = Vec3::new(b[0] as f32, b[1] as f32, b[2] as f32) / 255.0 * 2.007874 - 1.0;
        let n = if nrm[i] == Vec3::ZERO {
            Vec3::Z
        } else {
            nrm[i]
        };
        let c = indirect + sky * crate::env::sky_term(n, v);
        for v in [c.x, c.y, c.z, 1.0] {
            out.extend_from_slice(&crate::env::f32_to_half(v).to_le_bytes());
        }
    }
    out
}

/// Map `f` over `items` on every core, keeping the order (None where a worker panicked).
pub fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<Option<R>> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(items.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<(usize, R)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        done.push((i, f(item)));
                    }
                    done
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap_or_default())
            .collect()
    });
    let mut res: Vec<Option<R>> = (0..items.len()).map(|_| None).collect();
    for (i, r) in out.drain(..) {
        res[i] = Some(r);
    }
    res
}

pub fn build_static_model(
    gd: &GameData,
    path: &str,
    cache: &mut Cache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> anyhow::Result<(Vec<(Handle<Mesh>, Handle<StandardMaterial>)>, Vec<[f32; 3]>)> {
    let model = Model::parse(gd.read_file(path)?)?;
    let mut out = Vec::new();
    let mut tris = Vec::new();
    for md in model.meshes(&[])? {
        let tex = model
            .skin_families
            .first()
            .and_then(|f| f.get(md.material))
            .copied()
            .unwrap_or(0) as usize;
        let mat_name = model.textures.get(tex).cloned().unwrap_or_default();
        let mat = convert::material(gd, cache, images, materials, &mat_name, false);
        for &i in &md.indices {
            tris.push(md.positions[i as usize]);
        }
        let mesh = meshes.add(convert::build_mesh(
            md.positions,
            md.normals,
            md.uvs,
            md.indices,
            None,
        ));
        out.push((mesh, mat));
    }
    Ok((out, tris))
}

/// As `build_static_model`, with each part's model-space transform: its bone where the model's
/// first animation frame puts it. A part's mesh is stored around its bone, which can lie along
/// another axis than the model (the rocket projectiles' meshes run along Z, the models along X).
pub fn build_posed_model(
    gd: &GameData,
    path: &str,
    cache: &mut Cache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> anyhow::Result<Vec<(Handle<Mesh>, Handle<StandardMaterial>, Transform)>> {
    let model = Model::parse(gd.read_file(path)?)?;
    let pose = tf_assets::mdl::screen_cull::pose(&model, 0).ok();
    let mut out = Vec::new();
    for md in model.meshes(&[])? {
        let tex = model
            .skin_families
            .first()
            .and_then(|f| f.get(md.material))
            .copied()
            .unwrap_or(0) as usize;
        let mat_name = model.textures.get(tex).cloned().unwrap_or_default();
        let mat = convert::material(gd, cache, images, materials, &mat_name, false);
        // Bone pose x pose_to_bone, as screen_cull::skin places a vertex.
        let bone = md.joints.first().map(|j| j[0] as usize);
        let place = match (bone, pose.as_ref()) {
            (Some(b), Some(pose)) if b < pose.len() && b < model.bones.len() => {
                let ([qx, qy, qz, qw], p) = pose[b];
                let t = &model.bones[b].pose_to_bone;
                let inv = Mat4::from_cols(
                    Vec4::new(t[0][0], t[1][0], t[2][0], 0.0),
                    Vec4::new(t[0][1], t[1][1], t[2][1], 0.0),
                    Vec4::new(t[0][2], t[1][2], t[2][2], 0.0),
                    Vec4::new(t[0][3], t[1][3], t[2][3], 1.0),
                );
                Transform::from_matrix(
                    Mat4::from_rotation_translation(Quat::from_xyzw(qx, qy, qz, qw), Vec3::from(p))
                        * inv,
                )
            }
            _ => Transform::IDENTITY,
        };
        let mesh = meshes.add(convert::build_mesh(
            md.positions,
            md.normals,
            md.uvs,
            md.indices,
            None,
        ));
        out.push((mesh, mat, place));
    }
    Ok(out)
}

/// Static props (game lump `sprp`). Each distinct model is converted once and instanced; small
/// props go into batches (`propbatch.rs`). `root_tf` is the transform of `root` (which has no
/// parent).
#[allow(clippy::too_many_arguments)]
pub fn spawn_static_props(
    commands: &mut Commands,
    root: Entity,
    root_tf: &Transform,
    gd: &GameData,
    bsp: &Bsp,
    cache: &mut Cache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    (batch_materials, buffers): (
        &mut Assets<crate::propbatch::PropBatchMaterial>,
        &mut Assets<bevy::render::storage::ShaderStorageBuffer>,
    ),
    collision: &mut CollisionWorld,
) {
    let t = std::time::Instant::now();
    let sp = match bsp.static_props() {
        Ok(sp) => sp,
        Err(e) => {
            log::warn!("static props: {e}");
            return;
        }
    };
    // Per model: LODs as (switch point, parts) and the bounding radius. A part keeps its mesh
    // data when it can go into a batch.
    type Part = (Handle<Mesh>, Handle<StandardMaterial>, Option<Mesh>);
    let mut built: Vec<Option<(Vec<(f32, Vec<Part>)>, f32)>> = vec![None; sp.model_names.len()];
    let batching = crate::propbatch::enabled();
    // Model-space per-triangle meshes and collision hulls of each prop model.
    let mut shapes: Vec<Vec<[f32; 3]>> = vec![Vec::new(); sp.model_names.len()];
    let mut hulls: Vec<Vec<[[f32; 3]; 3]>> = vec![Vec::new(); sp.model_names.len()];
    // The props the map's collision lists, with their contents (the rest are never traced).
    let in_cm = bsp.static_prop_contents();
    let mut failed = 0;
    // Parse the models and build their meshes (tangents included) on every core; materials
    // and asset handles are made on this thread afterwards.
    let prep: &Cache = cache;
    let parsed = parallel_map(&sp.model_names, |name| -> anyhow::Result<PropModel> {
        if name.starts_with("models/vistas/") {
            anyhow::bail!("vista (built by sky.rs)");
        }
        if std::env::var("TF_HIDE").is_ok_and(|h| {
            !h.is_empty() && name.to_ascii_lowercase().contains(&h.to_ascii_lowercase())
        }) {
            anyhow::bail!("hidden (TF_HIDE)");
        }
        // Godray models are soft additive light-shaft volumes; uber.rs draws them with their
        // shader's fades. with TF_NO_UBER they are left out (as ordinary translucent meshes they become
        // hard walls of haze).
        if name.to_ascii_lowercase().contains("godray") && !crate::uber::enabled() {
            anyhow::bail!("godray volume (not drawn)");
        }
        let pm = static_model_data(gd, name)?;
        // The materials' textures decode here too (the main thread only adds them).
        for (_, parts) in &pm.lods {
            for (mat_name, _) in parts {
                convert::prefetch_material(gd, prep, mat_name, false);
            }
        }
        Ok(pm)
    });
    for (mi, r) in parsed.into_iter().enumerate() {
        match r.unwrap_or_else(|| Err(anyhow::anyhow!("worker panicked"))) {
            Ok(pm) => {
                shapes[mi] = pm.tris;
                hulls[mi] = pm.hull;
                let lods = pm
                    .lods
                    .into_iter()
                    .map(|(switch, parts)| {
                        let parts = parts
                            .into_iter()
                            .map(|(mat_name, mesh)| {
                                let mat = convert::material(
                                    gd, cache, images, materials, &mat_name, false,
                                );
                                let keep = batching
                                    && materials.get(&mat).is_some_and(|m| {
                                        crate::propbatch::batchable(
                                            &mesh,
                                            m,
                                            cache.uber.contains_key(&mat.id()),
                                        )
                                    });
                                let data = keep.then(|| mesh.clone());
                                (meshes.add(mesh), mat, data)
                            })
                            .collect();
                        (switch, parts)
                    })
                    .collect();
                built[mi] = Some((lods, pm.radius));
            }
            Err(e) => {
                let n = sp.model_names[mi].to_ascii_lowercase();
                if !n.starts_with("models/vistas/")
                    && !n.contains("godray")
                    && std::env::var_os("TF_HIDE").is_none()
                {
                    failed += 1;
                }
                log::debug!("prop {}: {e}", sp.model_names[mi]);
            }
        }
    }
    let mut count = 0;
    let (mut solid, mut no_hull, mut pilot_only) = (0, 0, 0);
    let (mut parts_spawned, mut parts_batched) = (0usize, 0usize);
    let opts = PropOptions::from_env();
    let mut batcher = crate::propbatch::PropBatcher::new(root_tf);
    for (pi, p) in sp.props.iter().enumerate() {
        // Vistas belong to the 3D skybox (sky.rs).
        if sp.model_names[p.model as usize].starts_with("models/vistas/") {
            continue;
        }
        let Some(Some((lods, radius))) = built.get(p.model as usize) else {
            continue;
        };
        let [pitch, yaw, roll] = p.angles.map(f32::to_radians);
        // Source angles: yaw about Z, then pitch about Y, then roll about X.
        let rot =
            Quat::from_rotation_z(yaw) * Quat::from_rotation_y(pitch) * Quat::from_rotation_x(roll);
        let tf = Transform {
            translation: Vec3::from(p.origin),
            rotation: rot,
            scale: Vec3::splat(p.scale.max(0.0001)),
        };
        // Collision as engine.dll traces static props (0x1801b7cf0): only the props in the
        // map's CM_PRIMITIVES, each by its contents there. Movement (the player's mask, detail
        // level 0) hits the model's convex hulls; shots (TRACEDETAILLEVEL_HIGH) hit the
        // per-triangle mesh. Small props and railings have PLAYER and BULLET but not TITAN: BT
        // walks through them.
        if let Some(&c) = in_cm.get(&pi) {
            use tf_assets::bsp::contents::{BULLET, PLAYER, TITAN};
            use tf_sim::collision::{CLIP_PILOT, CLIP_TITAN};
            let m = tf.to_matrix();
            let v = |a: [f32; 3]| SVec3::from(m.transform_point3(Vec3::from(a)).to_array());
            if c & BULLET != 0 {
                for tri in shapes[p.model as usize].chunks_exact(3) {
                    collision.add_detail(v(tri[0]), v(tri[1]), v(tri[2]));
                }
            }
            for tri in &hulls[p.model as usize] {
                for (bit, set) in [(PLAYER, CLIP_PILOT), (TITAN, CLIP_TITAN)] {
                    if c & bit != 0 {
                        collision.add_clip(set, v(tri[0]), v(tri[1]), v(tri[2]));
                    }
                }
            }
            solid += 1;
            no_hull += hulls[p.model as usize].is_empty() as usize;
            pilot_only += (c & PLAYER != 0 && c & TITAN == 0) as usize;
        }
        // One entity per mesh part and LOD, straight under the root (no per-prop parent), each
        // with the distance range it's drawn in (world units, i.e. after the root's scale).
        let r = radius * tf.scale.x;
        let draw_to = if !opts.cull || r >= PROP_ALWAYS_DRAWN {
            f32::INFINITY
        } else {
            (r * opts.draw_radii).max(600.0)
        };
        let lod_count = if opts.lod { lods.len() } else { 1 };
        for (l, (switch, parts)) in lods.iter().enumerate().take(lod_count) {
            let start = if l == 0 {
                0.0
            } else {
                switch * LOD_METRIC_TO_UNITS
            };
            let end = lods
                .get(l + 1)
                .filter(|_| l + 1 < lod_count)
                .map(|n| n.0 * LOD_METRIC_TO_UNITS)
                .unwrap_or(f32::INFINITY)
                .min(draw_to);
            if start >= end {
                continue;
            }
            let range = (start != 0.0 || end.is_finite()).then(|| {
                let (s, e) = (start * crate::player::UNIT, end * crate::player::UNIT);
                // Crossfade (dithered) over the last 10% before each switch.
                bevy::camera::visibility::VisibilityRange {
                    start_margin: s * 0.9..s,
                    end_margin: e * 0.9..e,
                    use_aabb: false,
                }
            });
            let small = opts.small_no_shadow && r < PROP_SHADOW_MIN_RADIUS;
            for (part, (mesh, mat, data)) in parts.iter().enumerate() {
                // Small props that don't cast shadows go into batches where they can.
                if let (true, Some(data), Some(range)) = (small, data, &range) {
                    if batcher.add(pi, p.model as usize, l, part, data, mat, &tf, range) {
                        parts_batched += 1;
                        continue;
                    }
                }
                let mut c = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), tf));
                if let Some(range) = &range {
                    c.insert(range.clone());
                }
                if opts.small_no_shadow && r < PROP_SHADOW_MIN_RADIUS {
                    c.insert(bevy::light::NotShadowCaster);
                }
                let c = c.id();
                commands.entity(root).add_child(c);
                parts_spawned += 1;
            }
        }
        count += 1;
    }
    drop(built);
    let batches = batcher.finish(commands, root, meshes, materials, batch_materials, buffers);
    log::info!(
        "static props: {count} instances ({solid} in the map's collision: {no_hull} without hulls stop shots only, {pilot_only} not titans; {parts_spawned} mesh entities, {parts_batched} parts in {batches} batches) of {} models ({failed} failed) in {:?}",
        sp.model_names.len(),
        t.elapsed()
    );
}

/// Add the parts of the map's own collision (`Bsp::collision_shapes`) that the render meshes
/// don't cover: `TOOLSNODRAW` backing surfaces (a floor under the terrain, the hidden faces
/// of buildings), glass, and the invisible clip brushes. Without them the Pilot dropped
/// through gaps in the render meshes and walked through clips. Solid shapes join the main
/// world; clips (no BULLET contents) only stop movement (`CLIP_PILOT`, `CLIP_TITAN`).
pub fn add_game_collision(bsp: &Bsp, collision: &mut CollisionWorld) {
    use tf_assets::bsp::contents::{BULLET, PLAYER, TITAN};
    use tf_sim::collision::{CLIP_PILOT, CLIP_TITAN};
    // Texture data whose world meshes `spawn_world` already turned into collision.
    let sorts = bsp.material_sorts();
    let texdata = bsp.texture_data();
    let skip = mesh_flags::SKY
        | mesh_flags::SKY_2D
        | mesh_flags::SKIP
        | mesh_flags::TRIGGER
        | mesh_flags::TRANSLUCENT;
    let mut rendered = std::collections::HashSet::new();
    let meshes = bsp.meshes();
    if let Some(world) = bsp.models().first() {
        for m in meshes
            .iter()
            .skip(world.first_mesh as usize)
            .take(world.num_meshes as usize)
        {
            let td = sorts[m.material_sort as usize].texture_data;
            let name = texdata
                .get(td as usize)
                .map(|t| t.name.to_ascii_lowercase())
                .unwrap_or_default();
            if m.flags & skip == 0 && !name.starts_with("tools") {
                rendered.insert(td);
            }
        }
    }
    let v = |p: [f32; 3]| SVec3::from(p);
    let (mut solid, mut pilot, mut titan) = (0usize, 0usize, 0usize);
    for shape in bsp.collision_shapes() {
        if shape.texture_data >= 0 && rendered.contains(&shape.texture_data) {
            continue;
        }
        let sky = texdata
            .get(shape.texture_data.max(0) as usize)
            .is_some_and(|t| {
                shape.texture_data >= 0 && t.name.to_ascii_lowercase().contains("toolsskybox")
            });
        let c = shape.contents;
        for t in &shape.tris {
            // The sky's box holds players in but isn't something to shoot or hook.
            if c & BULLET != 0 && c & PLAYER != 0 && !sky {
                collision.add(v(t[0]), v(t[1]), v(t[2]));
                solid += 1;
                continue;
            }
            if c & PLAYER != 0 {
                collision.add_clip(CLIP_PILOT, v(t[0]), v(t[1]), v(t[2]));
                pilot += 1;
            }
            if c & TITAN != 0 {
                collision.add_clip(CLIP_TITAN, v(t[0]), v(t[1]), v(t[2]));
                titan += 1;
            }
        }
    }
    log::info!("map collision: {solid} solid triangles the render meshes miss, {pilot} player clip and {titan} titan clip triangles");
}

/// Debug sweep (TF_COLLISION_SWEEP): drop rays on a 32-unit grid over `extra` (the map's own
/// collision that the render meshes miss, player clips included) and log where it catches a fall that `world` lets
/// through: a walkable hit of `extra` at least 16 units above `world`'s first hit, or with
/// nothing below in `world` at all.
pub fn collision_sweep(world: &CollisionWorld, extra: &CollisionWorld) {
    let (mut lo, mut hi) = (SVec3::splat(f32::MAX), SVec3::splat(f32::MIN));
    for t in extra
        .tris
        .iter()
        .chain(extra.clips.iter().flatten().flat_map(|c| c.tris.iter()))
    {
        lo = lo.min(t.a.min(t.b).min(t.c));
        hi = hi.max(t.a.max(t.b).max(t.c));
    }
    if extra.tris.is_empty() {
        log::info!("collision sweep: no extra solid triangles");
        return;
    }
    let (mut caught, mut bottomless, mut rays) = (0usize, 0usize, 0usize);
    let mut spots = Vec::new();
    let step = 32.0;
    let mut x = lo.x;
    while x <= hi.x {
        let mut y = lo.y;
        while y <= hi.y {
            rays += 1;
            // Start under the sky's lid and the height-limit clips under it (they're in the
            // player clips): from 10000 units down.
            let top = SVec3::new(x, y, hi.z.min(10000.0));
            let down = -SVec3::Z;
            let len = hi.z - lo.z + 20000.0;
            if let Some(e) = extra
                .raycast_moving(top, down, len, tf_sim::collision::CLIP_PILOT)
                .filter(|h| h.normal.z > 0.7)
            {
                let w = world.raycast(top, down, len);
                let missed = match w {
                    None => {
                        bottomless += 1;
                        true
                    }
                    Some(w) => w.point.z < e.point.z - 16.0,
                };
                if missed {
                    caught += 1;
                    if w.is_some() {
                        spots.push((e.point, w.map(|w| w.point.z)));
                    }
                }
            }
            y += step;
        }
        x += step;
    }
    log::info!("collision sweep: {rays} rays, the map's collision catches {caught} falls the render meshes miss ({bottomless} with nothing below)");
    // Group the catches into 512-unit cells (one line each, highest catch first).
    let mut cells: HashMap<(i32, i32), (usize, SVec3, f32)> = HashMap::new();
    for (p, below) in &spots {
        let e = cells
            .entry(((p.x / 512.0).floor() as i32, (p.y / 512.0).floor() as i32))
            .or_insert((0, *p, below.unwrap_or(0.0)));
        e.0 += 1;
        if p.z > e.1.z {
            e.1 = *p;
            e.2 = below.unwrap_or(0.0);
        }
    }
    let mut cells: Vec<_> = cells.into_values().collect();
    cells.sort_by(|a, b| b.0.cmp(&a.0));
    for (n, p, below) in cells.iter().take(60) {
        log::info!(
            "  {n} catches near {:.0} {:.0} {:.0} (render collision {below:.0})",
            p.x,
            p.y,
            p.z
        );
    }
}
