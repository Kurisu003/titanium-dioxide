//! Titanfall-rs: play as BT in a Titanfall 2 map, read straight from the game install.
//!
//! Usage: tf-viewer [--game DIR] [--map NAME] [--no-props]   (the default map is the built-in
//!                  flat movement-test grid; --map mp_forwardbase_kodai etc. for the game's maps)
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
mod aimassist;
mod audio;
mod autotitan;
mod bindcache;
mod cloak;
mod colorcorr;
mod combat;
mod convert;
mod crosshair;
mod diag;
mod dialogue;
mod env;
mod executions;
mod feed;
mod flatmap;
mod fparms;
mod fxuber;
mod game;
mod gamedata;
mod gamepad;
mod grenadearc;
mod hitbox;
mod hud;
mod inputrec;
mod kits;
mod launcherscreen;
mod minimap;
mod music;
mod nav;
mod net;
mod offhandvm;
mod particles;
mod pfx;
mod pilotability;
mod pilotbody;
mod pilotctl;
mod pilotweapon;
mod player;
mod propbatch;
mod rodeo;
mod ruiscreen;
mod settings;
mod shadowbatch;
mod sky;
mod smartammo;
mod spread;
mod spritebuf;
mod targets;
mod titankit;
mod transparentpass;
mod uber;
mod ui;
mod update;
mod vitals;
mod vmcam;
mod vmmotion;
mod weakpoints;
mod weapons;
mod world;
mod zipline;

use actor::ActorSpec;
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use gamedata::GameData;
use player::{
    to_bevy, CameraMode, Collision, MainCamera, PlayerInput, PlayerTitan, Script, TitanSettings,
    UNIT,
};
use std::f32::consts::FRAC_PI_2;
use tf_assets::settings::PlayerSettings;
use tf_sim::collision::CollisionWorld;
use tf_sim::titan::{TitanParams, TitanState};

#[derive(Resource, Clone)]
pub(crate) struct Args {
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
    /// Multiplayer from the command line: host on a port, or join an address (tests).
    host: Option<u16>,
    join: Option<String>,
    pvp: bool,
    /// BT's loadout at start (index into weapons::TITAN_ARSENAL).
    titan: usize,
}

