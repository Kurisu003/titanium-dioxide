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
const SHIELD: Color = Color::srgb(0.38, 0.78, 1.0);
const DOOM: Color = Color::srgb(1.0, 0.22, 0.12);
const AMBER: Color = Color::srgb(1.0, 0.68, 0.18);

#[derive(Component)]
pub struct HudRoot;
/// Only shown while in the Titan / on foot.
#[derive(Component)]
pub struct TitanOnly;
/// The Pilot's status block (on foot), laid out like BT's.
#[derive(Component)]
pub struct PilotOnly;
#[derive(Component)]
pub struct PilotSeg(usize);
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
/// The ability slot's icon and label (swapped per loadout by titankit).
#[derive(Component)]
pub struct SlotIcon(pub Slot);
#[derive(Component)]
pub struct SlotLabel(pub Slot);
#[derive(Component)]
pub struct CoreLabel;
#[derive(Component)]
pub struct CoreText;
#[derive(Component)]
pub struct WeaponName;
#[derive(Component)]
pub struct AmmoCount;
#[derive(Component)]
pub struct AmmoClip;
#[derive(Component)]
pub struct WaveTitle;
#[derive(Component)]
pub struct WaveSub;
#[derive(Component)]
pub struct Banner;
#[derive(Component)]
pub struct Prompt;
#[derive(Component)]
pub struct HurtOverlay;

const DAMAGE_ARCS: usize = 8;
/// The Pilot's health bar is split like BT's (20 health a segment).
const PILOT_SEGMENTS: usize = 5;
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
    mut arcs: Query<(&DamageArc, &mut Node, &mut UiTransform, &mut Visibility, &mut BackgroundGradient)>,
) {
    let dt = time.delta_secs();
    from.0.retain_mut(|(_, t)| {
        *t -= dt;
        *t > 0.0
    });
    let Ok(cam) = camera.single() else { return };
    let (eye, fwd, right) = (cam.translation(), cam.forward().as_vec3(), cam.right().as_vec3());
    for (arc, mut node, mut tf, mut vis, mut grad) in &mut arcs {
        let Some(&(p, t)) = from.0.get(arc.0) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let to = crate::player::to_bevy(p) - eye;
        let (f, r) = (to.dot(fwd.with_y(0.0).normalize_or_zero()), to.dot(right.with_y(0.0).normalize_or_zero()));
        let a = r.atan2(f);
        let radius = 15.0;
        node.left = vh(a.sin() * radius);
        node.top = vh(-a.cos() * radius);
        tf.rotation = Rot2::radians(a);
        *vis = Visibility::Inherited;
        let alpha = (t / DAMAGE_ARC_TIME).min(1.0).powf(0.7);
        let c = Color::srgba(1.0, 0.22, 0.1, alpha);
        if let Some(Gradient::Linear(g)) = grad.0.first_mut() {
            g.stops[1].color = c;
            g.stops[2].color = c;
        }
    }
}
/// Crosshair hit marker corners.
#[derive(Component)]
pub struct HitMarker;
/// Charge meter under the crosshair (railgun levels, Charge Rifle wind-up).
#[derive(Component)]
pub struct ChargeHud;
#[derive(Component)]
pub struct ChargePip(usize);
#[derive(Component)]
pub struct ChargePipFill(usize);
const CHARGE_PIPS: usize = 6;
const CHARGE_WIDTH: f32 = 12.0;

fn vh(v: f32) -> Val {
    Val::Vh(v)
}

fn text(commands: &mut Commands, s: &str, font: &Handle<Font>, size: f32, color: Color) -> Entity {
    commands.spawn((Text::new(s), TextFont { font: font.clone(), font_size: 16.0, ..default() }, TextColor(color), VhText(size))).id()
}

fn image(commands: &mut Commands, ui: &UiAssets, path: &str, node: Node, color: Color) -> Entity {
    match ui.image(path) {
        Some(mut img) => {
            img.color = color;
            commands.spawn((img, node)).id()
        }
        None => commands.spawn((node, BackgroundColor(color.with_alpha(0.3)))).id(),
    }
}

