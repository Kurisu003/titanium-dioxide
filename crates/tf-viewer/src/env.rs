//! The map's lighting environment from `maps/<map>_env.ent`: the sun (`light_environment`),
//! its ambient term, and the world fog (`env_fog_controller`). The baked lightmaps carry the
//! indirect light for world geometry; the sun is drawn as a real-time shadowed light.

use crate::gamedata::GameData;
use bevy::prelude::*;

/// Camera exposure (EV100) for every camera. It only sets the unit: light is given in the
/// game's own linear units times [`nits_for_white`], so after Bevy's exposure the HDR picture
/// holds the values the game's shaders write, and the game's auto exposure and tonemapping
/// (colorcorr.rs) take it from there. TF_EV100 overrides it.
pub fn ev100() -> f32 {
    static EV: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *EV.get_or_init(|| {
        std::env::var("TF_EV100")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(12.0)
    })
}

/// Bevy's tonemapping, off on every camera: the game's own exposure, tonemapping and colour
/// correction run once, on the camera that draws last (colorcorr.rs). With several cameras
/// drawing into the same textures, each one's tonemapping would grade the picture again.
pub fn tonemapping() -> bevy::core_pipeline::tonemapping::Tonemapping {
    bevy::core_pipeline::tonemapping::Tonemapping::None
}

/// One unit of the game's linear light in Bevy's units (the inverse of the camera exposure):
/// emissive luminance, lightmap and irradiance intensities, and (times pi) illuminance.
pub fn nits_for_white() -> f32 {
    1.2 * 2f32.powf(ev100())
}

