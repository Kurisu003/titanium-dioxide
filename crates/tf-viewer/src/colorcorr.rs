//! The game's last post-processing steps, as full-screen passes on the camera that draws last
//! (the viewmodel camera, [`GameTonemap`]), so they cover the world and the first-person
//! models but not the HUD. Every camera before it renders with Bevy's tonemapping off.
//!
//! - Auto exposure (`exposure_adaptation_ps40` / `logluminance_ps40`, parameters from
//!   client.dll): the geometric mean luminance `L` of the middle `$spotMeteringScreenRatio`
//!   (0.8) of the screen gives a target exposure `key / L` with `key = 1.03 - 2 / (2 +
//!   ln(L + 1))`, times 2^`$exposureCompensation` (0). Each frame the exposure moves toward
//!   it by at most `|d| * rate / max(target, previous)` (`mat_autoexposure_speed` 0.1), and
//!   it is held to `mat_autoexposure_min/max` (0.5 .. 2; Forwardbase Kodai has no
//!   env_tonemap_controller to override them).
//! - Tonemapping (`engine_post_ps50`): the exposed colour through `x (10x + 0.3) / (x (10x +
//!   0.5) + 1.5)`.
//! - Colour correction (`materials/correction/*.raw`: a 32x32x32 RGB lookup table, red
//!   fastest, sRGB on both sides), looked up with the sRGB-encoded tonemapped colour and
//!   blended in by a weight ([`ColorCorrection`]). Phase Shift uses the game's
//!   `mp_ability_shifter.raw` (sh_phase_shift.gnut registers it as its colour correction).

use bevy::asset::{embedded_asset, load_embedded_asset, RenderAssetUsages};
use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::core_pipeline::FullscreenShader;
use bevy::ecs::query::QueryItem;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_graph::{
    NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
};
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_3d, uniform_buffer,
};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use std::sync::atomic::{AtomicUsize, Ordering};

/// client.dll's defaults: `mat_autoexposure_min`, `mat_autoexposure_max`,
/// `mat_autoexposure_speed` and `$spotMeteringScreenRatio`.
const EXPOSURE_MIN: f32 = 0.5;
const EXPOSURE_MAX: f32 = 2.0;
const EXPOSURE_RATE: f32 = 0.1;
const METERING_RATIO: f32 = 0.8;
/// The adaptation step is per frame in the game; scaled to frames of this rate here.
const ADAPT_FPS: f32 = 60.0;

/// TF_EXPOSURE=x holds the exposure at x (the game's mat_autoexposure_force_value, which it
/// hands the shaders as `c_forceExposure`), for calibration.
fn forced_exposure() -> Option<f32> {
    static F: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *F.get_or_init(|| {
        std::env::var("TF_EXPOSURE")
            .ok()
            .and_then(|v| v.parse().ok())
    })
}

/// The map's own colour correction table (its `color_correction` entity), None without one
/// (the flat map).
#[derive(Resource, Default)]
pub struct MapColorCorrection(pub Option<Handle<Image>>);

/// Colour correction on a camera: the lookup table and how much of it applies.
#[derive(Component, Clone)]
pub struct ColorCorrection {
    pub lut: Handle<Image>,
    pub weight: f32,
}

impl ExtractComponent for ColorCorrection {
    type QueryData = Read<ColorCorrection>;
    type QueryFilter = With<Camera>;
    type Out = ColorCorrection;
    fn extract_component(c: QueryItem<'_, '_, Self::QueryData>) -> Option<Self::Out> {
        (c.weight > 0.0).then(|| c.clone())
    }
}

/// The game's exposure, tonemapping and colour correction run on this camera.
#[derive(Component, Clone, Copy, Default)]
pub struct GameTonemap;

impl ExtractComponent for GameTonemap {
    type QueryData = Read<GameTonemap>;
    type QueryFilter = With<Camera>;
    type Out = GameTonemap;
    fn extract_component(c: QueryItem<'_, '_, Self::QueryData>) -> Option<Self::Out> {
        Some(*c)
    }
}

