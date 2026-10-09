//! The Archer's sight: the `vgui_pilot_launcher_screen` VGUI screen that client.dll draws on the
//! launcher's flip-up sight. The weapon script has no `UiData` for it: client.dll's smart ammo HUD
//! gives `mp_weapon_rocket_launcher` the `rocket_launcher` HUD type (0x1804dc580 maps the class
//! name when `smart_ammo_hud_type` is unset), whose per-frame update (0x1804dfe80) drives this
//! screen and nothing else on screen. The screen spans the viewmodel's SCR_BL to SCR_TR
//! attachments on `def_c_sightRear_screen` (CreateVGuiScreen at 0x1804dcff1), its layout is
//! `scripts/screens/vgui_pilot_launcher.res` on a 1403x998 panel (vgui_screens.txt) and its art
//! the `vgui\hud\sram\*` materials of common.rpak.
//!
//! What the update does, each frame while zoomed (all from 0x1804dfe80):
//! - The lock: the first smart ammo target with a lock fraction above 0. `lockVis` = fraction / 0.9,
//!   clamped to 0..1.
//! - A tracked point eases toward the target's lock point (relative to the target, so it keeps up
//!   with a moving Titan), or with no target toward 500 units straight ahead in view space, through
//!   a critically damped spring (SmoothCD, omega 8). The ray from the eye to it is cut with the
//!   panel's plane, which gives the panel pixel the targeting circle and the two rings centre on.
//! - The rings are 337 px images sized 3 - 2 x lockVis (so they close from three times their size
//!   onto the target), and fade in over the first 0.2 of lockVis.
//! - No lock: "NO TARGET IN RANGE" (or "OFFLINE" with no rockets), the tickers along the top, the
//!   circle and the rocket icons in blue-white (177 213 227).
//! - Locking: "LOCKING", the target line "SCANNING..." and a random range each frame, in orange
//!   (238 146 8); the tickers are hidden. The rings spin (sram_targeting_inner/outer_ring.vmt
//!   LinearRamp: -375 and 325 degrees per second; -250 and -200 for the `_reverse_rotate` pair
//!   used while the target is out of sight).
//! - Locked: "LOCKED", the target's AI title ("%s1: %s2", the team title empty here) and range
//!   ("%04dm", 0.0254 m per unit), tickers and circle red (205 72 59), still rings (`_static`);
//!   rings, text and rocket icons blink red for 0.15 s and white (225 225 225) for 0.4 s.
//! - Everything fades in over the last 35% of the zoom (alpha = (zoomFrac - 0.65) / 0.35).
//! - Rocket icons: one per rocket in the clip and stockpile, at most six.
//!
//! The panel renders through a small UI camera into a texture shown on a quad between the two
//! attachments, additive and unlit like the other weapon screens (ruiscreen.rs).

use crate::pilotctl::{Control, PlayerPilot};
use crate::targets::Enemy;
use bevy::camera::RenderTarget;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};

/// The panel's size in its own pixels (vgui_screens.txt `vgui_pilot_launcher_screen`).
const PANEL: Vec2 = Vec2::new(1403.0, 998.0);
/// The texture it renders into (half the panel's pixels).
const TEX: (u32, u32) = (702, 499);

/// Colours set by the update (vgui Color bytes from the constants at 0x1804e0e0d..0x1804e0e52).
const IDLE: [u8; 3] = [177, 213, 227];
const LOCKING: [u8; 3] = [238, 146, 8];
const LOCKED: [u8; 3] = [205, 72, 59];
const BLINK: [u8; 3] = [225, 225, 225];
const NO_AMMO: [u8; 3] = [200, 50, 50];

/// The screen in its bone's space: the bottom-left corner (SCR_BL), the panel's right and up axes
/// (SCR_BL's X and Y, as the CreateVGuiScreen call measures the width and height along them) and
/// its size to SCR_TR.
#[derive(Clone, Debug)]
pub struct Panel {
    pub bone: String,
    bl: Vec3,
    right: Vec3,
    up: Vec3,
    size: Vec2,
}

/// The sight screen of a viewmodel that has one (the SCR_BL and SCR_TR attachments).
pub fn panel_for(model: &tf_assets::mdl::Model) -> Option<Panel> {
    let (bone, bl) = crate::pilotweapon::model_attachment(model, "SCR_BL")?;
    let (bone_tr, tr) = crate::pilotweapon::model_attachment(model, "SCR_TR")?;
    if bone != bone_tr {
        return None;
    }
    let (right, up) = (bl.rotation * Vec3::X, bl.rotation * Vec3::Y);
    let d = tr.translation - bl.translation;
    let size = Vec2::new(d.dot(right), d.dot(up));
    (size.min_element() > 0.0).then_some(Panel { bone, bl: bl.translation, right, up, size })
}

