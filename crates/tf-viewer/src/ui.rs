//! Front end in Titanfall 2's style, built from the game's own UI resources: its fonts
//! (`resource/*.vfont`), UI atlases (`ui.rpak` uimg, looked up by RUI image path), the title
//! key art and logo, and the map's load screen. Layout follows `resource/ui/menus/main.menu`
//! (logo above centre, a column of buttons below it).
//!
//! Screens: loading, main menu, pause (Esc) and game over.

use crate::game::{Game, GameState};
use crate::player::{CameraMode, PlayerInput};
use crate::vitals::Difficulty;
use crate::audio::MasterVolume;
use bevy::audio::Volume;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use std::collections::HashMap;
use tf_assets::rpak::Rpak;

/// The game's UI art and fonts.
#[derive(Resource, Default)]
pub struct UiAssets {
    pub title_font: Handle<Font>,
    pub font: Handle<Font>,
    pub bold_font: Handle<Font>,
    images: HashMap<u32, (Handle<Image>, Rect)>,
    pub loadscreen: Option<Handle<Image>>,
    /// Weapon id -> its loadout icon (the script's `menu_icon` UI image path).
    pub weapon_icons: HashMap<String, String>,
}

impl UiAssets {
    /// An atlas image by its RUI path, e.g. "rui/menu/common/keyart".
    pub fn image(&self, path: &str) -> Option<ImageNode> {
        let (h, r) = self.images.get(&tf_assets::uimg::path_hash(path))?;
        Some(ImageNode { image: h.clone(), rect: Some(*r), ..default() })
    }
}

/// Loading proceeds once the loading screen has been drawn.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadState {
    /// Frames drawn so far.
    Waiting(u32),
    Load,
    /// The world is built; the loadout and kits build under the loading screen for a few
    /// frames (built on the first playing frame, they froze it for a second).
    Warm(u32),
    Done,
}

/// When loading finished: the clock for scripted tests and screenshots.
#[derive(Resource, Clone, Copy)]
pub struct LoadedAt(pub std::time::Instant);

/// Log frames slower than 60 ms (TF_HITCH_MS) after loading, with the time since load (finding first-use
/// stalls; `RUST_LOG=tf_viewer::ui=debug`).
pub fn log_hitches(loaded: Option<Res<LoadedAt>>, mut last: Local<Option<std::time::Instant>>) {
    let now = std::time::Instant::now();
    if let (Some(l), Some(prev)) = (loaded, *last) {
        let dt = now - prev;
        static LIMIT: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
        if dt.as_millis() > *LIMIT.get_or_init(|| std::env::var("TF_HITCH_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(60)) {
            log::debug!("hitch: {:?} frame at {:.2} s", dt, l.secs());
        }
    }
    *last = Some(now);
}

impl LoadedAt {
    pub fn secs(&self) -> f32 {
        self.0.elapsed().as_secs_f32()
    }
}

pub fn loading(s: Res<LoadState>) -> bool {
    *s == LoadState::Load
}

pub fn loaded(s: Res<LoadState>) -> bool {
    *s == LoadState::Done
}

/// The world exists (warming up under the loading screen, or playing).
pub fn world_ready(s: Res<LoadState>) -> bool {
    matches!(*s, LoadState::Warm(_) | LoadState::Done)
}

/// Text sized relative to the window height (percent), like the game's 1080p-based layouts.
#[derive(Component)]
pub struct VhText(pub f32);

fn upload(images: &mut Assets<Image>, pak: &Rpak, guid: u64) -> Option<Handle<Image>> {
    use bevy::render::render_resource::{Extent3d, TextureDimension};
    let a = pak.asset(guid)?;
    let tex = tf_assets::texture::load(pak, a).ok()?;
    let format = crate::convert::wgpu_format(tex.info.format)?;
    let mut img = Image::default();
    img.data = Some(tex.data);
    img.texture_descriptor.size = Extent3d { width: tex.width, height: tex.height, depth_or_array_layers: 1 };
    img.texture_descriptor.mip_level_count = tex.mips;
    img.texture_descriptor.format = format;
    img.texture_descriptor.dimension = TextureDimension::D2;
    img.sampler = bevy::image::ImageSampler::linear();
    img.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
    Some(images.add(img))
}

/// Read fonts, atlases and the load screen (fast: well under a second).
pub fn load_ui_assets(mut commands: Commands, args: Res<crate::Args>, mut images: ResMut<Assets<Image>>, mut fonts: ResMut<Assets<Font>>) {
    let t = std::time::Instant::now();
    let root = std::path::Path::new(&args.game);
    let mut ui = UiAssets::default();
    // Fonts: Titanfall (titles) and Metronic Pro (everything else).
    let vpks: Vec<tf_assets::vpk::Vpk> = ["englishclient_frontend", "englishclient_mp_common"]
        .iter()
        .filter_map(|v| tf_assets::vpk::Vpk::open(root.join(format!("vpk/{v}.bsp.pak000_dir.vpk"))).ok())
        .collect();
    let mut font = |name: &str| -> Handle<Font> {
        let data = vpks.iter().find_map(|v| v.read(&format!("resource/{name}.vfont")).ok());
        match data.and_then(|d| tf_assets::vfont::decode(&d).ok()).and_then(|ttf| Font::try_from_bytes(ttf).ok()) {
            Some(f) => fonts.add(f),
            None => {
                log::warn!("font {name} unavailable");
                Handle::default()
            }
        }
    };
    // Localised strings (UTF-16 keyvalues): "KEY" "Text".
    let mut strings = std::collections::HashMap::new();
    if let Some(raw) = vpks.iter().find_map(|v| v.read("resource/r1_english.txt").ok()) {
        let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let text = String::from_utf16_lossy(&units);
        for line in text.lines() {
            let q: Vec<&str> = line.split('"').collect();
            if q.len() >= 4 {
                strings.insert(q[1].to_string(), q[3].to_string());
            }
        }
    }
    // Each arsenal weapon's display name, by weapon id, from its script's printname, and its
    // loadout icon (menu_icon).
    for id in crate::pilotweapon::ARSENAL.iter().map(|e| e.id).chain(crate::weapons::TITAN_ARSENAL.iter().map(|g| g.id)) {
        let text = vpks.iter().find_map(|v| v.read(&format!("scripts/weapons/{id}.txt")).ok()).map(|b| String::from_utf8_lossy(&b).to_string());
        if let Some(icon) = text.as_deref().and_then(|t| t.lines().find(|l| l.contains("\"menu_icon\"")).and_then(|l| l.split('"').nth(3))) {
            ui.weapon_icons.insert(id.to_string(), icon.to_string());
        }
        let key = text.as_deref().and_then(|t| t.lines().find(|l| l.contains("\"printname\"")).and_then(|l| l.split('"').nth(3)).map(|k| k.trim_start_matches('#').to_string()));
        if let Some(name) = key.and_then(|k| strings.get(&k).cloned()) {
            strings.entry(id.to_string()).or_insert(name);
        }
    }
    log::info!("{} localised strings", strings.len());
    commands.insert_resource(crate::pilotweapon::Strings(strings));
    ui.title_font = font("titanfall-regular");
    ui.font = font("metronicpro-regular");
    ui.bold_font = font("metronicpro-semibold");
    // UI atlases: every image, keyed by path hash.
    let paks = root.join("r2/paks/Win64");
    match Rpak::open(paks.join("ui.rpak")) {
        Ok(pak) => {
            let mut textures: HashMap<u64, Option<Handle<Image>>> = HashMap::new();
            for a in pak.assets.iter().filter(|a| a.kind_str() == "uimg") {
                let Ok(atlas) = tf_assets::uimg::load(&pak, a) else { continue };
                let tex = textures.entry(atlas.texture).or_insert_with(|| upload(&mut images, &pak, atlas.texture)).clone();
                let Some(tex) = tex else { continue };
                for i in atlas.images {
                    let r = Rect::new(i.x as f32, i.y as f32, (i.x + i.w) as f32, (i.y + i.h) as f32);
                    ui.images.insert(i.hash, (tex.clone(), r));
                }
            }
        }
        Err(e) => log::warn!("ui.rpak: {e:#}"),
    }
    // The map's load screen.
    let loadscreen = tf_assets::rpak::resolve(&paks, &format!("{}_loadscreen.rpak", args.map));
    if let Some(Ok(pak)) = loadscreen.map(Rpak::open) {
        ui.loadscreen = pak.assets.iter().find(|a| a.kind_str() == "txtr").and_then(|a| upload(&mut images, &pak, a.guid));
    }
    let icons_found = ui.weapon_icons.values().filter(|p| ui.image(p).is_some()).count();
    log::info!("ui: {} atlas images, load screen {}, weapon icons {}/{} in the atlases, in {:?}", ui.images.len(), ui.loadscreen.is_some(), icons_found, ui.weapon_icons.len(), t.elapsed());
    commands.insert_resource(ui);
}

#[derive(Component)]
pub struct UiCamera;

#[derive(Component)]
pub struct LoadingScreen;

/// Full-window image that keeps its aspect ratio by covering the window.
#[derive(Component)]
pub struct CoverImage(pub f32);

pub fn spawn_loading_screen(mut commands: Commands, ui: Res<UiAssets>, args: Res<crate::Args>, strings: Res<crate::pilotweapon::Strings>) {
    commands.spawn((
        Camera2d,
        // Last, over the world and viewmodel cameras that spawn while loading.
        Camera { order: 10, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
        IsDefaultUiCamera,
        UiCamera,
    ));
    let root = commands
        .spawn((
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), overflow: Overflow::clip(), ..default() },
            BackgroundColor(Color::BLACK),
            GlobalZIndex(100),
            LoadingScreen,
        ))
        .id();
    if let Some(img) = &ui.loadscreen {
        let bg = commands
            .spawn((ImageNode::new(img.clone()), Node { position_type: PositionType::Absolute, ..default() }, CoverImage(16.0 / 9.0)))
            .id();
        commands.entity(root).add_child(bg);
    }
    let map_name = map_name(&strings, &args.map);
    let panel = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: vh(5.0),
                bottom: vh(6.0),
                flex_direction: FlexDirection::Column,
                row_gap: vh(0.8),
                padding: UiRect::axes(vh(2.2), vh(1.6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        ))
        .id();
    let title = text(&mut commands, &map_name, &ui.title_font, 5.2, Color::WHITE);
    let sub = text(&mut commands, "LOADING", &ui.bold_font, 2.0, Color::srgba(1.0, 1.0, 1.0, 0.7));
    commands.entity(panel).add_children(&[title, sub]);
    commands.entity(root).add_child(panel);
}

