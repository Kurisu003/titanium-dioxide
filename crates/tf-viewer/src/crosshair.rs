//! Per-weapon crosshairs. Each weapon script names its crosshair RUI (`RUI_CrosshairData`,
//! e.g. `ui/crosshair_tri`); RUI is compiled UI code, so the layouts are rebuilt here from the
//! RUI atlas's crosshair parts (`rui/hud/crosshairs/*`). Pieces sit on a ring as wide as the
//! weapon's current spread cone (the RUI's `adjustedSpread weapon_spread` argument) at the
//! current zoom, and the crosshair fades out while aiming down sights. Which parts make up
//! which crosshair, and their size, are reconstructions.

use crate::ui::UiAssets;
use bevy::prelude::*;

#[derive(Component)]
pub struct CrosshairRoot;

#[derive(Component)]
pub struct Piece {
    /// Where on the ring (degrees clockwise from up); None: at the centre.
    angle: Option<f32>,
    /// Extra rotation (degrees clockwise) of the image.
    rot: f32,
    /// Size in vh.
    size: Vec2,
    /// A ring image sized to the spread (circle crosshairs).
    ring: bool,
    /// Offset from the centre in vh (non-ring pieces).
    offset: Vec2,
}

/// The crosshair being shown (its RUI name).
#[derive(Resource, Default)]
pub struct CrosshairState {
    name: Option<String>,
}

/// vh per atlas pixel (the atlas is drawn for a 1080-line screen; crosshairs at half size).
const VH_PER_PX: f32 = 0.05;
/// The ring never closes tighter than this (vh), so pieces don't overlap at zero spread.
const MIN_RADIUS: f32 = 1.1;

struct Part {
    path: &'static str,
    px: (f32, f32),
    angle: Option<f32>,
    rot: f32,
    ring: bool,
    offset: (f32, f32),
}

const fn on_ring(path: &'static str, px: (f32, f32), angle: f32, rot: f32) -> Part {
    Part { path, px, angle: Some(angle), rot, ring: false, offset: (0.0, 0.0) }
}
const fn centred(path: &'static str, px: (f32, f32), offset: (f32, f32)) -> Part {
    Part { path, px, angle: None, rot: 0.0, ring: false, offset }
}

const DOT: Part = centred("rui/hud/crosshairs/crosshair_dot", (9.0, 9.0), (0.0, 0.0));
// Triangles at 70% (they read larger than the game's next to the other parts).
const TRI: (f32, f32) = (37.0 * 0.7, 56.0 * 0.7);
const RECT: (f32, f32) = (51.0, 13.0);
const DASH: (f32, f32) = (7.0, 23.0);
const BRACKET: (f32, f32) = (11.0, 41.0);
const SMALL_RECT: (f32, f32) = (13.0, 25.0);

fn recipe(name: &str) -> Vec<Part> {
    let n = name.trim_start_matches("ui/crosshair_");
    let tri = |a: f32| on_ring("rui/hud/crosshairs/crosshair_triangle", TRI, a, a + 180.0);
    let rect = |a: f32| on_ring("rui/hud/crosshairs/crosshair_rect", RECT, a, a - 90.0);
    let dash = |a: f32| on_ring("rui/hud/crosshairs/crosshair_dash", DASH, a, a);
    let bracket = |a: f32| on_ring("rui/hud/crosshairs/crosshair_bracket", BRACKET, a, if a < 180.0 { 180.0 } else { 0.0 });
    let small = |a: f32| on_ring("rui/hud/crosshairs/crosshair_small_rect", SMALL_RECT, a, 0.0);
    let circle = Part { path: "rui/hud/crosshairs/crosshair_circle", px: (47.0, 47.0), angle: None, rot: 0.0, ring: true, offset: (0.0, 0.0) };
    match n {
        "tri" | "ion" | "scorch" => vec![DOT, tri(0.0), tri(120.0), tri(240.0)],
        "sniper_amped" | "titan_sniper" => vec![DOT, dash(0.0), dash(90.0), dash(180.0), dash(270.0)],
        "shotgun" | "mastiff" | "mozambique" | "leadwall" | "circle2" | "circle2_small" => vec![DOT, circle],
        "wingman" | "wingman_n" | "charge_rifle" | "smart_pistol" => vec![DOT, bracket(90.0), bracket(270.0)],
        "alternator" | "smr" | "lstar" => vec![DOT, small(90.0), small(270.0)],
        "grenade_launcher" | "grenade_launcher2" | "40mm" | "40mm_burst" | "tracker_rockets" => {
            vec![DOT, centred("rui/hud/crosshairs/crosshair_launcher", (15.0, 67.0), (0.0, 2.2))]
        }
        n if n.starts_with("titan_predator") => vec![centred("rui/hud/crosshairs/crosshair_cross_sides", (159.0, 49.0), (0.0, 0.0)), DOT],
        // crosshair_plus and anything not rebuilt yet.
        _ => vec![DOT, rect(0.0), rect(90.0), rect(180.0), rect(270.0)],
    }
}

