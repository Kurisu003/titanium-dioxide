//! In-game HUD in Titanfall 2's style, drawn with the game's own HUD art (ui.rpak atlases,
//! looked up by RUI path) and fonts: crosshair, Titan shield/health segments (doomed bar),
//! dash pips, ability slots with cooldowns, the core meter, weapon and ammo, the wave callout,
//! prompts and damage flash.

use crate::abilities::{CoreState, Ordnance, TitanCore};
use crate::combat::{TitanHealth, Vortex};
use crate::game::{Game, GameState};
use crate::pilotctl::{Control, PlayerPilot, Titanfall};
use crate::player::{CameraMode, PlayerTitan};
use crate::targets::Enemy;
use crate::ui::{UiAssets, VhText};
use bevy::prelude::*;

const WHITE: Color = Color::srgba(1.0, 1.0, 1.0, 0.92);
const DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.55);
const SHIELD: Color = Color::srgb(0.62, 0.42, 0.75);
/// The Titan health segments' cyan and the dash pips' blue, sampled on a 1080p cockpit shot.
const HEALTH_CYAN: Color = Color::srgb(0.42, 0.69, 0.82);
const PIP_BLUE: Color = Color::srgb(0.53, 0.69, 0.80);
const DOOM: Color = Color::srgb(1.0, 0.22, 0.12);
const AMBER: Color = Color::srgb(1.0, 0.68, 0.18);

#[derive(Component)]
pub struct HudRoot;
/// Only shown while in the Titan / on foot.
#[derive(Component)]
pub struct TitanOnly;
/// The Pilot's status block (on foot), laid out like BT's.
/// The bottom-left meter's ring fill (the earn meter on foot, the core meter in the Titan).
#[derive(Component)]
pub struct MeterFill;
/// The meter's caption ("44%", "TITANFALL") and the key box beside it.
#[derive(Component)]
pub struct MeterText;
#[derive(Component)]
/// The small percent sign after the meter's digits.
pub struct MeterPct;
#[derive(Component)]
pub struct MeterKey;
#[derive(Component)]
pub struct MeterKeyText;
/// The weapon silhouette on the ammo bar.
#[derive(Component)]
pub struct WeaponIcon;
#[derive(Component)]
pub struct AmmoBar;
#[derive(Component)]
pub struct HealthSeg(usize);

/// The part of a health segment just lost, flashing then fading (ajax_cockpit_lost_health_segment).
#[derive(Component)]
pub struct LostSeg(usize);
#[derive(Component)]
pub struct ShieldFill;
#[derive(Component)]
pub struct DoomFill;
#[derive(Component)]
pub struct DashPip(usize);
#[derive(Component)]
pub struct CooldownShade(Slot);
#[derive(Component)]
pub struct SlotText(Slot);
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Ordnance,
    Defensive,
    Utility,
}
#[derive(Component)]
pub struct CoreIcon;
/// The Titan silhouette over the earn meter art, on foot.
#[derive(Component)]
pub struct MeterSilhouette;
#[derive(Component)]
pub struct MeterPlus;
/// The ability slot's icon and label (swapped per loadout by titankit).
#[derive(Component)]
pub struct SlotIcon(pub Slot);
#[derive(Component)]
pub struct SlotLabel(pub Slot);
#[derive(Component)]
pub struct CoreLabel;
#[derive(Component)]
pub struct AmmoCount;
#[derive(Component)]
pub struct WaveTitle;
#[derive(Component)]
pub struct WaveSub;
/// The wave panel's bar fill and percentage.
#[derive(Component)]
pub struct WaveFill;
#[derive(Component)]
pub struct WavePct;
#[derive(Component)]
pub struct WaveType(usize);
#[derive(Component)]
pub struct WaveSplash;
#[derive(Component)]
pub struct WaveSplashText;
#[derive(Component)]
pub struct WaveSplashSeg(usize);
#[derive(Component)]
/// A splash band: `true` for the wave intro (Playing), `false` for "WAVE COMPLETE" (Intermission).
pub struct WaveSplashBar(bool);
#[derive(Component)]
/// The wave's name under the intro band.
pub struct WaveQuip;

/// The splash's big text: Metronic semibold at the game's size, thickened to its bold weight by
/// a second copy 1.5 px right, over a dark shadow.
fn spawn_splash_text(commands: &mut Commands, ui: &UiAssets, size: f32) -> Entity {
    let wrap = commands.spawn(Node::default()).id();
    let base = text(commands, "", &ui.bold_font, size, Color::WHITE);
    commands.entity(base).insert((
        WaveSplashText,
        TextLayout::new_with_no_wrap(),
        TextShadow {
            offset: Vec2::new(3.0, 3.0),
            color: Color::srgba(0.0, 0.0, 0.0, 0.75),
        },
    ));
    commands.entity(wrap).add_child(base);
    for dx in [1.5, 3.0] {
        let over = text(commands, "", &ui.bold_font, size, Color::WHITE);
        commands.entity(over).insert((
            WaveSplashText,
            TextLayout::new_with_no_wrap(),
            Node {
                position_type: PositionType::Absolute,
                left: px(dx),
                top: px(0),
                ..default()
            },
        ));
        commands.entity(wrap).add_child(over);
    }
    wrap
}
/// How long the wave splash stays up.
const SPLASH_SECS: f32 = 3.5;
#[derive(Component)]
pub struct Banner;
#[derive(Component)]
pub struct Prompt;
#[derive(Component)]
pub struct HurtOverlay;

const DAMAGE_ARCS: usize = 8;
/// The damage arcs' distance from the crosshair (vh).
const DAMAGE_ARC_RADIUS: f32 = 13.0;

// The bottom-left cockpit cluster, measured off the game's HUD (in vh of a 16:9 frame): the
// meter's centre and diameters, the ability slots' top-left and size, the ammo bar.
// Measured on 1080p frames of the game (dl/yt2 Kodai frames): ring 204 px across, disc
// 150 px, centre (215, 920); slots 80x72 px from x 340 with 10 px gaps; gun icon 115x43 px
// from x 555; ammo digits 38 px.
const METER_X: f32 = 19.9;
const METER_Y: f32 = 86.0;
const METER_D: f32 = 18.4;
const METER_RING: f32 = 1.0;
const DISC_D: f32 = 14.3;
pub const SLOTS_X: f32 = 31.5;
pub const SLOTS_Y: f32 = 86.1;
pub const SLOT_SIZE: f32 = 7.4;
pub const SLOT_H: f32 = 6.7;
pub const SLOT_GAP: f32 = 0.9;
const AMMO_X: f32 = 49.0;
const AMMO_W: f32 = 13.0;
const AMMO_H: f32 = SLOT_H;
const METER_BLUE: Color = Color::srgb(0.62, 0.84, 1.0);
/// The core ring when the core is ready (an amber ring in the cockpit).
const METER_AMBER: Color = Color::srgb(0.78, 0.56, 0.0);
// The Titan health bar on the windshield (frame 655..1285 x 210..246 px on a 1080p cockpit
// shot) and the dash pips under the crosshair (two 65 px bars at y 785).
const HEALTH_X: f32 = 60.6;
const HEALTH_Y: f32 = 19.4;
const HEALTH_W: f32 = 58.3;
const HEALTH_H: f32 = 3.3;
const PIPS_Y: f32 = 72.5;
// The Frontier Defense wave panel, top left (title at (500, 90) px, bar 500..760 x 118..160).
const FD_X: f32 = 46.3;
const FD_Y: f32 = 7.4;
/// How long a damage direction stays up.
const DAMAGE_ARC_TIME: f32 = 1.6;

#[derive(Component)]
pub struct DamageArc(usize);

/// Where recent damage to the player came from (game-space point, seconds left).
#[derive(Resource, Default)]
pub struct DamageFrom(pub Vec<(Vec3, f32)>);

/// The HUD is off while a synced execution plays (the game hides it for the cinematic camera);
/// set by `executions::execution_camera` each frame.
#[derive(Resource, Default)]
pub struct HudHidden(pub bool);

impl DamageFrom {
    pub fn push(&mut self, from: Vec3) {
        // Refresh an indicator already pointing the same way instead of stacking them.
        if let Some(d) = self.0.iter_mut().find(|(p, _)| p.distance(from) < 300.0) {
            *d = (from, DAMAGE_ARC_TIME);
            return;
        }
        if self.0.len() >= DAMAGE_ARCS {
            self.0.remove(0);
        }
        self.0.push((from, DAMAGE_ARC_TIME));
    }
}

/// Place the damage arcs: angle 0 is straight ahead (top of the ring), clockwise to the right.
pub fn update_damage_arcs(
    time: Res<Time>,
    mut from: ResMut<DamageFrom>,
    camera: Query<&GlobalTransform, With<crate::MainCamera>>,
    mut arcs: Query<(
        &DamageArc,
        &mut Node,
        &mut UiTransform,
        &mut Visibility,
        &mut ImageNode,
    )>,
) {
    let dt = time.delta_secs();
    from.0.retain_mut(|(_, t)| {
        *t -= dt;
        *t > 0.0
    });
    let Ok(cam) = camera.single() else { return };
    let (eye, fwd, right) = (
        cam.translation(),
        cam.forward().as_vec3(),
        cam.right().as_vec3(),
    );
    for (arc, mut node, mut tf, mut vis, mut img) in &mut arcs {
        let Some(&(p, t)) = from.0.get(arc.0) else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let to = crate::player::to_bevy(p) - eye;
        let (f, r) = (
            to.dot(fwd.with_y(0.0).normalize_or_zero()),
            to.dot(right.with_y(0.0).normalize_or_zero()),
        );
        let a = r.atan2(f);
        set(&mut node, |n| &mut n.left, vh(a.sin() * DAMAGE_ARC_RADIUS));
        set(&mut node, |n| &mut n.top, vh(-a.cos() * DAMAGE_ARC_RADIUS));
        tf.rotation = Rot2::radians(a);
        vis.set_if_neq(Visibility::Inherited);
        let alpha = (t / DAMAGE_ARC_TIME).min(1.0).powf(0.7);
        img.color = Color::srgba(1.0, 0.22, 0.1, alpha);
    }
}
/// Crosshair hit marker ticks.
#[derive(Component)]
pub struct HitMarker;
/// How long a hit marker stays (the game's hit_indicator runs 1 s with its fade; ours holds
/// half of this and fades over the rest).
const HIT_MARKER_TIME: f32 = 0.4;
/// Charge meter under the crosshair (railgun levels, Charge Rifle wind-up).
#[derive(Component)]
pub struct ChargeHud;
#[derive(Component)]
pub struct ChargePip(usize);
#[derive(Component)]
pub struct ChargePipFill(usize);
const CHARGE_PIPS: usize = 6;
const CHARGE_WIDTH: f32 = 12.0;

