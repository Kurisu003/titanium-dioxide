//! Static collision world: a triangle soup with a bounding volume hierarchy (game units, Z up).
//! The hierarchy is built on the first query, after all triangles have been added.

use glam::Vec3;
use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub struct Tri {
    pub a: Vec3,
    pub b: Vec3,
    pub c: Vec3,
    pub n: Vec3,
}

#[derive(Default)]
pub struct CollisionWorld {
    pub tris: Vec<Tri>,
    bvh: OnceLock<Bvh>,
    /// Invisible clip surfaces from the map's own collision (`CLIP_PILOT`, `CLIP_TITAN`): they
    /// stop movement (`push_sphere_moving`, `raycast_moving`) but not shots or other traces.
    pub clips: [Option<Box<CollisionWorld>>; 2],
    /// Surfaces only shots and other non-movement traces hit (`raycast`, `push_sphere`), not
    /// movement (`trace_box`, `raycast_moving`, `push_sphere_moving`): static props' render
    /// triangles, which the game traces only at TRACEDETAILLEVEL_HIGH (bullets); movement hits
    /// their collision hulls, which go in the clip sets.
    pub detail: Option<Box<CollisionWorld>>,
}

/// Clip surfaces that stop the Pilot (player clips) and BT (titan clips).
pub const CLIP_PILOT: usize = 0;
pub const CLIP_TITAN: usize = 1;

#[derive(Debug, Clone, Copy)]
pub struct Hit {
    pub t: f32,
    pub point: Vec3,
    pub normal: Vec3,
}

/// Flattened BVH node: a box, and either two children (`count == 0`, left child at
/// `start`, right child at `start + 1`) or a leaf range of `order`.
#[derive(Clone, Copy)]
struct Node {
    min: Vec3,
    max: Vec3,
    start: u32,
    count: u32,
}

#[derive(Default)]
struct Bvh {
    nodes: Vec<Node>,
    /// Triangle indices, grouped by leaf.
    order: Vec<u32>,
}

const LEAF_SIZE: usize = 4;

impl Bvh {
    fn build(tris: &[Tri]) -> Self {
        let mut order: Vec<u32> = (0..tris.len() as u32).collect();
        // Per-triangle bounds and centroids, computed once (cache friendly during the build).
        let boxes: Vec<(Vec3, Vec3)> = tris.iter().map(|t| (t.a.min(t.b).min(t.c), t.a.max(t.b).max(t.c))).collect();
        let centroids: Vec<Vec3> = boxes.iter().map(|(lo, hi)| (*lo + *hi) * 0.5).collect();
        if tris.is_empty() {
            return Self { nodes: Vec::new(), order };
        }
        let ctx = BuildCtx { boxes: &boxes, centroids: &centroids };
        // Split the top of the tree on this thread until there are enough subtrees to keep
        // every core busy, then finish the subtrees in parallel and stitch them in.
        let (lo, hi) = ctx.bounds(&order);
        let mut nodes = vec![Node { min: lo, max: hi, start: 0, count: tris.len() as u32 }];
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let mut pending: Vec<usize> = vec![0];
        while pending.len() < threads * 4 {
            let mut next = Vec::new();
            let mut split_any = false;
            for &ni in &pending {
                match ctx.split(&mut nodes, &mut order, ni) {
                    Some(left) => {
                        next.push(left);
                        next.push(left + 1);
                        split_any = true;
                    }
                    None => next.push(ni),
                }
            }
            pending = next;
            if !split_any {
                break;
            }
        }
        // Each pending node owns a disjoint range of `order`.
        pending.sort_by_key(|&ni| nodes[ni].start);
        let mut jobs: Vec<(usize, &mut [u32], u32)> = Vec::new();
        let mut rest: &mut [u32] = &mut order;
        let mut consumed = 0u32;
        for &ni in &pending {
            let (start, count) = (nodes[ni].start, nodes[ni].count);
            let tail = std::mem::take(&mut rest);
            let (_, tail) = tail.split_at_mut((start - consumed) as usize);
            let (mine, tail) = tail.split_at_mut(count as usize);
            rest = tail;
            consumed = start + count;
            jobs.push((ni, mine, start));
        }
        let roots: Vec<(usize, Node)> = pending.iter().map(|&ni| (ni, nodes[ni])).collect();
        let subtrees: Vec<Vec<Node>> = std::thread::scope(|scope| {
            let handles: Vec<_> = jobs
                .into_iter()
                .zip(roots)
                .map(|((_, slice, base), (_, root))| {
                    let ctx = &ctx;
                    scope.spawn(move || ctx.build_subtree(slice, base, root))
                })
                .collect();
            handles.into_iter().map(|h| h.join().expect("bvh worker")).collect()
        });
        for (ni, sub) in pending.into_iter().zip(subtrees) {
            // Local index 0 is the subtree root (it replaces the pending node); the rest are
            // appended, with child links remapped.
            let offset = nodes.len() as u32;
            let remap = |n: Node| if n.count == 0 { Node { start: offset + n.start - 1, ..n } } else { n };
            nodes[ni] = remap(sub[0]);
            nodes.extend(sub[1..].iter().map(|&n| remap(n)));
        }
        Self { nodes, order }
    }
}

