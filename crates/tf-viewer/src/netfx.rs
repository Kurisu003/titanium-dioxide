//! The other players' shots and ability objects, drawn here as harmless copies.
//!
//! What the local Titan fires and deploys (its rounds and tracers, Slow Trap canisters,
//! tethers, tripwire pylons, cluster rockets, missiles, smoke, sonar echoes, the Laser Shot
//! and Power Shot) goes out as a `Msg::Event` with where it starts and how fast. Everyone
//! else spawns the same object, flagged a ghost, and the same systems fly it, so it lands where
//! the owner's did and looks the same. A ghost deals no damage: the owner's hits do, as for
//! everything else in a session. What later happens to it (a trap going up in flames, a
//! tripwire group blowing, a tether catching a Titan) comes from the owner as its own event.
//! Shields ride on the snapshot flags.

use crate::net::{Net, POOL_TARGET};
use crate::player::to_bevy;
use crate::targets::{Enemy, EnemyKits};
use bevy::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The first event kind handled here (net.rs routes these to `Net::fx_in`).
pub const FIRST: u8 = 16;
/// A Titan round: position, velocity; arg the weapons::TITAN_ARSENAL index | BOLT_WILDFIRE.
pub const EV_BOLT: u8 = 16;
/// A hitscan Titan round: from the gun, to where it struck; arg the TITAN_ARSENAL index.
pub const EV_TRACER: u8 = 17;
/// Ion's Laser Shot: from the pod to where it struck; arg 1 if it struck a body.
pub const EV_LASER_SHOT: u8 = 18;
/// Legion's Power Shot: from the gun to where it struck; args (long range, struck: 0 the
/// world, 1 a Titan, 2 a Pilot).
pub const EV_POWER_SHOT: u8 = 19;
/// Northstar's Cluster Missile: position, velocity.
pub const EV_CLUSTER: u8 = 20;
/// A missile (Multi-Target Missiles, Tracker Rockets, Salvo and Flight Core): position,
/// velocity; args (target code or u32::MAX, homing | out_time ms << 16).
pub const EV_MISSILE: u8 = 21;
/// A Tether Trap tether: position, velocity.
pub const EV_TETHER: u8 = 22;
/// A tether catching a Titan: the trap, its surface normal; args (target code, catch_arg).
pub const EV_TETHER_CATCH: u8 = 23;
/// Scorch's Slow Trap canister: position, velocity.
pub const EV_SLOW_TRAP: u8 = 24;
/// A Slow Trap going up in flames, at the trap.
pub const EV_TRAP_IGNITE: u8 = 25;
/// A Laser Tripwire pylon: position, velocity; args (group, projectileID).
pub const EV_PYLON: u8 = 26;
/// A Laser Tripwire group blowing; arg the group.
pub const EV_PYLONS_BLOW: u8 = 27;
/// Electric Smoke: where, facing.
pub const EV_SMOKE: u8 = 28;
/// A Sonar Pulse echo: position, velocity.
pub const EV_SONAR: u8 = 29;
/// The Laser Core's beam, each damage tick while it fires: from its muzzle to where it
/// strikes.
pub const EV_LASER_CORE: u8 = 30;
/// A Pilot's round: position, velocity; args (pilotweapon::ARSENAL index, its gravity's
/// bits).
pub const EV_PILOT_BOLT: u8 = 31;
/// A Pilot's hitscan round: from the eye to where it struck; arg the ARSENAL index |
/// BOLT_AMPED.
pub const EV_PILOT_TRACER: u8 = 32;
/// A Pilot's thrown ordnance, Pulse Blade or A-Wall: position, velocity; args (an
/// pilotability::ORDNANCE index or GHOST_* code, fuse ms).
pub const EV_GRENADE: u8 = 33;
/// A Pilot setting off their satchels.
pub const EV_SATCHELS_BOOM: u8 = 34;
/// Someone shot down a player's Holo Pilot decoy (its 50 health): at the decoy; arg the
/// owner's id, whose decoy then dissolves.
pub const EV_DECOY_DOWN: u8 = 35;

