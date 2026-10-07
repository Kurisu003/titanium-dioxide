//! Animated studio models: skeleton entities, skinned meshes, clip blending and additive
//! (delta) layers such as aim matrices.

use crate::convert::{self, Cache};
use crate::gamedata::GameData;
use anyhow::{Context, Result};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tf_assets::mdl::Model;

/// Local bone transforms for every bone of the actor.
pub type Pose = Vec<(Vec3, Quat)>;

/// A decoded clip retargeted to the actor's skeleton.
pub struct Clip {
    /// The animation stores offsets from the bind pose (STUDIO_DELTA) rather than a full pose.
    pub delta: bool,
    pub frames: Vec<Pose>,
    /// Root-motion ground speed in units per second.
    pub ground_speed: f32,
    pub duration: f32,
    /// Cumulative root motion per frame (x forward, y left, z up, yaw degrees), if any.
    pub movement: Vec<[f32; 4]>,
    /// The sequence's sound events (AE_CL_PLAYSOUND): cycle and sound event name.
    pub sounds: Vec<(f32, String)>,
    /// AE_CL_PLAYSOUND_FOR_TYPE events: the actor's sound type is prefixed to the name
    /// ("Jog.Generic_3P" on an Ion plays "ion_jog_generic_3p").
    pub typed_sounds: Vec<(f32, String)>,
}

impl Clip {
    /// The average pose over all frames (quaternions summed in one hemisphere).
    pub fn mean(&self) -> Pose {
        let Some(first) = self.frames.first() else { return Vec::new() };
        let n = self.frames.len() as f32;
        first
            .iter()
            .enumerate()
            .map(|(b, &(_, q0))| {
                let (mut p, mut q) = (Vec3::ZERO, Vec4::ZERO);
                for f in &self.frames {
                    let (fp, fq) = f[b];
                    p += fp;
                    q += if Vec4::from(fq).dot(Vec4::from(q0)) < 0.0 { -Vec4::from(fq) } else { Vec4::from(fq) };
                }
                (p / n, Quat::from_vec4(q).normalize())
            })
            .collect()
    }
    /// Root motion at `t` seconds, interpolated between frames.
    pub fn movement_at(&self, t: f32) -> Option<[f32; 4]> {
        if self.movement.is_empty() {
            return None;
        }
        let n = self.movement.len();
        let f = (t / self.duration.max(1e-3)).clamp(0.0, 1.0) * (n - 1) as f32;
        let (i, k) = (f.floor() as usize, f.fract());
        let (a, b) = (self.movement[i.min(n - 1)], self.movement[(i + 1).min(n - 1)]);
        Some(std::array::from_fn(|c| a[c] + (b[c] - a[c]) * k))
    }

    pub fn sample(&self, cycle: f32) -> Pose {
        let n = self.frames.len();
        if n == 1 {
            return self.frames[0].clone();
        }
        let f = cycle.rem_euclid(1.0) * (n - 1) as f32;
        let a = (f.floor() as usize).min(n - 1);
        let b = (a + 1).min(n - 1);
        let s = f - a as f32;
        self.frames[a]
            .iter()
            .zip(&self.frames[b])
            .map(|(&(p0, q0), &(p1, q1))| (p0.lerp(p1, s), q0.slerp(q1, s)))
            .collect()
    }
}

/// An additive blend grid (e.g. `combat_aim_run`): clips indexed `x + y * width`.
pub struct AimGrid {
    pub clips: Vec<usize>,
    pub width: usize,
    pub height: usize,
    /// Parameter ranges in degrees: x (aim_yaw) and y (aim_pitch).
    pub x_range: (f32, f32),
    pub y_range: (f32, f32),
    /// `STUDIO_POST` (0x10): the delta is applied in the bone's local frame (base * delta)
    /// instead of before it (delta * base).
    pub post: bool,
}

/// The body part a mesh belongs to (for showing and hiding bodygroups at run time).
#[derive(Component, Clone, Copy)]
pub struct BodyPartMesh(pub usize);

/// One weighted clip contribution.
#[derive(Clone, Copy, Debug)]
pub struct Layer {
    pub clip: usize,
    pub cycle: f32,
    pub weight: f32,
}