struct BuildCtx<'a> {
    boxes: &'a [(Vec3, Vec3)],
    centroids: &'a [Vec3],
}

impl BuildCtx<'_> {
    fn bounds(&self, ids: &[u32]) -> (Vec3, Vec3) {
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for &i in ids {
            let (a, b) = self.boxes[i as usize];
            lo = lo.min(a);
            hi = hi.max(b);
        }
        (lo, hi)
    }

    /// Split node `ni` in place at the centroid median of its longest axis. Returns the index
    /// of the new left child (the right child follows it), or None for a leaf.
    fn split(&self, nodes: &mut Vec<Node>, order: &mut [u32], ni: usize) -> Option<usize> {
        let Node { start, count, min, max } = nodes[ni];
        if (count as usize) <= LEAF_SIZE {
            return None;
        }
        let ext = max - min;
        let axis = if ext.x >= ext.y && ext.x >= ext.z { 0 } else if ext.y >= ext.z { 1 } else { 2 };
        let (s, c) = (start as usize, count as usize);
        let mid = c / 2;
        order[s..s + c].select_nth_unstable_by(mid, |&a, &b| self.centroids[a as usize][axis].total_cmp(&self.centroids[b as usize][axis]));
        let left = nodes.len();
        let (llo, lhi) = self.bounds(&order[s..s + mid]);
        let (rlo, rhi) = self.bounds(&order[s + mid..s + c]);
        nodes.push(Node { min: llo, max: lhi, start, count: mid as u32 });
        nodes.push(Node { min: rlo, max: rhi, start: start + mid as u32, count: (c - mid) as u32 });
        nodes[ni] = Node { min, max, start: left as u32, count: 0 };
        Some(left)
    }

    /// Build the subtree under `root` (whose triangles are `slice`, starting at `base` in the
    /// global order). Node 0 of the result is the root; leaf starts are global.
    fn build_subtree(&self, slice: &mut [u32], base: u32, root: Node) -> Vec<Node> {
        let mut nodes = vec![Node { start: 0, ..root }];
        let mut stack = vec![0usize];
        while let Some(ni) = stack.pop() {
            if let Some(left) = self.split(&mut nodes, slice, ni) {
                stack.push(left);
                stack.push(left + 1);
            }
        }
        // Leaves were built relative to the slice; make them global.
        for n in nodes.iter_mut() {
            if n.count > 0 {
                n.start += base;
            }
        }
        nodes
    }
}

/// Ray vs box slab test; returns the entry distance if the box is hit before `max_t`.
fn ray_box(o: Vec3, inv: Vec3, min: Vec3, max: Vec3, max_t: f32) -> Option<f32> {
    let t1 = (min - o) * inv;
    let t2 = (max - o) * inv;
    let tmin = t1.min(t2).max_element().max(0.0);
    let tmax = t1.max(t2).min_element().min(max_t);
    (tmin <= tmax).then_some(tmin)
}

impl CollisionWorld {
    pub fn add(&mut self, a: Vec3, b: Vec3, c: Vec3) {
        let n = (b - a).cross(c - a);
        let len = n.length();
        if !(len > 1e-6) {
            return; // degenerate
        }
        self.tris.push(Tri { a, b, c, n: n / len });
        // Adding after a query invalidates the hierarchy.
        self.bvh = OnceLock::new();
    }

    fn bvh(&self) -> &Bvh {
        self.bvh.get_or_init(|| Bvh::build(&self.tris))
    }