/// `EV_BOLT` arg bit: the owner's Wildfire Launcher (their thermite's eight trails).
const BOLT_WILDFIRE: u32 = 1 << 8;
/// `EV_PILOT_*` arg bit: the round went out through the owner's A-Wall (its burn mod).
const BOLT_AMPED: u32 = 1 << 9;
/// `EV_TETHER_CATCH` catch_arg bit: the tether struck the Titan in flight.
const CATCH_DIRECT: u32 = 1 << 31;

/// Queue a local object or happening for the others.
pub fn send(kind: u8, pos: Vec3, dir: Vec3, args: [u32; 2]) {
    crate::net::event_args(kind, pos, dir, args);
}

/// A round the local Titan or Pilot fired (pilotweapon::spawn_bolt_from).
pub fn send_bolt(
    pos: Vec3,
    vel: Vec3,
    gravity: f32,
    def: &crate::weapons::WeaponDef,
    from_titan: bool,
) {
    if !from_titan {
        if let Some(i) = crate::pilotweapon::ARSENAL
            .iter()
            .position(|g| g.id == def.name)
        {
            send(EV_PILOT_BOLT, pos, vel, [i as u32, gravity.to_bits()]);
        }
        return;
    }
    let Some(i) = crate::weapons::TITAN_ARSENAL
        .iter()
        .position(|g| g.id == def.name)
    else {
        return;
    };
    let wildfire = if def.name == "mp_titanweapon_meteor" && crate::kits::has("pas_scorch_weapon") {
        BOLT_WILDFIRE
    } else {
        0
    };
    send(EV_BOLT, pos, vel, [i as u32 | wildfire, 0]);
}

/// A hitscan round the local Pilot fired (pilotweapon.rs): from the eye to where it struck,
/// `amped` out through an A-Wall (the burn mod's tracer).
pub fn send_pilot_tracer(from: Vec3, to: Vec3, arsenal: usize, amped: bool) {
    let amped = if amped { BOLT_AMPED } else { 0 };
    send(EV_PILOT_TRACER, from, to, [arsenal as u32 | amped, 0]);
}

/// A hitscan round the local Titan fired (weapons.rs): from its gun to where it struck.
pub fn send_tracer(from: Vec3, to: Vec3, arsenal: usize) {
    send(EV_TRACER, from, to, [arsenal as u32, 0]);
}

/// A body's code for the others: a player's id, or POOL_TARGET | its pool index.
pub fn code(e: &Enemy) -> Option<u32> {
    e.remote.or_else(|| e.pool.map(|i| POOL_TARGET | i as u32))
}

/// A tether catch's hold (ms) and whether it struck the Titan in flight.
pub fn catch_arg(hold: f32, direct: bool) -> u32 {
    ((hold * 1000.0).round().clamp(0.0, 1e6) as u32) | if direct { CATCH_DIRECT } else { 0 }
}

/// The local player's body (position, radius, height, a Pilot), for other players' copies to
/// strike; None when there's nothing to strike (dead, phased, in a free camera).
static LOCAL_BODY: Mutex<Option<(Vec3, f32, f32, bool)>> = Mutex::new(None);
static PVP: AtomicBool = AtomicBool::new(false);

pub fn local_body() -> Option<(Vec3, f32, f32, bool)> {
    LOCAL_BODY.lock().ok().and_then(|b| *b)
}

/// A PvP session: other players' things act on the local player (their gas, their tethers).
pub fn pvp() -> bool {
    PVP.load(Ordering::Relaxed)
}

/// Keep `local_body` and `pvp` current.
pub fn track_local(
    net: Res<Net>,
    control: Res<crate::pilotctl::Control>,
    settings: Res<crate::player::TitanSettings>,
    titans: Query<(&crate::player::PlayerTitan, &crate::combat::TitanHealth)>,
    pilots: Query<&crate::pilotctl::PlayerPilot>,
    mods: Res<crate::titankit::TitanMoveMods>,
) {
    PVP.store(net.active() && net.pvp(), Ordering::Relaxed);
    let body = match *control {
        crate::pilotctl::Control::Titan => titans
            .single()
            .ok()
            .filter(|(_, h)| h.dead_for.is_none() && !mods.phased)
            .map(|(t, _)| {
                (
                    Vec3::from(t.state.pos.to_array()),
                    settings.0.radius,
                    settings.0.height,
                    false,
                )
            }),
        crate::pilotctl::Control::Pilot => pilots.single().ok().map(|p| {
            (
                Vec3::from(p.state.pos.to_array()),
                crate::net::PILOT_RADIUS,
                crate::net::PILOT_HEIGHT,
                true,
            )
        }),
        _ => None,
    };
    if let Ok(mut b) = LOCAL_BODY.lock() {
        *b = body.filter(|_| net.active());
    }
}

