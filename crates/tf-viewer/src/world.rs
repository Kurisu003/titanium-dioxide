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
) {
    let t = std::time::Instant::now();
    let bsp_meshes = bsp.meshes();
    let sorts = bsp.material_sorts();
    let texdata = bsp.texture_data();
    let indices = bsp.mesh_indices();
    let Some(world) = bsp.models().first().copied() else { return };

    struct Batch {
        pos: Vec<[f32; 3]>,
        nrm: Vec<[f32; 3]>,
        uv: Vec<[f32; 2]>,
        lm: Vec<[f32; 2]>,
        col: Vec<[f32; 4]>,
        idx: Vec<u32>,
        blend: bool,
    }
    // Baked lighting: one image per lightmap page (sky A: indirect + sky light).
    let pages: Vec<Handle<Image>> = bsp
        .lightmaps()
        .into_iter()
        .map(|p| {
            let mut data = p.sky_a;
            if std::env::var_os("TF_LM_SUNVIS").is_some() {
                for (px, b) in data.chunks_exact_mut(4).zip(p.sky_b.chunks_exact(4)) {
                    px[0] = b[3];
                    px[1] = b[3];
                    px[2] = b[3];
                }
            }
            for px in data.chunks_exact_mut(4) {
                px[3] = 255;
            }
            let mut img = Image::new(
                bevy::render::render_resource::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 },
                bevy::render::render_resource::TextureDimension::D2,
                data,
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                bevy::asset::RenderAssetUsages::RENDER_WORLD,
            );
            img.sampler = bevy::image::ImageSampler::linear();
            images.add(img)
        })
        .collect();
    log::info!("{} lightmap pages", pages.len());
    let mut batches: HashMap<(i16, bool, i16), Batch> = HashMap::new();
    let skip = mesh_flags::SKY | mesh_flags::SKY_2D | mesh_flags::SKIP | mesh_flags::TRIGGER;
    let mut tris = 0usize;
    let mut sky_names = std::collections::BTreeSet::new();
    for m in &bsp_meshes[world.first_mesh as usize..(world.first_mesh + world.num_meshes) as usize] {
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
        if lname.starts_with("tools") || (lname.starts_with("world\\atmosphere") && std::env::var_os("TF_UBER").is_none()) {
            continue;
        }
        // TF_HIDE=substr: debug toggle that leaves out world materials whose name contains it.
        if std::env::var("TF_HIDE").is_ok_and(|h| !h.is_empty() && lname.contains(&h.to_ascii_lowercase())) {
            continue;
        }
        let blend = m.flags & mesh_flags::TRANSLUCENT != 0;
        let page = if (sort.lightmap as usize) < pages.len() && sort.lightmap >= 0 { sort.lightmap } else { -1 };
        let b = batches.entry((sort.texture_data, blend, page)).or_insert_with(|| Batch {
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
    // Collision on this thread, then the meshes (tangent generation is most of the work) on
    // every core.
    let batches: Vec<((i16, bool, i16), std::sync::Mutex<Option<Batch>>)> = batches
        .into_iter()
        .map(|(k, b)| {
            log::debug!("batch {:?} flags {:#x} blend {} tris {}", texdata[k.0 as usize].name, texdata[k.0 as usize].flags, b.blend, b.idx.len() / 3);
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
        !blend && name.to_ascii_lowercase().ends_with("_bm") && std::env::var_os("TF_NO_BM").is_none()
    };
    let built = parallel_map(&batches, |((td, _, page), b)| {
        let b = b.lock().ok()?.take()?;
        let blend = b.blend;
        let mut mesh = convert::build_mesh(b.pos, b.nrm, b.uv, b.idx, None);
        if *page >= 0 {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, b.lm);
        }
        let effect_vcol = crate::uber::info(gd, &texdata[*td as usize].name).is_some_and(|i| i.flags & (crate::uber::VCOLT | crate::uber::VCOLA) != 0);
        if two_layer_names(*td, blend) || effect_vcol {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, b.col);
        }
        Some((mesh, blend))
    });
    for (((td, _, page), _), r) in batches.iter().zip(built) {
        let (td, page) = (*td, *page);
        let Some(Some((mesh, blend))) = r else { continue };
        let name = &texdata[td as usize].name;
        // Two-layer materials blend by vertex alpha in their own shader.
        let two_layer = if two_layer_names(td, blend) { convert::blend_material(gd, cache, images, materials, blends, name) } else { None };
        let mesh = meshes.add(mesh);
        let e = match two_layer {
            Some(m) => commands.spawn((Mesh3d(mesh), MeshMaterial3d(m), Transform::IDENTITY)).id(),
            None => {
                let mat = convert::material(gd, cache, images, materials, name, blend);
                commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY)).id()
            }
        };
        if page >= 0 {
            commands.entity(e).insert(bevy::pbr::Lightmap { image: pages[page as usize].clone(), uv_rect: Rect::new(0.0, 0.0, 1.0, 1.0), bicubic_sampling: false });
        }
        commands.entity(root).add_child(e);
    }
    log::info!("sky materials: {sky_names:?}");
    log::info!("world: {tris} triangles in {n} batches, built in {:?}", t.elapsed());
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
    for (l, &switch) in switches.iter().enumerate().take(MAX_PROP_LODS).chain((switches.is_empty()).then_some((0, &0.0))) {
        let mds = model.meshes_lod(&[], l)?;
        let n: usize = mds.iter().map(|m| m.indices.len() / 3).sum();
        if l > 0 && n * 10 >= last_tris * 9 {
            continue; // not meaningfully lighter than the previous LOD
        }
        last_tris = n;
        let mut parts = Vec::new();
        for md in mds {
            let tex = model.skin_families.first().and_then(|f| f.get(md.material)).copied().unwrap_or(0) as usize;
            let mat_name = model.textures.get(tex).cloned().unwrap_or_default();
            if l == 0 {
                for &i in &md.indices {
                    let p = md.positions[i as usize];
                    radius = radius.max(Vec3::from(p).length());
                    tris.push(p);
                }
            }
            parts.push((mat_name, convert::build_mesh(md.positions, md.normals, md.uvs, md.indices, None)));
        }
        lods.push((if l == 0 { 0.0 } else { switch }, parts));
    }
    Ok(PropModel { lods, tris, radius })
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
            draw_radii: std::env::var("TF_PROP_RADII").ok().and_then(|v| v.parse().ok()).unwrap_or(PROP_DRAW_RADII),
        }
    }
}

