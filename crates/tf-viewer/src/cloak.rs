//! The Pilot's cloak in first person: while Cloak (`mp_ability_cloak`) runs, the arms and gun
//! draw as a refracting outline (cloak.wgsl) instead of their own materials. The game's cloak
//! look is engine-side (its scripts only call `EnableCloak`), so the strength of the warp, the
//! tint and the rim are by eye.

use crate::pilotctl::Control;
use crate::pilotability::PilotStatus;
use bevy::camera::visibility::RenderLayers;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};

#[derive(Clone, Copy, Debug, Default, ShaderType, Reflect)]
pub struct CloakParams {
    /// x: refraction offset (fraction of the screen), y: rim strength, z: rim exponent, w: tint.
    pub params: Vec4,
    pub rim_color: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct CloakExt {
    #[uniform(100)]
    pub params: CloakParams,
}

impl MaterialExtension for CloakExt {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://tf_viewer/cloak.wgsl".into()
    }
}

pub type CloakMaterial = ExtendedMaterial<StandardMaterial, CloakExt>;

pub struct CloakPlugin;

impl Plugin for CloakPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "cloak.wgsl");
        app.add_plugins(MaterialPlugin::<CloakMaterial>::default());
    }
}

/// A viewmodel mesh drawn cloaked, with its own material to put back.
#[derive(Component)]
pub struct Cloaked(Handle<StandardMaterial>);

fn tuned(d: Vec4) -> Vec4 {
    let v: Vec<f32> = std::env::var("TF_CLOAK").unwrap_or_default().split_whitespace().filter_map(|x| x.parse().ok()).collect();
    if v.len() == 4 { Vec4::new(v[0], v[1], v[2], v[3]) } else { d }
}

/// Swap the first-person meshes to the cloak material while the Pilot is cloaked.
#[allow(clippy::type_complexity)]
pub fn cloak_viewmodel(
    mut commands: Commands,
    status: Res<PilotStatus>,
    control: Res<Control>,
    mut mats: ResMut<Assets<CloakMaterial>>,
    mut shared: Local<Option<Handle<CloakMaterial>>>,
    meshes: Query<(Entity, &MeshMaterial3d<StandardMaterial>, &RenderLayers), Without<Cloaked>>,
    cloaked: Query<(Entity, &Cloaked)>,
) {
    let on = status.cloaked() && *control == Control::Pilot && std::env::var_os("TF_NO_CLOAK_FX").is_none();
    if on {
        let mat = shared
            .get_or_insert_with(|| {
                mats.add(CloakMaterial {
                    // In the transmissive phase (specular_transmission), drawn opaque-style.
                    base: StandardMaterial { specular_transmission: 1.0, unlit: true, alpha_mode: AlphaMode::Opaque, ..default() },
                    // TF_CLOAK="warp rim exponent tint" overrides the look (testing).
                    extension: CloakExt { params: CloakParams { params: tuned(Vec4::new(0.016, 0.05, 3.0, 0.3)), rim_color: Vec4::new(0.55, 0.75, 1.0, 1.0) } },
                })
            })
            .clone();
        for (e, m, layers) in &meshes {
            if layers.intersects(&RenderLayers::layer(crate::vmcam::VM_LAYER)) {
                commands.entity(e).remove::<MeshMaterial3d<StandardMaterial>>().insert((MeshMaterial3d(mat.clone()), Cloaked(m.0.clone())));
            }
        }
    } else {
        for (e, c) in &cloaked {
            commands.entity(e).remove::<(MeshMaterial3d<CloakMaterial>, Cloaked)>().insert(MeshMaterial3d(c.0.clone()));
        }
    }
}
