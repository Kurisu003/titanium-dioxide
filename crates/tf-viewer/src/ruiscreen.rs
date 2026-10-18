//! Weapon screens: the RUIs a weapon script's `UiDataN` blocks draw on the viewmodel's RUI
//! meshes (screens and sight glass baked into the .mdl; see `tf_assets::mdl::RuiMesh`).
//!
//! The RUIs themselves are compiled UI programs we can't run, so only the ammo counters are
//! drawn, as a stand-in: the rounds left in the clip, white, centred on the part of the RUI
//! canvas the mesh's faces show. Each counter renders through a small UI camera into a
//! texture that the mesh shows (additive, unlit). The R-201's factory sight (`ui/r101_sights`)
//! gets a built stand-in: the white-cyan holo frame on the rear sight glass with the ammo
//! digits inside it, and the red reticle on the front post, shown by the zoom like the game's
//! `vis player_zoomfrac`.

use bevy::camera::RenderTarget;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use std::collections::HashMap;

/// One screen's faces: the bone they ride and their corners (in its space) and UVs.
#[derive(Clone, Debug)]
pub struct ScreenMesh {
    pub ui: String,
    pub faces: Vec<(String, [[f32; 3]; 4], [[f32; 2]; 4])>,
}

/// The RUI meshes of a viewmodel that its weapon's enabled `UiData` blocks draw on.
pub fn screens_for(model: &tf_assets::mdl::Model, rui: &[(String, String)]) -> Vec<ScreenMesh> {
    rui.iter()
        .filter_map(|(ui, mesh)| {
            let m = model
                .rui_meshes
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(mesh))?;
            let faces = m
                .faces
                .iter()
                .filter_map(|f| Some((model.bones.get(f.bone)?.name.clone(), f.corners, f.uvs)))
                .collect();
            Some(ScreenMesh {
                ui: ui.clone(),
                faces,
            })
        })
        .collect()
}

/// Whether we draw this RUI (only the ammo counters for now).
fn drawn(ui: &str) -> bool {
    ui.contains("ammo_counter")
}