/// Spawn the copies of what the other players fired and deployed since last frame.
#[allow(clippy::too_many_arguments)]
pub fn receive(
    mut commands: Commands,
    mut net: ResMut<Net>,
    kits: Res<EnemyKits>,
    fx: Res<crate::weapons::FxAssets>,
    missiles: Option<Res<crate::abilities::MissileAssets>>,
    tethers: Option<Res<crate::titankit::TetherAssets>>,
    mut ghosts: ResMut<crate::titankit::GhostEvents>,
    mut beams: ResMut<LaserCores>,
    (gd, mut pilot_defs, mut satchels): (
        Res<crate::gamedata::GameData>,
        Local<PilotDefs>,
        ResMut<crate::pilotability::GhostSatchels>,
    ),
    bodies: Query<(Entity, &Enemy)>,
    (actors, globals): (Query<&crate::actor::Actor>, Query<&GlobalTransform>),
) {
    let fx_in = std::mem::take(&mut net.fx_in);
    if !net.active() {
        return;
    }
    let my_id = net.my_id;
    // A body here by its code: the local Titan (Ok(None)), another body, or nothing.
    let resolve = |code: u32| -> Option<Option<Entity>> {
        if code == u32::MAX {
            return None;
        }
        if code & POOL_TARGET != 0 {
            let idx = (code & !POOL_TARGET) as usize;
            return bodies
                .iter()
                .find(|(_, e)| e.pool == Some(idx))
                .map(|(id, _)| Some(id));
        }
        if code == my_id {
            return Some(None);
        }
        net.remotes.get(&code).and_then(|r| r.titan).map(Some)
    };
    // The gun's muzzle on the shooter's body here (Bevy space), flashed once a frame.
    let mut flashed: Vec<u32> = Vec::new();
    let muzzle = |id: u32, attach: &str| -> Option<(Vec3, Quat)> {
        let gun = net.remotes.get(&id)?.gun?;
        let a = actors.get(gun).ok()?;
        let j = a.joint(attach).or_else(|| a.joint("muzzle_flash"))?;
        let g = globals.get(j).ok()?.compute_transform();
        Some((g.translation, g.rotation))
    };
    let gun_def = |i: u32| {
        kits.0
            .iter()
            .find(|k| k.remote && !k.infantry && k.arsenal == i as usize)
            .map(|k| &k.weapon)
    };
    for (id, kind, pos, dir, [arg, arg2]) in fx_in {
        // What the others hear as it goes out: the weapon's fire_sound_1_player_3p.
        let sound = match kind {
            EV_SLOW_TRAP => Some("incendiary_trap_deploy_3p"),
            EV_PYLON if arg2 == 1 => Some("Wpn_LaserTripMine_Deploy_3P"),
            EV_TETHER => Some("Wpn_TetherTrap_Deploy_3P"),
            EV_CLUSTER => Some("ShoulderRocket_Cluster_Fire_3P"),
            EV_SONAR => Some("Titan_Tone_SonarLock_Fired_ShoulderPod_3P"),
            EV_LASER_SHOT => Some("Weapon_ShoulderLaser_Fire_3P"),
            EV_SMOKE => Some("Titan_Offhand_ElectricSmoke_Deploy_3P"),
            _ => None,
        };
        if let Some(name) = sound {
            crate::audio::event_at(&mut commands, name, pos);
        }
        if kind == EV_MISSILE {
            // Which pod fired it, by how it flies (titankit.rs / abilities.rs specs): Flight
            // Core rockets fly straight, Tracker Rockets turn twice as hard; the others home
            // like the Multi-Target Missiles, which on Tone are its Salvo Core.
            let homing = (arg2 & 0xFFFF) as f32;
            let tone = net
                .remotes
                .get(&id)
                .and_then(|r| crate::weapons::TITAN_ARSENAL.get(r.class as usize))
                .is_some_and(|g| g.kit == "TONE");
            let name = if homing == 0.0 {
                "weapon_titan_flightcore_rocket_fire_3p"
            } else if (homing - crate::abilities::MTMS.homing * 2.0).abs() < 1.0 {
                "weapon_trackingrockets_fire_3P"
            } else if tone {
                "ShoulderRocket_Salvo_Fire_3P"
            } else {
                "ShoulderRocket_Paint_Fire_3P"
            };
            crate::audio::event_at(&mut commands, name, pos);
        }
        match kind {
            EV_BOLT | EV_TRACER => {
                let i = arg & 0xFF;
                let Some(def) = gun_def(i) else { continue };
                let flash = muzzle(id, &def.fx.muzzle_attach);
                if !flashed.contains(&id) && !def.fx.muzzle_world.is_empty() {
                    flashed.push(id);
                    if let Some((p, r)) = flash {
                        crate::pfx::emit_named_frame(&mut commands, &def.fx.muzzle_world, p, r);
                    }
                }
                if kind == EV_BOLT {
                    let gravity = crate::weapons::TITAN_ARSENAL
                        .get(i as usize)
                        .map_or(0.0, |g| g.gravity);
                    crate::pilotweapon::spawn_ghost_bolt(
                        &mut commands,
                        &fx,
                        pos,
                        dir,
                        gravity,
                        def,
                        true,
                        arg & BOLT_WILDFIRE != 0,
                    );
                } else {
                    let from = flash.map_or(to_bevy(pos), |f| f.0);
                    let to = to_bevy(dir);
                    if !def.fx.tracer_world.is_empty() {
                        crate::pfx::emit_beam(&mut commands, &def.fx.tracer_world, from, to);
                    }
                    crate::particles::emit(
                        &mut commands,
                        crate::particles::Effect::Hit {
                            at: to,
                            normal: (from - to).normalize_or(Vec3::Y),
                            table: def.impact_table,
                            surface: crate::particles::Surface::World,
                            victim: false,
                            scale: 1.6,
                        },
                    );
                }
            }
            EV_PILOT_BOLT | EV_PILOT_TRACER => {
                let i = (arg & 0xFF) as usize;
                let Some(def) = pilot_defs.get(&gd, i) else {
                    continue;
                };
                if kind == EV_PILOT_BOLT {
                    crate::pilotweapon::spawn_ghost_bolt(
                        &mut commands,
                        &fx,
                        pos,
                        dir,
                        f32::from_bits(arg2),
                        def,
                        false,
                        false,
                    );
                    continue;
                }
                // From where the gun is held (the remote Pilot carries no gun model): 30 ahead
                // of the eye, as the round left it.
                let to = to_bevy(dir);
                let from = to_bevy(pos + (dir - pos).normalize_or_zero() * 30.0);
                let tdef = def
                    .burn
                    .as_deref()
                    .filter(|_| arg & BOLT_AMPED != 0)
                    .unwrap_or(def);
                if !tdef.fx.tracer_world.is_empty() {
                    crate::pfx::emit_beam(&mut commands, &tdef.fx.tracer_world, from, to);
                }
                crate::particles::emit(
                    &mut commands,
                    crate::particles::Effect::Hit {
                        at: to,
                        normal: (from - to).normalize_or(Vec3::Y),
                        table: def.impact_table,
                        surface: crate::particles::Surface::World,
                        victim: false,
                        scale: 1.0,
                    },
                );
            }
            EV_GRENADE => crate::pilotability::spawn_ghost_grenade(
                &mut commands,
                id,
                arg,
                pos,
                dir,
                arg2 as f32 / 1000.0,
            ),
            EV_SATCHELS_BOOM => satchels.0.push(id),
            EV_DECOY_DOWN if arg == my_id => DECOY_DOWN.store(true, Ordering::Relaxed),
            EV_LASER_SHOT => {
                let (a, b) = (to_bevy(pos), to_bevy(dir));
                crate::pfx::emit_beam(&mut commands, "P_wpn_hand_laser_beam", a, b);
                crate::particles::emit(
                    &mut commands,
                    crate::particles::Effect::Hit {
                        at: b,
                        normal: (a - b).normalize_or(Vec3::Y),
                        table: "exp_laser_lite",
                        surface: if arg != 0 {
                            crate::particles::Surface::Titan
                        } else {
                            crate::particles::Surface::World
                        },
                        victim: false,
                        scale: 2.5,
                    },
                );
            }
            EV_POWER_SHOT => {
                let long = arg != 0;
                let (a, rot) = muzzle(id, "muzzle_flash").unwrap_or((to_bevy(pos), Quat::IDENTITY));
                let b = to_bevy(dir);
                crate::pfx::emit_named_frame(
                    &mut commands,
                    if long {
                        "P_muzzleflash_predator_PS_ALT"
                    } else {
                        "P_muzzleflash_predator_PS"
                    },
                    a,
                    rot,
                );
                crate::pfx::emit_beam(&mut commands, "P_weapon_tracers_predator", a, b);
                crate::particles::emit(
                    &mut commands,
                    crate::particles::Effect::Hit {
                        at: b,
                        normal: (a - b).normalize_or(Vec3::Y),
                        table: if long {
                            "predator_bullet_ps_alt"
                        } else {
                            "predator_bullet_ps"
                        },
                        surface: match arg2 {
                            1 => crate::particles::Surface::Titan,
                            2 => crate::particles::Surface::Flesh,
                            _ => crate::particles::Surface::World,
                        },
                        victim: false,
                        scale: 2.5,
                    },
                );
            }
            EV_CLUSTER => crate::titankit::spawn_cluster(&mut commands, pos, dir, true),
            EV_MISSILE => {
                let Some(assets) = missiles.as_deref() else {
                    continue;
                };
                let target = resolve(arg);
                let speed = dir.length().max(1.0);
                let spec = crate::abilities::MissileSpec {
                    speed,
                    homing: (arg2 & 0xFFFF) as f32,
                    direct: 0.0,
                    splash: 0.0,
                    radius: 1.0,
                    out_time: (arg2 >> 16) as f32 / 1000.0,
                };
                crate::abilities::spawn_missile_as(
                    &mut commands,
                    assets,
                    pos,
                    dir / speed,
                    target.flatten(),
                    spec,
                    Some(target == Some(None)),
                );
            }
            EV_TETHER => {
                crate::titankit::spawn_ghost_tether(&mut commands, tethers.as_deref(), pos, dir)
            }
            EV_TETHER_CATCH => {
                let target = resolve(arg);
                ghosts.catch.push(crate::titankit::GhostCatch {
                    at: pos,
                    me: target == Some(None),
                    body: target.flatten(),
                    hold: (arg2 & !CATCH_DIRECT) as f32 / 1000.0,
                    direct: arg2 & CATCH_DIRECT != 0,
                });
            }
            EV_SLOW_TRAP => crate::titankit::spawn_slow_trap(&mut commands, pos, dir, true),
            EV_TRAP_IGNITE => ghosts.ignite.push(pos),
            EV_PYLON => crate::titankit::spawn_pylon(
                &mut commands,
                pos,
                dir,
                ghost_group(id, arg),
                arg2 as u8,
                true,
            ),
            EV_PYLONS_BLOW => ghosts.blow.push(ghost_group(id, arg)),
            EV_SMOKE => crate::titankit::spawn_smoke_as(&mut commands, pos, dir, false, true),
            EV_SONAR => crate::titankit::spawn_sonar(&mut commands, pos, dir, true),
            EV_LASER_CORE => {
                let Some(s) = net.remotes.get(&id).and_then(|r| r.snap) else {
                    continue;
                };
                let b = beams.0.entry(id).or_default();
                b.offset = pos - s.titan_pos;
                b.len = pos.distance(dir);
                b.age = 0.0;
            }
            _ => {}
        }
    }
}