#[derive(Clone, Debug)]
pub struct MapEnv {
    /// Direction the sunlight travels, in game space (Z up).
    pub sun_dir: Vec3,
    /// The sun (`c_sunColor`) and the sky ambient (`c_skyColor`) in the game's linear units,
    /// as the map compiler stores them in the BSP's world lights: `(colour / 255)^2.2 *
    /// brightness / 255` of `_light` and `_ambient` (Forwardbase Kodai: sun (8.0, 6.81,
    /// 5.27), sky (1.05, 1.21, 1.30)). engine.dll hands them to the shaders scaled by
    /// mat_sun_scale / mat_sky_scale (1).
    pub sun_light: Vec3,
    pub sky_light: Vec3,
    pub fog: Option<Fog>,
    /// The 3D skybox camera (sky_camera) for the playable level, game space.
    pub sky_camera: Option<Vec3>,
    /// sky_camera "skyscale": how much bigger the skybox scene is than authored.
    pub sky_scale: f32,
    /// The map's `color_correction` entity's lookup table (`materials/correction/*.raw`),
    /// applied over the whole picture (every shipped map has one, e.g. Kodai's sp_beacon.raw).
    pub color_correction: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Fog {
    pub color: Color,
    /// Max opacity (fogdensity, clamped to 0..1).
    pub density: f32,
    /// Distance (game units) at which the fog reaches half strength.
    pub half_dist: f32,
    pub dir_color: Color,
    pub dir_strength: f32,
}

impl Default for MapEnv {
    fn default() -> Self {
        Self {
            sun_dir: Vec3::new(-0.4, 0.6, -0.7).normalize(),
            sun_light: Vec3::splat(4.0),
            sky_light: Vec3::splat(0.5),
            fog: None,
            sky_camera: None,
            sky_scale: 1000.0,
            color_correction: None,
        }
    }
}

fn rgb_and_brightness(v: &str) -> (Color, f32) {
    let n: Vec<f32> = v
        .split_whitespace()
        .filter_map(|x| x.parse().ok())
        .collect();
    let c = |i: usize| n.get(i).copied().unwrap_or(255.0) / 255.0;
    (
        Color::srgb(c(0), c(1), c(2)),
        n.get(3).copied().unwrap_or(200.0),
    )
}

/// A light value ("r g b brightness") in the game's linear units, as the map compiler
/// converts it.
fn light_value(v: &str) -> Vec3 {
    let n: Vec<f32> = v
        .split_whitespace()
        .filter_map(|x| x.parse().ok())
        .collect();
    let c = |i: usize| (n.get(i).copied().unwrap_or(255.0) / 255.0).powf(2.2);
    Vec3::new(c(0), c(1), c(2)) * n.get(3).copied().unwrap_or(200.0) / 255.0
}

fn color(v: &str) -> Color {
    rgb_and_brightness(v).0
}

/// Source AngleVectors forward for pitch/yaw in degrees (positive pitch looks down).
fn forward(pitch: f32, yaw: f32) -> Vec3 {
    let (p, y) = (pitch.to_radians(), yaw.to_radians());
    Vec3::new(p.cos() * y.cos(), p.cos() * y.sin(), -p.sin())
}

impl MapEnv {
    pub fn load(gd: &GameData, map: &str) -> Self {
        let mut env = Self::default();
        let Ok(bytes) = gd.read_file(&format!("maps/{map}_env.ent")) else {
            log::warn!("no {map}_env.ent; default lighting");
            return env;
        };
        let ents = tf_assets::bsp::parse_entities(&String::from_utf8_lossy(&bytes));
        let get = |e: &Vec<(String, String)>, k: &str| {
            e.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone())
        };
        // Several light_environments can exist (skybox, intro); the one with the highest index
        // is the playable world's in the maps checked, and it has the cloud mask.
        let mut suns: Vec<&Vec<(String, String)>> = ents
            .iter()
            .filter(|e| get(e, "classname").as_deref() == Some("light_environment"))
            .collect();
        suns.sort_by_key(|e| {
            get(e, "lightEnvironmentIndex")
                .and_then(|v| v.trim_start_matches('*').parse::<i32>().ok())
                .unwrap_or(0)
        });
        if let Some(sun) = suns.last() {
            let angles: Vec<f32> = get(sun, "angles")
                .unwrap_or_default()
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            let yaw = angles.get(1).copied().unwrap_or(0.0);
            // "pitch" is negative for a sun above the horizon; the light travels downwards.
            let pitch = get(sun, "pitch")
                .and_then(|v| v.parse::<f32>().ok())
                .map(|p| -p)
                .or(angles.first().copied())
                .unwrap_or(45.0);
            // The angles are the direction the light travels: the map compiler stores exactly
            // this as the sun's world-light normal (Forwardbase Kodai: (-0.335, 0.703, -0.628)),
            // and engine.dll hands the shaders its negation as `c_sunDir`, which they dot
            // with the surface normal (so it points at the sun: its z is positive).
            env.sun_dir = forward(pitch, yaw).normalize();
            if let Some(l) = get(sun, "_light") {
                env.sun_light = light_value(&l);
            }
            if let Some(a) = get(sun, "_ambient") {
                env.sky_light = light_value(&a);
            }
        }
        if let Some(f) = ents
            .iter()
            .find(|e| get(e, "classname").as_deref() == Some("env_fog_controller"))
        {
            if get(f, "fogenable").as_deref() != Some("0") {
                let num = |k: &str, d: f32| get(f, k).and_then(|v| v.parse().ok()).unwrap_or(d);
                env.fog = Some(Fog {
                    color: color(&get(f, "fogcolor").unwrap_or_default()),
                    density: num("fogdensity", 0.5).clamp(0.0, 1.0),
                    half_dist: num("foghalfdisttop", 20000.0).max(100.0),
                    dir_color: color(&get(f, "fogdircolor").unwrap_or_default()),
                    dir_strength: num("fogdircolorstrength", 0.0),
                });
            }
        }
        // The colour correction: the first enabled, non-radial one (maxfalloff -1 means
        // everywhere; the maps checked have exactly one).
        env.color_correction = ents
            .iter()
            .filter(|e| {
                get(e, "classname").as_deref() == Some("color_correction")
                    && get(e, "StartDisabled").as_deref() != Some("1")
            })
            .find_map(|e| get(e, "filename"))
            .filter(|f| !f.is_empty());
        // Maps can have several sky cameras (intro, level); prefer the level's, else the one
        // with fog enabled, else the first.
        let cams: Vec<&Vec<(String, String)>> = ents
            .iter()
            .filter(|e| get(e, "classname").as_deref() == Some("sky_camera"))
            .collect();
        let pick = cams
            .iter()
            .find(|e| get(e, "targetname").is_some_and(|n| n.contains("level")))
            .or_else(|| {
                cams.iter()
                    .find(|e| get(e, "fogenable").as_deref() == Some("1"))
            })
            .or(cams.first());
        if let Some(sc) = pick
            .and_then(|e| get(e, "skyscale"))
            .and_then(|v| v.parse::<f32>().ok())
        {
            env.sky_scale = sc.max(1.0);
        }
        env.sky_camera = pick.and_then(|e| get(e, "origin")).map(|o| {
            let v: Vec<f32> = o
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            Vec3::new(v[0], v[1], v[2])
        });
        log::info!("map env: {env:?}");
        env
    }

