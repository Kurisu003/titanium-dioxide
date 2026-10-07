//! The pilot's arsenal: every pilot weapon is driven by its own weapon script
//! (`scripts/weapons/mp_weapon_*.txt`, SP values): fire rate, auto / semi-auto / burst, clip,
//! reload times, spread and kick, view punch, ADS zoom, hitscan or projectile (with gravity
//! and explosions), and the shotgun bolt patterns from the weapon's `.nut`. Each weapon shows
//! its own first-person viewmodel with the pilot's arms bone-merged onto it.
//!
//! A loadout is a primary, a sidearm and an anti-Titan weapon (keys 1/2/3 or the mouse wheel),
//! switched with the weapons' holster and deploy times.

use crate::actor::{Actor, ActorSpec, BoneMergeTo, GridLayer, Layer};
use crate::audio::{self, Cue, GunPart};
use crate::pilotctl::{Control, PilotSettings, PlayerPilot};
use crate::player::{to_bevy, CameraMode, Collision, MainCamera, PlayerInput};
use crate::targets::Enemy;
use crate::weapons::{Fx, FxAssets, ViewPunch, WeaponDef};
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use tf_sim::glam::Vec3 as SVec3;
use tf_sim::pilot::PilotMove;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Primary,
    Sidearm,
    AntiTitan,
}

/// First-person fire sounds, as source-name prefixes in the Miles bank.
pub struct GunSound {
    pub first: &'static [&'static str],
    pub shot: &'static [&'static str],
    pub tail: &'static [&'static str],
}

pub struct ArsenalEntry {
    pub id: &'static str,
    pub slot: Slot,
    pub sound: GunSound,
}

macro_rules! gun {
    ($id:literal, $slot:ident, [$($f:literal),*], [$($s:literal),*], [$($t:literal),*]) => {
        ArsenalEntry { id: $id, slot: Slot::$slot, sound: GunSound { first: &[$($f),*], shot: &[$($s),*], tail: &[$($t),*] } }
    };
}

/// The pilot weapons of Titanfall 2, with their fire sounds.
pub const ARSENAL: &[ArsenalEntry] = &[
    gun!("mp_weapon_rspn101", Primary, ["wpn_r101_1p_wpnfire_firstshot_core"], ["wpn_r101_1p_wpnfire_loop_core2"], ["wpn_r101_1p_wpnfire_tail_core"]),
    gun!("mp_weapon_rspn101_og", Primary, ["wpn_cbr101_1p_wpnfire_firstshot_core"], ["wpn_cbr101_1p_wpnfire_loop_core"], ["wpn_cbr101_1p_wpnfire_tail_core"]),
    gun!("mp_weapon_hemlok", Primary, ["wpn_hemlok_1p_wpnfire_core_3shotburst"], [], []),
    gun!("mp_weapon_g2", Primary, [], ["wpn_g2a4_1p_wpnfire_shot"], ["wpn_g2a4_1p_wpnfire_tail"]),
    gun!("mp_weapon_vinson", Primary, ["wpn_r101_1p_wpnfire_firstshot_core"], ["wpn_r101_1p_wpnfire_loop_core2"], ["wpn_r101_1p_wpnfire_tail_core"]),
    gun!("mp_weapon_car", Primary, ["wpn_r97_1p_wpnfire_firstshot_core"], ["wpn_r97_1p_wpnfire_loop_core"], []),
    gun!("mp_weapon_r97", Primary, ["wpn_r97_1p_wpnfire_firstshot_core"], ["wpn_r97_1p_wpnfire_loop_core"], []),
    gun!("mp_weapon_alternator_smg", Primary, [], ["wpn_alternator_3p_wpnfire_close"], []),
    gun!("mp_weapon_hemlok_smg", Primary, ["wpn_r97_1p_wpnfire_firstshot_core"], ["wpn_r97_1p_wpnfire_loop_core"], []),
    gun!("mp_weapon_lmg", Primary, ["wpn_lmg_1p_wpnfire_firstshot_core"], ["wpn_lmg_1p_wpnfire_loop_core"], ["wpn_lmg_1p_wpnfire_tail_core"]),
    gun!("mp_weapon_lstar", Primary, ["wpn_lstar_1p_wpnfire_firstshot_6ch"], ["wpn_lstar_1p_wpnfire_loop_corec"], ["wpn_lstar_1p_wpnfire_warbletail"]),
    gun!("mp_weapon_esaw", Primary, ["wpn_lmg_1p_wpnfire_firstshot_core"], ["wpn_lmg_1p_wpnfire_loop_core"], ["wpn_lmg_1p_wpnfire_tail_core"]),
    gun!("mp_weapon_sniper", Primary, [], ["wpn_krabersniper_1p_wpnfire_midshot"], ["wpn_krabersniper_1p_wpnfire_lsrstail_2ch"]),
    gun!("mp_weapon_doubletake", Primary, [], ["wpn_doubletake_1p_wpnfire_harshmetal"], ["wpn_doubletake_1p_wpnfire_tail_mid"]),
    gun!("mp_weapon_dmr", Primary, [], ["wpn_dmr_1p_wpnfire_reduced"], ["wpn_dmr_1p_wpnfire_taillr"]),
    gun!("mp_weapon_shotgun", Primary, [], ["wpn_shotgun_1p_wpnfire_reduced"], []),
    gun!("mp_weapon_mastiff", Primary, [], ["wpn_mastiff_1p_wpnfire_fire"], ["wpn_mastiff_1p_wpnfire_tail_6ch"]),
    gun!("mp_weapon_epg", Primary, [], ["wpn_epg_3p_shot_close"], []),
    gun!("mp_weapon_softball", Primary, [], ["wpn_softball_1p_wpnfire_shot"], []),
    gun!("mp_weapon_smr", Primary, [], ["wpn_epg_3p_shot_close"], []),
    gun!("mp_weapon_pulse_lmg", Primary, [], ["wpn_coldwar_1p_fire_burst_servopunch"], []),
    gun!("mp_weapon_smart_pistol", Primary, [], ["wpn_smartpistol_1p_wpnfire_core"], []),
    gun!("mp_weapon_semipistol", Sidearm, [], ["wpn_2011pistol_1p_wpnfire_shot"], ["wpn_2011pistol_1p_wpnfire_tail"]),
    gun!("mp_weapon_autopistol", Sidearm, [], ["wpn_re45_3p_wpnfire_firstshot_mid"], []),
    gun!("mp_weapon_wingman", Sidearm, [], ["wpn_wingman_1p_wpnfire_shot"], []),
    gun!("mp_weapon_wingman_n", Sidearm, [], ["wpn_wingman_1p_wpnfire_shot"], []),
    gun!("mp_weapon_shotgun_pistol", Sidearm, [], ["wpn_shotgun_1p_wpnfire_reduced"], []),
    gun!("mp_weapon_defender", AntiTitan, [], ["wpn_chargerifle_1p_fire_beam"], []),
    gun!("mp_weapon_mgl", AntiTitan, [], ["wpn_mglv2_1p_wpnfire_close"], []),
    gun!("mp_weapon_arc_launcher", AntiTitan, [], ["wpn_softball_1p_wpnfire_shot"], []),
    gun!("mp_weapon_rocket_launcher", AntiTitan, [], ["wpn_softball_1p_wpnfire_shot"], []),
];

pub fn arsenal_index(id: &str) -> Option<usize> {
    ARSENAL.iter().position(|e| e.id == id)
}

/// Bolt patterns from the weapons' scripts: (up, right) offsets scaled by the spread fraction.
fn bolt_pattern(id: &str) -> Option<&'static [[f32; 2]]> {
    const MASTIFF: [[f32; 2]; 8] = [[0.0, 0.15], [0.0, 0.3], [0.0, 0.6], [0.0, 1.2], [0.0, -0.3], [0.0, -0.6], [0.0, -1.2], [0.0, -0.15]];
    const MOZAMBIQUE: [[f32; 2]; 3] = [[-0.2, -0.4], [-0.2, 0.4], [0.0, 0.0]];
    const DOUBLETAKE: [[f32; 2]; 3] = [[0.0, -0.25], [0.0, 0.25], [0.0, 0.0]];
    match id {
        "mp_weapon_mastiff" => Some(&MASTIFF),
        "mp_weapon_shotgun_pistol" => Some(&MOZAMBIQUE),
        "mp_weapon_doubletake" => Some(&DOUBLETAKE),
        _ => None,
    }
}

/// The loadout picked in the menu (indices into `ARSENAL`).
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoadoutChoice {
    pub primary: usize,
    pub sidearm: usize,
    pub anti_titan: usize,
}

impl Default for LoadoutChoice {
    fn default() -> Self {
        Self {
            primary: arsenal_index("mp_weapon_rspn101").unwrap_or(0),
            sidearm: arsenal_index("mp_weapon_wingman").unwrap_or(0),
            anti_titan: arsenal_index("mp_weapon_defender").unwrap_or(0),
        }
    }
}

impl LoadoutChoice {
    pub fn get(&self, slot: Slot) -> usize {
        match slot {
            Slot::Primary => self.primary,
            Slot::Sidearm => self.sidearm,
            Slot::AntiTitan => self.anti_titan,
        }
    }
    pub fn set(&mut self, slot: Slot, index: usize) {
        match slot {
            Slot::Primary => self.primary = index,
            Slot::Sidearm => self.sidearm = index,
            Slot::AntiTitan => self.anti_titan = index,
        }
    }
    /// Step a slot through the weapons that fit it.
    pub fn cycle(&mut self, slot: Slot, step: i32) {
        let options: Vec<usize> = ARSENAL.iter().enumerate().filter(|(_, e)| e.slot == slot).map(|(i, _)| i).collect();
        let cur = options.iter().position(|&i| i == self.get(slot)).unwrap_or(0) as i32;
        let next = options[(cur + step).rem_euclid(options.len() as i32) as usize];
        match slot {
            Slot::Primary => self.primary = next,
            Slot::Sidearm => self.sidearm = next,
            Slot::AntiTitan => self.anti_titan = next,
        }
    }
}