pub fn spawn_hud(mut commands: Commands, ui: Res<UiAssets>) {
    let root = commands
        .spawn((Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, ..default() }, HudRoot))
        .id();

    // Damage flash (under everything else).
    let hurt = commands
        .spawn((
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
            BackgroundGradient::from(RadialGradient::new(
                UiPosition::CENTER,
                RadialGradientShape::FarthestCorner,
                vec![ColorStop::new(Color::NONE, percent(60)), ColorStop::new(Color::srgba(0.75, 0.04, 0.0, 0.5), percent(100))],
            )),
            Visibility::Hidden,
            HurtOverlay,
        ))
        .id();
    commands.entity(root).add_child(hurt);

    // Damage direction indicators: arcs on a ring around the crosshair, pooled.
    let ring = commands
        .spawn(Node { position_type: PositionType::Absolute, left: percent(50), top: percent(50), width: px(0), height: px(0), ..default() })
        .id();
    commands.entity(root).add_child(ring);
    for i in 0..DAMAGE_ARCS {
        let arc = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: vh(10.0),
                    height: vh(0.8),
                    margin: UiRect { left: vh(-5.0), top: vh(-0.4), ..default() },
                    ..default()
                },
                BackgroundGradient::from(LinearGradient::to_right(vec![
                    ColorStop::new(Color::NONE, percent(0)),
                    ColorStop::new(Color::srgb(1.0, 0.22, 0.1), percent(30)),
                    ColorStop::new(Color::srgb(1.0, 0.22, 0.1), percent(70)),
                    ColorStop::new(Color::NONE, percent(100)),
                ])),
                UiTransform::IDENTITY,
                Visibility::Hidden,
                DamageArc(i),
            ))
            .id();
        commands.entity(ring).add_child(arc);
    }

    // Crosshair: the active weapon's (crosshair.rs).
    crate::crosshair::spawn(&mut commands, root);
    // Hit marker: four corners around the centre, flipped into place.
    for (x, y, fx, fy) in [(-1.0f32, -1.0f32, false, false), (1.0, -1.0, true, false), (-1.0, 1.0, false, true), (1.0, 1.0, true, true)] {
        let node = Node { position_type: PositionType::Absolute, width: vh(1.3), height: vh(1.3), left: Val::Vh(x * 1.2 - 0.65), top: Val::Vh(y * 1.2 - 0.65), ..default() };
        let e = image(&mut commands, &ui, "rui/hud/crosshairs/crosshair_corner", node, Color::WHITE);
        commands.entity(e).insert((HitMarker, Visibility::Hidden));
        if let Some(mut c) = commands.get_entity(e).ok() {
            c.entry::<ImageNode>().and_modify(move |mut i| {
                i.flip_x = fx;
                i.flip_y = fy;
            });
        }
        let holder = commands.spawn(Node { position_type: PositionType::Absolute, left: percent(50), top: percent(50), ..default() }).id();
        commands.entity(holder).add_child(e);
        commands.entity(root).add_child(holder);
    }
    spawn_smart_hud(&mut commands, &ui, root);
    // Charge meter: one pip per charge level (a single bar for a continuous charge).
    let charge_holder = commands
        .spawn(Node { position_type: PositionType::Absolute, top: percent(50), left: px(0), right: px(0), justify_content: JustifyContent::Center, ..default() })
        .id();
    let charge = commands
        .spawn((Node { margin: UiRect::top(vh(3.2)), column_gap: vh(0.4), height: vh(0.5), ..default() }, ChargeHud, Visibility::Hidden))
        .id();
    for i in 0..CHARGE_PIPS {
        let frame = commands
            .spawn((Node { width: vh(CHARGE_WIDTH / CHARGE_PIPS as f32), height: percent(100), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)), ChargePip(i)))
            .id();
        let fill = commands.spawn((Node { width: percent(0), height: percent(100), ..default() }, BackgroundColor(WHITE), ChargePipFill(i))).id();
        commands.entity(frame).add_child(fill);
        commands.entity(charge).add_child(frame);
    }
    commands.entity(charge_holder).add_child(charge);
    commands.entity(root).add_child(charge_holder);

    // --- Bottom centre: BT's shield and health segments, dash pips, core meter ---
    let bottom = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: vh(4.0),
                left: px(0),
                right: px(0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexEnd,
                column_gap: vh(2.0),
                ..default()
            },
            TitanOnly,
        ))
        .id();
    commands.entity(root).add_child(bottom);
    let health = commands.spawn(Node { flex_direction: FlexDirection::Column, row_gap: vh(0.5), width: vh(48.0), ..default() }).id();
    let name_row = commands.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).id();
    let name = text(&mut commands, "BT-7274", &ui.bold_font, 1.8, WHITE);
    let tag = text(&mut commands, "VANGUARD CLASS", &ui.font, 1.4, DIM);
    commands.entity(name_row).add_children(&[name, tag]);
    let shield_frame = commands.spawn((Node { height: vh(0.6), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)))).id();
    let shield_fill = commands.spawn((Node { width: percent(100), height: percent(100), ..default() }, BackgroundColor(SHIELD), ShieldFill)).id();
    commands.entity(shield_frame).add_child(shield_fill);
    let segs = commands.spawn(Node { column_gap: vh(0.45), height: vh(1.5), ..default() }).id();
    for i in 0..crate::combat::SEGMENTS as usize {
        let frame = commands
            .spawn((Node { flex_grow: 1.0, height: percent(100), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45))))
            .id();
        let fill = commands.spawn((Node { width: percent(100), height: percent(100), ..default() }, BackgroundColor(WHITE), HealthSeg(i))).id();
        let lost = commands.spawn((Node { position_type: PositionType::Absolute, height: percent(100), width: percent(0), ..default() }, BackgroundColor(Color::NONE), LostSeg(i))).id();
        commands.entity(frame).add_children(&[fill, lost]);
        commands.entity(segs).add_child(frame);
    }
    // Doomed: one red bar with the game's doom pattern.
    let doom = image(&mut commands, &ui, "rui/hud/healthbar_doom_16x160", Node { height: vh(1.5), width: percent(100), ..default() }, DOOM);
    commands.entity(doom).insert((DoomFill, Visibility::Hidden));
    let pips = commands.spawn(Node { column_gap: vh(0.6), height: vh(0.45), margin: UiRect::top(vh(0.3)), ..default() }).id();
    for i in 0..2 {
        let frame = commands.spawn((Node { width: vh(6.0), height: percent(100), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)))).id();
        let fill = commands.spawn((Node { width: percent(100), height: percent(100), ..default() }, BackgroundColor(WHITE), DashPip(i))).id();
        commands.entity(frame).add_child(fill);
        commands.entity(pips).add_child(frame);
    }
    commands.entity(health).add_children(&[name_row, shield_frame, segs, doom, pips]);
    // Core meter.
    let core = commands.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: vh(0.3), ..default() }).id();
    let core_icon = image(&mut commands, &ui, "rui/hud/titan_core", Node { width: vh(6.5), height: vh(6.5), ..default() }, DIM);
    commands.entity(core_icon).insert(CoreIcon);
    let core_text = text(&mut commands, "0%", &ui.bold_font, 1.6, WHITE);
    commands.entity(core_text).insert(CoreText);
    let core_label = text(&mut commands, "BURST CORE", &ui.font, 1.2, DIM);
    commands.entity(core_label).insert(CoreLabel);
    commands.entity(core).add_children(&[core_icon, core_text, core_label]);
    commands.entity(bottom).add_children(&[health, core]);

    // --- Bottom centre on foot: the Pilot's health, in the same frame as BT's ---
    let pilot_bottom = commands
        .spawn((
            Node { position_type: PositionType::Absolute, bottom: vh(4.0), left: px(0), right: px(0), justify_content: JustifyContent::Center, align_items: AlignItems::FlexEnd, ..default() },
            PilotOnly,
            Visibility::Hidden,
        ))
        .id();
    commands.entity(root).add_child(pilot_bottom);
    // Same width and height as BT's bars (with its core meter beside and the doom bar and
    // dash pips below), so the two line up.
    let p_health = commands.spawn(Node { flex_direction: FlexDirection::Column, row_gap: vh(0.5), width: vh(48.0), margin: UiRect { right: vh(8.5), bottom: vh(3.25), ..default() }, ..default() }).id();
    let p_name_row = commands.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).id();
    let p_name = text(&mut commands, "COOPER", &ui.bold_font, 1.8, WHITE);
    let p_tag = text(&mut commands, "PILOT", &ui.font, 1.4, DIM);
    commands.entity(p_name_row).add_children(&[p_name, p_tag]);
    let p_segs = commands.spawn(Node { column_gap: vh(0.45), height: vh(1.5), ..default() }).id();
    for i in 0..PILOT_SEGMENTS {
        let frame = commands
            .spawn((Node { flex_grow: 1.0, height: percent(100), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45))))
            .id();
        let fill = commands.spawn((Node { width: percent(100), height: percent(100), ..default() }, BackgroundColor(WHITE), PilotSeg(i))).id();
        commands.entity(frame).add_child(fill);
        commands.entity(p_segs).add_child(frame);
    }
    commands.entity(p_health).add_children(&[p_name_row, p_segs]);
    commands.entity(pilot_bottom).add_child(p_health);

    // --- Bottom left: ability slots (ordnance Q, defensive E) ---
    let slots = commands
        .spawn((Node { position_type: PositionType::Absolute, left: vh(4.0), bottom: vh(4.0), column_gap: vh(1.2), ..default() }, TitanOnly))
        .id();
    commands.entity(root).add_child(slots);
    for (slot, icon, key, label) in [
        (Slot::Defensive, "rui/titan_loadout/defensive/titan_defensive_vortex_menu", "Q", "VORTEX SHIELD"),
        (Slot::Utility, "rui/titan_loadout/tactical/titan_tactical_electric_smoke_menu", "E", "ELECTRIC SMOKE"),
        (Slot::Ordnance, "rui/titan_loadout/ordnance/multilock_rockets_menu", "G", "MULTI-TARGET MISSILES"),
    ] {
        let col = commands.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: vh(0.4), ..default() }).id();
        let frame = commands
            .spawn((Node { width: vh(7.0), height: vh(7.0), border: UiRect::all(px(1)), overflow: Overflow::clip(), ..default() }, BorderColor::all(DIM), BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4))))
            .id();
        let ic = image(&mut commands, &ui, icon, Node { width: percent(100), height: percent(100), ..default() }, WHITE);
        commands.entity(ic).insert(SlotIcon(slot));
        let shade = commands
            .spawn((Node { position_type: PositionType::Absolute, left: px(0), right: px(0), bottom: px(0), height: percent(0), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)), CooldownShade(slot)))
            .id();
        let state = text(&mut commands, "", &ui.bold_font, 1.4, AMBER);
        commands.entity(state).insert((SlotText(slot), Node { position_type: PositionType::Absolute, bottom: vh(0.3), ..default() }));
        commands.entity(frame).add_children(&[ic, shade, state]);
        let k = text(&mut commands, key, &ui.bold_font, 1.5, WHITE);
        let l = text(&mut commands, label, &ui.font, 1.0, DIM);
        commands.entity(l).insert(SlotLabel(slot));
        commands.entity(col).add_children(&[frame, k, l]);
        commands.entity(slots).add_child(col);
    }

    // --- Bottom right: weapon and ammo ---
    let weapon = commands
        .spawn(Node { position_type: PositionType::Absolute, right: vh(4.0), bottom: vh(3.4), flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, ..default() })
        .id();
    commands.entity(root).add_child(weapon);
    let wname = text(&mut commands, "XO-16", &ui.bold_font, 1.8, DIM);
    commands.entity(wname).insert(WeaponName);
    let row = commands.spawn(Node { align_items: AlignItems::FlexEnd, column_gap: vh(0.6), ..default() }).id();
    let count = text(&mut commands, "30", &ui.bold_font, 6.0, WHITE);
    commands.entity(count).insert(AmmoCount);
    let clip = text(&mut commands, "/ 30", &ui.font, 2.2, DIM);
    commands.entity(clip).insert((AmmoClip, Node { margin: UiRect::bottom(vh(1.0)), ..default() }));
    commands.entity(row).add_children(&[count, clip]);
    commands.entity(weapon).add_children(&[wname, row]);

    // --- Top right: wave callout on the hazard strip ---
    let wave = commands
        .spawn(Node { position_type: PositionType::Absolute, right: vh(4.0), top: vh(4.0), flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, row_gap: vh(0.6), ..default() })
        .id();
    commands.entity(root).add_child(wave);
    let strip = image(
        &mut commands,
        &ui,
        "rui/hud/bounty_hunt/wave_callout_strip",
        Node { width: vh(30.0), height: vh(3.8), align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() },
        Color::srgba(1.0, 1.0, 1.0, 0.9),
    );
    let wave_title = text(&mut commands, "", &ui.title_font, 2.6, Color::BLACK);
    commands.entity(wave_title).insert(WaveTitle);
    commands.entity(strip).add_child(wave_title);
    let wave_sub = text(&mut commands, "", &ui.bold_font, 1.6, WHITE);
    commands.entity(wave_sub).insert((WaveSub, TextLayout::new_with_justify(Justify::Right)));
    commands.entity(wave).add_children(&[strip, wave_sub]);

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
        .spawn(Node { position_type: PositionType::Absolute, bottom: percent(22), left: px(0), right: px(0), justify_content: JustifyContent::Center, ..default() })
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
    titans: Query<(&PlayerTitan, &TitanHealth, &Ordnance, &TitanCore, &Vortex, Option<&crate::weapons::Weapon>, &Titanfall)>,
    pilots: Query<(&PlayerPilot, &crate::pilotctl::PilotHealth, Option<&crate::pilotweapon::PilotLoadout>)>,
    strings: Res<crate::pilotweapon::Strings>,
    enemies: Query<&Enemy>,
    mut segs: Query<(&HealthSeg, &mut Node, &mut BackgroundColor), (Without<ShieldFill>, Without<DashPip>, Without<CooldownShade>)>,
    mut shield: Query<&mut Node, (With<ShieldFill>, Without<HealthSeg>, Without<DashPip>, Without<CooldownShade>)>,
    mut doom: Query<(&mut Node, &mut Visibility), (With<DoomFill>, Without<HealthSeg>, Without<ShieldFill>, Without<DashPip>, Without<CooldownShade>, Without<TitanOnly>, Without<HudRoot>)>,
    mut pips: Query<(&DashPip, &mut Node, &mut BackgroundColor), (Without<HealthSeg>, Without<ShieldFill>, Without<CooldownShade>)>,
    mut shades: Query<(&CooldownShade, &mut Node), (Without<HealthSeg>, Without<ShieldFill>, Without<DashPip>, Without<DoomFill>)>,
    mut texts: Query<(&mut Text, &mut TextColor, Option<&SlotText>, Option<&CoreText>, Option<&WeaponName>, Option<&AmmoCount>, Option<&AmmoClip>, Option<&WaveTitle>, Option<&WaveSub>, Option<&Banner>, Option<&Prompt>)>,
    (kit_view, mut pilot_segs): (
        Option<Res<crate::titankit::KitView>>,
        Query<(&PilotSeg, &mut Node, &mut BackgroundColor), (Without<HealthSeg>, Without<ShieldFill>, Without<DashPip>, Without<CooldownShade>, Without<DoomFill>)>,
    ),
) {
    let Ok((titan, health, ord, core, vortex, weapon, titanfall)) = titans.single() else { return };
    let pilot = pilots.single().ok();
    if let Some((_, ph, _)) = pilot {
        let per = crate::pilotctl::PILOT_HEALTH / PILOT_SEGMENTS as f32;
        for (seg, mut node, mut bg) in &mut pilot_segs {
            let fill = ((ph.health - seg.0 as f32 * per) / per).clamp(0.0, 1.0);
            node.width = percent(fill * 100.0);
            bg.0 = if ph.hurt > 0.0 { Color::srgb(1.0, 0.55, 0.45) } else { WHITE };
        }
    }
    let in_titan = *control == Control::Titan && health.dead_for.is_none();

    // Health and shield.
    let doomed = health.v.doomed.is_some();
    for (seg, mut node, mut bg) in &mut segs {
        let fill = if doomed { 0.0 } else { ((health.v.health - seg.0 as f32 * crate::combat::SEGMENT) / crate::combat::SEGMENT).clamp(0.0, 1.0) };
        node.width = percent(fill * 100.0);
        bg.0 = if health.hurt > 0.0 { Color::srgb(1.0, 0.55, 0.45) } else { WHITE };
    }
    if let Ok(mut n) = shield.single_mut() {
        n.width = percent(health.v.shield / health.v.max_shield.max(1.0) * 100.0);
    }
    if let Ok((mut n, mut v)) = doom.single_mut() {
        *v = if doomed { Visibility::Inherited } else { Visibility::Hidden };
        n.width = percent(health.v.fraction() * 100.0);
    }
    for (pip, mut node, mut bg) in &mut pips {
        let fill = ((titan.state.power - pip.0 as f32 * 50.0) / 50.0).clamp(0.0, 1.0);
        node.width = percent(fill * 100.0);
        bg.0 = if fill >= 1.0 { WHITE } else { DIM };
    }
    // Cooldowns: the shade covers what is still recharging.
    for (s, mut node) in &mut shades {
        let left = match (s.0, kit_view.as_ref()) {
            (Slot::Ordnance, Some(k)) => k.ord_left,
            (Slot::Defensive, Some(k)) => k.def_left,
            (Slot::Utility, Some(k)) => k.util_left,
            (Slot::Utility, None) => 0.0,
            (Slot::Ordnance, None) => if ord.ready() || ord.charging { 0.0 } else { 1.0 - ord.fraction() },
            (Slot::Defensive, None) => 1.0 - vortex.fraction(),
        };
        node.height = percent(left.clamp(0.0, 1.0) * 100.0);
    }
    let alive = enemies.iter().filter(|e| e.alive() && !e.infantry).count();
    for (mut t, mut col, slot, core_t, wname, count, clip, wave_title, wave_sub, banner, prompt) in &mut texts {
        let s: String = if let Some(slot) = slot {
            match slot.0 {
                Slot::Ordnance if kit_view.is_some() => kit_view.as_ref().unwrap().ord_text.clone(),
                Slot::Defensive if kit_view.is_some() => kit_view.as_ref().unwrap().def_text.clone(),
                Slot::Utility if kit_view.is_some() => kit_view.as_ref().unwrap().util_text.clone(),
                Slot::Ordnance if ord.charging => format!("{}", ord.locks.len()),
                Slot::Ordnance if !ord.ready() => format!("{:.0}", (ord.cooldown + ord.delay.max(0.0)).ceil()),
                Slot::Defensive if vortex.active => format!("{}", vortex.caught + vortex.absorbed),
                _ => String::new(),
            }
        } else if core_t.is_some() {
            match core.state {
                CoreState::Building if core.meter >= 1.0 => "READY  [V]".into(),
                CoreState::Building => format!("{:.0}%", core.meter * 100.0),
                CoreState::Charging(_) => "CHARGING".into(),
                CoreState::Active(_) => "ACTIVE".into(),
            }
        } else if wname.is_some() {
            match pilot.and_then(|p| p.2.and_then(|l| l.active())) {
                Some(w) if *control == Control::Pilot => strings.weapon(&w.def).to_uppercase(),
                _ => weapon.map(|w| strings.weapon(&w.def).to_uppercase()).unwrap_or_default(),
            }
        } else if count.is_some() {
            match (*control, weapon, pilot.and_then(|p| p.2.and_then(|l| l.active()))) {
                (Control::Pilot, _, Some(w)) => if w.reloading() { "--".into() } else { format!("{}", w.ammo) },
                (_, Some(w), _) => if w.reloading() { "--".into() } else { format!("{}", w.ammo) },
                _ => String::new(),
            }
        } else if clip.is_some() {
            match (*control, weapon, pilot.and_then(|p| p.2.and_then(|l| l.active()))) {
                (Control::Pilot, _, Some(w)) => if w.reloading() { "RELOADING".into() } else { format!("/ {}", w.def.clip) },
                (_, Some(w), _) => if w.reloading() { "RELOADING".into() } else { format!("/ {}", w.def.clip) },
                _ => String::new(),
            }
        } else if wave_title.is_some() {
            match game.state {
                GameState::Intermission if game.wave == 0 => "GET READY".into(),
                GameState::Intermission => format!("WAVE {} CLEARED", game.wave),
                _ => format!("WAVE {}", game.wave),
            }
        } else if wave_sub.is_some() {
            match game.state {
                GameState::Intermission => format!("NEXT WAVE IN {:.0}    KILLS {}", game.timer.max(0.0).ceil(), game.kills),
                _ => format!("ENEMY TITANS {alive}    KILLS {}", game.kills),
            }
        } else if banner.is_some() {
            if health.dead_for.is_some_and(|t| t < 4.0) {
                "TITAN DESTROYED".into()
            } else if doomed && in_titan {
                col.0 = DOOM.with_alpha(0.6 + 0.4 * ((health.v.doomed.unwrap_or(0.0) * 6.0).sin() * 0.5 + 0.5));
                "WARNING: TITAN DOOMED".into()
            } else {
                String::new()
            }
        } else if prompt.is_some() {
            match *control {
                Control::Pilot => {
                    let near = pilot.is_some_and(|(p, _, _)| (titan.state.pos - p.state.pos).truncate().length() < 260.0);
                    if near && health.dead_for.is_none() {
                        "[X]  EMBARK".into()
                    } else if titanfall.rebuilding && titanfall.cooldown > 0.0 {
                        format!("TITAN READY IN {:.0}", titanfall.cooldown.ceil())
                    } else if titanfall.cooldown <= 0.0 && !titanfall.busy() && (titanfall.rebuilding || health.dead_for.is_some()) {
                        "[T]  TITANFALL".into()
                    } else {
                        String::new()
                    }
                }
                Control::Disembark(_) => "DISEMBARKING".into(),
                Control::Embark(_) => "EMBARKING".into(),
                // The doomed eject interface: three presses of the use button.
                Control::Titan if doomed && titanfall.eject.is_none() => {
                    format!("[X] x3  EJECT   {}/{}", titanfall.eject_presses, crate::pilotctl::EJECT_PRESSES)
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
    ui: Res<UiAssets>,
    titans: Query<(&TitanHealth, &TitanCore, Option<&crate::weapons::Weapon>)>,
    pilots: Query<(&crate::pilotctl::PilotHealth, Option<&crate::pilotweapon::PilotLoadout>)>,
    mut root: Query<&mut Visibility, (With<HudRoot>, Without<PilotOnly>)>,
    mut titan_only: Query<&mut Visibility, (With<TitanOnly>, Without<HudRoot>, Without<HurtOverlay>, Without<PilotOnly>)>,
    mut core_icon: Query<&mut ImageNode, With<CoreIcon>>,
    mut hurt: Query<(&mut Visibility, &mut BackgroundGradient), (With<HurtOverlay>, Without<HudRoot>, Without<TitanOnly>, Without<PilotOnly>)>,
    mut charge_hud: Query<&mut Visibility, (With<ChargeHud>, Without<HudRoot>, Without<TitanOnly>, Without<HurtOverlay>, Without<PilotOnly>)>,
    mut charge_pips: Query<(&ChargePip, &mut Node), Without<ChargePipFill>>,
    mut charge_fills: Query<(&ChargePipFill, &mut Node, &mut BackgroundColor), Without<ChargePip>>,
    (hidden, mut pilot_only): (Res<HudHidden>, Query<&mut Visibility, (With<PilotOnly>, Without<HudRoot>, Without<TitanOnly>, Without<HurtOverlay>, Without<ChargeHud>)>),
) {
    let Ok((health, core, weapon)) = titans.single() else { return };
    let (pilot, loadout) = pilots.single().map(|(h, l)| (Some(h), l)).unwrap_or((None, None));
    let in_game = game.in_play() && !menu.paused && *mode != CameraMode::Free && !hidden.0;
    if let Ok(mut v) = root.single_mut() {
        *v = if in_game { Visibility::Inherited } else { Visibility::Hidden };
    }
    let in_titan = *control == Control::Titan && health.dead_for.is_none();
    for mut v in &mut titan_only {
        *v = if in_titan { Visibility::Inherited } else { Visibility::Hidden };
    }
    for mut v in &mut pilot_only {
        *v = if *control == Control::Pilot && pilot.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    }
    if let Ok(mut img) = core_icon.single_mut() {
        let ready = core.state == CoreState::Building && core.meter >= 1.0 || core.active();
        let want = if ready { "rui/hud/titan_core_ready" } else { "rui/hud/titan_core" };
        if let Some(n) = ui.image(want) {
            if img.rect != n.rect {
                img.rect = n.rect;
            }
        }
        img.color = if ready { AMBER } else { DIM.mix(&WHITE, core.meter) };
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
        *v = if hurt_amount > 0.01 && in_game { Visibility::Inherited } else { Visibility::Hidden };
        if let Some(Gradient::Radial(r)) = g.0.first_mut() {
            if let Some(stop) = r.stops.last_mut() {
                stop.color = Color::srgba(0.75, 0.04, 0.0, hurt_amount.clamp(0.0, 0.5));
            }
        }
    }

    // Charge meter for the weapon in hand.
    let charge = match *control {
        Control::Pilot => loadout.and_then(|l| l.active()).map(|g| (g.def.charge.clone(), g.charge)),
        Control::Titan => weapon.map(|w| (w.def.charge.clone(), w.charge)),
        _ => None,
    }
    .filter(|(def, _)| def.time > 0.0);
    if let Ok(mut v) = charge_hud.single_mut() {
        *v = if charge.is_some() && in_game { Visibility::Inherited } else { Visibility::Hidden };
    }
    if let Some((def, c)) = charge {
        let n = def.levels.clamp(1, CHARGE_PIPS as u32) as usize;
        let full = c.frac >= 1.0;
        for (pip, mut node) in &mut charge_pips {
            node.display = if pip.0 < n { Display::Flex } else { Display::None };
            node.width = vh(CHARGE_WIDTH / n as f32);
        }
        for (fill, mut node, mut bg) in &mut charge_fills {
            let f = (c.frac * n as f32 - fill.0 as f32).clamp(0.0, 1.0);
            node.width = percent(f * 100.0);
            bg.0 = if full { AMBER } else if f >= 1.0 { WHITE } else { DIM };
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
    let hit = enemies.iter().filter(|e| e.active && e.since_hit <= dt * 1.01).count();
    let killed = enemies.iter().any(|e| e.just_died);
    if hit > 0 {
        *timer = 0.18;
        if *beep_cd <= 0.0 {
            crate::audio::cue(&mut commands, crate::audio::Cue::HitBeep, None);
            *beep_cd = 0.07;
        }
    }
    for (mut v, mut img) in &mut markers {
        *v = if *timer > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        img.color = if killed { DOOM } else { Color::WHITE.with_alpha((*timer / 0.18).clamp(0.0, 1.0)) };
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
    let circle = image(commands, ui, "rui/hud/crosshairs/crosshair_circle", Node { position_type: PositionType::Absolute, left: percent(50), top: percent(50), width: vh(20.0), height: vh(20.0), margin: UiRect { left: vh(-10.0), top: vh(-10.0), ..default() }, ..default() }, DIM);
    commands.entity(circle).insert((SmartCircle, Visibility::Hidden));
    commands.entity(root).add_child(circle);
    for i in 0..SMART_MARKERS {
        let holder = commands.spawn((Node { position_type: PositionType::Absolute, ..default() }, Visibility::Hidden, SmartMarker(i))).id();
        for (k, (fx, fy)) in [(false, false), (true, false), (false, true), (true, true)].into_iter().enumerate() {
            let e = image(commands, ui, "rui/hud/crosshairs/smart_ammo_corner", Node { position_type: PositionType::Absolute, width: vh(1.6), height: vh(1.6), ..default() }, WHITE);
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
        commands.entity(t).insert((Node { position_type: PositionType::Absolute, ..default() }, SmartCount(i)));
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
    mut circle: Query<(&mut Node, &mut Visibility), (With<SmartCircle>, Without<SmartMarker>, Without<SmartCorner>, Without<SmartCount>)>,
    mut markers: Query<(&SmartMarker, &mut Node, &mut Visibility), (Without<SmartCircle>, Without<SmartCorner>, Without<SmartCount>)>,
    mut corners: Query<(&SmartCorner, &mut Node), (Without<SmartCircle>, Without<SmartMarker>, Without<SmartCount>)>,
    mut counts: Query<(&SmartCount, &mut Node, &mut Text), (Without<SmartCircle>, Without<SmartMarker>, Without<SmartCorner>)>,
) {
    let Ok((camera, cam_tf, projection)) = cameras.single() else { return };
    let gun = loadouts.single().ok().and_then(|l| l.active()).filter(|_| *control == Control::Pilot && *mode != CameraMode::Free);
    let smart = gun.and_then(|g| g.def.smart.as_ref().map(|sd| (sd, g.smart.view())));
    let view_h = camera.logical_viewport_size().map(|v| v.y).unwrap_or(1080.0);
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
                node.margin = UiRect { left: Val::Px(-r), top: Val::Px(-r), ..default() };
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }

    // (screen centre, half size in px, lock fraction, full locks, max)
    let mut spots: Vec<(Vec2, f32, f32, u32, u32)> = Vec::new();
    if let Some((_, locks)) = &smart {
        for l in locks {
            let Ok(e) = enemies.get(l.target) else { continue };
            if !e.alive() {
                continue;
            }
            let centre = crate::player::to_bevy(e.pos + Vec3::Z * e.height * 0.5);
            let Ok(p) = camera.world_to_viewport(cam_tf, centre) else { continue };
            let dist = cam_tf.translation().distance(centre) / crate::player::UNIT;
            let half = (e.height * 0.55 / dist.max(1.0)) * px_per_tan;
            let full = (l.frac + 1e-4).floor();
            spots.push((p, half.clamp(view_h * 0.012, view_h * 0.25), l.frac - full, full as u32, l.max));
        }
    }
    for (m, mut node, mut vis) in &mut markers {
        match spots.get(m.0) {
            Some((p, ..)) => {
                node.left = Val::Px(p.x);
                node.top = Val::Px(p.y);
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for (c, mut node) in &mut corners {
        let Some(&(_, half, frac, full, _)) = spots.get(c.0) else { continue };
        // Brackets start on the target's box and close to a third of it as the first lock
        // forms; with a lock held they stay closed.
        let spread = if full > 0 { 0.35 } else { 1.0 - 0.65 * frac };
        let d = half * spread;
        let size = view_h * 0.016;
        let (sx, sy) = (if c.1 % 2 == 0 { -1.0 } else { 1.0 }, if c.1 < 2 { -1.0 } else { 1.0 });
        node.left = Val::Px(sx * d - size * 0.5);
        node.top = Val::Px(sy * d - size * 0.5);
    }
    for (c, mut node, mut t) in &mut counts {
        let Some(&(_, half, _, full, max)) = spots.get(c.0) else { continue };
        let s = if full > 0 { format!("x{full}{}", if full >= max { "" } else { "+" }) } else { String::new() };
        if t.0 != s {
            t.0 = s;
        }
        node.left = Val::Px(half * 0.35 + view_h * 0.012);
        node.top = Val::Px(-view_h * 0.012);
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
    let Ok(mut tf) = roots.single_mut() else { return };
    let on_foot = *control == crate::pilotctl::Control::Pilot;
    let (yaw, pitch, vel) = match pilots.single() {
        Ok(p) if on_foot => {
            let s = &p.state;
            let (cy, sy) = (s.yaw.cos(), s.yaw.sin());
            (s.yaw, s.pitch, Vec3::new(s.vel.x * cy + s.vel.y * sy, -s.vel.x * sy + s.vel.y * cy, s.vel.z))
        }
        _ => (0.0, 0.0, Vec3::ZERO),
    };
    if on_foot {
        sway.step(&crate::vmmotion::CockpitSwayDef::PILOT, yaw, pitch, vel, time.delta_secs());
    } else {
        *sway = Default::default();
    }
    let vfov = cams.single().ok().and_then(|p| if let Projection::Perspective(pp) = p { Some(pp.fov.to_degrees()) } else { None }).unwrap_or(60.0);
    let per_deg = 100.0 / vfov.max(1.0);
    // Yaw is positive to the left and pitch positive down: a yaw offset moves the HUD the
    // other way across the screen, a pitch offset moves it down.
    tf.translation = Val2::new(Val::Vh(-sway.angles.y * per_deg), Val::Vh(sway.angles.x * per_deg));
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

const BOOT_FLICKER_ON: [f32; 10] = [0.025, 0.035, 0.035, 0.035, 0.215, 0.33, 0.43, 0.45, 0.513, 0.538];
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
    let Ok((mut vis, mut tf)) = roots.single_mut() else { return };
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
        let graph = |x: f32, x0: f32, x1: f32, y0: f32, y1: f32| y0 + (y1 - y0) * ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
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
            *vis = Visibility::Hidden;
        }
    } else if t >= BOOT_FLICKER_OFF[BOOT_FLICKER_OFF.len() - 1] || !flicker_visible(&BOOT_FLICKER_OFF, t) {
        *vis = Visibility::Hidden;
    }
}

/// One `P_health_hex` effect on the Pilot's visor (cl_pilot_health_hud.gnut: started on the
/// cockpit at each hit while hurt, at most 10, cleared when health is full again).
#[derive(Component)]
pub struct HealthHex(f32);

/// Distance ahead of the eye of the visor plane the hexes sit in (the effect rings them 55-90
/// units around its control point in the point's XY plane; the cockpit's origin offset isn't
/// known, so this puts the ring at the screen edges - a guess).
const HEALTH_HEX_DIST: f32 = 110.0;

pub fn health_hex(
    mut commands: Commands,
    time: Res<Time>,
    control: Res<Control>,
    pilots: Query<&crate::pilotctl::PilotHealth>,
    cams: Query<Entity, With<crate::player::MainCamera>>,
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
    } else if let (Some(h), Some(prev), Ok(cam)) = (health, *last, cams.single()) {
        if h < prev - 0.01 {
            let mut list: Vec<_> = hexes.iter().collect();
            list.sort_by(|a, b| b.1 .0.total_cmp(&a.1 .0));
            for (e, _) in list.iter().skip(9) {
                commands.entity(*e).despawn();
            }
            // The effect's XY plane faces the camera: its up (Bevy Y) turned to point back at
            // the eye.
            let child = commands
                .spawn((
                    Transform::from_xyz(0.0, 0.0, -HEALTH_HEX_DIST * crate::player::UNIT).with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                    Visibility::default(),
                    crate::pfx::PfxTrail::oriented("P_health_hex"),
                    HealthHex(time.elapsed_secs()),
                ))
                .id();
            commands.entity(cam).add_child(child);
        }
    }
    *last = health;
}

/// Stim's screen effect: `P_heal` on the cockpit while it runs (sh_stim.gnut
/// StimVisualsEnabled), placed like `P_health_hex`; the script also feeds it the Pilot's speed
/// on control point 1, which none of its operators read.
#[derive(Component)]
pub struct StimFx;

pub fn stim_fx(
    mut commands: Commands,
    control: Res<Control>,
    status: Res<crate::pilotability::PilotStatus>,
    cams: Query<Entity, With<crate::player::MainCamera>>,
    fx: Query<Entity, With<StimFx>>,
) {
    let on = *control == Control::Pilot && status.speed_scale() > 1.0;
    match (on, fx.iter().next()) {
        (true, None) => {
            let Ok(cam) = cams.single() else { return };
            let child = commands
                .spawn((
                    Transform::from_xyz(0.0, 0.0, -HEALTH_HEX_DIST * crate::player::UNIT).with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                    Visibility::default(),
                    crate::pfx::PfxTrail::oriented("P_heal"),
                    StimFx,
                ))
                .id();
            commands.entity(cam).add_child(child);
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
    let now = if health.v.doomed.is_some() { 0.0 } else { health.v.health };
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
        let (lo, hi) = (seg.0 as f32 * crate::combat::SEGMENT, (seg.0 + 1) as f32 * crate::combat::SEGMENT);
        let (a, b) = (new.max(lo), old.min(hi));
        if b <= a {
            bg.0 = Color::NONE;
            continue;
        }
        node.left = percent((a - lo) / crate::combat::SEGMENT * 100.0);
        node.width = percent((b - a) / crate::combat::SEGMENT * 100.0);
        let alpha = if t < HOLD { 1.0 } else { 1.0 - (t - HOLD) / FADE };
        bg.0 = Color::srgba(1.0, 0.35, 0.25, alpha.clamp(0.0, 1.0));
    }
}
