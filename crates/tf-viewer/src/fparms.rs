//! First-person sequences on the Pilot's arms (`pov_pilot_medium_reaper_m.mdl`): the game's
//! `FirstPersonSequence` for embarking (`ptpov_mount_buddy_*`), disembarking
//! (`ptpov_dismount_*`), ejecting (`atpov_cockpit_eject`) and the rodeo (rodeo.rs drives the
//! same arms itself). The arms are parented to a frame (the Titan's `HIJACK` attachment, or
//! the cockpit eye for `atpov` clips) and the view follows their `jx_c_camera` bone
//! (attachment frame: forward = bone Z, up = bone Y), blending in from where the view was.

use crate::actor::{spawn_actor, Actor, ActorSpec, Layer};
use crate::player::{to_bevy, MainCamera};
use bevy::prelude::*;

/// The campaign Pilot's first-person arms (pilot_solo.set `armsmodel`: Jack Cooper's, from
/// the SP VPKs; the MP Pilots use pov_pilot_medium_reaper_m).
pub const PILOT_ARMS_MODEL: &str = "models/weapons/arms/pov_mlt_hero_jack.mdl";

/// The arms model (TF_ARMS=path overrides it, for testing).
pub fn arms_model() -> &'static str {
    static M: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    M.get_or_init(|| std::env::var("TF_ARMS").unwrap_or_else(|_| PILOT_ARMS_MODEL.into()))
}

/// The camera bone's frame: forward = bone Z, left = bone X, up = bone Y.
pub fn attach_frame() -> Quat {
    Quat::from_axis_angle(Vec3::ONE.normalize(), -2.0 * std::f32::consts::FRAC_PI_3)
}

/// The arms actor, under the world root (game units).
#[derive(Resource, Clone, Copy)]
pub struct FpArms {
    pub anchor: Entity,
    pub actor: Entity,
}

pub struct FpActive {
    pub clip: String,
    pub t: f32,
    /// Where the arms' root sits (game space): position and rotation (x forward).
    pub frame: (Vec3, Quat),
    /// For `atpov` cockpit clips: the frame is the eye, and the root is placed so the camera
    /// bone's first pose lands on it.
    pub eye_anchored: bool,
    /// The view the sequence blends in from.
    pub from: Option<(Vec3, Quat)>,
    pub blend: f32,
    pub len: f32,
}

#[derive(Resource, Default)]
pub struct FpSeq {
    pub active: Option<FpActive>,
    /// The view this frame (game space: eye, rotation with forward X / up Z).
    pub cam: Option<(Vec3, Quat)>,
}

impl FpSeq {
    pub fn start(&mut self, clip: &str, frame: (Vec3, Quat), from: Option<(Vec3, Quat)>, eye_anchored: bool) {
        log::info!("first-person sequence: {clip}");
        self.active = Some(FpActive { clip: clip.into(), t: 0.0, frame, eye_anchored, from, blend: if from.is_some() { 0.35 } else { 0.0 }, len: 1.0 });
    }
    pub fn stop(&mut self) {
        self.active = None;
        self.cam = None;
    }
    pub fn playing(&self) -> bool {
        self.active.is_some()
    }
    pub fn set_frame(&mut self, frame: (Vec3, Quat)) {
        if let Some(a) = self.active.as_mut() {
            a.frame = frame;
        }
    }
}

/// Every clip the arms may play.
fn sequences() -> Vec<String> {
    let mut v = Vec::new();
    for c in ["atlas", "ogre", "stryder"] {
        for d in ["front", "back", "left", "right", "front_lower", "back_lower", "back_mid"] {
            v.push(format!("ptpov_rodeo_move_{c}_{d}_entrance"));
        }
    }
    for s in [
        "ptpov_rodeo_move_atlas_back_idle",
        "ptpov_rodeo_move_stryder_back_idle",
        "ptpov_rodeo_ride_R_hijack_battery",
        "ptpov_rodeo_ogre_R_hijack_battery",
        "ptpov_rodeo_stryder_R_hijack_battery",
        "ptpov_rodeo_medium_grenade_1st",
        "ptpov_rodeo_heavy_grenade_1st",
        "ptpov_rodeo_light_grenade_1st",
        "ptpov_mount_buddy_stand_front",
        "ptpov_mount_buddy_stand_behind",
        "ptpov_mount_buddy_kneel_front",
        "ptpov_mount_buddy_kneel_behind",
        "ptpov_mount_buddy_kneel_left",
        "ptpov_mount_buddy_kneel_right",
        "ptpov_dismount_buddy_stand",
        "ptpov_dismount_buddy_crouch",
        "ptpov_dismount_atlas_stand",
        "atpov_cockpit_eject",
    ] {
        v.push(s.into());
    }
    v
}

