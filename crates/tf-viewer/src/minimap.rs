//! The minimap at the top left, as the game draws it: the map's overview image
//! (`resource/overviews/<map>.txt`: the image, the world position of its top-left corner, its
//! units per pixel and threatMaxDist, the radius it shows) rotated so the player's heading is
//! up, in a circle, with the player's arrow at the centre and BT and the enemies as the game's
//! compass icons (clamped to the rim when further than the radius, like its `_clamped` icons).

use crate::gamedata::GameData;
use crate::pilotctl::Control;
use crate::player::{MainCamera, PlayerTitan};
use crate::targets::Enemy;
use crate::ui::UiAssets;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

/// The circle's centre and diameter (vh of a 16:9 frame), measured off the game's HUD.
const CENTRE: (f32, f32) = (22.5, 15.0);
const DIAMETER: f32 = 22.5;
const ICON: f32 = 2.0;
const ENEMY_ICONS: usize = 16;
/// The game shows at least this radius (cl_minimap.gnut: max(threatMaxDist, 1800)).
const MIN_RADIUS: f32 = 1800.0;

#[derive(Resource)]
pub struct Minimap {
    pub image: Handle<Image>,
    /// World xy of the image's top-left corner.
    pub corner: Vec2,
    /// Game units across the whole image.
    pub extent: f32,
    /// Game units from the centre to the rim.
    pub radius: f32,
}

impl Minimap {
    /// The map's overview, if it has one (the flat map has none).
    pub fn load(gd: &GameData, map: &str, cache: &mut crate::convert::Cache, images: &mut Assets<Image>) -> Option<Self> {
        let text = match gd.read_file(&format!("resource/overviews/{map}.txt")) {
            Ok(t) => t,
            Err(e) => {
                log::warn!("no minimap: {e:#}");
                return None;
            }
        };
        let text = String::from_utf8_lossy(&text);
        let key = |k: &str| text.lines().find_map(|l| l.trim().strip_prefix(k).filter(|r| r.starts_with(char::is_whitespace)).map(|r| r.trim().to_string()));
        let num = |k: &str| key(k).and_then(|v| v.parse::<f32>().ok());
        let image_name = key("minimap")?;
        let Some((pi, guid)) = gd.texture_by_name(&image_name) else {
            log::warn!("minimap image {image_name} not in any rpak");
            return None;
        };
        let Some(image) = crate::convert::image(gd, cache, images, pi, guid) else {
            log::warn!("minimap image {image_name} failed to load");
            return None;
        };
        let size = images.get(&image).map(|i| i.width() as f32).unwrap_or(1024.0);
        let scale = num("scale").unwrap_or(10.0);
        let radius = num("threatMaxDist").unwrap_or(1400.0).max(MIN_RADIUS);
        log::info!("minimap {image_name}: corner ({}, {}), {scale} units/px, radius {radius}", num("pos_x").unwrap_or(0.0), num("pos_y").unwrap_or(0.0));
        Some(Self { image, corner: Vec2::new(num("pos_x").unwrap_or(0.0), num("pos_y").unwrap_or(0.0)), extent: scale * size, radius })
    }
}

#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub struct MinimapMaterial {
    /// The player's uv in the image, and the heading (cos, sin) in game xy.
    #[uniform(0)]
    pub params: Vec4,
    /// The radius as a fraction of the image, the image's alpha.
    #[uniform(1)]
    pub extent: Vec4,
    #[texture(2)]
    #[sampler(3)]
    pub map: Handle<Image>,
}

impl UiMaterial for MinimapMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://tf_viewer/minimap.wgsl".into()
    }
}

#[derive(Component)]
pub struct MinimapNode;
#[derive(Component)]
pub struct MinimapRim;
/// The rim's thickness (vh).
const RIM: f32 = 0.55;
/// A compass letter on the rim: its world bearing in radians (0 = +X east).
#[derive(Component)]
pub struct Compass(f32, bool);

#[derive(Component)]
pub enum Icon {
    You,
    Titan,
    Enemy(usize),
}

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "minimap.wgsl");
        app.add_plugins(UiMaterialPlugin::<MinimapMaterial>::default());
    }
}

