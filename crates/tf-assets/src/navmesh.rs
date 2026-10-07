//! Titanfall 2 navmeshes (`maps/navmesh/<map>_<size>.nm`): Respawn's build of Detour's tiled
//! mesh (`MSET` set header, `DNAV` tiles, both stored as little-endian ints: `TESM`, `VAND`).
//! Layout worked out from `sp_beacon_large.nm`:
//!
//! - set header: magic `MSET`, version 5, tile count, `dtNavMeshParams` (origin, tile width
//!   and height, max tiles, max polys), then three ints (poly group count, reachability
//!   table size and count; that table follows the tiles and isn't read here).
//! - per tile: `tileRef`, `dataSize`, then a 104-byte header: magic `DNAV`, version 13, x, y,
//!   layer, userId, polyCount, polyMapCount, vertCount, maxLinkCount, detailMeshCount,
//!   detailVertCount, detailTriCount, bvNodeCount, offMeshConCount, offMeshBase,
//!   walkableHeight/Radius/Climb, bmin, bmax, bvQuantFactor.
//! - verts (3 floats), polys (48 bytes: firstLink, verts[6], neis[6], flags, vertCount,
//!   areaAndtype, two shorts, centre[3]), polyMapCount ints per poly, links (16 bytes: ref, next, edge,
//!   side, bmin, bmax, 4 more), then detail meshes / verts / tris, the BV tree and off-mesh
//!   connections, which are skipped by `dataSize`.
//!
//! Coordinates are game units and axes (Z up). Poly refs are Detour's: salt, tile index
//! (`log2(maxTiles)` bits) and poly index (`log2(maxPolys)` bits).
use anyhow::{anyhow, bail, Result};
use std::collections::HashMap;

/// One walkable polygon (convex, up to 6 vertices).
#[derive(Debug, Clone)]
pub struct Poly {
    pub verts: Vec<[f32; 3]>,
    pub center: [f32; 3],
    /// Indices into `NavMesh::polys` of the polys linked to this one (internal and cross-tile).
    pub links: Vec<u32>,
    pub flags: u16,
    pub area: u8,
}

#[derive(Debug, Clone, Default)]
pub struct NavMesh {
    pub polys: Vec<Poly>,
    pub walkable_height: f32,
    pub walkable_radius: f32,
    pub walkable_climb: f32,
    pub tiles: usize,
    /// Off-mesh (jump) links found but not kept.
    pub off_mesh: usize,
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| anyhow!("navmesh: truncated at {o}"))
}
fn i32_at(d: &[u8], o: usize) -> Result<i32> {
    Ok(u32_at(d, o)? as i32)
}
fn f32_at(d: &[u8], o: usize) -> Result<f32> {
    Ok(f32::from_bits(u32_at(d, o)?))
}
fn u16_at(d: &[u8], o: usize) -> Result<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| anyhow!("navmesh: truncated at {o}"))
}

fn bits_for(n: u32) -> u32 {
    (32 - n.saturating_sub(1).leading_zeros()).max(1)
}

const TILE_HEADER: usize = 104;
const POLY_SIZE: usize = 48;
const LINK_SIZE: usize = 16;
/// Off-mesh connection polys (jump links) have this type in `areaAndtype`'s top bits.
const POLYTYPE_OFFMESH: u8 = 1;