/// Build the arms once the world root exists (loading).
#[allow(clippy::too_many_arguments)]
pub fn spawn_arms(
    mut commands: Commands,
    mut done: Local<bool>,
    gd: Res<crate::gamedata::GameData>,
    root: Option<Res<crate::pilotweapon::WorldRoot>>,
    mut cache: ResMut<crate::convert::Cache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    if *done {
        return;
    }
    let Some(root) = root else { return };
    *done = true;
    let seqs = sequences();
    let refs: Vec<&str> = seqs.iter().map(String::as_str).collect();
    let anchor = commands.spawn((Transform::IDENTITY, Visibility::Hidden, crate::vmcam::Viewmodel)).id();
    commands.entity(root.0).add_child(anchor);
    let spec = ActorSpec { path: arms_model(), sequences: &refs, grids: &[], body: &[] };
    match spawn_actor(&mut commands, anchor, &gd, &spec, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes) {
        Ok(a) => commands.insert_resource(FpArms { anchor, actor: a.entity }),
        Err(err) => log::warn!("first-person arms: {err:#}"),
    }
}

/// Advance the sequence, pose the arms and work out the view.
pub fn fp_seq_update(
    time: Res<Time>,
    mut seq: ResMut<FpSeq>,
    arms: Option<Res<FpArms>>,
    rodeo: Res<crate::rodeo::Rodeo>,
    mut actors: Query<&mut Actor>,
    mut tfs: Query<(&mut Transform, &mut Visibility)>,
) {
    let Some(arms) = arms else { return };
    if rodeo.riding() {
        // rodeo.rs drives the arms.
        return;
    }
    let Some(a) = seq.active.as_mut() else {
        seq.cam = None;
        if let Ok((_, mut vis)) = tfs.get_mut(arms.anchor) {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
        }
        return;
    };
    let Ok(mut actor) = actors.get_mut(arms.actor) else { return };
    let Some(clip) = actor.clip(&a.clip) else {
        log::warn!("first-person sequence {} not on the arms", a.clip);
        seq.stop();
        return;
    };
    a.len = actor.clips[clip].duration.max(0.05);
    let dt = time.delta_secs().min(0.1);
    a.t += dt;
    let cycle = (a.t / a.len).min(0.999);
    actor.autoplay = false;
    actor.aim = None;
    let (frame_pos, frame_rot) = a.frame;
    let (root_pos, root_rot) = if a.eye_anchored {
        actor.layers = vec![Layer { clip, cycle: 0.0, weight: 1.0 }];
        let cam0 = actor.bone_model_transform("jx_c_camera").map(|c| c.translation).unwrap_or(Vec3::ZERO);
        actor.layers = vec![Layer { clip, cycle, weight: 1.0 }];
        (frame_pos - frame_rot * cam0, frame_rot)
    } else {
        actor.layers = vec![Layer { clip, cycle, weight: 1.0 }];
        // The clip's `jx_c_start` bone marks the sequence's reference point (the hijack
        // frame) in the clip's root space: it walks back as the dismount carries the Pilot
        // off the hatch, and starts where the Pilot stood for a mount. Place the arms so it
        // stays on the frame: root = frame * start_aligned * start(t)^-1.
        // The aligned start: the root bone's standard Y-up to Z-up turn (it animates in the
        // mount clips, so not its current pose).
        let s0 = Transform::from_rotation(Quat::from_xyzw(0.5, 0.5, 0.5, 0.5));
        match (Some(s0), actor.bone_model_transform("jx_c_start")) {
            (Some(s0), Some(st)) => {
                let frame = Transform::from_translation(frame_pos).with_rotation(frame_rot);
                let root = frame.compute_affine() * s0.compute_affine() * st.compute_affine().inverse();
                let (_, r, t) = root.to_scale_rotation_translation();
                (t, r)
            }
            _ => (frame_pos, frame_rot),
        }
    };
    let mut view = (frame_pos, frame_rot);
    if let Some(c) = actor.bone_model_transform("jx_c_camera") {
        view = (root_pos + root_rot * c.translation, root_rot * c.rotation * attach_frame());
    }
    if let Ok((mut tf, mut vis)) = tfs.get_mut(arms.anchor) {
        tf.translation = root_pos;
        tf.rotation = root_rot;
        *vis = Visibility::Inherited;
    }
    let (eye, rot) = match a.from {
        Some((p, r)) if a.blend > 0.0 && a.t < a.blend => {
            let k = a.t / a.blend;
            let k = k * k * (3.0 - 2.0 * k);
            (p.lerp(view.0, k), r.slerp(view.1, k))
        }
        _ => view,
    };
    seq.cam = Some((eye, rot));
}

/// The view while a sequence plays (after `pilot_camera` and the rodeo camera).
pub fn fp_seq_camera(seq: Res<FpSeq>, mut cams: Query<&mut Transform, With<MainCamera>>) {
    let Some((eye, rot)) = seq.cam else { return };
    if let Ok(mut cam) = cams.single_mut() {
        *cam = Transform::from_translation(to_bevy(eye)).looking_to(to_bevy(rot * Vec3::X).normalize(), to_bevy(rot * Vec3::Z).normalize());
    }
}