    /// Bevy's sun: shines along the game-space sun direction. The world shader lights with
    /// `albedo * c_sunColor * N.L` where Bevy's diffuse is `albedo / pi * illuminance * N.L`.
    pub fn sun(&self) -> (DirectionalLight, Transform) {
        let d = self.sun_dir;
        let dir = Vec3::new(d.x, d.z, -d.y);
        let peak = self.sun_light.max_element().max(1e-6);
        let c = self.sun_light / peak;
        (
            DirectionalLight {
                illuminance: peak * std::f32::consts::PI * nits_for_white(),
                color: Color::linear_rgb(c.x, c.y, c.z),
                shadows_enabled: std::env::var_os("TF_NO_SHADOWS").is_none(),
                ..default()
            },
            Transform::default().looking_to(dir, Vec3::Y),
        )
    }

    /// Ambient fill for everything without a lightmap or probes: the sky light of an open
    /// upward-facing surface (the world shader's sky term is `c_skyColor` there).
    pub fn ambient(&self) -> GlobalAmbientLight {
        let peak = self.sky_light.max_element().max(1e-6);
        let c = self.sky_light / peak;
        GlobalAmbientLight {
            color: Color::linear_rgb(c.x, c.y, c.z),
            brightness: peak * nits_for_white(),
            affects_lightmapped_meshes: false,
        }
    }

