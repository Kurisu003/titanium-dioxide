//! Titan weak points: the AR markers on enemy Titans (bodypart "hitpoints", material
//! models/titans/titan_ar_marker; crit hitboxes are hitbox.rs's business). The game shows them
//! through the marker material's alpha, ShouldShowWeakpoints (cl_material_proxies.gnut): the
//! Titan is alive and an enemy, not cloaked, its soul has no shield, it isn't the one being
//! ridden, the player's weapon isn't disabled, the active weapon can crit (critical_hit), and
//! the horizontal direction to its world-space centre is within acos(0.996) of the view.

use crate::actor::Weakpoint;
use crate::pilotctl::{Control, PilotSettings, PlayerPilot};
use crate::pilotweapon::PilotLoadout;
use crate::player::CameraMode;
use crate::targets::Enemy;
use bevy::prelude::*;

/// Cosine of the view angle inside which the markers show (ShouldShowWeakpoints).
const VIEW_DOT: f32 = 0.996;

#[allow(clippy::too_many_arguments)]
pub fn update_weakpoints(
    control: Res<Control>,
    mode: Res<CameraMode>,
    rodeo: Res<crate::rodeo::Rodeo>,
    settings: Res<PilotSettings>,
    pilots: Query<(&PlayerPilot, &PilotLoadout)>,
    enemies: Query<(Entity, &Enemy)>,
    children: Query<&Children>,
    mut marks: Query<&mut Visibility, With<Weakpoint>>,
    net: Res<crate::net::Net>,
) {
    let Ok((pilot, loadout)) = pilots.single() else {
        return;
    };
    // Only a Pilot's gun crits (the Titan weapons' critical_hit is off), and only in play.
    let can_crit = *control == Control::Pilot
        && *mode != CameraMode::Free
        && !rodeo.riding()
        && loadout.guns.get(loadout.active).is_some_and(|g| g.def.crit);
    // TF_WEAKPOINTS=1: always on (checking the markers themselves; the Ion's are on its chest).
    let force = std::env::var_os("TF_WEAKPOINTS").is_some();
    let eye = Vec3::from(pilot.state.eye(&settings.0).to_array());
    let view = Vec2::new(pilot.state.yaw.cos(), pilot.state.yaw.sin());
    let ridden = rodeo.ride.as_ref().map(|r| r.target);
    let debug = log::log_enabled!(target: "tf_viewer::weakpoints", log::Level::Debug);
    for (id, e) in &enemies {
        let Ok(kids) = children.get(e.actor) else {
            continue;
        };
        if debug && !e.infantry && e.active {
            let centre = e.pos + Vec3::Z * (e.height * 0.5);
            let dot = (centre - eye).truncate().normalize_or_zero().dot(view);
            let n = kids.iter().filter(|k| marks.contains(*k)).count();
            log::debug!(
                "weak points: titan {:?}: crit {can_crit} alive {} shield {:.0} dot {dot:.3} marker meshes {n}",
                e.actor, e.alive(), e.v.shield
            );
        }
        let show = force
            || can_crit
                && e.active
                && e.alive()
                && !e.infantry
                // Team mates' Titans (PvE) show no weak points.
                && !(e.remote.is_some() && !net.pvp())
                && e.v.shield <= 0.0
                && ridden != Some(id)
                && {
                    let centre = e.pos + Vec3::Z * (e.height * 0.5);
                    let to = (centre - eye).truncate().normalize_or_zero();
                    to.dot(view) >= VIEW_DOT
                };
        let want = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for k in kids.iter() {
            if let Ok(mut v) = marks.get_mut(k) {
                v.set_if_neq(want);
            }
        }
    }
}