    /// Write the triangles, clip sets and detail set to `path` (testing: `TF_COLLISION_DUMP`),
    /// for offline movement tests with [`CollisionWorld::load`].
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut out = b"TFCW2".to_vec();
        for set in [Some(self), self.clips[0].as_deref(), self.clips[1].as_deref(), self.detail.as_deref()] {
            let tris = set.map_or(&[][..], |w| &w.tris[..]);
            out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
            for t in tris {
                for v in [t.a, t.b, t.c] {
                    for x in v.to_array() {
                        out.extend_from_slice(&x.to_le_bytes());
                    }
                }
            }
        }
        std::fs::write(path, out)
    }

    /// Read a world written by [`CollisionWorld::save`] (or an older TFCW1 dump, which has no
    /// detail set).
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let data = std::fs::read(path)?;
        let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, "not a TFCW1/TFCW2 collision dump");
        let sets = match data.get(..5) {
            Some(b"TFCW1") => 3,
            Some(b"TFCW2") => 4,
            _ => return Err(bad()),
        };
        let mut o = 5;
        let mut f = || -> std::io::Result<f32> {
            let b = data.get(o..o + 4).ok_or_else(bad)?;
            o += 4;
            Ok(f32::from_le_bytes(b.try_into().unwrap()))
        };
        let mut w = CollisionWorld::default();
        for set in 0..sets {
            let n = f()?.to_bits() as usize;
            for _ in 0..n {
                let mut v = [Vec3::ZERO; 3];
                for p in v.iter_mut() {
                    *p = Vec3::new(f()?, f()?, f()?);
                }
                match set {
                    0 => w.add(v[0], v[1], v[2]),
                    3 => w.add_detail(v[0], v[1], v[2]),
                    s => w.add_clip(s - 1, v[0], v[1], v[2]),
                }
            }
        }
        Ok(w)
    }

    /// Build the acceleration structure now instead of on the first query.
    pub fn finish(&self) {
        self.bvh();
        for c in self.clips.iter().flatten().chain(self.detail.iter()) {
            c.bvh();
        }
    }

    /// Add a triangle that only shots and other non-movement traces hit (`detail`).
    pub fn add_detail(&mut self, a: Vec3, b: Vec3, c: Vec3) {
        self.detail.get_or_insert_with(Default::default).add(a, b, c);
    }

    /// Add a triangle to clip set `which` (`CLIP_PILOT` or `CLIP_TITAN`).
    pub fn add_clip(&mut self, which: usize, a: Vec3, b: Vec3, c: Vec3) {
        self.clips[which].get_or_insert_with(Default::default).add(a, b, c);
    }

    /// This world and clip set `which`, for movement queries.
    fn with_clip(&self, which: usize) -> [Option<&CollisionWorld>; 2] {
        [Some(self), self.clips.get(which).and_then(|c| c.as_deref())]
    }

    /// A movement ray (ground checks, wall probes): this world's solid triangles and clip set
    /// `which`, not the `detail` set.
    pub fn raycast_moving(&self, origin: Vec3, dir: Vec3, max_t: f32, which: usize) -> Option<Hit> {
        let [_, clip] = self.with_clip(which);
        nearer(self.raycast_tris(origin, dir, max_t), clip.and_then(|c| c.raycast_tris(origin, dir, max_t)))
    }

    /// `push_sphere` that also pushes out of clip set `which`.
    pub fn push_sphere_moving(&self, center: Vec3, radius: f32, max_normal_z: f32, which: usize) -> (Vec3, Vec3) {
        push_sphere_in(&self.with_clip(which), center, radius, max_normal_z)
    }

    /// Visit every triangle whose bounds overlap the box.
    fn cells_in(&self, min: Vec3, max: Vec3, mut f: impl FnMut(&Tri)) {
        let bvh = self.bvh();
        if bvh.nodes.is_empty() {
            return;
        }
        let mut stack = [0u32; 64];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let n = bvh.nodes[stack[sp] as usize];
            if n.max.x < min.x || n.max.y < min.y || n.max.z < min.z || n.min.x > max.x || n.min.y > max.y || n.min.z > max.z {
                continue;
            }
            if n.count > 0 {
                for &i in &bvh.order[n.start as usize..(n.start + n.count) as usize] {
                    f(&self.tris[i as usize]);
                }
            } else if sp + 2 <= stack.len() {
                stack[sp] = n.start;
                stack[sp + 1] = n.start + 1;
                sp += 2;
            }
        }
    }

    /// Ray cast against both triangle sides, `detail` included (shots, sight lines; clips are
    /// not hit). `dir` need not be normalized; `t` is in units of `dir`.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<Hit> {
        nearer(self.raycast_tris(origin, dir, max_t), self.detail.as_ref().and_then(|d| d.raycast_tris(origin, dir, max_t)))
    }

    /// `raycast` against this world's own triangles only.
    fn raycast_tris(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<Hit> {
        let bvh = self.bvh();
        if bvh.nodes.is_empty() {
            return None;
        }
        let inv = Vec3::ONE / dir;
        let mut best: Option<Hit> = None;
        let mut limit = max_t;
        // Each entry keeps its box's entry distance from when it was pushed: whether the ray
        // reaches a box depends on the limit only through `entry <= limit` (`ray_box`), so the
        // box needn't be tested again when it is popped.
        let Some(t_root) = ray_box(origin, inv, bvh.nodes[0].min, bvh.nodes[0].max, limit) else { return None };
        let mut stack = [(0u32, 0f32); 64];
        stack[0] = (0, t_root);
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let (ni, entry) = stack[sp];
            if entry > limit {
                continue;
            }
            let n = bvh.nodes[ni as usize];
            if n.count > 0 {
                for &i in &bvh.order[n.start as usize..(n.start + n.count) as usize] {
                    let tri = &self.tris[i as usize];
                    if let Some(t) = ray_tri(origin, dir, tri) {
                        if t <= limit {
                            limit = t;
                            let nrm = if tri.n.dot(dir) > 0.0 { -tri.n } else { tri.n };
                            best = Some(Hit { t, point: origin + dir * t, normal: nrm });
                        }
                    }
                }
            } else if sp + 2 <= stack.len() {
                // Visit the nearer child first so `limit` shrinks sooner.
                let (a, b) = (bvh.nodes[n.start as usize], bvh.nodes[n.start as usize + 1]);
                let ta = ray_box(origin, inv, a.min, a.max, limit);
                let tb = ray_box(origin, inv, b.min, b.max, limit);
                let (first, second) = if ta.unwrap_or(f32::MAX) <= tb.unwrap_or(f32::MAX) { ((n.start, ta), (n.start + 1, tb)) } else { ((n.start + 1, tb), (n.start, ta)) };
                // A child the ray misses now misses at any later (smaller) limit too: don't
                // push it (the visiting order of the rest is unchanged).
                for (i, t) in [second, first] {
                    if let Some(t) = t {
                        stack[sp] = (i, t);
                        sp += 1;
                    }
                }
            }
        }
        best
    }

    /// Push a sphere out of the geometry, ignoring triangles whose (upward-facing) normal
    /// has z above `max_normal_z` (floors are handled by ground snapping).
    /// Returns the total correction and the summed contact normals.
    pub fn push_sphere(&self, center: Vec3, radius: f32, max_normal_z: f32) -> (Vec3, Vec3) {
        push_sphere_in(&[Some(self), self.detail.as_deref()], center, radius, max_normal_z)
    }
}

