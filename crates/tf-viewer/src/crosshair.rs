//! Per-weapon crosshairs. Each weapon script names its crosshair RUI (`RUI_CrosshairData`,
//! e.g. `ui/crosshair_tri`); RUI is compiled UI code, so the layouts are rebuilt here from the
//! RUI atlas's crosshair parts (`rui/hud/crosshairs/*`). Pieces sit on a ring as wide as the
//! weapon's current spread cone (the RUI's `adjustedSpread weapon_spread` argument) at the
//! current zoom, and a Pilot gun's crosshair fades out while aiming down sights (a Titan's
//! stays). Which parts make up which crosshair, and their size, are reconstructions.

use crate::ui::UiAssets;
use bevy::prelude::*;

#[derive(Component)]
pub struct CrosshairRoot;

/// A stand-in for the sight RUIs we can't draw (the R-97's, Alternator's, G2's, Thunderbolt's
/// and the other guns' factory reticles): a red dot at the aim point while aimed down sights,
/// so a gun whose sight would light up in the game isn't left with no reticle at all.
#[derive(Component)]
pub struct AimDot;

/// The sight RUIs drawn in 3D (ruiscreen.rs) or that are screens, not reticles.
fn sight_drawn_or_not_reticle(ui: &str) -> bool {
    ui == "ui/r101_sights"
        || ui.starts_with("ui/aog_")
        || ui.contains("ammo_counter")
        || ui.contains("pro_screen")
        || ui.contains("_bar")
        || ui.contains("panel")
        || ui.contains("screens")
}

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
    Part {
        path,
        px,
        angle: Some(angle),
        rot,
        ring: false,
        offset: (0.0, 0.0),
    }
}
const fn centred(path: &'static str, px: (f32, f32), offset: (f32, f32)) -> Part {
    Part {
        path,
        px,
        angle: None,
        rot: 0.0,
        ring: false,
        offset,
    }
}
/// A piece at a fixed offset (vh) and rotation, drawn at full atlas size (1 px = 1 screen px
/// at 1080 lines) rather than the half size the other recipes use.
const fn full(path: &'static str, px: (f32, f32), offset: (f32, f32), rot: f32) -> Part {
    Part {
        path,
        px: (px.0 * FULL, px.1 * FULL),
        angle: None,
        rot,
        ring: false,
        offset,
    }
}
const FULL: f32 = 0.0926 / VH_PER_PX;

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
    let bracket = |a: f32| {
        on_ring(
            "rui/hud/crosshairs/crosshair_bracket",
            BRACKET,
            a,
            if a < 180.0 { 180.0 } else { 0.0 },
        )
    };
    let small = |a: f32| {
        on_ring(
            "rui/hud/crosshairs/crosshair_small_rect",
            SMALL_RECT,
            a,
            0.0,
        )
    };
    let circle = Part {
        path: "rui/hud/crosshairs/crosshair_circle",
        px: (47.0, 47.0),
        angle: None,
        rot: 0.0,
        ring: true,
        offset: (0.0, 0.0),
    };
    match n {
        "tri" | "ion" | "scorch" => vec![DOT, tri(0.0), tri(120.0), tri(240.0)],
        "sniper_amped" | "titan_sniper" => {
            vec![DOT, dash(0.0), dash(90.0), dash(180.0), dash(270.0)]
        }
        "shotgun" | "mastiff" | "mozambique" | "leadwall" | "circle2" | "circle2_small" => {
            vec![DOT, circle]
        }
        "wingman" | "wingman_n" | "charge_rifle" | "smart_pistol" => {
            vec![DOT, bracket(90.0), bracket(270.0)]
        }
        "alternator" | "smr" | "lstar" => vec![DOT, small(90.0), small(270.0)],
        // The 40mm sight (a 1080p cockpit shot): the two rulers, four arcs 2.3 vh out on the
        // diagonals and a dash 3 vh above and below.
        "40mm" | "40mm_burst" | "tracker_rockets" => vec![
            full(
                "rui/hud/crosshairs/crosshair_cross_sides2",
                (246.0, 73.0),
                (0.0, 0.0),
                0.0,
            ),
            full(
                "rui/hud/crosshairs/crosshair_arc",
                (27.0, 27.0),
                (-2.2, 2.3),
                0.0,
            ),
            full(
                "rui/hud/crosshairs/crosshair_arc",
                (27.0, 27.0),
                (-2.2, -2.3),
                90.0,
            ),
            full(
                "rui/hud/crosshairs/crosshair_arc",
                (27.0, 27.0),
                (2.2, -2.3),
                180.0,
            ),
            full(
                "rui/hud/crosshairs/crosshair_arc",
                (27.0, 27.0),
                (2.2, 2.3),
                270.0,
            ),
            full(
                "rui/hud/crosshairs/crosshair_dash",
                (7.0, 23.0),
                (0.0, -3.0),
                0.0,
            ),
            full(
                "rui/hud/crosshairs/crosshair_dash",
                (7.0, 23.0),
                (0.0, 3.0),
                0.0,
            ),
        ],
        "grenade_launcher" | "grenade_launcher2" => {
            vec![
                DOT,
                centred(
                    "rui/hud/crosshairs/crosshair_launcher",
                    (15.0, 67.0),
                    (0.0, 2.2),
                ),
            ]
        }
        n if n.starts_with("titan_predator") => vec![
            centred(
                "rui/hud/crosshairs/crosshair_cross_sides",
                (159.0, 49.0),
                (0.0, 0.0),
            ),
            DOT,
        ],
        // crosshair_plus and anything not rebuilt yet.
        _ => vec![DOT, rect(0.0), rect(90.0), rect(180.0), rect(270.0)],
    }
}