/// TF_TIMESCALE=x runs the game at x times real speed (testing: slow motion for screenshots;
/// scripts and `--shots` stay on the wall clock).
fn time_scale(mut time: ResMut<Time<Virtual>>) {
    if let Some(x) = std::env::var("TF_TIMESCALE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|x| *x > 0.0)
    {
        time.set_relative_speed(x);
        log::info!("time scale {x}");
    }
}

/// The `timescale=x` script verb: game time runs at x times real speed while it's active
/// (slow motion from a chosen moment, so loading and weapon swaps aren't slowed too).
fn script_time_scale(
    script: Res<player::Script>,
    loaded_at: Option<Res<ui::LoadedAt>>,
    mut time: ResMut<Time<Virtual>>,
    mut base: Local<Option<f32>>,
) {
    let base = *base.get_or_insert(time.relative_speed());
    let t = loaded_at.map(|l| l.secs()).unwrap_or(0.0);
    let x = script
        .entries
        .iter()
        .filter(|(a, b, _)| script.enabled && t >= *a && t <= b.max(*a))
        .find_map(|(_, _, act)| {
            act.strip_prefix("timescale=")
                .and_then(|v| v.parse::<f32>().ok())
        })
        .filter(|x| *x > 0.0)
        .unwrap_or(base);
    if (time.relative_speed() - x).abs() > 1e-6 {
        time.set_relative_speed(x);
    }
}

fn parse_vec(s: &str) -> Option<Vec3> {
    let v: Vec<f32> = s
        .split_whitespace()
        .filter_map(|x| x.parse().ok())
        .collect();
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
        map: flatmap::NAME.into(),
        props: true,
        shots: Vec::new(),
        cam: None,
        look: None,
        script: None,
        difficulty: vitals::Difficulty::Normal,
        volume: None,
        loadout: None,
        // Tone's kit, matching the chassis every player gets.
        titan: weapons::TITAN_ARSENAL
            .iter()
            .position(|g| g.kit == "TONE")
            .unwrap_or(0),
        host: None,
        join: None,
        pvp: false,
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
                        a.shots
                            .push((t.trim().parse().unwrap_or(0.0), f.trim().to_string()));
                    }
                }
            }
            "--cam" => a.cam = it.next().and_then(|s| parse_vec(&s)),
            "--look" => a.look = it.next().and_then(|s| parse_vec(&s)),
            "--script" => a.script = it.next(),
            "--loadout" => a.loadout = it.next(),
            "--titan" => a.titan = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--host" => a.host = it.next().and_then(|v| v.parse().ok()),
            "--join" => a.join = it.next(),
            "--pvp" => a.pvp = true,
            "--volume" => a.volume = it.next().and_then(|v| v.parse().ok()),
            "--difficulty" => {
                a.difficulty = it
                    .next()
                    .and_then(|d| vitals::Difficulty::parse(&d))
                    .expect("--difficulty easy|normal|hard|master")
            }
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
    gamedata::set_multiplayer(!args.map.starts_with("sp_"));
    if !std::path::Path::new(&args.game).join("vpk").is_dir() {
        eprintln!(
            "Titanium Dioxide needs your own Titanfall 2 install, and none was found at {:?}.\n\
             Pass --game /path/to/Titanfall2 or set TITANFALL2_DIR.",
            args.game
        );
        std::process::exit(1);
    }
    let script = args
        .script
        .as_deref()
        .map(Script::parse)
        .unwrap_or_default();
    // First person in the cockpit, as the game always is (C toggles the third-person camera).
    let mode = if args.cam.is_some() {
        CameraMode::Free
    } else {
        CameraMode::Cockpit
    };
    let map_choice = ui::MapChoice(args.map.clone());
    // The flat test map is for movement: no enemy waves unless TF_NO_ENEMIES=0 asks for them.
    let no_enemies = match std::env::var("TF_NO_ENEMIES") {
        Ok(v) => v != "0",
        Err(_) => args.map == flatmap::NAME,
    };
    let user_settings = settings::Settings::load();
    // The kits picked on the LOADOUT page, before anything reads a weapon script.
    kits::set_fitted(kits::fitted_for(&user_settings, args.titan));
    let start_volume = args.volume.unwrap_or(user_settings.volume);
    let start_titan = args.titan;
    let mut loadout = pilotweapon::LoadoutChoice::default();
    if let Some(l) = &args.loadout {
        let ids: Vec<&str> = l.split(',').collect();
        for (slot, id) in [
            pilotweapon::Slot::Primary,
            pilotweapon::Slot::Sidearm,
            pilotweapon::Slot::AntiTitan,
        ]
        .into_iter()
        .zip(ids)
        {
            if let Some(i) = pilotweapon::arsenal_index(id.trim()) {
                loadout.set(slot, i);
            }
        }
    }
    let mut app = App::new();
    if ui::fixed_dt().is_some() {
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::ZERO,
        ))
        .add_systems(
            First,
            ui::fixed_clock
                .before(bevy::time::TimeSystems)
                .before(script_time_scale),
        );
    }
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Titanfall-rs".into(),
                    resolution: (1600u32, 900u32).into(),
                    // TF_NO_VSYNC=1 runs uncapped, for frame-rate measurements.
                    present_mode: if std::env::var_os("TF_NO_VSYNC").is_some() {
                        bevy::window::PresentMode::AutoNoVsync
                    } else {
                        default()
                    },
                    ..default()
                }),
                ..default()
            })
            .set(ImagePlugin {
                default_sampler: convert::sampler(),
            })
            .set(bevy::render::RenderPlugin {
                render_creation: bevy::render::settings::RenderCreation::Automatic(
                    bevy::render::settings::WgpuSettings {
                        // Materials without bindless binding arrays (lightmaps and light probes
                        // keep theirs: they don't need this feature). With them, every switch
                        // between material slabs (up to 2048 textures each) costs wgpu-core a
                        // walk over the whole slab when the pass is encoded: 4-6 ms per shadow
                        // cascade on Angel City. Per-material bind groups draw the same pixels.
                        // TF_BINDLESS=1 keeps them (A/B measurements).
                        disabled_features: std::env::var_os("TF_BINDLESS")
                            .is_none()
                            .then_some(bevy::render::settings::WgpuFeatures::BUFFER_BINDING_ARRAY),
                        ..default()
                    },
                ),
                ..default()
            }),
    )
    .add_plugins(FreeCameraPlugin)
    .add_plugins(ui::PipelineWatchPlugin)
    .add_plugins(audio::GameAudioPlugin)
    .add_plugins(convert::BlendMaterialPlugin)
    .add_plugins(propbatch::PropBatchPlugin)
    .add_plugins(convert::TransparentOrderPlugin)
    .add_plugins(pfx::SoftMaterialPlugin)
    .add_plugins(spritebuf::SpriteBufPlugin)
    .add_plugins(uber::UberPlugin)
    .add_plugins(fxuber::FxUberPlugin)
    .add_plugins(cloak::CloakPlugin)
    .add_plugins(colorcorr::ColorCorrectionPlugin)
    .add_plugins(minimap::MinimapPlugin)
    .init_resource::<feed::Feed>()
    .add_plugins(particles::ParticlePlugin)
    .add_plugins((
        GpuTimesPlugin,
        bindcache::MeshBindCachePlugin,
        shadowbatch::ShadowBatchPlugin,
        transparentpass::ParallelTransparentPlugin,
    ))
    .add_systems(
        Update,
        (
            perf_log,
            fps_overlay,
            speed_overlay,
            speed_graph,
            weapons::shimmer_shields,
        ),
    )
    .insert_resource(args)
    .insert_resource(script)
    .insert_resource(mode)
    .init_resource::<PlayerInput>()
    .insert_resource(crate::audio::MasterVolume::new(
        bevy::audio::Volume::Linear(start_volume.clamp(0.0, 1.0)),
    ))
    .insert_resource(user_settings)
    .init_resource::<gamepad::PadConfig>()
    .init_resource::<gamepad::PadState>()
    .init_resource::<aimassist::AimAssist>()
    .init_resource::<aimassist::PullClasses>()
    .init_resource::<ui::Subtitles>()
    .init_resource::<ui::MissionImages>()
    .add_systems(
        Update,
        (
            ui::sync_volume_setting,
            settings::save_settings,
            ui::update_subtitles,
        )
            .run_if(ui::loaded),
    )
    .add_systems(
        Update,
        (
            gamepad::load_pad_config,
            aimassist::load_pull_classes,
            hitbox::load_enemy_hitboxes,
        )
            .run_if(ui::world_ready),
    )
    .add_systems(
        Update,
        aimassist::aim_assist
            .run_if(ui::loaded)
            .before(player::gather_input),
    )
    .add_systems(
        Update,
        settings::apply_fov
            .after(player::update_camera)
            .after(pilotctl::pilot_camera)
            .run_if(ui::loaded),
    )
    .add_systems(Update, pilotctl::cockpit_boot.run_if(ui::loaded))
    .add_systems(First, ui::log_hitches)
    .add_systems(
        PostUpdate,
        vmcam::one_shadow_cascade
            .after(bevy::light::SimulationLightSystems::UpdateDirectionalLightCascades)
            .before(bevy::light::SimulationLightSystems::UpdateLightFrusta),
    )
    .add_systems(
        Update,
        (zipline::zipline_cables, zipline::zipline_sounds).run_if(ui::loaded),
    )
    .add_systems(
        Update,
        pilotability::grapple_cable
            .after(pilotctl::pilot_camera)
            .run_if(ui::loaded),
    )
    .add_systems(
        Update,
        pilotctl::bt_sequence_sounds
            .after(pilotctl::control_transitions)
            .run_if(ui::loaded),
    )
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
    .init_resource::<executions::BeingExecuted>()
    .init_resource::<executions::PilotExecution>()
    .init_resource::<executions::PilotMelee>()
    .init_resource::<pilotability::OrdnanceThrow>()
    .init_resource::<pilotability::Shields>()
    .init_resource::<game::Game>()
    .init_resource::<net::Net>()
    .init_resource::<update::Updater>()
    .insert_resource(pilotctl::Control::Titan)
    .insert_resource(pilotctl::PilotSettings(
        tf_sim::pilot::PilotParams::default(),
    ))
    .insert_resource(ClearColor(Color::srgb(0.55, 0.66, 0.78)))
    .insert_resource(ui::LoadState::Waiting(0))
    .init_resource::<ui::Menu>()
    .init_resource::<ui::Quality>()
    .insert_resource(loadout)
    .insert_resource(map_choice)
    .insert_resource(game::NoEnemies(no_enemies))
    .insert_resource(weapons::TitanKit(start_titan))
    .init_resource::<titankit::ActiveKit>()
    .init_resource::<offhandvm::Offhands>()
    .init_resource::<titankit::BoltShields>()
    .init_resource::<titankit::EnemyWalls>()
    .init_resource::<weapons::GunMuzzle>()
    .init_resource::<titankit::TitanBoltHits>()
    .init_resource::<titankit::FireSpots>()
    .init_resource::<titankit::KitView>()
    .init_resource::<titankit::TitanMoveMods>()
    .init_resource::<rodeo::Rodeo>()
    .init_resource::<rodeo::RodeoProps>()
    .init_resource::<autotitan::AutoTitan>()
    .init_resource::<fparms::FpSeq>()
    .init_resource::<hud::HudHidden>()
    .add_systems(
        Startup,
        (
            time_scale,
            weapons::setup_fx,
            ui::load_ui_assets,
            ui::spawn_loading_screen,
        )
            .chain(),
    )
    .add_systems(First, script_time_scale)
    .add_systems(
        Update,
        setup_world
            .run_if(ui::loading_world)
            .before(ui::loading_tick),
    )
    .add_systems(
        Update,
        (
            setup,
            hud::spawn_hud,
            pilotability::spawn_pilot_slots,
            pilotability::setup_pilot_kit,
            pilotbody::spawn_pilot_body,
            rodeo::spawn_battery_hud,
            fparms::spawn_arms,
        )
            .chain()
            .run_if(ui::loading)
            .before(ui::loading_tick),
    )
    // While warming up too: the reflection cubemap it copies changes the viewmodels' pipelines.
    .add_systems(
        Update,
        vmcam::follow_camera.run_if(ui::world_ready.and(not(ui::loaded))),
    )
    .add_systems(
        Update,
        (
            ui::loading_tick,
            ui::scale_ui,
            screenshots,
            env::nearest_reflection,
            uber::swap_materials,
            (vmcam::tag_meshes, vmcam::arms_world_layer).chain(),
        ),
    )
    .add_systems(
        Update,
        (
            ui::sync_menu,
            ui::menu_input,
            ui::menu_visuals,
            ui::mission_images,
            ui::apply_quality,
            pilotweapon::build_loadout,
            pilotweapon::ordnance_viewmodel,
            kits::sync_fitted,
            weapons::switch_titan_weapon,
            titankit::apply_kit,
            weapons::prepare_weapon_models,
            pilotability::preload_grenade_models,
            player::prepare_mirrored_clips,
            offhandvm::spawn_offhand_viewmodels,
        )
            .chain()
            .run_if(ui::world_ready)
            .before(player::gather_input),
    )
    .add_systems(
        Update,
        ruiscreen::weapon_screens
            .run_if(ui::loaded)
            .after(pilotweapon::pilot_weapon_fire),
    )
    .add_systems(
        Update,
        crosshair::aim_dot
            .run_if(ui::loaded)
            .after(pilotweapon::pilot_weapon_fire),
    )
    .add_systems(
        Update,
        launcherscreen::update
            .run_if(ui::loaded)
            .after(pilotweapon::pilot_weapon_fire),
    )
    .add_systems(
        Update,
        (
            (
                (
                    player::gather_input,
                    inputrec::replay_input,
                    inputrec::replay_skip,
                )
                    .chain(),
                (game::gate_input, inputrec::record_input).chain(),
                (
                    targets::script_aim,
                    net::auto_session,
                    net::local_flags,
                    net::update,
                    net::spawn_remotes,
                    net::remote_states,
                    game::director,
                    update::update,
                )
                    .chain(),
                (
                    zipline::zipline_grab,
                    pilotctl::control_transitions,
                    autotitan::auto_titan_think,
                )
                    .chain(),
                player::route_input,
                player::simulate,
                (
                    pilotctl::simulate_pilot,
                    rodeo::rodeo_update,
                    rodeo::rodeo_props,
                    fparms::fp_seq_update,
                )
                    .chain(),
                (
                    executions::titan_execution,
                    executions::pilot_execution,
                    executions::executed_victim,
                )
                    .chain(),
                weapons::weapon_fire,
                pilotweapon::pilot_weapon_fire,
                (
                    pilotweapon::update_pilot_bolts,
                    pilotweapon::attach_projectile_models,
                    grenadearc::grenade_arc,
                )
                    .chain(),
                abilities::update_abilities,
                abilities::update_missiles,
                titankit::kit_input,
                (targets::update_enemies, weakpoints::update_weakpoints).chain(),
                (
                    titankit::meteor_thermite,
                    titankit::kit_world,
                    titankit::utility_world,
                    titankit::laser_towers,
                    titankit::sonar_shots,
                )
                    .chain(),
                (targets::update_bolts, combat::update_vortex_caught_fx).chain(),
                targets::death_explosions,
                combat::player_combat,
            )
                .chain(),
            (
                player::drive_animation,
                (weapons::weapon_viewmodel, offhandvm::offhand_viewmodels).chain(),
                pilotweapon::pilot_viewmodel,
                (pilotbody::update_pilot_body, actor::animate_actors).chain(),
                actor::bone_merge,
                player::update_camera,
                (
                    pilotctl::pilot_camera,
                    rodeo::rodeo_camera,
                    fparms::fp_seq_camera,
                    executions::execution_camera,
                )
                    .chain(),
                (
                    sky::follow_camera,
                    vmcam::follow_camera,
                    pilotweapon::place_viewmodels,
                ),
                weapons::update_fx,
                targets::update_health_bars,
                titankit::kit_view,
                (
                    hud::update_hud,
                    crosshair::update,
                    hud::hud_sway,
                    hud::lost_health_flash,
                    hud::fit_icons,
                    minimap::update_minimap,
                    feed::update,
                    hud::hide_in_pvp,
                ),
                (
                    hud::update_hud_state,
                    hud::hud_boot,
                    (hud::health_hex, hud::stim_fx),
                    player::cockpit_damage_light,
                )
                    .chain(),
                hud::hit_feedback,
                hud::update_damage_arcs,
                (
                    hud::update_smart_hud,
                    hud::update_tone_hud,
                    hud::update_death_icons,
                ),
                (pilotability::pilot_abilities, pilotability::update_decoy).chain(),
                (pilotability::update_grenades, pilotability::grenade_visuals).chain(),
                (
                    pilotability::update_pilot_slots,
                    cloak::cloak_viewmodel,
                    cloak::cloak_remote,
                    cloak::phase_shift_fx,
                ),
                (pilotability::update_pulse_markers, rodeo::battery_hud).chain(),
            )
                .chain(),
        )
            .chain()
            .run_if(ui::loaded),
    )
    .add_systems(Last, diag::diag)
    .run();
}

/// An enemy's sequences: locomotion, death, Titanfall, and the execution victim sequences.
pub fn enemy_sequences(kit: &targets::EnemyKit) -> Vec<&'static str> {
    let c = targets::enemy_clips(&kit.model);
    let mut seq_list: Vec<&'static str> = vec![c[0], c[1], c[2], c[3]];
    if kit.infantry {
        seq_list.extend(executions::human_victim_sequences());
    } else {
        seq_list.extend([
            "at_IDLE_2",
            "at_hotdrop_drop_2knee_turbo",
            "at_hotdrop_quickstand",
        ]);
        if kit.model.contains("titan_medium") {
            seq_list.push("at_rodeo_ride_R_hijack_battery");
        }
        seq_list.extend(executions::titan_victim_sequences());
    }
    seq_list
}

/// How many grunts can be on the field at once.
const GRUNT_POOL: usize = 12;

/// The chassis every player's Titan uses (Tone; BT's buddy model lacks the rodeo panel and
/// battery, which multiplayer needs on other players' Titans).
pub const PLAYER_TITAN_MODEL: &str = "models/titans/medium/titan_medium_wraith.mdl";