    /// TF_NO_FOG=1 leaves it out (for calibration).
    pub fn distance_fog(&self) -> Option<DistanceFog> {
        if std::env::var_os("TF_NO_FOG").is_some() {
            return None;
        }
        let f = self.fog.as_ref()?;
        let half_m = f.half_dist * crate::player::UNIT;
        let dir = f.dir_color.to_linear() * f.dir_strength.min(1.0);
        Some(DistanceFog {
            color: f.color.with_alpha(f.density),
            directional_light_color: Color::LinearRgba(dir.with_alpha(f.density * 0.5)),
            directional_light_exponent: 12.0,
            falloff: FogFalloff::Exponential {
                density: std::f32::consts::LN_2 / half_m,
            },
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Baked light probes and cubemaps.

/// Probe values (int16) per unit of the game's linear light (engine.dll 0x1800cd050).
const PROBE_UNITS: f32 = 8192.0;
/// Largest irradiance volume, in voxels per axis (x, y, z), and the smallest voxel (units).
const VOLUME_MAX: [usize; 3] = [112, 112, 40];
const VOXEL_MIN: f32 = 128.0;

/// The world and model shaders' sky light for a surface facing `n`, with `v` the baked sky
/// visibility vector (a lightmap B texel or a probe's): `saturate((max(|v|^2, n.v) + n.v) *
/// |v| / 2)`, times `c_skyColor`.
pub fn sky_term(n: Vec3, v: Vec3) -> f32 {
    let (len2, nv) = (v.length_squared(), n.dot(v));
    ((len2.max(nv) + nv) * len2.sqrt() * 0.5).clamp(0.0, 1.0)
}

/// Give everything without a lightmap (props, Titans, the pilot's arms) the map's baked
/// light: the LIGHTPROBES resampled onto a Bevy irradiance volume covering the probes. The
/// model shader (uber*_skn/_fix ps_5_0) lights a normal `n` with `max(c.n + dc, 0)` per
/// channel plus the sky term of the probe's sky vector times `sky` (`c_skyColor`); the sun
/// is Bevy's directional light. Returns false (keep the flat ambient light) when the map has
/// no probes.
pub fn spawn_irradiance_volume(
    commands: &mut Commands,
    root: Entity,
    bsp: &tf_assets::bsp::Bsp,
    images: &mut Assets<Image>,
    sky: Vec3,
) -> bool {
    let t = std::time::Instant::now();
    let probes = bsp.light_probes();
    let probe_sky = bsp.light_probe_sky();
    let refs: Vec<([f32; 3], u32)> = bsp
        .light_probe_refs()
        .into_iter()
        .filter(|r| (r.1 as usize) < probes.len())
        .collect();
    if refs.len() < 8 {
        return false;
    }
    // Bounds of the probe positions, ignoring the few out in the 3D skybox.
    let pct = |axis: usize, q: f32| {
        let mut v: Vec<f32> = refs.iter().map(|r| r.0[axis]).collect();
        v.sort_by(f32::total_cmp);
        v[((v.len() - 1) as f32 * q) as usize]
    };
    let lo = Vec3::new(pct(0, 0.002), pct(1, 0.002), pct(2, 0.002)) - Vec3::splat(256.0);
    let hi = Vec3::new(pct(0, 0.998), pct(1, 0.998), pct(2, 0.998)) + Vec3::splat(256.0);
    let size = hi - lo;
    let cell = (0..3)
        .map(|a| size[a] / VOLUME_MAX[a] as f32)
        .fold(VOXEL_MIN, f32::max);
    let dims = [0, 1, 2].map(|a| ((size[a] / cell).ceil() as usize).max(1));

    // Splat each probe onto the voxels around it (inverse-distance weights), then give voxels
    // no probe reached (inside rock, open sky) the value of the nearest voxel that has one.
    let n = dims[0] * dims[1] * dims[2];
    let vidx = |x: usize, y: usize, z: usize| (z * dims[1] + y) * dims[0] + x;
    // Rows: the three colour channels' SH, then the sky vector.
    let mut acc = vec![[[0f32; 4]; 4]; n];
    let mut weight = vec![0f32; n];
    for r in &refs {
        let p = Vec3::from(r.0);
        let c = ((p - lo) / cell - 0.5).floor().as_ivec3();
        let sh = &probes[r.1 as usize];
        let sv = probe_sky.get(r.1 as usize).copied().unwrap_or_default();
        for dz in 0..2 {
            for dy in 0..2 {
                for dx in 0..2 {
                    let v = c + IVec3::new(dx, dy, dz);
                    if v.min_element() < 0
                        || v.x >= dims[0] as i32
                        || v.y >= dims[1] as i32
                        || v.z >= dims[2] as i32
                    {
                        continue;
                    }
                    let center = lo + (v.as_vec3() + 0.5) * cell;
                    let wgt = 1.0 / (center.distance_squared(p) + cell * cell * 0.0625);
                    let i = vidx(v.x as usize, v.y as usize, v.z as usize);
                    for ch in 0..3 {
                        for k in 0..4 {
                            acc[i][ch][k] += sh[ch][k] as f32 * wgt;
                        }
                    }
                    for k in 0..3 {
                        acc[i][3][k] += sv[k] as f32 * wgt;
                    }
                    weight[i] += wgt;
                }
            }
        }
    }
    let mut queue = std::collections::VecDeque::new();
    for i in 0..n {
        if weight[i] > 0.0 {
            let w = weight[i];
            acc[i] = acc[i].map(|row| row.map(|v| v / w));
            queue.push_back(i);
        }
    }
    let mut filled: Vec<bool> = weight.iter().map(|&w| w > 0.0).collect();
    while let Some(i) = queue.pop_front() {
        let (x, y, z) = (i % dims[0], i / dims[0] % dims[1], i / (dims[0] * dims[1]));
        let mut visit = |x: usize, y: usize, z: usize| {
            let j = vidx(x, y, z);
            if !filled[j] {
                filled[j] = true;
                acc[j] = acc[i];
                queue.push_back(j);
            }
        };
        if x > 0 {
            visit(x - 1, y, z)
        }
        if x + 1 < dims[0] {
            visit(x + 1, y, z)
        }
        if y > 0 {
            visit(x, y - 1, z)
        }
        if y + 1 < dims[1] {
            visit(x, y + 1, z)
        }
        if z > 0 {
            visit(x, y, z - 1)
        }
        if z + 1 < dims[2] {
            visit(x, y, z + 1)
        }
    }
    // Ambient-cube faces in Bevy's order (+X -X, +Y -Y, +Z -Z of Bevy space) as game directions.
    let faces = [Vec3::X, -Vec3::X, Vec3::Z, -Vec3::Z, -Vec3::Y, Vec3::Y];
    let (w, h, d) = (dims[0], dims[1] * 2, dims[2] * 3);
    let mut data = vec![0u16; w * h * d * 4];
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                let sh = acc[vidx(x, y, z)];
                let sky_vec = Vec3::new(sh[3][0], sh[3][1], sh[3][2]) / PROBE_UNITS;
                for (f, dir) in faces.iter().enumerate() {
                    let (slab, neg) = (f / 2, f % 2);
                    let idx = (((slab * dims[2] + z) * h) + neg * dims[1] + y) * w + x;
                    let sky_light = sky * sky_term(*dir, sky_vec);
                    for ch in 0..3 {
                        let [cx, cy, cz, dc] = sh[ch];
                        let e = (dc + cx * dir.x + cy * dir.y + cz * dir.z).max(0.0) / PROBE_UNITS
                            + sky_light[ch];
                        data[idx * 4 + ch] = f32_to_half(e);
                    }
                    data[idx * 4 + 3] = 0x3C00;
                }
            }
        }
    }
    let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut img = Image::new(
        bevy::render::render_resource::Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: d as u32,
        },
        bevy::render::render_resource::TextureDimension::D3,
        bytes,
        bevy::render::render_resource::TextureFormat::Rgba16Float,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = bevy::image::ImageSampler::linear();
    let voxels = images.add(img);
    let gain = std::env::var("TF_PROBE_GAIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    let volume = commands
        .spawn((
            bevy::light::IrradianceVolume {
                voxels,
                intensity: nits_for_white() * gain,
                affects_lightmapped_meshes: false,
            },
            Transform::from_translation((lo + hi) * 0.5).with_scale(size),
        ))
        .id();
    commands.entity(root).add_child(volume);
    log::info!("irradiance volume: {} probes ({} refs) on {dims:?} voxels of {cell:.0} units, built in {:?}", probes.len(), refs.len(), t.elapsed());
    true
}

pub fn f32_to_half(v: f32) -> u16 {
    let v = v.clamp(0.0, 65000.0);
    if v < 6.1e-5 {
        return (v / 5.96e-8).round() as u16;
    }
    let bits = v.to_bits();
    let exp = ((bits >> 23) & 0xFF) as i32 - 127 + 15;
    let mant = (bits >> 13) & 0x3FF;
    ((exp as u32) << 10 | mant) as u16
}

/// The map's baked cubemaps (from the BSP's PAKFILE) as Bevy environment maps. The camera
/// reflects the one captured nearest to it.
#[derive(Resource, Default)]
pub struct Reflections {
    /// Capture points, game space.
    pub origins: Vec<Vec3>,
    /// (diffuse, specular) cube images per cubemap.
    pub maps: Vec<(Handle<Image>, Handle<Image>)>,
    current: Option<usize>,
}

/// Brightest value kept from a cubemap (a few texels hold f16-max hot spots).
const CUBEMAP_CLAMP: u16 = 0x5400; // 64.0

impl Reflections {
    pub fn load(bsp: &tf_assets::bsp::Bsp, images: &mut Assets<Image>) -> Self {
        let t = std::time::Instant::now();
        let Some(vtf) = bsp.pakfile_entry("cubemaps.hdr.vtf") else {
            log::info!("no baked cubemaps");
            return Self::default();
        };
        let set = match tf_assets::cubemap::CubemapSet::parse(vtf) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("cubemaps: {e:#}");
                return Self::default();
            }
        };
        let origins: Vec<Vec3> = bsp.cubemap_origins().into_iter().map(Vec3::from).collect();
        let n = set.count.min(origins.len());
        let maps: Vec<(Handle<Image>, Handle<Image>)> = std::thread::scope(|s| {
            let set = &set;
            let jobs: Vec<_> = (0..n)
                .map(|i| s.spawn(move || cube_texels(set, i)))
                .collect();
            jobs.into_iter()
                .map(|j| j.join().unwrap_or_default())
                .collect::<Vec<_>>()
        })
        .into_iter()
        .map(|levels| {
            let spec = images.add(cube_image(&levels));
            // Diffuse lighting comes from the lightmaps and the irradiance volume; this blurry
            // low mip only lights what lies outside the volume.
            let diffuse = images.add(cube_image(
                &levels[levels.len().saturating_sub(4).min(levels.len() - 1)..],
            ));
            (diffuse, spec)
        })
        .collect();
        log::info!(
            "{} cubemaps ({}px) decoded in {:?}",
            maps.len(),
            set.size,
            t.elapsed()
        );
        Self {
            origins: origins[..n].to_vec(),
            maps,
            current: None,
        }
    }
}