/// Pilot weapons by pilotweapon::ARSENAL index, loaded as other players first fire them.
#[derive(Default)]
pub struct PilotDefs(std::collections::HashMap<usize, crate::weapons::WeaponDef>);

impl PilotDefs {
    fn get(
        &mut self,
        gd: &crate::gamedata::GameData,
        i: usize,
    ) -> Option<&crate::weapons::WeaponDef> {
        let entry = crate::pilotweapon::ARSENAL.get(i)?;
        Some(self.0.entry(i).or_insert_with(|| {
            let mut read = |p: &str| {
                gd.read_file(p)
                    .ok()
                    .map(|b| String::from_utf8_lossy(&b).to_string())
            };
            crate::weapons::WeaponDef::load_player(entry.id, &mut read)
        }))
    }
}

/// Another player's tripwire group here: apart from the local player's and each other's.
fn ghost_group(player: u32, group: u32) -> u32 {
    crate::titankit::GHOST_GROUP | ((player & 0x7FF) << 20) | (group & 0xF_FFFF)
}

/// The others' Laser Cores as their owners last sent them: the beam's start from the Titan's
/// origin, its length, and seconds since.
#[derive(Resource, Default)]
pub struct LaserCores(std::collections::HashMap<u32, LaserCore>);

#[derive(Default)]
pub struct LaserCore {
    offset: Vec3,
    len: f32,
    age: f32,
    fx: Option<(Entity, Entity)>,
}

