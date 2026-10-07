//! Titanfall-rs: play as BT in a Titanfall 2 map, read straight from the game install.
//!
//! Usage: tf-viewer [--game DIR] [--map NAME] [--no-props]
//!                  [--screenshot FILE [--after SECS]] [--shots "T:FILE,T:FILE"]
//!                  [--cam "x y z" --look "x y z"]   (start in free camera, game units)
//!                  [--script "0-2:fwd,1-2:sprint,2.5:dash,..."]  (scripted input, for tests)
//!
//! Controls: click grabs the mouse (Esc releases), WASD moves, Shift sprints, Space dashes,
//! C switches cockpit / third person, F1 toggles a free camera. Left mouse fires the XO-16,
//! right mouse zooms, R reloads, hold Q to lock and fire missiles, V triggers Burst Core,
//! hold E for the Vortex Shield, F punches. X disembarks/embarks; on foot Space jumps (twice),
//! Ctrl crouches/slides, running along walls wall-runs, and T calls BT down with a Titanfall.

mod abilities;
mod actor;
mod audio;
mod combat;
mod convert;
mod crosshair;
mod dialogue;
mod env;
mod game;
mod gamedata;
mod hitbox;
mod hud;
mod executions;
mod music;
mod nav;
mod aimassist;
mod gamepad;
mod pilotability;
mod pilotbody;
mod autotitan;
mod rodeo;
mod fparms;
mod settings;
mod pilotctl;
mod pilotweapon;
mod particles;
mod pfx;
mod player;
mod ruiscreen;
mod sky;
mod spread;
mod smartammo;
mod targets;
mod titankit;
mod uber;
mod ui;
mod vitals;
mod vmcam;
mod vmmotion;
mod weapons;
mod world;
mod zipline;

use actor::ActorSpec;
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use gamedata::GameData;
use player::{to_bevy, CameraMode, Collision, MainCamera, PlayerInput, PlayerTitan, Script, TitanSettings, UNIT};
use std::f32::consts::FRAC_PI_2;
use tf_assets::settings::PlayerSettings;
use tf_sim::collision::CollisionWorld;
use tf_sim::titan::{TitanParams, TitanState};

#[derive(Resource, Clone)]
struct Args {
    game: String,
    map: String,
    props: bool,
    shots: Vec<(f32, String)>,
    cam: Option<Vec3>,
    look: Option<Vec3>,
    script: Option<String>,
    difficulty: vitals::Difficulty,
    volume: Option<f32>,
    /// "primary,sidearm,antititan" weapon ids (e.g. mp_weapon_hemlok).
    loadout: Option<String>,
    /// BT's loadout at start (index into weapons::TITAN_ARSENAL).
    titan: usize,
}

fn parse_vec(s: &str) -> Option<Vec3> {
    let v: Vec<f32> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2]))
}