/// Whether every face shows (nearly) the whole canvas region the mesh covers: a ring of faces
/// (the SMR's) each shows a slice of the RUI's own layout, which centred digits would garble.
fn shows_whole(screen: &ScreenMesh) -> bool {
    let bounds = |uvs: &mut dyn Iterator<Item = Vec2>| {
        uvs.fold(
            (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
            |(a, b), uv| (a.min(uv), b.max(uv)),
        )
    };
    let (lo, hi) = bounds(
        &mut screen
            .faces
            .iter()
            .flat_map(|f| f.2.iter().map(|uv| Vec2::from(*uv))),
    );
    let span = (hi - lo).max(Vec2::splat(1e-3));
    screen.faces.iter().all(|f| {
        let (a, b) = bounds(&mut f.2.iter().map(|uv| Vec2::from(*uv)));
        ((b - a) / span).min_element() > 0.8
    })
}

pub struct Built {
    camera: Entity,
    ui_root: Entity,
    text: Entity,
    material: Handle<StandardMaterial>,
    shown: Option<u32>,
    /// Frames left to keep rendering the counter after its text last changed.
    hot: u32,
}

/// How many frames a counter's camera keeps rendering after its text changed (and until the
/// text has been laid out), before it stops: the texture keeps what it drew last.
const HOT_FRAMES: u32 = 30;

/// Build each gun's screens once its viewmodel's joints exist, and keep the counters' text on
/// the rounds in the clip. A counter's camera only renders for a while after its text changed:
/// the texture keeps the last picture, and drawing the same digits every frame cost two
/// cameras' worth of render graph, UI preparation and command buffers.
#[allow(clippy::too_many_arguments)]
pub fn weapon_screens(
    mut commands: Commands,
    loadouts: Query<&crate::pilotweapon::PilotLoadout>,
    actors: Query<&crate::actor::Actor>,
    ui: Option<Res<crate::ui::UiAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut texts: Query<&mut Text>,
    layouts: Query<&bevy::text::TextLayoutInfo>,
    mut cameras: Query<&mut Camera>,
    ui_scale: Res<UiScale>,
    mut built: Local<HashMap<(Entity, usize), Built>>,
    mut sights: Local<HashMap<(Entity, usize), (Handle<StandardMaterial>, f32)>>,
    mut aogs: Local<HashMap<(Entity, usize), Aog>>,
) {
    let Ok(loadout) = loadouts.single() else {
        return;
    };
    let Some(ui) = ui else { return };
    // Drop screens whose viewmodel is gone (the loadout was rebuilt).
    built.retain(|(vm, _), b| {
        let alive = actors.contains(*vm);
        if !alive {
            commands.entity(b.camera).despawn();
            commands.entity(b.ui_root).despawn();
        }
        alive
    });
    if std::env::var_os("TF_NO_WEAPON_SCREENS").is_some() {
        return;
    }
    sights.retain(|(vm, _), _| actors.contains(*vm));
    aogs.retain(|(vm, _), _| actors.contains(*vm));
    for gun in &loadout.guns {
        let Some((_, vm)) = gun.viewmodel else {
            continue;
        };
        let Ok(actor) = actors.get(vm) else { continue };
        for (i, screen) in gun.screens.iter().enumerate() {
            if is_aog(&screen.ui) {
                let key = (vm, i);
                if !aogs.contains_key(&key) {
                    match build_aog(&mut commands, actor, screen, &mut meshes, &mut materials) {
                        Some(a) => {
                            aogs.insert(key, a);
                        }
                        None => continue,
                    }
                }
                if let Some(a) = aogs.get_mut(&key) {
                    a.update(gun.ammo, gun.def.clip, gun.ads, &mut meshes, &mut materials);
                }
                continue;
            }
            if is_sight(&screen.ui) {
                let key = (vm, i);
                if !sights.contains_key(&key) {
                    if let Some((h, digits)) =
                        build_sight(&mut commands, actor, screen, &mut meshes, &mut materials)
                    {
                        if let Some(b) = build(
                            &mut commands,
                            actor,
                            &digits,
                            &ui.bold_font,
                            &mut meshes,
                            &mut images,
                            &mut materials,
                        ) {
                            built.insert((vm, i + 1000), b);
                        }
                        sights.insert(key, (h, -1.0));
                    }
                }
                if let Some((h, shown)) = sights.get_mut(&key) {
                    // `vis player_zoomfrac`: the hologram comes up with the zoom.
                    let a = (gun.ads * 100.0).round() / 100.0;
                    if *shown != a {
                        if let Some(m) = materials.get_mut(&*h) {
                            m.base_color = Color::srgba(1.0, 1.0, 1.0, a);
                        }
                        if let Some(b) = built.get(&(vm, i + 1000)) {
                            if let Some(m) = materials.get_mut(&b.material) {
                                m.base_color = Color::srgb(a, a, a);
                            }
                        }
                        *shown = a;
                    }
                }
                if let Some(b) = built.get_mut(&(vm, i + 1000)) {
                    tick_counter(b, gun.ammo, &mut texts, &layouts, &mut cameras, &ui_scale);
                }
                continue;
            }
            if !drawn(&screen.ui) || !shows_whole(screen) {
                continue;
            }
            let key = (vm, i);
            if !built.contains_key(&key) {
                let Some(b) = build(
                    &mut commands,
                    actor,
                    screen,
                    &ui.bold_font,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                ) else {
                    continue;
                };
                built.insert(key, b);
            }
            let Some(b) = built.get_mut(&key) else {
                continue;
            };
            tick_counter(b, gun.ammo, &mut texts, &layouts, &mut cameras, &ui_scale);
        }
    }
}

/// Keep a counter's text on `ammo`, rendering only while it changes.
fn tick_counter(
    b: &mut Built,
    ammo: u32,
    texts: &mut Query<&mut Text>,
    layouts: &Query<&bevy::text::TextLayoutInfo>,
    cameras: &mut Query<&mut Camera>,
    ui_scale: &Res<UiScale>,
) {
    if b.shown != Some(ammo) {
        if let Ok(mut t) = texts.get_mut(b.text) {
            t.0 = ammo.to_string();
            b.shown = Some(ammo);
            b.hot = HOT_FRAMES;
        }
    }
    let laid_out = b.shown.is_some() && layouts.get(b.text).is_ok_and(|l| !l.glyphs.is_empty());
    if ui_scale.is_changed() {
        b.hot = HOT_FRAMES;
    }
    let active = b.hot > 0 || !laid_out;
    b.hot = b.hot.saturating_sub(1);
    if let Ok(mut cam) = cameras.get_mut(b.camera) {
        if cam.is_active != active {
            cam.is_active = active;
        }
    }
}

fn build(
    commands: &mut Commands,
    actor: &crate::actor::Actor,
    screen: &ScreenMesh,
    font: &Handle<Font>,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Option<Built> {
    let bone = &screen.faces.first()?.0;
    let joint = actor.joint(bone)?;
    // The part of the 256-unit RUI canvas the faces show.
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for (_, _, uvs) in &screen.faces {
        for uv in uvs {
            lo = lo.min(Vec2::from(*uv));
            hi = hi.max(Vec2::from(*uv));
        }
    }
    let span = (hi - lo).max(Vec2::splat(1e-3));
    let (w, h) = (
        ((span.x * 256.0) as u32).clamp(16, 512),
        ((span.y * 256.0) as u32).clamp(16, 512),
    );
    let mut image = Image::new_fill(
        Extent3d {
            width: w,
            height: h,
            ..default()
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Bgra8UnormSrgb,
        default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image = images.add(image);
    // UI only: no render layers (see ui::spawn_loading_screen).
    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: -20,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            bevy::camera::visibility::RenderLayers::none(),
        ))
        .id();
    let ui_root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            UiTargetCamera(camera),
        ))
        .id();
    let text = commands
        .spawn((
            Text::new(""),
            TextFont {
                font: font.clone(),
                font_size: h as f32 * 0.7,
                ..default()
            },
            TextColor(Color::srgb(0.85, 0.95, 1.0)),
        ))
        .id();
    commands.entity(ui_root).add_child(text);
    // The faces, in the bone's space, UVs remapped onto the texture.
    let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for (_, corners, uvs) in screen.faces.iter().filter(|f| &f.0 == bone) {
        let base = pos.len() as u32;
        for k in 0..4 {
            pos.push(corners[k]);
            uv.push(((Vec2::from(uvs[k]) - lo) / span).to_array());
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, default());
    let n = pos.len();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_indices(bevy::mesh::Indices::U32(idx));
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(image),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let m = commands
        .spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(mat.clone()),
            Transform::IDENTITY,
            NotShadowCaster,
        ))
        .id();
    commands.entity(joint).add_child(m);
    log::debug!(
        "weapon screen {} on {bone}: {} faces, {w}x{h}",
        screen.ui,
        screen.faces.len()
    );
    Some(Built {
        camera,
        ui_root,
        text,
        material: mat,
        shown: None,
        hot: HOT_FRAMES,
    })
}