pub fn spawn(commands: &mut Commands, root: Entity) {
    let c = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                width: Val::Px(0.0),
                height: Val::Px(0.0),
                ..default()
            },
            CrosshairRoot,
        ))
        .id();
    commands.entity(root).add_child(c);
    let d = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                width: Val::Px(0.0),
                height: Val::Px(0.0),
                ..default()
            },
            AimDot,
        ))
        .id();
    commands.entity(root).add_child(d);
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
    let Ok((root, children)) = roots.single() else {
        return;
    };
    // How far the crosshair fades: a Pilot's gun hands over to its sight while aiming down
    // sights; a Titan's guns have none (ADS only zooms), so theirs stays up.
    // A gun with no sight RUI and no ADS scope, or whose aim pose leaves it at the hip (the
    // EPG, Sidewinder, MGL, Charge Rifle), keeps its crosshair while zoomed: nothing else
    // marks the aim point.
    let (name, spread, ads) = match *control {
        Control::Pilot => match pilots.single().ok().and_then(|l| l.active()) {
            Some(g) => {
                let has_sight =
                    g.aim_moves_gun && (!g.def.rui.is_empty() || g.def.ads_scope.is_some());
                (
                    g.def.crosshair.clone(),
                    g.cur_spread,
                    if has_sight { g.ads } else { 0.0 },
                )
            }
            None => (String::new(), 0.0, 0.0),
        },
        _ => match titans.single() {
            Ok(w) => (w.def.crosshair.clone(), w.cur_spread, 0.0),
            Err(_) => (String::new(), 0.0, 0.0),
        },
    };
    if state.name.as_deref() != Some(name.as_str()) {
        for &c in children.into_iter().flatten() {
            commands.entity(c).despawn();
        }
        for p in recipe(&name) {
            let size = Vec2::new(p.px.0, p.px.1) * VH_PER_PX;
            let Some(img) = ui.image(p.path) else {
                continue;
            };
            let e = commands
                .spawn((
                    img,
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Vh(size.x),
                        height: Val::Vh(size.y),
                        ..default()
                    },
                    UiTransform::IDENTITY,
                    Piece {
                        angle: p.angle,
                        rot: p.rot,
                        size,
                        ring: p.ring,
                        offset: Vec2::new(p.offset.0, p.offset.1),
                    },
                ))
                .id();
            commands.entity(root).add_child(e);
        }
        log::debug!("crosshair {name:?}");
        state.name = Some(name);
        return;
    }
    // The spread cone's radius on screen, through the current (zoomed) lens.
    let fov = cams
        .single()
        .ok()
        .and_then(|p| {
            if let Projection::Perspective(pp) = p {
                Some(pp.fov)
            } else {
                None
            }
        })
        .unwrap_or(1.0);
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
        crate::hud::set(&mut node, |n| &mut n.width, Val::Vh(size.x));
        crate::hud::set(&mut node, |n| &mut n.height, Val::Vh(size.y));
        crate::hud::set(&mut node, |n| &mut n.left, Val::Vh(centre.x - size.x * 0.5));
        crate::hud::set(&mut node, |n| &mut n.top, Val::Vh(centre.y - size.y * 0.5));
        tf.rotation = Rot2::degrees(p.rot);
        img.color = Color::srgba(1.0, 1.0, 1.0, alpha);
    }
}

/// Show [`AimDot`] while a Pilot aims a gun whose factory sight RUI we don't draw.
pub fn aim_dot(
    mut commands: Commands,
    ui: Res<UiAssets>,
    control: Res<crate::pilotctl::Control>,
    pilots: Query<&crate::pilotweapon::PilotLoadout>,
    dots: Query<(Entity, Option<&Children>), With<AimDot>>,
    mut images: Query<&mut ImageNode>,
) {
    let Ok((dot, children)) = dots.single() else {
        return;
    };
    let alpha = match *control {
        crate::pilotctl::Control::Pilot => pilots
            .single()
            .ok()
            .and_then(|l| l.active())
            .filter(|g| {
                g.aim_moves_gun
                    && g.def.ads_scope.is_none()
                    && g.def
                        .rui
                        .iter()
                        .any(|(u, _)| !sight_drawn_or_not_reticle(u))
                    && !g
                        .def
                        .rui
                        .iter()
                        .any(|(u, _)| u == "ui/r101_sights" || u.starts_with("ui/aog_"))
            })
            .map_or(0.0, |g| ((g.ads - 0.6) / 0.4).clamp(0.0, 1.0)),
        _ => 0.0,
    };
    match children.and_then(|c| c.first()) {
        Some(&c) => {
            if let Ok(mut img) = images.get_mut(c) {
                img.color = Color::srgba(1.0, 0.2, 0.15, alpha);
            }
        }
        None => {
            let Some(img) = ui.image("rui/hud/crosshairs/crosshair_dot") else {
                return;
            };
            let size = 15.0 * VH_PER_PX;
            let e = commands
                .spawn((
                    img,
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Vh(size),
                        height: Val::Vh(size),
                        left: Val::Vh(-size * 0.5),
                        top: Val::Vh(-size * 0.5),
                        ..default()
                    },
                ))
                .id();
            commands.entity(dot).add_child(e);
        }
    }
}