impl Reflections {
    /// One box reflection probe per baked cubemap, so each surface reflects the capture made
    /// near it (Bevy uses the nearest eight probes in view; the camera's environment map is the
    /// fallback outside every box). Each box reaches 0.6 of the way to the nearest other
    /// capture (at least 384 units) horizontally and 1.5 times that vertically.
    pub fn spawn_probes(&self, commands: &mut Commands, root: Entity) {
        // Off by default: the probes cost ~40% of the frame rate (2026-10-05); TF_CUBEMAP_PROBES=1
        // turns them on, the camera's nearest cubemap is used otherwise.
        if std::env::var_os("TF_CUBEMAP_PROBES").is_none()
            || std::env::var_os("TF_NO_CUBEMAPS").is_some()
            || std::env::var_os("TF_ONE_CUBEMAP").is_some()
        {
            return;
        }
        for (i, o) in self.origins.iter().enumerate() {
            let near = self
                .origins
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, q)| q.truncate().distance(o.truncate()).max((q.z - o.z).abs()))
                .fold(f32::MAX, f32::min);
            let r = if near < f32::MAX {
                (near * 0.6).clamp(384.0, 3000.0)
            } else {
                3000.0
            };
            let (diffuse, specular) = self.maps[i].clone();
            let e = commands
                .spawn((
                    bevy::light::LightProbe,
                    bevy::light::EnvironmentMapLight {
                        diffuse_map: diffuse,
                        specular_map: specular,
                        intensity: cubemap_intensity(),
                        rotation: Quat::IDENTITY,
                        affects_lightmapped_mesh_diffuse: false,
                    },
                    Transform::from_translation(*o).with_scale(Vec3::new(
                        r * 2.0,
                        r * 2.0,
                        r * 3.0,
                    )),
                ))
                .id();
            commands.entity(root).add_child(e);
        }
        log::info!("{} reflection probes", self.origins.len());
    }
}