/// Find a Titan start spawn point in the map's spawn entity file.
fn titan_spawn(gd: &GameData, map: &str) -> Option<(Vec3, f32)> {
    let text =
        String::from_utf8_lossy(&gd.read_file(&format!("maps/{map}_spawn.ent")).ok()?).to_string();
    let ents = tf_assets::bsp::parse_entities(&text);
    let get = |e: &Vec<(String, String)>, k: &str| {
        e.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.clone())
    };
    let e = ents
        .iter()
        .find(|e| get(e, "classname").as_deref() == Some("info_spawnpoint_titan_start"))?;
    let origin = parse_vec(&get(e, "origin")?)?;
    let yaw = parse_vec(&get(e, "angles").unwrap_or_default())
        .map(|a| a.y)
        .unwrap_or(0.0);
    Some((origin, yaw))
}

/// Campaign maps have no Titan start: put BT where the level's enemy Titans are placed
/// (`npc_titan` in `_script.ent`, minus BT-model friendlies), at the placement with the most
/// others within 5000 units, facing their centre. Returns that spot, its yaw and every enemy
/// placement. Maps without Titans fall back to `info_player_start`.
fn campaign_arena(gd: &GameData, map: &str) -> Option<(Vec3, f32, Vec<Vec3>)> {
    let text =
        String::from_utf8_lossy(&gd.read_file(&format!("maps/{map}_script.ent")).ok()?).to_string();
    let ents = tf_assets::bsp::parse_entities(&text);
    let get = |e: &Vec<(String, String)>, k: &str| {
        e.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.clone())
    };
    let titans: Vec<Vec3> = ents
        .iter()
        .filter(|e| get(e, "classname").is_some_and(|c| c.starts_with("npc_titan")))
        .filter(|e| !get(e, "model").unwrap_or_default().contains("buddy"))
        .filter_map(|e| parse_vec(&get(e, "origin")?))
        .collect();
    let near = |p: &Vec3| titans.iter().filter(|q| q.distance(*p) < 5000.0).count();
    if let Some(best) = titans.iter().max_by_key(|p| near(p)) {
        let group: Vec<Vec3> = titans
            .iter()
            .copied()
            .filter(|q| q.distance(*best) < 5000.0)
            .collect();
        let centre = group.iter().copied().sum::<Vec3>() / group.len() as f32;
        let to = centre - *best;
        let yaw = if to.truncate().length() > 1.0 {
            to.y.atan2(to.x).to_degrees()
        } else {
            0.0
        };
        return Some((*best, yaw, titans));
    }
    let start = ents
        .iter()
        .find(|e| get(e, "classname").as_deref() == Some("info_player_start"))?;
    let origin = parse_vec(&get(start, "origin")?)?;
    let yaw = parse_vec(&get(start, "angles").unwrap_or_default())
        .map(|a| a.y)
        .unwrap_or(0.0);
    Some((origin, yaw, Vec::new()))
}

/// Pilot movement from the multiplayer pilot's .set file (pilot_mp.set over pilot_base.set: every
/// MP Pilot class is pilot_mp.set plus models and footsteps), as in a Northstar match.
pub(crate) fn pilot_params(gd: &GameData) -> tf_sim::pilot::PilotParams {
    let mut read = |p: &str| {
        gd.read_file(p)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).to_string())
    };
    let d = tf_sim::pilot::PilotParams::default();
    let Some(s) = PlayerSettings::load("scripts/players/mp/pilot_mp.set", true, &mut read) else {
        return d;
    };
    // The Pilot's kits (pas_ads_hover, pas_wallhang, pas_power_cell: its ClassMods).
    let s = kits::apply(s);
    let view = |k: &str, def: f32| s.vec3(k).map(|v| v[2]).unwrap_or(def);
    let ground_accel = s.f32("stand.acceleration", d.ground_accel);
    tf_sim::pilot::PilotParams {
        walk_speed: s.f32("stand.speed", d.walk_speed),
        sprint_speed: s.f32("stand.sprintspeed", d.sprint_speed),
        crouch_speed: s.f32("crouch.speed", d.crouch_speed),
        sprint_start_delay: s.f32("global.sprintstartdelay", d.sprint_start_delay),
        sprint_start: s.f32("global.sprintstartduration", d.sprint_start),
        sprint_start_fast: s.f32("global.sprintstartfastduration", d.sprint_start_fast),
        sprint_end: s.f32("global.sprintendduration", d.sprint_end),
        sprint_view_offset: s.f32("global.sprintviewoffset", d.sprint_view_offset),
        pitch_offset_scale: s.f32("global.pitchoffsetscale", d.pitch_offset_scale),
        ground_accel,
        // deceleration -1 (the default): acceleration * 0.6 (GetDeceleration 0x1801fcf10).
        ground_decel: Some(s.f32("stand.deceleration", -1.0))
            .filter(|v| *v >= 0.0)
            .unwrap_or(ground_accel * 0.6),
        air_speed: s.f32("global.airspeed", d.air_speed),
        air_accel: s.f32("global.airacceleration", d.air_accel),
        jump_height: s.f32("global.jumpheight", d.jump_height),
        double_jump_height: s.f32("global.superjumpmaxheight", d.double_jump_height),
        double_jump_horz: s.f32("global.superjumphorzspeed", d.double_jump_horz),
        gravity: d.jump_gravity * s.f32("global.gravityscale", d.gravity / d.jump_gravity),
        slide_boost: s.f32("global.slidespeedboost", d.slide_boost),
        slide_min_speed: s.f32("global.sliderequiredstartspeed", d.slide_min_speed),
        slide_decel: s.f32("global.slidedecel", d.slide_decel),
        slide_velocity_decay: s.f32("global.slidevelocitydecay", d.slide_velocity_decay),
        slide_boost_cap: s.f32("global.slidespeedboostcap", d.slide_boost_cap),
        slide_stop_speed: s.f32("global.slidestopspeed", d.slide_stop_speed),
        slide_max_stop_speed: s.f32("global.slidemaxstopspeed", d.slide_max_stop_speed),
        slide_want_stop_decel: s.f32("global.slidewanttostopdecel", d.slide_want_stop_decel),
        slide_accel: s.f32("global.slideaccel", d.slide_accel),
        slide_max_jump_speed: s.f32("global.slidemaxjumpspeed", d.slide_max_jump_speed),
        slide_jump_height: s.f32("global.slidejumpheight", d.slide_jump_height),
        wallrun_accel_v: s.f32("global.wallrunacceleratevertical", d.wallrun_accel_v),
        wallrun_hang_time: s.f32("global.wallrun_hangtimelimit", d.wallrun_hang_time),
        // TF_WALLHANG=1: the MP wall-hang kit (pas_wallhang: wallrunAdsType "wallhang").
        wallhang_on_ads: s
            .get("global.wallrunadstype")
            .is_some_and(|v| v.eq_ignore_ascii_case("wallhang"))
            || std::env::var_os("TF_WALLHANG").is_some(),
        impact_speed: s.f32("global.impactspeed", d.impact_speed),
        viewkick_fall_min: s.f32("global.viewkickfalldistmin", d.viewkick_fall_min),
        viewkick_fall_max: s.f32("global.viewkickfalldistmax", d.viewkick_fall_max),
        viewkick_fall_max_scale: s
            .f32("global.viewkickfalldistmaxscale", d.viewkick_fall_max_scale),
        wallrun_time: s.f32("global.wallrun_timelimit", d.wallrun_time),
        wallrun_max_h: s.f32("global.wallrunmaxspeedhorizontal", d.wallrun_max_h),
        wallrun_max_v: s.f32("global.wallrunmaxspeedvertical", d.wallrun_max_v),
        wallrun_accel_h: s.f32("global.wallrunacceleratehorizontal", d.wallrun_accel_h),
        wallrun_jump_out: s.f32("global.wallrunjumpoutwardspeed", d.wallrun_jump_out),
        wallrun_jump_up: s.f32("global.wallrunjumpupspeed", d.wallrun_jump_up),
        wallrun_jump_input: s.f32("global.wallrunjumpinputdirspeed", d.wallrun_jump_input),
        wallrun: tf_sim::pilot::WallrunParams {
            friction: s.f32("global.wallrunfriction", d.wallrun.friction),
            same_wall_allowed: s.f32("global.wallrunsamewallallowed", 1.0) != 0.0,
            ..d.wallrun.clone()
        },
        step_height: s.f32("global.stepheight", d.step_height),
        pitch_max_up: s.f32("global.pitchmaxup", d.pitch_max_up),
        pitch_max_down: s.f32("global.pitchmaxdown", d.pitch_max_down),
        eye_height: view("stand.viewheight", d.eye_height),
        crouch_eye_height: view("crouch.viewheight", d.crouch_eye_height),
        crouch_height: s
            .vec3("crouch.hull_max")
            .map(|v| v[2])
            .unwrap_or(d.crouch_height),
        zipline: tf_sim::pilot::ZiplineParams {
            speed: s.f32("global.ziplinespeed", d.zipline.speed),
            accel: s.f32("global.ziplineacceleration", d.zipline.accel),
            jump_off: s.f32("global.ziplinejumpoffspeed", d.zipline.jump_off),
            mount_time: s.f32("global.mountziplinetime", d.zipline.mount_time),
            cooldown: s.f32("global.useziplinecooldown", d.zipline.cooldown),
            ..d.zipline.clone()
        },
        // The grapple's .set keys (pilot_base.set global); the rest are client.dll convars.
        grapple: tf_sim::pilot::GrappleParams {
            detach_length: (
                s.f32("global.grapple_detachlengthmin", d.grapple.detach_length.0),
                s.f32("global.grapple_detachlengthmax", d.grapple.detach_length.1),
            ),
            detach_away_speed: s.f32(
                "global.grapple_detachawayspeed",
                d.grapple.detach_away_speed,
            ),
            gravity_frac: (
                s.f32("global.grapple_gravityfracmin", d.grapple.gravity_frac.0),
                s.f32("global.grapple_gravityfracmax", d.grapple.gravity_frac.1),
            ),
            detach_boost: s.f32("global.grapple_detachverticalboost", d.grapple.detach_boost),
            detach_max: s.f32(
                "global.grapple_detachverticalmaxspeed",
                d.grapple.detach_max,
            ),
            speed_loss: s.f32("global.grapple_detachspeedloss", d.grapple.speed_loss),
            speed_loss_min: s.f32(
                "global.grapple_detachspeedlossmin",
                d.grapple.speed_loss_min,
            ),
            low_speed: s.f32(
                "global.grapple_detachlowspeedthreshold",
                d.grapple.low_speed,
            ),
            low_speed_time: s.f32(
                "global.grapple_detachlowspeedtime",
                d.grapple.low_speed_time,
            ),
            low_speed_wall_time: s.f32(
                "global.grapple_detachlowspeedwalltime",
                d.grapple.low_speed_wall_time,
            ),
            low_speed_ground_time: s.f32(
                "global.grapple_detachlowspeedgroundtime",
                d.grapple.low_speed_ground_time,
            ),
            impact_boost: s.f32("global.grapple_impactverticalboost", d.grapple.impact_boost),
            impact_max: s.f32(
                "global.grapple_impactverticalmaxspeed",
                d.grapple.impact_max,
            ),
            air_speed: s.f32("global.grapple_airspeedmax", d.grapple.air_speed),
            air_accel: s.f32("global.grapple_airaccel", d.grapple.air_accel),
            ..d.grapple.clone()
        },
        ..d
    }
}