/// A looping or one-shot additive blend grid (viewmodel idle/walk/jump layers): grid name,
/// the two pose parameter values, the cycle and the weight.
#[derive(Clone, Debug)]
pub struct GridLayer {
    pub grid: &'static str,
    pub x: f32,
    pub y: f32,
    pub cycle: f32,
    pub weight: f32,
    /// Apply only each clip's motion about its own average pose (a layer whose clips carry
    /// constant offsets, like the viewmodels' walk_seq: the ADS walk clip holds the gun 2.6
    /// units and 2.3 degrees off the ADS pose, which pushed the sight off centre).
    pub relative: bool,
}

/// Additive aim layer: grid name, aim yaw and pitch in degrees, weight.
#[derive(Clone, Debug)]
pub struct Aim {
    pub grid: String,
    pub yaw: f32,
    pub pitch: f32,
    pub weight: f32,
}

#[derive(Component)]
pub struct Actor {
    pub joints: Vec<Entity>,
    pub bone_names: Vec<String>,
    pub parents: Vec<i32>,
    pub rest: Pose,
    /// Shared between all instances of the same model.
    pub clips: Arc<Vec<Clip>>,
    pub by_name: Arc<HashMap<String, usize>>,
    pub grids: Arc<HashMap<String, AimGrid>>,
    /// Base layers, blended by weight.
    pub layers: Vec<Layer>,
    pub aim: Option<Aim>,
    /// Additive (post-delta) clips applied after the aim layer, e.g. weapon fire kick.
    pub additive: Vec<Layer>,
    /// Additive grids applied after the aim layer and before `additive`.
    pub grid_layers: Vec<GridLayer>,
    /// Root bones moved by code after animation: (bone, model-space offset added).
    pub root_offsets: Vec<(usize, Vec3)>,
    /// When true, `animate_actors` advances every layer's cycle by itself (viewer mode).
    pub autoplay: bool,
    /// Play the clips' sound events as their cycles pass them (first-person viewmodels).
    pub event_sounds: bool,
    /// Last seen cycle of each clip, for sound events.
    pub last_cycles: Vec<(usize, f32)>,
    /// Sound type for typed events (e.g. "ion"); None plays only plain events.
    pub sound_type: Option<&'static str>,
    /// Where the actor is (game space) for positional sounds; None plays them on the player.
    pub sound_at: Option<Vec3>,
}

impl Actor {
    /// Joint entity of a bone by name.
    pub fn joint(&self, name: &str) -> Option<Entity> {
        self.bone_names.iter().position(|b| b.eq_ignore_ascii_case(name)).map(|i| self.joints[i])
    }

    /// Model-space transform of a bone in the current pose.
    pub fn bone_model_transform(&self, name: &str) -> Option<Transform> {
        let target = self.bone_names.iter().position(|b| b.eq_ignore_ascii_case(name))?;
        let pose = self.evaluate();
        let mut chain = vec![target];
        while let Some(&b) = chain.last() {
            match self.parents[b] {
                p if p >= 0 => chain.push(p as usize),
                _ => break,
            }
        }
        let mut tf = Transform::IDENTITY;
        for &b in chain.iter().rev() {
            tf = tf * Transform::from_translation(pose[b].0).with_rotation(pose[b].1);
        }
        Some(tf)
    }

    /// Add a clip to `layers` (full pose) or `additive` (delta), depending on how it's stored.
    pub fn push_clip(&self, layers: &mut Vec<Layer>, additive: &mut Vec<Layer>, clip: usize, cycle: f32, weight: f32) {
        let l = Layer { clip, cycle, weight };
        if self.clips[clip].delta {
            additive.push(l);
        } else {
            layers.push(l);
        }
    }

    pub fn clip(&self, name: &str) -> Option<usize> {
        self.by_name.get(&name.to_ascii_lowercase()).copied()
    }