fn vh(v: f32) -> Val {
    Val::Vh(v)
}

/// Spawn the minimap under the HUD root once the map's overview is known, then keep it on the
/// player: the material's heading and position, the icons' places.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_minimap(
    mut commands: Commands,
    minimap: Option<Res<Minimap>>,
    ui: Res<UiAssets>,
    control: Res<Control>,
    mut materials: ResMut<Assets<MinimapMaterial>>,
    hud: Query<Entity, With<crate::hud::HudRoot>>,
    camera: Query<&GlobalTransform, With<MainCamera>>,
    titans: Query<(&PlayerTitan, &crate::combat::TitanHealth)>,
    enemies: Query<&Enemy>,
    mut nodes: Query<(&MaterialNode<MinimapMaterial>, &mut Visibility), (With<MinimapNode>, Without<Icon>, Without<MinimapRim>)>,
    mut rims: Query<&mut Visibility, (With<MinimapRim>, Without<MinimapNode>, Without<Icon>)>,
    mut icons: Query<(&Icon, &mut Node, &mut Visibility, &mut UiTransform), (Without<MinimapNode>, Without<MinimapRim>)>,
    mut letters: Query<(&Compass, &mut Node), (Without<Icon>, Without<MinimapNode>, Without<MinimapRim>)>,
) {
    let Some(minimap) = minimap else { return };
    let Ok(hud) = hud.single() else { return };
    if nodes.is_empty() {
        let circle = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: vh(CENTRE.0 - DIAMETER * 0.5),
                    top: vh(CENTRE.1 - DIAMETER * 0.5),
                    width: vh(DIAMETER),
                    height: vh(DIAMETER),
                    ..default()
                },
                MaterialNode(materials.add(MinimapMaterial { params: Vec4::new(0.5, 0.5, 1.0, 0.0), extent: Vec4::new(minimap.radius / minimap.extent, 1.0, 0.0, 0.0), map: minimap.image.clone() })),
                MinimapNode,
            ))
            .id();
        commands.entity(hud).add_child(circle);
        // The dark rim and the compass letters on it (the game's minimap_base: N/E/S/W large,
        // the half-winds small), turning with the heading.
        let rim = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: vh(CENTRE.0 - DIAMETER * 0.5 - RIM),
                    top: vh(CENTRE.1 - DIAMETER * 0.5 - RIM),
                    width: vh(DIAMETER + RIM * 2.0),
                    height: vh(DIAMETER + RIM * 2.0),
                    border: UiRect::all(vh(RIM)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.8)),
                MinimapRim,
            ))
            .id();
        commands.entity(hud).add_child(rim);
        for (i, name) in ["E", "NE", "N", "NW", "W", "SW", "S", "SE"].into_iter().enumerate() {
            let big = i % 2 == 0;
            let e = commands
                .spawn((
                    Node { position_type: PositionType::Absolute, left: percent(50), top: percent(50), ..default() },
                    Text::new(name),
                    TextFont { font: ui.bold_font.clone(), font_size: 16.0, ..default() },
                    TextColor(Color::srgba(1.0, 1.0, 1.0, if big { 0.9 } else { 0.6 })),
                    crate::ui::VhText(if big { 1.7 } else { 1.1 }),
                    Compass(i as f32 * std::f32::consts::FRAC_PI_4, big),
                ))
                .id();
            commands.entity(circle).add_child(e);
        }
        let mut icon = |kind: Icon, path: &str, color: Color| {
            let mut img = ui.image(path).unwrap_or_default();
            img.color = color;
            let e = commands
                .spawn((
                    Node { position_type: PositionType::Absolute, left: percent(50), top: percent(50), width: vh(ICON), height: vh(ICON), margin: UiRect::all(vh(-ICON * 0.5)), ..default() },
                    img,
                    UiTransform::IDENTITY,
                    Visibility::Hidden,
                    kind,
                ))
                .id();
            commands.entity(circle).add_child(e);
        };
        for i in 0..ENEMY_ICONS {
            icon(Icon::Enemy(i), "rui/hud/minimap/compass_icon_enemy_titan", Color::srgb(1.0, 0.3, 0.2));
        }
        icon(Icon::Titan, "rui/hud/minimap/compass_icon_your_titan", Color::srgb(0.45, 0.8, 1.0));
        icon(Icon::You, "rui/hud/minimap/compass_icon_you", Color::WHITE);
        return;
    }
    let Ok(cam) = camera.single() else { return };
    let pos = crate::player::from_bevy(cam.translation());
    let fwd = crate::player::from_bevy(cam.forward().as_vec3()).truncate().normalize_or(Vec2::X);
    let right = Vec2::new(fwd.y, -fwd.x);
    // The cockpit has no minimap.
    let shown = if *control == Control::Titan { Visibility::Hidden } else { Visibility::Inherited };
    for mut vis in &mut rims {
        vis.set_if_neq(shown);
    }
    for (node, mut vis) in &mut nodes {
        vis.set_if_neq(shown);
        if let Some(m) = materials.get_mut(&node.0) {
            let uv = Vec2::new((pos.x - minimap.corner.x) / minimap.extent, (minimap.corner.y - pos.y) / minimap.extent);
            let params = Vec4::new(uv.x, uv.y, fwd.x, fwd.y);
            if m.params != params {
                m.params = params;
            }
        }
    }
    // An icon's place in the circle for a world position: screen right and up in vh, clamped
    // to the rim.
    let place = |p: Vec3| -> (f32, f32, bool) {
        let d = (p - pos).truncate();
        let mut sx = d.dot(right) / minimap.radius;
        let mut sy = d.dot(fwd) / minimap.radius;
        let len = (sx * sx + sy * sy).sqrt();
        let clamped = len > 0.95;
        if clamped {
            sx *= 0.95 / len;
            sy *= 0.95 / len;
        }
        (sx * DIAMETER * 0.5, -sy * DIAMETER * 0.5, clamped)
    };
    // Compass letters: a bearing's screen angle is its angle from the heading, clockwise.
    let heading = fwd.y.atan2(fwd.x);
    for (c, mut node) in &mut letters {
        // Yaw grows anticlockwise (east 0, north 90), so a bearing right of the heading has
        // a smaller angle.
        let a = heading - c.0;
        let r = DIAMETER * 0.5 - if c.1 { 1.3 } else { 1.0 };
        let (x, y) = (a.sin() * r, -a.cos() * r);
        let half = if c.1 { 0.6 } else { 0.7 };
        crate::hud::set(&mut node, |n| &mut n.left, vh(DIAMETER * 0.5 + x - half));
        crate::hud::set(&mut node, |n| &mut n.top, vh(DIAMETER * 0.5 + y - half));
    }
    let titan = titans.single().ok();
    let enemies: Vec<&Enemy> = enemies.iter().filter(|e| e.alive() && !e.infantry).collect();
    for (icon, mut node, mut vis, mut tf) in &mut icons {
        let (shown, x, y, yaw) = match icon {
            Icon::You => (true, 0.0, 0.0, 0.0),
            Icon::Titan => match titan {
                Some((t, h)) if *control != Control::Titan && h.dead_for.is_none() => {
                    let (x, y, _) = place(t.state.pos);
                    (true, x, y, t.state.yaw)
                }
                _ => (false, 0.0, 0.0, 0.0),
            },
            Icon::Enemy(i) => match enemies.get(*i) {
                Some(e) => {
                    let (x, y, _) = place(e.pos);
                    (true, x, y, e.state.yaw)
                }
                None => (false, 0.0, 0.0, 0.0),
            },
        };
        vis.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
        if !shown {
            continue;
        }
        crate::hud::set(&mut node, |n| &mut n.left, vh(DIAMETER * 0.5 + x));
        crate::hud::set(&mut node, |n| &mut n.top, vh(DIAMETER * 0.5 + y));
        // Icons point the way their owner faces (yaw in radians), relative to the player's
        // heading (up).
        let heading = fwd.y.atan2(fwd.x);
        let rot = if matches!(icon, Icon::You) { Rot2::IDENTITY } else { Rot2::radians(heading - yaw) };
        if tf.rotation != rot {
            tf.rotation = rot;
        }
    }
}