/// The nearer of two ray hits.
fn nearer(a: Option<Hit>, b: Option<Hit>) -> Option<Hit> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.t < a.t { b } else { a }),
        (a, b) => a.or(b),
    }
}

/// Margin (game units) on the plane-distance test that skips triangles before the exact
/// distance check: far above the rounding in `closest_point_on_tri` at map coordinates, so
/// every triangle the exact test would accept still reaches it.
const PLANE_SLACK: f32 = 0.25;

/// `CollisionWorld::push_sphere` against several worlds at once.
fn push_sphere_in(worlds: &[Option<&CollisionWorld>], center: Vec3, radius: f32, max_normal_z: f32) -> (Vec3, Vec3) {
    let mut c = center;
    let mut normals = Vec3::ZERO;
    for _ in 0..4 {
        let mut push = Vec3::ZERO;
        let r = Vec3::splat(radius);
        let mut visit = |tri: &Tri| {
            // Farther from the triangle's plane than the radius (with room for rounding in
            // the closest point): no contact, skip the closest-point search.
            if (c - tri.a).dot(tri.n).abs() > radius + PLANE_SLACK {
                return;
            }
            let p = closest_point_on_tri(c, tri);
            let d = c - p;
            let dist = d.length();
            if dist >= radius || dist < 1e-4 {
                return;
            }
            let n = d / dist;
            if n.z > max_normal_z {
                return;
            }
            // Only add the part of this contact's depth not already resolved by
            // pushes from other triangles.
            let needed = (radius - dist) - push.dot(n);
            if needed > 0.0 {
                push += n * needed;
            }
            normals += n;
        };
        for w in worlds.iter().flatten() {
            w.cells_in(c - r, c + r, &mut visit);
        }
        if push.length_squared() < 1e-4 {
            break;
        }
        c += push;
    }
    (c - center, normals)
}