pub fn parse(d: &[u8]) -> Result<NavMesh> {
    if d.get(0..4) != Some(b"TESM") {
        bail!("navmesh: not an MSET file");
    }
    let version = i32_at(d, 4)?;
    if version != 5 {
        bail!("navmesh: version {version}, expected 5");
    }
    let tiles = i32_at(d, 8)?.max(0) as usize;
    let max_tiles = i32_at(d, 32)?.max(1) as u32;
    let max_polys = i32_at(d, 36)?.max(1) as u32;
    let poly_bits = bits_for(max_polys);
    let tile_bits = bits_for(max_tiles);
    let poly_mask = (1u32 << poly_bits) - 1;
    let tile_mask = (1u32 << tile_bits) - 1;
    let mut off = 52;

    struct RawTile {
        index: u32,
        first: usize,
        polys: Vec<(Poly, u32, Vec<u32>)>, // poly, first link, link refs
    }
    let mut raw: Vec<RawTile> = Vec::with_capacity(tiles);
    let mut mesh = NavMesh::default();
    let mut global = 0usize;
    for _ in 0..tiles {
        let tile_ref = u32_at(d, off)?;
        let size = i32_at(d, off + 4)?.max(0) as usize;
        off += 8;
        let t = &d[off..(off + size).min(d.len())];
        if t.get(0..4) != Some(b"VAND") {
            bail!("navmesh: tile without DNAV magic");
        }
        let poly_count = i32_at(t, 24)?.max(0) as usize;
        // Number of per-poly int arrays (one per "poly map") stored after the polys.
        let poly_maps = i32_at(t, 28)?.clamp(0, 16) as usize;
        let vert_count = i32_at(t, 32)?.max(0) as usize;
        let max_links = i32_at(t, 36)?.max(0) as usize;
        mesh.walkable_height = f32_at(t, 64)?;
        mesh.walkable_radius = f32_at(t, 68)?;
        mesh.walkable_climb = f32_at(t, 72)?;
        let mut p = TILE_HEADER;
        let mut verts = Vec::with_capacity(vert_count);
        for i in 0..vert_count {
            let o = p + i * 12;
            verts.push([f32_at(t, o)?, f32_at(t, o + 4)?, f32_at(t, o + 8)?]);
        }
        p += vert_count * 12;
        let links_at = p + poly_count * POLY_SIZE + poly_count * 4 * poly_maps;
        let mut polys = Vec::with_capacity(poly_count);
        for i in 0..poly_count {
            let o = p + i * POLY_SIZE;
            let first_link = u32_at(t, o)?;
            let vc = (t.get(o + 30).copied().unwrap_or(0) as usize).min(6);
            let area_type = t.get(o + 31).copied().unwrap_or(0);
            let flags = u16_at(t, o + 28)?;
            let mut pv = Vec::with_capacity(vc);
            for k in 0..vc {
                let vi = u16_at(t, o + 4 + k * 2)? as usize;
                pv.push(*verts.get(vi).ok_or_else(|| anyhow!("navmesh: vertex {vi} of {vert_count}"))?);
            }
            let center = [f32_at(t, o + 36)?, f32_at(t, o + 40)?, f32_at(t, o + 44)?];
            // Walk the link chain.
            let mut refs = Vec::new();
            let mut l = first_link;
            let mut guard = 0;
            while l != u32::MAX && (l as usize) < max_links && guard < 64 {
                let lo = links_at + l as usize * LINK_SIZE;
                refs.push(u32_at(t, lo)?);
                l = u32_at(t, lo + 4)?;
                guard += 1;
            }
            let off_mesh = (area_type >> 6) == POLYTYPE_OFFMESH;
            if off_mesh {
                mesh.off_mesh += 1;
            }
            polys.push((Poly { verts: pv, center, links: Vec::new(), flags, area: area_type & 0x3f }, if off_mesh { u32::MAX } else { first_link }, refs));
        }
        raw.push(RawTile { index: (tile_ref >> poly_bits) & tile_mask, first: global, polys });
        global += poly_count;
        off += size;
    }
    // Resolve link refs to global poly indices.
    let by_tile: HashMap<u32, usize> = raw.iter().enumerate().map(|(i, t)| (t.index, i)).collect();
    let mut out = Vec::with_capacity(global);
    for t in &raw {
        for (poly, first, refs) in &t.polys {
            let mut p = poly.clone();
            if *first != u32::MAX {
                for &r in refs {
                    let ti = (r >> poly_bits) & tile_mask;
                    let pi = (r & poly_mask) as usize;
                    if let Some(&rt) = by_tile.get(&ti) {
                        let rtile = &raw[rt];
                        if let Some((rp, rfirst, _)) = rtile.polys.get(pi) {
                            if *rfirst != u32::MAX && !rp.verts.is_empty() {
                                p.links.push((rtile.first + pi) as u32);
                            }
                        }
                    }
                }
            }
            out.push(p);
        }
    }
    mesh.polys = out;
    mesh.tiles = tiles;
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a one-tile set with two linked squares.
    fn two_squares() -> Vec<u8> {
        let mut d = Vec::new();
        let push_i = |d: &mut Vec<u8>, v: i32| d.extend_from_slice(&v.to_le_bytes());
        let push_f = |d: &mut Vec<u8>, v: f32| d.extend_from_slice(&v.to_le_bytes());
        d.extend_from_slice(b"TESM");
        push_i(&mut d, 5);
        push_i(&mut d, 1);
        for v in [0.0, 0.0, 0.0, 960.0, 960.0] {
            push_f(&mut d, v);
        }
        push_i(&mut d, 2048);
        push_i(&mut d, 2048);
        push_i(&mut d, 1);
        push_i(&mut d, 0);
        push_i(&mut d, 0);
        // Tile.
        let mut t = Vec::new();
        t.extend_from_slice(b"VAND");
        for v in [13, 0, 0, 0, 0, 2, 1, 6, 2, 2, 0, 0, 4, 0, 2] {
            push_i(&mut t, v);
        }
        for v in [225.0, 60.0, 80.0, 0.0, 0.0, 0.0, 200.0, 100.0, 10.0, 1.0] {
            push_f(&mut t, v);
        }
        assert_eq!(t.len(), TILE_HEADER);
        for v in [[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 100.0, 0.0], [0.0, 100.0, 0.0], [200.0, 0.0, 0.0], [200.0, 100.0, 0.0]] {
            for c in v {
                push_f(&mut t, c);
            }
        }
        let tile_ref = 1u32 << 22;
        // Poly 0: verts 0,1,2,3; link 0 -> poly 1. Poly 1: verts 1,4,5,2; link 1 -> poly 0.
        for (first, verts, center) in [(0u32, [0u16, 1, 2, 3, 0, 0], [50.0, 50.0, 0.0]), (1, [1, 4, 5, 2, 0, 0], [150.0, 50.0, 0.0])] {
            t.extend_from_slice(&first.to_le_bytes());
            for v in verts {
                t.extend_from_slice(&v.to_le_bytes());
            }
            for _ in 0..6 {
                t.extend_from_slice(&0u16.to_le_bytes());
            }
            t.extend_from_slice(&1u16.to_le_bytes());
            t.push(4);
            t.push(0);
            t.extend_from_slice(&[0u8; 4]);
            for c in center {
                push_f(&mut t, c);
            }
        }
        push_i(&mut t, 0);
        push_i(&mut t, 0);
        for r in [tile_ref | 1, tile_ref] {
            t.extend_from_slice(&r.to_le_bytes());
            t.extend_from_slice(&u32::MAX.to_le_bytes());
            t.extend_from_slice(&[0u8; 8]);
        }
        d.extend_from_slice(&tile_ref.to_le_bytes());
        push_i(&mut d, t.len() as i32);
        d.extend_from_slice(&t);
        d
    }

    #[test]
    fn parses_links() {
        let m = parse(&two_squares()).unwrap();
        assert_eq!(m.polys.len(), 2);
        assert_eq!(m.polys[0].links, vec![1]);
        assert_eq!(m.polys[1].links, vec![0]);
        assert_eq!(m.polys[1].verts[1], [200.0, 0.0, 0.0]);
        assert_eq!(m.walkable_radius, 60.0);
    }
}