/// Movement tuning from a Titan .set file (falls back to built-in defaults).
/// The player's Titan settings: BT's own in the campaign, else the multiplayer chassis file of
/// the loadout's Titan (titan_<chassis>_<weapon>.set over titan_atlas/stryder/ogre.set: speed,
/// dash, dash power and health per chassis), read with the [$mp] values.
pub(crate) fn player_titan_settings(map: &str, titan: usize) -> (&'static str, bool) {
    if map.starts_with("sp_") {
        return ("scripts/players/mp/titan_buddy.set", true);
    }
    let kit = weapons::TITAN_ARSENAL
        .get(titan)
        .map(|g| g.kit)
        .unwrap_or("TONE");
    let path = match kit {
        "EXPEDITION" => "scripts/players/mp/titan_atlas_vanguard.set",
        "ION" => "scripts/players/mp/titan_atlas_stickybomb.set",
        "TONE" => "scripts/players/mp/titan_atlas_tracker.set",
        "SCORCH" => "scripts/players/mp/titan_ogre_meteor.set",
        "LEGION" => "scripts/players/mp/titan_ogre_minigun.set",
        "NORTHSTAR" => "scripts/players/mp/titan_stryder_sniper.set",
        "RONIN" => "scripts/players/mp/titan_stryder_leadwall.set",
        "BRUTE" => "scripts/players/mp/titan_stryder_rocketeer.set",
        _ => "scripts/players/mp/titan_atlas.set",
    };
    (path, false)
}

fn titan_params(gd: &GameData, path: &str) -> TitanParams {
    titan_params_for(gd, path, true, false)
}

/// `kits`: the player's Titan, with the fitted kits' ClassMods (Turbo Engine's dash).
pub(crate) fn titan_params_for(gd: &GameData, path: &str, sp: bool, kits: bool) -> TitanParams {
    let mut read = |p: &str| {
        gd.read_file(p)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).to_string())
    };
    let Some(s) = PlayerSettings::load(path, sp, &mut read) else {
        log::warn!("{path} not found, using defaults");
        return TitanParams::default();
    };
    let s = if kits { kits::apply(s) } else { s };
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
        dash_height_max: s.f32("global.dodgeheightmax", d.dash_height_max),
        dash_keep_frac: s.f32("global.dodgekeepspeedfrac", d.dash_keep_frac),
        air_speed: s.f32("global.airspeed", d.air_speed),
        air_accel: s.f32("global.airacceleration", d.air_accel),
        dash_drain: s.f32("global.dodgepowerdrain", d.dash_drain),
        power_regen: s.f32("global.powerregenrate", d.power_regen),
        power_delay: s.f32("global.dodgepowerdelay", d.power_delay),
        step_height: s.f32("global.stepheight", d.step_height),
        radius: hull_max[0],
        height: hull_max[2],
        eye_height: s
            .vec3("stand.viewheight")
            .map(|v| v[2])
            .unwrap_or(d.eye_height),
        gravity: d.gravity,
        crouch_speed: s.f32("crouch.speed", d.crouch_speed),
        crouch_accel: s.f32("crouch.acceleration", d.crouch_accel),
        crouch_decel: s.f32("crouch.deceleration", d.crouch_decel),
        crouch_height: s
            .vec3("crouch.hull_max")
            .map(|v| v[2])
            .unwrap_or(d.crouch_height),
        crouch_eye_height: s
            .vec3("crouch.viewheight")
            .map(|v| v[2])
            .unwrap_or(d.crouch_eye_height),
    }
}

/// Load profiling: each `mark` logs the time since the previous one.
struct Phases(std::time::Instant);

impl Phases {
    fn mark(&mut self, what: &str) {
        log::info!("load phase: {what} {:?}", self.0.elapsed());
        self.0 = std::time::Instant::now();
    }
}

/// What the first load frame (`setup_world`) hands to the second (`setup`).
struct WorldStage {
    t: std::time::Instant,
    ph: Phases,
    gd: GameData,
    cache: convert::Cache,
    /// The collision hierarchy, building on its own thread.
    collision: std::thread::JoinHandle<(CollisionWorld, std::time::Duration)>,
    sweep: Option<CollisionWorld>,
    kits: Vec<targets::EnemyKit>,
    bt_vitals: vitals::Vitals,
    params: tf_sim::titan::TitanParams,
    sounds: std::thread::JoinHandle<audio::Sfx>,
    root: Entity,
    camera: Entity,
    spawn: (Vec3, f32, Vec<Vec3>),
}

#[derive(Resource)]
struct PendingWorld(Option<Box<WorldStage>>);