pub fn spawn(commands: &mut Commands, root: Entity) {
    let c = commands
        .spawn((Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), top: Val::Percent(50.0), width: Val::Px(0.0), height: Val::Px(0.0), ..default() }, CrosshairRoot))
        .id();
    commands.entity(root).add_child(c);
}

/// What the crosshair should show: the active weapon's crosshair, spread and zoom.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update(
    mut commands: Commands,
    ui: Res<UiAssets>,
    mut state: ResMut<CrosshairState>,
    control: Res<crate::pilotctl::Control>,
    pilots: Query<&crate::pilotweapon::PilotLoadout>,
    titans: Query<&crate::weapons::Weapon>,
    cams: Query<&Projection, With<crate::player::MainCamera>>,
    roots: Query<(Entity, Option<&Children>), With<CrosshairRoot>>,
    mut pieces: Query<(&Piece, &mut Node, &mut UiTransform, &mut ImageNode)>,
) {
    use crate::pilotctl::Control;
    let Ok((root, children)) = roots.single() else { return };
    let (name, spread, ads) = match *control {
        Control::Pilot => match pilots.single().ok().and_then(|l| l.active()) {
            Some(g) => (g.def.crosshair.clone(), g.cur_spread, g.ads),
            None => (String::new(), 0.0, 0.0),
        },
        _ => match titans.single() {
            Ok(w) => (w.def.crosshair.clone(), w.cur_spread, w.ads),
            Err(_) => (String::new(), 0.0, 0.0),
        },
    };
    if state.name.as_deref() != Some(name.as_str()) {
        for &c in children.into_iter().flatten() {
            commands.entity(c).despawn();
        }
        for p in recipe(&name) {
            let size = Vec2::new(p.px.0, p.px.1) * VH_PER_PX;
            let Some(img) = ui.image(p.path) else { continue };
            let e = commands
                .spawn((
                    img,
                    Node { position_type: PositionType::Absolute, width: Val::Vh(size.x), height: Val::Vh(size.y), ..default() },
                    UiTransform::IDENTITY,
                    Piece { angle: p.angle, rot: p.rot, size, ring: p.ring, offset: Vec2::new(p.offset.0, p.offset.1) },
                ))
                .id();
            commands.entity(root).add_child(e);
        }
        log::debug!("crosshair {name:?}");
        state.name = Some(name);
        return;
    }
    // The spread cone's radius on screen, through the current (zoomed) lens.
    let fov = cams.single().ok().and_then(|p| if let Projection::Perspective(pp) = p { Some(pp.fov) } else { None }).unwrap_or(1.0);
    let radius = ((spread.to_radians() * 0.5).tan() / (fov * 0.5).tan() * 50.0).max(MIN_RADIUS);
    let alpha = (1.0 - ads * 1.5).clamp(0.0, 1.0);
    for (p, mut node, mut tf, mut img) in &mut pieces {
        let (size, centre) = if p.ring {
            let d = (radius * 2.0).max(p.size.x);
            (Vec2::splat(d), Vec2::ZERO)
        } else {
            let c = match p.angle {
                Some(a) => {
                    let a = a.to_radians();
                    // Out to the ring plus half the piece, so its inner end touches the ring.
                    let r = radius + p.size.y.min(p.size.x).max(p.size.y * 0.5) * 0.5;
                    Vec2::new(a.sin(), -a.cos()) * r
                }
                None => p.offset,
            };
            (p.size, c)
        };
        node.width = Val::Vh(size.x);
        node.height = Val::Vh(size.y);
        node.left = Val::Vh(centre.x - size.x * 0.5);
        node.top = Val::Vh(centre.y - size.y * 0.5);
        tf.rotation = Rot2::degrees(p.rot);
        img.color = Color::srgba(1.0, 1.0, 1.0, alpha);
    }
}