/// Set one field of a component, marking the component changed only when the value differs.
/// Writing a field marks it changed even with the same value, and a changed UI `Node` is laid
/// out again with its ancestors; most HUD values are the same from one frame to the next.
pub fn set<C, T: PartialEq>(c: &mut Mut<C>, field: impl FnOnce(&mut C) -> &mut T, v: T) {
    let slot = field(c.bypass_change_detection());
    if *slot != v {
        *slot = v;
        c.set_changed();
    }
}

fn vh(v: f32) -> Val {
    Val::Vh(v)
}

fn text(commands: &mut Commands, s: &str, font: &Handle<Font>, size: f32, color: Color) -> Entity {
    commands
        .spawn((
            Text::new(s),
            TextFont {
                font: font.clone(),
                font_size: 16.0,
                ..default()
            },
            TextColor(color),
            VhText(size),
        ))
        .id()
}

/// An icon drawn at its own aspect ratio (UI atlas images aren't square), as large as fits
/// its parent and centred in it; see `fit_icons`.
#[derive(Component)]
pub struct FitIcon;

/// Size and centre each [`FitIcon`] for its image whenever the image changes.
pub fn fit_icons(
    images: Res<Assets<Image>>,
    mut icons: Query<
        (&ImageNode, &mut Node),
        (With<FitIcon>, Or<(Changed<ImageNode>, Added<FitIcon>)>),
    >,
) {
    for (img, mut node) in &mut icons {
        let size = img
            .rect
            .map(|r| r.size())
            .or_else(|| images.get(&img.image).map(|i| i.size_f32()));
        let Some(size) = size.filter(|s| s.x > 0.0 && s.y > 0.0) else {
            continue;
        };
        let aspect = size.x / size.y;
        let (w, h) = if aspect >= 1.0 {
            (100.0, 100.0 / aspect)
        } else {
            (100.0 * aspect, 100.0)
        };
        node.position_type = PositionType::Absolute;
        node.width = percent(w);
        node.height = percent(h);
        node.left = percent((100.0 - w) * 0.5);
        node.top = percent((100.0 - h) * 0.5);
    }
}

fn image(commands: &mut Commands, ui: &UiAssets, path: &str, node: Node, color: Color) -> Entity {
    match ui.image(path) {
        Some(mut img) => {
            img.color = color;
            commands.spawn((img, node)).id()
        }
        None => commands
            .spawn((node, BackgroundColor(color.with_alpha(0.3))))
            .id(),
    }
}

/// The pieces of an ability slot from [`slot_column`], for the caller's marker components.
pub struct SlotParts {
    pub col: Entity,
    pub icon: Entity,
    pub shade: Entity,
    pub state: Entity,
    pub label: Entity,
}

/// One ability slot of the cockpit cluster (the game's ability_hud): the icon on a dark square,
/// a cooldown shade rising over it, a state text on it, and the key in a box under it. The
/// label is kept for callers that name the ability but isn't shown (the game shows none).
pub fn slot_column(commands: &mut Commands, ui: &UiAssets, icon: &str, key: &str) -> SlotParts {
    let col = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: vh(0.5),
            width: vh(SLOT_SIZE),
            ..default()
        })
        .id();
    let frame = image(
        commands,
        ui,
        "rui/hud/ability_background_square",
        Node {
            width: vh(SLOT_SIZE),
            height: vh(SLOT_H),
            overflow: Overflow::clip(),
            ..default()
        },
        Color::srgba(0.0, 0.0, 0.0, 0.25),
    );
    let icon_box = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: percent(15),
            top: percent(12),
            width: percent(70),
            height: percent(70),
            ..default()
        })
        .id();
    let ic = commands
        .spawn((Node::default(), ui.image(icon).unwrap_or_default(), FitIcon))
        .id();
    commands.entity(icon_box).add_child(ic);
    let shade = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(0),
                height: percent(0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        ))
        .id();
    let state = text(commands, "", &ui.bold_font, 1.5, AMBER);
    commands.entity(state).insert((
        Node {
            position_type: PositionType::Absolute,
            bottom: vh(0.3),
            left: px(0),
            right: px(0),
            ..default()
        },
        TextLayout::new_with_justify(Justify::Center),
    ));
    commands
        .entity(frame)
        .add_children(&[icon_box, shade, state]);
    let key_box = commands
        .spawn((
            Node {
                border: UiRect::all(px(1)),
                padding: UiRect::axes(vh(0.5), vh(0.05)),
                ..default()
            },
            BorderColor::all(DIM),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        ))
        .id();
    let k = text(commands, key, &ui.bold_font, 1.5, WHITE);
    commands.entity(key_box).add_child(k);
    let label = text(commands, "", &ui.font, 1.3, DIM);
    commands.entity(label).insert(Node {
        display: Display::None,
        ..default()
    });
    commands.entity(col).add_children(&[frame, key_box, label]);
    SlotParts {
        col,
        icon: ic,
        shade,
        state,
        label,
    }
}

/// Where the meter arc starts (clockwise from the top) and how far it runs: the game's earn
/// meter fills clockwise from about 8 o'clock to 4 o'clock, leaving the bottom for the caption.
const METER_START: f32 = 245.0;
const METER_SPAN: f32 = 230.0;

/// The meter ring's conic gradient: `frac` of the arc in `color`.
fn meter_gradient(frac: f32, color: Color) -> ConicGradient {
    let d = std::f32::consts::PI / 180.0;
    let (a0, a1) = (
        METER_START * d,
        (METER_START + METER_SPAN * frac.clamp(0.0, 1.0)) * d,
    );
    let tau = std::f32::consts::TAU;
    let stops = if a1 <= tau {
        vec![
            AngularColorStop::new(Color::NONE, 0.0),
            AngularColorStop::new(Color::NONE, a0),
            AngularColorStop::new(color, a0),
            AngularColorStop::new(color, a1),
            AngularColorStop::new(Color::NONE, a1),
            AngularColorStop::new(Color::NONE, tau),
        ]
    } else {
        vec![
            AngularColorStop::new(color, 0.0),
            AngularColorStop::new(color, a1 - tau),
            AngularColorStop::new(Color::NONE, a1 - tau),
            AngularColorStop::new(Color::NONE, a0),
            AngularColorStop::new(color, a0),
            AngularColorStop::new(color, tau),
        ]
    };
    ConicGradient::new(UiPosition::CENTER, stops)
}