/// Faces of cubemap `frame` for each mip (largest first), re-indexed for Bevy: Bevy samples a
/// cube with (x, y, -z) of its Y-up world direction, which is (x, z, y) of the game direction,
/// while the game's cubes are indexed by the game direction itself. Swapping y and z maps faces
/// to faces and texel centres to texel centres, so this is an exact shuffle.
fn cube_texels(set: &tf_assets::cubemap::CubemapSet, frame: usize) -> Vec<[Vec<u16>; 6]> {
    (0..set.mips)
        .map(|mip| {
            let src: Vec<Vec<[u16; 3]>> = (0..6).map(|f| set.face(frame, f, mip)).collect();
            let s = (set.size >> mip).max(1) as usize;
            std::array::from_fn(|face| {
                let mut out = Vec::with_capacity(s * s * 4);
                for i in 0..s {
                    for j in 0..s {
                        let u = (j as f32 + 0.5) / s as f32 * 2.0 - 1.0;
                        let v = (i as f32 + 0.5) / s as f32 * 2.0 - 1.0;
                        let sd = cube_dir(face, u, v);
                        let (sf, su, sv) = cube_lookup(Vec3::new(sd.x, sd.z, sd.y));
                        let (x, y) = (
                            ((su + 1.0) * 0.5 * s as f32) as usize,
                            ((sv + 1.0) * 0.5 * s as f32) as usize,
                        );
                        let px = src[sf][y.min(s - 1) * s + x.min(s - 1)];
                        for c in px {
                            out.push(if c & 0x8000 != 0 {
                                0
                            } else {
                                c.min(CUBEMAP_CLAMP)
                            });
                        }
                        out.push(0x3C00);
                    }
                }
                out
            })
        })
        .collect()
}