/// Critically damped spring (Game Programming Gems 4 SmoothCD), as client.dll inlines it for the
/// tracked point: x = omega dt, exp ~ 1 / (1 + x + 0.48 x^2 + 0.235 x^3).
fn smooth_cd(from: Vec3, vel: &mut Vec3, to: Vec3, dt: f32, omega: f32) -> Vec3 {
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = from - to;
    let temp = (*vel + change * omega) * dt;
    *vel = (*vel - temp * omega) * exp;
    to + (change + temp) * exp
}

/// Where the eye's ray through `dir` (camera space) cuts the panel, in panel pixels (y down),
/// given the panel's corner and unit-size axes in camera space. Clamped to +-32000 like the game.
fn panel_pixel(dir: Vec3, bl: Vec3, right: Vec3, up: Vec3, size: Vec2) -> Vec2 {
    let n = right.cross(up);
    let denom = dir.dot(n);
    if denom.abs() < 1e-9 {
        return Vec2::splat(-32000.0);
    }
    let t = bl.dot(n) / denom;
    if t <= 0.0 {
        return Vec2::splat(-32000.0);
    }
    let rel = dir * t - bl;
    let u = rel.dot(right) / right.length_squared() / size.x;
    let v = rel.dot(up) / up.length_squared() / size.y;
    Vec2::new(u * PANEL.x, (1.0 - v) * PANEL.y).clamp(Vec2::splat(-32000.0), Vec2::splat(32000.0))
}