    /// Weighted blend of the base layers followed by the aim layer.
    pub fn evaluate(&self) -> Pose {
        let total: f32 = self.layers.iter().map(|l| l.weight.max(0.0)).sum();
        let mut pose = if total <= 1e-4 {
            self.rest.clone()
        } else {
            let mut acc: Vec<(Vec3, Vec4)> = vec![(Vec3::ZERO, Vec4::ZERO); self.rest.len()];
            for l in &self.layers {
                if l.weight <= 0.0 {
                    continue;
                }
                let w = l.weight / total;
                for (i, (pos, q)) in self.clips[l.clip].sample(l.cycle).into_iter().enumerate() {
                    acc[i].0 += pos * w;
                    let mut v = Vec4::from(q);
                    // Keep quaternions in the same hemisphere before summing.
                    if acc[i].1.dot(v) < 0.0 {
                        v = -v;
                    }
                    acc[i].1 += v * w;
                }
            }
            acc.into_iter().map(|(p, q)| (p, Quat::from_vec4(q).normalize())).collect()
        };
        if let Some(aim) = &self.aim {
            if let Some(g) = self.grids.get(&aim.grid) {
                let delta = self.sample_grid(g, aim.yaw, aim.pitch);
                for (b, (dp, dq)) in pose.iter_mut().zip(delta) {
                    // Source's SlerpBones: QuaternionMA (post) or QuaternionSM; pos += delta.
                    let dq = Quat::IDENTITY.slerp(dq, aim.weight);
                    b.1 = if g.post { b.1 * dq } else { dq * b.1 }.normalize();
                    b.0 += dp * aim.weight;
                }
            }
        }
        for l in &self.grid_layers {
            let Some(g) = self.grids.get(l.grid) else { continue };
            if l.weight <= 0.0 {
                continue;
            }
            let mut delta = self.sample_grid_at(g, l.x, l.y, l.cycle);
            if l.relative {
                let base = self.sample_grid_mean(g, l.x, l.y);
                for ((dp, dq), (bp, bq)) in delta.iter_mut().zip(base) {
                    *dp -= bp;
                    *dq = if g.post { bq.inverse() * *dq } else { *dq * bq.inverse() };
                }
            }
            for (b, (dp, dq)) in pose.iter_mut().zip(delta) {
                let dq = Quat::IDENTITY.slerp(dq, l.weight);
                b.1 = if g.post { b.1 * dq } else { dq * b.1 }.normalize();
                b.0 += dp * l.weight;
            }
        }
        for l in &self.additive {
            if l.weight <= 0.0 {
                continue;
            }
            let delta = self.clips[l.clip].sample(l.cycle);
            for (b, (dp, dq)) in pose.iter_mut().zip(delta) {
                b.1 = (b.1 * Quat::IDENTITY.slerp(dq, l.weight)).normalize();
                b.0 += dp * l.weight;
            }
        }
        for &(b, off) in &self.root_offsets {
            if let Some(p) = pose.get_mut(b) {
                p.0 += off;
            }
        }
        pose
    }

    fn sample_grid(&self, g: &AimGrid, x: f32, y: f32) -> Pose {
        self.sample_grid_at(g, x, y, 0.0)
    }

    /// Bilinear blend of the grid's clips' average poses at the parameter values.
    pub fn sample_grid_mean(&self, g: &AimGrid, x: f32, y: f32) -> Pose {
        let norm = |v: f32, r: (f32, f32), n: usize| if n < 2 || (r.1 - r.0).abs() < 1e-6 { 0.0 } else { ((v - r.0) / (r.1 - r.0)).clamp(0.0, 1.0) * (n - 1) as f32 };
        let fx = norm(x, g.x_range, g.width);
        let fy = norm(y, g.y_range, g.height);
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(g.width - 1), (y0 + 1).min(g.height - 1));
        let (sx, sy) = (fx - x0 as f32, fy - y0 as f32);
        let get = |xx: usize, yy: usize| self.clips[g.clips[xx + yy * g.width]].mean();
        let lerp = |a: Pose, b: Pose, s: f32| -> Pose {
            a.into_iter().zip(b).map(|((p0, q0), (p1, q1))| (p0.lerp(p1, s), q0.slerp(q1, s))).collect()
        };
        let top = lerp(get(x0, y0), get(x1, y0), sx);
        let bottom = lerp(get(x0, y1), get(x1, y1), sx);
        lerp(top, bottom, sy)
    }

    /// Bilinear blend of the grid's clips at the parameter values, each sampled at `cycle`.
    pub fn sample_grid_at(&self, g: &AimGrid, x: f32, y: f32, cycle: f32) -> Pose {
        // A one-wide axis has an empty parameter range (0/0): treat it as 0.
        let norm = |v: f32, r: (f32, f32), n: usize| if n < 2 || (r.1 - r.0).abs() < 1e-6 { 0.0 } else { ((v - r.0) / (r.1 - r.0)).clamp(0.0, 1.0) * (n - 1) as f32 };
        let fx = norm(x, g.x_range, g.width);
        let fy = norm(y, g.y_range, g.height);
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(g.width - 1), (y0 + 1).min(g.height - 1));
        let (sx, sy) = (fx - x0 as f32, fy - y0 as f32);
        let get = |xx: usize, yy: usize| self.clips[g.clips[xx + yy * g.width]].sample(cycle);
        let lerp = |a: Pose, b: Pose, s: f32| -> Pose {
            a.into_iter().zip(b).map(|((p0, q0), (p1, q1))| (p0.lerp(p1, s), q0.slerp(q1, s))).collect()
        };
        let top = lerp(get(x0, y0), get(x1, y0), sx);
        let bottom = lerp(get(x0, y1), get(x1, y1), sx);
        lerp(top, bottom, sy)
    }
}