/// The first load frame: game data, the map's world, props, sky and probes, and the cameras.
/// Loading takes two frames so the render world uploads the map's meshes and textures and
/// compiles their pipelines while the second frame (`setup`) builds the collision hierarchy,
/// the actors and the nav grids: in one frame, all of that waited until loading was done.
#[allow(clippy::too_many_arguments)]
fn setup_world(
    mut commands: Commands,
    args: Res<Args>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut blends: ResMut<Assets<convert::BlendMaterial>>,
    (mut batch_materials, mut buffers): (
        ResMut<Assets<propbatch::PropBatchMaterial>>,
        ResMut<Assets<bevy::render::storage::ShaderStorageBuffer>>,
    ),
) {
    let t = std::time::Instant::now();
    let mut ph = Phases(t);
    let mut gd = GameData::new(&args.game);
    // frontend holds resource/ files such as the closed captions.
    // The built-in flat test map has no files of its own.
    let flat = args.map == flatmap::NAME;
    for v in [args.map.as_str(), "sp_beacon", "mp_common", "frontend"] {
        if v == flatmap::NAME {
            continue;
        }
        if let Err(e) = gd.add_vpk(v) {
            log::warn!("vpk {v}: {e:#}");
        }
    }
    let patch_stems: &[&str] = if std::env::var_os("TF_NO_PATCH_PAKS").is_none() {
        &["common", "common_mp"]
    } else {
        &[]
    };
    let mut paks = vec!["common.rpak".to_string()];
    if !flat {
        paks.insert(0, format!("{}.rpak", args.map));
    }
    gd.add_rpaks(&paks, patch_stems);
    ph.mark("vpks and rpaks");
    let map_env = env::MapEnv::load(&gd, &args.map);
    commands.insert_resource(map_env.ambient());
    let map_lut = map_env
        .color_correction
        .as_deref()
        .and_then(|f| match gd.read_file(f) {
            Ok(raw) => colorcorr::lut_image(&raw).map(|img| images.add(img)),
            Err(e) => {
                log::warn!("map colour correction {f}: {e:#}");
                None
            }
        });
    commands.insert_resource(colorcorr::MapColorCorrection(map_lut));
    let (titan_set, titan_sp) = player_titan_settings(&args.map, args.titan);
    log::info!("player Titan settings: {titan_set}");
    let params = titan_params_for(&gd, titan_set, titan_sp, true);
    let pilot = pilot_params(&gd);
    log::info!("pilot params: {pilot:?}");
    commands.insert_resource(pilotctl::PilotSettings(pilot));
    log::info!("titan params: {params:?}");
    let (bt_vitals, mut kits) = {
        let mut read = |p: &str| {
            gd.read_file(p)
                .ok()
                .map(|b| String::from_utf8_lossy(&b).to_string())
        };
        let buddy = PlayerSettings::load("scripts/players/mp/titan_buddy.set", true, &mut read)
            .unwrap_or_default();
        // Health, shield and doomed health of the player's chassis.
        let chassis = PlayerSettings::load(titan_set, titan_sp, &mut read).unwrap_or_default();
        let bt = vitals::Vitals::from_settings(&chassis, None);
        combat::set_titan_layout(
            chassis.f32("global.healthpersegment", 1800.0),
            bt.max_health,
            params.dash_drain,
        );
        if let (Some(f), Some(g)) = (
            chassis.get("global.hud_follow_icon"),
            chassis.get("global.hud_guard_icon"),
        ) {
            combat::set_pet_icons(f, g);
        }
        let feel = player::TitanFeel::from_settings(&buddy);
        log::info!("BT feel: {feel:?}");
        commands.insert_resource(feel);
        commands.init_resource::<player::TitanView>();
        let mut kits: Vec<targets::EnemyKit> = (0..targets::ENEMY_CLASSES.len())
            .map(|i| targets::EnemyKit::class(i, &mut read))
            .collect();
        kits.push(targets::EnemyKit::infantry(&mut read));
        kits.extend(net::remote_kits(&mut read));
        commands.insert_resource(executions::PilotMeleeDef::load(&mut read));
        commands.insert_resource(vmmotion::WeaponSprings::load(&mut read));
        let defs = pilotability::PilotAbilityDefs::load(&mut read);
        // TF_TACTICAL=N / TF_ORDNANCE=N pick the starting kit (scripted tests).
        let pick = |k: &str, n: usize| {
            std::env::var(k)
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .map_or(0, |i| i.min(n - 1))
        };
        let mut status = pilotability::PilotStatus {
            tactical: pick("TF_TACTICAL", pilotability::TACTICALS.len()),
            ordnance: pick("TF_ORDNANCE", pilotability::ORDNANCE.len()),
            ..default()
        };
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
            kit.name,
            kit.vitals,
            kit.weapon.name,
            kit.weapon.fire_rate,
            kit.weapon.npc_min_burst,
            kit.weapon.npc_max_burst,
            kit.projectile
        );
    }
    commands.insert_resource(args.difficulty);
    ph.mark("settings and kits");
    // The sounds decode on their own thread, with their own VPK handles, while the map loads
    // (joined at the end of setup).
    let sounds = {
        let (game, sgd) = (args.game.clone(), gd.vpks_only());
        let ability_sounds: Vec<String> = kits
            .iter()
            .filter_map(|k| k.ability.as_ref())
            .map(|a| a.sound.clone())
            .filter(|s| !s.is_empty())
            .collect();
        std::thread::spawn(move || {
            let mut sfx = audio::Sfx::open(&game);
            let ts = std::time::Instant::now();
            let mut cues: Vec<audio::Cue> = audio::ALL_CUES.to_vec();
            cues.extend((0..pilotweapon::ARSENAL.len()).flat_map(|i| {
                [
                    audio::GunPart::First,
                    audio::GunPart::Shot,
                    audio::GunPart::Tail,
                ]
                .map(|p| audio::Cue::Gun(i as u8, p))
            }));
            sfx.preload(&cues);
            sfx.load_game_data(&sgd);
            sfx.preload_events(ability_sounds.iter().map(String::as_str));
            sfx.preload_code_events();
            log::info!("sounds decoded in {:?}", ts.elapsed());
            sfx
        })
    };
    if let Some(v) = args.volume {
        commands.insert_resource(crate::audio::MasterVolume::new(
            bevy::audio::Volume::Linear(v.clamp(0.0, 1.0)),
        ));
    }

    // Everything from the game lives under this root, in game units and axes.
    let root = commands
        .spawn((
            Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2))
                .with_scale(Vec3::splat(UNIT)),
            Visibility::default(),
        ))
        .id();
    // The map's static geometry (world meshes, props, probes) hangs under its own root with the
    // same transform. Bevy skips a hierarchy nothing in has moved, but recomputes every direct
    // child of a root whose tree changed: under `root`, with BT and everything that moves,
    // that was ~47k prop and world entities every frame (2-4 ms on one thread).
    let static_root_tf =
        Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2)).with_scale(Vec3::splat(UNIT));
    let static_root = commands.spawn((static_root_tf, Visibility::default())).id();

    let mut cache = convert::Cache::default();
    let mut collision = CollisionWorld::default();
    let mut sweep: Option<CollisionWorld> = None;
    match if flat {
        Err(anyhow::anyhow!("built-in"))
    } else {
        gd.read_map(&args.map)
    } {
        Err(_) if flat => {
            flatmap::spawn(
                &mut commands,
                static_root,
                &mut meshes,
                &mut images,
                &mut materials,
                &mut collision,
            );
            sky::spawn_sky_camera(&mut commands);
            ph.mark("flat test map");
        }
        Ok(bsp) => {
            ph.mark("bsp read");
            world::spawn_world(
                &mut commands,
                static_root,
                &gd,
                &bsp,
                &mut cache,
                &mut meshes,
                &mut images,
                &mut materials,
                &mut blends,
                &mut collision,
            );
            if let Some(m) = minimap::Minimap::load(&gd, &args.map, &mut cache, &mut images) {
                commands.insert_resource(m);
            }
            if std::env::var_os("TF_NO_MAP_COLLISION").is_none() {
                world::add_game_collision(&bsp, &mut collision);
            }
            // TF_COLLISION_SWEEP=1 (with TF_NO_MAP_COLLISION=1): where would the map's own
            // collision catch a fall the render meshes let through? Logged after the build.
            if std::env::var_os("TF_COLLISION_SWEEP").is_some() {
                let mut extra = CollisionWorld::default();
                world::add_game_collision(&bsp, &mut extra);
                sweep = Some(extra);
            }
            ph.mark("world and map collision");
            if args.props {
                world::spawn_static_props(
                    &mut commands,
                    static_root,
                    &static_root_tf,
                    &gd,
                    &bsp,
                    &mut cache,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                    (&mut batch_materials, &mut buffers),
                    &mut collision,
                );
                sky::spawn_sky(
                    &mut commands,
                    root,
                    &gd,
                    &bsp,
                    &map_env,
                    &mut cache,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                );
            }
            ph.mark("props and sky");
            // Baked probes replace the flat ambient term for everything without a lightmap.
            if std::env::var_os("TF_NO_PROBES").is_none()
                && env::spawn_irradiance_volume(&mut commands, static_root, &bsp, &mut images)
            {
                commands.insert_resource(GlobalAmbientLight {
                    brightness: 0.0,
                    ..map_env.ambient()
                });
            }
            let reflections = env::Reflections::load(&bsp, &mut images);
            reflections.spawn_probes(&mut commands, static_root);
            commands.insert_resource(reflections);
            commands.insert_resource(pfx::ParticleLight::load(&bsp));
        }
        Err(e) => log::error!("map {}: {e:#}", args.map),
    }
    ph.mark("probes, reflections, particle light");
    // The collision hierarchy builds on its own thread from here, through the end of this
    // frame, into the next.
    let collision = std::thread::spawn(move || {
        let tb = std::time::Instant::now();
        collision.finish();
        let took = tb.elapsed();
        (collision, took)
    });
    let (bt_pos, bt_yaw, placements) = match titan_spawn(&gd, &args.map) {
        Some((p, y)) => (p, y, Vec::new()),
        None => campaign_arena(&gd, &args.map).unwrap_or((Vec3::ZERO, 0.0, Vec::new())),
    };
    // The cameras and the sun exist from this frame on, so the render world prepares the map
    // for them while the next frame runs. `setup` moves the camera to its final place (BT's
    // spawn on the ground).
    let camera = commands
        .spawn((
            Camera3d::default(),
            Projection::Perspective(PerspectiveProjection {
                fov: 60f32.to_radians(),
                ..default()
            }),
            camera_start(&args, bt_pos, bt_yaw.to_radians()),
            FreeCamera {
                walk_speed: 8.0,
                run_speed: 40.0,
                ..default()
            },
            MainCamera,
            bevy::camera::Exposure {
                ev100: env::ev100(),
            },
            env::tonemapping(),
            // The sky camera (sky.rs) clears and draws the skybox first.
            // The viewmodel camera, drawing last into the same textures, copies the frame to the
            // window (sky.rs).
            Camera {
                clear_color: ClearColorConfig::None,
                output_mode: bevy::camera::CameraOutputMode::Skip,
                ..default()
            },
            // Contact shading (SSAO needs MSAA off; SMAA smooths edges instead).
            Msaa::Off,
            bevy::pbr::ScreenSpaceAmbientOcclusion::default(),
            bevy::anti_alias::smaa::Smaa::default(),
            bevy::post_process::bloom::Bloom {
                intensity: 0.05,
                ..bevy::post_process::bloom::Bloom::NATURAL
            },
        ))
        .id();
    if let Some(fog) = map_env.distance_fog() {
        commands.entity(camera).insert(fog);
    }
    // The sun also lights the first-person models (their own camera and layer, vmcam.rs).
    commands.spawn((
        map_env.sun(),
        bevy::camera::visibility::RenderLayers::from_layers(&[0, vmcam::VM_LAYER]),
    ));
    vmcam::spawn_camera(&mut commands);
    ph.mark("cameras");
    commands.insert_resource(PendingWorld(Some(Box::new(WorldStage {
        t,
        ph,
        gd,
        cache,
        collision,
        sweep,
        kits,
        bt_vitals,
        params,
        sounds,
        root,
        camera,
        spawn: (bt_pos, bt_yaw, placements),
    }))));
}

/// Where the world camera starts: the free camera start if one was asked for, else behind
/// and above BT.
fn camera_start(args: &Args, bt_pos: Vec3, yaw: f32) -> Transform {
    let fwd = Quat::from_rotation_z(yaw) * Vec3::X;
    let cam = args
        .cam
        .unwrap_or(bt_pos + fwd * 650.0 + Vec3::new(0.0, 0.0, 220.0));
    let look = args.look.unwrap_or(bt_pos + Vec3::new(0.0, 0.0, 170.0));
    Transform::from_translation(to_bevy(cam)).looking_at(to_bevy(look), Vec3::Y)
}