/// The owner sends its beam every damage tick (0.1 s); a beam not heard from for longer than
/// this is over.
const LASER_CORE_TIMEOUT: f32 = 0.3;

/// Draw the others' Laser Cores: P_wpn_lasercannon (the weapon's sustained_laser_effect_3p)
/// from the Titan's muzzle along its aim, as long as the owner's beam reached, and
/// P_lasercannon_endglow where it strikes.
pub fn remote_laser_cores(
    mut commands: Commands,
    time: Res<Time>,
    net: Res<Net>,
    mut cores: ResMut<LaserCores>,
    bodies: Query<&Enemy>,
) {
    let dt = time.delta_secs();
    for (id, c) in cores.0.iter_mut() {
        c.age += dt;
        let r = net.remotes.get(id);
        let live = r.and_then(|r| Some((r.snap?, bodies.get(r.titan?).ok()?)));
        let Some((s, body)) = live.filter(|_| c.age < LASER_CORE_TIMEOUT) else {
            if let Some((beam, glow)) = c.fx.take() {
                commands.entity(beam).despawn();
                commands.entity(glow).despawn();
            }
            continue;
        };
        let dir = Vec3::new(
            s.titan_pitch.cos() * s.titan_yaw.cos(),
            s.titan_pitch.cos() * s.titan_yaw.sin(),
            -s.titan_pitch.sin(),
        );
        let from = body.pos + c.offset;
        let (a, b) = (to_bevy(from), to_bevy(from + dir * c.len));
        let (beam, glow) = *c.fx.get_or_insert_with(|| {
            crate::audio::event_at(&mut commands, "Titan_Core_Laser_FireBeam_3P", from);
            (
                commands
                    .spawn((
                        Transform::from_translation(a),
                        crate::pfx::PfxTrail::new("P_wpn_lasercannon"),
                        crate::pfx::PfxBeamEnd(b),
                    ))
                    .id(),
                commands
                    .spawn((
                        Transform::from_translation(b),
                        crate::pfx::PfxTrail::new("P_lasercannon_endglow"),
                    ))
                    .id(),
            )
        });
        commands.entity(beam).insert((
            Transform::from_translation(a).looking_at(b, Vec3::Y),
            crate::pfx::PfxBeamEnd(b),
        ));
        commands.entity(glow).insert(Transform::from_translation(b));
    }
    cores
        .0
        .retain(|_, c| c.fx.is_some() || c.age < LASER_CORE_TIMEOUT);
}