/// Decode `anim_index` of `owner`, retargeted onto `model`'s skeleton by bone name.
fn retarget(model: &Model, owner: &Model, anim_index: usize) -> Result<Clip> {
    let anim = owner.animation(anim_index)?;
    let delta = anim.flags & 0x4 != 0;
    let map: Vec<Option<usize>> = model.bones.iter().map(|b| owner.bone_index(&b.name)).collect();
    let frames = anim
        .frames
        .iter()
        .map(|pose| {
            model
                .bones
                .iter()
                .enumerate()
                .map(|(bi, b)| match map[bi] {
                    Some(oi) => (Vec3::from(pose[oi].0), Quat::from_array(pose[oi].1).normalize()),
                    None if delta => (Vec3::ZERO, Quat::IDENTITY),
                    None => (Vec3::from(b.pos), Quat::from_array(b.quat).normalize()),
                })
                .collect()
        })
        .collect();
    let fps = anim.fps.max(1.0);
    Ok(Clip {
        delta,
        frames,
        ground_speed: anim.ground_speed(),
        duration: (anim.num_frames.max(2) - 1) as f32 / fps,
        movement: anim.movement.clone(),
        sounds: Vec::new(),
        typed_sounds: Vec::new(),
    })
}

/// What an actor should load: plain sequences and additive grids.
pub struct ActorSpec<'a> {
    pub path: &'a str,
    pub sequences: &'a [&'a str],
    pub grids: &'a [&'a str],
    pub body: &'a [usize],
}

/// A spawned actor: its entity plus bone joints (usable before the `Actor` component lands).
pub struct Spawned {
    pub entity: Entity,
    pub joints: Vec<Entity>,
    pub bone_names: Vec<String>,
}

impl Spawned {
    pub fn joint(&self, name: &str) -> Option<Entity> {
        self.bone_names.iter().position(|b| b.eq_ignore_ascii_case(name)).map(|i| self.joints[i])
    }
}