/// A `.raw` colour correction table as a 3D texture (None if it isn't 32x32x32 RGB).
pub fn lut_image(raw: &[u8]) -> Option<Image> {
    const N: u32 = 32;
    if raw.len() != (N * N * N * 3) as usize {
        return None;
    }
    let data: Vec<u8> = raw
        .chunks_exact(3)
        .flat_map(|c| [c[0], c[1], c[2], 255])
        .collect();
    let mut img = Image::new(
        Extent3d {
            width: N,
            height: N,
            depth_or_array_layers: N,
        },
        TextureDimension::D3,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = bevy::image::ImageSampler::linear();
    Some(img)
}

pub struct ColorCorrectionPlugin;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct ColorCorrectionLabel;

impl Plugin for ColorCorrectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MapColorCorrection>();
        embedded_asset!(app, "colorcorr.wgsl");
        app.add_plugins((
            ExtractComponentPlugin::<ColorCorrection>::default(),
            ExtractComponentPlugin::<GameTonemap>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<SpecializedRenderPipelines<CcPipeline>>()
            .init_resource::<CcUniforms>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(
                Render,
                (prepare_pipelines, prepare_uniforms).in_set(RenderSystems::Prepare),
            )
            .add_render_graph_node::<ViewNodeRunner<CcNode>>(Core3d, ColorCorrectionLabel)
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::Tonemapping,
                    ColorCorrectionLabel,
                    Node3d::EndMainPassPostProcessing,
                ),
            );
    }
}

