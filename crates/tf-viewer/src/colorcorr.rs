//! Source colour correction (`materials/correction/*.raw`: a 32x32x32 RGB lookup table, red
//! fastest) as a full-screen pass after tonemapping, blended in by a weight. Put a
//! [`ColorCorrection`] on the camera that draws last (the viewmodel camera), so it covers the
//! world and the first-person models but not the HUD. Phase Shift uses the game's
//! `mp_ability_shifter.raw` (sh_phase_shift.gnut registers it as its colour correction).

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
        TextureFormat::Rgba8Unorm,
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
        app.add_plugins(ExtractComponentPlugin::<ColorCorrection>::default());
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
    sampler: Sampler,
    fullscreen: FullscreenShader,
    shader: Handle<Shader>,
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
    pad0: f32,
    pad1: f32,
    pad2: f32,
}

#[derive(Resource, Default)]
struct CcUniforms(DynamicUniformBuffer<CcSettings>);

#[derive(Component)]
struct CcOffset(u32);

fn init_pipeline(
    mut commands: Commands,
    device: Res<RenderDevice>,
    fullscreen: Res<FullscreenShader>,
    assets: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "colour correction layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_3d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<CcSettings>(true),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        min_filter: FilterMode::Linear,
        mag_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(CcPipeline {
        layout,
        sampler,
        fullscreen: fullscreen.clone(),
        shader: load_embedded_asset!(assets.as_ref(), "colorcorr.wgsl"),
    });
}

impl SpecializedRenderPipeline for CcPipeline {
    type Key = CcKey;
    fn specialize(&self, key: CcKey) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("colour correction".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
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
    views: Query<(Entity, &ExtractedView), With<ColorCorrection>>,
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
    views: Query<(Entity, &ColorCorrection)>,
) {
    uniforms.0.clear();
    for (e, c) in &views {
        let off = uniforms.0.push(&CcSettings {
            weight: c.weight.clamp(0.0, 1.0),
            pad0: 0.0,
            pad1: 0.0,
            pad2: 0.0,
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
        Read<ColorCorrection>,
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
        let Some(render_pipeline) = cache.get_render_pipeline(id.0) else {
            return Ok(());
        };
        let Some(lut) = world.resource::<RenderAssets<GpuImage>>().get(&cc.lut) else {
            return Ok(());
        };
        let Some(uniform) = world.resource::<CcUniforms>().0.binding() else {
            return Ok(());
        };
        let post = target.post_process_write();
        let bind_group = ctx.render_device().create_bind_group(
            Some("colour correction"),
            &cache.get_bind_group_layout(&pipeline.layout),
            &BindGroupEntries::sequential((
                post.source,
                &pipeline.sampler,
                &lut.texture_view,
                &pipeline.sampler,
                uniform,
            )),
        );
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("colour correction"),
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