/// The multiplayer maps (all of them ship with the game).
pub const MAPS: &[&str] = &[
    "mp_forwardbase_kodai", "mp_angel_city", "mp_black_water_canal", "mp_colony02", "mp_complex3", "mp_crashsite3", "mp_drydock", "mp_eden",
    "mp_glitch", "mp_grave", "mp_homestead", "mp_relic02", "mp_rise", "mp_thaw", "mp_wargames", "mp_lf_deck", "mp_lf_meadow", "mp_lf_stacks",
    "mp_lf_township", "mp_lf_traffic", "mp_lf_uma", "mp_coliseum", "mp_coliseum_column",
    // The campaign, in mission order (not The Ark, sp_s2s: its ships are script-moved entities).
    "sp_training", "sp_crashsite", "sp_sewers1", "sp_boomtown_start", "sp_boomtown", "sp_boomtown_end", "sp_hub_timeshift",
    "sp_timeshift_spoke02", "sp_beacon", "sp_beacon_spoke0", "sp_tday", "sp_skyway_v1",
];

/// The map picked in the main menu.
#[derive(Resource)]
pub struct MapChoice(pub String);

/// "MP_ANGEL_CITY_ALLCAPS" from the game's strings, else the file name.
/// Campaign maps use their mission name ("SP_SEWERS1_CAMPAIGN_NAME"), numbered when a mission
/// spans several maps.
pub fn map_name(strings: &crate::pilotweapon::Strings, map: &str) -> String {
    if map.starts_with("sp_") {
        let mission = |m: &str| strings.0.get(&format!("{}_CAMPAIGN_NAME", m.to_uppercase())).cloned();
        if let Some(name) = mission(map) {
            let parts: Vec<&&str> = MAPS.iter().filter(|m| mission(m).as_ref() == Some(&name)).collect();
            let name = name.to_uppercase();
            return match parts.iter().position(|m| **m == map) {
                Some(i) if parts.len() > 1 => format!("{name} {}", i + 1),
                _ => name,
            };
        }
    }
    strings.0.get(&format!("{}_ALLCAPS", map.to_uppercase())).cloned().unwrap_or_else(|| map.trim_start_matches("mp_").trim_start_matches("sp_").replace('_', " ").to_uppercase())
}

/// Start the game again on another map with the same settings, then quit this one.
fn relaunch(args: &crate::Args, map: &str, difficulty: Difficulty, volume: f32, loadout: &crate::pilotweapon::LoadoutChoice, titan: usize) {
    use crate::pilotweapon::{Slot, ARSENAL};
    let Ok(exe) = std::env::current_exe() else { return };
    let ids = [Slot::Primary, Slot::Sidearm, Slot::AntiTitan].map(|s| ARSENAL[loadout.get(s)].id).join(",");
    let r = std::process::Command::new(exe)
        .args(["--game", &args.game, "--map", map, "--difficulty", &difficulty.name().to_lowercase(), "--volume", &format!("{volume:.2}"), "--loadout", &ids, "--titan", &titan.to_string()])
        .spawn();
    log::info!("relaunching on {map}: {:?}", r.map(|c| c.id()));
}

fn vh(v: f32) -> Val {
    Val::Vh(v)
}

fn text(commands: &mut Commands, s: &str, font: &Handle<Font>, size_vh: f32, color: Color) -> Entity {
    commands.spawn((Text::new(s), TextFont { font: font.clone(), font_size: 20.0, ..default() }, TextColor(color), VhText(size_vh))).id()
}

/// Count drawn frames, then let the world load; when it has, drop the loading screen.
/// Render pipelines still compiling, counted in the render world each frame. Bevy skips
/// meshes whose pipeline isn't ready, so a world shown too early draws in pieces.
#[derive(Resource, Clone, Default)]
pub struct PendingPipelines(pub std::sync::Arc<std::sync::atomic::AtomicUsize>);

pub struct PipelineWatchPlugin;

impl Plugin for PipelineWatchPlugin {
    fn build(&self, app: &mut App) {
        let pending = PendingPipelines::default();
        app.insert_resource(pending.clone());
        if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render.insert_resource(pending).add_systems(bevy::render::Render, count_pending_pipelines.in_set(bevy::render::RenderSystems::Cleanup));
        }
    }
}

