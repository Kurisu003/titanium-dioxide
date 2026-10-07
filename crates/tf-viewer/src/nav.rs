//! Pathfinding on the game's navmeshes (`tf_assets::navmesh`): the Titan-sized `_large` mesh
//! for enemy Titans, `_small` for grunts. A* over the polygons, waypoints at the shared edges' midpoints; the
//! followers straighten the path with line-of-sight checks as they go. Maps without a client
//! navmesh (every MP map) get a grid probed from the collision mesh instead (`GridNav`).
use bevy::prelude::*;
use std::collections::{BinaryHeap, HashMap};
use tf_assets::navmesh::NavMesh;
use tf_sim::glam::Vec3 as SVec3;

/// Whichever navigation data a map has.
pub enum Nav {
    Mesh(MeshNav),
    Grid(GridNav),
}

impl Nav {
    /// Load the game's navmesh, or None when the map ships none (then `Nav::probe`).
    pub fn load(gd: &crate::gamedata::GameData, map: &str, size: &str) -> Option<Self> {
        MeshNav::load(gd, map, size).map(Nav::Mesh)
    }

    /// Build a grid from the collision mesh around `center`.
    pub fn probe(world: &tf_sim::collision::CollisionWorld, center: Vec3, radius: f32, agent: Agent) -> Self {
        Nav::Grid(GridNav::build(world, center, radius, agent))
    }

    pub fn path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        match self {
            Nav::Mesh(m) => m.path(from, to),
            Nav::Grid(g) => g.path(from, to),
        }
    }
}

/// XY grid cell size for the poly lookup (game units).
const CELL: f32 = 512.0;
/// A point counts as on a poly when within this height of its plane (game units).
const Z_SLACK: f32 = 260.0;
/// Fallback: nearest poly centre within this 2D distance.
const NEAR_FALLBACK: f32 = 400.0;
const MAX_EXPANSIONS: usize = 6000;

pub struct MeshNav {
    pub mesh: NavMesh,
    grid: HashMap<(i32, i32), Vec<u32>>,
    bounds: Vec<([f32; 2], [f32; 2])>,
}

#[derive(Copy, Clone, PartialEq)]
struct Open {
    f: f32,
    poly: u32,
}
impl Eq for Open {}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.f.total_cmp(&self.f)
    }
}

fn cell(x: f32, y: f32) -> (i32, i32) {
    ((x / CELL).floor() as i32, (y / CELL).floor() as i32)
}

impl MeshNav {
    pub fn new(mesh: NavMesh) -> Self {
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        let mut bounds = Vec::with_capacity(mesh.polys.len());
        for (i, p) in mesh.polys.iter().enumerate() {
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for v in &p.verts {
                lo = [lo[0].min(v[0]), lo[1].min(v[1])];
                hi = [hi[0].max(v[0]), hi[1].max(v[1])];
            }
            bounds.push((lo, hi));
            if p.verts.is_empty() {
                continue;
            }
            let (a, b) = (cell(lo[0], lo[1]), cell(hi[0], hi[1]));
            for cx in a.0..=b.0 {
                for cy in a.1..=b.1 {
                    grid.entry((cx, cy)).or_default().push(i as u32);
                }
            }
        }
        Self { mesh, grid, bounds }
    }

