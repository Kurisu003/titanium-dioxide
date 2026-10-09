//! BT's offhand viewmodels: the first-person models the kit's abilities play (the Vortex
//! blocker's raised left hand, the shoulder rocket pod, Scorch's Firewall slam, the cores'
//! animations), each with BT's arms bone-merged as on the gun's. What a slot plays comes from
//! its weapon script: `viewmodel`, `deploy_time`, `holster_time` and
//! `offhand_keep_primary_in_hand` (when it isn't set, the offhand replaces the gun's viewmodel
//! while it plays; when it is, the offhand's animations move only the left arm and the gun's
//! viewmodel keeps the right hand). The `empty_handed` models (Scorch's Firewall and Flame
//! Core) slam with the right fist, so the gun is put away while they play and drawn again after
//! (a guess: their scripts keep the primary in hand, but their animation uses the gun hand).
//!
//! Using an ability plays draw_seq over deploy_time. A held ability (the Vortex and Thermal
//! Shields, Sword Block, the Multi-Target Missiles' lock-on) then holds the end of the draw with
//! charge_seq; firing or letting go plays attack_seq, then holster_seq over holster_time. An
//! attack stored as a delta (the Vortex blocker's: a push that drops the arm out of view), or a
//! model without a holster (the empty-handed ones), ends the offhand with the attack.
//!
//! The abilities aren't told about this: their use is read from their state (a held flag, a
//! cooldown that jumps up, a utility charge spent, the core leaving its build-up).

use crate::actor::{Actor, ActorSpec, BoneMergeSplit, BoneMergeTo, Layer};
use crate::player::{CameraMode, PlayerTitan};
use crate::weapons::Weapon;
use bevy::prelude::*;
use std::collections::HashMap;

/// An offhand weapon's first-person data from its script.
#[derive(Clone, Debug)]
struct OffhandDef {
    path: String,
    deploy: f32,
    holster: f32,
    keep_primary: bool,
    /// An `empty_handed` model: its animation takes the gun hand.
    empty_hand: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Hidden,
    Draw,
    Hold,
    Attack,
    Holster,
}

struct Inst {
    script: &'static str,
    def: OffhandDef,
    anchor: Entity,
    actor: Entity,
    /// BT's arms bone-merged onto it.
    arms: Option<Entity>,
    phase: Phase,
    t: f32,
}

#[derive(Resource, Default)]
pub struct Offhands {
    defs: HashMap<&'static str, Option<OffhandDef>>,
    insts: Vec<Inst>,
    /// Last frame's ordnance and defensive cooldowns, utility charges and whether the core
    /// was building, for spotting a use.
    prev: Option<(f32, f32, u32, bool, bool)>,
}

fn load_def(gd: &crate::gamedata::GameData, script: &str) -> Option<OffhandDef> {
    let mut read = |p: &str| gd.read_file(p).ok().map(|b| String::from_utf8_lossy(&b).to_string());
    let s = tf_assets::settings::PlayerSettings::load(&format!("scripts/weapons/{script}.txt"), true, &mut read)?;
    let f = |k: &str, d: f32| s.get(&format!(".{k}")).and_then(|v| v.trim().parse().ok()).unwrap_or(d);
    // (A path with a doubled slash, the Laser Core's `models//weapons/...`, reads as a comment;
    // take the quoted value from the file then.)
    let raw = || {
        let text = read(&format!("scripts/weapons/{script}.txt"))?;
        let line = text.lines().find(|l| l.trim_start().starts_with("\"viewmodel\""))?;
        line.split('"').nth(3).map(str::to_string)
    };
    let path = s.get(".viewmodel").map(|v| v.trim().to_string()).filter(|v| v.ends_with(".mdl")).or_else(raw);
    let path = path.map(|v| v.replace("//", "/")).filter(|v| !v.is_empty());
    let path = match path {
        Some(p) => p,
        // Ion's shield script only overrides numbers; its viewmodel is the Vortex blocker.
        None if script.ends_with("_ion") => return load_def(gd, script.trim_end_matches("_ion")),
        None => return None,
    };
    let empty_hand = path.contains("/empty_handed/");
    Some(OffhandDef { path, deploy: f("deploy_time", 0.2), holster: f("holster_time", 0.2), keep_primary: f("offhand_keep_primary_in_hand", 0.0) > 0.5, empty_hand })
}