/// The second load frame: the collision hierarchy, BT, the viewmodels, nav grids, enemies.
#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    args: Res<Args>,
    mut input: ResMut<PlayerInput>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut pending: ResMut<PendingWorld>,
) {
    let Some(stage) = pending.0.take() else {
        return;
    };
    commands.remove_resource::<PendingWorld>();
    let WorldStage {
        t,
        mut ph,
        gd,
        mut cache,
        collision,
        sweep,
        kits,
        bt_vitals,
        params,
        sounds,
        root,
        camera,
        spawn,
    } = *stage;
    ph.mark("between the load frames");
    // The actors' templates (models, clips, meshes, textures) build on every core while the
    // collision hierarchy does; spawning them below only adds their assets.
    // The player's Titan: Tone's chassis (titan_medium_wraith). The bt_ names are BT's own
    // set, used when the buddy model is swapped back in; the MP set covers Tone.
    let bt_spec = ActorSpec {
        path: PLAYER_TITAN_MODEL,
        sequences: &[
            "CQB_Idle_MP",
            "Walk_forward_MP",
            "Walk_backward_MP",
            "Walk_aim_all_left",
            "Walk_aim_all_right",
            "Sprint_mp_forward",
            "mp_vortex_add",
            "mp_vortex_ion_add",
            "at_rodeo_ride_R_hijack_battery",
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
            // Executed by another player (executions::executed_victim).
            "titan_synced_bt_execute_flip_takedown_V",
            "titan_synced_bt_execute_kickshoot_V",
            "titan_synced_bt_execute_pilot_rip_V",
            "at_dismount_stand",
            "at_dismount_crouch",
            "at_MP_disembark_back2idle",
            "at_mount_stand_front",
            "at_mount_stand_behind",
            "at_mount_stand_left",
            "at_mount_stand_right",
            "at_mount_stand_airgrab",
            "at_mount_above_left",
            "at_mount_above_right",
            "at_mount_kneel_front",
            "at_mount_kneel_behind",
            "at_mount_kneel_left",
            "at_mount_kneel_right",
            "at_mount_kneel_above",
            "at_mount_kneel_airgrab",
            // Phase Embark (pas_fast_embark).
            "at_dismount_stand_fast",
            "at_dismount_crouch_fast",
            "at_mount_stand_front_fast",
            "at_mount_stand_behind_fast",
            "at_mount_stand_left_fast",
            "at_mount_stand_right_fast",
            "at_mount_stand_airgrab_fast",
            "at_mount_above_left_fast",
            "at_mount_above_right_fast",
            "at_mount_kneel_front_fast",
            "at_mount_kneel_behind_fast",
            "at_mount_kneel_left_fast",
            "at_mount_kneel_right_fast",
            "at_mount_kneel_above_fast",
            "at_mount_kneel_airgrab_fast",
            "at_MP_eject_stand_start",
            "at_hotdrop_drop_2knee_turbo",
            "?at_hotdrop_drop_2knee_turbo_upgraded",
            "at_hotdrop_quickstand",
            "?CQB_CrouchIdle_MP",
            "?bt_vortex_arm_pose_add",
            "?bt_vortex_ion_arm_pose_add",
        ],
        // The crouch walks are 3x1 grids over move_yaw (their clips are blended by hand).
        grids: &[
            "?combat_aim_stand",
            "?combat_aim_run",
            "?MP_stand_Aim_all",
            "?Aim_combat_walk_MP",
            "?MP_crouch_Aim_all",
            "?Walk_Crouch_forward_MP",
            "?Walk_Crouch_backward_MP",
        ],
        body: &[],
    };
    let cockpit_spec = ActorSpec {
        path: "models/weapons/arms/pov_titan_medium_cockpit.mdl",
        sequences: &["atpov_cockpit_hatch_close_idle"],
        grids: &[],
        body: &[],
    };
    let vm_spec = ActorSpec {
        path: "models/weapons/titan_xo16_shorty/atpov_xo16shorty.mdl",
        sequences: &[
            "draw_seq",
            "ads_in_seq",
            "reload_seq",
            "reload_empty_seq",
            "?reload_late1_seq",
            "?reload_late2_seq",
            "?reload_late3_seq",
            "?reload_empty_late1_seq",
            "?reload_empty_late2_seq",
            "?reload_empty_late3_seq",
            "sprint_seq",
            "?sprintraise_seq",
            "melee_seq",
            "?melee_dash_seq",
            "?ammo_swap_seq",
            "?charge_seq",
            "?power_shot_fire_seq",
        ],
        grids: &["idle_seq", "attack_seq"],
        body: &[],
    };
    let arms_spec = ActorSpec {
        path: "models/weapons/arms/buddypov.mdl",
        sequences: &[],
        grids: &[],
        body: &[],
    };
    let gun_spec = ActorSpec {
        path: "models/weapons/titan_xo16_shorty/w_xo16shorty.mdl",
        sequences: &[],
        grids: &[],
        body: &[],
    };
    let enemy_seqs: Vec<(Vec<&str>, [&str; 2])> = kits
        .iter()
        .map(|k| (enemy_sequences(k), targets::aim_grids(&k.model)))
        .collect();
    let mut specs = vec![bt_spec, cockpit_spec, vm_spec, arms_spec, gun_spec];
    for (kit, (seqs, grids)) in kits.iter().zip(&enemy_seqs) {
        specs.push(ActorSpec {
            path: &kit.model,
            sequences: seqs,
            grids,
            body: &[],
        });
        specs.push(ActorSpec {
            path: &kit.weapon.playermodel,
            sequences: &[],
            grids: &[],
            body: &[],
        });
    }
    let collision = std::thread::scope(|scope| {
        scope.spawn(|| actor::prefetch_actors(&gd, &cache, &specs));
        let (collision, took) = collision.join().expect("collision builder panicked");
        log::info!(
            "collision: {} triangles, BVH built in {:?}",
            collision.tris.len(),
            took
        );
        collision
    });
    // TF_COLLISION_DUMP=<file>: save the movement collision for offline tests (tf-sim's
    // `pilotscan` example).
    if let Some(p) = std::env::var_os("TF_COLLISION_DUMP") {
        if let Err(e) = collision.save(std::path::Path::new(&p)) {
            log::error!("collision dump: {e}");
        }
    }
    if let Some(extra) = sweep {
        world::collision_sweep(&collision, &extra);
    }

    ph.mark("collision BVH");
    let (mut bt_pos, bt_yaw, placements) = spawn;
    // Stand on the ground under the spawn (campaign placements can float a little).
    if let Some(h) = collision.raycast(
        tf_sim::glam::Vec3::new(bt_pos.x, bt_pos.y, bt_pos.z + 100.0),
        -tf_sim::glam::Vec3::Z,
        3000.0,
    ) {
        bt_pos.z = h.point.z;
    }
    let yaw = bt_yaw.to_radians();
    log::debug!("BT spawns at {bt_pos} facing {bt_yaw}");
    input.yaw = yaw;

    // BT: the placement entity carries the simulation state; the actor hangs below it.
    let anchor = commands
        .spawn((Transform::from_translation(bt_pos), Visibility::default()))
        .id();
    commands.entity(root).add_child(anchor);
    let bt = actor::spawn_actor(
        &mut commands,
        anchor,
        &gd,
        &bt_spec,
        &mut cache,
        &mut meshes,
        &mut images,
        &mut materials,
        &mut bindposes,
    );

    // First-person cockpit, placed at the eye every frame.
    let cockpit_anchor = commands
        .spawn((Transform::IDENTITY, Visibility::Hidden, vmcam::Viewmodel))
        .id();
    commands.entity(root).add_child(cockpit_anchor);
    let cockpit = actor::spawn_actor(
        &mut commands,
        cockpit_anchor,
        &gd,
        &cockpit_spec,
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
    let mut read = |p: &str| {
        gd.read_file(p)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).to_string())
    };
    let weapon_def = weapons::WeaponDef::load_player("mp_titanweapon_xo16_shorty", &mut read);
    log::info!("weapon: {weapon_def:?}");
    let vm_anchor = commands
        .spawn((Transform::IDENTITY, Visibility::Hidden, vmcam::Viewmodel))
        .id();
    commands.entity(root).add_child(vm_anchor);
    let viewmodel = actor::spawn_actor(
        &mut commands,
        vm_anchor,
        &gd,
        &vm_spec,
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
                &arms_spec,
                &mut cache,
                &mut meshes,
                &mut images,
                &mut materials,
                &mut bindposes,
            ) {
                Ok(arms) => {
                    commands
                        .entity(arms.entity)
                        .insert(actor::BoneMergeTo(vm.entity));
                }
                Err(e) => log::warn!("arms: {e:#}"),
            }
        }
        Err(e) => log::warn!("viewmodel: {e:#}"),
    }
    ph.mark("BT, cockpit, viewmodel");
    let mut world_gun = None;
    if let Some(hand) = bt.as_ref().ok().and_then(|b| b.joint("ja_c_propGun")) {
        commands.insert_resource(weapons::BtHand(hand));
        match actor::spawn_actor(
            &mut commands,
            hand,
            &gd,
            &gun_spec,
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
                PlayerTitan::new(
                    state,
                    actor.entity,
                    cockpit.ok().map(|c| (cockpit_anchor, c.entity)),
                ),
                weapons::Weapon::new(
                    weapon_def,
                    viewmodel.ok().map(|v| (vm_anchor, v.entity)),
                    world_gun,
                ),
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
    commands.spawn((
        pilotctl::PlayerPilot::new(),
        pilotctl::PilotHealth::default(),
        pilotweapon::PilotLoadout::default(),
    ));
    commands.insert_resource(pilotweapon::WorldRoot(root));

    // Enemy spawn spots: open ground on rings around the Titan spawn, near its height and
    // with nothing overhead.
    let sv = |v: Vec3| tf_sim::glam::Vec3::new(v.x, v.y, v.z);
    let down = -tf_sim::glam::Vec3::Z;
    let bt_ground = collision
        .raycast(sv(bt_pos + Vec3::Z * 50.0), down, 2000.0)
        .map(|h| h.point.z)
        .unwrap_or(bt_pos.z);
    // The game's navmesh where the map ships one (campaign maps); a grid probed from the
    // collision mesh around the arena otherwise (MP maps have no client navmesh).
    ph.mark("world gun, pilot");
    let nav_titan = nav::Nav::load(&gd, &args.map, "large")
        .unwrap_or_else(|| nav::Nav::probe(&collision, bt_pos, 6000.0, nav::Agent::TITAN));
    ph.mark("titan nav");
    let nav_small = nav::Nav::load(&gd, &args.map, "small")
        .unwrap_or_else(|| nav::Nav::probe(&collision, bt_pos, 4000.0, nav::Agent::GRUNT));
    ph.mark("grunt nav");
    let mut spots: Vec<Vec3> = Vec::new();
    // The campaign's own Titan placements near the arena come first.
    for p in placements
        .iter()
        .filter(|p| p.distance(bt_pos) > 1200.0 && p.distance(bt_pos) < 6000.0)
    {
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
            let guess = Vec3::new(
                bt_pos.x + a.cos() * ring,
                bt_pos.y + a.sin() * ring,
                bt_ground,
            );
            let Some(hit) = collision.raycast(sv(guess + Vec3::Z * 400.0), down, 900.0) else {
                continue;
            };
            let p = Vec3::new(guess.x, guess.y, hit.point.z);
            let open_above = collision
                .raycast(sv(p + Vec3::Z * 20.0), tf_sim::glam::Vec3::Z, 400.0)
                .is_none();
            let clear = (0..8).all(|i| {
                let b = i as f32 / 8.0 * std::f32::consts::TAU;
                collision
                    .raycast(
                        sv(p + Vec3::Z * 120.0),
                        tf_sim::glam::Vec3::new(b.cos(), b.sin(), 0.0),
                        150.0,
                    )
                    .is_none()
            });
            let spaced = spots.iter().all(|q| q.distance(p) > 450.0);
            if hit.normal.z > 0.8
                && (p.z - bt_ground).abs() < 400.0
                && open_above
                && clear
                && spaced
            {
                spots.push(p);
            }
        }
    }
    // Only spots a Titan can walk to the player from (a gantry or a roof the ring probes
    // landed on would leave it pacing up there).
    ph.mark("spawn spot probes");
    let reachable: Vec<Vec3> = spots
        .iter()
        .copied()
        .filter(|s| nav_titan.path(*s, bt_pos).is_some())
        .collect();
    if reachable.len() < spots.len() {
        log::info!(
            "{} of {} enemy spawn spots have no walking route to the player, dropped",
            spots.len() - reachable.len(),
            spots.len()
        );
    }
    if !reachable.is_empty() {
        spots = reachable;
    }
    if spots.is_empty() {
        // Nowhere passes the checks (tight interiors): spawn around the player anyway.
        spots = (0..8)
            .map(|k| {
                bt_pos + Vec3::new((k as f32 * 0.785).cos(), (k as f32 * 0.785).sin(), 0.0) * 1200.0
            })
            .collect();
    }
    log::info!("{} enemy spawn spots", spots.len());
    commands.insert_resource(game::Arena {
        player_spawn: bt_pos,
        player_yaw: yaw,
        enemy_spots: spots,
    });

    // A pool of enemy Titans, two of each class (hidden until a wave needs them), each holding
    // its class's weapon.
    ph.mark("spawn spot routes");
    let mut prefetch: Vec<String> = kits
        .iter()
        .flat_map(|k| [k.model.clone(), k.weapon.playermodel.clone()])
        .collect();
    prefetch.sort();
    prefetch.dedup();
    actor::prefetch_models(&gd, &prefetch);
    // Then a squad of grunts (the last kit), each with an R-201.
    let titan_classes = kits.iter().filter(|k| !k.infantry && !k.remote).count();
    let grunt_class = kits.iter().position(|k| k.infantry && !k.remote);
    for i in 0..(titan_classes * 2 + grunt_class.map_or(0, |_| GRUNT_POOL)) as u64 {
        let class = if (i as usize) < titan_classes * 2 {
            i as usize % titan_classes
        } else {
            grunt_class.unwrap()
        };
        let kit = &kits[class];
        let seq_list = enemy_sequences(kit);
        let seqs: &[&str] = &seq_list;
        let e_anchor = commands
            .spawn((Transform::from_translation(bt_pos), Visibility::Hidden))
            .id();
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
                    let gun = ActorSpec {
                        path: &kit.weapon.playermodel,
                        sequences: &[],
                        grids: &[],
                        body: &[],
                    };
                    if let Err(e) = actor::spawn_actor(
                        &mut commands,
                        hand,
                        &gd,
                        &gun,
                        &mut cache,
                        &mut meshes,
                        &mut images,
                        &mut materials,
                        &mut bindposes,
                    ) {
                        log::warn!("enemy gun: {e:#}");
                    }
                }
                let mut enemy =
                    targets::Enemy::new(bt_pos, a.entity, 0x9E37_79B9 * (i + 1), class, kit);
                enemy.pool = Some(i as usize);
                commands.entity(e_anchor).insert(enemy);
            }
            Err(e) => log::warn!("enemy titan: {e:#}"),
        }
    }
    ph.mark("enemies");
    if !cache.missing_materials.is_empty() {
        log::warn!(
            "{} materials not found, e.g. {:?}",
            cache.missing_materials.len(),
            &cache.missing_materials[..cache.missing_materials.len().min(8)]
        );
    }

    match world::build_static_model(
        &gd,
        "models/weapons/bullets/projectile_rocket_large.mdl",
        &mut cache,
        &mut meshes,
        &mut images,
        &mut materials,
    ) {
        Ok((parts, _)) => commands.insert_resource(abilities::MissileAssets { parts, root }),
        Err(e) => log::warn!("missile model: {e:#}"),
    }
    match world::build_static_model(
        &gd,
        "models/weapons/caber_shot/caber_shot_thrown_xl.mdl",
        &mut cache,
        &mut meshes,
        &mut images,
        &mut materials,
    ) {
        Ok((parts, _)) => commands.insert_resource(titankit::TetherAssets { parts, root }),
        Err(e) => log::warn!("tether model: {e:#}"),
    }
    commands.insert_resource(Collision(collision, Some(nav_titan), Some(nav_small)));
    commands.insert_resource(zipline::Ziplines(zipline::load(&gd, &args.map)));
    commands.insert_resource(TitanSettings(params));
    commands.insert_resource(targets::EnemyKits(kits));

    // Camera: the player controller places it unless a free camera start was requested.
    if let Some(cam) = args.cam {
        let look = args.look.unwrap_or(bt_pos + Vec3::new(0.0, 0.0, 170.0));
        commands.insert_resource(player::FreeCamStart(to_bevy(cam), to_bevy(look)));
    }
    commands
        .entity(camera)
        .insert(camera_start(&args, bt_pos, yaw));
    ph.mark("rest");
    match sounds.join() {
        Ok(sfx) => commands.insert_resource(sfx),
        Err(_) => log::error!("sound loader panicked"),
    }
    ph.mark("waiting for sounds");
    log::info!(
        "materials {:.2}s: texture loads {:.2}s, spec/gloss {:.2}s",
        convert::MAT_US.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e6,
        convert::TEX_US.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e6,
        convert::SPEC_US.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e6
    );
    log::info!("scene ready in {:?}", t.elapsed());
    cache.prepared.clear();
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
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()));
            *next += 1;
        }
    } else if t > args.shots.last().unwrap().0 + 1.5 {
        exit.write(AppExit::Success);
    }
}

