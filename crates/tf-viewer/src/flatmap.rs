//! `--map flat`: a built-in test map, a flat plane with a grid, for movement tests with no
//! geometry in the way. Nothing is read from the game for it.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use tf_sim::collision::CollisionWorld;
use tf_sim::glam::Vec3 as SVec3;

/// The map name that selects this map.
pub const NAME: &str = "flat";
/// Half the side of the plane (units).
const HALF: f32 = 8192.0;
/// The plane is tiled this finely, so the collision grid and the lighting cascades see
/// ordinary-sized triangles.
const TILE: f32 = 512.0;
/// One texture repeat covers this many units; it holds four grid cells of 64 units.
const TEX_UNITS: f32 = 256.0;

/// Spawn the plane under `root` (game units) and add its collision.
pub fn spawn(
    commands: &mut Commands,
    root: Entity,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    collision: &mut CollisionWorld,
) {
    let n = (2.0 * HALF / TILE) as usize;
    let mut pos = Vec::with_capacity((n + 1) * (n + 1));
    let mut nrm = Vec::with_capacity(pos.capacity());
    let mut uv = Vec::with_capacity(pos.capacity());
    for j in 0..=n {
        for i in 0..=n {
            let (x, y) = (-HALF + i as f32 * TILE, -HALF + j as f32 * TILE);
            pos.push([x, y, 0.0]);
            nrm.push([0.0, 0.0, 1.0]);
            uv.push([x / TEX_UNITS, y / TEX_UNITS]);
        }
    }
    let mut idx = Vec::with_capacity(n * n * 6);
    for j in 0..n {
        for i in 0..n {
            let a = (j * (n + 1) + i) as u32;
            let (b, c, d) = (a + 1, a + (n + 1) as u32, a + (n + 1) as u32 + 1);
            idx.extend([a, b, d, a, d, c]);
        }
    }
    for t in idx.chunks_exact(3) {
        let v = |i: u32| SVec3::from(pos[i as usize]);
        collision.add(v(t[0]), v(t[1]), v(t[2]));
    }
    let mesh = meshes.add(crate::convert::build_mesh(pos, nrm, uv, idx, None));
    let mat = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(grid_image())),
        perceptual_roughness: 0.9,
        reflectance: 0.2,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let e = commands
        .spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY))
        .id();
    commands.entity(root).add_child(e);
    log::info!(
        "flat test map: {}x{} units, {} tiles",
        2.0 * HALF,
        2.0 * HALF,
        n * n
    );
}

/// A light grey texture with dark lines every quarter (64 units) and a darker line on the
/// repeat (256 units), so speed and distance can be read off the ground.
fn grid_image() -> Image {
    const N: usize = 256;
    let mut data = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let major = x < 3 || y < 3;
            let minor = x % 64 < 2 || y % 64 < 2;
            let v: u8 = if major {
                0x40
            } else if minor {
                0x80
            } else {
                0xb8
            };
            data.extend([v, v, v, 255]);
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: N as u32,
            height: N as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = bevy::image::ImageSampler::Descriptor(crate::convert::sampler());
    img
}