/// The sight RUIs we stand in for.
fn is_sight(ui: &str) -> bool {
    ui == "ui/r101_sights"
}

/// The AOG's sight (`ui/aog_multi_reticles`, the R-101's factory scope and an optic of the
/// Spitfire, Devotion, L-STAR and Hemlok), built on its RUI mesh's two planes: a thick ammo
/// ring that shortens toward the bottom as the clip empties (cyan, orange
/// when low), and two red dashes either side of the aim, both on the front plane. Measured on a
/// 1080p frame of the game (the ring 455 px across with a 90 degree gap at the top when full,
/// 13 px thick; the dashes from 0.40 to 0.76 of the ring's radius).
fn is_aog(ui: &str) -> bool {
    ui == "ui/aog_multi_reticles" || ui == "ui/aog_multi_reticles_lstar"
}

pub struct Aog {
    ring: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    /// The rear plane's centre (bone space) and the ring's radius.
    centre: Vec3,
    radius: f32,
    shown: (u32, f32),
}

/// Ammo left under this fraction of the clip turns the ring orange.
const AOG_LOW: f32 = 0.35;

impl Aog {
    fn update(
        &mut self,
        ammo: u32,
        clip: u32,
        ads: f32,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) {
        let a = (ads * 100.0).round() / 100.0;
        if self.shown.1 != a {
            if let Some(m) = materials.get_mut(&self.material) {
                m.base_color = Color::srgba(1.0, 1.0, 1.0, a);
            }
        }
        if self.shown.0 != ammo || self.shown.1 < 0.0 {
            let frac = ammo as f32 / clip.max(1) as f32;
            let colour = if frac < AOG_LOW {
                [3.0, 1.1, 0.25, 1.0]
            } else {
                [0.12, 1.25, 2.1, 1.0]
            };
            // Full: 270 degrees, from the upper left round the bottom to the upper right.
            let half = 135f32.to_radians() * frac;
            let (mut pos, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new());
            let steps = 48;
            let w = self.radius * 0.075;
            let bottom = -std::f32::consts::FRAC_PI_2;
            for k in 0..=steps {
                let t = bottom - half + 2.0 * half * k as f32 / steps as f32;
                let d = Vec2::new(t.cos(), t.sin());
                for r in [self.radius - w * 0.5, self.radius + w * 0.5] {
                    let p = self.centre.truncate() + d * r;
                    pos.push([p.x, p.y, self.centre.z]);
                    col.push(colour);
                }
            }
            if frac > 0.0 {
                for k in 0..steps as u32 {
                    let b = k * 2;
                    idx.extend([b, b + 2, b + 1, b + 1, b + 2, b + 3]);
                }
            }
            if let Some(m) = meshes.get_mut(&self.ring) {
                let n = pos.len();
                m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
                m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n]);
                m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
                m.insert_indices(bevy::mesh::Indices::U32(idx));
            }
        }
        self.shown = (ammo, a);
    }
}