/// The world root everything from the game hangs under (game units and axes).
#[derive(Resource, Clone, Copy)]
pub struct WorldRoot(pub Entity);

/// Display names (`#WPN_...` → "R-201 Carbine") from the game's localisation.
#[derive(Resource, Default)]
pub struct Strings(pub std::collections::HashMap<String, String>);

impl Strings {
    pub fn weapon(&self, def: &WeaponDef) -> String {
        self.0.get(&def.printname).cloned().unwrap_or_else(|| def.name.trim_start_matches("mp_weapon_").to_uppercase())
    }
}

/// One gun's state.
pub struct PilotWeapon {
    pub arsenal: usize,
    pub def: WeaponDef,
    pub ammo: u32,
    pub reload_left: f32,
    /// Seconds since the current reload started (for the viewmodel).
    pub reload_t: f32,
    cooldown: f32,
    spread_kick: f32,
    since_fire: f32,
    /// The current spread cone (degrees, full width), for the crosshair.
    pub cur_spread: f32,
    /// How far the fire rate has ramped toward `fire_rate_max` (Devotion).
    rate_ramp: f32,
    /// The stage the running reload started from (0: from the start) and whether it is an
    /// empty reload; and an interrupted reload's stage to resume from.
    pub reload_stage: u8,
    reload_empty: bool,
    reload_resume: Option<(bool, u8)>,
    pub ads: f32,
    /// Seconds since the last shot (for the viewmodel kick).
    pub fire_t: f32,
    burst_left: u32,
    trigger_held: bool,
    firing_audio: bool,
    rng: u64,
    /// Placement entity and viewmodel actor.
    pub viewmodel: Option<(Entity, Entity)>,
    pub charge: crate::weapons::Charge,
    /// Smart ammo locks (Smart Pistol, Archer).
    pub smart: crate::smartammo::Locks,
    /// Rounds in the burst being fired (the script's burst, or one per smart lock).
    burst_size: u32,
    /// The script's `projectilemodel`, for smart missiles.
    projectile: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
    /// Sway pivots (`sway_rotate_attach`, `_zoomed`): the attachment's bone and its offset.
    pub pivots: [(String, Vec3); 2],
    /// The ADS scope's body part and the bone code puts at the eye (`def_c_scope_ads`).
    pub ads_scope: Option<(usize, usize)>,
    /// The muzzle flash and shell eject attachments of the viewmodel.
    pub fx_atts: [Option<(String, Transform)>; 2],
    /// The viewmodel's screens its script's RUIs draw on (ruiscreen.rs).
    pub screens: Vec<crate::ruiscreen::ScreenMesh>,
}