impl CollisionWorld {
    /// Nearest steep surface (|normal.z| below `max_normal_z`) within `radius` of `center`.
    /// Returns the outward normal (horizontal) and the distance.
    pub fn nearest_wall(&self, center: Vec3, radius: f32, max_normal_z: f32) -> Option<(Vec3, f32)> {
        let mut best: Option<(Vec3, f32)> = None;
        let r = Vec3::splat(radius);
        self.cells_in(center - r, center + r, |tri| {
            if tri.n.z.abs() > max_normal_z || (center - tri.a).dot(tri.n).abs() > radius + PLANE_SLACK {
                return;
            }
            let p = closest_point_on_tri(center, tri);
            let d = center - p;
            let dist = d.length();
            if dist < radius && dist > 1e-4 && best.is_none_or(|b| dist < b.1) {
                let n = Vec3::new(d.x, d.y, 0.0).normalize_or_zero();
                if n != Vec3::ZERO {
                    best = Some((n, dist));
                }
            }
        });
        best
    }
}

/// A box swept through the world: the parts of the engine's trace_t that player movement reads.
#[derive(Debug, Clone, Copy)]
pub struct Trace {
    /// How far toward `end` the box got (1: nothing in the way).
    pub fraction: f32,
    /// Where the box's origin stopped.
    pub end: Vec3,
    /// The blocking surface's normal, facing the box (zero when nothing was hit).
    pub normal: Vec3,
    /// The box started inside something and the move would go deeper (fraction 0).
    pub start_solid: bool,
}

impl Trace {
    pub fn hit(&self) -> bool {
        self.fraction < 1.0
    }
}

/// The engine's DIST_EPSILON: box traces stop this far (along the surface normal) short of
/// what they hit, so a box resting on a floor or against a wall doesn't touch it.
pub const DIST_EPSILON: f32 = 0.03125;

impl CollisionWorld {
    /// Sweep the axis-aligned box `mins..maxs` (relative to the origin) from `start` to `end`
    /// against the world and clip set `which`, as the engine's hull traces do for player
    /// movement (client.dll 0x1801fbdc0). Triangles count from both sides; a box that starts
    /// overlapping one is only stopped when the move goes deeper into it.
    pub fn trace_box(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3, which: usize) -> Trace {
        let half = (maxs - mins) * 0.5;
        let c0 = start + (mins + maxs) * 0.5;
        let d = end - start;
        let pad = Vec3::splat(1.0);
        let (lo, hi) = (c0.min(c0 + d) - half - pad, c0.max(c0 + d) + half + pad);
        let mut toi = f32::INFINITY;
        let mut normal = Vec3::ZERO;
        let mut face = false;
        // Contacts this close (in units travelled) count as simultaneous; then a face normal
        // beats an edge-edge one. A box reaching the corner of a step, or the edge where two
        // triangles meet, touches the edge and the face together; the edge's diagonal normal
        // would turn the slide (and StepMove) sideways where the engine's brush planes don't.
        let tie = 0.01 / d.length().max(1e-6);
        let mut solid: Option<Vec3> = None;
        for w in self.with_clip(which).iter().flatten() {
            w.cells_in(lo, hi, |tri| match sweep_box_tri_in(c0, half, d, tri, lo, hi) {
                Sweep::Miss => {}
                Sweep::Inside(mtv) => {
                    if d.dot(mtv) < 0.0 || d == Vec3::ZERO {
                        solid = Some(mtv);
                    }
                }
                Sweep::Hit(t, n, f) => {
                    if t < toi - tie || (t < toi + tie && f && !face) {
                        toi = t.min(toi);
                        normal = n;
                        face = f;
                    }
                }
            });
        }
        if let Some(n) = solid {
            return Trace { fraction: 0.0, end: start, normal: n, start_solid: true };
        }
        if !toi.is_finite() {
            return Trace { fraction: 1.0, end, normal: Vec3::ZERO, start_solid: false };
        }
        // Stop DIST_EPSILON short along the normal.
        let closing = -d.dot(normal);
        let fraction = if closing > 1e-6 { (toi - DIST_EPSILON / closing).clamp(0.0, 1.0) } else { 0.0 };
        Trace { fraction, end: start + d * fraction, normal, start_solid: false }
    }
}