/// One segment of another player's grapple rope: (player, index from the hand).
#[derive(Component)]
pub struct RemoteCable(u32, usize);

/// Draw the others' grapple ropes from their snapshots (`PlayerSnap::rope`): from the Pilot
/// body's left wrist through the bends to the anchor, or to the hook in flight, the way
/// `pilotability::grapple_cable` draws the local one.
#[allow(clippy::type_complexity)]
pub fn remote_cables(
    mut commands: Commands,
    net: Res<Net>,
    bodies: Query<&Enemy>,
    actors: Query<&crate::actor::Actor>,
    globals: Query<&GlobalTransform>,
    cams: Query<&GlobalTransform, With<crate::player::MainCamera>>,
    mut cables: Query<(Entity, &RemoteCable, &mut Transform, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut look: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    let mut ropes: Vec<(u32, Vec<Vec3>)> = Vec::new();
    for (id, r) in &net.remotes {
        let Some(s) = r.snap.filter(|s| s.on_foot && s.rope_len > 0) else {
            continue;
        };
        let Some(body) = r.pilot.and_then(|p| bodies.get(p).ok()) else {
            continue;
        };
        let hand = actors
            .get(body.actor)
            .ok()
            .and_then(|a| a.joint("def_l_wrist"))
            .and_then(|j| globals.get(j).ok())
            .map(|g| g.translation())
            .unwrap_or_else(|| to_bevy(body.pos + Vec3::Z * 48.0));
        let points: Vec<Vec3> = std::iter::once(hand)
            .chain(s.rope[..s.rope_len as usize].iter().map(|p| to_bevy(*p)))
            .collect();
        log::debug!(
            "rope of player {id}: {} points, pilot at {:.0}, end {:.0}",
            points.len(),
            body.pos,
            s.rope[s.rope_len as usize - 1]
        );
        ropes.push((*id, points));
    }
    let (mesh, mat) = look
        .get_or_insert_with(|| {
            (
                meshes.add(Cylinder::new(1.0, 1.0)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.1, 0.1, 0.11),
                    metallic: 0.2,
                    perceptual_roughness: 0.6,
                    ..default()
                }),
            )
        })
        .clone();
    // Four segments per rope, made the first time a player grapples.
    for (id, _) in &ropes {
        if !cables.iter().any(|(_, c, _, _)| c.0 == *id) {
            for i in 0..4 {
                commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                    RemoteCable(*id, i),
                    bevy::light::NotShadowCaster,
                ));
            }
        }
    }
    let cam = cams.single().map(|c| c.translation()).unwrap_or(Vec3::ZERO);
    let u = crate::player::UNIT;
    for (e, c, mut tf, mut vis) in &mut cables {
        if !net.remotes.contains_key(&c.0) {
            commands.entity(e).despawn();
            continue;
        }
        let seg = ropes
            .iter()
            .find(|(id, _)| *id == c.0)
            .and_then(|(_, p)| Some((*p.get(c.1)?, *p.get(c.1 + 1)?)));
        let Some((a, b)) = seg.filter(|(a, b)| a.distance(*b) > 1e-3) else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let d = b - a;
        let len = d.length();
        let mid = a + d * 0.5;
        let r = (0.35 * u).max((mid - cam).length() * 0.0012);
        *tf = Transform {
            translation: mid,
            rotation: Quat::from_rotation_arc(Vec3::Y, d / len),
            scale: Vec3::new(r, len, r),
        };
        vis.set_if_neq(Visibility::Visible);
    }
}