fn color(c: [u8; 3], alpha: f32) -> Color {
    Color::srgba_u8(c[0], c[1], c[2], (alpha.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Panel pixels as a percentage of its width / height.
fn pct_x(x: f32) -> Val {
    Val::Percent(x / PANEL.x * 100.0)
}
fn pct_y(y: f32) -> Val {
    Val::Percent(y / PANEL.y * 100.0)
}

/// A `vgui\hud\sram` image: its material's colour texture, decoded with alpha from brightness so
/// that the additive art (black backgrounds) can overlap in the panel texture.
fn vgui_image(gd: &crate::gamedata::GameData, images: &mut Assets<Image>, material: &str) -> Option<Handle<Image>> {
    let (pi, mat) = gd.material(material)?;
    let (pi, ai) = gd.find_texture(pi, *mat.textures.first()?)?;
    let pak = &gd.paks[pi];
    let tex = tf_assets::texture::load_max(pak, &pak.assets[ai], 512).ok()?;
    let (w, h) = (tex.width as usize, tex.height as usize);
    let rgb = match tex.info.format {
        tf_assets::texture::TexFormat::Bc1 | tf_assets::texture::TexFormat::Bc1Srgb => tf_assets::texture::decode_bc1(&tex.data, w, h),
        f => {
            log::warn!("{material}: texture format {f:?} not decoded");
            return None;
        }
    };
    let mut data = Vec::with_capacity(w * h * 4);
    for p in rgb {
        let a = p[0].max(p[1]).max(p[2]);
        let k = if a > 0 { 255.0 / a as f32 } else { 0.0 };
        data.extend_from_slice(&[(p[0] as f32 * k) as u8, (p[1] as f32 * k) as u8, (p[2] as f32 * k) as u8, a]);
    }
    let img = Image::new(Extent3d { width: w as u32, height: h as u32, ..default() }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, default());
    Some(images.add(img))
}

struct Built {
    vm: Entity,
    camera: Entity,
    root: Entity,
    quad: Entity,
    tickers: [Entity; 2],
    icons: [Entity; 6],
    ammo_label: Entity,
    lock_status: Entity,
    target_name: Entity,
    range: Entity,
    circle: Entity,
    /// Inner, outer.
    rings: [Entity; 2],
}

/// The screen and the update's state (client.dll keeps it in statics at 0x18290229c..0x1829022c0).
#[derive(Default)]
pub struct Sight {
    built: Option<Built>,
    /// Tracked point (Bevy space) and its spring velocity.
    point: Option<Vec3>,
    vel: Vec3,
    /// Locked blink: seconds left in the current colour, and whether it is red.
    blink_t: f32,
    blink_red: bool,
    rng: u32,
}

type Parts<'a> = (&'a mut Node, &'a mut Visibility, Option<&'a mut ImageNode>, Option<&'a mut TextColor>, Option<&'a mut Text>, Option<&'a mut UiTransform>);

#[allow(clippy::too_many_arguments)]
fn build(
    commands: &mut Commands,
    gd: &crate::gamedata::GameData,
    ui: &crate::ui::UiAssets,
    actor: &crate::actor::Actor,
    vm: Entity,
    panel: &Panel,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Option<Built> {
    let joint = actor.joint(&panel.bone)?;
    let mut target = Image::new_fill(Extent3d { width: TEX.0, height: TEX.1, ..default() }, TextureDimension::D2, &[0, 0, 0, 0], TextureFormat::Bgra8UnormSrgb, default());
    target.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let target = images.add(target);
    let camera = commands
        .spawn((Camera2d, Camera { order: -21, clear_color: ClearColorConfig::Custom(Color::NONE), is_active: false, ..default() }, RenderTarget::Image(target.clone().into())))
        .id();
    let root = commands.spawn((Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, UiTargetCamera(camera))).id();
    let px = TEX.1 as f32 / PANEL.y;
    let mut image = |path: &str, (x, y, w, h): (f32, f32, f32, f32)| {
        let handle = vgui_image(gd, images, path);
        if handle.is_none() {
            log::warn!("pilot launcher screen: no image {path}");
        }
        let node = Node { position_type: PositionType::Absolute, left: pct_x(x), top: pct_y(y), width: pct_x(w), height: pct_y(h), ..default() };
        let e = commands.spawn((node, ImageNode::new(handle.unwrap_or_default()), UiTransform::IDENTITY, Visibility::Hidden)).id();
        commands.entity(root).add_child(e);
        e
    };
    // vgui_pilot_launcher.res, in its zpos order.
    let icons = std::array::from_fn(|i| image("vgui\\hud\\sram\\sram_rocket_icon", (320.0 - 54.0 * i as f32, 800.0, 72.0, 144.0)));
    let tickers = [image("vgui\\hud\\sram\\sram_ticker", (45.0, 0.0, 899.0, 112.0)), image("vgui\\hud\\sram\\sram_ticker", (944.0, 0.0, 899.0, 112.0))];
    let circle = image("vgui\\hud\\sram\\sram_targeting_circle", (697.0, 483.0, 90.0, 79.0));
    let rings = [
        image("vgui\\hud\\sram\\sram_targeting_inner_ring_static", (0.0, 0.0, 337.0, 337.0)),
        image("vgui\\hud\\sram\\sram_targeting_outer_ring_static", (0.0, 0.0, 337.0, 337.0)),
    ];
    // Labels: Default_55_Responsive / Default_69_Responsive (font height in panel pixels).
    let mut label = |(x, y, w, h): (f32, f32, f32, f32), font: f32, centred: bool| {
        let node = Node {
            position_type: PositionType::Absolute,
            left: pct_x(x),
            top: pct_y(y),
            width: pct_x(w),
            height: pct_y(h),
            justify_content: if centred { JustifyContent::Center } else { JustifyContent::FlexStart },
            align_items: AlignItems::Center,
            overflow: Overflow::visible(),
            ..default()
        };
        let text = commands
            .spawn((Text::new(""), TextFont { font: ui.font.clone(), font_size: font * px, ..default() }, TextColor(Color::WHITE), TextLayout::new_with_no_wrap(), Node::default(), Visibility::Hidden))
            .id();
        let e = commands.spawn(node).id();
        commands.entity(e).add_child(text);
        commands.entity(root).add_child(e);
        text
    };
    let target_name = label((100.0, 101.0, 562.0, 49.0), 55.0, false);
    let range = label((104.0, 160.0, 562.0, 49.0), 55.0, false);
    let lock_status = label((247.0, 850.0, 899.0, 90.0), 69.0, true);
    let ammo_label = label((0.0, 924.0, 315.0, 56.0), 69.0, true);

    // The quad from SCR_BL to SCR_TR, in the bone's space; texture rows run top down.
    let (bl, r, u) = (panel.bl, panel.right * panel.size.x, panel.up * panel.size.y);
    let pos = vec![bl.to_array(), (bl + r).to_array(), (bl + r + u).to_array(), (bl + u).to_array()];
    let n = r.cross(u).normalize_or_zero().to_array();
    let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![n; 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
    mesh.insert_indices(bevy::mesh::Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(target),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let quad = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), Transform::IDENTITY, NotShadowCaster)).id();
    commands.entity(joint).add_child(quad);
    log::debug!("pilot launcher screen on {}: {:?} x {:?}", panel.bone, panel.size, panel.right);
    Some(Built { vm, camera, root, quad, tickers, icons, ammo_label, lock_status, target_name, range, circle, rings })
}

/// Build the active launcher's sight screen and run client.dll's update on it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    control: Res<Control>,
    (gd, ui, strings, kits): (Option<Res<crate::gamedata::GameData>>, Option<Res<crate::ui::UiAssets>>, Res<crate::pilotweapon::Strings>, Option<Res<crate::targets::EnemyKits>>),
    loadouts: Query<&crate::pilotweapon::PilotLoadout, With<PlayerPilot>>,
    actors: Query<&crate::actor::Actor>,
    globals: Query<&GlobalTransform>,
    cameras: Query<(&Transform, &GlobalTransform, &Projection), With<crate::player::MainCamera>>,
    enemies: Query<&Enemy>,
    mut cams: Query<&mut Camera>,
    mut parts: Query<Parts, Without<Camera>>,
    mut quads: Query<&mut Visibility, (With<Mesh3d>, Without<Node>)>,
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>),
    mut sight: Local<Sight>,
) {
    let (Some(gd), Some(ui)) = (gd, ui) else { return };
    let gun = loadouts.single().ok().and_then(|l| l.active()).filter(|_| *control == Control::Pilot);
    let screen = gun.and_then(|g| Some((g, g.sight.as_ref()?, g.viewmodel?.1)));
    // Drop a screen whose viewmodel is gone (the loadout was rebuilt) or that belongs to another
    // launcher's viewmodel.
    if let Some(b) = sight.built.take_if(|b| !actors.contains(b.vm) || screen.is_some_and(|s| s.2 != b.vm)) {
        commands.entity(b.camera).despawn();
        commands.entity(b.root).despawn();
        if let Ok(mut q) = commands.get_entity(b.quad) {
            q.despawn();
        }
    }
    let set_active = |cams: &mut Query<&mut Camera>, b: &Option<Built>, on: bool| {
        if let Some(b) = b {
            if let Ok(mut c) = cams.get_mut(b.camera) {
                if c.is_active != on {
                    c.is_active = on;
                }
            }
        }
    };
    let Some((gun, panel, vm)) = screen else {
        set_active(&mut cams, &sight.built, false);
        return;
    };
    if sight.built.is_none() {
        let Ok(actor) = actors.get(vm) else { return };
        sight.built = build(&mut commands, &gd, &ui, actor, vm, panel, &mut meshes, &mut images, &mut materials);
        return;
    }
    let Ok((cam_t, cam_g, projection)) = cameras.single() else { return };
    // Everything fades in over the last 35% of the zoom; at no zoom the game skips the update.
    let alpha = ((gun.ads - 0.65) / 0.35).clamp(0.0, 1.0);
    set_active(&mut cams, &sight.built, alpha > 0.0);
    // The texture keeps its last picture while the camera is off, so the quad hides with it.
    if let Some(mut v) = sight.built.as_ref().and_then(|b| quads.get_mut(b.quad).ok()) {
        let want = if alpha > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if gun.ads <= 0.0 {
        sight.point = None;
        sight.vel = Vec3::ZERO;
        return;
    }
    let dt = time.delta_secs();
    let s = &mut *sight;
    let b = s.built.as_ref().unwrap();

    // The lock: the first target with a lock forming; the update tracks it while it is in sight.
    let lock = gun.smart.targets.iter().find(|l| l.frac > 0.0);
    let lock_frac = lock.map(|l| l.frac.min(1.0)).unwrap_or(0.0);
    let target = lock.filter(|l| l.lost == 0.0).and_then(|l| enemies.get(l.target).ok()).filter(|e| e.alive());
    let lock_vis = (lock_frac / 0.9).clamp(0.0, 1.0);

    // The tracked point: toward the lock point relative to the target, else 500 units ahead.
    let cam_m = cam_t.compute_affine();
    let cam_inv = cam_m.inverse();
    let ahead = cam_m.transform_point3(Vec3::NEG_Z * 500.0 * crate::player::UNIT);
    let point = s.point.unwrap_or(ahead);
    s.point = Some(match target {
        Some(e) => {
            let origin = crate::player::to_bevy(e.pos);
            origin + smooth_cd(point - origin, &mut s.vel, crate::player::to_bevy(crate::smartammo::chest(e)) - origin, dt, 8.0)
        }
        None => {
            let rel = cam_inv.transform_point3(point);
            cam_m.transform_point3(smooth_cd(rel, &mut s.vel, Vec3::NEG_Z * 500.0 * crate::player::UNIT, dt, 8.0))
        }
    });

    // The panel in camera space (last frame's joint and camera, a matching pair) and the tracked
    // point's direction through the viewmodel's lens: the ring sits over the target as seen
    // (the game cuts the world-space ray; with the world zoomed past the viewmodel FOV that
    // would pull the rings toward the middle, so the direction is rescaled to the viewmodel FOV).
    let Some(joint) = actors.get(vm).ok().and_then(|a| a.joint(&panel.bone)) else { return };
    let Ok(jg) = globals.get(joint) else { return };
    let cam_g_inv = cam_g.affine().inverse();
    let to_cam = |p: Vec3| cam_g_inv.transform_point3(jg.transform_point(p));
    let bl = to_cam(panel.bl);
    let (right, up) = (to_cam(panel.bl + panel.right) - bl, to_cam(panel.bl + panel.up) - bl);
    let world_fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => crate::vmcam::vertical_from_43(crate::vmcam::PILOT_VM_FOV),
    };
    let k = (crate::vmcam::vertical_from_43(crate::vmcam::PILOT_VM_FOV) * 0.5).tan() / (world_fov * 0.5).tan();
    let mut dir = cam_inv.transform_point3(s.point.unwrap_or(ahead));
    dir.x *= k;
    dir.y *= k;
    let at = panel_pixel(dir, bl, right, up, panel.size);

    // Rockets: the clip and the stockpile, at most six. This game keeps no stockpile (reserve
    // ammo is unlimited), so all six show.
    let rockets = 6usize;
    let text_of = |key: &str| strings.0.get(key).cloned().unwrap_or_default();

    // Every element starts hidden each update; the branches show what they use.
    let mut tint: Vec<(Entity, [u8; 3])> = Vec::new();
    let mut texts: Vec<(Entity, String)> = Vec::new();
    let mut shown: Vec<Entity> = vec![b.lock_status, b.circle];
    shown.extend(&b.icons[..rockets.min(6)]);
    if rockets == 0 {
        texts.push((b.ammo_label, text_of("HUD_NO_AMMO")));
        tint.push((b.ammo_label, NO_AMMO));
        shown.push(b.ammo_label);
    }
    shown.extend(&b.rings);
    // Ring art and spin.
    let spin = time.elapsed_secs();
    let ring_rot = if lock_vis >= 1.0 {
        [0.0, 0.0]
    } else if target.is_some() {
        [-375.0 * spin, 325.0 * spin]
    } else {
        [-250.0 * spin, -200.0 * spin]
    };
    if lock_vis <= 0.0 {
        s.blink_t = 0.0;
        s.blink_red = false;
        texts.push((b.lock_status, text_of(if rockets > 0 { "HUD_NO_TARGET_IN_RANGE" } else { "HUD_OFFLINE" })));
        // The rings keep their colour, at alpha 0.
        for e in [b.lock_status, b.tickers[0], b.tickers[1], b.circle, b.rings[0], b.rings[1]].into_iter().chain(b.icons) {
            tint.push((e, IDLE));
        }
        shown.extend(b.tickers);
    } else if lock_vis >= 1.0 {
        let name = lock.and_then(|l| enemies.get(l.target).ok()).map(|e| title(e, kits.as_deref(), &strings)).unwrap_or_default();
        texts.push((b.target_name, text_of("HUD_ARCHER_TARGET").replace("%s1", &name).replace("%s2", "")));
        let dist = lock
            .and_then(|l| enemies.get(l.target).ok())
            .map(|e| (cam_t.translation.distance(crate::player::to_bevy(e.pos)) / crate::player::UNIT * 0.0254) as i32)
            .unwrap_or(0);
        texts.push((b.range, text_of("HUD_DISTANCE_METERS").replace("%s1", &format!("{dist:04}"))));
        texts.push((b.lock_status, text_of("HUD_LOCKED")));
        for e in [b.tickers[0], b.tickers[1], b.circle] {
            tint.push((e, LOCKED));
        }
        // The blink: red 0.15 s, then white 0.4 s.
        s.blink_t -= dt;
        if s.blink_t <= 0.0 {
            s.blink_red = !s.blink_red;
            s.blink_t = if s.blink_red { 0.15 } else { 0.4 };
        }
        let c = if s.blink_red { LOCKED } else { BLINK };
        for e in [b.rings[0], b.rings[1], b.range, b.target_name, b.lock_status].into_iter().chain(b.icons) {
            tint.push((e, c));
        }
        shown.extend([b.target_name, b.range]);
        shown.extend(b.tickers);
    } else {
        s.blink_t = 0.0;
        s.blink_red = false;
        texts.push((b.lock_status, text_of("HUD_LOCKING")));
        texts.push((b.target_name, text_of("HUD_SCANNING")));
        s.rng = s.rng.wrapping_mul(1_103_515_245).wrapping_add(12345);
        texts.push((b.range, text_of("HUD_DISTANCE_METERS").replace("%s1", &format!("{}", (s.rng >> 8) % 10000))));
        for e in [b.lock_status, b.tickers[0], b.tickers[1], b.range, b.target_name, b.circle, b.rings[0], b.rings[1]].into_iter().chain(b.icons) {
            tint.push((e, LOCKING));
        }
        shown.extend([b.target_name, b.range]);
    }
    for &e in b.tickers.iter().chain(&b.icons).chain(&b.rings).chain(&[b.ammo_label, b.lock_status, b.target_name, b.range, b.circle]) {
        if let Ok(p) = parts.get_mut(e) {
            let v = if shown.contains(&e) { Visibility::Inherited } else { Visibility::Hidden };
            let mut vis = p.1;
            if *vis != v {
                *vis = v;
            }
        }
    }
    let ring_alpha = (lock_vis / 0.2).clamp(0.0, 1.0) * alpha;
    for (e, c) in tint {
        let a = if b.rings.contains(&e) { ring_alpha } else { alpha };
        if let Ok(p) = parts.get_mut(e) {
            if let Some(mut img) = p.2 {
                img.color = color(c, a);
            }
            if let Some(mut tc) = p.3 {
                tc.0 = color(c, a);
            }
        }
    }
    for (e, t) in texts {
        if let Ok(p) = parts.get_mut(e) {
            if let Some(mut text) = p.4 {
                if text.0 != t {
                    text.0 = t;
                }
            }
        }
    }
    // Circle and rings centred on the tracked point; rings 337 px x (3 - 2 lockVis).
    let ring = 337.0 * (3.0 - 2.0 * lock_vis);
    for (e, (w, h), rot) in [(b.circle, (90.0, 79.0), 0.0), (b.rings[0], (ring, ring), ring_rot[0]), (b.rings[1], (ring, ring), ring_rot[1])] {
        if let Ok(p) = parts.get_mut(e) {
            let mut node = p.0;
            node.left = pct_x(at.x - w * 0.5);
            node.top = pct_y(at.y - h * 0.5);
            node.width = pct_x(w);
            node.height = pct_y(h);
            if let Some(mut tf) = p.5 {
                tf.rotation = Rot2::degrees(rot);
            }
        }
    }
}

/// The target line's name: the enemy's AI settings `title` (`#NPC_TITAN_ATLAS_STICKYBOMB` is
/// "Ion"), "Titan" without one.
fn title(e: &Enemy, kits: Option<&crate::targets::EnemyKits>, strings: &crate::pilotweapon::Strings) -> String {
    let kit = kits.and_then(|k| k.0.get(e.kit));
    let ai = kit.and_then(|k| crate::targets::ENEMY_CLASSES.iter().find(|c| c.3 == k.name)).map(|c| c.0.to_ascii_uppercase());
    ai.and_then(|k| strings.0.get(&k).cloned()).or_else(|| strings.0.get("NPC_TITAN").cloned()).unwrap_or_else(|| "Titan".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_cd_settles() {
        let (mut p, mut v) = (Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO);
        for _ in 0..120 {
            p = smooth_cd(p, &mut v, Vec3::ZERO, 1.0 / 60.0, 8.0);
        }
        assert!(p.length() < 0.05, "{p}");
    }

    #[test]
    fn panel_pixel_centre_and_corner() {
        // A 2 x 1 panel 5 units ahead, its bottom-left at (-1, -0.5).
        let (bl, right, up, size) = (Vec3::new(-1.0, -0.5, -5.0), Vec3::X, Vec3::Y, Vec2::new(2.0, 1.0));
        let c = panel_pixel(Vec3::NEG_Z, bl, right, up, size);
        assert!((c - PANEL * 0.5).length() < 1e-3, "{c}");
        let tl = panel_pixel(Vec3::new(-1.0, 0.5, -5.0), bl, right, up, size);
        assert!(tl.length() < 1e-3, "{tl}");
    }
}