/// Spawn an animated model under `parent`.
#[allow(clippy::too_many_arguments)]
pub fn spawn_actor(
    commands: &mut Commands,
    parent: Entity,
    gd: &GameData,
    spec: &ActorSpec,
    cache: &mut Cache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
) -> Result<Spawned> {
    let key = format!("{}|{:?}|{:?}|{:?}", spec.path, spec.sequences, spec.grids, spec.body);
    let tpl = match cache.actors.get(&key) {
        Some(t) => t.clone(),
        None => {
            let t = build_template(gd, spec, cache, meshes, images, materials, bindposes)?;
            cache.actors.insert(key, t.clone());
            t
        }
    };

    // Skeleton: one entity per bone, in the bind pose until animated.
    let actor = commands.spawn((Transform::IDENTITY, Visibility::default())).id();
    commands.entity(parent).add_child(actor);
    let mut joints = Vec::with_capacity(tpl.bones.len());
    for (_, parent_idx, p, q) in tpl.bones.iter() {
        let j = commands.spawn((Transform::from_translation(*p).with_rotation(*q), Visibility::default())).id();
        let par = if *parent_idx >= 0 { joints[*parent_idx as usize] } else { actor };
        commands.entity(par).add_child(j);
        joints.push(j);
    }
    for (mesh, mat, part) in tpl.parts.iter() {
        let e = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                SkinnedMesh { inverse_bindposes: tpl.inverse.clone(), joints: joints.clone() },
                NoFrustumCulling,
                BodyPartMesh(*part),
            ))
            .id();
        commands.entity(actor).add_child(e);
    }
    let bone_names: Vec<String> = tpl.bones.iter().map(|b| b.0.clone()).collect();
    let parents = tpl.bones.iter().map(|b| b.1).collect();
    let rest: Pose = tpl.bones.iter().map(|b| (b.2, b.3)).collect();
    let spawned = Spawned { entity: actor, joints: joints.clone(), bone_names: bone_names.clone() };
    let layers = if tpl.clips.is_empty() { vec![] } else { vec![Layer { clip: 0, cycle: 0.0, weight: 1.0 }] };
    commands.entity(actor).insert(Actor {
        joints,
        bone_names,
        parents,
        rest,
        clips: tpl.clips.clone(),
        by_name: tpl.by_name.clone(),
        grids: tpl.grids.clone(),
        layers,
        aim: None,
        additive: Vec::new(),
        grid_layers: Vec::new(),
        root_offsets: Vec::new(),
        autoplay: true,
        event_sounds: spec.path.contains("/atpov_") || spec.path.contains("/ptpov_"),
        last_cycles: Vec::new(),
        sound_type: None,
        sound_at: None,
    });
    Ok(spawned)
}

/// Everything about a model that can be shared between instances.
#[derive(Clone)]
pub struct ActorTemplate {
    /// name, parent, rest position, rest rotation
    pub bones: Arc<Vec<(String, i32, Vec3, Quat)>>,
    pub clips: Arc<Vec<Clip>>,
    pub by_name: Arc<HashMap<String, usize>>,
    pub grids: Arc<HashMap<String, AimGrid>>,
    /// Meshes with their material and body part index.
    pub parts: Arc<Vec<(Handle<Mesh>, Handle<StandardMaterial>, usize)>>,
    pub inverse: Handle<SkinnedMeshInverseBindposes>,
}

#[allow(clippy::too_many_arguments)]
/// Parsed models by path, shared between actors (Titan chassis share their animation includes).
static MODELS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, Arc<Model>>>> = std::sync::LazyLock::new(Default::default);

/// A model's default hitbox set (bone indices match the actor's joints).
pub fn model_hitboxes(gd: &GameData, path: &str) -> Vec<tf_assets::mdl::Hitbox> {
    parsed_model(gd, path).map(|m| m.hitboxes.clone()).unwrap_or_default()
}

pub fn parsed_model(gd: &GameData, path: &str) -> Result<Arc<Model>> {
    if let Some(m) = MODELS.lock().unwrap().get(path) {
        return Ok(m.clone());
    }
    let m = Arc::new(Model::parse(gd.read_file(path)?)?);
    MODELS.lock().unwrap().insert(path.to_string(), m.clone());
    Ok(m)
}

/// Read and parse models (and then their includes) on every core ahead of spawning them, so
/// the per-actor builds find them cached. VPK reads dominate model build time.
pub fn prefetch_models(gd: &GameData, paths: &[String]) {
    let t = std::time::Instant::now();
    let models = crate::world::parallel_map(paths, |p| parsed_model(gd, p).ok());
    let mut includes: Vec<String> = models.into_iter().flatten().flatten().flat_map(|m| m.include_models.clone()).collect();
    includes.sort();
    includes.dedup();
    crate::world::parallel_map(&includes, |p| parsed_model(gd, p).ok());
    log::info!("prefetched {} models and {} includes in {:?}", paths.len(), includes.len(), t.elapsed());
}