/// The Titanfall 2 install: `TITANFALL2_DIR`, else the first Steam library (from each Steam
/// root's `libraryfolders.vdf`) that has `steamapps/common/Titanfall2/vpk`. `--game DIR`
/// overrides both.
fn find_game_dir() -> String {
    if let Ok(dir) = std::env::var("TITANFALL2_DIR") {
        return dir;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let roots = [
        format!("{home}/.local/share/Steam"),
        format!("{home}/.steam/steam"),
        format!("{home}/.var/app/com.valvesoftware.Steam/.local/share/Steam"),
        "C:/Program Files (x86)/Steam".to_string(),
    ];
    let mut libraries: Vec<String> = Vec::new();
    for root in &roots {
        libraries.push(root.clone());
        if let Ok(vdf) = std::fs::read_to_string(format!("{root}/steamapps/libraryfolders.vdf")) {
            for line in vdf.lines() {
                let parts: Vec<&str> = line.split('"').collect();
                if parts.len() >= 4 && parts[1] == "path" {
                    libraries.push(parts[3].replace("\\\\", "/"));
                }
            }
        }
    }
    libraries
        .iter()
        .map(|lib| format!("{lib}/steamapps/common/Titanfall2"))
        .find(|dir| std::path::Path::new(&format!("{dir}/vpk")).is_dir())
        .unwrap_or_else(|| "Titanfall2".to_string())
}

fn parse_args() -> Args {
    let mut a = Args {
        game: find_game_dir(),
        map: "mp_forwardbase_kodai".into(),
        props: true,
        shots: Vec::new(),
        cam: None,
        look: None,
        script: None,
        difficulty: vitals::Difficulty::Normal,
        volume: None,
        loadout: None,
        titan: 0,
    };
    let mut single: Option<String> = None;
    let mut after = 4.0;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--game" => a.game = it.next().expect("--game DIR"),
            "--map" => a.map = it.next().expect("--map NAME"),
            "--no-props" => a.props = false,
            "--screenshot" => single = it.next(),
            "--after" => after = it.next().and_then(|s| s.parse().ok()).unwrap_or(4.0),
            "--shots" => {
                for item in it.next().unwrap_or_default().split(',') {
                    if let Some((t, f)) = item.split_once(':') {
                        a.shots.push((t.trim().parse().unwrap_or(0.0), f.trim().to_string()));
                    }
                }
            }
            "--cam" => a.cam = it.next().and_then(|s| parse_vec(&s)),
            "--look" => a.look = it.next().and_then(|s| parse_vec(&s)),
            "--script" => a.script = it.next(),
            "--loadout" => a.loadout = it.next(),
            "--titan" => a.titan = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--volume" => a.volume = it.next().and_then(|v| v.parse().ok()),
            "--difficulty" => a.difficulty = it.next().and_then(|d| vitals::Difficulty::parse(&d)).expect("--difficulty easy|normal|hard|master"),
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    if let Some(f) = single {
        a.shots.push((after, f));
    }
    a
}

fn main() {
    let args = parse_args();
    if !std::path::Path::new(&args.game).join("vpk").is_dir() {
        eprintln!(
            "Titanium Dioxide needs your own Titanfall 2 install, and none was found at {:?}.\n\
             Pass --game /path/to/Titanfall2 or set TITANFALL2_DIR.",
            args.game
        );
        std::process::exit(1);
    }
    let script = args.script.as_deref().map(Script::parse).unwrap_or_default();
    // First person in the cockpit, as the game always is (C toggles the third-person camera).
    let mode = if args.cam.is_some() { CameraMode::Free } else { CameraMode::Cockpit };
    let map_choice = ui::MapChoice(args.map.clone());
    let user_settings = settings::Settings::load();
    let start_volume = args.volume.unwrap_or(user_settings.volume);
    let start_titan = args.titan;
    let mut loadout = pilotweapon::LoadoutChoice::default();
    if let Some(l) = &args.loadout {
        let ids: Vec<&str> = l.split(',').collect();
        for (slot, id) in [pilotweapon::Slot::Primary, pilotweapon::Slot::Sidearm, pilotweapon::Slot::AntiTitan].into_iter().zip(ids) {
            if let Some(i) = pilotweapon::arsenal_index(id.trim()) {
                loadout.set(slot, i);
            }
        }
    }
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Titanfall-rs".into(),
                        resolution: (1600u32, 900u32).into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(ImagePlugin { default_sampler: convert::sampler() }),
        )
        .add_plugins(FreeCameraPlugin)
        .add_plugins(ui::PipelineWatchPlugin)
        .add_plugins(audio::GameAudioPlugin)
        .add_plugins(convert::BlendMaterialPlugin)
        .add_plugins(pfx::SoftMaterialPlugin)
        .add_plugins(uber::UberPlugin)
        .add_plugins(particles::ParticlePlugin)
        .add_systems(Update, (perf_log, weapons::shimmer_shields))
        .insert_resource(args)
        .insert_resource(script)
        .insert_resource(mode)
        .init_resource::<PlayerInput>()
        .insert_resource(crate::audio::MasterVolume::new(bevy::audio::Volume::Linear(start_volume.clamp(0.0, 1.0))))
        .insert_resource(user_settings)
        .init_resource::<gamepad::PadConfig>()
        .init_resource::<gamepad::PadState>()
        .init_resource::<aimassist::AimAssist>()
        .init_resource::<aimassist::PullClasses>()
        .init_resource::<ui::Subtitles>()
        .init_resource::<ui::MissionImages>()
        .add_systems(Update, (ui::sync_volume_setting, settings::save_settings, ui::update_subtitles).run_if(ui::loaded))
        .add_systems(Update, (gamepad::load_pad_config, aimassist::load_pull_classes, hitbox::load_enemy_hitboxes).run_if(ui::world_ready))
        .add_systems(Update, aimassist::aim_assist.run_if(ui::loaded).before(player::gather_input))
        .add_systems(Update, settings::apply_fov.after(player::update_camera).after(pilotctl::pilot_camera).run_if(ui::loaded))
        .add_systems(Update, pilotctl::cockpit_boot.run_if(ui::loaded))
        .add_systems(First, ui::log_hitches)
        .add_systems(Update, (zipline::zipline_cables, zipline::zipline_sounds).run_if(ui::loaded))
        .add_systems(Update, pilotability::grapple_cable.after(pilotctl::pilot_camera).run_if(ui::loaded))
        .add_systems(Update, pilotctl::bt_sequence_sounds.after(pilotctl::control_transitions).run_if(ui::loaded))
        .init_resource::<weapons::ViewPunch>()
        .init_resource::<pilotctl::VmEye>()
        .init_resource::<pilotability::GrenadeModels>()
        .init_resource::<pilotweapon::VmPlacements>()
        .init_resource::<crosshair::CrosshairState>()
        .init_resource::<pilotctl::Shake>()
        .init_resource::<hud::DamageFrom>()
        .init_resource::<hitbox::RecentHits>()
        .init_resource::<pilotability::PilotStatus>()
        .init_resource::<executions::TitanExecution>()
        .init_resource::<executions::PilotExecution>()
        .init_resource::<executions::PilotMelee>()
        .init_resource::<pilotability::OrdnanceThrow>()
        .init_resource::<pilotability::Shields>()
        .init_resource::<game::Game>()
        .insert_resource(pilotctl::Control::Titan)
        .insert_resource(pilotctl::PilotSettings(tf_sim::pilot::PilotParams::default()))
        .insert_resource(ClearColor(Color::srgb(0.55, 0.66, 0.78)))
        .insert_resource(ui::LoadState::Waiting(0))
        .init_resource::<ui::Menu>()
        .init_resource::<ui::Quality>()
        .insert_resource(loadout)
        .insert_resource(map_choice)
        .insert_resource(weapons::TitanKit(start_titan))
        .init_resource::<titankit::ActiveKit>()
        .init_resource::<titankit::BoltShields>()
        .init_resource::<titankit::EnemyWalls>()
        .init_resource::<titankit::TitanBoltHits>()
        .init_resource::<titankit::KitView>()
        .init_resource::<titankit::TitanMoveMods>()
        .init_resource::<rodeo::Rodeo>()
        .init_resource::<autotitan::AutoTitan>()
        .init_resource::<fparms::FpSeq>()
        .init_resource::<hud::HudHidden>()
        .add_systems(Startup, (weapons::setup_fx, ui::load_ui_assets, ui::spawn_loading_screen).chain())
        .add_systems(Update, (setup, hud::spawn_hud, pilotability::spawn_pilot_slots, pilotability::setup_pilot_kit, pilotbody::spawn_pilot_body, rodeo::spawn_battery_hud, fparms::spawn_arms).chain().run_if(ui::loading).before(ui::loading_tick))
        .add_systems(Update, (ui::loading_tick, ui::scale_ui, screenshots, env::nearest_reflection, uber::swap_materials, vmcam::tag_meshes))
        .add_systems(Update, (ui::sync_menu, ui::menu_input, ui::menu_visuals, ui::mission_images, ui::apply_quality, pilotweapon::build_loadout, pilotweapon::ordnance_viewmodel, weapons::switch_titan_weapon, titankit::apply_kit, weapons::prepare_weapon_models, pilotability::preload_grenade_models).chain().run_if(ui::world_ready).before(player::gather_input))
        .add_systems(Update, ruiscreen::weapon_screens.run_if(ui::loaded).after(pilotweapon::pilot_weapon_fire))
        .add_systems(
            Update,
            (
                (
                    player::gather_input,
                    game::gate_input,
                    targets::script_aim,
                    game::director,
                    (zipline::zipline_grab, pilotctl::control_transitions, autotitan::auto_titan_think).chain(),
                    player::route_input,
                    player::simulate,
                    (pilotctl::simulate_pilot, rodeo::rodeo_update, fparms::fp_seq_update).chain(),
                    (executions::titan_execution, executions::pilot_execution).chain(),
                    weapons::weapon_fire,
                    pilotweapon::pilot_weapon_fire,
                    pilotweapon::update_pilot_bolts,
                    abilities::update_abilities,
                    abilities::update_missiles,
                    titankit::kit_input,
                    targets::update_enemies,
                    (titankit::kit_world, titankit::utility_world).chain(),
                    targets::update_bolts,
                    targets::death_explosions,
                    combat::player_combat,
                )
                    .chain(),
                (
                    player::drive_animation,
                    weapons::weapon_viewmodel,
                    pilotweapon::pilot_viewmodel,
                    (pilotbody::update_pilot_body, actor::animate_actors).chain(),
                    actor::bone_merge,
                    player::update_camera,
                    (pilotctl::pilot_camera, rodeo::rodeo_camera, fparms::fp_seq_camera, executions::execution_camera).chain(),
                    (sky::follow_camera, vmcam::follow_camera, pilotweapon::place_viewmodels),
                    weapons::update_fx,
                    targets::update_health_bars,
                    titankit::kit_view,
                    (hud::update_hud, crosshair::update, hud::hud_sway, hud::lost_health_flash),
                    (hud::update_hud_state, hud::hud_boot, (hud::health_hex, hud::stim_fx), player::cockpit_damage_light).chain(),
                    hud::hit_feedback,
                    hud::update_damage_arcs,
                    hud::update_smart_hud,
                    pilotability::pilot_abilities,
                    (pilotability::update_grenades, pilotability::grenade_visuals).chain(),
                    pilotability::update_pilot_slots,
                    (pilotability::update_pulse_markers, rodeo::battery_hud).chain(),
                )
                    .chain(),
            )
                .chain()
                .run_if(ui::loaded),
        )
        .run();
}