pub fn spawn_hud(mut commands: Commands, ui: Res<UiAssets>) {
    let root = commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                position_type: PositionType::Absolute,
                ..default()
            },
            HudRoot,
        ))
        .id();

    // Damage flash (under everything else).
    let hurt = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundGradient::from(RadialGradient::new(
                UiPosition::CENTER,
                RadialGradientShape::FarthestCorner,
                vec![
                    ColorStop::new(Color::NONE, percent(60)),
                    ColorStop::new(Color::srgba(0.75, 0.04, 0.0, 0.5), percent(100)),
                ],
            )),
            Visibility::Hidden,
            HurtOverlay,
        ))
        .id();
    commands.entity(root).add_child(hurt);

    // Damage direction indicators: the game's arc art on a ring around the crosshair, pooled.
    let ring = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: percent(50),
            width: px(0),
            height: px(0),
            ..default()
        })
        .id();
    commands.entity(root).add_child(ring);
    for i in 0..DAMAGE_ARCS {
        let arc = image(
            &mut commands,
            &ui,
            "rui/hud/damage_indicator_small",
            Node {
                position_type: PositionType::Absolute,
                width: vh(10.0),
                height: vh(3.6),
                margin: UiRect {
                    left: vh(-5.0),
                    top: vh(-1.8),
                    ..default()
                },
                ..default()
            },
            Color::srgba(1.0, 0.22, 0.1, 0.0),
        );
        commands
            .entity(arc)
            .insert((UiTransform::IDENTITY, Visibility::Hidden, DamageArc(i)));
        commands.entity(ring).add_child(arc);
    }

    // Crosshair: the active weapon's (crosshair.rs).
    crate::crosshair::spawn(&mut commands, root);
    // Hit marker: the game's hit_indicator tick (29 px at 1080p) four times around the centre,
    // flipped into an X with a gap at the crosshair.
    for (x, y, fx, fy) in [
        (-1.0f32, -1.0f32, false, false),
        (1.0, -1.0, true, false),
        (-1.0, 1.0, false, true),
        (1.0, 1.0, true, true),
    ] {
        let node = Node {
            position_type: PositionType::Absolute,
            width: vh(2.7),
            height: vh(2.7),
            left: Val::Vh(x * 1.55 - 1.35),
            top: Val::Vh(y * 1.55 - 1.35),
            ..default()
        };
        let e = image(
            &mut commands,
            &ui,
            "rui/hit_indicators/hit_indicator_full",
            node,
            Color::WHITE,
        );
        commands.entity(e).insert((HitMarker, Visibility::Hidden));
        if let Some(mut c) = commands.get_entity(e).ok() {
            c.entry::<ImageNode>().and_modify(move |mut i| {
                i.flip_x = fx;
                i.flip_y = fy;
            });
        }
        let holder = commands
            .spawn(Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(50),
                ..default()
            })
            .id();
        commands.entity(holder).add_child(e);
        commands.entity(root).add_child(holder);
    }
    spawn_smart_hud(&mut commands, &ui, root);
    spawn_tone_hud(&mut commands, &ui, root);
    // Charge meter: one pip per charge level (a single bar for a continuous charge).
    let charge_holder = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: percent(50),
            left: px(0),
            right: px(0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .id();
    let charge = commands
        .spawn((
            Node {
                margin: UiRect::top(vh(3.2)),
                column_gap: vh(0.4),
                height: vh(0.5),
                ..default()
            },
            ChargeHud,
            Visibility::Hidden,
        ))
        .id();
    for i in 0..CHARGE_PIPS {
        let frame = commands
            .spawn((
                Node {
                    width: vh(CHARGE_WIDTH / CHARGE_PIPS as f32),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                ChargePip(i),
            ))
            .id();
        let fill = commands
            .spawn((
                Node {
                    width: percent(0),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(WHITE),
                ChargePipFill(i),
            ))
            .id();
        commands.entity(frame).add_child(fill);
        commands.entity(charge).add_child(frame);
    }
    commands.entity(charge_holder).add_child(charge);
    commands.entity(root).add_child(charge_holder);

    // --- Top centre: the Titan's shield and health segments on the windshield (the game's
    // cockpit RUI draws them there; measured on a 1080p cockpit shot: black frame with five
    // cyan segments 18 px tall under a 3 px shield line). ---
    let bottom = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: vh(HEALTH_X),
                top: vh(HEALTH_Y),
                width: vh(HEALTH_W),
                height: vh(HEALTH_H),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(vh(0.45), vh(0.3)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
            TitanOnly,
        ))
        .id();
    commands.entity(root).add_child(bottom);
    let health = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: vh(0.3),
            width: percent(100),
            flex_grow: 1.0,
            ..default()
        })
        .id();
    let shield_frame = commands
        .spawn((
            Node {
                height: vh(0.3),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        ))
        .id();
    let shield_fill = commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(SHIELD),
            ShieldFill,
        ))
        .id();
    commands.entity(shield_frame).add_child(shield_fill);
    let segs = commands
        .spawn(Node {
            column_gap: vh(0.2),
            height: vh(1.7),
            ..default()
        })
        .id();
    for i in 0..crate::combat::SEGMENTS as usize {
        let frame = commands
            .spawn((
                Node {
                    flex_grow: 1.0,
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            ))
            .id();
        // Each segment has the game's lighter edge along its top and bottom.
        let fill = commands
            .spawn((
                Node {
                    width: percent(100),
                    height: percent(100),
                    border: UiRect::vertical(vh(0.2)),
                    ..default()
                },
                BackgroundColor(HEALTH_CYAN),
                BorderColor::all(Color::srgba(0.8, 0.95, 1.0, 0.9)),
                HealthSeg(i),
            ))
            .id();
        let lost = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    height: percent(100),
                    width: percent(0),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                LostSeg(i),
            ))
            .id();
        commands.entity(frame).add_children(&[fill, lost]);
        commands.entity(segs).add_child(frame);
    }
    // Doomed: one red bar with the game's doom pattern.
    let doom = image(
        &mut commands,
        &ui,
        "rui/hud/healthbar_doom_16x160",
        Node {
            height: vh(1.7),
            width: percent(100),
            ..default()
        },
        DOOM,
    );
    commands.entity(doom).insert((DoomFill, Visibility::Hidden));
    commands
        .entity(health)
        .add_children(&[shield_frame, segs, doom]);
    commands.entity(bottom).add_child(health);
    // The dash pips sit under the crosshair.
    let pips = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: vh(PIPS_Y),
                left: px(0),
                right: px(0),
                justify_content: JustifyContent::Center,
                column_gap: vh(0.9),
                height: vh(0.35),
                ..default()
            },
            TitanOnly,
        ))
        .id();
    for i in 0..2 {
        let frame = commands
            .spawn((
                Node {
                    width: vh(6.0),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            ))
            .id();
        let fill = commands
            .spawn((
                Node {
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(PIP_BLUE),
                DashPip(i),
            ))
            .id();
        commands.entity(frame).add_child(fill);
        commands.entity(pips).add_child(frame);
    }
    commands.entity(root).add_child(pips);
    crate::feed::spawn(&mut commands, &ui, root);

    // --- Bottom left: the cockpit cluster, laid out like the game's ---
    // The meter: a ring filled anticlockwise from the top around a dark disc with an icon in
    // it. On foot it is the earn meter (BT's health while he is up, his rebuild while he is
    // down, TITANFALL when he can drop); in the Titan it is the core meter.
    let meter = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: vh(METER_X - METER_D * 0.5),
            top: vh(METER_Y - METER_D * 0.5),
            width: vh(METER_D),
            height: vh(METER_D),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .id();
    commands.entity(root).add_child(meter);
    let ring_node = || Node {
        position_type: PositionType::Absolute,
        left: px(0),
        top: px(0),
        width: percent(100),
        height: percent(100),
        border: UiRect::all(vh(METER_RING)),
        border_radius: BorderRadius::MAX,
        ..default()
    };
    let track = commands
        .spawn((
            ring_node(),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.16)),
        ))
        .id();
    let fill = commands
        .spawn((
            ring_node(),
            BorderGradient::from(meter_gradient(0.0, METER_BLUE)),
            MeterFill,
        ))
        .id();
    let disc = commands
        .spawn((
            Node {
                width: vh(DISC_D),
                height: vh(DISC_D),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        ))
        .id();
    let icon_box = commands
        .spawn(Node {
            width: vh(9.5),
            height: vh(9.5),
            ..default()
        })
        .id();
    let meter_icon = image(
        &mut commands,
        &ui,
        "rui/hud/earn_meter/titan_follow",
        Node::default(),
        WHITE,
    );
    commands.entity(meter_icon).insert((CoreIcon, FitIcon));
    commands.entity(icon_box).add_child(meter_icon);
    // On foot the Titan's silhouette sits over the meter art (the game's earn meter draws
    // the two layers).
    let sil_box = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(18),
                top: percent(5),
                width: percent(64),
                height: percent(90),
                ..default()
            },
            MeterSilhouette,
        ))
        .id();
    let sil = image(
        &mut commands,
        &ui,
        "rui/hud/earn_meter/titan_building",
        Node::default(),
        WHITE,
    );
    commands.entity(sil).insert(FitIcon);
    commands.entity(sil_box).add_child(sil);
    commands.entity(icon_box).add_child(sil_box);
    commands.entity(disc).add_child(icon_box);
    commands.entity(meter).add_children(&[track, fill, disc]);
    // The white tick at the arc's end, radial, like the game's.
    {
        let end = (METER_START + METER_SPAN).to_radians();
        let r = METER_D * 0.5;
        let (w, h) = (0.5, 2.0);
        let tick = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: vh(METER_X + r * end.sin() - w * 0.5),
                    top: vh(METER_Y - r * end.cos() - h * 0.5),
                    width: vh(w),
                    height: vh(h),
                    ..default()
                },
                BackgroundColor(WHITE),
                UiTransform {
                    rotation: Rot2::radians(-end),
                    ..default()
                },
            ))
            .id();
        commands.entity(root).add_child(tick);
    }
    // Its caption under the disc: the percentage, or what it is ready for; the key sits
    // outside the ring at 5 o'clock.
    let caption = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: vh(METER_X - 9.0),
            top: vh(METER_Y + 3.9),
            width: vh(18.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: vh(0.5),
            ..default()
        })
        .id();
    commands.entity(root).add_child(caption);
    // On foot the game prefixes the percentage with an orange "+".
    let plus = text(&mut commands, "+", &ui.bold_font, 2.0, AMBER);
    commands.entity(plus).insert((
        MeterPlus,
        Node {
            margin: UiRect::right(vh(0.2)),
            ..default()
        },
    ));
    commands.entity(caption).add_child(plus);
    let meter_text = text(&mut commands, "", &ui.bold_font, 2.2, WHITE);
    commands.entity(meter_text).insert(MeterText);
    // The percent sign is smaller than the digits.
    let meter_pct = commands
        .spawn((
            TextSpan::new(""),
            TextFont {
                font: ui.bold_font.clone(),
                font_size: 16.0,
                ..default()
            },
            TextColor(WHITE),
            VhText(1.45),
            MeterPct,
        ))
        .id();
    commands.entity(meter_text).add_child(meter_pct);
    let key_box = commands
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                left: vh(METER_X + 5.3),
                top: vh(METER_Y + 4.7),
                border: UiRect::all(px(1)),
                padding: UiRect::axes(vh(0.5), vh(0.05)),
                ..default()
            },
            BorderColor::all(WHITE),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            MeterKey,
        ))
        .id();
    let key_text = text(&mut commands, "", &ui.bold_font, 1.7, WHITE);
    commands.entity(key_text).insert(MeterKeyText);
    commands.entity(key_box).add_child(key_text);
    commands.entity(caption).add_child(meter_text);
    commands.entity(root).add_child(key_box);

    // The Titan's ability slots (the Pilot's are pilotability::spawn_pilot_slots, same place).
    let slots = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: vh(SLOTS_X),
                top: vh(SLOTS_Y),
                column_gap: vh(SLOT_GAP),
                ..default()
            },
            TitanOnly,
        ))
        .id();
    commands.entity(root).add_child(slots);
    for (slot, icon, key) in [
        (
            Slot::Defensive,
            "rui/titan_loadout/defensive/titan_defensive_vortex_menu",
            "Q",
        ),
        (
            Slot::Utility,
            "rui/titan_loadout/tactical/titan_tactical_electric_smoke_menu",
            "E",
        ),
        (
            Slot::Ordnance,
            "rui/titan_loadout/ordnance/multilock_rockets_menu",
            "G",
        ),
    ] {
        let p = slot_column(&mut commands, &ui, icon, key);
        commands.entity(p.icon).insert(SlotIcon(slot));
        commands.entity(p.shade).insert(CooldownShade(slot));
        commands.entity(p.state).insert(SlotText(slot));
        commands.entity(p.label).insert(SlotLabel(slot));
        commands.entity(slots).add_child(p.col);
    }

    // The ammo bar: the weapon's silhouette and its magazine (the game shows no reserve).
    let ammo = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: vh(AMMO_X),
                top: vh(SLOTS_Y),
                height: vh(AMMO_H),
                align_items: AlignItems::Center,
                column_gap: vh(1.2),
                padding: UiRect::right(vh(1.5)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.3)),
            AmmoBar,
        ))
        .id();
    commands.entity(root).add_child(ammo);
    let frame = commands
        .spawn(Node {
            width: vh(AMMO_W),
            height: vh(AMMO_H),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .id();
    let wicon_box = commands
        .spawn(Node {
            width: vh(10.6),
            height: vh(4.2),
            ..default()
        })
        .id();
    let wicon = commands
        .spawn((
            Node::default(),
            ImageNode {
                color: Color::NONE,
                ..default()
            },
            WeaponIcon,
            FitIcon,
        ))
        .id();
    commands.entity(wicon_box).add_child(wicon);
    commands.entity(frame).add_child(wicon_box);
    let count = text(&mut commands, "", &ui.bold_font, 3.5, WHITE);
    commands.entity(count).insert(AmmoCount);
    commands.entity(ammo).add_children(&[frame, count]);

    // --- Top left: the Frontier Defense wave panel (wave title, enemies, difficulty, the
    // blue bar with its percentage), laid out like the game's ---
    let wave = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: vh(FD_X),
            top: vh(FD_Y),
            width: vh(52.0),
            height: vh(8.0),
            ..default()
        })
        .id();
    commands.entity(root).add_child(wave);
    let wave_title = text(&mut commands, "", &ui.bold_font, 2.0, WHITE);
    commands.entity(wave_title).insert((
        WaveTitle,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            ..default()
        },
    ));
    let wave_sub = text(&mut commands, "", &ui.font, 1.6, WHITE);
    commands.entity(wave_sub).insert((
        WaveSub,
        Node {
            position_type: PositionType::Absolute,
            left: vh(16.2),
            top: vh(0.5),
            ..default()
        },
    ));
    let diff = text(&mut commands, "REGULAR", &ui.font, 1.9, WHITE);
    commands.entity(diff).insert(Node {
        position_type: PositionType::Absolute,
        left: vh(38.0),
        top: vh(0.2),
        ..default()
    });
    let bar = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: vh(3.5),
                width: vh(24.0),
                height: vh(3.9),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.08, 0.12, 0.45)),
        ))
        .id();
    let bar_top = commands
        .spawn((
            Node {
                height: vh(0.7),
                width: percent(100),
                ..default()
            },
            BackgroundColor(Color::srgb(0.35, 0.6, 0.87)),
        ))
        .id();
    let bar_body = commands
        .spawn(Node {
            flex_grow: 1.0,
            align_items: AlignItems::Center,
            padding: UiRect::left(vh(4.0)),
            ..default()
        })
        .id();
    let bar_fill = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                bottom: px(0),
                width: percent(100),
                ..default()
            },
            BackgroundColor(Color::srgb(0.145, 0.275, 0.41)),
            WaveFill,
        ))
        .id();
    let bar_pct = text(&mut commands, "", &ui.bold_font, 2.2, WHITE);
    commands.entity(bar_pct).insert(WavePct);
    // The percent sign is smaller than the digits.
    let pct_sign = commands
        .spawn((
            TextSpan::new("%"),
            TextFont {
                font: ui.bold_font.clone(),
                font_size: 16.0,
                ..default()
            },
            TextColor(WHITE),
            VhText(1.5),
        ))
        .id();
    commands.entity(bar_pct).add_child(pct_sign);
    commands.entity(bar_body).add_children(&[bar_fill, bar_pct]);
    commands.entity(bar).add_children(&[bar_top, bar_body]);
    // The blue triangle badge over the bar's left end (the game's wave icon: a harvester
    // glyph on a blue triangle).
    let badge = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: vh(-2.8),
                top: vh(2.95),
                width: vh(5.3),
                height: vh(4.2),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexEnd,
                ..default()
            },
            ImageNode {
                image: ui.triangle.clone(),
                image_mode: NodeImageMode::Stretch,
                ..default()
            },
        ))
        .id();
    let glyph = commands
        .spawn((
            Node {
                width: vh(3.4),
                height: vh(3.0),
                margin: UiRect::bottom(vh(0.3)),
                ..default()
            },
            ImageNode {
                image: ui.harvester.clone(),
                image_mode: NodeImageMode::Stretch,
                ..default()
            },
        ))
        .id();
    commands.entity(badge).add_child(glyph);
    // The wave's enemies as type icons under the bar: one blue triangle with the Titan
    // silhouette per Titan.
    let types = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: vh(0.5),
            top: vh(8.6),
            column_gap: vh(1.0),
            ..default()
        })
        .id();
    for i in 0..6 {
        let e = commands
            .spawn((
                Node {
                    display: Display::None,
                    width: vh(3.4),
                    height: vh(3.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::FlexEnd,
                    overflow: Overflow::visible(),
                    ..default()
                },
                ImageNode {
                    image: ui.triangle.clone(),
                    image_mode: NodeImageMode::Stretch,
                    ..default()
                },
                WaveType(i),
            ))
            .id();
        let sil = image(
            &mut commands,
            &ui,
            "rui/hud/scoreboard/status_titan",
            Node {
                width: vh(1.6),
                height: vh(2.7),
                margin: UiRect::bottom(vh(0.1)),
                ..default()
            },
            WHITE,
        );
        commands.entity(e).add_child(sil);
        commands.entity(types).add_child(e);
    }
    commands
        .entity(wave)
        .add_children(&[wave_title, wave_sub, diff, bar, badge, types]);

    // --- Centre: the wave splash (announcement_center): "WAVE N" over a dark band with the
    // wave bar under it at the start of a wave, "WAVE COMPLETE" when it is cleared.
    // Measured on 1080p frames: text cap 60 px at y 275 (wave) / 110 px (complete), the
    // band 600 px wide from y 260 to 390, the five-segment bar 340 px wide at y 360. ---
    let splash = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                left: px(0),
                right: px(0),
                top: vh(23.1),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
            WaveSplash,
        ))
        .id();
    commands.entity(root).add_child(splash);
    // "WAVE N": a dark band 46 x 12.5 vh with the text and the five wave segments in it.
    let band = commands
        .spawn((
            Node {
                min_width: vh(30.0),
                height: vh(12.5),
                padding: UiRect {
                    left: vh(3.0),
                    right: vh(3.0),
                    top: vh(0.8),
                    bottom: px(0),
                },
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            // The game blends its UI in gamma space; this UI blends in linear light, where a
            // dark overlay needs more alpha for the same look (0.66 there is about 0.88 here).
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.88)),
            WaveSplashBar(true),
        ))
        .id();
    // The band fades out over 15 vh at both ends: strips of falling alpha outside its edges
    // (strips, since image and gradient nodes are far fainter than their alpha here).
    for left in [true, false] {
        let mut fade = Node {
            position_type: PositionType::Absolute,
            top: px(0),
            bottom: px(0),
            width: vh(15.0),
            ..default()
        };
        if left {
            fade.right = percent(100);
            fade.flex_direction = FlexDirection::RowReverse;
        } else {
            fade.left = percent(100);
        }
        let fade = commands.spawn(fade).id();
        for i in 0..15 {
            let t = 1.0 - (i as f32 + 0.5) / 15.0;
            let t = t * t * (3.0 - 2.0 * t);
            let strip = commands
                .spawn((
                    Node {
                        width: vh(1.0),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.88 * t)),
                ))
                .id();
            commands.entity(fade).add_child(strip);
        }
        commands.entity(band).add_child(fade);
    }
    let splash_text = spawn_splash_text(&mut commands, &ui, 7.3);
    let segs = commands
        .spawn(Node {
            column_gap: vh(0.55),
            ..default()
        })
        .id();
    for i in 0..5 {
        let e = commands
            .spawn((
                Node {
                    width: vh(5.7),
                    height: vh(0.95),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.5, 0.5, 0.5, 0.6)),
                WaveSplashSeg(i),
            ))
            .id();
        commands.entity(segs).add_child(e);
    }
    commands.entity(band).add_children(&[splash_text, segs]);
    // "WAVE COMPLETE": a light band 74 x 5.5 vh with the text alone.
    let done = commands
        .spawn((
            Node {
                min_width: vh(74.0),
                height: vh(5.0),
                margin: UiRect::top(vh(0.5)),
                padding: UiRect::axes(vh(1.5), px(0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.82, 0.88, 0.97, 0.42)),
            WaveSplashBar(false),
        ))
        .id();
    let done_text = spawn_splash_text(&mut commands, &ui, 8.35);
    commands.entity(done).add_child(done_text);
    // The wave's name under the intro band, in amber quotes (the map's FD_WAVE_N string).
    let quip = text(
        &mut commands,
        "",
        &ui.bold_font,
        2.2,
        Color::srgb(1.0, 0.64, 0.07),
    );
    commands.entity(quip).insert((
        WaveQuip,
        Node {
            margin: UiRect::top(vh(2.3)),
            ..default()
        },
        TextShadow {
            offset: Vec2::splat(1.5),
            color: Color::srgba(0.0, 0.0, 0.0, 0.8),
        },
    ));
    commands.entity(splash).add_children(&[band, done, quip]);

    // --- Centre: banner and prompts ---
    let centre = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: percent(30),
            left: px(0),
            right: px(0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: vh(1.0),
            ..default()
        })
        .id();
    commands.entity(root).add_child(centre);
    let banner = text(&mut commands, "", &ui.title_font, 4.4, DOOM);
    commands.entity(banner).insert(Banner);
    commands.entity(centre).add_child(banner);
    let prompt = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: percent(22),
            left: px(0),
            right: px(0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .id();
    let ptext = text(&mut commands, "", &ui.bold_font, 2.0, WHITE);
    commands.entity(ptext).insert(Prompt);
    commands.entity(prompt).add_child(ptext);
    commands.entity(root).add_child(prompt);
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_hud(
    game: Res<Game>,
    control: Res<Control>,
    titans: Query<(
        &PlayerTitan,
        &TitanHealth,
        &Ordnance,
        &TitanCore,
        &Vortex,
        Option<&crate::weapons::Weapon>,
        &Titanfall,
    )>,
    pilots: Query<(
        &PlayerPilot,
        &crate::pilotctl::PilotHealth,
        Option<&crate::pilotweapon::PilotLoadout>,
    )>,
    enemies: Query<&Enemy>,
    mut segs: Query<
        (&HealthSeg, &mut Node, &mut BackgroundColor),
        (
            Without<ShieldFill>,
            Without<DashPip>,
            Without<CooldownShade>,
        ),
    >,
    mut shield: Query<
        &mut Node,
        (
            With<ShieldFill>,
            Without<HealthSeg>,
            Without<DashPip>,
            Without<CooldownShade>,
        ),
    >,
    mut doom: Query<
        (&mut Node, &mut Visibility),
        (
            With<DoomFill>,
            Without<HealthSeg>,
            Without<ShieldFill>,
            Without<DashPip>,
            Without<CooldownShade>,
            Without<TitanOnly>,
            Without<HudRoot>,
        ),
    >,
    mut pips: Query<
        (&DashPip, &mut Node, &mut BackgroundColor),
        (
            Without<HealthSeg>,
            Without<ShieldFill>,
            Without<CooldownShade>,
        ),
    >,
    mut shades: Query<
        (&CooldownShade, &mut Node),
        (
            Without<HealthSeg>,
            Without<ShieldFill>,
            Without<DashPip>,
            Without<DoomFill>,
        ),
    >,
    mut texts: Query<
        (
            &mut Text,
            &mut TextColor,
            Option<&SlotText>,
            Option<&MeterText>,
            Option<&MeterKeyText>,
            Option<&AmmoCount>,
            Option<&WaveTitle>,
            Option<&WaveSub>,
            Option<&Banner>,
            Option<&Prompt>,
            Option<&WavePct>,
            Option<&WaveSplashText>,
        ),
        Without<WaveQuip>,
    >,
    (mut quip, mut meter_pct, strings, map): (
        Query<&mut Text, With<WaveQuip>>,
        Query<&mut TextSpan, With<MeterPct>>,
        Res<crate::pilotweapon::Strings>,
        Res<crate::ui::MapChoice>,
    ),
    (
        kit_view,
        ui,
        mut wicon,
        mut ammo_bar,
        mut meter_fill,
        mut meter_key,
        mut meter_icon,
        mut wave_fill,
        mut silhouette,
        mut plus,
    ): (
        Option<Res<crate::titankit::KitView>>,
        Res<UiAssets>,
        Query<&mut ImageNode, (With<WeaponIcon>, Without<CoreIcon>)>,
        Query<
            &mut Node,
            (
                With<AmmoBar>,
                Without<HealthSeg>,
                Without<ShieldFill>,
                Without<DashPip>,
                Without<CooldownShade>,
                Without<DoomFill>,
                Without<MeterKey>,
            ),
        >,
        Query<&mut BorderGradient, With<MeterFill>>,
        Query<
            &mut Node,
            (
                With<MeterKey>,
                Without<HealthSeg>,
                Without<ShieldFill>,
                Without<DashPip>,
                Without<CooldownShade>,
                Without<DoomFill>,
            ),
        >,
        Query<&mut ImageNode, (With<CoreIcon>, Without<WeaponIcon>)>,
        Query<
            &mut Node,
            (
                With<WaveFill>,
                Without<HealthSeg>,
                Without<ShieldFill>,
                Without<DashPip>,
                Without<CooldownShade>,
                Without<DoomFill>,
                Without<MeterKey>,
                Without<AmmoBar>,
            ),
        >,
        Query<
            &mut Visibility,
            (
                With<MeterSilhouette>,
                Without<HudRoot>,
                Without<TitanOnly>,
                Without<DoomFill>,
            ),
        >,
        Query<
            &mut Node,
            (
                With<MeterPlus>,
                Without<HealthSeg>,
                Without<ShieldFill>,
                Without<DashPip>,
                Without<CooldownShade>,
                Without<DoomFill>,
                Without<MeterKey>,
                Without<AmmoBar>,
                Without<WaveFill>,
            ),
        >,
    ),
    (time, mut splash, mut splash_nodes, mut splash_segs, mut wave_types, mut splash_bar): (
        Res<Time>,
        Local<(Option<GameState>, f32)>,
        Query<
            &mut Node,
            (
                With<WaveSplash>,
                Without<HealthSeg>,
                Without<ShieldFill>,
                Without<DashPip>,
                Without<CooldownShade>,
                Without<DoomFill>,
                Without<MeterKey>,
                Without<AmmoBar>,
                Without<WaveFill>,
                Without<MeterPlus>,
            ),
        >,
        Query<(&WaveSplashSeg, &mut BackgroundColor), (Without<HealthSeg>, Without<DashPip>)>,
        Query<
            (&WaveType, &mut Node),
            (
                Without<WaveSplash>,
                Without<HealthSeg>,
                Without<ShieldFill>,
                Without<DashPip>,
                Without<CooldownShade>,
                Without<DoomFill>,
                Without<MeterKey>,
                Without<AmmoBar>,
                Without<WaveFill>,
                Without<MeterPlus>,
            ),
        >,
        Query<
            (&WaveSplashBar, &mut Node),
            (
                Without<WaveType>,
                Without<WaveSplash>,
                Without<HealthSeg>,
                Without<ShieldFill>,
                Without<DashPip>,
                Without<CooldownShade>,
                Without<DoomFill>,
                Without<MeterKey>,
                Without<AmmoBar>,
                Without<WaveFill>,
                Without<MeterPlus>,
            ),
        >,
    ),
) {
    let Ok((titan, health, ord, core, vortex, weapon, titanfall)) = titans.single() else {
        return;
    };
    let pilot = pilots.single().ok();
    let in_titan = *control == Control::Titan && health.dead_for.is_none();
    let gun = match (
        *control,
        weapon,
        pilot.and_then(|p| p.2.and_then(|l| l.active())),
    ) {
        (Control::Pilot, _, Some(w)) => Some((w.ammo, w.reloading(), w.def.name.as_str())),
        (_, Some(w), _) => Some((w.ammo, w.reloading(), w.def.name.as_str())),
        _ => None,
    };

    // The meter: what it fills with, whether it is ready, its icon and caption.
    let doomed = health.v.doomed.is_some();
    let (frac, ready, icon, caption, key, color) = if in_titan {
        let ready = core.state == CoreState::Building && core.meter >= 1.0 || core.active();
        let caption = match core.state {
            CoreState::Building if ready => "100%".to_string(),
            CoreState::Building => format!("{:.0}%", core.meter * 100.0),
            CoreState::Charging(_) => "CHARGING".to_string(),
            CoreState::Active(_) => "ACTIVE".to_string(),
        };
        let icon = kit_view
            .as_ref()
            .map(|k| k.core_icon.clone())
            .unwrap_or_else(|| "rui/hud/titan_core".to_string());
        (
            if core.active() { 1.0 } else { core.meter },
            ready,
            icon,
            caption,
            "V",
            if ready { METER_AMBER } else { METER_BLUE },
        )
    } else if health.dead_for.is_some() || titanfall.rebuilding {
        let ready = titanfall.cooldown <= 0.0 && !titanfall.busy();
        let frac = 1.0 - titanfall.cooldown / crate::pilotctl::TITAN_REBUILD_SECS;
        if ready {
            (
                1.0,
                true,
                "rui/hud/earn_meter/titan_ready".to_string(),
                "TITANFALL".to_string(),
                "T",
                METER_BLUE,
            )
        } else {
            (
                frac,
                false,
                "rui/hud/earn_meter/titan_ready".to_string(),
                format!("{:.0}%", frac * 100.0),
                "T",
                METER_BLUE,
            )
        }
    } else {
        let frac = health.v.fraction();
        (
            frac,
            false,
            "rui/hud/earn_meter/titan_follow".to_string(),
            format!("{:.0}%", frac * 100.0),
            "",
            if doomed { DOOM } else { METER_BLUE },
        )
    };
    if let Ok(mut g) = meter_fill.single_mut() {
        if let Some(Gradient::Conic(c)) = g.0.first_mut() {
            let want = meter_gradient(frac, color);
            if c.stops != want.stops {
                c.stops = want.stops;
            }
        }
    }
    if let Ok(mut n) = meter_key.single_mut() {
        set(
            &mut n,
            |n| &mut n.display,
            if ready && !key.is_empty() {
                Display::Flex
            } else {
                Display::None
            },
        );
    }
    if let Ok(mut v) = silhouette.single_mut() {
        v.set_if_neq(if in_titan {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
    for mut n in &mut plus {
        set(
            &mut n,
            |n| &mut n.display,
            if in_titan {
                Display::None
            } else {
                Display::Flex
            },
        );
    }
    if let Ok(mut img) = meter_icon.single_mut() {
        if let Some(n) = ui.image(&icon) {
            if img.image != n.image || img.rect != n.rect {
                img.image = n.image;
                img.rect = n.rect;
            }
        }
        // White in every state (the game's core-ready cockpit shows it white on the amber ring).
        let want = WHITE;
        if img.color != want {
            img.color = want;
        }
    }
    // The ammo bar sits after the slots (the Titan has one more).
    if let Ok(mut n) = ammo_bar.single_mut() {
        set(
            &mut n,
            |n| &mut n.left,
            vh(if in_titan {
                AMMO_X + SLOT_SIZE + SLOT_GAP
            } else {
                AMMO_X
            }),
        );
    }
    if let Ok(mut img) = wicon.single_mut() {
        if let Some(n) = gun
            .and_then(|g| ui.weapon_icons.get(g.2))
            .and_then(|p| ui.image(p))
        {
            if img.image != n.image || img.rect != n.rect {
                img.image = n.image;
                img.rect = n.rect;
                img.color = WHITE;
            }
        }
    }

    // Health and shield.
    for (seg, mut node, mut bg) in &mut segs {
        let fill = if doomed {
            0.0
        } else {
            ((health.v.health - seg.0 as f32 * crate::combat::SEGMENT) / crate::combat::SEGMENT)
                .clamp(0.0, 1.0)
        };
        set(&mut node, |n| &mut n.width, percent(fill * 100.0));
        bg.0 = if health.hurt > 0.0 {
            Color::srgb(1.0, 0.55, 0.45)
        } else {
            HEALTH_CYAN
        };
    }
    if let Ok(mut n) = shield.single_mut() {
        set(
            &mut n,
            |n| &mut n.width,
            percent(health.v.shield / health.v.max_shield.max(1.0) * 100.0),
        );
    }
    if let Ok((mut n, mut v)) = doom.single_mut() {
        v.set_if_neq(if doomed {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        set(
            &mut n,
            |n| &mut n.width,
            percent(health.v.fraction() * 100.0),
        );
    }
    for (pip, mut node, mut bg) in &mut pips {
        let fill = ((titan.state.power - pip.0 as f32 * 50.0) / 50.0).clamp(0.0, 1.0);
        set(&mut node, |n| &mut n.width, percent(fill * 100.0));
        bg.0 = if fill >= 1.0 { PIP_BLUE } else { DIM };
    }
    // Cooldowns: the shade covers what is still recharging.
    for (s, mut node) in &mut shades {
        let left = match (s.0, kit_view.as_ref()) {
            (Slot::Ordnance, Some(k)) => k.ord_left,
            (Slot::Defensive, Some(k)) => k.def_left,
            (Slot::Utility, Some(k)) => k.util_left,
            (Slot::Utility, None) => 0.0,
            (Slot::Ordnance, None) => {
                if ord.ready() || ord.charging {
                    0.0
                } else {
                    1.0 - ord.fraction()
                }
            }
            (Slot::Defensive, None) => 1.0 - vortex.fraction(),
        };
        set(
            &mut node,
            |n| &mut n.height,
            percent(left.clamp(0.0, 1.0) * 100.0),
        );
    }
    let alive = enemies.iter().filter(|e| e.alive() && !e.infantry).count();
    // The splash: on a state change into Playing or Intermission (from a wave), restart it.
    if splash.0 != Some(game.state) {
        let from_wave = splash.0 == Some(GameState::Playing);
        splash.0 = Some(game.state);
        splash.1 = match game.state {
            GameState::Playing => 0.0,
            GameState::Intermission if from_wave => 0.0,
            _ => SPLASH_SECS,
        };
    } else {
        splash.1 += time.delta_secs();
    }
    let splash_on = splash.1 < SPLASH_SECS
        && matches!(game.state, GameState::Playing | GameState::Intermission);
    for mut n in &mut splash_nodes {
        set(
            &mut n,
            |n| &mut n.display,
            if splash_on {
                Display::Flex
            } else {
                Display::None
            },
        );
    }
    for (bar, mut n) in &mut splash_bar {
        set(
            &mut n,
            |n| &mut n.display,
            if bar.0 == (game.state == GameState::Playing) {
                Display::Flex
            } else {
                Display::None
            },
        );
    }
    for (seg, mut bg) in &mut splash_segs {
        let lit = (seg.0 as u32) < game.wave.max(1).min(5);
        let want = if lit {
            Color::srgb(1.0, 0.72, 0.1)
        } else {
            Color::srgba(0.5, 0.5, 0.5, 0.6)
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    for (t, mut n) in &mut wave_types {
        set(
            &mut n,
            |n| &mut n.display,
            if (t.0 as u32) < game.wave_size {
                Display::Flex
            } else {
                Display::None
            },
        );
    }
    // "44%" is drawn as the digits with a smaller "%" after them.
    let (caption, caption_pct) = match caption.strip_suffix('%') {
        Some(c) => (c.to_string(), "%"),
        None => (caption.clone(), ""),
    };
    if let Ok(mut t) = meter_pct.single_mut() {
        if t.0 != caption_pct {
            t.0 = caption_pct.to_string();
        }
    }
    if let Ok(mut t) = quip.single_mut() {
        let key = format!("{}_FD_WAVE_{}", map.0.to_uppercase(), game.wave.clamp(1, 5));
        let want = match strings.0.get(&key) {
            Some(name) if game.state == GameState::Playing => format!("\"{name}\""),
            _ => String::new(),
        };
        if t.0 != want {
            t.0 = want;
        }
    }
    let wave_frac = if game.state == GameState::Playing && game.wave_size > 0 {
        (alive as f32 / game.wave_size as f32).clamp(0.0, 1.0)
    } else {
        1.0
    };
    if let Ok(mut n) = wave_fill.single_mut() {
        set(&mut n, |n| &mut n.width, percent(wave_frac * 100.0));
    }
    for (
        mut t,
        mut col,
        slot,
        meter_t,
        meter_k,
        count,
        wave_title,
        wave_sub,
        banner,
        prompt,
        wave_pct,
        splash_t,
    ) in &mut texts
    {
        let s: String = if let Some(slot) = slot {
            match slot.0 {
                Slot::Ordnance if kit_view.is_some() => kit_view.as_ref().unwrap().ord_text.clone(),
                Slot::Defensive if kit_view.is_some() => {
                    kit_view.as_ref().unwrap().def_text.clone()
                }
                Slot::Utility if kit_view.is_some() => kit_view.as_ref().unwrap().util_text.clone(),
                Slot::Ordnance if ord.charging => format!("{}", ord.locks.len()),
                Slot::Ordnance if !ord.ready() => {
                    format!("{:.0}", (ord.cooldown + ord.delay.max(0.0)).ceil())
                }
                Slot::Defensive if vortex.active => format!("{}", vortex.caught + vortex.absorbed),
                _ => String::new(),
            }
        } else if meter_t.is_some() {
            caption.clone()
        } else if meter_k.is_some() {
            key.to_string()
        } else if count.is_some() {
            match gun {
                Some((_, true, _)) => "--".into(),
                Some((ammo, false, _)) => format!("{ammo}"),
                None => String::new(),
            }
        } else if wave_title.is_some() {
            format!("Wave {}", game.wave.max(1))
        } else if wave_sub.is_some() {
            match game.state {
                GameState::Intermission if game.wave == 0 => "Get ready".into(),
                GameState::Intermission => "Complete".into(),
                _ if splash.1 < SPLASH_SECS => "Incoming".into(),
                _ => format!("Enemies: {alive}"),
            }
        } else if wave_pct.is_some() {
            format!("{:.0}", wave_frac * 100.0)
        } else if splash_t.is_some() {
            match game.state {
                GameState::Intermission => "WAVE COMPLETE".into(),
                _ => format!("WAVE {}", game.wave.max(1)),
            }
        } else if banner.is_some() {
            if health.dead_for.is_some_and(|t| t < 4.0) {
                "TITAN DESTROYED".into()
            } else if doomed && in_titan {
                col.0 = DOOM.with_alpha(
                    0.6 + 0.4 * ((health.v.doomed.unwrap_or(0.0) * 6.0).sin() * 0.5 + 0.5),
                );
                "WARNING: TITAN DOOMED".into()
            } else {
                String::new()
            }
        } else if prompt.is_some() {
            match *control {
                Control::Pilot => {
                    let near = pilot.is_some_and(|(p, _, _)| {
                        (titan.state.pos - p.state.pos).truncate().length() < 260.0
                    });
                    if near && health.dead_for.is_none() {
                        "[X]  EMBARK".into()
                    } else if titanfall.rebuilding && titanfall.cooldown > 0.0 {
                        format!("TITAN READY IN {:.0}", titanfall.cooldown.ceil())
                    } else if titanfall.cooldown <= 0.0
                        && !titanfall.busy()
                        && (titanfall.rebuilding || health.dead_for.is_some())
                    {
                        "[T]  TITANFALL".into()
                    } else {
                        String::new()
                    }
                }
                Control::Disembark(_) => "DISEMBARKING".into(),
                Control::Embark(_) => "EMBARKING".into(),
                // The doomed eject interface: three presses of the use button.
                Control::Titan if doomed && titanfall.eject.is_none() => {
                    format!(
                        "[X] x3  EJECT   {}/{}",
                        titanfall.eject_presses,
                        crate::pilotctl::EJECT_PRESSES
                    )
                }
                Control::Titan => String::new(),
            }
        } else {
            continue;
        };
        if t.0 != s {
            t.0 = s;
        }
    }
}

/// HUD visibility, the core icon and the damage flash.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_hud_state(
    game: Res<Game>,
    menu: Res<crate::ui::Menu>,
    mode: Res<CameraMode>,
    control: Res<Control>,
    titans: Query<(&TitanHealth, Option<&crate::weapons::Weapon>)>,
    pilots: Query<(
        &crate::pilotctl::PilotHealth,
        Option<&crate::pilotweapon::PilotLoadout>,
    )>,
    mut root: Query<&mut Visibility, With<HudRoot>>,
    mut titan_only: Query<
        &mut Visibility,
        (With<TitanOnly>, Without<HudRoot>, Without<HurtOverlay>),
    >,
    mut hurt: Query<
        (&mut Visibility, &mut BackgroundGradient),
        (With<HurtOverlay>, Without<HudRoot>, Without<TitanOnly>),
    >,
    mut charge_hud: Query<
        &mut Visibility,
        (
            With<ChargeHud>,
            Without<HudRoot>,
            Without<TitanOnly>,
            Without<HurtOverlay>,
        ),
    >,
    mut charge_pips: Query<(&ChargePip, &mut Node), Without<ChargePipFill>>,
    mut charge_fills: Query<(&ChargePipFill, &mut Node, &mut BackgroundColor), Without<ChargePip>>,
    hidden: Res<HudHidden>,
) {
    let Ok((health, weapon)) = titans.single() else {
        return;
    };
    let (pilot, loadout) = pilots
        .single()
        .map(|(h, l)| (Some(h), l))
        .unwrap_or((None, None));
    let in_game = game.in_play() && !menu.paused && *mode != CameraMode::Free && !hidden.0;
    if let Ok(mut v) = root.single_mut() {
        v.set_if_neq(if in_game {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let in_titan = *control == Control::Titan && health.dead_for.is_none();
    for mut v in &mut titan_only {
        v.set_if_neq(if in_titan {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }

    // The Pilot's damage bloom (DamageOverlayUpdate, cl_pilot_health_hud.gnut): with 100 health
    // the display range is 0.48..1.0; the bloom brightens from full health to 84% (colour 0 to
    // 220) and reddens below that, down to 48%. The bloom art isn't recreated: here those two
    // ramps drive the red vignette's alpha (0.22 and 0.15, judged by eye), plus the hit flash.
    let hurt_amount = match *control {
        Control::Pilot => pilot
            .map(|h| {
                let frac = h.health / crate::pilotctl::PILOT_HEALTH;
                let bright = ((1.0 - frac) / (1.0 - 0.844)).clamp(0.0, 1.0);
                let red = ((0.844 - frac) / (0.844 - 0.48)).clamp(0.0, 1.0);
                h.hurt * 4.0 + bright * 0.22 + red * 0.15
            })
            .unwrap_or(0.0),
        _ => health.hurt * 2.0,
    };
    if let Ok((mut v, mut g)) = hurt.single_mut() {
        v.set_if_neq(if hurt_amount > 0.01 && in_game {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if let Some(Gradient::Radial(r)) = g.0.first_mut() {
            if let Some(stop) = r.stops.last_mut() {
                stop.color = Color::srgba(0.75, 0.04, 0.0, hurt_amount.clamp(0.0, 0.5));
            }
        }
    }

    // Charge meter for the weapon in hand.
    let charge = match *control {
        Control::Pilot => loadout
            .and_then(|l| l.active())
            .map(|g| (g.def.charge.clone(), g.charge)),
        Control::Titan => weapon.map(|w| (w.def.charge.clone(), w.charge)),
        _ => None,
    }
    .filter(|(def, _)| def.time > 0.0);
    if let Ok(mut v) = charge_hud.single_mut() {
        v.set_if_neq(if charge.is_some() && in_game {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    if let Some((def, c)) = charge {
        let n = def.levels.clamp(1, CHARGE_PIPS as u32) as usize;
        let full = c.frac >= 1.0;
        for (pip, mut node) in &mut charge_pips {
            set(
                &mut node,
                |n| &mut n.display,
                if pip.0 < n {
                    Display::Flex
                } else {
                    Display::None
                },
            );
            set(&mut node, |n| &mut n.width, vh(CHARGE_WIDTH / n as f32));
        }
        for (fill, mut node, mut bg) in &mut charge_fills {
            let f = (c.frac * n as f32 - fill.0 as f32).clamp(0.0, 1.0);
            set(&mut node, |n| &mut n.width, percent(f * 100.0));
            bg.0 = if full {
                AMBER
            } else if f >= 1.0 {
                WHITE
            } else {
                DIM
            };
        }
    }
}

/// Hit confirmation: when an enemy takes damage, flash the marker and beep (hitbeep2).
pub fn hit_feedback(
    mut commands: Commands,
    time: Res<Time>,
    enemies: Query<&Enemy>,
    mut timer: Local<f32>,
    mut beep_cd: Local<f32>,
    mut markers: Query<(&mut Visibility, &mut ImageNode), With<HitMarker>>,
) {
    let dt = time.delta_secs();
    *timer -= dt;
    *beep_cd -= dt;
    let hit = enemies
        .iter()
        .filter(|e| e.active && e.since_hit <= dt * 1.01)
        .count();
    let killed = enemies.iter().any(|e| e.just_died);
    if hit > 0 {
        *timer = HIT_MARKER_TIME;
        if *beep_cd <= 0.0 {
            crate::audio::cue(&mut commands, crate::audio::Cue::HitBeep, None);
            *beep_cd = 0.07;
        }
    }
    for (mut v, mut img) in &mut markers {
        v.set_if_neq(if *timer > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        img.color = if killed {
            DOOM
        } else {
            Color::WHITE.with_alpha((*timer / HIT_MARKER_TIME * 2.0).clamp(0.0, 1.0))
        };
    }
}

// ---- Smart ammo (Smart Pistol / Archer) ----
//
// The game's smart ammo HUD is engine RUI, so it is recreated: the Smart Pistol's search cone
// as a circle around the crosshair (`smart_ammo_hud_type smart_pistol`), and on each target
// four `smart_ammo_corner` brackets that close in while a lock forms, with the lock count once
// one is full.

#[derive(Component)]
pub struct SmartCircle;
#[derive(Component)]
pub struct SmartMarker(usize);
#[derive(Component)]
pub struct SmartCorner(usize, usize);
#[derive(Component)]
pub struct SmartCount(usize);
const SMART_MARKERS: usize = 12;

fn spawn_smart_hud(commands: &mut Commands, ui: &UiAssets, root: Entity) {
    let circle = image(
        commands,
        ui,
        "rui/hud/crosshairs/crosshair_circle",
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: percent(50),
            width: vh(20.0),
            height: vh(20.0),
            margin: UiRect {
                left: vh(-10.0),
                top: vh(-10.0),
                ..default()
            },
            ..default()
        },
        DIM,
    );
    commands
        .entity(circle)
        .insert((SmartCircle, Visibility::Hidden));
    commands.entity(root).add_child(circle);
    for i in 0..SMART_MARKERS {
        let holder = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Visibility::Hidden,
                SmartMarker(i),
            ))
            .id();
        for (k, (fx, fy)) in [(false, false), (true, false), (false, true), (true, true)]
            .into_iter()
            .enumerate()
        {
            let e = image(
                commands,
                ui,
                "rui/hud/crosshairs/smart_ammo_corner",
                Node {
                    position_type: PositionType::Absolute,
                    width: vh(1.6),
                    height: vh(1.6),
                    ..default()
                },
                WHITE,
            );
            commands.entity(e).insert(SmartCorner(i, k));
            if let Ok(mut c) = commands.get_entity(e) {
                c.entry::<ImageNode>().and_modify(move |mut img| {
                    img.flip_x = fx;
                    img.flip_y = fy;
                });
            }
            commands.entity(holder).add_child(e);
        }
        let t = text(commands, "", &ui.bold_font, 1.6, AMBER);
        commands.entity(t).insert((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            SmartCount(i),
        ));
        commands.entity(holder).add_child(t);
        commands.entity(root).add_child(holder);
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update_smart_hud(
    control: Res<Control>,
    mode: Res<CameraMode>,
    loadouts: Query<&crate::pilotweapon::PilotLoadout, With<PlayerPilot>>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection), With<crate::player::MainCamera>>,
    enemies: Query<&Enemy>,
    mut circle: Query<
        (&mut Node, &mut Visibility),
        (
            With<SmartCircle>,
            Without<SmartMarker>,
            Without<SmartCorner>,
            Without<SmartCount>,
        ),
    >,
    mut markers: Query<
        (&SmartMarker, &mut Node, &mut Visibility),
        (
            Without<SmartCircle>,
            Without<SmartCorner>,
            Without<SmartCount>,
        ),
    >,
    mut corners: Query<
        (&SmartCorner, &mut Node),
        (
            Without<SmartCircle>,
            Without<SmartMarker>,
            Without<SmartCount>,
        ),
    >,
    mut counts: Query<
        (&SmartCount, &mut Node, &mut Text),
        (
            Without<SmartCircle>,
            Without<SmartMarker>,
            Without<SmartCorner>,
        ),
    >,
) {
    let Ok((camera, cam_tf, projection)) = cameras.single() else {
        return;
    };
    let gun = loadouts
        .single()
        .ok()
        .and_then(|l| l.active())
        .filter(|_| *control == Control::Pilot && *mode != CameraMode::Free);
    // The Archer's locks show on its sight screen only (launcherscreen.rs).
    let smart = gun.and_then(|g| {
        g.def
            .smart
            .as_ref()
            .filter(|sd| !sd.launcher_screen)
            .map(|sd| (sd, g.smart.view()))
    });
    let view_h = camera
        .logical_viewport_size()
        .map(|v| v.y)
        .unwrap_or(1080.0);
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => 70f32.to_radians(),
    };
    // Pixels per unit of tan(angle) from the view centre.
    let px_per_tan = view_h * 0.5 / (fov * 0.5).tan();

    if let Ok((mut node, mut vis)) = circle.single_mut() {
        match smart.as_ref().filter(|(sd, _)| sd.hud_circle) {
            Some((sd, _)) => {
                let r = (sd.search_angle * 0.5).to_radians().tan() * px_per_tan;
                node.width = Val::Px(r * 2.0);
                node.height = Val::Px(r * 2.0);
                node.margin = UiRect {
                    left: Val::Px(-r),
                    top: Val::Px(-r),
                    ..default()
                };
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }

    // (screen centre, half size in px, lock fraction, full locks, max)
    let mut spots: Vec<(Vec2, f32, f32, u32, u32)> = Vec::new();
    if let Some((_, locks)) = &smart {
        for l in locks {
            let Ok(e) = enemies.get(l.target) else {
                continue;
            };
            if !e.alive() {
                continue;
            }
            let centre = crate::player::to_bevy(e.pos + Vec3::Z * e.height * 0.5);
            let Ok(p) = camera.world_to_viewport(cam_tf, centre) else {
                continue;
            };
            let dist = cam_tf.translation().distance(centre) / crate::player::UNIT;
            let half = (e.height * 0.55 / dist.max(1.0)) * px_per_tan;
            let full = (l.frac + 1e-4).floor();
            spots.push((
                p,
                half.clamp(view_h * 0.012, view_h * 0.25),
                l.frac - full,
                full as u32,
                l.max,
            ));
        }
    }
    for (m, mut node, mut vis) in &mut markers {
        match spots.get(m.0) {
            Some((p, ..)) => {
                node.left = Val::Px(p.x);
                node.top = Val::Px(p.y);
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
    for (c, mut node) in &mut corners {
        let Some(&(_, half, frac, full, _)) = spots.get(c.0) else {
            continue;
        };
        // Brackets start on the target's box and close to a third of it as the first lock
        // forms; with a lock held they stay closed.
        let spread = if full > 0 { 0.35 } else { 1.0 - 0.65 * frac };
        let d = half * spread;
        let size = view_h * 0.016;
        let (sx, sy) = (
            if c.1 % 2 == 0 { -1.0 } else { 1.0 },
            if c.1 < 2 { -1.0 } else { 1.0 },
        );
        node.left = Val::Px(sx * d - size * 0.5);
        node.top = Val::Px(sy * d - size * 0.5);
    }
    for (c, mut node, mut t) in &mut counts {
        let Some(&(_, half, _, full, max)) = spots.get(c.0) else {
            continue;
        };
        let s = if full > 0 {
            format!("x{full}{}", if full >= max { "" } else { "+" })
        } else {
            String::new()
        };
        if t.0 != s {
            t.0 = s;
        }
        node.left = Val::Px(half * 0.35 + view_h * 0.012);
        node.top = Val::Px(-view_h * 0.012);
    }
}

// ---- Tone's tracker marks ----
//
// cl_tone_hud.gnut: while Tracker Rockets are the ordnance, each enemy carrying tracker marks
// gets a fullscreen `ui/tone_tracker_hud` RUI at its origin plus (world space centre - origin +
// 42 up), shown while the enemy is in sight or sonar-detected, given the mark count. That RUI is
// compiled, so it is recreated from its art: the ring's lower-left third (1 mark), lower two
// thirds (2) and the whole ring in red (3, locked), cut from one 191x183 frame. The art is drawn
// at its own pixel size on a 1080-high canvas (a guess at the RUI's scale).

#[derive(Component)]
pub struct ToneMarker(usize);
#[derive(Component)]
pub struct ToneMarkArt(usize, usize);
const TONE_MARKERS: usize = 8;
/// The ring's frame in 1080p pixels and where each piece sits in it: (path, x, y, w, h).
const TONE_RING: (f32, f32) = (191.0, 183.0);
const TONE_MARK_ART: [(&str, f32, f32, f32, f32); 3] = [
    (
        "rui/hud/tone_tracker_hud/tone_tracker_1marks",
        0.0,
        66.0,
        95.0,
        117.0,
    ),
    (
        "rui/hud/tone_tracker_hud/tone_tracker_2marks",
        0.0,
        66.0,
        191.0,
        117.0,
    ),
    (
        "rui/hud/tone_tracker_hud/tone_tracker_3marks",
        0.0,
        0.0,
        191.0,
        183.0,
    ),
];

fn spawn_tone_hud(commands: &mut Commands, ui: &UiAssets, root: Entity) {
    let v = |px: f32| vh(px / 10.8);
    for i in 0..TONE_MARKERS {
        let holder = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Visibility::Hidden,
                ToneMarker(i),
            ))
            .id();
        for (k, (path, x, y, w, h)) in TONE_MARK_ART.into_iter().enumerate() {
            let node = Node {
                position_type: PositionType::Absolute,
                left: v(x - TONE_RING.0 * 0.5),
                top: v(y - TONE_RING.1 * 0.5),
                width: v(w),
                height: v(h),
                ..default()
            };
            let e = image(commands, ui, path, node, Color::WHITE);
            commands
                .entity(e)
                .insert((ToneMarkArt(i, k + 1), Visibility::Hidden));
            commands.entity(holder).add_child(e);
        }
        commands.entity(root).add_child(holder);
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update_tone_hud(
    control: Res<Control>,
    mode: Res<CameraMode>,
    active: Res<crate::titankit::ActiveKit>,
    status: Res<crate::pilotability::PilotStatus>,
    world: Res<crate::player::Collision>,
    kits: Query<&crate::titankit::KitState, With<PlayerTitan>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::player::MainCamera>>,
    enemies: Query<&Enemy>,
    mut markers: Query<(&ToneMarker, &mut Node, &mut Visibility), Without<ToneMarkArt>>,
    mut art: Query<(&ToneMarkArt, &mut Visibility), Without<ToneMarker>>,
) {
    let mut spots: Vec<(Vec2, usize)> = Vec::new();
    let shown = *control == Control::Titan
        && *mode != CameraMode::Free
        && active.ord == crate::titankit::OrdKind::Tracker;
    if let (true, Ok(kit), Ok((camera, cam_tf))) = (shown, kits.single(), cameras.single()) {
        let eye = crate::player::from_bevy(cam_tf.translation());
        for (target, marks) in &kit.locks {
            let Ok(e) = enemies.get(*target) else {
                continue;
            };
            if !e.alive() || marks.is_empty() {
                continue;
            }
            let at = e.pos + Vec3::Z * (e.height * 0.5 + 42.0);
            let sonar = status
                .pulse
                .is_some_and(|(p, _)| e.pos.distance(p) < crate::titankit::SONAR_PULSE_RADIUS);
            let to = at - eye;
            let seen = world
                .0
                .raycast(
                    tf_sim::glam::Vec3::from(eye.to_array()),
                    tf_sim::glam::Vec3::from(to.normalize_or_zero().to_array()),
                    to.length() - e.radius,
                )
                .is_none();
            if !(seen || sonar) {
                continue;
            }
            if let Ok(p) = camera.world_to_viewport(cam_tf, crate::player::to_bevy(at)) {
                spots.push((p, marks.len().min(crate::titankit::TRACKER_MAX)));
            }
        }
    }
    for (m, mut node, mut vis) in &mut markers {
        match spots.get(m.0) {
            Some((p, _)) => {
                node.left = Val::Px(p.x);
                node.top = Val::Px(p.y);
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
    for (a, mut vis) in &mut art {
        let on = spots.get(a.0).is_some_and(|s| s.1 == a.1);
        vis.set_if_neq(if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// The Pilot's HUD sways with the helmet (`cockpitSway*` of pilot_base.set, vmmotion.rs): it
/// lags turns and movement, shifted and rolled on screen.
#[allow(clippy::type_complexity)]
pub fn hud_sway(
    time: Res<Time>,
    control: Res<crate::pilotctl::Control>,
    pilots: Query<&crate::pilotctl::PlayerPilot>,
    cams: Query<&Projection, With<crate::player::MainCamera>>,
    mut roots: Query<&mut UiTransform, With<HudRoot>>,
    mut sway: Local<crate::vmmotion::CockpitSway>,
) {
    let Ok(mut tf) = roots.single_mut() else {
        return;
    };
    let on_foot = *control == crate::pilotctl::Control::Pilot;
    let (yaw, pitch, vel) = match pilots.single() {
        Ok(p) if on_foot => {
            let s = &p.state;
            let (cy, sy) = (s.yaw.cos(), s.yaw.sin());
            (
                s.yaw,
                s.pitch,
                Vec3::new(
                    s.vel.x * cy + s.vel.y * sy,
                    -s.vel.x * sy + s.vel.y * cy,
                    s.vel.z,
                ),
            )
        }
        _ => (0.0, 0.0, Vec3::ZERO),
    };
    if on_foot {
        sway.step(
            &crate::vmmotion::CockpitSwayDef::PILOT,
            yaw,
            pitch,
            vel,
            time.delta_secs(),
        );
    } else {
        *sway = Default::default();
    }
    let vfov = cams
        .single()
        .ok()
        .and_then(|p| {
            if let Projection::Perspective(pp) = p {
                Some(pp.fov.to_degrees())
            } else {
                None
            }
        })
        .unwrap_or(60.0);
    let per_deg = 100.0 / vfov.max(1.0);
    // Yaw is positive to the left and pitch positive down: a yaw offset moves the HUD the
    // other way across the screen, a pitch offset moves it down.
    tf.translation = Val2::new(
        Val::Vh(-sway.angles.y * per_deg),
        Val::Vh(sway.angles.x * per_deg),
    );
    tf.rotation = Rot2::degrees(sway.angles.z);
}

/// Main HUD power on/off (`MainHud_TurnOn_RUI`/`MainHud_TurnOff_RUI` of cl_main_hud.nut). The HUD
/// is off while embarking or disembarking (`ShouldMainHudBeVisible`). Turning on, its sphere opens
/// wide then tall while it moves out from 10% of its distance, blinking at the authored flicker
/// times; turning off only blinks. The sphere is drawn flat here: its apparent size is scale/zoom.
#[derive(Default)]
pub struct HudBootState {
    on: bool,
    t: f32,
}

const BOOT_FLICKER_ON: [f32; 10] = [
    0.025, 0.035, 0.035, 0.035, 0.215, 0.33, 0.43, 0.45, 0.513, 0.538,
];
const BOOT_FLICKER_OFF: [f32; 6] = [0.025, 0.035, 0.035, 0.035, 0.215, 0.23];

fn flicker_visible(times: &[f32], t: f32) -> bool {
    times.iter().filter(|&&x| t > x).count() % 2 == 0
}

pub fn hud_boot(
    time: Res<Time>,
    control: Res<Control>,
    mut roots: Query<(&mut Visibility, &mut UiTransform), With<HudRoot>>,
    mut st: Local<HudBootState>,
) {
    let Ok((mut vis, mut tf)) = roots.single_mut() else {
        return;
    };
    let want = matches!(*control, Control::Titan | Control::Pilot);
    tf.scale = Vec2::ONE;
    if *vis == Visibility::Hidden {
        // Out of play (menus, loading): come back without a power cycle.
        *st = HudBootState { on: want, t: 10.0 };
        return;
    }
    if want != st.on {
        log::debug!("main hud power {}", if want { "on" } else { "off" });
        *st = HudBootState { on: want, t: 0.0 };
    }
    st.t += time.delta_secs();
    let t = st.t;
    if st.on {
        // Ends with the slowest part: the vertical opening at 0.75 s.
        if t >= 0.75 {
            return;
        }
        let half_pi = std::f32::consts::FRAC_PI_2;
        let graph = |x: f32, x0: f32, x1: f32, y0: f32, y1: f32| {
            y0 + (y1 - y0) * ((x - x0) / (x1 - x0)).clamp(0.0, 1.0)
        };
        let axis = |min: f32, start: f32, end: f32| {
            if t <= start {
                min
            } else if t < end {
                graph(t, start, end, min, half_pi).sin()
            } else {
                1.0
            }
        };
        let (sx, sy) = (axis(0.01, 0.0, 0.225), axis(0.1, 0.25, 0.75));
        let zoom = axis(0.1, 0.0, 0.4);
        tf.scale = Vec2::new(sx / zoom, sy / zoom);
        if !flicker_visible(&BOOT_FLICKER_ON, t) {
            vis.set_if_neq(Visibility::Hidden);
        }
    } else if t >= BOOT_FLICKER_OFF[BOOT_FLICKER_OFF.len() - 1]
        || !flicker_visible(&BOOT_FLICKER_OFF, t)
    {
        vis.set_if_neq(Visibility::Hidden);
    }
}

/// One `P_health_hex` effect on the Pilot's visor (cl_pilot_health_hud.gnut: started on the
/// cockpit at each hit while hurt, at most 10, cleared when health is full again).
#[derive(Component)]
pub struct HealthHex(f32);

pub fn health_hex(
    mut commands: Commands,
    time: Res<Time>,
    control: Res<Control>,
    pilots: Query<&crate::pilotctl::PilotHealth>,
    hexes: Query<(Entity, &HealthHex)>,
    mut last: Local<Option<f32>>,
) {
    let health = match (pilots.single(), *control) {
        (Ok(h), Control::Pilot) => Some(h.health),
        _ => None,
    };
    let clear = health.is_none_or(|h| h >= crate::pilotctl::PILOT_HEALTH);
    if clear {
        for (e, _) in &hexes {
            commands.entity(e).despawn();
        }
    } else if let (Some(h), Some(prev)) = (health, *last) {
        if h < prev - 0.01 {
            let mut list: Vec<_> = hexes.iter().collect();
            list.sort_by(|a, b| b.1 .0.total_cmp(&a.1 .0));
            for (e, _) in list.iter().skip(9) {
                commands.entity(*e).despawn();
            }
            // A screen effect: the hexes ring 55-90 screen units around the middle.
            commands.spawn((
                Transform::default(),
                crate::pfx::PfxTrail::new("P_health_hex"),
                crate::pfx::PfxScreen,
                HealthHex(time.elapsed_secs()),
            ));
        }
    }
    *last = health;
}

/// Stim's screen effect: `P_heal` on the cockpit while it runs (sh_stim.gnut
/// StimVisualsEnabled; a screen effect, sparkles and streaks around the edges of the view),
/// stopped with its end cap when Stim runs out. The script also feeds it the Pilot's speed on
/// control point 1, which none of its operators read.
#[derive(Component)]
pub struct StimFx;

pub fn stim_fx(
    mut commands: Commands,
    control: Res<Control>,
    status: Res<crate::pilotability::PilotStatus>,
    fx: Query<Entity, With<StimFx>>,
) {
    let on = *control == Control::Pilot && status.stimmed();
    match (on, fx.iter().next()) {
        (true, None) => {
            commands.spawn((
                Transform::default(),
                crate::pfx::PfxTrail::new("P_heal"),
                crate::pfx::PfxScreen,
                StimFx,
            ));
        }
        (false, Some(e)) => commands.entity(e).despawn(),
        _ => {}
    }
}

/// BT's lost health flashes on the bar: the span between the health before and after a hit
/// (cl_titan_cockpit.nut TitanCockpitHealthChangedThink creates
/// `ajax_cockpit_lost_health_segment` with oldHealthFrac / newHealthFrac). The RUI is
/// compiled; the look here (orange-red, held 0.15 s, fading over 0.5 s, hits during the flash
/// extending it) is by eye.
pub fn lost_health_flash(
    time: Res<Time>,
    titans: Query<&TitanHealth, With<PlayerTitan>>,
    mut segs: Query<(&LostSeg, &mut Node, &mut BackgroundColor)>,
    mut st: Local<(Option<f32>, Option<(f32, f32, f32)>)>,
) {
    const HOLD: f32 = 0.15;
    const FADE: f32 = 0.5;
    let Ok(health) = titans.single() else { return };
    let now = if health.v.doomed.is_some() {
        0.0
    } else {
        health.v.health
    };
    if let Some(prev) = st.0 {
        if now < prev - 0.5 {
            let top = match st.1 {
                Some((old, _, t)) if t < HOLD + FADE => old.max(prev),
                _ => prev,
            };
            st.1 = Some((top, now, 0.0));
        } else if now > prev + 0.5 {
            st.1 = None;
        }
    }
    st.0 = Some(now);
    if let Some(f) = st.1.as_mut() {
        f.1 = f.1.min(now);
        f.2 += time.delta_secs();
    }
    let flash = st.1.filter(|f| f.2 < HOLD + FADE);
    for (seg, mut node, mut bg) in &mut segs {
        let Some((old, new, t)) = flash else {
            bg.0 = Color::NONE;
            continue;
        };
        let (lo, hi) = (
            seg.0 as f32 * crate::combat::SEGMENT,
            (seg.0 + 1) as f32 * crate::combat::SEGMENT,
        );
        let (a, b) = (new.max(lo), old.min(hi));
        if b <= a {
            bg.0 = Color::NONE;
            continue;
        }
        set(
            &mut node,
            |n| &mut n.left,
            percent((a - lo) / crate::combat::SEGMENT * 100.0),
        );
        set(
            &mut node,
            |n| &mut n.width,
            percent((b - a) / crate::combat::SEGMENT * 100.0),
        );
        let alpha = if t < HOLD {
            1.0
        } else {
            1.0 - (t - HOLD) / FADE
        };
        bg.0 = Color::srgba(1.0, 0.35, 0.25, alpha.clamp(0.0, 1.0));
    }
}