fn build_template(
    gd: &GameData,
    spec: &ActorSpec,
    cache: &mut Cache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
) -> Result<ActorTemplate> {
    let t = std::time::Instant::now();
    let path = spec.path;
    let model = parsed_model(gd, path)?;
    // Included animation models are shared between chassis; read them in parallel.
    let includes: Vec<Arc<Model>> = std::thread::scope(|scope| {
        let jobs: Vec<_> = model.include_models.iter().map(|p| scope.spawn(move || (p, parsed_model(gd, p)))).collect();
        jobs.into_iter()
            .filter_map(|j| match j.join() {
                Ok((_, Ok(m))) => Some(m),
                Ok((p, Err(e))) => {
                    log::warn!("include {p}: {e}");
                    None
                }
                Err(_) => None,
            })
            .collect()
    });
    let t_parse = t.elapsed();
    let owners: Vec<&Model> = std::iter::once(&*model).chain(includes.iter().map(|m| &**m)).collect();
    let find = |label: &str| {
        owners.iter().find_map(|o| {
            o.sequences.iter().position(|s| s.label.eq_ignore_ascii_case(label)).map(|i| (*o, i))
        })
    };

    let mut clips = Vec::new();
    let mut by_name = HashMap::new();
    for &label in spec.sequences {
        // A leading '?' marks an optional sequence (some models of a family lack it).
        let (label, optional) = match label.strip_prefix('?') {
            Some(l) => (l, true),
            None => (label, false),
        };
        let r = (|| -> Result<Clip> {
            let (owner, si) = find(label).with_context(|| format!("sequence {label} not found"))?;
            let mut clip = retarget(&model, owner, *owner.sequences[si].anims.first().context("empty sequence")?)?;
            clip.sounds = owner.sequences[si]
                .events
                .iter()
                .filter(|e| e.name == "AE_CL_PLAYSOUND" && !e.options.is_empty())
                .map(|e| (e.cycle, e.options.clone()))
                .collect();
            clip.typed_sounds = owner.sequences[si]
                .events
                .iter()
                .filter(|e| e.name == "AE_CL_PLAYSOUND_FOR_TYPE" && !e.options.is_empty())
                .map(|e| (e.cycle, e.options.to_ascii_lowercase().replace('.', "_")))
                .collect();
            Ok(clip)
        })();
        match r {
            Ok(c) => {
                by_name.insert(label.to_ascii_lowercase(), clips.len());
                clips.push(c);
            }
            Err(e) if optional => log::debug!("{path}: {e:#}"),
            Err(e) => log::warn!("{path}: {e:#}"),
        }
    }
    let mut grids = HashMap::new();
    for &label in spec.grids {
        // A leading '?' marks an optional grid (viewmodel layers some guns don't have).
        let (label, optional) = match label.strip_prefix('?') {
            Some(l) => (l, true),
            None => (label, false),
        };
        let r = (|| -> Result<AimGrid> {
            let (owner, si) = find(label).with_context(|| format!("grid {label} not found"))?;
            let seq = &owner.sequences[si];
            // Parameter ranges live at paramstart/paramend in the sequence descriptor.
            let o = i32::from_le_bytes(owner.raw[0xC4..0xC8].try_into().unwrap()) as usize + si * 0xE8;
            let f = |off: usize| f32::from_le_bytes(owner.raw[o + off..o + off + 4].try_into().unwrap());
            let mut ids = Vec::new();
            for &ai in &seq.anims {
                ids.push(clips.len());
                clips.push(retarget(&model, owner, ai)?);
            }
            Ok(AimGrid {
                clips: ids,
                width: seq.group_size[0].max(1) as usize,
                height: seq.group_size[1].max(1) as usize,
                x_range: (f(0x54), f(0x5C)),
                y_range: (f(0x58), f(0x60)),
                post: seq.flags & 0x10 != 0,
            })
        })();
        match r {
            Ok(g) => {
                grids.insert(label.to_string(), g);
            }
            Err(e) if optional => log::debug!("{path}: {e:#}"),
            Err(e) => log::warn!("{path}: {e:#}"),
        }
    }

    let t_anim = t.elapsed();
    let inverse: Vec<Mat4> = model
        .bones
        .iter()
        .map(|b| {
            let m = b.pose_to_bone;
            Mat4::from_cols(
                Vec4::new(m[0][0], m[1][0], m[2][0], 0.0),
                Vec4::new(m[0][1], m[1][1], m[2][1], 0.0),
                Vec4::new(m[0][2], m[1][2], m[2][2], 0.0),
                Vec4::new(m[0][3], m[1][3], m[2][3], 1.0),
            )
        })
        .collect();
    let inverse = bindposes.add(SkinnedMeshInverseBindposes::from(inverse));
    let mut parts = Vec::new();
    let mut tris = 0;
    // Meshes (tangent generation is most of the work) on every core; materials here.
    let mut mesh_data = model.meshes(spec.body)?;
    hide_behind_screen(&model, &mut mesh_data, spec.sequences.first().copied());
    let mds: Vec<std::sync::Mutex<Option<tf_assets::mdl::MeshData>>> = mesh_data.into_iter().map(|m| std::sync::Mutex::new(Some(m))).collect();
    let built = crate::world::parallel_map(&mds, |slot| {
        let md = slot.lock().ok()?.take()?;
        let tex = model.skin_families.first().and_then(|f| f.get(md.material)).copied().unwrap_or(0) as usize;
        let mat_name = model.textures.get(tex).cloned().unwrap_or_default();
        // The cockpit's main screen shows a camera feed in the game; leave it out so the
        // world is visible through it.
        // TF_HIDE=substr also leaves out animated models' materials (debugging).
        let hidden = std::env::var("TF_HIDE").is_ok_and(|h| !h.is_empty() && mat_name.to_ascii_lowercase().contains(&h.to_ascii_lowercase()));
        if hidden || mat_name.contains("debugempty") || mat_name.to_ascii_lowercase().contains("_int_screen") {
            return None;
        }
        let n = md.indices.len() / 3;
        let part = md.bodypart;
        Some((mat_name, n, part, convert::build_mesh(md.positions, md.normals, md.uvs, md.indices, Some((md.joints, md.weights)))))
    });
    for (mat_name, n, part, mesh) in built.into_iter().flatten().flatten() {
        let mat = convert::material(gd, cache, images, materials, &mat_name, false);
        tris += n;
        parts.push((meshes.add(mesh), mat, part));
    }
    log::info!(
        "{path}: {} bones, {tris} triangles, {} includes, {} clips, grids {:?} in {:?} (parse {:?}, animations {:?})",
        model.bones.len(),
        includes.len(),
        clips.len(),
        grids.keys().collect::<Vec<_>>(),
        t.elapsed(),
        t_parse,
        t_anim - t_parse
    );
    let bones = model
        .bones
        .iter()
        .map(|b| (b.name.clone(), b.parent, Vec3::from(b.pos), Quat::from_array(b.quat).normalize()))
        .collect();
    Ok(ActorTemplate {
        bones: Arc::new(bones),
        clips: Arc::new(clips),
        by_name: Arc::new(by_name),
        grids: Arc::new(grids),
        parts: Arc::new(parts),
        inverse,
    })
}