fn build_aog(
    commands: &mut Commands,
    actor: &crate::actor::Actor,
    screen: &ScreenMesh,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Option<Aog> {
    let bone = &screen.faces.first()?.0;
    let joint = actor.joint(bone)?;
    // The two planes (by z): their centres and half sizes.
    let mut planes: Vec<(f32, Vec2, Vec2)> = Vec::new();
    for f in &screen.faces {
        let z = f.1[0][2];
        let (lo, hi) = f.1.iter().fold(
            (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
            |(lo, hi), c| (lo.min(Vec2::new(c[0], c[1])), hi.max(Vec2::new(c[0], c[1]))),
        );
        match planes.iter_mut().find(|p| (p.0 - z).abs() < 0.01) {
            Some(p) => {
                p.1 = p.1.min(lo);
                p.2 = p.2.max(hi);
            }
            None => planes.push((z, lo, hi)),
        }
    }
    planes.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Both on the front plane (the rear one sits too near the eye to draw); the ring fills
    // 0.945 of it, which puts it just inside the scope's aperture as in the footage.
    let (fz, flo, fhi) = *planes.last()?;
    let front_c = (flo + fhi) * 0.5;
    let radius = (fhi.x - flo.x) * 0.5 * 0.945;
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    // The red dashes at the aim height, in the ring's radii.
    let k = radius;
    let (mut pos, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    let red = [2.6, 0.2, 0.25, 1.0];
    for side in [-1.0f32, 1.0] {
        strip(
            front_c + Vec2::new(side * 0.40 * k, 0.0),
            front_c + Vec2::new(side * 0.76 * k, 0.0),
            k * 0.018,
            fz,
            red,
            &mut pos,
            &mut col,
            &mut idx,
        );
    }
    let n = pos.len();
    let mut dashes = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, default());
    dashes.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    dashes.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n]);
    dashes.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    dashes.insert_indices(bevy::mesh::Indices::U32(idx));
    let mut ring = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, default());
    ring.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
    ring.insert_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new());
    ring.insert_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new());
    ring.insert_indices(bevy::mesh::Indices::U32(Vec::new()));
    let ring = meshes.add(ring);
    for mesh in [meshes.add(dashes), ring.clone()] {
        let e = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                NotShadowCaster,
                bevy::camera::visibility::NoFrustumCulling,
            ))
            .id();
        commands.entity(joint).add_child(e);
    }
    log::debug!(
        "weapon sight {} on {bone}: AOG ring r {radius:.2}",
        screen.ui
    );
    Some(Aog {
        ring,
        material,
        centre: front_c.extend(fz),
        radius,
        shown: (u32::MAX, -1.0),
    })
}