impl PilotWeapon {
    pub fn new(arsenal: usize, def: WeaponDef, viewmodel: Option<(Entity, Entity)>) -> Self {
        Self {
            arsenal,
            ammo: def.clip,
            def,
            reload_left: 0.0,
            reload_t: 0.0,
            cooldown: 0.0,
            spread_kick: 0.0,
            since_fire: 9.0,
            cur_spread: 0.0,
            rate_ramp: 0.0,
            reload_stage: 0,
            reload_empty: false,
            reload_resume: None,
            ads: 0.0,
            fire_t: 9.0,
            burst_left: 0,
            trigger_held: false,
            firing_audio: false,
            rng: 0x51ED_2701 ^ arsenal as u64 * 7919,
            viewmodel,
            charge: Default::default(),
            smart: Default::default(),
            burst_size: 1,
            projectile: Vec::new(),
            pivots: Default::default(),
            ads_scope: None,
            fx_atts: [None, None],
            screens: Vec::new(),
        }
    }
    pub fn reloading(&self) -> bool {
        self.reload_left > 0.0
    }
    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[derive(Component)]
pub struct PilotLoadout {
    pub guns: Vec<PilotWeapon>,
    pub active: usize,
    /// Switching: the weapon being put away and the one coming up, and time spent so far.
    pub switching: Option<(usize, f32)>,
    /// Seconds since the active weapon was drawn.
    pub deploy_t: f32,
    choice: Option<LoadoutChoice>,
    vm_cycle: f32,
    sprint_blend: f32,
    /// Seconds since the gun was last lowered by sprinting (it fires once `raise_time` passes).
    raise_t: f32,
    /// Viewmodel layer clocks: real time for `walk_seq`/`idle_seq_autoplay`, seconds since
    /// leaving the ground / landing / the sprint ending, and what they were last frame.
    vm_layers: VmLayers,
    vm_motion: crate::vmmotion::VmMotion,
    /// The empty-handed melee viewmodel (`ptpov_emptyhand.mdl` with the Pilot's arms).
    melee_vm: Option<(Entity, Entity)>,
    /// The selected ordnance's throw viewmodel: ordnance index, anchor, actor.
    ord_vm: Option<(usize, Entity, Entity)>,
    /// The selected tactical's, for thrown tacticals (Pulse Blade, A-Wall).
    tac_vm: Option<(usize, Entity, Entity)>,
}

#[derive(Default, Clone, Copy)]
struct VmLayers {
    clock: f32,
    jump_t: f32,
    land_t: f32,
    sprint_end_t: f32,
    was_ground: bool,
    was_sprint: bool,
    jumps: u32,
    ground_blend: f32,
}

impl Default for PilotLoadout {
    fn default() -> Self {
        Self { guns: Vec::new(), active: 0, switching: None, deploy_t: 9.0, choice: None, vm_cycle: 0.0, sprint_blend: 0.0, raise_t: 9.0, vm_layers: VmLayers { jump_t: 9.0, land_t: 9.0, sprint_end_t: 9.0, ..Default::default() }, vm_motion: Default::default(), melee_vm: None, ord_vm: None, tac_vm: None }
    }
}

impl PilotLoadout {
    pub fn active(&self) -> Option<&PilotWeapon> {
        self.guns.get(self.active)
    }
    /// The gun is up (not mid-switch or still drawing).
    pub fn ready(&self) -> bool {
        self.switching.is_none() && self.active().is_some_and(|g| self.deploy_t >= g.def.deploy_time.min(1.0) * 0.6)
    }
}

/// (Re)build the loadout's guns and viewmodels when the choice changes.
#[allow(clippy::too_many_arguments)]
pub fn build_loadout(
    mut commands: Commands,
    choice: Res<LoadoutChoice>,
    gd: Res<crate::gamedata::GameData>,
    mut cache: ResMut<crate::convert::Cache>,
    root: Res<WorldRoot>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut pilots: Query<&mut PilotLoadout>,
) {
    let Ok(mut loadout) = pilots.single_mut() else { return };
    if loadout.choice == Some(*choice) {
        return;
    }
    let t = std::time::Instant::now();
    for g in loadout.guns.drain(..) {
        if let Some((anchor, _)) = g.viewmodel {
            commands.entity(anchor).despawn();
        }
    }
    let mut read = |p: &str| gd.read_file(p).ok().map(|b| String::from_utf8_lossy(&b).to_string());
    let defs: Vec<(usize, WeaponDef)> = [Slot::Primary, Slot::Sidearm, Slot::AntiTitan]
        .into_iter()
        .map(|slot| {
            let index = choice.get(slot);
            (index, WeaponDef::load_player(ARSENAL[index].id, &mut read))
        })
        .collect();
    // Parse the viewmodels (and the arms) on every core first: one by one they held the first
    // second of the game for 0.9 s.
    let mut paths: Vec<String> = defs.iter().map(|(_, d)| d.viewmodel.clone()).collect();
    paths.push(crate::fparms::arms_model().to_string());
    paths.push(MELEE_VIEWMODEL.to_string());
    crate::actor::prefetch_models(&gd, &paths);
    for (index, def) in defs {
        let anchor = commands.spawn((Transform::IDENTITY, Visibility::Hidden, crate::vmcam::Viewmodel)).id();
        commands.entity(root.0).add_child(anchor);
        let vm_model = crate::actor::parsed_model(&gd, &def.viewmodel).ok();
        let body = vm_model.as_ref().map(|m| def.body_for(m)).unwrap_or_default();
        let ads_scope = vm_model.as_ref().zip(def.ads_scope.as_ref()).and_then(|(m, (name, _))| {
            Some((m.bodyparts.iter().position(|b| b.name.eq_ignore_ascii_case(name))?, m.bone_index("def_c_scope_ads")?))
        });
        let spec = ActorSpec {
            path: &def.viewmodel,
            sequences: &[
                "draw_seq", "holster_seq", "reload_seq", "reload_empty_seq", "sprint_seq", "sprintraise_seq", "ads_in_seq", "?rechamber_seq", "?raise_frommelee_seq", "?raise_seq",
                "?reload_late1_seq", "?reload_late2_seq", "?reload_late3_seq", "?reload_empty_late1_seq", "?reload_empty_late2_seq", "?reload_empty_late3_seq",
            ],
            // The engine's additive viewmodel layers (the snipers and the Charge Rifle name
            // their jump/land transitions without `_iron`).
            grids: &["attack_seq", "?idle_seq_autoplay", "?walk_seq", "?run_layer_reload", "?switch_to_jump_iron", "?switch_to_land_iron", "?switch_to_jump", "?switch_to_land"],
            body: &body,
        };
        let vm = crate::actor::spawn_actor(&mut commands, anchor, &gd, &spec, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes);
        let viewmodel = match vm {
            Ok(vm) => {
                let arms = ActorSpec { path: crate::fparms::arms_model(), sequences: &[], grids: &[], body: &[] };
                match crate::actor::spawn_actor(&mut commands, anchor, &gd, &arms, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes) {
                    Ok(a) => {
                        commands.entity(a.entity).insert(BoneMergeTo(vm.entity));
                    }
                    Err(e) => log::warn!("pilot arms: {e:#}"),
                }
                Some((anchor, vm.entity))
            }
            Err(e) => {
                log::warn!("viewmodel {}: {e:#}", def.viewmodel);
                None
            }
        };
        // Smart launchers fire the script's projectile model.
        let mut projectile = Vec::new();
        if def.smart.as_ref().is_some_and(|s| s.missile) && !def.projectile_model.is_empty() {
            match crate::world::build_static_model(&gd, &def.projectile_model, &mut cache, &mut meshes, &mut images, &mut materials) {
                Ok((parts, _)) => projectile = parts,
                Err(e) => log::warn!("projectile {}: {e:#}", def.projectile_model),
            }
        }
        let pivots = sway_pivots(&gd, &def);
        let mut gun = PilotWeapon::new(index, def, viewmodel);
        gun.projectile = projectile;
        gun.pivots = pivots;
        gun.ads_scope = ads_scope;
        gun.fx_atts = [
            vm_model.as_ref().and_then(|m| model_attachment(m, if gun.def.fx.muzzle_attach.is_empty() { "muzzle_flash" } else { &gun.def.fx.muzzle_attach })),
            vm_model.as_ref().and_then(|m| model_attachment(m, if gun.def.fx.shell_attach.is_empty() { "shell" } else { &gun.def.fx.shell_attach })),
        ];
        gun.screens = vm_model.as_ref().map(|m| crate::ruiscreen::screens_for(m, &gun.def.rui)).unwrap_or_default();
        log::debug!("{}: body {:?}, ads scope {:?} ({:?}), screens {:?}", gun.def.name, body, ads_scope, gun.def.ads_scope, gun.screens.iter().map(|s| &s.ui).collect::<Vec<_>>());
        loadout.guns.push(gun);
    }
    if loadout.melee_vm.is_none() {
        let anchor = commands.spawn((Transform::IDENTITY, Visibility::Hidden, crate::vmcam::Viewmodel)).id();
        commands.entity(root.0).add_child(anchor);
        let spec = ActorSpec { path: MELEE_VIEWMODEL, sequences: MELEE_SEQS, grids: &[], body: &[] };
        match crate::actor::spawn_actor(&mut commands, anchor, &gd, &spec, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes) {
            Ok(vm) => {
                let arms = ActorSpec { path: crate::fparms::arms_model(), sequences: &[], grids: &[], body: &[] };
                if let Ok(a) = crate::actor::spawn_actor(&mut commands, anchor, &gd, &arms, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes) {
                    commands.entity(a.entity).insert(BoneMergeTo(vm.entity));
                }
                loadout.melee_vm = Some((anchor, vm.entity));
            }
            Err(e) => log::warn!("melee viewmodel: {e:#}"),
        }
    }
    loadout.active = 0;
    loadout.switching = None;
    loadout.deploy_t = 0.0;
    loadout.choice = Some(*choice);
    log::info!("loadout: {:?} in {:?}", loadout.guns.iter().map(|g| g.def.name.as_str()).collect::<Vec<_>>(), t.elapsed());
}

/// A model attachment: its bone's name and its frame in that bone's space.
pub fn model_attachment(m: &tf_assets::mdl::Model, name: &str) -> Option<(String, Transform)> {
    let a = m.attachments.iter().find(|a| a.name.eq_ignore_ascii_case(name))?;
    let l = a.local;
    let rot = Mat3::from_cols(Vec3::new(l[0][0], l[1][0], l[2][0]), Vec3::new(l[0][1], l[1][1], l[2][1]), Vec3::new(l[0][2], l[1][2], l[2][2]));
    Some((m.bones.get(a.bone)?.name.clone(), Transform::from_translation(Vec3::new(l[0][3], l[1][3], l[2][3])).with_rotation(Quat::from_mat3(&rot).normalize())))
}

/// Where an attachment of a viewmodel actor is (Bevy space position and rotation).
pub fn vm_attachment(actors: &Query<&Actor>, globals: &Query<&GlobalTransform>, vm: Entity, att: &(String, Transform)) -> Option<(Vec3, Quat)> {
    let j = actors.get(vm).ok()?.joint(&att.0)?;
    let g = globals.get(j).ok()?.mul_transform(att.1);
    let (_, r, t) = g.to_scale_rotation_translation();
    Some((t, r))
}

/// The selected ordnance's viewmodel (its script's `viewmodel`, with the Pilot's arms), for
/// the throw; rebuilt when the ordnance changes.
#[allow(clippy::too_many_arguments)]
pub fn ordnance_viewmodel(
    mut commands: Commands,
    status: Res<crate::pilotability::PilotStatus>,
    defs: Option<Res<crate::pilotability::PilotAbilityDefs>>,
    gd: Res<crate::gamedata::GameData>,
    mut cache: ResMut<crate::convert::Cache>,
    root: Res<WorldRoot>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut pilots: Query<&mut PilotLoadout>,
) {
    let (Some(defs), Ok(mut lo)) = (defs, pilots.single_mut()) else { return };
    if lo.choice.is_none() {
        return;
    }
    let oi = status.ordnance.min(defs.ordnance.len().saturating_sub(1));
    let ti = status.tactical.min(defs.tactical.len().saturating_sub(1));
    let mut spawn = |path: &str, idx: usize, commands: &mut Commands| -> (usize, Entity, Entity) {
        let anchor = commands.spawn((Transform::IDENTITY, Visibility::Hidden, crate::vmcam::Viewmodel)).id();
        commands.entity(root.0).add_child(anchor);
        if path.is_empty() {
            return (idx, anchor, Entity::PLACEHOLDER);
        }
        let spec = ActorSpec { path, sequences: &["toss_prep_pullout_seq", "?toss_hold_seq", "toss_seq", "?idle_seq"], grids: &[], body: &[] };
        match crate::actor::spawn_actor(commands, anchor, &gd, &spec, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes) {
            Ok(vm) => {
                let arms = ActorSpec { path: crate::fparms::arms_model(), sequences: &[], grids: &[], body: &[] };
                if let Ok(a) = crate::actor::spawn_actor(commands, anchor, &gd, &arms, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes) {
                    commands.entity(a.entity).insert(BoneMergeTo(vm.entity));
                }
                (idx, anchor, vm.entity)
            }
            Err(e) => {
                log::warn!("throw viewmodel {path}: {e:#}");
                (idx, anchor, Entity::PLACEHOLDER)
            }
        }
    };
    if !lo.ord_vm.is_some_and(|(i, ..)| i == oi) {
        if let Some((_, anchor, _)) = lo.ord_vm.take() {
            commands.entity(anchor).despawn();
        }
        let path = defs.ordnance[oi].viewmodel.clone();
        lo.ord_vm = Some(spawn(&path, oi, &mut commands));
    }
    if !lo.tac_vm.is_some_and(|(i, ..)| i == ti) {
        if let Some((_, anchor, _)) = lo.tac_vm.take() {
            commands.entity(anchor).despawn();
        }
        let path = defs.tactical[ti].viewmodel.clone();
        lo.tac_vm = Some(spawn(&path, ti, &mut commands));
    }
}

/// `melee_pilot_emptyhanded.txt`'s viewmodel and its ACT_VM_MELEE_ATTACK1 sequences (the
/// engine picks one per swing).
const MELEE_VIEWMODEL: &str = "models/weapons/empty_handed/ptpov_emptyhand.mdl";
const MELEE_SEQS: &[&str] = &["melee_02_seq", "melee_03_seq", "melee_04_seq", "melee_05_seq", "melee_06_seq", "melee_07_seq", "melee_09_seq"];

/// The viewmodel attachments the sway rotates about (hip and zoomed): bone name and offset.
pub fn sway_pivots(gd: &crate::gamedata::GameData, def: &WeaponDef) -> [(String, Vec3); 2] {
    let model = gd.read_file(&def.viewmodel).ok().and_then(|b| tf_assets::mdl::Model::parse(b).ok());
    let find = |name: &str| -> (String, Vec3) {
        let Some(m) = model.as_ref() else { return Default::default() };
        m.attachments
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .and_then(|a| m.bones.get(a.bone).map(|b| (b.name.clone(), Vec3::new(a.local[0][3], a.local[1][3], a.local[2][3]))))
            .unwrap_or_default()
    };
    [find(&def.motion.hip.attach), find(&def.motion.zoomed.attach)]
}

/// A projectile from a pilot weapon (game units).
#[derive(Component)]
pub struct PilotBolt {
    pos: Vec3,
    vel: Vec3,
    gravity: f32,
    travelled: f32,
    damage: (f32, f32, f32, f32),
    explosion: (f32, f32, f32),
    stops_regen: bool,
    life: f32,
    /// Fired by BT: hits build his core meter.
    from_titan: bool,
    /// Multipliers for a head hit on infantry and a crit on a Titan.
    headshot: f32,
    crit: f32,
    /// A smart missile's target and turn rate (degrees/s).
    homing: Option<(Entity, f32)>,
    /// The weapon's impact table (what its hits look like).
    impact_table: &'static str,
    /// Its burn mod's damage and explosion, taken on when it leaves through your A-Wall.
    burn: Option<((f32, f32), (f32, f32, f32))>,
}

impl PilotBolt {
    fn new(pos: Vec3, vel: Vec3, gravity: f32, def: &WeaponDef, from_titan: bool) -> Self {
        Self {
            pos,
            vel,
            gravity,
            travelled: 0.0,
            damage: (def.damage_near, def.damage_far, def.near_dist, def.far_dist),
            explosion: (def.explosion_damage, def.explosion_inner_radius, def.explosion_radius),
            stops_regen: def.stops_regen,
            life: 6.0,
            from_titan,
            headshot: def.headshot_scale,
            crit: if def.crit { def.crit_scale } else { 1.0 },
            homing: None,
            impact_table: def.impact_table,
            burn: def.burn.as_deref().filter(|_| !from_titan).map(|b| ((b.damage_near, b.damage_far), (b.explosion_damage, b.explosion_inner_radius, b.explosion_radius))),
        }
    }
}

/// Fire a projectile with a weapon's damage and explosion values (game units).
pub fn spawn_bolt(commands: &mut Commands, fx: &FxAssets, pos: Vec3, vel: Vec3, gravity: f32, def: &WeaponDef, from_titan: bool) {
    let _ = fx;
    let size = if from_titan { 0.35 } else { 0.12 };
    let energy = ["particle_accelerator", "sniper", "arc", "lstar", "plasma"].iter().any(|k| def.name.contains(k));
    let color = if energy { Vec3::new(1.0, 2.5, 6.0) } else if def.name.contains("meteor") { Vec3::new(6.0, 2.0, 0.5) } else { Vec3::new(5.0, 2.6, 1.0) };
    let smoke = !energy && def.explosion_radius > 0.0;
    commands.spawn((
        Transform::from_translation(to_bevy(pos)),
        crate::particles::Glow::new(color, size, smoke),
        PilotBolt::new(pos, vel, gravity, def, from_titan),
    ));
}

/// Launch a smart missile (the Archer's rocket): the script's projectile model with a smoke
/// trail, homing on `target` when it has one (`SmartAmmo_FireWeapon_HomingMissile`; missiles
/// live 10 s).
fn spawn_smart_missile(commands: &mut Commands, pos: Vec3, vel: Vec3, def: &WeaponDef, target: Option<Entity>, homing: f32, model: &[(Handle<Mesh>, Handle<StandardMaterial>)]) {
    let mut bolt = PilotBolt::new(pos, vel, 0.0, def, false);
    bolt.life = 10.0;
    bolt.homing = target.map(|t| (t, homing));
    let id = commands
        .spawn((
            Transform::from_translation(to_bevy(pos)).with_rotation(Quat::from_rotation_arc(Vec3::Y, to_bevy(vel).normalize_or(Vec3::Y))),
            Visibility::default(),
            crate::particles::Glow::new(Vec3::new(5.0, 2.6, 1.0), 0.1, true),
            bolt,
        ))
        .id();
    // The model is in game axes (nose along +Z): turn it into Bevy's Y-up, metre space.
    let local = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)).with_scale(Vec3::splat(crate::player::UNIT));
    for (mesh, mat) in model {
        let part = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), local)).id();
        commands.entity(id).add_child(part);
    }
}