/// How many grunts can be on the field at once.
const GRUNT_POOL: usize = 12;

/// Find a Titan start spawn point in the map's spawn entity file.
fn titan_spawn(gd: &GameData, map: &str) -> Option<(Vec3, f32)> {
    let text = String::from_utf8_lossy(&gd.read_file(&format!("maps/{map}_spawn.ent")).ok()?).to_string();
    let ents = tf_assets::bsp::parse_entities(&text);
    let get = |e: &Vec<(String, String)>, k: &str| e.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.clone());
    let e = ents.iter().find(|e| get(e, "classname").as_deref() == Some("info_spawnpoint_titan_start"))?;
    let origin = parse_vec(&get(e, "origin")?)?;
    let yaw = parse_vec(&get(e, "angles").unwrap_or_default()).map(|a| a.y).unwrap_or(0.0);
    Some((origin, yaw))
}

/// Campaign maps have no Titan start: put BT where the level's enemy Titans are placed
/// (`npc_titan` in `_script.ent`, minus BT-model friendlies), at the placement with the most
/// others within 5000 units, facing their centre. Returns that spot, its yaw and every enemy
/// placement. Maps without Titans fall back to `info_player_start`.
fn campaign_arena(gd: &GameData, map: &str) -> Option<(Vec3, f32, Vec<Vec3>)> {
    let text = String::from_utf8_lossy(&gd.read_file(&format!("maps/{map}_script.ent")).ok()?).to_string();
    let ents = tf_assets::bsp::parse_entities(&text);
    let get = |e: &Vec<(String, String)>, k: &str| e.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.clone());
    let titans: Vec<Vec3> = ents
        .iter()
        .filter(|e| get(e, "classname").is_some_and(|c| c.starts_with("npc_titan")))
        .filter(|e| !get(e, "model").unwrap_or_default().contains("buddy"))
        .filter_map(|e| parse_vec(&get(e, "origin")?))
        .collect();
    let near = |p: &Vec3| titans.iter().filter(|q| q.distance(*p) < 5000.0).count();
    if let Some(best) = titans.iter().max_by_key(|p| near(p)) {
        let group: Vec<Vec3> = titans.iter().copied().filter(|q| q.distance(*best) < 5000.0).collect();
        let centre = group.iter().copied().sum::<Vec3>() / group.len() as f32;
        let to = centre - *best;
        let yaw = if to.truncate().length() > 1.0 { to.y.atan2(to.x).to_degrees() } else { 0.0 };
        return Some((*best, yaw, titans));
    }
    let start = ents.iter().find(|e| get(e, "classname").as_deref() == Some("info_player_start"))?;
    let origin = parse_vec(&get(start, "origin")?)?;
    let yaw = parse_vec(&get(start, "angles").unwrap_or_default()).map(|a| a.y).unwrap_or(0.0);
    Some((origin, yaw, Vec::new()))
}