fn count_pending_pipelines(cache: Res<bevy::render::render_resource::PipelineCache>, pending: Res<PendingPipelines>) {
    pending.0.store(cache.waiting_pipelines().count(), std::sync::atomic::Ordering::Relaxed);
}

/// Frames the world warms up under the loading screen: at least `WARM_MIN`, then until no
/// pipeline has been compiling for `WARM_SETTLE` frames, at most `WARM_MAX`.
const WARM_MIN: u32 = 3;
const WARM_SETTLE: u32 = 4;
const WARM_MAX: u32 = 600;

pub fn loading_tick(
    mut commands: Commands,
    mut state: ResMut<LoadState>,
    screens: Query<Entity, With<LoadingScreen>>,
    cams: Query<Entity, With<UiCamera>>,
    pending: Res<PendingPipelines>,
    mut settled: Local<u32>,
    mut first_person: Query<(Entity, &mut Visibility), With<crate::vmcam::Viewmodel>>,
    mut shown: Local<Vec<Entity>>,
) {
    let compiling = pending.0.load(std::sync::atomic::Ordering::Relaxed);
    // First-person models (cockpit, guns, arms) are hidden until play decides which show:
    // draw them all behind the loading screen meanwhile, so their pipelines are ready too.
    if matches!(*state, LoadState::Warm(_)) {
        for (e, mut v) in &mut first_person {
            if *v == Visibility::Hidden {
                *v = Visibility::Inherited;
                shown.push(e);
            }
        }
    }
    match *state {
        LoadState::Waiting(n) if n >= 6 => *state = LoadState::Load,
        LoadState::Waiting(n) => *state = LoadState::Waiting(n + 1),
        LoadState::Load => *state = LoadState::Warm(0),
        LoadState::Warm(n) if n < WARM_MIN || (*settled < WARM_SETTLE && n < WARM_MAX) => {
            *settled = if compiling == 0 { *settled + 1 } else { 0 };
            *state = LoadState::Warm(n + 1);
        }
        LoadState::Warm(n) => {
            log::info!("world warmed up in {n} frames ({compiling} pipelines still compiling)");
            for e in shown.drain(..) {
                if let Ok((_, mut v)) = first_person.get_mut(e) {
                    *v = Visibility::Hidden;
                }
            }
            *state = LoadState::Done;
            commands.insert_resource(LoadedAt(std::time::Instant::now()));
            for e in &screens {
                commands.entity(e).despawn();
            }
            // From now on the UI draws on the 3D camera.
            for c in &cams {
                commands.entity(c).despawn();
            }
        }
        LoadState::Done => {}
    }
}

/// Scale `VhText` fonts and cover images with the window.
pub fn scale_ui(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut texts: Query<(&VhText, &mut TextFont)>,
    mut covers: Query<(&CoverImage, &mut Node)>,
) {
    let Ok(w) = windows.single() else { return };
    let (ww, wh) = (w.width(), w.height().max(1.0));
    for (v, mut f) in &mut texts {
        let size = (v.0 * wh / 100.0).max(6.0);
        if (f.font_size - size).abs() > 0.1 {
            f.font_size = size;
        }
    }
    for (c, mut n) in &mut covers {
        // Cover: scale to the larger of the two fits, centred.
        let (iw, ih) = if ww / wh > c.0 { (ww, ww / c.0) } else { (wh * c.0, wh) };
        n.width = px(iw);
        n.height = px(ih);
        n.left = px((ww - iw) * 0.5);
        n.top = px((wh - ih) * 0.5);
    }
}

// ---------------------------------------------------------------------------------------------
// Menus

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    None,
    Main,
    Pause,
    GameOver,
    /// Pilot and Titan loadout (editpilotloadout.menu / edittitanloadout.menu).
    Loadout,
    /// Mouse, controller, FOV, audio (controls.menu, video.menu, audio.menu).
    Settings,
    /// Key bindings (mousekeyboardbindings.menu).
    Bindings,
    /// Campaign mission select.
    Missions,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Play,
    Resume,
    Restart,
    Difficulty,
    Volume,
    Graphics,
    Primary,
    Sidearm,
    AntiTitan,
    Tactical,
    Ordnance,
    TitanLoadout,
    Map,
    Campaign,
    Mission(usize),
    Loadout,
    Settings,
    Bindings,
    Bind(usize),
    ResetBinds,
    MouseSens,
    AdsSens,
    Fov,
    InvertY,
    PadLook,
    PadLookAds,
    PadCurve,
    PadInvert,
    AimAssist,
    PadLayout,
    Back,
    MainMenu,
    Quit,
}

impl Action {
    /// Options that change with left/right (or cycle when clicked).
    fn is_option(self) -> bool {
        matches!(
            self,
            Action::Difficulty
                | Action::Volume
                | Action::Graphics
                | Action::Primary
                | Action::Sidearm
                | Action::AntiTitan
                | Action::Tactical
                | Action::Ordnance
                | Action::TitanLoadout
                | Action::Map
                | Action::MouseSens
                | Action::AdsSens
                | Action::Fov
                | Action::InvertY
                | Action::PadLook
                | Action::PadLookAds
                | Action::PadCurve
                | Action::PadInvert
                | Action::AimAssist
                | Action::PadLayout
        )
    }
}

/// Sliders: the value as a fraction of its range, read from and written to the settings.
fn slider_get(a: Action, settings: &crate::settings::Settings, volume: &MasterVolume) -> Option<f32> {
    Some(match a {
        Action::Volume => volume.volume.to_linear(),
        Action::MouseSens => settings.mouse_sensitivity / 20.0,
        Action::AdsSens => settings.mouse_sensitivity_zoomed / 20.0,
        Action::Fov => (settings.fov_scale - 1.0) / 0.55,
        _ => return None,
    })
}

fn slider_set(a: Action, f: f32, settings: &mut crate::settings::Settings, volume: &mut MasterVolume) {
    let f = f.clamp(0.0, 1.0);
    match a {
        Action::Volume => volume.volume = Volume::Linear((f * 100.0).round() / 100.0),
        // The game's slider steps (controls.menu, video.menu).
        Action::MouseSens => settings.mouse_sensitivity = (f * 100.0).round() / 5.0,
        Action::AdsSens => settings.mouse_sensitivity_zoomed = (f * 100.0).round() / 5.0,
        Action::Fov => settings.fov_scale = (1.0 + (f * 20.0).round() * 0.0275).min(1.55),
        _ => {}
    }
}

/// A slider's track (clickable and draggable) and its fill.
#[derive(Component)]
pub struct SliderTrack(Action);

#[derive(Component)]
pub struct SliderFill(Action);

fn opt(title: &str, value: impl std::fmt::Display) -> String {
    format!("{title}      <  {value}  >")
}

fn on_off(b: bool) -> &'static str {
    if b {
        "ON"
    } else {
        "OFF"
    }
}

/// The campaign's missions in order: (mission name, first map).
pub fn missions(strings: &crate::pilotweapon::Strings) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = Vec::new();
    for m in MAPS.iter().filter(|m| m.starts_with("sp_")) {
        let name = strings.0.get(&format!("{}_CAMPAIGN_NAME", m.to_uppercase())).cloned().unwrap_or_else(|| map_name(strings, m));
        if out.last().is_none_or(|(n, _)| *n != name) {
            out.push((name, m));
        }
    }
    out
}