/// Direction through (u, v) in [-1, 1] on a cube face (Direct3D/wgpu conventions).
fn cube_dir(face: usize, sc: f32, tc: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1.0, -tc, -sc),
        1 => Vec3::new(-1.0, -tc, sc),
        2 => Vec3::new(sc, 1.0, tc),
        3 => Vec3::new(sc, -1.0, -tc),
        4 => Vec3::new(sc, -tc, 1.0),
        _ => Vec3::new(-sc, -tc, -1.0),
    }
}

/// The face and (u, v) in [-1, 1] a direction samples.
fn cube_lookup(d: Vec3) -> (usize, f32, f32) {
    let a = d.abs();
    if a.x >= a.y && a.x >= a.z {
        if d.x > 0.0 {
            (0, -d.z / a.x, -d.y / a.x)
        } else {
            (1, d.z / a.x, -d.y / a.x)
        }
    } else if a.y >= a.z {
        if d.y > 0.0 {
            (2, d.x / a.y, d.z / a.y)
        } else {
            (3, d.x / a.y, -d.z / a.y)
        }
    } else if d.z > 0.0 {
        (4, d.x / a.z, -d.y / a.z)
    } else {
        (5, -d.x / a.z, -d.y / a.z)
    }
}

/// A cube texture from per-mip faces (largest first), RGBA16F.
fn cube_image(levels: &[[Vec<u16>; 6]]) -> Image {
    let size = (levels[0][0].len() / 4).isqrt() as u32;
    // Bevy's default data order is layer-major: each face's whole mip chain in turn.
    let mut bytes = Vec::new();
    for face in 0..6 {
        for level in levels {
            bytes.extend(level[face].iter().flat_map(|v| v.to_le_bytes()));
        }
    }
    let mut img = Image::default();
    img.data = Some(bytes);
    img.texture_descriptor.size = bevy::render::render_resource::Extent3d {
        width: size,
        height: size,
        depth_or_array_layers: 6,
    };
    img.texture_descriptor.mip_level_count = levels.len() as u32;
    img.texture_descriptor.format = bevy::render::render_resource::TextureFormat::Rgba16Float;
    img.texture_descriptor.dimension = bevy::render::render_resource::TextureDimension::D2;
    img.texture_view_descriptor = Some(bevy::render::render_resource::TextureViewDescriptor {
        dimension: Some(bevy::render::render_resource::TextureViewDimension::Cube),
        ..default()
    });
    img.sampler = bevy::image::ImageSampler::linear();
    img.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
    img
}

/// Cubemap reflections relative to display white. The captures include direct sun on bright
/// surfaces, and full strength washed lit scenes out (tuned by eye on Forwardbase Kodai).
const CUBEMAP_GAIN: f32 = 0.35;

/// Cubemap brightness: a texel of 1.0 is about display white, like the game's HDR captures.
pub fn cubemap_intensity() -> f32 {
    CUBEMAP_GAIN
        * nits_for_white()
        * std::env::var("TF_CUBEMAP_GAIN")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0)
}

/// Point the camera's environment map at the cubemap captured nearest to it.
pub fn nearest_reflection(
    mut commands: Commands,
    refl: Option<ResMut<Reflections>>,
    cams: Query<(Entity, &GlobalTransform), With<crate::player::MainCamera>>,
) {
    let Some(mut refl) = refl else { return };
    let Ok((cam, gt)) = cams.single() else { return };
    if refl.maps.is_empty() || std::env::var_os("TF_NO_CUBEMAPS").is_some() {
        return;
    }
    let b = gt.translation() / crate::player::UNIT;
    let pos = Vec3::new(b.x, -b.z, b.y);
    let nearest = refl
        .origins
        .iter()
        .enumerate()
        .min_by(|a, b| {
            a.1.distance_squared(pos)
                .total_cmp(&b.1.distance_squared(pos))
        })
        .map(|(i, _)| i);
    if nearest == refl.current {
        return;
    }
    refl.current = nearest;
    let Some(i) = nearest else { return };
    let (diffuse, specular) = refl.maps[i].clone();
    commands
        .entity(cam)
        .insert(bevy::light::EnvironmentMapLight {
            diffuse_map: diffuse,
            specular_map: specular,
            intensity: cubemap_intensity(),
            rotation: Quat::IDENTITY,
            affects_lightmapped_mesh_diffuse: false,
        });
}