/// Pilot movement from the campaign pilot's .set file (pilot_solo.set over pilot_base.set).
fn pilot_params(gd: &GameData) -> tf_sim::pilot::PilotParams {
    let mut read = |p: &str| gd.read_file(p).ok().map(|b| String::from_utf8_lossy(&b).to_string());
    let d = tf_sim::pilot::PilotParams::default();
    let Some(s) = PlayerSettings::load("scripts/players/mp/pilot_solo.set", true, &mut read) else { return d };
    let view = |k: &str, def: f32| s.vec3(k).map(|v| v[2]).unwrap_or(def);
    tf_sim::pilot::PilotParams {
        walk_speed: s.f32("stand.speed", d.walk_speed),
        sprint_speed: s.f32("stand.sprintspeed", d.sprint_speed),
        crouch_speed: s.f32("crouch.speed", d.crouch_speed),
        ground_accel: s.f32("stand.acceleration", d.ground_accel),
        air_speed: s.f32("global.airspeed", d.air_speed),
        air_accel: s.f32("global.airacceleration", d.air_accel),
        jump_height: s.f32("global.jumpheight", d.jump_height),
        double_jump_height: s.f32("global.superjumpmaxheight", d.double_jump_height),
        double_jump_horz: s.f32("global.superjumphorzspeed", d.double_jump_horz),
        gravity: 750.0 * s.f32("global.gravityscale", 0.75),
        slide_boost: s.f32("global.slidespeedboost", d.slide_boost),
        slide_min_speed: s.f32("global.sliderequiredstartspeed", d.slide_min_speed),
        slide_decel: s.f32("global.slidedecel", d.slide_decel),
        slide_velocity_decay: s.f32("global.slidevelocitydecay", d.slide_velocity_decay),
        slide_boost_cap: s.f32("global.slidespeedboostcap", d.slide_boost_cap),
        slide_stop_speed: s.f32("global.slidestopspeed", d.slide_stop_speed),
        slide_jump_height: s.f32("global.slidejumpheight", d.slide_jump_height),
        wallrun_accel_v: s.f32("global.wallrunacceleratevertical", d.wallrun_accel_v),
        wallrun_hang_time: s.f32("global.wallrun_hangtimelimit", d.wallrun_hang_time),
        // TF_WALLHANG=1: the MP wall-hang kit (pas_wallhang: wallrunAdsType "wallhang").
        wallhang_on_ads: s.get("global.wallrunadstype").is_some_and(|v| v.eq_ignore_ascii_case("wallhang")) || std::env::var_os("TF_WALLHANG").is_some(),
        impact_speed: s.f32("global.impactspeed", d.impact_speed),
        wallrun_time: s.f32("global.wallrun_timelimit", d.wallrun_time),
        wallrun_max_h: s.f32("global.wallrunmaxspeedhorizontal", d.wallrun_max_h),
        wallrun_max_v: s.f32("global.wallrunmaxspeedvertical", d.wallrun_max_v),
        wallrun_accel_h: s.f32("global.wallrunacceleratehorizontal", d.wallrun_accel_h),
        wallrun_jump_out: s.f32("global.wallrunjumpoutwardspeed", d.wallrun_jump_out),
        wallrun_jump_up: s.f32("global.wallrunjumpupspeed", d.wallrun_jump_up),
        wallrun_jump_input: s.f32("global.wallrunjumpinputdirspeed", d.wallrun_jump_input),
        step_height: s.f32("global.stepheight", d.step_height),
        pitch_max_up: s.f32("global.pitchmaxup", d.pitch_max_up),
        pitch_max_down: s.f32("global.pitchmaxdown", d.pitch_max_down),
        eye_height: view("stand.viewheight", d.eye_height),
        crouch_eye_height: view("crouch.viewheight", d.crouch_eye_height),
        crouch_height: s.vec3("crouch.hull_max").map(|v| v[2]).unwrap_or(d.crouch_height),
        zipline: tf_sim::pilot::ZiplineParams {
            speed: s.f32("global.ziplinespeed", d.zipline.speed),
            accel: s.f32("global.ziplineacceleration", d.zipline.accel),
            jump_off: s.f32("global.ziplinejumpoffspeed", d.zipline.jump_off),
            mount_time: s.f32("global.mountziplinetime", d.zipline.mount_time),
            cooldown: s.f32("global.useziplinecooldown", d.zipline.cooldown),
            ..d.zipline.clone()
        },
        ..d
    }
}