fn view_dir(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin())
}

#[allow(clippy::too_many_arguments)]
pub fn pilot_weapon_fire(
    mut commands: Commands,
    rodeo: Res<crate::rodeo::Rodeo>,
    time: Res<Time>,
    control: Res<Control>,
    mode: Res<CameraMode>,
    mut input: ResMut<PlayerInput>,
    world: Res<Collision>,
    settings: Res<PilotSettings>,
    fx: Res<FxAssets>,
    mut punch: ResMut<ViewPunch>,
    mut pilots: Query<(&PlayerPilot, &mut PilotLoadout)>,
    mut enemies: Query<(Entity, &mut Enemy)>,
    actors: Query<&Actor>,
    globals: Query<&GlobalTransform>,
    (hitboxes, shields): (Option<Res<crate::hitbox::EnemyHitboxes>>, Res<crate::pilotability::Shields>),
    (mut hit_feedback, mut enemy_walls, springs, pfx, melee_state, throw_state, lenses): (
        ResMut<crate::hitbox::RecentHits>,
        ResMut<crate::titankit::EnemyWalls>,
        Res<crate::vmmotion::WeaponSprings>,
        Option<Res<crate::pfx::Pfx>>,
        Res<crate::executions::PilotMelee>,
        Res<crate::pilotability::OrdnanceThrow>,
        Query<(&GlobalTransform, &Projection, Has<crate::vmcam::ViewmodelCamera>), Or<(With<crate::player::MainCamera>, With<crate::vmcam::ViewmodelCamera>)>>,
    ),
) {
    let pfx_ready = pfx.as_deref().is_some_and(|p| p.ready());
    let melee_busy = melee_state.busy() || throw_state.busy();
    let dt = time.delta_secs();
    let no_boxes = crate::hitbox::EnemyHitboxes::default();
    let hitboxes = hitboxes.as_deref().unwrap_or(&no_boxes);
    let Ok((pilot, mut lo)) = pilots.single_mut() else { return };
    let lo = &mut *lo;
    lo.deploy_t += dt;
    for g in &mut lo.guns {
        g.fire_t += dt;
    }
    if *control == Control::Pilot {
        punch.step(dt, lo.guns.get(lo.active).map(|g| &g.def.kick));
    }
    if *control != Control::Pilot || *mode == CameraMode::Free || lo.guns.is_empty() || rodeo.riding() {
        for g in &mut lo.guns {
            g.ads = 0.0;
            g.smart.clear();
        }
        input.pilot_slot = None;
        // The gun is drawn again when it comes back to the Pilot's hands (after disembarking,
        // a rodeo or a first-person sequence).
        lo.deploy_t = 0.0;
        return;
    }
    // Weapon switching: holster the current one, then draw the next.
    if let Some(slot) = input.pilot_slot.take() {
        let slot = slot as usize;
        if slot < lo.guns.len() && slot != lo.active && lo.switching.is_none() {
            if let Some(g) = lo.guns.get_mut(lo.active) {
                // An interrupted reload resumes from the furthest stage it reached (the
                // `_late` times: once the remaining time is within one, that stage is done).
                if g.reload_left > 0.0 {
                    let lates = if g.reload_empty { g.def.reloadempty_late } else { g.def.reload_late };
                    let stage = (1..=3u8).filter(|&n| lates[n as usize - 1] > 0.0 && g.reload_left <= lates[n as usize - 1]).max().unwrap_or(0).max(g.reload_stage);
                    g.reload_resume = (stage > 0).then_some((g.reload_empty, stage));
                }
                g.reload_left = 0.0;
                g.burst_left = 0;
                g.smart.clear();
            }
            if lo.guns[slot].def.fast_swap {
                // fast_swap_to (the Smart Pistol, Quick Swap): the new gun is up at once.
                lo.active = slot;
                lo.deploy_t = 9.0;
            } else {
                lo.switching = Some((slot, 0.0));
            }
        }
    }
    if let Some((to, t)) = lo.switching {
        let t = t + dt;
        let holster = lo.guns[lo.active].def.holster_time.min(0.6);
        if t >= holster {
            lo.active = to;
            lo.switching = None;
            lo.deploy_t = 0.0;
        } else {
            lo.switching = Some((to, t));
        }
    }
    let ready = lo.ready() && !melee_busy;
    let active = lo.active;
    let s = &pilot.state;
    // Only sprinting on the ground lowers the gun: Pilots shoot and aim while wall-running,
    // sliding and in the air (wallrunAdsType "ADS"), even right after a sprint jump.
    let running = s.sprinting && s.mode == PilotMove::Ground;
    // Sprinting lowers the gun (unless primary_fire_does_not_block_sprint); once it stops, the
    // gun takes raise_time to come back up before it can fire.
    let lowered = running && !lo.guns[active].def.fire_while_sprinting;
    lo.raise_t = if lowered { 0.0 } else { lo.raise_t + dt };
    let raised = lo.raise_t >= lo.guns[active].def.raise_time;
    let w = &mut lo.guns[active];

    // attack_button_presses_ads: the trigger aims too (Archer, Thunderbolt).
    let aim = input.pilot_ads || (w.def.attack_presses_ads && input.pilot_fire);
    let ads_target = if aim && !running && !w.reloading() && ready { 1.0 } else { 0.0 };
    let zoom = if ads_target >= w.ads { w.def.zoom_time } else { w.def.zoom_time_out };
    let rate = dt / zoom.max(0.05);
    w.ads = if ads_target > w.ads { (w.ads + rate).min(ads_target) } else { (w.ads - rate).max(ads_target) };
    let eye = Vec3::from(s.eye(&settings.0).to_array());

    w.cooldown = (w.cooldown - dt).max(-dt);
    w.since_fire += dt;
    w.spread_kick = w.def.spread.decayed(w.spread_kick, w.since_fire, dt);
    w.cur_spread = w.def.spread.base(crate::spread::pilot_stance(s), w.ads) + w.spread_kick;
    let gun_id = w.arsenal as u8;
    let cue_gun = move |part: GunPart| Cue::Gun(gun_id, part);
    if w.firing_audio && w.since_fire > 1.6 / w.def.fire_rate.max(0.1) {
        w.firing_audio = false;
        audio::cue(&mut commands, cue_gun(GunPart::Tail), None);
    }
    if w.reload_left > 0.0 {
        w.reload_left -= dt;
        w.reload_t += dt;
        if w.reload_left <= 0.0 {
            w.ammo = w.def.clip;
            w.reload_resume = None;
        }
    }
    // Stockpile-only weapons (the Charge Rifle's ammo_clip_size 0) never reload.
    let want_reload = !w.def.no_reload && (std::mem::take(&mut input.pilot_reload) && w.ammo < w.def.clip || (w.ammo == 0 && input.pilot_fire));
    if want_reload && !w.reloading() && ready {
        let empty = w.ammo == 0;
        let resume = w.reload_resume.filter(|&(e, _)| e == empty).map(|(_, s)| s).unwrap_or(0);
        let lates = if empty { w.def.reloadempty_late } else { w.def.reload_late };
        w.reload_left = match resume {
            0 => if empty { w.def.reload_empty_time } else { w.def.reload_time },
            s => lates[s as usize - 1],
        };
        w.reload_stage = resume;
        w.reload_empty = empty;
        w.reload_t = 0.0;
        log::debug!("{} reload ({}) from stage {resume}: {:.2}s", w.def.name, if empty { "empty" } else { "tactical" }, w.reload_left);
        w.burst_left = 0;
    }
    // Smart ammo: search for and paint locks (hip fire and/or ADS, as the script allows).
    if let Some(sd) = w.def.smart.clone() {
        let zoomed = w.ads >= 0.99;
        let searching = ready && !w.reloading() && !lowered && (w.ammo > 0 || !w.smart.queue.is_empty()) && if zoomed { sd.ads_lock } else { sd.hip_lock };
        crate::smartammo::search(&mut commands, &sd, &w.def, &mut w.smart, searching, eye, view_dir(input.yaw, input.pitch), dt, &world, &enemies);
    }
    // Charge weapons: holding the trigger charges (Charge Rifle 1.3 s, Cold War 0.5 s); a full
    // charge fires (OnWeaponPrimaryAttack_weapon_defender refuses anything less), and letting go
    // drains it over charge_cooldown_time. charge_require_input 0 keeps charging once started.
    let cdef = w.def.charge.clone();
    let charged = cdef.time > 0.0;
    if charged {
        let can = ready && !w.reloading() && w.ammo > 0 && !lowered && raised && w.cooldown <= 0.0 && w.burst_left == 0;
        let held = if cdef.by_ads { input.pilot_ads } else { input.pilot_fire };
        if held && can && !cdef.require_input {
            w.charge.latched = true;
        }
        if !can {
            w.charge.latched = false;
        }
        let charging = can && (held || w.charge.latched);
        if charging && !w.charge.charging {
            audio::cue(&mut commands, Cue::ChargeRifleTrigger, None);
            audio::cue_for(&mut commands, Cue::ChargeRifleWindUp, None, (1.0 - w.charge.frac) * cdef.time + 0.1);
        } else if !charging && w.charge.charging && w.charge.frac > 0.05 {
            audio::cue_for(&mut commands, Cue::ChargeRifleWindDown, None, w.charge.frac * cdef.cooldown.max(0.3) + 0.2);
        }
        w.charge.charging = charging;
        w.charge.update(&cdef, charging, dt);
    }
    // Trigger: automatic weapons fire while held; others once (or one burst) per pull.
    let pulled = input.pilot_fire && !w.trigger_held;
    w.trigger_held = input.pilot_fire;
    if !ready || w.reloading() || w.ammo == 0 || lowered || !raised {
        if pulled {
            log::debug!("{}: trigger refused (ready {ready}, reloading {}, ammo {}, lowered {lowered}, raised {raised})", w.def.name, w.reloading(), w.ammo);
        }
        if w.burst_left > 0 && !w.smart.queue.is_empty() {
            // A smart burst cut short still unlocks.
            w.smart.clear();
        }
        w.burst_left = 0;
        return;
    }
    // Charge level at the shot (ADS-charged weapons add damage per level, as the railgun does).
    let charge_level = 1 + w.charge.level(&cdef);
    let mut shot_def = w.def.clone();
    if charged && cdef.extra_per_level > 0.0 {
        shot_def.damage_near += cdef.extra_per_level * charge_level as f32;
        shot_def.damage_far += cdef.extra_per_level * charge_level as f32;
    }
    if w.burst_left == 0 {
        let start = if charged && !cdef.by_ads {
            w.charge.frac >= 1.0
        } else if w.def.automatic && w.def.burst_count <= 1 {
            input.pilot_fire
        } else {
            pulled
        };
        // attack_button_presses_ads weapons only fire fully zoomed.
        if !start || w.cooldown > 0.0 || (w.def.attack_presses_ads && w.ads < 0.999) {
            if pulled {
                log::debug!("{}: trigger refused (start {start}, cooldown {:.2}, ads {:.2})", w.def.name, w.cooldown, w.ads);
            }
            return;
        }
        // SmartAmmo_FireWeapon: one round per full lock, or the weapon's own burst unlocked.
        w.smart.queue = w.def.smart.as_ref().map(|sd| w.smart.burst(sd.max_targeted_burst)).unwrap_or_default();
        w.burst_size = if w.smart.queue.is_empty() { w.def.burst_count.max(1) } else { w.smart.queue.len() as u32 };
        w.burst_left = w.burst_size;
        if charged {
            log::info!("{} fires charged: {:.0} damage", w.def.name, shot_def.damage_near);
            w.charge = crate::weapons::Charge::default();
        }
        if !w.smart.queue.is_empty() {
            log::info!("{} fires at {} smart locks", w.def.name, w.burst_size);
        }
    }
    let burst = w.burst_size.max(1);
    // Within a burst shots come at the fire rate; bursts are separated by burst_fire_delay.
    w.rate_ramp = w.def.ramp_step(w.rate_ramp, w.trigger_held && w.ammo > 0 && !lowered, w.ads, dt);
    let interval = 1.0 / w.def.rate_at(w.rate_ramp).max(0.1);
    while w.cooldown <= 0.0 && w.ammo > 0 && w.burst_left > 0 {
        w.burst_left -= 1;
        w.cooldown += if w.burst_left == 0 && burst > 1 { interval + w.def.burst_delay } else { interval };
        // This round's smart target. A target that died since isn't shot at (the script returns
        // without firing), and the locks clear after the last round (SetUnlockAfterBurst).
        let smart_target = (!w.smart.queue.is_empty()).then(|| w.smart.queue.remove(0));
        if w.burst_left == 0 && smart_target.is_some() {
            w.smart.clear();
        }
        let target_at = match smart_target {
            Some(t) => match enemies.get(t) {
                Ok((_, e)) if e.alive() => Some(crate::hitbox::aim_point(e, hitboxes, &actors, &globals)),
                _ => continue,
            },
            None => None,
        };
        w.ammo -= 1;
        w.since_fire = 0.0;
        w.fire_t = 0.0;
        let first_of_burst = w.burst_left + 1 == burst;
        let part = if !w.firing_audio || (burst > 1 && first_of_burst) { GunPart::First } else { GunPart::Shot };
        if burst <= 1 || first_of_burst || smart_target.is_some() {
            audio::cue(&mut commands, cue_gun(part), None);
        }
        w.firing_audio = true;

        let ads = w.ads;
        let base = w.def.spread.base(crate::spread::pilot_stance(s), ads);
        let cone = (base + w.spread_kick).to_radians() * 0.5;
        let (r1, r2) = (w.rand() - 0.5, w.rand() - 0.5);
        let yaw = input.yaw + punch.yaw.to_radians();
        let pitch = input.pitch + punch.pitch.to_radians();
        let forward = view_dir(yaw, pitch);
        let right = Vec3::new(yaw.sin(), -yaw.cos(), 0.0);
        let up = right.cross(forward).normalize_or_zero();
        let smart = w.def.smart.clone();
        let missile = smart.as_ref().filter(|sd| sd.missile);
        // Pellet directions: a smart bullet straight at its target (FireWeaponBullet_Special
        // ignores spread), the script's bolt pattern, the engine's shotgun spread, or one shot.
        let mut dirs: Vec<Vec3> = Vec::new();
        if let (Some(at), None) = (target_at, missile) {
            dirs.push((at - eye).normalize_or(forward));
        } else if let Some(pattern) = bolt_pattern(&w.def.name) {
            let frac = w.def.bolt_spread.0 + (w.def.bolt_spread.1 - w.def.bolt_spread.0) * ads;
            for o in pattern {
                dirs.push((forward + up * o[0] * frac + right * o[1] * frac).normalize());
            }
        } else if w.def.shotgun {
            for _ in 0..8 {
                let (a, r) = (w.rand() * std::f32::consts::TAU, w.rand().sqrt() * base.to_radians() * 0.5);
                dirs.push((forward + right * a.cos() * r + up * a.sin() * r).normalize());
            }
        } else {
            dirs.push((forward + right * r1 * 2.0 * cone + up * r2 * 2.0 * cone).normalize());
        }
        let muzzle = w
            .viewmodel
            .and_then(|(_, vm)| actors.get(vm).ok())
            .and_then(|a| a.joint("muzzle_flash"))
            .and_then(|j| globals.get(j).ok())
            .map(|g| g.translation())
            .unwrap_or(to_bevy(eye));
        for dir in dirs {
            if let Some(sd) = missile {
                spawn_smart_missile(&mut commands, eye + dir * 40.0, dir * sd.missile_speed, &shot_def, smart_target, sd.homing_speed, &w.projectile);
                continue;
            }
            if let Some(speed) = w.def.projectile_speed {
                spawn_bolt(&mut commands, &fx, eye + dir * 40.0, dir * speed, w.def.gravity, &shot_def, false);
                continue;
            }
            let max = 15000.0;
            let mut hit_t = world.0.raycast(SVec3::from(eye.to_array()), SVec3::from(dir.to_array()), max).map(|h| h.t).unwrap_or(max);
            // An enemy Tone's Particle Wall stops the shot (and takes its damage).
            let mut wall = None;
            if let Some((we, t)) = enemy_walls.hit(eye, dir, hit_t) {
                hit_t = t;
                wall = Some(we);
            }
            let mut victim = None;
            for (id, e) in enemies.iter() {
                if e.alive() {
                    if let Some(h) = crate::hitbox::trace(eye, dir, hit_t, e, hitboxes, &actors, &globals) {
                        hit_t = h.t;
                        victim = Some((id, h));
                    }
                }
            }
            if let (Some(we), None) = (wall, victim.as_ref()) {
                enemy_walls.absorbed.push((we, shot_def.damage_near));
                crate::particles::emit(&mut commands, crate::particles::Effect::Hit { at: to_bevy(eye + dir * hit_t), normal: -to_bevy(dir).normalize_or(Vec3::Y), table: shot_def.impact_table, surface: crate::particles::Surface::Shield, victim: false, scale: 1.0 });
            }
            // Out through your A-Wall: the round is amped (the weapon's burn mod: its damage
            // and tracer; CodeCallback_CheckPassThroughAddsMods).
            let amped = shields.passes_out(eye, dir, hit_t).then_some(w.def.burn.as_deref()).flatten();
            let shot = amped.unwrap_or(&shot_def);
            if amped.is_some() {
                log::debug!("{} round amped through an a-wall", w.def.name);
            }
            let mut surface = crate::particles::Surface::World;
            if let Some((id, h)) = victim {
                if let Ok((_, mut e)) = enemies.get_mut(id) {
                    surface = if e.infantry { crate::particles::Surface::Flesh } else { crate::particles::Surface::Titan };
                    let dmg = crate::hitbox::damage(shot, &h, hit_t, e.infantry);
                    log::debug!("{} hit {} group {} crit {} for {dmg:.0}", w.def.name, if e.infantry { "grunt" } else { "titan" }, h.group, h.crit);
                    e.damage(dmg, w.def.stops_regen);
                }
            }
            let stop = to_bevy(eye + dir * hit_t);
            let dir_b = to_bevy(dir).normalize_or(Vec3::NEG_Z);
            if (stop - muzzle).length() > 0.3 {
                // Charged shots are beams (P_wpn_defender_beam): thicker and hotter.
                // The script's first-person tracer when the game's systems are loaded.
                let tdef = amped.unwrap_or(&w.def);
                let tracer = if tdef.fx.tracer_view.is_empty() { &tdef.fx.tracer_world } else { &tdef.fx.tracer_view };
                if pfx_ready && !charged && !tracer.is_empty() && std::env::var_os("TF_OLD_FX").is_none() {
                    // The tracer starts where the gun's muzzle is seen (FormatViewModelAttachment).
                    let fov = |vm: bool| lenses.iter().find(|l| l.2 == vm).and_then(|(g, p, _)| if let Projection::Perspective(pp) = p { Some((*g, pp.fov)) } else { None });
                    let start = match (fov(false), fov(true)) {
                        (Some((cam, w)), Some((_, v))) => crate::vmcam::to_world(muzzle, &cam, w, v),
                        _ => muzzle,
                    };
                    crate::pfx::emit_beam(&mut commands, tracer, start, stop);
                } else {
                    let (width, color) = if charged { (0.12, Vec3::new(2.5, 3.5, 6.0)) } else { (0.035, Vec3::new(4.0, 2.4, 1.0)) };
                    crate::particles::emit(&mut commands, crate::particles::Effect::Tracer { from: muzzle, to: stop, width, color });
                }
            }
            crate::particles::emit(&mut commands, crate::particles::Effect::Hit { at: stop, normal: -dir_b, table: shot_def.impact_table, surface, victim: false, scale: if victim.is_some() { 1.4 } else { 1.0 } });
            if let Some((_, h)) = victim {
                hit_feedback.push(h);
            }
        }
        w.spread_kick = w.def.spread.kicked(w.spread_kick, crate::spread::pilot_stance(&pilot.state), ads);
        let r = [w.rand(), w.rand(), w.rand(), w.rand()];
        punch.kick(&w.def.kick, &springs, ads, pilot.state.crouched, 1.0, r);
        // viewkick_perm_*: the part of the kick that stays in the aim.
        input.pitch += punch.perm.x.to_radians();
        input.yaw += punch.perm.y.to_radians();
        punch.perm = Vec2::ZERO;
        // The script's first-person muzzle flash and shell eject at their attachments, drawn
        // with the viewmodel; the hand-built flash when the game's systems aren't available.
        let vm = w.viewmodel.map(|v| v.1);
        let at = |i: usize| vm.zip(w.fx_atts[i].as_ref()).and_then(|(vm, a)| vm_attachment(&actors, &globals, vm, a));
        let game_fx = pfx_ready && std::env::var_os("TF_OLD_FX").is_none();
        match (at(0), game_fx && !w.def.fx.muzzle_view.is_empty()) {
            (Some((p, r)), true) => crate::pfx::emit_named_vm_frame(&mut commands, &w.def.fx.muzzle_view, p, r),
            _ => {
                let energy = ["arc", "lstar", "plasma"].iter().any(|k| w.def.name.contains(k));
                crate::particles::emit(&mut commands, crate::particles::Effect::Muzzle { at: muzzle, dir: to_bevy(forward).normalize_or(Vec3::NEG_Z), scale: 0.3, energy });
            }
        }
        if let (Some((p, r)), true) = (at(1), game_fx && !w.def.fx.shell_view.is_empty()) {
            crate::pfx::emit_named_vm_frame(&mut commands, &w.def.fx.shell_view, p, r);
        }
        commands.spawn((
            PointLight { color: Color::srgb(1.0, 0.75, 0.45), intensity: 60_000.0, range: 6.0, ..default() },
            Transform::from_translation(muzzle),
            Fx { life: 0.04, max: 0.04, base: Vec3::splat(0.15), keep_z: false },
        ));
    }
}

