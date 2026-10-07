//! Weapon screens: the RUIs a weapon script's `UiDataN` blocks draw on the viewmodel's RUI
//! meshes (screens and sight glass baked into the .mdl; see `tf_assets::mdl::RuiMesh`).
//!
//! The RUIs themselves are compiled UI programs we can't run, so only the ammo counters are
//! drawn, as a stand-in: the rounds left in the clip, white, centred on the part of the RUI
//! canvas the mesh's faces show. Each counter renders through a small UI camera into a
//! texture that the mesh shows (additive, unlit). Sights and reticles are not drawn.

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
            let m = model.rui_meshes.iter().find(|m| m.name.eq_ignore_ascii_case(mesh))?;
            let faces = m.faces.iter().filter_map(|f| Some((model.bones.get(f.bone)?.name.clone(), f.corners, f.uvs))).collect();
            Some(ScreenMesh { ui: ui.clone(), faces })
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
    let bounds = |uvs: &mut dyn Iterator<Item = Vec2>| uvs.fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(a, b), uv| (a.min(uv), b.max(uv)));
    let (lo, hi) = bounds(&mut screen.faces.iter().flat_map(|f| f.2.iter().map(|uv| Vec2::from(*uv))));
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
    shown: Option<u32>,
}

/// Build each gun's screens once its viewmodel's joints exist, and keep the counters' text on
/// the rounds in the clip.
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
    mut built: Local<HashMap<(Entity, usize), Built>>,
) {
    let Ok(loadout) = loadouts.single() else { return };
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
    for gun in &loadout.guns {
        let Some((_, vm)) = gun.viewmodel else { continue };
        let Ok(actor) = actors.get(vm) else { continue };
        for (i, screen) in gun.screens.iter().enumerate() {
            if !drawn(&screen.ui) || !shows_whole(screen) {
                continue;
            }
            let key = (vm, i);
            if !built.contains_key(&key) {
                let Some(b) = build(&mut commands, actor, screen, &ui.bold_font, &mut meshes, &mut images, &mut materials) else { continue };
                built.insert(key, b);
            }
            let Some(b) = built.get_mut(&key) else { continue };
            if b.shown != Some(gun.ammo) {
                if let Ok(mut t) = texts.get_mut(b.text) {
                    t.0 = gun.ammo.to_string();
                    b.shown = Some(gun.ammo);
                }
            }
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
    let (w, h) = (((span.x * 256.0) as u32).clamp(16, 512), ((span.y * 256.0) as u32).clamp(16, 512));
    let mut image = Image::new_fill(Extent3d { width: w, height: h, ..default() }, TextureDimension::D2, &[0, 0, 0, 0], TextureFormat::Bgra8UnormSrgb, default());
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image = images.add(image);
    let camera = commands
        .spawn((Camera2d, Camera { order: -20, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() }, RenderTarget::Image(image.clone().into())))
        .id();
    let ui_root = commands
        .spawn((
            Node { width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            UiTargetCamera(camera),
        ))
        .id();
    let text = commands
        .spawn((Text::new(""), TextFont { font: font.clone(), font_size: h as f32 * 0.7, ..default() }, TextColor(Color::srgb(0.85, 0.95, 1.0))))
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
    let m = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), Transform::IDENTITY, NotShadowCaster)).id();
    commands.entity(joint).add_child(m);
    log::debug!("weapon screen {} on {bone}: {} faces, {w}x{h}", screen.ui, screen.faces.len());
    Some(Built { camera, ui_root, text, shown: None })
}