enum Sweep {
    Miss,
    /// Time of impact (0..1 of the move), the surface normal facing the box, and whether that
    /// normal is a face's (the box's or the triangle's) rather than an edge-edge axis.
    Hit(f32, Vec3, bool),
    /// Overlapping at the start; the shortest way out.
    Inside(Vec3),
}

/// `sweep_box_tri` for a triangle that may lie outside `lo..hi`, the swept box's bounds
/// padded by a unit: one entirely outside them (the leaf's box overlapped, the triangle doesn't)
/// misses on that world axis, so the full test is skipped.
fn sweep_box_tri_in(c: Vec3, e: Vec3, d: Vec3, t: &Tri, lo: Vec3, hi: Vec3) -> Sweep {
    if t.a.max(t.b).max(t.c).cmplt(lo).any() || t.a.min(t.b).min(t.c).cmpgt(hi).any() {
        return Sweep::Miss;
    }
    sweep_box_tri(c, e, d, t)
}

/// Separating-axis sweep of a box (centre `c`, half extents `e`, moving by `d`) against a
/// triangle: the box's three axes, the triangle's normal and the nine edge cross products.
fn sweep_box_tri(c: Vec3, e: Vec3, d: Vec3, t: &Tri) -> Sweep {
    let v = [t.a - c, t.b - c, t.c - c];
    let edges = [v[1] - v[0], v[2] - v[1], v[0] - v[2]];
    let (mut t_enter, mut t_exit) = (f32::NEG_INFINITY, f32::INFINITY);
    let mut n_enter = Vec3::ZERO;
    let mut face = true;
    // An edge-edge axis only takes over the entry from a face axis when it enters this much
    // (in units travelled) later.
    let tie = 0.01 / d.length().max(1e-6);
    let mut inside = true;
    let (mut depth, mut mtv) = (f32::INFINITY, Vec3::ZERO);
    let mut test = |axis: Vec3, is_face: bool| -> bool {
        let len2 = axis.length_squared();
        if len2 < 1e-10 {
            return true;
        }
        let l = axis / len2.sqrt();
        let (p0, p1, p2) = (v[0].dot(l), v[1].dot(l), v[2].dot(l));
        let r = e.x * l.x.abs() + e.y * l.y.abs() + e.z * l.z.abs();
        // The box's centre at t is s = t * dl along l; it overlaps while lo < s < hi.
        let (lo, hi) = (p0.min(p1).min(p2) - r, p0.max(p1).max(p2) + r);
        let dl = d.dot(l);
        if dl.abs() < 1e-9 {
            if lo >= 0.0 || hi <= 0.0 {
                return false;
            }
        } else {
            let (t0, t1, n) = if dl > 0.0 { (lo / dl, hi / dl, -l) } else { (hi / dl, lo / dl, l) };
            if t0 > t_enter + if is_face || !face { 0.0 } else { tie } || (is_face && !face && t0 > t_enter - tie) {
                t_enter = t0.max(t_enter);
                n_enter = n;
                face = is_face;
            }
            t_exit = t_exit.min(t1);
            if t_enter >= t_exit {
                return false;
            }
        }
        if lo < 0.0 && hi > 0.0 {
            if -lo < depth {
                depth = -lo;
                mtv = -l;
            }
            if hi < depth {
                depth = hi;
                mtv = l;
            }
        } else {
            inside = false;
        }
        true
    };
    let n = (v[1] - v[0]).cross(v[2] - v[0]);
    if !(test(Vec3::X, true) && test(Vec3::Y, true) && test(Vec3::Z, true) && test(n, true)) {
        return Sweep::Miss;
    }
    for ed in edges {
        for ax in [Vec3::X, Vec3::Y, Vec3::Z] {
            if !test(ax.cross(ed), false) {
                return Sweep::Miss;
            }
        }
    }
    if inside {
        return Sweep::Inside(mtv);
    }
    if t_exit <= 0.0 || t_enter > 1.0 {
        return Sweep::Miss;
    }
    Sweep::Hit(t_enter.max(0.0), n_enter, face)
}