/// The local decoy as the snapshot sends it: feet, velocity, yaw, body move code.
static LOCAL_DECOY: Mutex<Option<(Vec3, Vec3, f32, u8)>> = Mutex::new(None);
/// Another player shot our decoy down (`EV_DECOY_DOWN`).
static DECOY_DOWN: AtomicBool = AtomicBool::new(false);

pub fn local_decoy() -> Option<(Vec3, Vec3, f32, u8)> {
    LOCAL_DECOY.lock().ok().and_then(|d| *d)
}

/// Keep `LOCAL_DECOY` current for the snapshot, and end our decoy when someone else shot it
/// down (its timeout runs out, so `update_decoy` dissolves it as at the end of its time).
pub fn track_decoy(
    time: Res<Time>,
    mut status: ResMut<crate::pilotability::PilotStatus>,
    mut prev: Local<Option<Vec3>>,
) {
    use crate::net::*;
    use crate::pilotability::DecoyState;
    if DECOY_DOWN.swap(false, Ordering::Relaxed) {
        if let Some(d) = status.decoy.as_mut() {
            log::info!("net: our decoy was shot down");
            d.time = 0.0;
        }
    }
    let dt = time.delta_secs();
    let now = status.decoy.map(|d| {
        let vel = prev
            .filter(|_| dt > 0.0)
            .map_or(Vec3::ZERO, |p| (d.pos - p) / dt);
        let left = Vec3::new(-d.dir.y, d.dir.x, 0.0);
        let code = match d.state {
            DecoyState::Idle | DecoyState::CrouchIdle => MOVE_IDLE,
            DecoyState::CrouchWalk | DecoyState::CrouchStrafe { .. } => MOVE_CROUCH_RUN,
            DecoyState::Run | DecoyState::Strafe { .. } | DecoyState::Sprint => MOVE_RUN,
            DecoyState::Slide { .. } => MOVE_SLIDE,
            DecoyState::Air => MOVE_AIR,
            DecoyState::WallRun { normal, .. } if normal.dot(left) < 0.0 => MOVE_WALLRUN_LEFT,
            DecoyState::WallRun { .. } => MOVE_WALLRUN_RIGHT,
            DecoyState::WallHang => MOVE_WALL_HANG,
        };
        (d.pos, vel, d.dir.y.atan2(d.dir.x), code)
    });
    if now.is_some() != prev.is_some() {
        log::info!(
            "net: our decoy {}",
            if now.is_some() { "out" } else { "gone" }
        );
    }
    *prev = now.map(|d| d.0);
    if let Ok(mut d) = LOCAL_DECOY.lock() {
        *d = now;
    }
}