/// Movement tuning from a Titan .set file (falls back to built-in defaults).
fn titan_params(gd: &GameData, path: &str) -> TitanParams {
    let mut read = |p: &str| gd.read_file(p).ok().map(|b| String::from_utf8_lossy(&b).to_string());
    let Some(s) = PlayerSettings::load(path, true, &mut read) else {
        log::warn!("{path} not found, using defaults");
        return TitanParams::default();
    };
    let d = TitanParams::default();
    let hull_max = s.vec3("stand.hull_max").unwrap_or([60.0, 60.0, 235.0]);
    TitanParams {
        speed: s.f32("stand.speed", d.speed),
        accel: s.f32("stand.acceleration", d.accel),
        decel: s.f32("stand.deceleration", d.decel),
        sprint_speed: s.f32("stand.sprintspeed", d.sprint_speed),
        sprint_accel: s.f32("stand.sprintacceleration", d.sprint_accel),
        sprint_decel: s.f32("stand.sprintdeceleration", d.sprint_decel),
        low_speed: s.f32("stand.lowspeed", d.low_speed),
        low_accel: s.f32("stand.lowacceleration", d.low_accel),
        side_scale: s.f32("global.speedscaleside", d.side_scale),
        back_scale: s.f32("global.speedscaleback", d.back_scale),
        dash_speed: s.f32("global.dodgespeed", d.dash_speed),
        dash_duration: s.f32("global.dodgeduration", d.dash_duration),
        dash_interval: s.f32("global.dodgeinterval", d.dash_interval),
        dash_stop_speed: s.f32("global.dodgestopspeed", d.dash_stop_speed),
        dash_height: s.f32("global.dodgeheight", d.dash_height),
        dash_drain: s.f32("global.dodgepowerdrain", d.dash_drain),
        power_regen: s.f32("global.powerregenrate", d.power_regen),
        power_delay: s.f32("global.dodgepowerdelay", d.power_delay),
        step_height: s.f32("global.stepheight", d.step_height),
        radius: hull_max[0],
        height: hull_max[2],
        eye_height: s.vec3("stand.viewheight").map(|v| v[2]).unwrap_or(d.eye_height),
        gravity: d.gravity,
    }
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    args: Res<Args>,
    mut input: ResMut<PlayerInput>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut blends: ResMut<Assets<convert::BlendMaterial>>,
) {
    let t = std::time::Instant::now();
    let mut gd = GameData::new(&args.game);
    // frontend holds resource/ files such as the closed captions.
    for v in [args.map.as_str(), "sp_beacon", "mp_common", "frontend"] {
        if let Err(e) = gd.add_vpk(v) {
            log::warn!("vpk {v}: {e:#}");
        }
    }
    for p in [format!("{}.rpak", args.map), "common.rpak".to_string()] {
        if let Err(e) = gd.add_rpak(&p) {
            log::warn!("rpak {p}: {e:#}");
        }
    }
    if std::env::var_os("TF_NO_PATCH_PAKS").is_none() {
        for stem in ["common", "common_mp"] {
            gd.add_rpak_patches(stem);
        }
    }
    let map_env = env::MapEnv::load(&gd, &args.map);
    commands.insert_resource(map_env.ambient());
    let params = titan_params(&gd, "scripts/players/mp/titan_buddy.set");
    let pilot = pilot_params(&gd);
    log::info!("pilot params: {pilot:?}");
    commands.insert_resource(pilotctl::PilotSettings(pilot));
    log::info!("titan params: {params:?}");
    let (bt_vitals, mut kits) = {
        let mut read = |p: &str| gd.read_file(p).ok().map(|b| String::from_utf8_lossy(&b).to_string());
        let buddy = PlayerSettings::load("scripts/players/mp/titan_buddy.set", true, &mut read).unwrap_or_default();
        let bt = vitals::Vitals::from_settings(&buddy, None);
        let mut kits: Vec<targets::EnemyKit> = (0..targets::ENEMY_CLASSES.len()).map(|i| targets::EnemyKit::class(i, &mut read)).collect();
        kits.push(targets::EnemyKit::infantry(&mut read));
        commands.insert_resource(executions::PilotMeleeDef::load(&mut read));
        commands.insert_resource(vmmotion::WeaponSprings::load(&mut read));
        let defs = pilotability::PilotAbilityDefs::load(&mut read);
        // TF_TACTICAL=N / TF_ORDNANCE=N pick the starting kit (scripted tests).
        let pick = |k: &str, n: usize| std::env::var(k).ok().and_then(|v| v.parse::<usize>().ok()).map_or(0, |i| i.min(n - 1));
        let mut status = pilotability::PilotStatus { tactical: pick("TF_TACTICAL", pilotability::TACTICALS.len()), ordnance: pick("TF_ORDNANCE", pilotability::ORDNANCE.len()), ..default() };
        status.refill(&defs);
        commands.insert_resource(status);
        commands.insert_resource(defs);
        (bt, kits)
    };
    log::info!("BT: {bt_vitals:?}");
    for kit in &mut kits {
        if !kit.infantry {
            kit.params = titan_params(&gd, &kit.settings_path);
        }
        log::info!(
            "enemy {}: {:?}, weapon {} ({} rps, bursts {}-{}, {} u/s)",
            kit.name, kit.vitals, kit.weapon.name, kit.weapon.fire_rate, kit.weapon.npc_min_burst, kit.weapon.npc_max_burst, kit.projectile
        );
    }
    commands.insert_resource(args.difficulty);
    let mut sfx = audio::Sfx::open(&args.game);
    let ts = std::time::Instant::now();
    let mut cues: Vec<audio::Cue> = audio::ALL_CUES.to_vec();
    cues.extend((0..pilotweapon::ARSENAL.len()).flat_map(|i| [audio::GunPart::First, audio::GunPart::Shot, audio::GunPart::Tail].map(|p| audio::Cue::Gun(i as u8, p))));
    sfx.preload(&cues);
    sfx.load_game_data(&gd);
    sfx.preload_events(kits.iter().filter_map(|k| k.ability.as_ref()).map(|a| a.sound.as_str()).filter(|s| !s.is_empty()));
    sfx.preload_code_events();
    log::info!("sounds decoded in {:?}", ts.elapsed());
    commands.insert_resource(sfx);
    if let Some(v) = args.volume {
        commands.insert_resource(crate::audio::MasterVolume::new(bevy::audio::Volume::Linear(v.clamp(0.0, 1.0))));
    }

    // Everything from the game lives under this root, in game units and axes.
    let root = commands
        .spawn((
            Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2)).with_scale(Vec3::splat(UNIT)),
            Visibility::default(),
        ))
        .id();

    let mut cache = convert::Cache::default();
    let mut collision = CollisionWorld::default();
    match gd.read_map(&args.map) {
        Ok(bsp) => {
            world::spawn_world(&mut commands, root, &gd, &bsp, &mut cache, &mut meshes, &mut images, &mut materials, &mut blends, &mut collision);
            if args.props {
                world::spawn_static_props(&mut commands, root, &gd, &bsp, &mut cache, &mut meshes, &mut images, &mut materials, &mut collision);
                sky::spawn_sky(&mut commands, root, &gd, &bsp, &map_env, &mut cache, &mut meshes, &mut images, &mut materials);
            }
            // Baked probes replace the flat ambient term for everything without a lightmap.
            if std::env::var_os("TF_NO_PROBES").is_none() && env::spawn_irradiance_volume(&mut commands, root, &bsp, &mut images) {
                commands.insert_resource(GlobalAmbientLight { brightness: 0.0, ..map_env.ambient() });
            }
            let reflections = env::Reflections::load(&bsp, &mut images);
            reflections.spawn_probes(&mut commands, root);
            commands.insert_resource(reflections);
        }
        Err(e) => log::error!("map {}: {e:#}", args.map),
    }
    let tb = std::time::Instant::now();
    collision.finish();
    log::info!("collision: {} triangles, BVH built in {:?}", collision.tris.len(), tb.elapsed());

    let (mut bt_pos, bt_yaw, placements) = match titan_spawn(&gd, &args.map) {
        Some((p, y)) => (p, y, Vec::new()),
        None => campaign_arena(&gd, &args.map).unwrap_or((Vec3::ZERO, 0.0, Vec::new())),
    };
    // Stand on the ground under the spawn (campaign placements can float a little).
    if let Some(h) = collision.raycast(tf_sim::glam::Vec3::new(bt_pos.x, bt_pos.y, bt_pos.z + 100.0), -tf_sim::glam::Vec3::Z, 3000.0) {
        bt_pos.z = h.point.z;
    }
    let yaw = bt_yaw.to_radians();
    input.yaw = yaw;

    // BT: the placement entity carries the simulation state; the actor hangs below it.
    let anchor = commands.spawn((Transform::from_translation(bt_pos), Visibility::default())).id();
    commands.entity(root).add_child(anchor);
    let bt = actor::spawn_actor(
        &mut commands,
        anchor,
        &gd,
        &ActorSpec {
            path: "models/titans/buddy/titan_buddy.mdl",
            sequences: &[
                "bt_combat_idle",
                "bt_combat_walk_forward",
                "bt_combat_walk_backward",
                "bt_combat_walk_left",
                "bt_combat_walk_right",
                "bt_combat_sprint_forward_noaim",
                "at_player_melee_punch_01",
                "bt_synced_titan_execute_flip_takedown_A",
                "bt_synced_titan_execute_kickshoot_A",
                "bt_synced_titan_execute_pilot_rip_A",
                "at_dismount_stand",
                "at_dismount_crouch",
                "at_MP_disembark_back2idle",
                "at_mount_stand_front",
                "at_mount_stand_behind",
                "at_mount_stand_left",
                "at_mount_stand_right",
                "at_MP_eject_stand_start",
                "at_hotdrop_drop_2knee_turbo",
                "at_hotdrop_quickstand",
            ],
            grids: &["combat_aim_stand", "combat_aim_run"],
            body: &[],
        },
        &mut cache,
        &mut meshes,
        &mut images,
        &mut materials,
        &mut bindposes,
    );

    // First-person cockpit, placed at the eye every frame.
    let cockpit_anchor = commands.spawn((Transform::IDENTITY, Visibility::Hidden, vmcam::Viewmodel)).id();
    commands.entity(root).add_child(cockpit_anchor);
    let cockpit = actor::spawn_actor(
        &mut commands,
        cockpit_anchor,
        &gd,
        &ActorSpec {
            path: "models/weapons/arms/pov_titan_medium_cockpit.mdl",
            sequences: &["atpov_cockpit_hatch_close_idle"],
            grids: &[],
            body: &[],
        },
        &mut cache,
        &mut meshes,
        &mut images,
        &mut materials,
        &mut bindposes,
    );
    if let Err(e) = &cockpit {
        log::warn!("cockpit: {e:#}");
    }

    // XO-16: script, first-person viewmodel with BT's arms merged onto it, gun in BT's hand.
    let mut read = |p: &str| gd.read_file(p).ok().map(|b| String::from_utf8_lossy(&b).to_string());
    let weapon_def = weapons::WeaponDef::load_player("mp_titanweapon_xo16_shorty", &mut read);
    log::info!("weapon: {weapon_def:?}");
    let vm_anchor = commands.spawn((Transform::IDENTITY, Visibility::Hidden, vmcam::Viewmodel)).id();
    commands.entity(root).add_child(vm_anchor);
    let viewmodel = actor::spawn_actor(
        &mut commands,
        vm_anchor,
        &gd,
        &ActorSpec {
            path: "models/weapons/titan_xo16_shorty/atpov_xo16shorty.mdl",
            sequences: &["draw_seq", "ads_in_seq", "reload_seq", "reload_empty_seq", "sprint_seq", "?sprintraise_seq", "melee_seq", "?melee_dash_seq"],
            grids: &["idle_seq", "attack_seq"],
            body: &[],
        },
        &mut cache,
        &mut meshes,
        &mut images,
        &mut materials,
        &mut bindposes,
    );
    match &viewmodel {
        Ok(vm) => {
            match actor::spawn_actor(
                &mut commands,
                vm_anchor,
                &gd,
                &ActorSpec { path: "models/weapons/arms/buddypov.mdl", sequences: &[], grids: &[], body: &[] },
                &mut cache,
                &mut meshes,
                &mut images,
                &mut materials,
                &mut bindposes,
            ) {
                Ok(arms) => {
                    commands.entity(arms.entity).insert(actor::BoneMergeTo(vm.entity));
                }
                Err(e) => log::warn!("arms: {e:#}"),
            }
        }
        Err(e) => log::warn!("viewmodel: {e:#}"),
    }
    let mut world_gun = None;
    if let Some(hand) = bt.as_ref().ok().and_then(|b| b.joint("ja_c_propGun")) {
        commands.insert_resource(weapons::BtHand(hand));
        match actor::spawn_actor(
            &mut commands,
            hand,
            &gd,
            &ActorSpec { path: "models/weapons/titan_xo16_shorty/w_xo16shorty.mdl", sequences: &[], grids: &[], body: &[] },
            &mut cache,
            &mut meshes,
            &mut images,
            &mut materials,
            &mut bindposes,
        ) {
            Ok(g) => world_gun = Some(g.entity),
            Err(e) => log::warn!("world gun: {e:#}"),
        }
    }

    match bt {
        Ok(actor) => {
            let state = TitanState::new(tf_sim::glam::Vec3::new(bt_pos.x, bt_pos.y, bt_pos.z), yaw);
            commands.entity(anchor).insert((
                PlayerTitan::new(state, actor.entity, cockpit.ok().map(|c| (cockpit_anchor, c.entity))),
                weapons::Weapon::new(weapon_def, viewmodel.ok().map(|v| (vm_anchor, v.entity)), world_gun),
                abilities::Ordnance::default(),
                abilities::TitanCore::default(),
                combat::TitanHealth::new(bt_vitals.clone()),
                combat::Vortex::default(),
                combat::Melee::default(),
                titankit::KitState::default(),
                pilotctl::Titanfall::default(),
            ));
        }
        Err(e) => log::error!("BT: {e:#}"),
    }

    // The pilot; the loadout's guns and viewmodels are built by pilotweapon::build_loadout.
    commands.spawn((pilotctl::PlayerPilot::new(), pilotctl::PilotHealth::default(), pilotweapon::PilotLoadout::default()));
    commands.insert_resource(pilotweapon::WorldRoot(root));

    // Enemy spawn spots: open ground on rings around the Titan spawn, near its height and
    // with nothing overhead.
    let sv = |v: Vec3| tf_sim::glam::Vec3::new(v.x, v.y, v.z);
    let down = -tf_sim::glam::Vec3::Z;
    let bt_ground = collision.raycast(sv(bt_pos + Vec3::Z * 50.0), down, 2000.0).map(|h| h.point.z).unwrap_or(bt_pos.z);
    // The game's navmesh where the map ships one (campaign maps); a grid probed from the
    // collision mesh around the arena otherwise (MP maps have no client navmesh).
    let nav_titan = nav::Nav::load(&gd, &args.map, "large").unwrap_or_else(|| nav::Nav::probe(&collision, bt_pos, 6000.0, nav::Agent::TITAN));
    let nav_small = nav::Nav::load(&gd, &args.map, "small").unwrap_or_else(|| nav::Nav::probe(&collision, bt_pos, 4000.0, nav::Agent::GRUNT));
    let mut spots: Vec<Vec3> = Vec::new();
    // The campaign's own Titan placements near the arena come first.
    for p in placements.iter().filter(|p| p.distance(bt_pos) > 1200.0 && p.distance(bt_pos) < 6000.0) {
        if let Some(hit) = collision.raycast(sv(*p + Vec3::Z * 100.0), down, 1000.0) {
            let q = Vec3::new(p.x, p.y, hit.point.z);
            if spots.iter().all(|s| s.distance(q) > 450.0) {
                spots.push(q);
            }
        }
    }
    for ring in [1600.0f32, 2200.0, 2800.0, 3400.0] {
        for k in 0..24 {
            let a = k as f32 / 24.0 * std::f32::consts::TAU;
            let guess = Vec3::new(bt_pos.x + a.cos() * ring, bt_pos.y + a.sin() * ring, bt_ground);
            let Some(hit) = collision.raycast(sv(guess + Vec3::Z * 400.0), down, 900.0) else { continue };
            let p = Vec3::new(guess.x, guess.y, hit.point.z);
            let open_above = collision.raycast(sv(p + Vec3::Z * 20.0), tf_sim::glam::Vec3::Z, 400.0).is_none();
            let clear = (0..8).all(|i| {
                let b = i as f32 / 8.0 * std::f32::consts::TAU;
                collision.raycast(sv(p + Vec3::Z * 120.0), tf_sim::glam::Vec3::new(b.cos(), b.sin(), 0.0), 150.0).is_none()
            });
            let spaced = spots.iter().all(|q| q.distance(p) > 450.0);
            if hit.normal.z > 0.8 && (p.z - bt_ground).abs() < 400.0 && open_above && clear && spaced {
                spots.push(p);
            }
        }
    }
    // Only spots a Titan can walk to the player from (a gantry or a roof the ring probes
    // landed on would leave it pacing up there).
    let reachable: Vec<Vec3> = spots.iter().copied().filter(|s| nav_titan.path(*s, bt_pos).is_some()).collect();
    if reachable.len() < spots.len() {
        log::info!("{} of {} enemy spawn spots have no walking route to the player, dropped", spots.len() - reachable.len(), spots.len());
    }
    if !reachable.is_empty() {
        spots = reachable;
    }
    if spots.is_empty() {
        // Nowhere passes the checks (tight interiors): spawn around the player anyway.
        spots = (0..8).map(|k| bt_pos + Vec3::new((k as f32 * 0.785).cos(), (k as f32 * 0.785).sin(), 0.0) * 1200.0).collect();
    }
    log::info!("{} enemy spawn spots", spots.len());
    commands.insert_resource(game::Arena { player_spawn: bt_pos, player_yaw: yaw, enemy_spots: spots });

    // A pool of enemy Titans, two of each class (hidden until a wave needs them), each holding
    // its class's weapon.
    let mut prefetch: Vec<String> = kits.iter().flat_map(|k| [k.model.clone(), k.weapon.playermodel.clone()]).collect();
    prefetch.sort();
    prefetch.dedup();
    actor::prefetch_models(&gd, &prefetch);
    // Then a squad of grunts (the last kit), each with an R-201.
    let titan_classes = kits.iter().filter(|k| !k.infantry).count();
    let grunt_class = kits.iter().position(|k| k.infantry);
    for i in 0..(titan_classes * 2 + grunt_class.map_or(0, |_| GRUNT_POOL)) as u64 {
        let class = if (i as usize) < titan_classes * 2 { i as usize % titan_classes } else { grunt_class.unwrap() };
        let kit = &kits[class];
        let c = targets::enemy_clips(&kit.model);
        // Locomotion, death, Titanfall, and the execution victim sequences.
        let mut seq_list: Vec<&str> = vec![c[0], c[1], c[2], c[3]];
        if kit.infantry {
            seq_list.extend(executions::human_victim_sequences());
        } else {
            seq_list.extend(["at_IDLE_2", "at_hotdrop_drop_2knee_turbo", "at_hotdrop_quickstand"]);
            if kit.model.contains("titan_medium") {
                seq_list.push("at_rodeo_ride_R_hijack_battery");
            }
            seq_list.extend(executions::titan_victim_sequences());
        }
        let seqs: &[&str] = &seq_list;
        let e_anchor = commands.spawn((Transform::from_translation(bt_pos), Visibility::Hidden)).id();
        commands.entity(root).add_child(e_anchor);
        match actor::spawn_actor(
            &mut commands,
            e_anchor,
            &gd,
            &ActorSpec {
                path: &kit.model,
                sequences: seqs,
                grids: &targets::aim_grids(&kit.model),
                body: &[],
            },
            &mut cache,
            &mut meshes,
            &mut images,
            &mut materials,
            &mut bindposes,
        ) {
            Ok(a) => {
                if let Some(hand) = a.joint("ja_c_propGun") {
                    let gun = ActorSpec { path: &kit.weapon.playermodel, sequences: &[], grids: &[], body: &[] };
                    if let Err(e) = actor::spawn_actor(&mut commands, hand, &gd, &gun, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes) {
                        log::warn!("enemy gun: {e:#}");
                    }
                }
                commands.entity(e_anchor).insert(targets::Enemy::new(bt_pos, a.entity, 0x9E37_79B9 * (i + 1), class, kit));
            }
            Err(e) => log::warn!("enemy titan: {e:#}"),
        }
    }
    if !cache.missing_materials.is_empty() {
        log::warn!("{} materials not found, e.g. {:?}", cache.missing_materials.len(), &cache.missing_materials[..cache.missing_materials.len().min(8)]);
    }

    match world::build_static_model(&gd, "models/weapons/bullets/projectile_rocket_large.mdl", &mut cache, &mut meshes, &mut images, &mut materials) {
        Ok((parts, _)) => commands.insert_resource(abilities::MissileAssets { parts, root }),
        Err(e) => log::warn!("missile model: {e:#}"),
    }
    match world::build_static_model(&gd, "models/weapons/caber_shot/caber_shot_thrown_xl.mdl", &mut cache, &mut meshes, &mut images, &mut materials) {
        Ok((parts, _)) => commands.insert_resource(titankit::TetherAssets { parts, root }),
        Err(e) => log::warn!("tether model: {e:#}"),
    }
    commands.insert_resource(Collision(collision, Some(nav_titan), Some(nav_small)));
    commands.insert_resource(zipline::Ziplines(zipline::load(&gd, &args.map)));
    commands.insert_resource(TitanSettings(params));
    commands.insert_resource(targets::EnemyKits(kits));

    // Camera: the player controller places it unless a free camera start was requested.
    let fwd = Quat::from_rotation_z(yaw) * Vec3::X;
    let cam = args.cam.unwrap_or(bt_pos + fwd * 650.0 + Vec3::new(0.0, 0.0, 220.0));
    let look = args.look.unwrap_or(bt_pos + Vec3::new(0.0, 0.0, 170.0));
    if args.cam.is_some() {
        commands.insert_resource(player::FreeCamStart(to_bevy(cam), to_bevy(look)));
    }
    let camera = commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: 60f32.to_radians(), ..default() }),
        Transform::from_translation(to_bevy(cam)).looking_at(to_bevy(look), Vec3::Y),
        FreeCamera { walk_speed: 8.0, run_speed: 40.0, ..default() },
        MainCamera,
        bevy::camera::Exposure { ev100: env::EV100 },
        // The sky camera (sky.rs) clears and draws the skybox first.
        Camera { clear_color: ClearColorConfig::None, ..default() },
        // Contact shading (SSAO needs MSAA off; SMAA smooths edges instead).
        Msaa::Off,
        bevy::pbr::ScreenSpaceAmbientOcclusion::default(),
        bevy::anti_alias::smaa::Smaa::default(),
        bevy::post_process::bloom::Bloom { intensity: 0.05, ..bevy::post_process::bloom::Bloom::NATURAL },
    )).id();
    if let Some(fog) = map_env.distance_fog() {
        commands.entity(camera).insert(fog);
    }
    // The sun also lights the first-person models (their own camera and layer, vmcam.rs).
    commands.spawn((map_env.sun(), bevy::camera::visibility::RenderLayers::from_layers(&[0, vmcam::VM_LAYER])));
    vmcam::spawn_camera(&mut commands);
    log::info!(
        "texture loads {:.2}s, spec/gloss {:.2}s",
        convert::TEX_US.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e6,
        convert::SPEC_US.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e6
    );
    log::info!("scene ready in {:?}", t.elapsed());
    // Keep the game data and caches for things loaded later (e.g. weapon viewmodels).
    commands.insert_resource(gd);
    commands.insert_resource(cache);
}