#[derive(Resource)]
pub struct Menu {
    pub page: Page,
    pub focus: usize,
    pub paused: bool,
    root: Option<Entity>,
    /// Sub-pages opened over the current page (Loadout, Settings, ...), innermost last.
    stack: Vec<(Page, usize)>,
    /// Waiting for a key to bind to this command (armed after the click that started it).
    capture: Option<(crate::settings::Command, bool)>,
    /// Number of campaign missions (for the Missions page's list).
    missions: usize,
    /// Focus to restore when returning to a parent page.
    restore: Option<usize>,
    /// The slider being dragged with the mouse.
    dragging: Option<Action>,
}

impl Default for Menu {
    fn default() -> Self {
        Self { page: Page::None, focus: 0, paused: false, root: None, stack: Vec::new(), capture: None, missions: 0, restore: None, dragging: None }
    }
}

#[derive(Component)]
pub struct MenuButton {
    action: Action,
    index: usize,
}

#[derive(Component)]
pub struct MenuLabel;

#[derive(Component)]
pub struct FocusBar;

/// Icon shown on a loadout row (updated as the choice changes).
#[derive(Component)]
pub struct MenuIcon;

/// The focused mission's load screen on the Missions page.
#[derive(Component)]
pub struct MissionImage;

/// Load screens for the mission select page, by map (loaded on first focus).
#[derive(Resource, Default)]
pub struct MissionImages(HashMap<String, Option<Handle<Image>>>);

fn actions(page: Page, missions: usize) -> Vec<Action> {
    use Action::*;
    match page {
        Page::Main => vec![Play, Campaign, Map, Loadout, Difficulty, Settings, Quit],
        Page::Pause => vec![Resume, Restart, Loadout, Difficulty, Settings, MainMenu, Quit],
        Page::GameOver => vec![Restart, Loadout, Difficulty, MainMenu, Quit],
        Page::Loadout => vec![Primary, Sidearm, AntiTitan, Tactical, Ordnance, TitanLoadout, Back],
        Page::Settings => vec![MouseSens, AdsSens, InvertY, Fov, Volume, Graphics, Bindings, PadLook, PadLookAds, PadCurve, PadInvert, AimAssist, PadLayout, Back],
        Page::Bindings => (0..crate::settings::Command::ALL.len()).map(Bind).chain([ResetBinds, Back]).collect(),
        Page::Missions => (0..missions).map(Mission).chain([Back]).collect(),
        Page::None => vec![],
    }
}

/// Which page the game state calls for (or the open sub-page); rebuild it when that changes.
#[allow(clippy::too_many_arguments)]
pub fn sync_menu(
    mut commands: Commands,
    mut menu: ResMut<Menu>,
    game: Res<Game>,
    ui: Res<UiAssets>,
    settings: Res<crate::settings::Settings>,
    strings: Res<crate::pilotweapon::Strings>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let base = match game.state {
        GameState::Title => Page::Main,
        GameState::GameOver => Page::GameOver,
        _ if menu.paused => Page::Pause,
        _ => Page::None,
    };
    if base == Page::None {
        menu.stack.clear();
        menu.capture = None;
    }
    let want = menu.stack.last().map(|s| s.0).unwrap_or(base);
    if want == menu.page {
        return;
    }
    if let Some(r) = menu.root.take() {
        commands.entity(r).despawn();
    }
    menu.page = want;
    menu.focus = menu.restore.take().unwrap_or(0);
    menu.missions = missions(&strings).len();
    if want != Page::None {
        if let Ok(mut c) = cursor.single_mut() {
            c.grab_mode = CursorGrabMode::None;
            c.visible = true;
        }
        menu.root = Some(build_page(&mut commands, &ui, want, &game, &settings, menu.missions));
    }
}