/// Fly pilot projectiles (smart missiles turning toward their target); impact and explosion
/// damage on enemies.
#[allow(clippy::too_many_arguments)]
pub fn update_pilot_bolts(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<Collision>,
    _fx: Res<FxAssets>,
    mut bolts: Query<(Entity, &mut PilotBolt, &mut Transform)>,
    mut enemies: Query<(Entity, &mut Enemy)>,
    mut cores: Query<&mut crate::abilities::TitanCore>,
    mut titan_hits: ResMut<crate::titankit::TitanBoltHits>,
    actors: Query<&Actor>,
    globals: Query<&GlobalTransform>,
    hitboxes: Option<Res<crate::hitbox::EnemyHitboxes>>,
    mut enemy_walls: ResMut<crate::titankit::EnemyWalls>,
    shields: Res<crate::pilotability::Shields>,
) {
    let dt = time.delta_secs().min(0.1);
    let no_boxes = crate::hitbox::EnemyHitboxes::default();
    let hitboxes = hitboxes.as_deref().unwrap_or(&no_boxes);
    for (id, mut b, mut tf) in &mut bolts {
        b.life -= dt;
        b.vel.z -= 750.0 * b.gravity * dt;
        // Smart missiles turn toward their target's chest while it lives.
        let homing = b.homing.and_then(|(t, turn)| enemies.get(t).ok().filter(|(_, e)| e.alive()).map(|(_, e)| (crate::smartammo::chest(e), turn)));
        if let Some((at, turn)) = homing {
            let speed = b.vel.length();
            let (cur, want) = (b.vel / speed.max(1e-3), (at - b.pos).normalize_or_zero());
            let angle = cur.angle_between(want);
            let axis = cur.cross(want).normalize_or_zero();
            if angle > 1e-4 && axis != Vec3::ZERO {
                b.vel = Quat::from_axis_angle(axis, angle.min(turn.to_radians() * dt)) * cur * speed;
            }
        }
        let step = b.vel * dt;
        let len = step.length().max(1e-4);
        let dir = step / len;
        let mut t_end = len;
        let mut hit_world = false;
        if let Some(h) = world.0.raycast(SVec3::from(b.pos.to_array()), SVec3::from(dir.to_array()), len) {
            t_end = h.t;
            hit_world = true;
        }
        // An enemy Tone's Particle Wall: the round (or missile) ends on it and it takes the hit.
        let mut wall = None;
        if let Some((we, t)) = enemy_walls.hit(b.pos, dir, t_end) {
            t_end = t;
            hit_world = true;
            wall = Some(we);
        }
        let mut victim = None;
        for (eid, e) in enemies.iter() {
            if e.alive() {
                if let Some(h) = crate::hitbox::trace(b.pos, dir, t_end, e, hitboxes, &actors, &globals) {
                    t_end = h.t;
                    victim = Some((eid, h));
                }
            }
        }
        if b.burn.is_some() && shields.passes_out(b.pos, dir, t_end) {
            let ((near, far), explosion) = b.burn.take().unwrap();
            b.damage.0 = near;
            b.damage.1 = far;
            b.explosion = explosion;
        }
        b.pos += dir * t_end;
        b.travelled += t_end;
        if b.from_titan && (victim.is_some() || hit_world) {
            let nearest = enemies.iter().filter(|(_, e)| e.alive()).map(|(_, e)| (e.pos + Vec3::Z * e.height * 0.5).distance(b.pos) - e.radius).fold(f32::MAX, f32::min);
            log::debug!("titan bolt ends at {:.0} after {:.0}: victim {} world {} nearest enemy {:.0}", b.pos, b.travelled, victim.is_some(), hit_world, nearest);
        }
        tf.translation = to_bevy(b.pos);
        tf.rotation = Quat::from_rotation_arc(Vec3::Y, to_bevy(dir).normalize_or(Vec3::Y));
        if victim.is_none() && !hit_world {
            if b.life <= 0.0 {
                commands.entity(id).despawn();
            }
            continue;
        }
        commands.entity(id).despawn();
        let (near, far, nd, fd) = b.damage;
        let f = ((b.travelled - nd) / (fd - nd).max(1.0)).clamp(0.0, 1.0);
        if let (Some(we), None) = (wall, victim.as_ref()) {
            enemy_walls.absorbed.push((we, near + (far - near) * f + b.explosion.0));
        }
        let mut hits = Vec::new();
        let mut surface = if wall.is_some() { crate::particles::Surface::Shield } else { crate::particles::Surface::World };
        if let Some((eid, h)) = victim {
            if let Ok((_, mut e)) = enemies.get_mut(eid) {
                surface = if e.infantry { crate::particles::Surface::Flesh } else { crate::particles::Surface::Titan };
                let base = near + (far - near) * f;
                // Projectiles carry Titan-armour damage; headshots and crits multiply it.
                let k = if e.infantry && h.group == crate::hitbox::HITGROUP_HEAD { b.headshot } else if !e.infantry && h.crit { b.crit } else { 1.0 };
                hits.push(e.damage(base * k, b.stops_regen));
            }
        }
        let (exp, inner, radius) = b.explosion;
        if exp > 0.0 && radius > 0.0 {
            for (_, mut e) in &mut enemies {
                if !e.alive() {
                    continue;
                }
                let d = ((e.pos + Vec3::Z * e.height * 0.5) - b.pos).length() - e.radius;
                if d < radius {
                    let k = 1.0 - ((d - inner) / (radius - inner).max(1.0)).clamp(0.0, 1.0);
                    hits.push(e.damage(exp * k, b.stops_regen));
                }
            }
            audio::cue(&mut commands, Cue::MissileExplode, Some(b.pos));
        }
        if b.from_titan {
            if victim.is_some() {
                log::debug!("titan bolt hit at {:.0}", b.pos);
                titan_hits.0.push(b.pos);
            }
            for mut c in &mut cores {
                for h in &hits {
                    c.credit_inflicted(*h);
                }
            }
        }
        let at = to_bevy(b.pos);
        let effect = if exp > 0.0 && radius > 0.0 {
            crate::particles::Effect::Explosion { at, scale: (radius * crate::player::UNIT / 2.5).clamp(0.4, 2.0) }
        } else {
            let back = -to_bevy(b.vel).normalize_or(Vec3::NEG_Z);
            crate::particles::Effect::Hit { at, normal: back, table: b.impact_table, surface, victim: false, scale: if b.from_titan { 1.8 } else { 0.7 } }
        };
        crate::particles::emit(&mut commands, effect);
    }
}