/// Spawn, animate and place the current kit's offhand viewmodels, on the gun viewmodel's eye
/// frame so they share the gun's sway.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn offhand_viewmodels(
    mut commands: Commands,
    time: Res<Time>,
    mode: Res<CameraMode>,
    control: Res<crate::pilotctl::Control>,
    kit: Res<crate::titankit::ActiveKit>,
    mut offhands: ResMut<Offhands>,
    gd: Res<crate::gamedata::GameData>,
    root: Option<Res<crate::pilotweapon::WorldRoot>>,
    mut assets: (
        ResMut<crate::convert::Cache>,
        ResMut<Assets<Mesh>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    ),
    mut titans: Query<(&mut Weapon, &crate::combat::Vortex, &crate::abilities::Ordnance, &crate::abilities::TitanCore, &crate::titankit::KitState), With<PlayerTitan>>,
    mut actors: Query<&mut Actor>,
    mut tfs: Query<(&mut Transform, &mut Visibility), Without<PlayerTitan>>,
    merges: Query<(Entity, &BoneMergeTo, Has<BoneMergeSplit>)>,
) {
    use crate::titankit::{DefKind, OrdKind};
    let Ok((mut w, vortex, ord, core, st)) = titans.single_mut() else { return };
    let scripts = crate::titankit::kit_scripts(&kit);

    // Spawn the kit's offhand viewmodels the first time the kit is used.
    let Some(root) = root else { return };
    let o = &mut *offhands;
    for script in scripts {
        if o.insts.iter().any(|i| i.script == script) {
            continue;
        }
        let def = o.defs.entry(script).or_insert_with(|| load_def(&gd, script)).clone();
        let Some(def) = def else { continue };
        let (cache, meshes, images, materials, bindposes) = (&mut *assets.0, &mut *assets.1, &mut *assets.2, &mut *assets.3, &mut *assets.4);
        let anchor = commands.spawn((Transform::IDENTITY, Visibility::Hidden, crate::vmcam::Viewmodel)).id();
        commands.entity(root.0).add_child(anchor);
        let spec = ActorSpec { path: &def.path, sequences: &["?draw_seq", "?charge_seq", "?attack_seq", "?holster_seq"], grids: &[], body: &[] };
        let vm = match crate::actor::spawn_actor(&mut commands, anchor, &gd, &spec, cache, meshes, images, materials, bindposes) {
            Ok(vm) => vm,
            Err(e) => {
                log::warn!("offhand viewmodel {}: {e:#}", def.path);
                o.defs.insert(script, None);
                continue;
            }
        };
        let arms = ActorSpec { path: "models/weapons/arms/buddypov.mdl", sequences: &[], grids: &[], body: &[] };
        let arms = match crate::actor::spawn_actor(&mut commands, anchor, &gd, &arms, cache, meshes, images, materials, bindposes) {
            Ok(a) => {
                commands.entity(a.entity).insert(BoneMergeTo(vm.entity));
                Some(a.entity)
            }
            Err(e) => {
                log::warn!("offhand arms: {e:#}");
                None
            }
        };
        log::info!("offhand viewmodel for {script}: {def:?}");
        o.insts.push(Inst { script, def, anchor, actor: vm.entity, arms, phase: Phase::Hidden, t: 0.0 });
    }

    // What each slot (ordnance, defensive, utility, core) is doing: (held, used this frame,
    // firing). A core is held while it charges; the Laser Core's beam and the Salvo Core's
    // missiles keep firing while it is active.
    let building = matches!(core.state, crate::abilities::CoreState::Building);
    let pending = st.ord_pending.is_some();
    let (p_ord, p_def, p_util, p_build, p_pending) = o.prev.unwrap_or((st.ord_cd, st.def_cd, st.util_charges, building, pending));
    o.prev = Some((st.ord_cd, st.def_cd, st.util_charges, building, pending));
    // Ordnance that waits for its deploy starts its draw on the press; Tracker Rockets when
    // they fire.
    let ord_used = (pending && !p_pending) || (kit.ord == OrdKind::Tracker && st.ord_cd > p_ord + 1e-3);
    let ord_held = kit.ord == OrdKind::Missiles && ord.charging;
    let def_held = match kit.def {
        DefKind::Vortex | DefKind::IonVortex | DefKind::HeatShield => vortex.active,
        DefKind::SwordBlock => st.blocking,
        _ => false,
    };
    let core_charging = matches!(core.state, crate::abilities::CoreState::Charging(_));
    let core_firing = matches!(kit.core, crate::titankit::CoreKind::Laser | crate::titankit::CoreKind::Salvo) && matches!(core.state, crate::abilities::CoreState::Active(_));
    let signals = [
        (ord_held, ord_used, false),
        (def_held, st.def_cd > p_def + 1e-3, false),
        (false, st.util_charges < p_util, false),
        (core_charging, p_build && !building, core_firing),
    ];

    // The gun viewmodel's eye frame: its anchor plus its camera bone.
    let eye_frame = w.viewmodel.and_then(|(xa, xv)| {
        let off = actors.get(xv).ok()?.bone_model_transform("jx_c_camera")?.translation;
        let tf = tfs.get(xa).ok()?.0;
        Some((tf.translation + tf.rotation * off, tf.rotation))
    });
    let cockpit = *mode == CameraMode::Cockpit && *control == crate::pilotctl::Control::Titan;
    let dt = time.delta_secs();
    let mut hide_gun = false;
    let mut left_arm = None;
    let mut redeploy = false;
    for inst in &mut o.insts {
        let slot = scripts.iter().position(|&s| s == inst.script);
        let (held, used, firing) = slot.map(|s| signals[s]).unwrap_or((false, false, false));
        if slot.is_none() {
            inst.phase = Phase::Hidden;
        }
        let Ok(mut actor) = actors.get_mut(inst.actor) else { continue };
        actor.autoplay = false;
        // The abilities cue their own sounds (the draws' events repeat them).
        actor.event_sounds = false;
        let (draw, charge, attack, holster) = (actor.clip("draw_seq"), actor.clip("charge_seq"), actor.clip("attack_seq"), actor.clip("holster_seq"));
        let dur = |c: Option<usize>| c.map(|c| actor.clips[c].duration).unwrap_or(0.0);
        let delta = |c: Option<usize>| c.is_some_and(|c| actor.clips[c].delta);
        let (deploy, holster_time) = (inst.def.deploy.max(0.05), inst.def.holster.max(0.05));
        let was_held = inst.phase == Phase::Hold || (inst.phase == Phase::Draw && held);
        if used || (held && !was_held && matches!(inst.phase, Phase::Hidden | Phase::Attack | Phase::Holster)) {
            inst.phase = Phase::Draw;
            inst.t = 0.0;
        }
        inst.t += dt;
        match inst.phase {
            Phase::Draw if inst.t >= deploy => {
                inst.phase = if held { Phase::Hold } else { Phase::Attack };
                inst.t = 0.0;
            }
            Phase::Hold if !held => {
                inst.phase = Phase::Attack;
                inst.t = 0.0;
            }
            Phase::Attack if inst.t >= dur(attack) && !firing => {
                inst.phase = if delta(attack) || holster.is_none() { Phase::Hidden } else { Phase::Holster };
                inst.t = 0.0;
                if inst.phase == Phase::Hidden && inst.def.empty_hand {
                    redeploy = true;
                }
            }
            Phase::Holster if inst.t >= holster_time => {
                inst.phase = Phase::Hidden;
                redeploy |= inst.def.empty_hand;
            }
            _ => {}
        }
        let t = inst.t;
        let looped = |c: usize| (t / actor.clips[c].duration.max(1e-3)).rem_euclid(1.0);
        let once = |c: usize| (t / actor.clips[c].duration.max(1e-3)).min(0.999);
        let mut layers = Vec::new();
        let mut additive = Vec::new();
        let draw_end = draw.map(|c| Layer { clip: c, cycle: 0.999, weight: 1.0 });
        match inst.phase {
            Phase::Draw => layers.extend(draw.map(|c| Layer { clip: c, cycle: (t / deploy).min(0.999), weight: 1.0 })),
            Phase::Hold => match charge {
                Some(c) if actor.clips[c].delta => {
                    layers.extend(draw_end);
                    additive.push(Layer { clip: c, cycle: looped(c), weight: 1.0 });
                }
                Some(c) => layers.push(Layer { clip: c, cycle: looped(c), weight: 1.0 }),
                None => layers.extend(draw_end),
            },
            Phase::Attack => match attack {
                Some(c) if actor.clips[c].delta => {
                    layers.extend(draw_end);
                    additive.push(Layer { clip: c, cycle: once(c), weight: 1.0 });
                }
                Some(c) => layers.push(Layer { clip: c, cycle: once(c), weight: 1.0 }),
                None => layers.extend(draw_end),
            },
            Phase::Holster => layers.extend(holster.map(|c| Layer { clip: c, cycle: (t / holster_time).min(0.999), weight: 1.0 })),
            Phase::Hidden => {}
        }
        actor.layers = layers;
        actor.additive = additive;
        let offset = actor.bone_model_transform("jx_c_camera").map(|t| t.translation).unwrap_or(Vec3::ZERO);
        let show = inst.phase != Phase::Hidden && cockpit && eye_frame.is_some();
        if show && (!inst.def.keep_primary || inst.def.empty_hand) {
            hide_gun = true;
        }
        // A keep-primary offhand animates only the left arm (its viewmodel parks the right one
        // behind the camera) and the gun keeps the right hand, so BT shows two arms, not the
        // gun's two plus the offhand's left. How the engine joins them isn't traced: the gun's
        // arms take their left arm from the offhand's viewmodel (both hold BT's body in the
        // same place under the camera) and the offhand's own arms are hidden.
        let split = show && inst.def.keep_primary && !inst.def.empty_hand;
        if split {
            left_arm = Some(inst.actor);
        }
        if let Some((_, mut vis)) = inst.arms.and_then(|a| tfs.get_mut(a).ok()) {
            vis.set_if_neq(if split { Visibility::Hidden } else { Visibility::Inherited });
        }
        if let Ok((mut tf, mut vis)) = tfs.get_mut(inst.anchor) {
            vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
            if let Some((eye, rot)) = eye_frame {
                tf.rotation = rot;
                tf.translation = eye - rot * offset;
            }
        }
    }
    if redeploy {
        w.redeploy();
    }
    // The gun's arms follow the offhand for their left arm while one is up.
    if let Some((_, gun)) = w.viewmodel {
        for (e, to, has) in &merges {
            if to.0 != gun {
                continue;
            }
            match left_arm {
                Some(leader) => {
                    commands.entity(e).insert(BoneMergeSplit { leader, root: "def_l_clav" });
                }
                None if has => {
                    commands.entity(e).remove::<BoneMergeSplit>();
                }
                None => {}
            }
        }
    }
    // An offhand that doesn't keep the primary in hand replaces the gun's viewmodel.
    if hide_gun {
        if let Some((xa, _)) = w.viewmodel {
            if let Ok((_, mut vis)) = tfs.get_mut(xa) {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}