#[derive(Resource)]
struct CcPipeline {
    layout: BindGroupLayoutDescriptor,
    exposure_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen: FullscreenShader,
    shader: Handle<Shader>,
    /// The adapted exposure, 1x1: written to one, the previous frame's read from the other.
    exposure: [TextureView; 2],
    frame: AtomicUsize,
    exposure_pipeline: CachedRenderPipelineId,
    /// Bound when the camera has no colour correction (weight 0).
    no_lut: TextureView,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct CcKey {
    format: TextureFormat,
}

#[derive(Component)]
struct CcPipelineId(CachedRenderPipelineId);

#[derive(ShaderType)]
struct CcSettings {
    weight: f32,
    exposure_min: f32,
    exposure_max: f32,
    compensation: f32,
    /// The adaptation rate times the frames this one stands for.
    rate: f32,
    metering: f32,
    pad0: f32,
    pad1: f32,
}

#[derive(Resource, Default)]
struct CcUniforms(DynamicUniformBuffer<CcSettings>);

#[derive(Component)]
struct CcOffset(u32);

fn init_pipeline(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    fullscreen: Res<FullscreenShader>,
    assets: Res<AssetServer>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "game tonemap layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_3d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<CcSettings>(true),
                texture_2d(TextureSampleType::Float { filterable: false }),
            ),
        ),
    );
    let exposure_layout = BindGroupLayoutDescriptor::new(
        "auto exposure layout",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            (
                (0, texture_2d(TextureSampleType::Float { filterable: true })),
                (1, sampler(SamplerBindingType::Filtering)),
                (4, uniform_buffer::<CcSettings>(true)),
                (
                    5,
                    texture_2d(TextureSampleType::Float { filterable: false }),
                ),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        min_filter: FilterMode::Linear,
        mag_filter: FilterMode::Linear,
        ..default()
    });
    let exposure = std::array::from_fn(|_| {
        // Starts at 0: the first frame takes the target exposure at once.
        device
            .create_texture(&TextureDescriptor {
                label: Some("adapted exposure"),
                size: Extent3d::default(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::R32Float,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default())
    });
    let no_lut = device.create_texture_with_data(
        &queue,
        &TextureDescriptor {
            label: Some("no colour correction"),
            size: Extent3d::default(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D3,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        bevy::render::render_resource::TextureDataOrder::default(),
        &[0, 0, 0, 255],
    );
    let shader = load_embedded_asset!(assets.as_ref(), "colorcorr.wgsl");
    let exposure_pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("auto exposure".into()),
        layout: vec![exposure_layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: shader.clone(),
            entry_point: Some("exposure".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::R32Float,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(CcPipeline {
        layout,
        exposure_layout,
        sampler,
        fullscreen: fullscreen.clone(),
        shader,
        exposure,
        frame: AtomicUsize::new(0),
        exposure_pipeline,
        no_lut: no_lut.create_view(&TextureViewDescriptor::default()),
    });
}

impl SpecializedRenderPipeline for CcPipeline {
    type Key = CcKey;
    fn specialize(&self, key: CcKey) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("game tonemap".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: key.format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

fn prepare_pipelines(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<CcPipeline>>,
    pipeline: Res<CcPipeline>,
    views: Query<(Entity, &ExtractedView), With<GameTonemap>>,
) {
    for (e, view) in &views {
        let format = if view.hdr {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };
        let id = pipelines.specialize(&cache, &pipeline, CcKey { format });
        commands.entity(e).insert(CcPipelineId(id));
    }
}

fn prepare_uniforms(
    mut commands: Commands,
    mut uniforms: ResMut<CcUniforms>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    time: Res<Time>,
    views: Query<(Entity, Option<&ColorCorrection>), With<GameTonemap>>,
) {
    uniforms.0.clear();
    let frames = (time.delta_secs() * ADAPT_FPS).min(10.0);
    for (e, c) in &views {
        let (exposure_min, exposure_max) = match forced_exposure() {
            Some(x) => (x, x),
            None => (EXPOSURE_MIN, EXPOSURE_MAX),
        };
        let off = uniforms.0.push(&CcSettings {
            weight: c.map_or(0.0, |c| c.weight.clamp(0.0, 1.0)),
            exposure_min,
            exposure_max,
            compensation: 0.0,
            rate: EXPOSURE_RATE * frames,
            metering: METERING_RATIO,
            pad0: 0.0,
            pad1: 0.0,
        });
        commands.entity(e).insert(CcOffset(off));
    }
    uniforms.0.write_buffer(&device, &queue);
}

#[derive(Default)]
struct CcNode;

impl ViewNode for CcNode {
    type ViewQuery = (
        Read<ViewTarget>,
        Read<CcPipelineId>,
        Option<Read<ColorCorrection>>,
        Read<CcOffset>,
    );

    fn run<'w>(
        &self,
        _: &mut RenderGraphContext,
        ctx: &mut RenderContext<'w>,
        (target, id, cc, off): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let cache = world.resource::<PipelineCache>();
        let pipeline = world.resource::<CcPipeline>();
        let (Some(render_pipeline), Some(exposure_pipeline)) = (
            cache.get_render_pipeline(id.0),
            cache.get_render_pipeline(pipeline.exposure_pipeline),
        ) else {
            return Ok(());
        };
        let lut = match cc {
            Some(cc) => match world.resource::<RenderAssets<GpuImage>>().get(&cc.lut) {
                Some(l) => &l.texture_view,
                None => &pipeline.no_lut,
            },
            None => &pipeline.no_lut,
        };
        let Some(uniform) = world.resource::<CcUniforms>().0.binding() else {
            return Ok(());
        };
        let frame = pipeline.frame.fetch_add(1, Ordering::Relaxed);
        let (cur, prev) = (
            &pipeline.exposure[frame % 2],
            &pipeline.exposure[(frame + 1) % 2],
        );
        let post = target.post_process_write();

        // The adapted exposure for this frame.
        let bind_group = ctx.render_device().create_bind_group(
            Some("auto exposure"),
            &cache.get_bind_group_layout(&pipeline.exposure_layout),
            &BindGroupEntries::with_indices((
                (0, post.source),
                (1, &pipeline.sampler),
                (4, uniform.clone()),
                (5, prev),
            )),
        );
        {
            let mut pass = ctx
                .command_encoder()
                .begin_render_pass(&RenderPassDescriptor {
                    label: Some("auto exposure"),
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: cur,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations::default(),
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            pass.set_pipeline(exposure_pipeline);
            pass.set_bind_group(0, &bind_group, &[off.0]);
            pass.draw(0..3, 0..1);
        }

        let bind_group = ctx.render_device().create_bind_group(
            Some("game tonemap"),
            &cache.get_bind_group_layout(&pipeline.layout),
            &BindGroupEntries::sequential((
                post.source,
                &pipeline.sampler,
                lut,
                &pipeline.sampler,
                uniform,
                cur,
            )),
        );
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("game tonemap"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: post.destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations::default(),
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        pass.set_pipeline(render_pipeline);
        pass.set_bind_group(0, &bind_group, &[off.0]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}