/// Log the average frame rate every two seconds, with the slowest frame in that time
/// (RUST_LOG=tf_viewer=info shows it).
fn perf_log(time: Res<Time<Real>>, mut acc: Local<(f32, u32, f32)>) {
    let dt = time.delta_secs();
    acc.0 += dt;
    acc.1 += 1;
    acc.2 = acc.2.max(dt);
    if acc.0 >= 2.0 {
        log::info!(
            "perf: {:.1} fps ({:.1} ms/frame, slowest {:.1} ms)",
            acc.1 as f32 / acc.0,
            acc.0 * 1000.0 / acc.1 as f32,
            acc.2 * 1000.0
        );
        *acc = (0.0, 0, 0.0);
    }
}

/// TF_FPS=1 shows the frame rate in the top-left corner: the average over the last half
/// second, its frame time, and the slowest frame in it.
#[derive(Component)]
struct FpsText;

fn fps_overlay(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut text: Query<&mut Text, With<FpsText>>,
    mut acc: Local<(f32, u32, f32)>,
) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("TF_FPS").is_some_and(|v| v != "0")) {
        return;
    }
    let Ok(mut text) = text.single_mut() else {
        commands.spawn((
            Text::new(""),
            TextFont {
                font_size: 18.0,
                ..default()
            },
            TextColor(Color::srgb(1.0, 1.0, 0.3)),
            Node {
                position_type: PositionType::Absolute,
                left: px(8),
                top: px(6),
                ..default()
            },
            GlobalZIndex(200),
            FpsText,
        ));
        return;
    };
    let dt = time.delta_secs();
    acc.0 += dt;
    acc.1 += 1;
    acc.2 = acc.2.max(dt);
    if acc.0 >= 0.5 {
        text.0 = format!(
            "{:.0} fps  {:.1} ms  (slowest {:.1} ms)",
            acc.1 as f32 / acc.0,
            acc.0 * 1000.0 / acc.1 as f32,
            acc.2 * 1000.0
        );
        *acc = (0.0, 0, 0.0);
    }
}

/// A speed readout at the bottom of the screen while on foot (TF_SPEED=1 shows it): the
/// Pilot's horizontal speed in game units per second, its vertical speed, and the move state.
#[derive(Component)]
struct SpeedText;

fn speed_overlay(
    mut commands: Commands,
    control: Res<pilotctl::Control>,
    pilots: Query<&pilotctl::PlayerPilot>,
    mut text: Query<(&mut Text, &mut Visibility), With<SpeedText>>,
) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("TF_SPEED").is_some_and(|v| v != "0")) {
        return;
    }
    let Ok((mut text, mut vis)) = text.single_mut() else {
        commands.spawn((
            Text::new(""),
            TextFont {
                font_size: 22.0,
                ..default()
            },
            TextColor(Color::srgb(1.0, 1.0, 0.3)),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(8.0),
                bottom: Val::Px(40.0),
                ..default()
            },
            GlobalZIndex(200),
            SpeedText,
        ));
        return;
    };
    let on_foot = *control == pilotctl::Control::Pilot;
    *vis = if on_foot {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if !on_foot {
        return;
    }
    let Ok(pilot) = pilots.single() else { return };
    let s = &pilot.state;
    let h = s.vel.truncate().length();
    let mode = match s.mode {
        tf_sim::pilot::PilotMove::Ground => {
            if s.sprinting {
                "sprint"
            } else if s.crouched {
                "crouch"
            } else {
                "walk"
            }
        }
        tf_sim::pilot::PilotMove::Air => "air",
        tf_sim::pilot::PilotMove::Slide => "slide",
        tf_sim::pilot::PilotMove::WallRun { .. } => "wallrun",
        tf_sim::pilot::PilotMove::Mantle { .. } => "mantle",
        _ => "",
    };
    text.0 = format!("{h:.0} u/s   vz {:+.0}   {mode}", s.vel.z);
}

/// A scrolling graph of the Pilot's horizontal speed over the last `GRAPH_SECONDS`, with the
/// keys held under it, above the speed readout (shown with it by TF_SPEED=1). The speed line
/// is coloured by move state: yellow on the ground, orange sliding, cyan in the air, green on
/// a wall. Reference lines at 260 (sprint), 350 and 400 u/s; the scale is 0 to `GRAPH_MAX`.
/// The key lanes (labelled on the left) are W A S D, Shift (bright while the key is down,
/// dim while the sprint latch holds without it), Ctrl, Space (one-shot presses) and ADS.
#[derive(Component)]
struct SpeedGraph {
    image: Handle<Image>,
    /// (speed, colour, keys) per column, oldest first; keys are `KEY_LANES` bits, any frame
    /// in the column counts.
    samples: std::collections::VecDeque<(f32, [u8; 3], u16)>,
    /// Time since the last column was added.
    acc: f32,
}

