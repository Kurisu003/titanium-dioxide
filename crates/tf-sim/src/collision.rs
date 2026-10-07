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
}

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

    /// Build the acceleration structure now instead of on the first query.
    pub fn finish(&self) {
        self.bvh();
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

    /// Ray cast against both triangle sides. `dir` need not be normalized; `t` is in units of `dir`.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<Hit> {
        let bvh = self.bvh();
        if bvh.nodes.is_empty() {
            return None;
        }
        let inv = Vec3::ONE / dir;
        let mut best: Option<Hit> = None;
        let mut limit = max_t;
        let mut stack = [0u32; 64];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let n = bvh.nodes[stack[sp] as usize];
            if ray_box(origin, inv, n.min, n.max, limit).is_none() {
                continue;
            }
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
                let (first, second) = if ta.unwrap_or(f32::MAX) <= tb.unwrap_or(f32::MAX) { (n.start, n.start + 1) } else { (n.start + 1, n.start) };
                stack[sp] = second;
                stack[sp + 1] = first;
                sp += 2;
            }
        }
        best
    }

    /// Push a sphere out of the geometry, ignoring triangles whose (upward-facing) normal
    /// has z above `max_normal_z` (floors are handled by ground snapping).
    /// Returns the total correction and the summed contact normals.
    pub fn push_sphere(&self, center: Vec3, radius: f32, max_normal_z: f32) -> (Vec3, Vec3) {
        let mut c = center;
        let mut normals = Vec3::ZERO;
        for _ in 0..4 {
            let mut push = Vec3::ZERO;
            let r = Vec3::splat(radius);
            self.cells_in(c - r, c + r, |tri| {
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
            });
            if push.length_squared() < 1e-4 {
                break;
            }
            c += push;
        }
        (c - center, normals)
    }
}

impl CollisionWorld {
    /// Nearest steep surface (|normal.z| below `max_normal_z`) within `radius` of `center`.
    /// Returns the outward normal (horizontal) and the distance.
    pub fn nearest_wall(&self, center: Vec3, radius: f32, max_normal_z: f32) -> Option<(Vec3, f32)> {
        let mut best: Option<(Vec3, f32)> = None;
        let r = Vec3::splat(radius);
        self.cells_in(center - r, center + r, |tri| {
            if tri.n.z.abs() > max_normal_z {
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