/// A static prop model ready to instance: LODs as (switch point, parts), its model-space
/// triangles for collision, and its bounding radius about the origin.
struct PropModel {
    lods: Vec<(f32, Vec<(String, Mesh)>)>,
    tris: Vec<[f32; 3]>,
    radius: f32,
}

/// Map `f` over `items` on every core, keeping the order (None where a worker panicked).
pub fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<Option<R>> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(items.len().max(1));
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
        workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
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
        let tex = model.skin_families.first().and_then(|f| f.get(md.material)).copied().unwrap_or(0) as usize;
        let mat_name = model.textures.get(tex).cloned().unwrap_or_default();
        let mat = convert::material(gd, cache, images, materials, &mat_name, false);
        for &i in &md.indices {
            tris.push(md.positions[i as usize]);
        }
        let mesh = meshes.add(convert::build_mesh(md.positions, md.normals, md.uvs, md.indices, None));
        out.push((mesh, mat));
    }
    Ok((out, tris))
}

/// Static props (game lump `sprp`). Each distinct model is converted once and instanced.
pub fn spawn_static_props(
    commands: &mut Commands,
    root: Entity,
    gd: &GameData,
    bsp: &Bsp,
    cache: &mut Cache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
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
    // Per model: LODs as (switch point, parts) and the bounding radius.
    let mut built: Vec<Option<(Vec<(f32, Vec<(Handle<Mesh>, Handle<StandardMaterial>)>)>, f32)>> = vec![None; sp.model_names.len()];
    // Model-space triangles of each prop model, for solid instances.
    let mut shapes: Vec<Vec<[f32; 3]>> = vec![Vec::new(); sp.model_names.len()];
    let mut failed = 0;
    // Parse the models and build their meshes (tangents included) on every core; materials
    // and asset handles are made on this thread afterwards.
    let parsed = parallel_map(&sp.model_names, |name| -> anyhow::Result<PropModel> {
        if name.starts_with("models/vistas/") {
            anyhow::bail!("vista (built by sky.rs)");
        }
        if std::env::var("TF_HIDE").is_ok_and(|h| !h.is_empty() && name.to_ascii_lowercase().contains(&h.to_ascii_lowercase())) {
            anyhow::bail!("hidden (TF_HIDE)");
        }
        // Godray models are soft additive light-shaft volumes; uber.rs draws them with their
        // shader's fades. without TF_UBER they are left out (as ordinary translucent meshes they become
        // hard walls of haze).
        if name.to_ascii_lowercase().contains("godray") && std::env::var_os("TF_UBER").is_none() {
            anyhow::bail!("godray volume (not drawn)");
        }
        static_model_data(gd, name)
    });
    for (mi, r) in parsed.into_iter().enumerate() {
        match r.unwrap_or_else(|| Err(anyhow::anyhow!("worker panicked"))) {
            Ok(pm) => {
                shapes[mi] = pm.tris;
                let lods = pm
                    .lods
                    .into_iter()
                    .map(|(switch, parts)| {
                        let parts = parts.into_iter().map(|(mat_name, mesh)| (meshes.add(mesh), convert::material(gd, cache, images, materials, &mat_name, false))).collect();
                        (switch, parts)
                    })
                    .collect();
                built[mi] = Some((lods, pm.radius));
            }
            Err(e) => {
                let n = sp.model_names[mi].to_ascii_lowercase();
                if !n.starts_with("models/vistas/") && !n.contains("godray") && std::env::var_os("TF_HIDE").is_none() {
                    failed += 1;
                }
                log::debug!("prop {}: {e}", sp.model_names[mi]);
            }
        }
    }
    let mut count = 0;
    let mut solid = 0;
    let mut parts_spawned = 0usize;
    let opts = PropOptions::from_env();
    for p in &sp.props {
        // Vistas belong to the 3D skybox (sky.rs).
        if sp.model_names[p.model as usize].starts_with("models/vistas/") {
            continue;
        }
        let Some(Some((lods, radius))) = built.get(p.model as usize) else { continue };
        let [pitch, yaw, roll] = p.angles.map(f32::to_radians);
        // Source angles: yaw about Z, then pitch about Y, then roll about X.
        let rot = Quat::from_rotation_z(yaw) * Quat::from_rotation_y(pitch) * Quat::from_rotation_x(roll);
        let tf = Transform { translation: Vec3::from(p.origin), rotation: rot, scale: Vec3::splat(p.scale.max(0.0001)) };
        if p.solid != 0 {
            let m = tf.to_matrix();
            for tri in shapes[p.model as usize].chunks_exact(3) {
                let v = |a: [f32; 3]| SVec3::from(m.transform_point3(Vec3::from(a)).to_array());
                collision.add(v(tri[0]), v(tri[1]), v(tri[2]));
            }
            solid += 1;
        }
        // One entity per mesh part and LOD, straight under the root (no per-prop parent), each
        // with the distance range it's drawn in (world units, i.e. after the root's scale).
        let r = radius * tf.scale.x;
        let draw_to = if !opts.cull || r >= PROP_ALWAYS_DRAWN { f32::INFINITY } else { (r * opts.draw_radii).max(600.0) };
        let lod_count = if opts.lod { lods.len() } else { 1 };
        for (l, (switch, parts)) in lods.iter().enumerate().take(lod_count) {
            let start = if l == 0 { 0.0 } else { switch * LOD_METRIC_TO_UNITS };
            let end = lods.get(l + 1).filter(|_| l + 1 < lod_count).map(|n| n.0 * LOD_METRIC_TO_UNITS).unwrap_or(f32::INFINITY).min(draw_to);
            if start >= end {
                continue;
            }
            let range = (start != 0.0 || end.is_finite()).then(|| {
                let (s, e) = (start * crate::player::UNIT, end * crate::player::UNIT);
                // Crossfade (dithered) over the last 10% before each switch.
                bevy::camera::visibility::VisibilityRange { start_margin: s * 0.9..s, end_margin: e * 0.9..e, use_aabb: false }
            });
            for (mesh, mat) in parts {
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
    log::info!(
        "static props: {count} instances ({solid} solid, {parts_spawned} mesh entities) of {} models ({failed} failed) in {:?}",
        sp.model_names.len(),
        t.elapsed()
    );
}