fn build_page(commands: &mut Commands, ui: &UiAssets, page: Page, game: &Game, settings: &crate::settings::Settings, missions: usize) -> Entity {
    let root = commands
        .spawn((
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), overflow: Overflow::clip(), ..default() },
            GlobalZIndex(50),
        ))
        .id();
    // Background: the title key art on the main menu and its sub-pages before play, a dark
    // veil over the game otherwise.
    let over_title = matches!(page, Page::Main) || (game.state == GameState::Title && page != Page::None);
    if over_title {
        commands.entity(root).insert(BackgroundColor(Color::BLACK));
        if let Some(img) = ui.image("rui/menu/common/keyart") {
            let bg = commands.spawn((img, Node { position_type: PositionType::Absolute, ..default() }, CoverImage(1919.0 / 1079.0))).id();
            commands.entity(root).add_child(bg);
        }
    } else {
        commands.entity(root).insert(BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.62)));
    }
    // Shade the lower half so the buttons read well over the art.
    let shade = commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: px(0), right: px(0), bottom: px(0), height: percent(if page == Page::Main { 55.0 } else { 100.0 }), ..default() },
            BackgroundGradient::from(LinearGradient::to_top(vec![
                ColorStop::auto(Color::srgba(0.0, 0.0, 0.0, 0.85)),
                ColorStop::auto(Color::srgba(0.0, 0.0, 0.0, if page == Page::Main { 0.0 } else { 0.45 })),
            ])),
        ))
        .id();
    commands.entity(root).add_child(shade);

    let column = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        })
        .id();
    commands.entity(root).add_child(column);
    // Header: the logo on the main menu, a title elsewhere.
    let title = |commands: &mut Commands, s: &str| {
        let t = text(commands, s, &ui.title_font, 6.0, Color::WHITE);
        commands.entity(t).insert(Node { margin: UiRect::bottom(vh(3.5)), ..default() });
        t
    };
    match page {
        Page::Main => {
            if let Some(mut logo) = ui.image("rui/hud/wilds/TF2_Logo") {
                logo.color = Color::WHITE;
                // TitleRui: 1408 x 288 at 1080p, 160 above centre.
                let l = commands.spawn((logo, Node { width: vh(130.4), max_width: Val::Vw(90.0), aspect_ratio: Some(2988.0 / 288.0), margin: UiRect::bottom(vh(9.0)), ..default() })).id();
                commands.entity(column).add_child(l);
            }
        }
        Page::Pause => {
            let t = title(commands, "PAUSED");
            commands.entity(column).add_child(t);
        }
        Page::Loadout => {
            let t = title(commands, "LOADOUT");
            commands.entity(column).add_child(t);
        }
        Page::Settings => {
            let t = title(commands, "SETTINGS");
            commands.entity(column).add_child(t);
        }
        Page::Bindings => {
            let t = title(commands, "KEY BINDINGS");
            commands.entity(column).add_child(t);
        }
        Page::Missions => {
            let t = title(commands, "CAMPAIGN");
            commands.entity(column).add_child(t);
        }
        Page::GameOver => {
            let t = text(commands, "PILOT DOWN", &ui.title_font, 8.0, Color::srgb(1.0, 0.42, 0.25));
            let s = text(
                commands,
                &format!("WAVE {}    TITAN KILLS {}    BEST WAVE {}", game.wave, game.kills, game.best_wave),
                &ui.bold_font,
                2.4,
                Color::srgba(1.0, 1.0, 1.0, 0.85),
            );
            commands.entity(s).insert(Node { margin: UiRect::bottom(vh(5.0)), ..default() });
            commands.entity(column).add_children(&[t, s]);
        }
        Page::None => {}
    }
    // Buttons (RuiSmallButton), pinned to the left edge of main.menu's 1328-wide PinFrame
    // that sits under the title. Long lists (settings, bindings) use smaller rows.
    let list_actions = actions(page, missions);
    let long = list_actions.len() > 10;
    let (row_h, font_vh, list_w) = if long { (3.6, 1.9, 62.0) } else { (5.2, 2.5, 58.0) };
    let frame = commands
        .spawn(Node { width: vh(123.0), max_width: Val::Vw(92.0), flex_direction: FlexDirection::Row, column_gap: vh(4.0), align_items: AlignItems::FlexStart, ..default() })
        .id();
    let list = commands.spawn(Node { flex_direction: FlexDirection::Column, row_gap: vh(if long { 0.35 } else { 0.6 }), width: vh(list_w), max_width: Val::Vw(86.0), ..default() }).id();
    commands.entity(frame).add_child(list);
    commands.entity(column).add_child(frame);
    for (i, a) in list_actions.iter().enumerate() {
        let b = commands
            .spawn((
                Button,
                Node { height: vh(row_h), align_items: AlignItems::Center, padding: UiRect::horizontal(vh(2.4)), column_gap: vh(1.6), ..default() },
                BackgroundColor(Color::NONE),
                MenuButton { action: *a, index: i },
            ))
            .id();
        let bar = commands
            .spawn((Node { position_type: PositionType::Absolute, left: px(0), top: px(0), bottom: px(0), width: vh(0.5), ..default() }, BackgroundColor(Color::NONE), FocusBar))
            .id();
        commands.entity(b).add_child(bar);
        // Loadout rows show the chosen item's icon from the game's loadout art.
        if page == Page::Loadout && *a != Action::Back {
            let icon = commands
                .spawn((ImageNode::default(), Node { width: vh(row_h * 1.9), height: vh(row_h * 0.85), ..default() }, MenuIcon))
                .id();
            commands.entity(b).add_child(icon);
        }
        let label = text(commands, "", &ui.bold_font, font_vh, Color::srgba(1.0, 1.0, 1.0, 0.7));
        commands.entity(label).insert(MenuLabel);
        commands.entity(b).add_child(label);
        if matches!(a, Action::Volume | Action::MouseSens | Action::AdsSens | Action::Fov) {
            // The track is the full row height so it is easy to hit; the bar is drawn inside.
            let track = commands
                .spawn((
                    Node { width: vh(18.0), height: vh(row_h), margin: UiRect::left(Val::Auto), align_items: AlignItems::Center, ..default() },
                    bevy::ui::RelativeCursorPosition::default(),
                    SliderTrack(*a),
                ))
                .id();
            let bar = commands
                .spawn((Node { width: percent(100), height: vh(0.7), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.18))))
                .id();
            let fill = commands
                .spawn((Node { width: percent(0), height: percent(100), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.85)), SliderFill(*a)))
                .id();
            commands.entity(bar).add_child(fill);
            commands.entity(track).add_child(bar);
            commands.entity(b).add_child(track);
        }
        commands.entity(list).add_child(b);
    }
    // The focused mission's load screen beside the mission list.
    if page == Page::Missions {
        let img = commands
            .spawn((ImageNode::default(), Node { width: vh(56.0), height: vh(31.5), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)), MissionImage))
            .id();
        commands.entity(frame).add_child(img);
    }
    // Controls, on the pause menu, from the current key bindings.
    if page == Page::Pause {
        use crate::settings::Command as C;
        let panel = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: vh(5.0),
                    top: percent(50),
                    margin: UiRect::top(vh(-22.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: vh(0.5),
                    padding: UiRect::all(vh(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            ))
            .id();
        let head = text(commands, "CONTROLS", &ui.bold_font, 2.0, Color::srgb(1.0, 0.62, 0.15));
        commands.entity(panel).add_child(head);
        let k = |c: C| settings.bind(c).name();
        let rows = [
            (format!("{}{}{}{}", k(C::Forward), k(C::Left), k(C::Back), k(C::Right)), "Move"),
            (k(C::Sprint), "Sprint"),
            (k(C::Jump), "Dash / Jump"),
            (k(C::Crouch), "Crouch / Slide"),
            (format!("{} / {}", k(C::Fire), k(C::Ads)), "Fire / Aim"),
            (k(C::Reload), "Reload"),
            (k(C::Ability), "Tactical / Titan Defensive"),
            (k(C::Utility), "Titan Utility"),
            (k(C::Throw), "Ordnance"),
            (k(C::Melee), "Melee"),
            (k(C::Core), "Titan Core"),
            (k(C::Use), "Disembark / Embark"),
            (k(C::Titanfall), "Titanfall"),
            (k(C::View), "Cockpit / Third Person"),
            ("- / =".to_string(), "Volume"),
        ];
        for (kk, v) in rows {
            let row = commands.spawn(Node { column_gap: vh(2.0), ..default() }).id();
            let kt = text(commands, &kk, &ui.bold_font, 1.55, Color::WHITE);
            commands.entity(kt).insert(Node { width: vh(13.0), ..default() });
            let vt = text(commands, v, &ui.font, 1.55, Color::srgba(1.0, 1.0, 1.0, 0.75));
            commands.entity(row).add_children(&[kt, vt]);
            commands.entity(panel).add_child(row);
        }
        commands.entity(root).add_child(panel);
    }
    // Footer hints, bottom right like the game's footer buttons.
    let hint = text(commands, "[W/S] SELECT    [A/D] CHANGE    [ENTER] CONFIRM    [ESC] BACK", &ui.font, 1.6, Color::srgba(1.0, 1.0, 1.0, 0.55));
    commands.entity(hint).insert(Node { position_type: PositionType::Absolute, right: vh(5.0), bottom: vh(4.0), ..default() });
    commands.entity(root).add_child(hint);
    if page == Page::Main {
        let note = text(commands, "TITANFALL-RS  ·  RUNS FROM YOUR TITANFALL 2 INSTALL", &ui.font, 1.4, Color::srgba(1.0, 1.0, 1.0, 0.4));
        commands.entity(note).insert(Node { position_type: PositionType::Absolute, left: vh(5.0), bottom: vh(4.0), ..default() });
        commands.entity(root).add_child(note);
    }
    root
}

/// Keyboard, mouse and controller navigation; Esc pauses/unpauses during play and closes
/// sub-pages.
#[allow(clippy::too_many_arguments)]
pub fn menu_input(
    mut menu: ResMut<Menu>,
    mut game: ResMut<Game>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut input: ResMut<PlayerInput>,
    mut difficulty: ResMut<Difficulty>,
    mut volume: ResMut<MasterVolume>,
    mut quality: ResMut<Quality>,
    mut loadout: ResMut<crate::pilotweapon::LoadoutChoice>,
    mut map: ResMut<MapChoice>,
    args: Res<crate::Args>,
    mut time: ResMut<Time<Virtual>>,
    mode: Res<CameraMode>,
    buttons: Query<(&MenuButton, &Interaction), Changed<Interaction>>,
    mut enemies: Query<&mut crate::targets::Enemy>,
    (mut exit, mut pilot_status, ability_defs, mut settings, mut titan_kit, gamepads, strings, tracks): (
        MessageWriter<AppExit>,
        ResMut<crate::pilotability::PilotStatus>,
        Option<Res<crate::pilotability::PilotAbilityDefs>>,
        ResMut<crate::settings::Settings>,
        ResMut<crate::weapons::TitanKit>,
        Query<&Gamepad>,
        Res<crate::pilotweapon::Strings>,
        Query<(&SliderTrack, &bevy::ui::RelativeCursorPosition)>,
    ),
) {
    // Rebinding: the next key or mouse button (after the click that started it) is the new
    // binding; Esc cancels.
    if let Some((cmd, armed)) = menu.capture {
        if !armed {
            menu.capture = Some((cmd, true));
            return;
        }
        if keys.just_pressed(KeyCode::Escape) || input.menu_nav.take() == Some(5) {
            menu.capture = None;
            return;
        }
        let key = keys.get_just_pressed().next().map(|k| crate::settings::Bind::Key(*k));
        let btn = mouse.get_just_pressed().next().map(|m| crate::settings::Bind::Mouse(*m));
        if let Some(b) = key.or(btn) {
            settings.rebind(cmd, b);
            menu.capture = None;
        }
        return;
    }
    let pad = gamepads.iter().next();
    let pad_hit = |b: GamepadButton| pad.is_some_and(|g| g.just_pressed(b));
    let nav = std::mem::take(&mut input.menu_nav);
    let back_key = keys.just_pressed(KeyCode::Escape) || pad_hit(GamepadButton::East) || nav == Some(5);
    // Esc closes an open sub-page first.
    if back_key && !menu.stack.is_empty() {
        if let Some((_, focus)) = menu.stack.pop() {
            // Restore the parent's focus once it is rebuilt.
            menu.restore = Some(focus);
        }
        return;
    }
    // Esc toggles the pause menu while playing (the free camera keeps Esc for the cursor).
    let pause_key = (keys.just_pressed(KeyCode::Escape) && *mode != CameraMode::Free) || pad_hit(GamepadButton::Start) || std::mem::take(&mut input.pause);
    if pause_key && game.in_play() {
        menu.paused = !menu.paused;
        if menu.paused { time.pause() } else { time.unpause() }
        return;
    }
    let list = actions(menu.page, menu.missions);
    if list.is_empty() {
        return;
    }
    let mut activate = None;
    let mut change = 0i32;
    if keys.just_pressed(KeyCode::KeyW) || keys.just_pressed(KeyCode::ArrowUp) || pad_hit(GamepadButton::DPadUp) || nav == Some(0) {
        menu.focus = (menu.focus + list.len() - 1) % list.len();
    }
    if keys.just_pressed(KeyCode::KeyS) || keys.just_pressed(KeyCode::ArrowDown) || pad_hit(GamepadButton::DPadDown) || nav == Some(1) {
        menu.focus = (menu.focus + 1) % list.len();
    }
    if keys.just_pressed(KeyCode::KeyA) || keys.just_pressed(KeyCode::ArrowLeft) || pad_hit(GamepadButton::DPadLeft) || nav == Some(2) {
        change = -1;
    }
    if keys.just_pressed(KeyCode::KeyD) || keys.just_pressed(KeyCode::ArrowRight) || pad_hit(GamepadButton::DPadRight) || nav == Some(3) {
        change = 1;
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || pad_hit(GamepadButton::South) || nav == Some(4) {
        activate = Some(list[menu.focus.min(list.len() - 1)]);
    }
    for (b, i) in &buttons {
        match i {
            Interaction::Hovered => menu.focus = b.index,
            Interaction::Pressed => {
                menu.focus = b.index;
                activate = Some(b.action);
            }
            Interaction::None => {}
        }
    }
    // Sliders follow the mouse while the button is held on their track.
    let mut on_track = false;
    if mouse.pressed(MouseButton::Left) {
        for (t, rel) in &tracks {
            if mouse.just_pressed(MouseButton::Left) && rel.cursor_over {
                menu.dragging = Some(t.0);
            }
            if let (true, Some(p)) = (menu.dragging == Some(t.0), rel.normalized) {
                slider_set(t.0, p.x + 0.5, &mut settings, &mut volume);
                on_track = true;
            }
        }
    } else {
        menu.dragging = None;
    }
    let focused = list[menu.focus.min(list.len() - 1)];
    // Options change with left/right or cycle on click (right-click goes back); clicking a
    // slider's label only focuses it.
    if change == 0 && activate.is_some_and(Action::is_option) {
        let slider = slider_get(focused, &settings, &volume).is_some();
        change = if slider || on_track { 0 } else { 1 };
        activate = None;
    }
    if change == 0 && mouse.just_pressed(MouseButton::Right) && focused.is_option() && slider_get(focused, &settings, &volume).is_none() {
        change = -1;
    }
    if change != 0 {
        let step = |v: usize, n: usize| (v as i32 + change).rem_euclid(n as i32) as usize;
        match focused {
            Action::Difficulty => {
                let all = [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard, Difficulty::Master];
                let i = all.iter().position(|d| *d == *difficulty).unwrap_or(1);
                *difficulty = all[step(i, 4)];
            }
            Action::Volume => {
                let v = ((volume.volume.to_linear() * 20.0).round() + change as f32).clamp(0.0, 20.0) / 20.0;
                volume.volume = Volume::Linear(v);
            }
            Action::Map => {
                let i = MAPS.iter().position(|m| *m == map.0).unwrap_or(0);
                map.0 = MAPS[step(i, MAPS.len())].to_string();
            }
            Action::Primary => loadout.cycle(crate::pilotweapon::Slot::Primary, change),
            Action::Sidearm => loadout.cycle(crate::pilotweapon::Slot::Sidearm, change),
            Action::AntiTitan => loadout.cycle(crate::pilotweapon::Slot::AntiTitan, change),
            Action::Tactical => {
                pilot_status.tactical = step(pilot_status.tactical, crate::pilotability::TACTICALS.len());
                if let Some(d) = ability_defs.as_ref() {
                    pilot_status.refill(d);
                }
            }
            Action::Ordnance => {
                pilot_status.ordnance = step(pilot_status.ordnance, crate::pilotability::ORDNANCE.len());
                pilot_status.satchels = 0;
                if let Some(d) = ability_defs.as_ref() {
                    pilot_status.refill(d);
                }
            }
            Action::TitanLoadout => titan_kit.0 = step(titan_kit.0, crate::weapons::TITAN_ARSENAL.len()),
            Action::Graphics => {
                let all = [Quality::Low, Quality::Medium, Quality::High];
                let i = all.iter().position(|q| *q == *quality).unwrap_or(2);
                *quality = all[step(i, 3)];
            }
            // The game's slider steps (controls.menu, video.menu).
            Action::MouseSens => settings.mouse_sensitivity = ((settings.mouse_sensitivity + 0.2 * change as f32) * 5.0).round().clamp(0.0, 100.0) / 5.0,
            Action::AdsSens => settings.mouse_sensitivity_zoomed = ((settings.mouse_sensitivity_zoomed + 0.2 * change as f32) * 5.0).round().clamp(0.0, 100.0) / 5.0,
            Action::Fov => settings.fov_scale = (1.0 + (((settings.fov_scale - 1.0) / 0.0275).round() + change as f32).clamp(0.0, 20.0) * 0.0275).min(1.55),
            Action::InvertY => settings.invert_y = !settings.invert_y,
            Action::PadLook => settings.gamepad_look = step(settings.gamepad_look, 8),
            Action::PadLookAds => settings.gamepad_look_ads = step(settings.gamepad_look_ads, 8),
            Action::PadCurve => settings.gamepad_curve = step(settings.gamepad_curve, crate::settings::LOOK_CURVES.len()),
            Action::PadInvert => settings.gamepad_invert_y = !settings.gamepad_invert_y,
            Action::AimAssist => settings.aim_assist = !settings.aim_assist,
            Action::PadLayout => {
                let all = crate::settings::PAD_LAYOUTS;
                let i = all.iter().position(|l| *l == settings.gamepad_layout).unwrap_or(0);
                settings.gamepad_layout = all[step(i, all.len())].to_string();
            }
            _ => {}
        }
    }
    let Some(a) = activate else { return };
    log::info!("menu: {a:?}");
    let open = |menu: &mut Menu, p: Page| {
        let f = menu.focus;
        menu.stack.push((p, f));
    };
    match a {
        Action::Play if map.0 != args.map => {
            relaunch(&args, &map.0, *difficulty, volume.volume.to_linear(), &loadout, titan_kit.0);
            exit.write(AppExit::Success);
        }
        Action::Mission(i) => {
            if let Some((_, m)) = missions(&strings).get(i) {
                relaunch(&args, m, *difficulty, volume.volume.to_linear(), &loadout, titan_kit.0);
                exit.write(AppExit::Success);
            }
        }
        Action::Play | Action::Restart => {
            menu.paused = false;
            menu.stack.clear();
            time.unpause();
            // The director starts a fresh run from the title or game-over state.
            game.state = GameState::GameOver;
            input.start = true;
        }
        Action::Resume => {
            menu.paused = false;
            time.unpause();
        }
        Action::Campaign => open(&mut menu, Page::Missions),
        Action::Loadout => open(&mut menu, Page::Loadout),
        Action::Settings => open(&mut menu, Page::Settings),
        Action::Bindings => open(&mut menu, Page::Bindings),
        Action::Bind(i) => {
            if let Some(c) = crate::settings::Command::ALL.get(i) {
                menu.capture = Some((*c, false));
            }
        }
        Action::ResetBinds => {
            settings.binds = crate::settings::Settings::default().binds;
        }
        Action::Back => {
            if let Some((_, focus)) = menu.stack.pop() {
                menu.restore = Some(focus);
            }
        }
        Action::MainMenu => {
            menu.paused = false;
            menu.stack.clear();
            time.unpause();
            game.state = GameState::Title;
            for mut e in &mut enemies {
                e.active = false;
            }
        }
        Action::Quit => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

/// Focus highlight, live option labels and loadout icons.
#[allow(clippy::too_many_arguments)]
pub fn menu_visuals(
    menu: Res<Menu>,
    difficulty: Res<Difficulty>,
    volume: Res<MasterVolume>,
    quality: Res<Quality>,
    loadout: Res<crate::pilotweapon::LoadoutChoice>,
    strings: Res<crate::pilotweapon::Strings>,
    map: Res<MapChoice>,
    pilot_status: Res<crate::pilotability::PilotStatus>,
    (settings, titan_kit, ui): (Res<crate::settings::Settings>, Res<crate::weapons::TitanKit>, Res<UiAssets>),
    mut buttons: Query<(&MenuButton, &Children, &mut BackgroundColor)>,
    mut bars: Query<&mut BackgroundColor, (With<FocusBar>, Without<MenuButton>)>,
    mut labels: Query<(&mut Text, &mut TextColor), With<MenuLabel>>,
    mut icons: Query<&mut ImageNode, With<MenuIcon>>,
    mut fills: Query<(&SliderFill, &mut Node)>,
) {
    use crate::pilotweapon::{Slot, ARSENAL};
    for (f, mut node) in &mut fills {
        let w = percent(slider_get(f.0, &settings, &volume).unwrap_or(0.0).clamp(0.0, 1.0) * 100.0);
        if node.width != w {
            node.width = w;
        }
    }
    let mission_list = missions(&strings);
    for (b, children, mut bg) in &mut buttons {
        let focused = b.index == menu.focus;
        bg.0 = if focused { Color::srgba(1.0, 1.0, 1.0, 0.14) } else { Color::srgba(0.0, 0.0, 0.0, 0.25) };
        let gun_id = |slot: Slot| ARSENAL[loadout.get(slot)].id;
        let gun = |slot: Slot, title: &str| {
            let id = gun_id(slot);
            let name = strings.0.get(id).cloned().unwrap_or_else(|| id.trim_start_matches("mp_weapon_").to_uppercase());
            opt(title, name.to_uppercase())
        };
        let titan = &crate::weapons::TITAN_ARSENAL[titan_kit.0.min(crate::weapons::TITAN_ARSENAL.len() - 1)];
        let icon_path: Option<String> = match b.action {
            Action::Primary => ui.weapon_icons.get(gun_id(Slot::Primary)).cloned(),
            Action::Sidearm => ui.weapon_icons.get(gun_id(Slot::Sidearm)).cloned(),
            Action::AntiTitan => ui.weapon_icons.get(gun_id(Slot::AntiTitan)).cloned(),
            Action::Tactical => Some(crate::pilotability::TACTICALS[pilot_status.tactical].2.to_string()),
            Action::Ordnance => Some(crate::pilotability::ORDNANCE[pilot_status.ordnance].2.to_string()),
            Action::TitanLoadout => ui.weapon_icons.get(titan.id).cloned(),
            _ => None,
        };
        let capturing = menu.capture.is_some() && matches!(b.action, Action::Bind(i) if menu.capture.map(|c| c.0) == crate::settings::Command::ALL.get(i).copied());
        for c in children.iter() {
            if let Ok(mut bar) = bars.get_mut(c) {
                bar.0 = if focused { Color::srgb(1.0, 0.62, 0.15) } else { Color::NONE };
            }
            if let (Ok(mut img), Some(path)) = (icons.get_mut(c), icon_path.as_ref()) {
                if let Some(new) = ui.image(path) {
                    if img.rect != new.rect || img.image != new.image {
                        *img = new;
                    }
                }
            }
            if let Ok((mut t, mut col)) = labels.get_mut(c) {
                let s = match b.action {
                    Action::Play => "PLAY".into(),
                    Action::Resume => "RESUME".into(),
                    Action::Restart => "RESTART".into(),
                    Action::Campaign => "CAMPAIGN".into(),
                    Action::Loadout => "LOADOUT".into(),
                    Action::Settings => "SETTINGS".into(),
                    Action::Bindings => "KEY BINDINGS".into(),
                    Action::ResetBinds => "RESTORE DEFAULTS".into(),
                    Action::Back => "BACK".into(),
                    Action::MainMenu => "QUIT TO MAIN MENU".into(),
                    Action::Quit => "QUIT GAME".into(),
                    Action::Difficulty => opt("DIFFICULTY", difficulty.name()),
                    Action::Volume => opt("MASTER VOLUME", format!("{:.0}%", volume.volume.to_linear() * 100.0)),
                    Action::Graphics => opt("GRAPHICS", quality.name()),
                    Action::Primary => gun(Slot::Primary, "PRIMARY"),
                    Action::Sidearm => gun(Slot::Sidearm, "SIDEARM"),
                    Action::AntiTitan => gun(Slot::AntiTitan, "ANTI-TITAN"),
                    Action::Tactical => opt("TACTICAL", crate::pilotability::TACTICALS[pilot_status.tactical].3),
                    Action::Ordnance => opt("ORDNANCE", crate::pilotability::ORDNANCE[pilot_status.ordnance].3),
                    Action::TitanLoadout => opt("TITAN LOADOUT", titan.kit.to_uppercase()),
                    Action::Map => opt("MAP", map_name(&strings, &map.0)),
                    Action::Mission(i) => mission_list.get(i).map(|(n, _)| n.to_uppercase()).unwrap_or_default(),
                    Action::Bind(i) => {
                        let c = crate::settings::Command::ALL[i];
                        let key = if capturing { "PRESS A KEY".to_string() } else { settings.bind(c).name() };
                        format!("{:<28}{key}", c.label())
                    }
                    Action::MouseSens => opt("MOUSE SENSITIVITY", format!("{:.1}", settings.mouse_sensitivity)),
                    Action::AdsSens => opt("ADS MOUSE SENSITIVITY", format!("{:.1}", settings.mouse_sensitivity_zoomed)),
                    Action::InvertY => opt("INVERT MOUSE", on_off(settings.invert_y)),
                    Action::Fov => opt("FIELD OF VIEW", format!("{:.0}", crate::settings::BASE_FOV_DEG * settings.fov_scale)),
                    Action::PadLook => opt("CONTROLLER LOOK SENSITIVITY", settings.gamepad_look + 1),
                    Action::PadLookAds => opt("CONTROLLER ADS SENSITIVITY", settings.gamepad_look_ads + 1),
                    Action::PadCurve => opt("CONTROLLER LOOK CURVE", crate::settings::LOOK_CURVES[settings.gamepad_curve.min(4)]),
                    Action::PadInvert => opt("CONTROLLER INVERT LOOK", on_off(settings.gamepad_invert_y)),
                    Action::AimAssist => opt("AIM ASSIST", on_off(settings.aim_assist)),
                    Action::PadLayout => opt("BUTTON LAYOUT", settings.gamepad_layout.replace('_', " ").to_uppercase()),
                };
                if t.0 != s {
                    t.0 = s;
                }
                col.0 = if focused { Color::WHITE } else { Color::srgba(1.0, 1.0, 1.0, 0.7) };
            }
        }
    }
}

/// Load the focused mission's load screen on the Missions page.
pub fn mission_images(
    menu: Res<Menu>,
    args: Res<crate::Args>,
    strings: Res<crate::pilotweapon::Strings>,
    mut cache: ResMut<MissionImages>,
    mut images: ResMut<Assets<Image>>,
    mut nodes: Query<&mut ImageNode, With<MissionImage>>,
) {
    if menu.page != Page::Missions {
        return;
    }
    let Some((_, m)) = missions(&strings).get(menu.focus).cloned() else { return };
    let paks = std::path::Path::new(&args.game).join("r2/paks/Win64");
    let handle = cache
        .0
        .entry(m.to_string())
        .or_insert_with(|| {
            let pak = tf_assets::rpak::resolve(&paks, &format!("{m}_loadscreen.rpak")).and_then(|p| Rpak::open(p).ok())?;
            let guid = pak.assets.iter().find(|a| a.kind_str() == "txtr")?.guid;
            upload(&mut images, &pak, guid)
        })
        .clone();
    for mut n in &mut nodes {
        match &handle {
            Some(h) if n.image != *h => *n = ImageNode::new(h.clone()),
            None if n.image != Handle::default() => *n = ImageNode::default(),
            _ => {}
        }
    }
}

/// Keep the saved master volume in step with the game's (menu or - / = keys), except in
/// scripted test runs or when `--volume` was given.
pub fn sync_volume_setting(volume: Res<MasterVolume>, args: Res<crate::Args>, script: Res<crate::player::Script>, mut settings: ResMut<crate::settings::Settings>) {
    if !volume.is_changed() || args.volume.is_some() || script.enabled {
        return;
    }
    let v = volume.volume.to_linear();
    if (settings.volume - v).abs() > 1e-4 {
        settings.volume = v;
    }
}

// ---------------------------------------------------------------------------------------------
// Subtitles

/// Subtitles at the bottom of the screen, like the game's: speaker name and line. Other code
/// pushes lines with `Subtitles::push`.
#[derive(Resource, Default)]
pub struct Subtitles {
    lines: Vec<(String, String, f32)>,
    root: Option<Entity>,
    shown: String,
}

impl Subtitles {
    /// Show `text` (said by `speaker`, may be empty) for `secs` seconds.
    pub fn push(&mut self, speaker: &str, text: &str, secs: f32) {
        self.lines.retain(|l| l.1 != text);
        self.lines.push((speaker.to_uppercase(), text.to_string(), secs));
        if self.lines.len() > 3 {
            self.lines.remove(0);
        }
    }
}

#[derive(Component)]
pub struct SubtitleText;

pub fn update_subtitles(mut commands: Commands, time: Res<Time>, ui: Res<UiAssets>, mut subs: ResMut<Subtitles>, mut texts: Query<&mut Text, With<SubtitleText>>, mut vis: Query<&mut Visibility>) {
    let dt = time.delta_secs();
    subs.lines.retain_mut(|l| {
        l.2 -= dt;
        l.2 > 0.0
    });
    let root = match subs.root {
        Some(r) => r,
        None => {
            let r = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(20),
                        right: percent(20),
                        bottom: vh(15.0),
                        justify_content: JustifyContent::Center,
                        padding: UiRect::axes(vh(1.4), vh(0.7)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                    GlobalZIndex(40),
                    Visibility::Hidden,
                ))
                .id();
            let t = commands
                .spawn((Text::new(""), TextFont { font: ui.font.clone(), font_size: 20.0, ..default() }, TextColor(Color::WHITE), TextLayout::new_with_justify(Justify::Center), VhText(2.0), SubtitleText))
                .id();
            commands.entity(r).add_child(t);
            subs.root = Some(r);
            return;
        }
    };
    let want: String = subs.lines.iter().map(|(s, t, _)| if s.is_empty() { t.clone() } else { format!("{s}: {t}") }).collect::<Vec<_>>().join("\n");
    if want != subs.shown {
        for mut t in &mut texts {
            t.0 = want.clone();
        }
        if let Ok(mut v) = vis.get_mut(root) {
            *v = if want.is_empty() { Visibility::Hidden } else { Visibility::Inherited };
        }
        subs.shown = want;
    }
}

/// Graphics quality: what the optional effects cost.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Quality {
    Low,
    Medium,
    #[default]
    High,
}

impl Quality {
    fn name(self) -> &'static str {
        match self {
            Quality::Low => "LOW",
            Quality::Medium => "MEDIUM",
            Quality::High => "HIGH",
        }
    }
}

/// High: SSAO, bloom, 4096 shadows. Medium: no SSAO. Low: no SSAO or bloom, 1024 shadows.
pub fn apply_quality(
    mut commands: Commands,
    quality: Res<Quality>,
    cams: Query<Entity, With<crate::player::MainCamera>>,
    mut shadow_map: ResMut<bevy::light::DirectionalLightShadowMap>,
) {
    if !quality.is_changed() {
        return;
    }
    for cam in &cams {
        let mut e = commands.entity(cam);
        match *quality {
            Quality::High => {
                e.insert(bevy::pbr::ScreenSpaceAmbientOcclusion::default());
            }
            _ => {
                e.remove::<bevy::pbr::ScreenSpaceAmbientOcclusion>();
            }
        }
        match *quality {
            Quality::Low => {
                e.remove::<bevy::post_process::bloom::Bloom>();
            }
            _ => {
                e.insert(bevy::post_process::bloom::Bloom { intensity: 0.05, ..bevy::post_process::bloom::Bloom::NATURAL });
            }
        }
    }
    shadow_map.size = match *quality {
        Quality::High => 4096,
        Quality::Medium => 2048,
        Quality::Low => 1024,
    };
    log::info!("graphics quality {:?}", *quality);
}