const GRAPH_W: usize = 300;
/// Height of the speed plot.
const GRAPH_H: usize = 80;
const GRAPH_SECONDS: f32 = 10.0;
const GRAPH_MAX: f32 = 600.0;
/// Key lanes under the plot: label and the bit in a sample's keys.
const KEY_LANES: &[(&str, u16)] = &[
    ("W", 1),
    ("A", 2),
    ("S", 4),
    ("D", 8),
    ("Shift", 16),
    ("Ctrl", 64),
    ("Space", 128),
    ("ADS", 256),
];
/// The sprint latch without the key: drawn dim in the Shift lane.
const KEY_LATCH: u16 = 32;
const LANE_H: usize = 10;
const LABEL_W: f32 = 44.0;
const GRAPH_TOTAL_H: usize = GRAPH_H + LANE_H * KEY_LANES.len();

fn graph_keys(i: &player::PlayerInput) -> u16 {
    let mut k = 0;
    if i.forward > 0.0 {
        k |= 1
    }
    if i.right < 0.0 {
        k |= 2
    }
    if i.forward < 0.0 {
        k |= 4
    }
    if i.right > 0.0 {
        k |= 8
    }
    if i.sprint_key {
        k |= 16
    }
    if i.sprint && !i.sprint_key {
        k |= KEY_LATCH
    }
    if i.crouch || i.crouch_toggle_key {
        k |= 64
    }
    if i.jump {
        k |= 128
    }
    if i.ads || i.pilot_ads {
        k |= 256
    }
    k
}

fn speed_graph(
    mut commands: Commands,
    control: Res<pilotctl::Control>,
    time: Res<Time>,
    input: Res<player::PlayerInput>,
    pilots: Query<&pilotctl::PlayerPilot>,
    mut images: ResMut<Assets<Image>>,
    mut graph: Query<(&mut SpeedGraph, &mut Visibility)>,
) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("TF_SPEED").is_some_and(|v| v != "0")) {
        return;
    }
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let Ok((mut g, mut vis)) = graph.single_mut() else {
        let img = Image::new_fill(
            Extent3d {
                width: GRAPH_W as u32,
                height: GRAPH_TOTAL_H as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0, 0, 0],
            TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::MAIN_WORLD
                | bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        let image = images.add(img);
        let bottom = 175.0;
        let labels: Vec<String> = KEY_LANES.iter().map(|(n, _)| n.to_string()).collect();
        commands
            .spawn((
                ImageNode::new(image.clone()),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(8.0 + LABEL_W),
                    bottom: Val::Px(bottom),
                    width: Val::Px(GRAPH_W as f32),
                    height: Val::Px(GRAPH_TOTAL_H as f32),
                    ..default()
                },
                GlobalZIndex(200),
                SpeedGraph {
                    image,
                    samples: std::collections::VecDeque::with_capacity(GRAPH_W + 1),
                    acc: 0.0,
                },
            ))
            .with_children(|p| {
                p.spawn((
                    Text::new(labels.join("\n")),
                    TextFont {
                        font_size: 9.0,
                        ..default()
                    },
                    bevy::text::LineHeight::Px(LANE_H as f32),
                    TextColor(Color::srgb(0.9, 0.9, 0.9)),
                    TextLayout::new_with_justify(Justify::Right),
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(GRAPH_W as f32 + 4.0),
                        top: Val::Px(GRAPH_H as f32),
                        width: Val::Px(LABEL_W - 4.0),
                        ..default()
                    },
                ));
            });
        return;
    };
    let on_foot = *control == pilotctl::Control::Pilot;
    *vis = if on_foot {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    let Ok(pilot) = pilots.single() else { return };
    if !on_foot {
        g.samples.clear();
        return;
    }
    let s = &pilot.state;
    let colour = match s.mode {
        tf_sim::pilot::PilotMove::Slide => [255, 150, 40],
        tf_sim::pilot::PilotMove::Air => [80, 220, 255],
        tf_sim::pilot::PilotMove::WallRun { .. } | tf_sim::pilot::PilotMove::WallHang { .. } => {
            [80, 255, 120]
        }
        _ => [255, 255, 80],
    };
    // One column per GRAPH_SECONDS / GRAPH_W of game time; the peak within the slot wins, so
    // short spikes (a slide boost frame) still show, and any key down in the slot shows.
    let speed = s.vel.truncate().length();
    let keys = graph_keys(&input);
    g.acc += time.delta_secs();
    let slot = GRAPH_SECONDS / GRAPH_W as f32;
    if g.acc >= slot || g.samples.is_empty() {
        g.acc = (g.acc - slot).max(0.0).min(slot);
        g.samples.push_back((speed, colour, keys));
        while g.samples.len() > GRAPH_W {
            g.samples.pop_front();
        }
    } else if let Some(last) = g.samples.back_mut() {
        if speed > last.0 {
            last.0 = speed;
            last.1 = colour;
        }
        last.2 |= keys;
    }
    let Some(img) = images.get_mut(&g.image) else {
        return;
    };
    let Some(data) = img.data.as_mut() else {
        return;
    };
    // y counts up from the bottom of the plot; lanes are below it (negative rows via `lane`).
    let mut put = |x: usize, row_from_top: usize, c: [u8; 3], a: u8| {
        if x < GRAPH_W && row_from_top < GRAPH_TOTAL_H {
            let i = (row_from_top * GRAPH_W + x) * 4;
            data[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], a]);
        }
    };
    let row = |v: f32| {
        GRAPH_H - 1 - ((v / GRAPH_MAX).clamp(0.0, 1.0) * (GRAPH_H - 1) as f32).round() as usize
    };
    for y in 0..GRAPH_TOTAL_H {
        let lane = y >= GRAPH_H && (y - GRAPH_H) / LANE_H % 2 == 1;
        for x in 0..GRAPH_W {
            put(x, y, if lane { [20, 20, 20] } else { [0, 0, 0] }, 110);
        }
    }
    for (v, c) in [
        (260.0, [255, 255, 80]),
        (350.0, [80, 220, 255]),
        (400.0, [255, 150, 40]),
    ] {
        for x in (0..GRAPH_W).step_by(3) {
            put(x, row(v), c, 90);
        }
    }
    let n = g.samples.len();
    let x0 = GRAPH_W - n;
    let mut prev: Option<usize> = None;
    for (i, &(v, c, keys)) in g.samples.iter().enumerate() {
        let y = row(v);
        let (lo, hi) = match prev {
            Some(p) => (p.min(y), p.max(y)),
            None => (y, y),
        };
        for yy in lo..=hi {
            put(x0 + i, yy, c, 255);
        }
        prev = Some(y);
        for (l, &(_, bit)) in KEY_LANES.iter().enumerate() {
            let top = GRAPH_H + l * LANE_H + 2;
            let (on, c) = if keys & bit != 0 {
                (true, [230, 230, 230])
            } else if bit == 16 && keys & KEY_LATCH != 0 {
                (true, [120, 120, 120])
            } else {
                (false, [0, 0, 0])
            };
            if on {
                // One-shot presses (Space) are a frame long: widen them to three columns.
                let w = if bit == 128 { 3 } else { 1 };
                for x in (x0 + i).saturating_sub(w / 2)..=(x0 + i + w / 2) {
                    for yy in top..top + LANE_H - 4 {
                        put(x, yy, c, 255);
                    }
                }
            }
        }
    }
}

/// TF_GPU_TIMES=1 logs the CPU and GPU time of every render pass (Bevy's render diagnostics,
/// timestamp queries) every few seconds: for finding what the frame spends its time on.
struct GpuTimesPlugin;

impl Plugin for GpuTimesPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var_os("TF_GPU_TIMES").is_some() {
            app.add_plugins((
                bevy::render::diagnostic::RenderDiagnosticsPlugin,
                bevy::diagnostic::LogDiagnosticsPlugin {
                    wait_duration: std::time::Duration::from_secs(4),
                    ..default()
                },
            ))
            .add_systems(Update, gpu_frame_log);
        }
    }
}

/// With TF_GPU_TIMES: every two seconds, the GPU time per frame of each render pass summed over
/// all the cameras that run it (Bevy's own log averages a pass name over its cameras), split
/// into the cameras' runs (largest first), and the frame's total.
fn gpu_frame_log(
    store: Res<bevy::diagnostic::DiagnosticsStore>,
    mut last: Local<Option<std::time::Instant>>,
    mut acc: Local<(u32, f32, std::collections::BTreeMap<String, Vec<f64>>)>,
    time: Res<Time<Real>>,
) {
    // All of one frame's render diagnostics are stored with the same instant.
    let Some(newest) = store
        .iter()
        .filter(|d| d.path().as_str().starts_with("render/"))
        .filter_map(|d| d.measurement().map(|m| m.time))
        .max()
    else {
        return;
    };
    if *last == Some(newest) {
        return;
    }
    *last = Some(newest);
    acc.0 += 1;
    for d in store.iter() {
        let Some(pass) = d
            .path()
            .as_str()
            .strip_prefix("render/")
            .and_then(|p| p.strip_suffix("/elapsed_gpu"))
        else {
            continue;
        };
        // Each camera's run, largest first.
        let mut runs: Vec<f64> = d
            .measurements()
            .filter(|m| m.time == newest)
            .map(|m| m.value)
            .collect();
        runs.sort_by(|a, b| b.total_cmp(a));
        let e = acc.2.entry(pass.to_string()).or_default();
        e.resize(e.len().max(runs.len()), 0.0);
        for (s, v) in e.iter_mut().zip(runs) {
            *s += v;
        }
    }
    acc.1 += time.delta_secs();
    if acc.1 >= 2.0 {
        let frames = acc.0 as f64;
        let mut rows: Vec<_> = acc
            .2
            .iter()
            .map(|(k, runs)| {
                (
                    k.clone(),
                    runs.iter().sum::<f64>() / frames,
                    runs.iter()
                        .map(|v| format!("{:.3}", v / frames))
                        .collect::<Vec<_>>()
                        .join("+"),
                )
            })
            .collect();
        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
        let total: f64 = rows.iter().map(|r| r.1).sum();
        let list: Vec<String> = rows
            .iter()
            .filter(|r| r.1 >= 0.005)
            .map(|r| format!("{} {}", r.0, r.2))
            .collect();
        log::info!(
            "gpu: {total:.2} ms/frame over {} frames: {}",
            acc.0,
            list.join(", ")
        );
        *acc = (0, 0.0, Default::default());
    }
}