pub fn animate_actors(time: Res<Time>, mut actors: Query<&mut Actor>, mut transforms: Query<&mut Transform>) {
    let dt = time.delta_secs();
    for mut actor in &mut actors {
        if actor.clips.is_empty() {
            continue;
        }
        if actor.autoplay {
            let durations: Vec<f32> = actor.layers.iter().map(|l| actor.clips[l.clip].duration).collect();
            for (l, d) in actor.layers.iter_mut().zip(durations) {
                l.cycle = (l.cycle + dt / d.max(1e-3)).rem_euclid(1.0);
            }
        }
        let pose = actor.evaluate();
        for (&j, (p, q)) in actor.joints.iter().zip(pose) {
            if let Ok(mut tf) = transforms.get_mut(j) {
                tf.translation = p;
                tf.rotation = q;
            }
        }
    }
}

/// Copies a leader actor's bone transforms onto this actor's bones of the same name
/// (Source's bone merge), e.g. BT's first-person arms following the weapon viewmodel.
#[derive(Component)]
pub struct BoneMerge {
    /// For each of this actor's bones, the leader's bone index.
    pub map: Vec<Option<usize>>,
}

/// Play the sound events of every viewmodel clip whose cycle passed them since last frame. A
/// clip that newly appears near its start counts from 0; one held at a fixed cycle (an end pose
/// used as a base) plays nothing.
pub fn actor_event_sounds(mut commands: Commands, mut actors: Query<&mut Actor>) {
    for mut actor in &mut actors {
        if !actor.event_sounds {
            continue;
        }
        let typed = actor.sound_type.is_some();
        let mut seen: Vec<(usize, f32)> = Vec::new();
        for l in actor.layers.iter().chain(actor.additive.iter()) {
            let c = &actor.clips[l.clip];
            let has = !c.sounds.is_empty() || (typed && !c.typed_sounds.is_empty());
            if l.weight >= 0.3 && has && !seen.iter().any(|(c, _)| *c == l.clip) {
                seen.push((l.clip, l.cycle));
            }
        }
        for &(clip, cur) in &seen {
            let prev = actor.last_cycles.iter().find(|(c, _)| *c == clip).map(|(_, p)| *p);
            let from = match prev {
                Some(p) if cur > p => p,
                Some(p) if cur < p => -1.0, // restarted or looped
                Some(_) => continue,
                None if cur < 0.25 => -1.0,
                None => continue,
            };
            let passed = |at: f32| at > from && at <= cur;
            let c = &actor.clips[clip];
            let mut names: Vec<String> = c.sounds.iter().filter(|(at, _)| passed(*at)).map(|(_, n)| n.clone()).collect();
            if let Some(t) = actor.sound_type {
                names.extend(c.typed_sounds.iter().filter(|(at, _)| passed(*at)).map(|(_, n)| format!("{t}_{n}")));
            }
            for name in names {
                match actor.sound_at {
                    Some(p) => crate::audio::event_at(&mut commands, &name, p),
                    None => crate::audio::event(&mut commands, &name),
                }
            }
        }
        actor.last_cycles = seen;
    }
}