    /// Load `maps/navmesh/<map>_<size>.nm` (`large` for Titans, `small` for infantry).
    pub fn load(gd: &crate::gamedata::GameData, map: &str, size: &str) -> Option<Self> {
        let path = format!("maps/navmesh/{map}_{size}.nm");
        let data = match gd.read_file(&path) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("navmesh: {e:#}");
                return None;
            }
        };
        match tf_assets::navmesh::parse(&data) {
            Ok(mesh) => {
                let links: usize = mesh.polys.iter().map(|p| p.links.len()).sum();
                log::info!("navmesh {path}: {} tiles, {} polys, {links} links, {} off-mesh, radius {} height {}", mesh.tiles, mesh.polys.len(), mesh.off_mesh, mesh.walkable_radius, mesh.walkable_height);
                Some(Self::new(mesh))
            }
            Err(e) => {
                log::warn!("navmesh {path}: {e:#}");
                None
            }
        }
    }

    fn contains_2d(&self, poly: u32, x: f32, y: f32) -> bool {
        let p = &self.mesh.polys[poly as usize];
        let n = p.verts.len();
        if n < 3 {
            return false;
        }
        // Convex polygon: the point is on the same side of every edge.
        let mut sign = 0.0f32;
        for i in 0..n {
            let (a, b) = (p.verts[i], p.verts[(i + 1) % n]);
            let c = (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
            if c.abs() < 1e-3 {
                continue;
            }
            if sign == 0.0 {
                sign = c.signum();
            } else if c.signum() != sign {
                return false;
            }
        }
        true
    }

    /// Height of the poly's plane at (x, y): the vertex-weighted average is close enough for
    /// walkable polys, which are nearly flat.
    fn poly_z(&self, poly: u32) -> f32 {
        self.mesh.polys[poly as usize].center[2]
    }

    /// The poly under (or nearest to) a point.
    pub fn locate(&self, p: Vec3) -> Option<u32> {
        let mut best: Option<(f32, u32)> = None;
        let c = cell(p.x, p.y);
        for cx in c.0 - 1..=c.0 + 1 {
            for cy in c.1 - 1..=c.1 + 1 {
                let Some(list) = self.grid.get(&(cx, cy)) else { continue };
                for &i in list {
                    let (lo, hi) = self.bounds[i as usize];
                    let inside = p.x >= lo[0] - 1.0 && p.x <= hi[0] + 1.0 && p.y >= lo[1] - 1.0 && p.y <= hi[1] + 1.0 && self.contains_2d(i, p.x, p.y);
                    let dz = (p.z - self.poly_z(i)).abs();
                    let score = if inside {
                        if dz > Z_SLACK {
                            continue;
                        }
                        dz
                    } else {
                        let c = self.mesh.polys[i as usize].center;
                        let d = Vec2::new(c[0] - p.x, c[1] - p.y).length();
                        if d > NEAR_FALLBACK || dz > Z_SLACK {
                            continue;
                        }
                        Z_SLACK + d + dz
                    };
                    if best.is_none_or(|b| score < b.0) {
                        best = Some((score, i));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Midpoint of the edge two polys share (verts in both), or the second's centre.
    fn portal(&self, a: u32, b: u32) -> Vec3 {
        let (pa, pb) = (&self.mesh.polys[a as usize], &self.mesh.polys[b as usize]);
        let shared: Vec<Vec3> = pa.verts.iter().filter(|v| pb.verts.iter().any(|w| (v[0] - w[0]).abs() < 1.0 && (v[1] - w[1]).abs() < 1.0 && (v[2] - w[2]).abs() < 40.0)).map(|v| Vec3::from(*v)).collect();
        match shared.len() {
            2 => (shared[0] + shared[1]) * 0.5,
            _ => Vec3::from(pb.center),
        }
    }

    /// Waypoints from `from` to `to` (excluding `from`, ending at `to`), or None when either
    /// end is off the mesh or no route exists.
    pub fn path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let (s, g) = (self.locate(from)?, self.locate(to)?);
        if s == g {
            return Some(vec![to]);
        }
        let goal = Vec2::new(to.x, to.y);
        let h = |i: u32| {
            let c = self.mesh.polys[i as usize].center;
            Vec2::new(c[0], c[1]).distance(goal)
        };
        let mut came: HashMap<u32, u32> = HashMap::new();
        let mut cost: HashMap<u32, f32> = HashMap::new();
        let mut open = BinaryHeap::new();
        cost.insert(s, 0.0);
        open.push(Open { f: h(s), poly: s });
        let mut expansions = 0;
        let mut found = false;
        while let Some(Open { poly, .. }) = open.pop() {
            if poly == g {
                found = true;
                break;
            }
            expansions += 1;
            if expansions > MAX_EXPANSIONS {
                break;
            }
            let here = cost[&poly];
            let pc = self.mesh.polys[poly as usize].center;
            for &n in &self.mesh.polys[poly as usize].links {
                let nc = self.mesh.polys[n as usize].center;
                let step = Vec3::from(pc).distance(Vec3::from(nc));
                let c = here + step;
                if cost.get(&n).is_none_or(|&old| c < old) {
                    cost.insert(n, c);
                    came.insert(n, poly);
                    open.push(Open { f: c + h(n), poly: n });
                }
            }
        }
        if !found {
            return None;
        }
        let mut chain = vec![g];
        let mut cur = g;
        while let Some(&p) = came.get(&cur) {
            chain.push(p);
            cur = p;
            if chain.len() > 4096 {
                break;
            }
        }
        chain.reverse();
        let mut way = Vec::with_capacity(chain.len());
        for w in chain.windows(2) {
            way.push(self.portal(w[0], w[1]));
        }
        way.push(to);
        Some(way)
    }
}

// ---------------------------------------------------------------------------------------------
// Probed grid for maps without a navmesh

/// Who the grid is for: cell size, how far they can step up or drop, their height and
/// half-width for the clearance probes (game units).
#[derive(Clone, Copy)]
pub struct Agent {
    pub cell: f32,
    pub step_up: f32,
    pub drop: f32,
    pub height: f32,
    pub radius: f32,
}

impl Agent {
    /// Enemy Titans: 80 is the Titan step height; they will drop a storey like the game's do.
    pub const TITAN: Agent = Agent { cell: 128.0, step_up: 80.0, drop: 420.0, height: 230.0, radius: 65.0 };
    /// Grunts: 18 units of step, 60 of hull height, 16 of half-width.
    pub const GRUNT: Agent = Agent { cell: 64.0, step_up: 50.0, drop: 200.0, height: 60.0, radius: 20.0 };
}

/// Walkable surfaces found by casting down through each cell: a cell can hold several (a
/// bridge over a road, a building's floors), each one a node.
pub struct GridNav {
    origin: Vec2,
    n: i32,
    agent: Agent,
    /// Per cell, the z of each walkable surface, highest first.
    layers: Vec<Vec<f32>>,
    /// Per node, whether it lies in a component of at least MIN_COMPONENT cells (a Titan
    /// standing on a prop's roof or in a gap the probes rejected is sent to the nearest real
    /// one instead).
    connected: HashMap<u32, bool>,
    /// Per node, a bit per neighbour direction (`DIRS` order) whose crossing is blocked by
    /// something between the two cell centres (a parapet, a railing, a wall).
    blocked: HashMap<u32, u8>,
}

/// Neighbour offsets, in bit order.
const DIRS: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

/// Islands smaller than this many cells are ignored by `locate`.
const MIN_COMPONENT: usize = 12;

/// Standing room below a roof before a lower surface counts, and the slope limit.
const MIN_HEADROOM: f32 = 1.0;
const MAX_SLOPE_NORMAL_Z: f32 = 0.7;
const GRID_MAX_EXPANSIONS: usize = 40_000;

impl GridNav {
    pub fn build(world: &tf_sim::collision::CollisionWorld, center: Vec3, radius: f32, agent: Agent) -> Self {
        let t = std::time::Instant::now();
        let n = ((radius * 2.0) / agent.cell).ceil() as i32;
        let origin = Vec2::new(center.x - radius, center.y - radius);
        let top = center.z + 1500.0;
        let mut layers = vec![Vec::new(); (n * n) as usize];
        let mut walkable = 0usize;
        let dirs: Vec<SVec3> = (0..8).map(|k| {
            let a = k as f32 / 8.0 * std::f32::consts::TAU;
            SVec3::new(a.cos(), a.sin(), 0.0)
        }).collect();
        for cy in 0..n {
            for cx in 0..n {
                let x = origin.x + (cx as f32 + 0.5) * agent.cell;
                let y = origin.y + (cy as f32 + 0.5) * agent.cell;
                let mut from = SVec3::new(x, y, top);
                let mut remaining = top - (center.z - 3000.0);
                let list = &mut layers[(cy * n + cx) as usize];
                while remaining > 0.0 {
                    let Some(h) = world.raycast(from, -SVec3::Z, remaining) else { break };
                    let z = h.point.z;
                    // Standing room above, and nothing within the hull's half-width at chest
                    // height or just above step height (railings, kerbs too tall to step).
                    let ok = h.normal.z >= MAX_SLOPE_NORMAL_Z
                        && world.raycast(SVec3::new(x, y, z + MIN_HEADROOM), SVec3::Z, agent.height).is_none()
                        && dirs.iter().all(|d| {
                            world.raycast(SVec3::new(x, y, z + agent.height * 0.5), *d, agent.radius).is_none()
                                && world.raycast(SVec3::new(x, y, z + agent.step_up + 10.0), *d, agent.radius).is_none()
                        });
                    if ok {
                        list.push(z);
                        walkable += 1;
                    }
                    remaining -= h.t + 2.0;
                    from = SVec3::new(x, y, z - 2.0);
                }
            }
        }
        log::info!("nav grid: {n}x{n} cells of {} around {:.0}, {walkable} walkable surfaces in {:?}", agent.cell, center, t.elapsed());
        let mut grid = Self { origin, n, agent, layers, connected: HashMap::new(), blocked: HashMap::new() };
        grid.probe_edges(world);
        grid.label_components();
        // TF_NAV_DUMP=<dir>: a PGM per grid (brightness = surfaces per cell, 0 = none).
        if let Ok(dir) = std::env::var("TF_NAV_DUMP") {
            let mut out = format!("P2\n{n} {n}\n255\n");
            for cy in (0..n).rev() {
                for cx in 0..n {
                    let k = grid.layers[(cy * n + cx) as usize].len();
                    out.push_str(&format!("{} ", (k * 85).min(255)));
                }
                out.push('\n');
            }
            let _ = std::fs::write(format!("{dir}/nav_{}.pgm", agent.cell as i32), out);
        }
        grid
    }

    /// Cast between neighbouring cell centres at step and chest height: the radial probes
    /// miss thin walls that sit between two centres.
    fn probe_edges(&mut self, world: &tf_sim::collision::CollisionWorld) {
        let mut blocked = HashMap::new();
        let mut edges = 0usize;
        for cy in 0..self.n {
            for cx in 0..self.n {
                for l in 0..self.layers[(cy * self.n + cx) as usize].len().min(16) {
                    let id = self.node(cx, cy, l);
                    let c = self.center(id);
                    let mut mask = 0u8;
                    for (k, &(dx, dy)) in DIRS.iter().enumerate() {
                        if self.step_to(cx + dx, cy + dy, c.z).is_none() {
                            continue;
                        }
                        let d = SVec3::new(dx as f32, dy as f32, 0.0);
                        let len = d.length() * self.agent.cell;
                        let d = d / d.length();
                        // Three rays abreast, as wide as the hull, at two heights.
                        let side = SVec3::new(-d.y, d.x, 0.0) * self.agent.radius * 0.8;
                        let hit = [self.agent.step_up + 10.0, self.agent.height * 0.5].iter().any(|&h| {
                            [-1.0, 0.0, 1.0].iter().any(|&k| world.raycast(SVec3::new(c.x, c.y, c.z + h) + side * k, d, len).is_some())
                        });
                        if hit {
                            mask |= 1 << k;
                            edges += 1;
                        }
                    }
                    if mask != 0 {
                        blocked.insert(id, mask);
                    }
                }
            }
        }
        log::debug!("nav grid: {edges} blocked edges");
        self.blocked = blocked;
    }

    /// The node reached from `id` by stepping to the neighbouring cell `(dx, dy)`.
    fn neighbour(&self, id: u32, dx: i32, dy: i32) -> Option<u32> {
        let k = DIRS.iter().position(|&d| d == (dx, dy))?;
        if self.blocked.get(&id).copied().unwrap_or(0) & (1 << k) != 0 {
            return None;
        }
        let (cx, cy, _) = self.unpack(id);
        self.step_to(cx + dx, cy + dy, self.center(id).z)
    }

    /// Flood-fill the nodes into components and mark the ones big enough to use.
    fn label_components(&mut self) {
        let mut seen: HashMap<u32, bool> = HashMap::new();
        let mut islands = 0;
        for cy in 0..self.n {
            for cx in 0..self.n {
                for l in 0..self.layers[(cy * self.n + cx) as usize].len().min(16) {
                    let start = self.node(cx, cy, l);
                    if seen.contains_key(&start) {
                        continue;
                    }
                    let mut members = vec![start];
                    seen.insert(start, false);
                    let mut i = 0;
                    while i < members.len() {
                        let id = members[i];
                        i += 1;
                        for &(dx, dy) in &DIRS {
                            if let Some(nb) = self.neighbour(id, dx, dy) {
                                if !seen.contains_key(&nb) {
                                    seen.insert(nb, false);
                                    members.push(nb);
                                }
                            }
                        }
                    }
                    let big = members.len() >= MIN_COMPONENT;
                    if !big {
                        islands += 1;
                    }
                    for m in members {
                        seen.insert(m, big);
                    }
                }
            }
        }
        log::debug!("nav grid: {islands} islands under {MIN_COMPONENT} cells ignored");
        self.connected = seen;
    }

    fn cell_of(&self, p: Vec2) -> Option<(i32, i32)> {
        let c = ((p - self.origin) / self.agent.cell).floor();
        let (cx, cy) = (c.x as i32, c.y as i32);
        (cx >= 0 && cy >= 0 && cx < self.n && cy < self.n).then_some((cx, cy))
    }

    fn node(&self, cx: i32, cy: i32, layer: usize) -> u32 {
        (((cy * self.n + cx) as u32) << 4) | layer as u32
    }

    fn unpack(&self, id: u32) -> (i32, i32, usize) {
        let cell = (id >> 4) as i32;
        (cell % self.n, cell / self.n, (id & 15) as usize)
    }

    fn center(&self, id: u32) -> Vec3 {
        let (cx, cy, l) = self.unpack(id);
        Vec3::new(self.origin.x + (cx as f32 + 0.5) * self.agent.cell, self.origin.y + (cy as f32 + 0.5) * self.agent.cell, self.layers[(cy * self.n + cx) as usize][l])
    }

    /// The node under `p`: the surface nearest its height in its cell, else in the ring of
    /// cells around it.
    fn locate(&self, p: Vec3) -> Option<u32> {
        let (cx, cy) = self.cell_of(Vec2::new(p.x, p.y))?;
        let mut best: Option<(f32, u32)> = None;
        for r in 0i32..=3 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let (x, y) = (cx + dx, cy + dy);
                    if x < 0 || y < 0 || x >= self.n || y >= self.n {
                        continue;
                    }
                    for (l, &z) in self.layers[(y * self.n + x) as usize].iter().enumerate().take(16) {
                        let dz = (z - p.z).abs();
                        if dz > self.agent.drop || !self.connected.get(&self.node(x, y, l)).copied().unwrap_or(false) {
                            continue;
                        }
                        let cost = dz + r as f32 * self.agent.cell;
                        if best.is_none_or(|b| cost < b.0) {
                            best = Some((cost, self.node(x, y, l)));
                        }
                    }
                }
            }
            if best.is_some() {
                return best.map(|b| b.1);
            }
        }
        None
    }

    /// A step from a surface at `z` to a neighbouring cell: the layer there within step-up /
    /// drop range, nearest in height.
    fn step_to(&self, cx: i32, cy: i32, z: f32) -> Option<u32> {
        if cx < 0 || cy < 0 || cx >= self.n || cy >= self.n {
            return None;
        }
        let list = &self.layers[(cy * self.n + cx) as usize];
        let mut best: Option<(f32, usize)> = None;
        for (l, &nz) in list.iter().enumerate().take(16) {
            let up = nz - z;
            if up > self.agent.step_up || -up > self.agent.drop {
                continue;
            }
            if best.is_none_or(|b| up.abs() < b.0) {
                best = Some((up.abs(), l));
            }
        }
        best.map(|b| self.node(cx, cy, b.1))
    }

    pub fn path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let (Some(s), Some(g)) = (self.locate(from), self.locate(to)) else {
            log::debug!("grid: no node under {:?} / {:?}", self.locate(from).is_none().then_some(from), self.locate(to).is_none().then_some(to));
            return None;
        };
        if s == g {
            return Some(vec![to]);
        }
        let goal = Vec2::new(to.x, to.y);
        let h = |id: u32| {
            let c = self.center(id);
            Vec2::new(c.x, c.y).distance(goal)
        };
        let mut came: HashMap<u32, u32> = HashMap::new();
        let mut cost: HashMap<u32, f32> = HashMap::new();
        let mut open = BinaryHeap::new();
        cost.insert(s, 0.0);
        open.push(Open { f: h(s), poly: s });
        let mut expansions = 0;
        let mut found = false;
        while let Some(Open { poly: id, .. }) = open.pop() {
            if id == g {
                found = true;
                break;
            }
            expansions += 1;
            if expansions > GRID_MAX_EXPANSIONS {
                break;
            }
            let here = cost[&id];
            let z = self.center(id).z;
            for &(dx, dy) in &DIRS {
                // No cutting corners: a diagonal needs both orthogonal neighbours open.
                if dx != 0 && dy != 0 && (self.neighbour(id, dx, 0).is_none() || self.neighbour(id, 0, dy).is_none()) {
                    continue;
                }
                let Some(nb) = self.neighbour(id, dx, dy) else { continue };
                let nz = self.center(nb).z;
                let step = self.agent.cell * if dx != 0 && dy != 0 { std::f32::consts::SQRT_2 } else { 1.0 } + (nz - z).abs() * 0.5;
                let c = here + step;
                if cost.get(&nb).is_none_or(|&old| c < old) {
                    cost.insert(nb, c);
                    came.insert(nb, id);
                    open.push(Open { f: c + h(nb), poly: nb });
                }
            }
        }
        if !found {
            log::debug!("grid: no route from {:.0} to {:.0} after {expansions} expansions ({} nodes reached)", from, to, cost.len());
            return None;
        }
        let mut chain = vec![g];
        let mut cur = g;
        while let Some(&p) = came.get(&cur) {
            chain.push(p);
            cur = p;
            if chain.len() > 8192 {
                break;
            }
        }
        chain.reverse();
        let mut way: Vec<Vec3> = chain.iter().skip(1).map(|&id| self.center(id)).collect();
        way.push(to);
        Some(way)
    }
}