/// Push a quad of width `w` along a-b (a bone-space plane at `z`), with a vertex colour.
fn strip(
    a: Vec2,
    b: Vec2,
    w: f32,
    z: f32,
    c: [f32; 4],
    pos: &mut Vec<[f32; 3]>,
    col: &mut Vec<[f32; 4]>,
    idx: &mut Vec<u32>,
) {
    let d = (b - a).normalize_or_zero();
    let n = Vec2::new(-d.y, d.x) * (w * 0.5);
    // Extend the ends by half the width so joints close.
    let (a, b) = (a - d * w * 0.5, b + d * w * 0.5);
    let base = pos.len() as u32;
    for p in [a + n, b + n, b - n, a - n] {
        pos.push([p.x, p.y, z]);
        col.push(c);
    }
    idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// The R-201's factory sight, built as geometry on its RUI mesh's planes (bone space; on
/// screen y grows downward and x to the right): the white-cyan holo frame on the rear glass
/// face (open top, straight sides, lower corners cut at 45 degrees, a shallow chevron dipping
/// at the bottom centre) and the red reticle on the front post faces (a bar with a gap in the
/// middle, short ticks at the gap, a dotted stem below). Shapes from a 1080p frame of the
/// game. The ammo digits inside the frame are a counter screen on the glass (see the caller).
fn build_sight(
    commands: &mut Commands,
    actor: &crate::actor::Actor,
    screen: &ScreenMesh,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Option<(Handle<StandardMaterial>, ScreenMesh)> {
    let bone = &screen.faces.first()?.0;
    let joint = actor.joint(bone)?;
    // Each face's plane: z, x range, y range.
    let extent = |f: &(String, [[f32; 3]; 4], [[f32; 2]; 4])| {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for c in &f.1 {
            lo = lo.min(Vec3::from(*c));
            hi = hi.max(Vec3::from(*c));
        }
        (lo, hi)
    };
    // Planes by distance: the glass (nearest), then the reticle plane; the far pair at the
    // front post is left alone.
    let mut faces: Vec<(Vec3, Vec3)> = screen.faces.iter().map(extent).collect();
    faces.sort_by(|a, b| a.0.z.total_cmp(&b.0.z));
    let (glo, ghi) = faces[0];
    let (plo, phi) = *faces.get(1)?;
    let (mut pos, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    // Frame: u 0..1 left to right, v 0..1 top to bottom across the glass (bone y is up).
    // Measured on a 1080p frame (the glass 572 px wide, centred on the aim): sides at u
    // 0.08/0.92 from v 0.40 to 0.70, 45 degree cuts to v 0.79, a chevron dipping to v 0.82,
    // lines 1.4% of the glass thick.
    let gw = ghi.x - glo.x;
    let gh = ghi.y - glo.y;
    let g = |u: f32, v: f32| Vec2::new(glo.x + u * gw, ghi.y - v * gh);
    // HDR colours (bright enough to bloom like the game's hologram).
    let frame = [2.2, 2.8, 3.0, 1.0];
    let path = [
        (0.078, 0.395),
        (0.078, 0.705),
        (0.20, 0.788),
        (0.314, 0.788),
        (0.50, 0.82),
        (0.686, 0.788),
        (0.80, 0.788),
        (0.922, 0.705),
        (0.922, 0.395),
    ];
    for k in 0..path.len() - 1 {
        let (a, b) = (path[k], path[k + 1]);
        strip(
            g(a.0, a.1),
            g(b.0, b.1),
            gw * 0.014,
            glo.z,
            frame,
            &mut pos,
            &mut col,
            &mut idx,
        );
    }
    // Reticle on the mid plane (centred on the aim, 45% of the glass's height on screen): a
    // bar across the aim line, 55% of the plane wide with an 8% gap, 10% ticks hanging from
    // the gap, a dotted stem 28% long below; lines 2% of the plane thick.
    {
        let pw = phi.x - plo.x;
        let ph = phi.y - plo.y;
        let q = |u: f32, v: f32| Vec2::new(plo.x + u * pw, phi.y - v * ph);
        let red = [2.4, 0.25, 0.5, 1.0];
        let t = pw * 0.02;
        let bar = 0.5;
        for (u0, u1) in [(0.23, 0.46), (0.54, 0.77)] {
            strip(
                q(u0, bar),
                q(u1, bar),
                t,
                plo.z,
                red,
                &mut pos,
                &mut col,
                &mut idx,
            );
        }
        for u in [0.46, 0.54] {
            strip(
                q(u, bar),
                q(u, bar + 0.10),
                t,
                plo.z,
                red,
                &mut pos,
                &mut col,
                &mut idx,
            );
        }
        let mut v = bar + 0.13;
        while v < bar + 0.28 {
            strip(
                q(0.5, v),
                q(0.5, v + 0.04),
                t,
                plo.z,
                red,
                &mut pos,
                &mut col,
                &mut idx,
            );
            v += 0.08;
        }
    }
    let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, default());
    let n = pos.len();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(bevy::mesh::Indices::U32(idx));
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let m = commands
        .spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(mat.clone()),
            Transform::IDENTITY,
            NotShadowCaster,
        ))
        .id();
    commands.entity(joint).add_child(m);
    // The digits: a small quad on the glass just above the chevron (v 0.66-0.71 of the
    // glass in the footage), the texture's top row at its top.
    let (x, top, bot, z) = (
        gw * 0.07,
        ghi.y - gh * 0.655,
        ghi.y - gh * 0.715,
        glo.z + 0.005,
    );
    let digits = ScreenMesh {
        ui: format!("{}#ammo", screen.ui),
        faces: vec![(
            bone.clone(),
            // Bone +x is screen left; the texture's row 0 is the top of the quad.
            [[-x, top, z], [x, top, z], [x, bot, z], [-x, bot, z]],
            [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        )],
    };
    log::debug!(
        "weapon sight {} on {bone}: {} faces",
        screen.ui,
        screen.faces.len()
    );
    Some((mat, digits))
}