/// A remote decoy body's state here.
#[derive(Default)]
pub struct DecoyGhost {
    /// Shown for a decoy of its owner's.
    live: bool,
    /// Its death sent to the owner.
    reported: bool,
    /// Seconds into its dissolve.
    dissolve: Option<f32>,
}

/// The others' Holo Pilot decoys: a Pilot body run from their snapshot (`PlayerSnap::decoy`),
/// with the decoy's 50 health. Shooting it down plays its death here and tells the owner,
/// whose decoy then ends; when the owner's decoy ends it dissolves (Decoy_Dissolve: the
/// `dissolve` effect and sounds, the body gone once its fade is over).
pub fn remote_decoys(
    mut commands: Commands,
    time: Res<Time>,
    net: Res<Net>,
    kits: Res<EnemyKits>,
    mut bodies: Query<&mut Enemy>,
    mut ghosts: Local<std::collections::HashMap<Entity, DecoyGhost>>,
) {
    use crate::pilotability::{DECOY_DISSOLVE_FADE, DECOY_DISSOLVE_TOTAL, DECOY_HEALTH};
    let dt = time.delta_secs();
    ghosts.retain(|e, _| bodies.contains(*e));
    for (id, r) in &net.remotes {
        let (Some(s), Some(de)) = (r.snap, r.decoy) else {
            continue;
        };
        let Ok(mut e) = bodies.get_mut(de) else {
            continue;
        };
        let g = ghosts.entry(de).or_default();
        if let Some(t) = g.dissolve.as_mut() {
            *t += dt;
            if *t >= DECOY_DISSOLVE_FADE.1 {
                e.active = false;
                g.dissolve = None;
            }
            continue;
        }
        if s.decoy != 0 {
            if !g.live {
                let kit = &kits.0[e.kit.min(kits.0.len() - 1)];
                e.activate(s.decoy_pos, s.decoy_yaw, kit);
                e.cancel_arrival();
                e.v.health = DECOY_HEALTH;
                e.v.max_health = DECOY_HEALTH;
                g.live = true;
                g.reported = false;
                log::info!("net: player {id}'s decoy at {:.0}", s.decoy_pos);
            }
            if e.alive() {
                e.set_remote_pose(s.decoy_pos, s.decoy_vel, s.decoy_yaw, 0.0);
                e.body_move = s.decoy - 1;
                log::debug!(
                    "decoy body: snap {:.0} body {:.0} active {} hidden {} move {}",
                    s.decoy_pos,
                    e.pos,
                    e.active,
                    e.hidden,
                    e.body_move
                );
            } else if !g.reported {
                g.reported = true;
                send(EV_DECOY_DOWN, e.pos, Vec3::ZERO, [*id, 0]);
            }
        } else if g.live {
            g.live = false;
            g.dissolve = Some(0.0);
            log::info!("net: player {id}'s decoy dissolves");
            crate::audio::event_at(&mut commands, "holopilot_end_3P", e.pos);
            crate::audio::event_at(&mut commands, "Death.Dissolve", e.pos);
            commands.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 36.0)),
                crate::pfx::PfxTrail::oriented("dissolve"),
                crate::pfx::PfxOneShot(DECOY_DISSOLVE_TOTAL),
                ChildOf(de),
            ));
        } else if e.active {
            // A new body before any decoy.
            e.active = false;
        }
    }
}