fn screenshots(
    mut commands: Commands,
    args: Res<Args>,
    loaded_at: Option<Res<ui::LoadedAt>>,
    load: Res<ui::LoadState>,
    mut next: Local<usize>,
    mut exit: MessageWriter<AppExit>,
) {
    if args.shots.is_empty() {
        return;
    }
    // Shot times count from the end of loading; a time of 0 is taken straight away.
    let t = loaded_at.map(|l| l.secs()).unwrap_or(-1.0);
    if let Some((at, path)) = args.shots.get(*next) {
        // A time of 0 is the loading screen, once it has had a few frames to appear.
        let loading_shot = *at <= 0.0 && matches!(*load, ui::LoadState::Waiting(n) if n >= 4);
        if (t > *at && *at > 0.0) || loading_shot {
            log::info!("screenshot {path} at {t:.1}s");
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
            *next += 1;
        }
    } else if t > args.shots.last().unwrap().0 + 1.5 {
        exit.write(AppExit::Success);
    }
}

/// Log the average frame time every two seconds (RUST_LOG=tf_viewer=info shows it).
fn perf_log(time: Res<Time>, mut acc: Local<(f32, u32)>) {
    acc.0 += time.delta_secs();
    acc.1 += 1;
    if acc.0 >= 2.0 {
        log::info!("perf: {:.1} fps ({:.1} ms/frame)", acc.1 as f32 / acc.0, acc.0 * 1000.0 / acc.1 as f32);
        *acc = (0.0, 0);
    }
}