/// How far ahead of the eye an ADS scope's bone sits: the length of its tube.
const SCOPE_TUBE_DEPTH: f32 = 3.0;

/// `TF_VM_NO=walk,idle,jump,land,raise` turns viewmodel layers off (for comparisons).
fn vm_off(layer: &str) -> bool {
    static OFF: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    OFF.get_or_init(|| std::env::var("TF_VM_NO").map(|v| v.split(',').map(|s| s.trim().to_string()).collect()).unwrap_or_default()).iter().any(|s| s == layer)
}

/// The active gun's viewmodel: rest pose (draw / ADS end), sprint, reload, draw and holster,
/// and the firing kick; the others are hidden.
pub fn pilot_viewmodel(
    time: Res<Time>,
    rodeo: Res<crate::rodeo::Rodeo>,
    fpseq: Res<crate::fparms::FpSeq>,
    control: Res<Control>,
    mode: Res<CameraMode>,
    punch: Res<ViewPunch>,
    melee: Res<crate::executions::PilotMelee>,
    throw: Res<crate::pilotability::OrdnanceThrow>,
    defs: Option<Res<crate::pilotability::PilotAbilityDefs>>,
    mut pilots: Query<(&PlayerPilot, &mut PilotLoadout)>,
    mut actors: Query<&mut Actor>,
    mut tfs: Query<(&mut Transform, &mut Visibility), (Without<MainCamera>, Without<crate::actor::BodyPartMesh>)>,
    mut part_meshes: Query<(&ChildOf, &crate::actor::BodyPartMesh, &mut Visibility)>,
    mut place: ResMut<VmPlacements>,
) {
    place.0.clear();
    let Ok((pilot, mut lo)) = pilots.single_mut() else { return };
    let show = *control == Control::Pilot && *mode != CameraMode::Free && !rodeo.riding() && !fpseq.playing();
    let active = lo.active;
    // The melee swing replaces the gun with the empty hands; the gun then raises from melee.
    let swinging = melee.t.filter(|&t| t < crate::executions::MELEE_ANIM_SECS);
    if let Some((anchor, vm)) = lo.melee_vm {
        if let Ok((_, mut v)) = tfs.get_mut(anchor) {
            *v = if show && swinging.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        }
        if let Ok(mut a) = actors.get_mut(vm) {
            a.event_sounds = show && swinging.is_some();
            if let Some(t) = swinging.filter(|_| show) {
                a.autoplay = false;
                let n = MELEE_SEQS.len();
                if let Some(c) = (0..n).map(|k| MELEE_SEQS[(melee.seq + k) % n]).find_map(|name| a.clip(name)) {
                    let d = a.clips[c].duration.max(1e-3);
                    a.layers = vec![Layer { clip: c, cycle: (t / d).min(0.999), weight: 1.0 }];
                    a.additive.clear();
                    a.grid_layers.clear();
                }
                let offset = a.bone_model_transform("jx_c_camera").map(|t| t.translation).unwrap_or(Vec3::ZERO);
                place.0.push((anchor, Transform::from_translation(-offset)));
            }
        }
    }
    // A throw replaces the gun with the grenade in hand: pull out, hold, toss.
    let throwing = throw.phase.filter(|(p, _)| *p != crate::pilotability::ThrowPhase::Raise);
    for (which, slot) in [(false, lo.ord_vm), (true, lo.tac_vm)] {
        let Some((idx, anchor, vm)) = slot else { continue };
        let mine = throwing.is_some() && throw.tactical == which;
        if let Ok((_, mut v)) = tfs.get_mut(anchor) {
            *v = if show && mine { Visibility::Inherited } else { Visibility::Hidden };
        }
        if !mine {
            continue;
        }
        if let (Ok(mut a), Some((phase, t))) = (actors.get_mut(vm), throwing.filter(|_| show)) {
            use crate::pilotability::ThrowPhase as T;
            a.autoplay = false;
            a.event_sounds = true;
            let times = defs.as_deref().and_then(|d| if which { d.tactical.get(idx).map(|t| (t.pullout, t.toss)) } else { d.ordnance.get(idx).map(|o| (o.pullout, o.toss)) });
            let (pullout, toss) = times.unwrap_or((0.35, 0.33));
            let (name, cycle) = match phase {
                T::Pullout => ("toss_prep_pullout_seq", t / pullout),
                T::Hold => ("toss_hold_seq", 0.0),
                _ => ("toss_seq", t / toss),
            };
            if let Some(c) = a.clip(name).or_else(|| a.clip("idle_seq")) {
                let cycle = if phase == T::Hold { (t / a.clips[c].duration.max(0.1)).fract() } else { cycle.min(0.999) };
                a.layers = vec![Layer { clip: c, cycle, weight: 1.0 }];
                a.additive.clear();
                a.grid_layers.clear();
            }
            let offset = a.bone_model_transform("jx_c_camera").map(|t| t.translation).unwrap_or(Vec3::ZERO);
            place.0.push((anchor, Transform::from_translation(-offset)));
        }
    }
    for (i, g) in lo.guns.iter().enumerate() {
        if let Some((anchor, vm)) = g.viewmodel {
            if let Ok((_, mut v)) = tfs.get_mut(anchor) {
                *v = if show && i == active && swinging.is_none() && throwing.is_none() { Visibility::Inherited } else { Visibility::Hidden };
            }
            // Only the gun in your hands plays its animation sounds.
            if let Ok(mut a) = actors.get_mut(vm) {
                a.event_sounds = show && i == active;
            }
        }
    }
    if !show {
        return;
    }
    let dt = time.delta_secs();
    let s = pilot.state.clone();
    let sprint = s.sprinting && s.mode == PilotMove::Ground;
    let on_ground = s.on_ground();
    // Layer clocks: leaving the ground (or a double jump) starts the jump transition, touching
    // down the land one, and the end of a sprint the sprint-raise.
    let mut vl = lo.vm_layers;
    vl.clock += dt;
    vl.jump_t += dt;
    vl.land_t += dt;
    vl.sprint_end_t += dt;
    if !on_ground && (vl.was_ground || s.jumps > vl.jumps) {
        vl.jump_t = 0.0;
    }
    if on_ground && !vl.was_ground {
        vl.land_t = 0.0;
    }
    if vl.was_sprint && !sprint {
        vl.sprint_end_t = 0.0;
    }
    vl.jumps = s.jumps;
    vl.was_ground = on_ground;
    vl.was_sprint = sprint;
    vl.ground_blend += (if on_ground { 1.0 } else { 0.0 } - vl.ground_blend) * (1.0 - (-dt * 10.0).exp());
    lo.vm_layers = vl;
    let target = if sprint { 1.0 } else { 0.0 };
    lo.sprint_blend += (target - lo.sprint_blend) * (1.0 - (-dt * 8.0).exp());
    let (switching, deploy_t) = (lo.switching, lo.deploy_t);
    let Some(g) = lo.guns.get(active) else { return };
    let Some((anchor, vm)) = g.viewmodel else { return };
    let (ads, reloading, fire_t, empty_reload, reload_stage) = (g.ads, g.reloading(), g.fire_t, g.reload_empty, g.reload_stage);
    // Reload, draw and holster sequences play scaled to the script's times (the R-201's 2 s
    // reload_seq over reload_time 2.2; the Wingman's 2.9 s one over 2.1), as the engine does.
    let reload_frac = (g.reload_t / (g.reload_t + g.reload_left).max(1e-3)).min(0.999);
    let (deploy_time, holster_time) = (g.def.deploy_time.max(0.05), g.def.holster_time.min(0.6).max(0.05));
    let (pivots, motion_def, ads_scope, scope_frac, vm_offset) = (g.pivots.clone(), g.def.motion.clone(), g.ads_scope, g.def.scope_frac, g.def.vm_offset);
    let Ok(mut actor) = actors.get_mut(vm) else { return };
    actor.autoplay = false;
    let clip = |a: &Actor, n: &str| a.clip(n);
    let sprint_clip = clip(&actor, "sprint_seq");
    // Leaving a sprint plays `sprintraise_seq` (ACT_VM_RAISE_FROM_SPRINT) instead of fading.
    let raise_clip = clip(&actor, "sprintraise_seq").filter(|_| !vm_off("raise"));
    let raising = raise_clip.filter(|&c| !sprint && vl.sprint_end_t < actor.clips[c].duration);
    if raising.is_some() {
        lo.sprint_blend = 0.0;
    }
    let sprint_blend = lo.sprint_blend;
    let dur = sprint_clip.map(|c| actor.clips[c].duration).unwrap_or(1.0);
    lo.vm_cycle = (lo.vm_cycle + dt / dur.max(0.1)).rem_euclid(1.0);
    let once = |a: &Actor, c: usize, t: f32| (t / a.clips[c].duration.max(1e-3)).min(0.999);
    let mut layers = Vec::new();
    let mut additive = Vec::new();
    let mut grid_layers = Vec::new();
    let grid_dur = |a: &Actor, n: &str| a.grids.get(n).and_then(|g| g.clips.first()).map(|&c| a.clips[c].duration.max(1e-3));
    let speed = Vec2::new(s.vel.x, s.vel.y).length();
    let draw = clip(&actor, "draw_seq");
    let mut steady = false;
    if let Some((_, t)) = switching {
        if let Some(c) = clip(&actor, "holster_seq") {
            layers.push(Layer { clip: c, cycle: (t / holster_time).min(0.999), weight: 1.0 });
        }
    } else if draw.is_some() && deploy_t < deploy_time {
        let c = draw.unwrap();
        layers.push(Layer { clip: c, cycle: (deploy_t / deploy_time).min(0.999), weight: 1.0 });
    } else if let (true, Some(c)) = (reloading, {
        // A resumed reload plays its stage's `_late` sequence.
        let late = (reload_stage > 0).then(|| if empty_reload { format!("reload_empty_late{reload_stage}_seq") } else { format!("reload_late{reload_stage}_seq") }).and_then(|n| clip(&actor, &n));
        late.or(if empty_reload { clip(&actor, "reload_empty_seq").or(clip(&actor, "reload_seq")) } else { clip(&actor, "reload_seq") })
    }) {
        layers.push(Layer { clip: c, cycle: reload_frac, weight: 1.0 });
        // Walking while reloading sways the gun with the reload's own run layer.
        if let Some(d) = grid_dur(&actor, "run_layer_reload").filter(|_| !vm_off("walk")) {
            grid_layers.push(GridLayer { grid: "run_layer_reload", x: 0.0, y: 0.0, cycle: (vl.clock / d).fract(), weight: (speed / 173.0).min(1.0) * vl.ground_blend, relative: false });
        }
    } else if let (Some((crate::pilotability::ThrowPhase::Raise, t)), Some(c)) = (throw.phase, clip(&actor, "raise_seq")) {
        // Back up after a throw.
        layers.push(Layer { clip: c, cycle: (t / crate::pilotability::THROW_RAISE_SECS).min(0.999), weight: 1.0 });
    } else if let (Some(t), Some(c)) = (melee.t.filter(|&t| t >= crate::executions::MELEE_ANIM_SECS), clip(&actor, "raise_frommelee_seq")) {
        // Back up after a melee swing, over melee_raise_recovery_animtime.
        let k = ((t - crate::executions::MELEE_ANIM_SECS) / melee.raise.max(0.05)).min(0.999);
        layers.push(Layer { clip: c, cycle: k, weight: 1.0 });
    } else if let Some((c, t)) = s.zipline.and_then(|r| Some((clip(&actor, "ptpov_zipline_start")?, r.mount))).filter(|&(c, t)| t < actor.clips[c].duration) {
        // Grabbing a zipline: `ptpov_zipline_start` (ACT_VM_ZIPLINE_MOUNT).
        layers.push(Layer { clip: c, cycle: once(&actor, c, t), weight: 1.0 });
    } else if let Some(c) = raising {
        layers.push(Layer { clip: c, cycle: once(&actor, c, vl.sprint_end_t), weight: 1.0 });
    } else {
        steady = true;
        let still = 1.0 - sprint_blend;
        if let Some(c) = draw {
            layers.push(Layer { clip: c, cycle: 0.999, weight: (still * (1.0 - ads)).max(1e-3) });
        }
        if let Some(c) = clip(&actor, "ads_in_seq") {
            layers.push(Layer { clip: c, cycle: 0.999, weight: still * ads });
        }
        if let Some(c) = sprint_clip {
            layers.push(Layer { clip: c, cycle: lo.vm_cycle, weight: sprint_blend });
        }
        if let Some(g) = actor.grids.get("attack_seq") {
            for (i, wgt) in [(0usize, 1.0 - ads), (1, ads)] {
                if let Some(&c) = g.clips.get(i) {
                    let d = actor.clips[c].duration.max(1e-3);
                    if fire_t < d {
                        actor.push_clip(&mut layers, &mut additive, c, fire_t / d, wgt * still);
                    }
                }
            }
        }
    }
    if steady {
        let still = 1.0 - sprint_blend;
        // Breathing: `idle_seq_autoplay` (STUDIO_AUTOPLAY) loops in real time, hip or ADS.
        if let Some(d) = grid_dur(&actor, "idle_seq_autoplay").filter(|_| !vm_off("idle")) {
            grid_layers.push(GridLayer { grid: "idle_seq_autoplay", x: ads, y: 0.0, cycle: (vl.clock / d).fract(), weight: still, relative: false });
        }
        // Walking: `walk_seq` (STUDIO_REALTIME) over the velocity and ads_blend parameters. Its
        // clips carry constant offsets from the rest pose (the gun 0.8 units off and the
        // clavicle turned 30 degrees even standing still; the ADS walk another 2.6 units),
        // which pulled the ADS sight off centre, so only each clip's motion is applied.
        if let Some(d) = grid_dur(&actor, "walk_seq").filter(|_| !vm_off("walk")) {
            grid_layers.push(GridLayer { grid: "walk_seq", x: speed.min(173.0), y: ads, cycle: (vl.clock / d).fract(), weight: still * vl.ground_blend, relative: true });
        }
    }
    if !reloading && switching.is_none() {
        for (names, t, off) in [(["switch_to_jump_iron", "switch_to_jump"], vl.jump_t, "jump"), (["switch_to_land_iron", "switch_to_land"], vl.land_t, "land")] {
            if vm_off(off) {
                continue;
            }
            let Some(name) = names.into_iter().find(|n| actor.grids.get(*n).is_some_and(|g| g.clips.first().is_some_and(|&c| actor.clips[c].delta))) else { continue };
            let d = grid_dur(&actor, name).unwrap_or(1.0);
            if t < d {
                grid_layers.push(GridLayer { grid: name, x: ads, y: 0.0, cycle: t / d, weight: 1.0 - sprint_blend, relative: false });
            }
        }
    }
    actor.grid_layers = grid_layers;
    actor.layers = layers;
    actor.additive = additive;
    actor.root_offsets.clear();
    let offset = actor.bone_model_transform("jx_c_camera").map(|t| t.translation).unwrap_or(Vec3::ZERO);
    // Scopes: the engine puts the ADS scope (a dark tube with the reticle and a glowing
    // ring, on a root bone animated relative to the model origin) in front of the eye; it
    // shows from zoom_scope_frac_start, and from zoom_scope_frac_end it is all that is drawn
    // (the scope model would block the view). The tube ends at its bone (z -3..0) and the eye
    // goes at its rear opening. Guesses at the engine's rules, from the keys' names and the
    // tube's shape.
    if let Some((part, bone)) = ads_scope {
        let depth = std::env::var("TF_SCOPE_DEPTH").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(SCOPE_TUBE_DEPTH);
        if let Some(t) = actor.bone_model_transform("def_c_scope_ads") {
            actor.root_offsets.push((bone, offset + Vec3::X * depth - t.translation));
        }
        let (show_scope, only_scope) = (ads >= scope_frac.0, ads >= scope_frac.1);
        for (parent, p, mut v) in &mut part_meshes {
            if parent.parent() != vm {
                continue;
            }
            let on = if p.0 == part { show_scope } else { !only_scope };
            *v = if on { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
    // Sway, bob and the gun's share of the recoil turn the viewmodel about its sway pivot
    // (SWAY_ROTATE on the weapon bone; SWAY_ROTATE_ZOOMED far out along the barrel in ADS, so
    // the sight stays on the aim point).
    let pivot_of = |a: &Actor, (bone, off): &(String, Vec3)| a.bone_model_transform(bone).map(|t| t.transform_point(*off));
    let hp = pivot_of(&actor, &pivots[0]).unwrap_or(offset);
    let zp = pivot_of(&actor, &pivots[1]).unwrap_or(hp);
    let pivot = hp.lerp(zp, ads);
    let def = motion_def;
    let (cy, sy) = (s.yaw.cos(), s.yaw.sin());
    let vel = Vec3::new(s.vel.x * cy + s.vel.y * sy, -s.vel.x * sy + s.vel.y * cy, s.vel.z);
    let inp = crate::vmmotion::MotionInput { yaw: s.yaw, pitch: s.pitch, vel, on_ground, bob: sprint_blend < 0.5, ads };
    let off = if vm_off("sway") { crate::vmmotion::VmOffset { rot: Vec3::ZERO, trans: Vec3::ZERO } } else { lo.vm_motion.step(&def, &inp, dt) };
    let rs = crate::vmmotion::angles_quat(off.rot + punch.vm);
    // viewmodel_offset_hip/_ads, (right, forward, up) in the view.
    let (oh, oa) = vm_offset;
    let o = oh.lerp(oa, ads);
    let script_off = Vec3::new(o.y, -o.x, o.z);
    place.0.push((anchor, Transform::from_translation(-offset + script_off + pivot + off.trans + punch.vm_shake - rs * pivot).with_rotation(rs)));
}

/// Where each shown first-person model sits in the view frame (camera bone at the eye),
/// placed after the camera has moved this frame.
#[derive(Resource, Default)]
pub struct VmPlacements(pub Vec<(Entity, Transform)>);

/// Put the Pilot's viewmodels at this frame's view origin (`VmEye`, set by `pilot_camera`).
pub fn place_viewmodels(place: Res<VmPlacements>, vm_eye: Res<crate::pilotctl::VmEye>, settings: Res<PilotSettings>, pilots: Query<&PlayerPilot>, mut tfs: Query<&mut Transform>) {
    let Ok(pilot) = pilots.single() else { return };
    let (eye, rot) = vm_eye.frame(&pilot.state, &settings.0);
    for &(e, local) in &place.0 {
        if let Ok(mut tf) = tfs.get_mut(e) {
            tf.rotation = rot * local.rotation;
            tf.translation = eye + rot * local.translation;
        }
    }
}