fn ray_tri(o: Vec3, d: Vec3, t: &Tri) -> Option<f32> {
    let e1 = t.b - t.a;
    let e2 = t.c - t.a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t.a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let tt = e2.dot(q) * inv;
    (tt >= 0.0).then_some(tt)
}

/// Closest point on a triangle (Ericson, Real-Time Collision Detection 5.1.5).
pub fn closest_point_on_tri(p: Vec3, t: &Tri) -> Vec3 {
    let (a, b, c) = (t.a, t.b, t.c);
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_stop_movement_queries_only() {
        let mut w = CollisionWorld::default();
        // A wall in the pilot clips at x = 10.
        w.add_clip(CLIP_PILOT, Vec3::new(10.0, -50.0, -50.0), Vec3::new(10.0, 50.0, -50.0), Vec3::new(10.0, 0.0, 50.0));
        w.finish();
        assert!(w.raycast(Vec3::ZERO, Vec3::X, 100.0).is_none(), "shots pass clips");
        assert!(w.raycast_moving(Vec3::ZERO, Vec3::X, 100.0, CLIP_TITAN).is_none(), "titan ignores player clips");
        let hit = w.raycast_moving(Vec3::ZERO, Vec3::X, 100.0, CLIP_PILOT).expect("pilot hits the clip");
        assert!((hit.t - 10.0).abs() < 1e-3);
        let (push, _) = w.push_sphere_moving(Vec3::new(5.0, 0.0, 0.0), 8.0, 0.7, CLIP_PILOT);
        assert!((push.x + 3.0).abs() < 1e-3, "pushed out of the clip: {push}");
        assert_eq!(w.push_sphere(Vec3::new(5.0, 0.0, 0.0), 8.0, 0.7).0, Vec3::ZERO);
    }

    #[test]
    fn detail_stops_shots_only() {
        let mut w = CollisionWorld::default();
        // A prop's render triangle at x = 10 (detail) and its hull face at x = 12 (pilot clip).
        w.add_detail(Vec3::new(10.0, -50.0, -50.0), Vec3::new(10.0, 50.0, -50.0), Vec3::new(10.0, 0.0, 50.0));
        w.add_clip(CLIP_PILOT, Vec3::new(12.0, -50.0, -50.0), Vec3::new(12.0, 50.0, -50.0), Vec3::new(12.0, 0.0, 50.0));
        w.finish();
        let shot = w.raycast(Vec3::ZERO, Vec3::X, 100.0).expect("shots hit the render triangles");
        assert!((shot.t - 10.0).abs() < 1e-3);
        let walk = w.raycast_moving(Vec3::ZERO, Vec3::X, 100.0, CLIP_PILOT).expect("movement hits the hull");
        assert!((walk.t - 12.0).abs() < 1e-3);
        assert!(w.raycast_moving(Vec3::ZERO, Vec3::X, 100.0, CLIP_TITAN).is_none());
        let tr = w.trace_box(Vec3::ZERO, Vec3::new(40.0, 0.0, 0.0), Vec3::splat(-1.0), Vec3::splat(1.0), CLIP_PILOT);
        assert!((tr.end.x - (11.0 - DIST_EPSILON)).abs() < 1e-3, "box stops at the hull: {}", tr.end);
        assert!(w.push_sphere(Vec3::new(5.0, 0.0, 0.0), 8.0, 0.7).0.x < -2.9, "non-moving pushes see detail");
        assert_eq!(w.push_sphere_moving(Vec3::new(5.0, 0.0, 0.0), 6.0, 0.7, CLIP_PILOT).0, Vec3::ZERO);
        // The dump keeps the sets apart.
        let path = std::env::temp_dir().join(format!("tfcw_detail_{}.tfcw", std::process::id()));
        w.save(&path).unwrap();
        let back = CollisionWorld::load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(back.tris.is_empty());
        assert_eq!(back.detail.as_ref().map(|d| d.tris.len()), Some(1));
        assert_eq!(back.clips[CLIP_PILOT].as_ref().map(|d| d.tris.len()), Some(1));
    }
}