pub fn bone_merge(
    mut commands: Commands,
    followers: Query<(Entity, &Actor, Option<&BoneMerge>), With<BoneMergeTo>>,
    leaders: Query<&Actor, Without<BoneMergeTo>>,
    targets: Query<&BoneMergeTo>,
    mut transforms: Query<&mut Transform>,
) {
    for (e, actor, merge) in &followers {
        let Ok(target) = targets.get(e) else { continue };
        let Ok(leader) = leaders.get(target.0) else { continue };
        let Some(merge) = merge else {
            // Build the bone map once both actors exist.
            let map = actor.bone_names.iter().map(|n| leader.bone_names.iter().position(|l| l.eq_ignore_ascii_case(n))).collect();
            commands.entity(e).insert(BoneMerge { map });
            continue;
        };
        for (i, &j) in actor.joints.iter().enumerate() {
            let Some(li) = merge.map[i] else { continue };
            let Ok(src) = transforms.get(leader.joints[li]).copied() else { continue };
            if let Ok(mut dst) = transforms.get_mut(j) {
                *dst = src;
            }
        }
    }
}

/// Request a bone merge onto the given leader actor entity.
#[derive(Component)]
pub struct BoneMergeTo(pub Entity);

/// The cockpit's main screen is left out (see `spawn_actor`), which would show the parts of the
/// cockpit it covers in the game: drop the triangles that lie behind it (or on it) seen from
/// the camera bone in the first frame of `sequence`.
fn hide_behind_screen(model: &tf_assets::mdl::Model, meshes: &mut [tf_assets::mdl::MeshData], sequence: Option<&str>) {
    let mat = |md: &tf_assets::mdl::MeshData| {
        let tex = model.skin_families.first().and_then(|f| f.get(md.material)).copied().unwrap_or(0) as usize;
        model.textures.get(tex).cloned().unwrap_or_default()
    };
    let mats: Vec<String> = meshes.iter().map(mat).collect();
    if !mats.iter().any(|m| m.to_ascii_lowercase().contains("_int_screen")) {
        return;
    }
    let Some(cam) = model.bone_index("jx_c_camera") else { return };
    let Some(seq) = sequence.and_then(|n| model.sequences.iter().find(|s| s.label.eq_ignore_ascii_case(n))) else { return };
    let Some(pose) = seq.anims.first().and_then(|&a| tf_assets::mdl::screen_cull::pose(model, a).ok()) else { return };
    let hidden = tf_assets::mdl::screen_cull::hidden_triangles(model, meshes, &mats, &pose, cam);
    let mut n = 0;
    for (md, hid) in meshes.iter_mut().zip(&hidden) {
        let kept: Vec<u32> = md.indices.chunks_exact(3).zip(hid).filter(|(_, &h)| !h).flat_map(|(t, _)| t.iter().copied()).collect();
        n += (md.indices.len() - kept.len()) / 3;
        md.indices = kept;
    }
    log::debug!("{n} triangles hidden behind the cockpit screen");
}
