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
    /// Queries walk the median tree itself, without a search tree (see `Bvh`): set for
    /// `detail`, whose small, even prop triangles make tight median-tree leaves (on Angel City
    /// a search tree didn't speed its queries up but took ~0.3 s and ~130 MB more).
    median_only: bool,
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
/// `start`, right child at `start + 1`) or a leaf range (of `Bvh::order` in the median
/// tree, of `Bvh::leaves` in the search tree).
#[derive(Clone, Copy, Default)]
struct Node {
    min: Vec3,
    max: Vec3,
    start: u32,
    count: u32,
}

/// Two trees. The median tree (centroid median splits, up to `LEAF_SIZE` triangles per leaf)
/// defines what the queries see: a triangle is visited when its median-tree leaf's box passes
/// the query's box test, box queries visit the leaves in that tree's depth-first order (right
/// child first), which is descending `start`, and long rays walk it as they always did
/// (`raycast_median`). Its splits overlap a lot, so box queries and short rays don't walk it:
/// they walk a binned-SAH tree over its leaves (`nodes`), which finds the same leaves with
/// far fewer box tests, and short rays then replay the median-tree walk over the leaves they
/// hit (`raycast_replay`).
#[derive(Default)]
struct Bvh {
    /// The search tree (none for `CollisionWorld::median_only`); its leaves are ranges of
    /// `leaves`.
    nodes: Vec<Node>,
    /// The median tree's leaves (their boxes and ranges of `order`), in search tree order.
    leaves: Vec<Node>,
    /// The median tree (a node of `count` triangles splits them `count / 2` to the left).
    median: Vec<Node>,
    /// Triangle indices, grouped by median-tree leaf.
    order: Vec<u32>,
    /// The triangles in that order, with the search tree (not for `median_only`, whose sets are
    /// big): queries read a leaf's triangles side by side instead of through `order`.
    sorted: Vec<Tri>,
    /// The median-tree leaves' top heights (`max.z`, -0 as 0), sorted, for `raycast_replay`.
    tops: Vec<f32>,
}

const LEAF_SIZE: usize = 4;

impl Bvh {
    fn build(tris: &[Tri], search: bool) -> Self {
        let (median, order) = Self::build_median(tris);
        if !search {
            return Self { median, order, ..Default::default() };
        }
        let mut leaves: Vec<Node> = median.iter().copied().filter(|n| n.count > 0).collect();
        let mut tops: Vec<f32> = leaves.iter().map(|l| l.max.z + 0.0).collect();
        let mut sorted = Vec::new();
        let nodes = std::thread::scope(|scope| {
            scope.spawn(|| {
                tops.sort_unstable_by(f32::total_cmp);
                tops.dedup();
                sorted = order.iter().map(|&i| tris[i as usize]).collect();
            });
            sah_build(&mut leaves)
        });
        Self { nodes, leaves, median, order, sorted, tops }
    }

    /// Walk the search tree for the median-tree leaves overlapping the box (in no particular
    /// order).
    fn search(&self, min: Vec3, max: Vec3, mut f: impl FnMut(&Node)) {
        if !overlaps(&self.nodes[0], min, max) {
            return;
        }
        let mut stack = [0u32; STACK];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let n = self.nodes[stack[sp] as usize];
            if n.count > 0 {
                for leaf in &self.leaves[n.start as usize..(n.start + n.count) as usize] {
                    if overlaps(leaf, min, max) {
                        f(leaf);
                    }
                }
            } else {
                // Children are tested here, so only overlapping ones are pushed.
                for c in [n.start, n.start + 1] {
                    if overlaps(&self.nodes[c as usize], min, max) {
                        stack[sp] = c;
                        sp += 1;
                    }
                }
            }
        }
    }

    /// The triangle at position `at` of `order`.
    fn tri<'a>(&'a self, tris: &'a [Tri], at: u32) -> &'a Tri {
        match self.sorted.get(at as usize) {
            Some(t) => t,
            None => &tris[self.order[at as usize] as usize],
        }
    }

    /// Whether the median-tree ray walk (`raycast_median`: nearer child first, the left one
    /// on a tie) reaches leaf `a` after leaf `b` (leaves by their starts; distinct). At their
    /// lowest common ancestor the child entered later comes second.
    fn ray_after(&self, o: Vec3, inv: Vec3, max_t: f32, a: u32, b: u32) -> bool {
        let (mut ni, mut start, mut count) = (0usize, 0u32, self.order.len() as u32);
        loop {
            let n = self.median[ni];
            // Distinct leaves part ways at an inner node before either is reached.
            debug_assert!(n.count == 0);
            let mid = start + count / 2;
            let (a_left, b_left) = (a < mid, b < mid);
            if a_left == b_left {
                ni = (n.start + !a_left as u32) as usize;
                (start, count) = if a_left { (start, count / 2) } else { (mid, count - count / 2) };
                continue;
            }
            let (l, r) = (self.median[n.start as usize], self.median[n.start as usize + 1]);
            let tl = ray_box(o, inv, l.min, l.max, max_t).unwrap_or(f32::MAX);
            let tr = ray_box(o, inv, r.min, r.max, max_t).unwrap_or(f32::MAX);
            return if tl <= tr { !a_left } else { a_left };
        }
    }

    /// The median tree: its nodes (empty without triangles) and the triangle order.
    fn build_median(tris: &[Tri]) -> (Vec<Node>, Vec<u32>) {
        let mut order: Vec<u32> = (0..tris.len() as u32).collect();
        // Per-triangle bounds and centroids, computed once (cache friendly during the build).
        let boxes: Vec<(Vec3, Vec3)> = tris.iter().map(|t| (t.a.min(t.b).min(t.c), t.a.max(t.b).max(t.c))).collect();
        let centroids: Vec<Vec3> = boxes.iter().map(|(lo, hi)| (*lo + *hi) * 0.5).collect();
        if tris.is_empty() {
            return (Vec::new(), order);
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
            for (&ni, split) in pending.iter().zip(ctx.split_level(&mut nodes, &mut order, &pending)) {
                match split {
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
        (nodes, order)
    }
}

/// Search tree leaves hold up to this many median-tree leaves (their boxes are tested one
/// after another, which is cheaper than more levels).
const SAH_LEAF: usize = 8;
const SAH_BINS: usize = 16;
/// From this depth on the search tree splits at the median (halving: at most ~32 more
/// levels), so the traversal stacks (`STACK`) never fill up.
const SAH_MAX_DEPTH: usize = 64;
const STACK: usize = 128;
/// The top levels of the search tree split on this many threads (2^depth subtrees).
const SAH_PAR_DEPTH: usize = 6;

fn half_area(lo: Vec3, hi: Vec3) -> f32 {
    let e = (hi - lo).max(Vec3::ZERO);
    e.x * e.y + e.y * e.z + e.z * e.x
}

/// A box's min and max corners.
type Bounds = (Vec3, Vec3);

fn bounds_of(prims: &[Node]) -> Bounds {
    prims.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(p.min), hi.max(p.max)))
}

/// Per-axis bins of box centres: each bin's bounds and count.
#[derive(Clone, Copy)]
struct Bins {
    lo: [[Vec3; SAH_BINS]; 3],
    hi: [[Vec3; SAH_BINS]; 3],
    n: [[u32; SAH_BINS]; 3],
}

impl Bins {
    fn new() -> Self {
        Self { lo: [[Vec3::splat(f32::MAX); SAH_BINS]; 3], hi: [[Vec3::splat(f32::MIN); SAH_BINS]; 3], n: [[0; SAH_BINS]; 3] }
    }

    fn add(&mut self, prims: &[Node], clo: Vec3, scale: Vec3) {
        for p in prims {
            for (axis, &i) in bin_of(p, clo, scale).iter().enumerate() {
                self.lo[axis][i] = self.lo[axis][i].min(p.min);
                self.hi[axis][i] = self.hi[axis][i].max(p.max);
                self.n[axis][i] += 1;
            }
        }
    }

    fn merge(&mut self, o: &Bins) {
        for axis in 0..3 {
            for i in 0..SAH_BINS {
                self.lo[axis][i] = self.lo[axis][i].min(o.lo[axis][i]);
                self.hi[axis][i] = self.hi[axis][i].max(o.hi[axis][i]);
                self.n[axis][i] += o.n[axis][i];
            }
        }
    }

    /// Bounds of bins `range` on `axis`.
    fn bounds(&self, axis: usize, range: std::ops::Range<usize>) -> Bounds {
        range.fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), i| (lo.min(self.lo[axis][i]), hi.max(self.hi[axis][i])))
    }
}

fn centre(p: &Node) -> Vec3 {
    (p.min + p.max) * 0.5
}

/// The bin of `p`'s centre on `axis` (as `bin_of`).
fn bin_on(p: &Node, axis: usize, clo: Vec3, scale: Vec3) -> usize {
    (((centre(p)[axis] - clo[axis]) * scale[axis]) as usize).min(SAH_BINS - 1)
}

/// The bin of `p`'s centre on each axis.
fn bin_of(p: &Node, clo: Vec3, scale: Vec3) -> [usize; 3] {
    let f = (centre(p) - clo) * scale;
    [(f.x as usize).min(SAH_BINS - 1), (f.y as usize).min(SAH_BINS - 1), (f.z as usize).min(SAH_BINS - 1)]
}

/// Nodes with at least this many boxes are binned on several threads.
const SAH_PAR_BIN: usize = 1 << 16;

/// Reorder `prims` into the two halves of a binned-SAH split (on box centres) and return the
/// left half's size and both halves' bounds, or None for a leaf.
fn sah_split(prims: &mut [Node], depth: usize) -> Option<(usize, Bounds, Bounds)> {
    let n = prims.len();
    if n <= SAH_LEAF {
        return None;
    }
    let (clo, chi) = prims.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(centre(p)), hi.max(centre(p))));
    let ext = chi - clo;
    let scale = Vec3::splat(SAH_BINS as f32) / ext;
    if depth < SAH_MAX_DEPTH {
        let mut bins = Bins::new();
        if n >= SAH_PAR_BIN {
            let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
            let parts: Vec<Bins> = std::thread::scope(|scope| {
                let handles: Vec<_> = prims
                    .chunks(n.div_ceil(threads))
                    .map(|chunk| {
                        scope.spawn(move || {
                            let mut b = Bins::new();
                            b.add(chunk, clo, scale);
                            b
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().expect("bvh worker")).collect()
            });
            for p in &parts {
                bins.merge(p);
            }
        } else {
            bins.add(prims, clo, scale);
        }
        // Best (cost, axis, first bin of the right half).
        let mut best: Option<(f32, usize, usize)> = None;
        for axis in 0..3 {
            if !(ext[axis] > 0.0) {
                continue;
            }
            let mut right_cost = [0f32; SAH_BINS];
            let (mut lo, mut hi, mut cnt) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN), 0u32);
            for i in (1..SAH_BINS).rev() {
                lo = lo.min(bins.lo[axis][i]);
                hi = hi.max(bins.hi[axis][i]);
                cnt += bins.n[axis][i];
                right_cost[i] = if cnt > 0 { half_area(lo, hi) * cnt as f32 } else { 0.0 };
            }
            let (mut lo, mut hi, mut cnt) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN), 0u32);
            for i in 1..SAH_BINS {
                lo = lo.min(bins.lo[axis][i - 1]);
                hi = hi.max(bins.hi[axis][i - 1]);
                cnt += bins.n[axis][i - 1];
                if cnt == 0 || cnt as usize == n {
                    continue;
                }
                let cost = half_area(lo, hi) * cnt as f32 + right_cost[i];
                if best.is_none_or(|b| cost < b.0) {
                    best = Some((cost, axis, i));
                }
            }
        }
        if let Some((_, axis, split)) = best {
            // Branch-free partition (the side is a coin flip to the branch predictor).
            let mut left = 0;
            for i in 0..n {
                let goes_left = bin_on(&prims[i], axis, clo, scale) < split;
                prims.swap(i, left);
                left += goes_left as usize;
            }
            return Some((left, bins.bounds(axis, 0..split), bins.bounds(axis, split..SAH_BINS)));
        }
    }
    // Every centre in one bin, or too deep: halve at the median of the longest centre axis.
    let axis = if ext.x >= ext.y && ext.x >= ext.z { 0 } else if ext.y >= ext.z { 1 } else { 2 };
    let mid = n / 2;
    prims.select_nth_unstable_by(mid, |a, b| centre(a)[axis].total_cmp(&centre(b)[axis]));
    Some((mid, bounds_of(&prims[..mid]), bounds_of(&prims[mid..])))
}

/// A search subtree being built: split nodes at the top (built on their own threads), then
/// flat subtrees (node 0 is the root, leaf starts relative to `base`).
enum SahPart {
    Split(Vec3, Vec3, Box<SahPart>, Box<SahPart>),
    Flat(Vec<Node>, u32),
}

/// Build the search subtree over `prims` (bounds `lo..hi`, starting at `base` in the leaf list).
fn sah_part(prims: &mut [Node], (lo, hi): Bounds, base: u32, depth: usize) -> SahPart {
    if depth < SAH_PAR_DEPTH && prims.len() > 4096 {
        if let Some((mid, lb, rb)) = sah_split(prims, depth) {
            let (l, r) = prims.split_at_mut(mid);
            let (lp, rp) = std::thread::scope(|scope| {
                let lh = scope.spawn(|| sah_part(l, lb, base, depth + 1));
                let rp = sah_part(r, rb, base + mid as u32, depth + 1);
                (lh.join().expect("bvh worker"), rp)
            });
            return SahPart::Split(lo, hi, Box::new(lp), Box::new(rp));
        }
    }
    // (node, depth) of the nodes still to split; leaf starts are relative to `prims`.
    let mut nodes = vec![Node { min: lo, max: hi, start: 0, count: prims.len() as u32 }];
    let mut stack = vec![(0usize, depth)];
    while let Some((ni, d)) = stack.pop() {
        let Node { start, count, min, max } = nodes[ni];
        let slice = &mut prims[start as usize..(start + count) as usize];
        if let Some((mid, (llo, lhi), (rlo, rhi))) = sah_split(slice, d) {
            let left = nodes.len();
            nodes.push(Node { min: llo, max: lhi, start, count: mid as u32 });
            nodes.push(Node { min: rlo, max: rhi, start: start + mid as u32, count: count - mid as u32 });
            nodes[ni] = Node { min, max, start: left as u32, count: 0 };
            stack.push((left, d + 1));
            stack.push((left + 1, d + 1));
        }
    }
    SahPart::Flat(nodes, base)
}

/// Lay `part` out with its root at `slot` of `out` (children pairs adjacent).
fn sah_emit(part: SahPart, slot: usize, out: &mut Vec<Node>) {
    match part {
        SahPart::Split(min, max, l, r) => {
            let c = out.len();
            out.push(Node { min, max, start: 0, count: 0 });
            out.push(Node { min, max, start: 0, count: 0 });
            out[slot] = Node { min, max, start: c as u32, count: 0 };
            sah_emit(*l, c, out);
            sah_emit(*r, c + 1, out);
        }
        SahPart::Flat(sub, base) => {
            let offset = out.len() as u32;
            let remap = |n: Node| if n.count == 0 { Node { start: offset + n.start - 1, ..n } } else { Node { start: n.start + base, ..n } };
            out[slot] = remap(sub[0]);
            out.extend(sub[1..].iter().map(|&n| remap(n)));
        }
    }
}

/// Build the search tree over `prims` (reordered to its leaf order).
fn sah_build(prims: &mut [Node]) -> Vec<Node> {
    if prims.is_empty() {
        return Vec::new();
    }
    let part = sah_part(prims, bounds_of(prims), 0, 0);
    let mut out = vec![Node { min: Vec3::ZERO, max: Vec3::ZERO, start: 0, count: 0 }];
    sah_emit(part, 0, &mut out);
    out
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

    /// `split` every node of `pending` (disjoint ranges of `order`), partitioning their
    /// triangles in parallel; the nodes are appended in `pending` order, exactly as splitting
    /// them one after another would.
    fn split_level(&self, nodes: &mut Vec<Node>, order: &mut [u32], pending: &[usize]) -> Vec<Option<usize>> {
        let mut by_start: Vec<usize> = (0..pending.len()).collect();
        by_start.sort_by_key(|&i| nodes[pending[i]].start);
        let mut slices: Vec<Option<&mut [u32]>> = (0..pending.len()).map(|_| None).collect();
        let mut rest: &mut [u32] = order;
        let mut consumed = 0u32;
        for &i in &by_start {
            let Node { start, count, .. } = nodes[pending[i]];
            let tail = std::mem::take(&mut rest);
            let (_, tail) = tail.split_at_mut((start - consumed) as usize);
            let (mine, tail) = tail.split_at_mut(count as usize);
            rest = tail;
            consumed = start + count;
            slices[i] = Some(mine);
        }
        // Per node: the left half's size and both halves' bounds (None for a leaf).
        type Halves = Option<(usize, (Vec3, Vec3), (Vec3, Vec3))>;
        let halves: Vec<Halves> = std::thread::scope(|scope| {
            let handles: Vec<_> = pending
                .iter()
                .zip(slices)
                .map(|(&ni, slice)| {
                    let Node { count, min, max, .. } = nodes[ni];
                    let slice = slice.expect("every pending node has a range");
                    scope.spawn(move || -> Halves {
                        if (count as usize) <= LEAF_SIZE {
                            return None;
                        }
                        let ext = max - min;
                        let axis = if ext.x >= ext.y && ext.x >= ext.z { 0 } else if ext.y >= ext.z { 1 } else { 2 };
                        let mid = count as usize / 2;
                        slice.select_nth_unstable_by(mid, |&a, &b| self.centroids[a as usize][axis].total_cmp(&self.centroids[b as usize][axis]));
                        Some((mid, self.bounds(&slice[..mid]), self.bounds(&slice[mid..])))
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().expect("bvh worker")).collect()
        });
        pending
            .iter()
            .zip(halves)
            .map(|(&ni, h)| {
                let (mid, (llo, lhi), (rlo, rhi)) = h?;
                let Node { start, count, min, max } = nodes[ni];
                let left = nodes.len();
                nodes.push(Node { min: llo, max: lhi, start, count: mid as u32 });
                nodes.push(Node { min: rlo, max: rhi, start: start + mid as u32, count: count - mid as u32 });
                nodes[ni] = Node { min, max, start: left as u32, count: 0 };
                Some(left)
            })
            .collect()
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

/// Rays up to this long (game units) replay the median-tree walk over the leaves they reach
/// (`raycast_replay`); longer ones, which the walk's limit cuts short, walk the median tree.
const SHORT_RAY: f32 = 400.0;

/// A list kept on the stack while it is short.
struct SmallBuf<T: Copy + Default, const N: usize> {
    first: [T; N],
    len: usize,
    rest: Vec<T>,
}

impl<T: Copy + Default, const N: usize> SmallBuf<T, N> {
    fn new() -> Self {
        Self { first: [T::default(); N], len: 0, rest: Vec::new() }
    }

    fn len(&self) -> usize {
        self.len + self.rest.len()
    }

    fn push(&mut self, v: T) {
        if self.len < N {
            self.first[self.len] = v;
            self.len += 1;
        } else {
            self.rest.push(v);
        }
    }

    /// The items (in push order while they fit on the stack; `as_mut_slice` keeps that order).
    fn as_slice(&self) -> &[T] {
        if self.rest.is_empty() {
            &self.first[..self.len]
        } else {
            assert!(self.len == 0, "as_mut_slice first");
            &self.rest
        }
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        if self.rest.is_empty() {
            return &mut self.first[..self.len];
        }
        if self.len > 0 {
            self.rest.splice(0..0, self.first[..self.len].iter().copied());
            self.len = 0;
        }
        &mut self.rest
    }
}

fn overlaps(n: &Node, min: Vec3, max: Vec3) -> bool {
    !(n.max.x < min.x || n.max.y < min.y || n.max.z < min.z || n.min.x > max.x || n.min.y > max.y || n.min.z > max.z)
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
        self.bvh.get_or_init(|| Bvh::build(&self.tris, !self.median_only))
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
        // The sets' hierarchies are independent: build them at the same time.
        std::thread::scope(|scope| {
            for c in self.clips.iter().flatten().chain(self.detail.iter()) {
                scope.spawn(|| c.bvh());
            }
            self.bvh();
        });
    }

    /// Add a triangle that only shots and other non-movement traces hit (`detail`).
    pub fn add_detail(&mut self, a: Vec3, b: Vec3, c: Vec3) {
        self.detail.get_or_insert_with(|| Box::new(CollisionWorld { median_only: true, ..Default::default() })).add(a, b, c);
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
        push_sphere_in(&self.with_clip(which)[..], center, radius, max_normal_z)
    }

    /// Visit the triangles of every median-tree leaf whose box overlaps the box, in that
    /// tree's depth-first order (see `Bvh`).
    fn cells_in(&self, min: Vec3, max: Vec3, mut f: impl FnMut(&Tri)) {
        let bvh = self.bvh();
        if bvh.nodes.is_empty() {
            return self.cells_in_median(bvh, min, max, f);
        }
        // The overlapping leaves' (start, count) ranges of `order`.
        let mut found: SmallBuf<(u32, u32), 64> = SmallBuf::new();
        bvh.search(min, max, |leaf| found.push((leaf.start, leaf.count)));
        let found = found.as_mut_slice();
        found.sort_unstable_by_key(|&(start, _)| std::cmp::Reverse(start));
        for &(start, count) in found.iter() {
            for tri in &bvh.sorted[start as usize..(start + count) as usize] {
                f(tri);
            }
        }
    }

    /// The median-tree leaves overlapping the box, in that tree's depth-first order (`cells_in`
    /// visits their triangles).
    fn leaves_in<const N: usize>(&self, min: Vec3, max: Vec3, out: &mut SmallBuf<Node, N>) {
        let bvh = self.bvh();
        if bvh.nodes.is_empty() {
            // The median walk (`cells_in_median`) meets them in that order.
            let mut stack = [0u32; 64];
            let mut sp = if bvh.median.is_empty() { 0 } else { 1 };
            while sp > 0 {
                sp -= 1;
                let n = bvh.median[stack[sp] as usize];
                if !overlaps(&n, min, max) {
                    continue;
                }
                if n.count > 0 {
                    out.push(n);
                } else if sp + 2 <= stack.len() {
                    stack[sp] = n.start;
                    stack[sp + 1] = n.start + 1;
                    sp += 2;
                }
            }
            out.as_mut_slice();
            return;
        }
        bvh.search(min, max, |leaf| out.push(*leaf));
        out.as_mut_slice().sort_unstable_by_key(|n| std::cmp::Reverse(n.start));
    }

    /// `cells_in` by walking the median tree, right child first.
    fn cells_in_median(&self, bvh: &Bvh, min: Vec3, max: Vec3, mut f: impl FnMut(&Tri)) {
        if bvh.median.is_empty() {
            return;
        }
        let mut stack = [0u32; 64];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let n = bvh.median[stack[sp] as usize];
            if !overlaps(&n, min, max) {
                continue;
            }
            if n.count > 0 {
                for at in n.start..n.start + n.count {
                    f(bvh.tri(&self.tris, at));
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
        if bvh.median.is_empty() {
            return None;
        }
        if !bvh.nodes.is_empty() && dir.length() * max_t <= SHORT_RAY {
            self.raycast_replay(bvh, origin, dir, max_t, None)
        } else {
            self.raycast_median(bvh, origin, dir, max_t)
        }
    }

    /// `raycast_tris` for a short ray. The median-tree walk (`raycast_median`) visits a leaf
    /// when `ray_box` reaches it at the limit of the time (its ancestors' boxes contain it, so
    /// they pass too, unless the ray lies on a box face: see below) and takes each hit no
    /// farther than the limit; leaves without hits change nothing. So this finds every leaf the
    /// ray reaches within `max_t` and every hit in them, then visits the leaves with hits in
    /// the walk's order (`ray_after`) keeping the limit as the walk does: the same result, ties
    /// and hits a hair in front of their leaf's box included, without walking the median
    /// tree's overlapping boxes.
    /// `region`: a box and the leaves overlapping it, which serve instead of the search tree
    /// when the ray's box is inside it (see `Region`).
    fn raycast_replay(&self, bvh: &Bvh, origin: Vec3, dir: Vec3, max_t: f32, region: Option<(Vec3, Vec3, &[Node])>) -> Option<Hit> {
        let inv = Vec3::ONE / dir;
        // `ray_box` is monotone in the box (a box containing one the ray reaches is reached
        // too) except where a slab is 0 * inf: a ray that doesn't move along an axis and lies
        // exactly on a face. glam's min and max then skip the NaN on x and y (the slab counts
        // as passed: harmless, and leaves with hits that see it go to the walk below), but a
        // NaN on z (a level ray at exactly a box's top) takes `max_element` and `min_element`
        // with it, and the box passes wherever it is. Such rays walk the median tree.
        if !(origin.is_finite() && dir.is_finite()) || (!inv.z.is_finite() && bvh.tops.binary_search_by(|z| z.total_cmp(&(origin.z + 0.0))).is_ok()) {
            return self.raycast_median(bvh, origin, dir, max_t);
        }
        // The search tree is walked, and its leaves screened before `ray_box`, with the ray's
        // box padded: every box `ray_box` reaches overlaps it, so the cheaper test drops none of
        // them. Off the padded range on an axis, `ray_box` misses: behind the origin both slab
        // distances are negative, beyond the end the nearer one exceeds `max_t` by far more
        // than a ray this short can round, and a ray that doesn't move along the axis lies
        // within the slab or misses it (the 0 * inf slabs above are on a face, within range).
        let end = origin + dir * max_t;
        let pad = Vec3::splat(1.0 + 1e-5 * origin.abs().max_element());
        let (lo, hi) = (origin.min(end) - pad, origin.max(end) + pad);
        // Hit leaves (the leaf, its first hit, hit count) and the hits (position in `order`, t).
        let mut leaves: SmallBuf<(Node, u32, u32), 16> = SmallBuf::new();
        let mut hits: SmallBuf<(u32, f32), 32> = SmallBuf::new();
        let mut on_face = false;
        let visit = |leaf: &Node| {
            if on_face || ray_box(origin, inv, leaf.min, leaf.max, max_t).is_none() {
                return;
            }
            let first = hits.len() as u32;
            for at in leaf.start..leaf.start + leaf.count {
                let tri = &bvh.sorted[at as usize];
                if let Some(t) = ray_tri(origin, dir, tri) {
                    if t <= max_t {
                        hits.push((at, t));
                    }
                }
            }
            if hits.len() as u32 > first {
                // The ray lies on a face of this box (see above). Rare; let the walk sort it
                // out.
                on_face = ((leaf.min - origin) * inv).is_nan() || ((leaf.max - origin) * inv).is_nan();
                leaves.push((*leaf, first, hits.len() as u32 - first));
            }
        };
        match region {
            Some((rlo, rhi, list)) if lo.cmpge(rlo).all() && hi.cmple(rhi).all() => list.iter().filter(|l| overlaps(l, lo, hi)).for_each(visit),
            _ => bvh.search(lo, hi, visit),
        }
        if on_face {
            return self.raycast_median(bvh, origin, dir, max_t);
        }
        let leaves = leaves.as_mut_slice();
        // Into walk order (insertion sort: there are seldom more than a few).
        for i in 1..leaves.len() {
            let mut j = i;
            while j > 0 && bvh.ray_after(origin, inv, max_t, leaves[j - 1].0.start, leaves[j].0.start) {
                leaves.swap(j - 1, j);
                j -= 1;
            }
        }
        let hits = hits.as_mut_slice();
        let mut limit = max_t;
        let mut best: Option<(u32, f32)> = None;
        for (leaf, first, count) in leaves.iter() {
            if ray_box(origin, inv, leaf.min, leaf.max, limit).is_none() {
                continue;
            }
            for &(at, t) in &hits[*first as usize..(first + count) as usize] {
                if t <= limit {
                    limit = t;
                    best = Some((at, t));
                }
            }
        }
        best.map(|(at, t)| {
            let tri = &bvh.sorted[at as usize];
            let nrm = if tri.n.dot(dir) > 0.0 { -tri.n } else { tri.n };
            Hit { t, point: origin + dir * t, normal: nrm }
        })
    }

    /// `raycast_tris` by walking the median tree, nearer child first.
    fn raycast_median(&self, bvh: &Bvh, origin: Vec3, dir: Vec3, max_t: f32) -> Option<Hit> {
        let inv = Vec3::ONE / dir;
        let mut best: Option<Hit> = None;
        let mut limit = max_t;
        // Each entry keeps its box's entry distance from when it was pushed: whether the ray
        // reaches a box depends on the limit only through `entry <= limit` (`ray_box`), so the
        // box needn't be tested again when it is popped.
        let Some(t_root) = ray_box(origin, inv, bvh.median[0].min, bvh.median[0].max, limit) else { return None };
        let mut stack = [(0u32, 0f32); 64];
        stack[0] = (0, t_root);
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let (ni, entry) = stack[sp];
            if entry > limit {
                continue;
            }
            let n = bvh.median[ni as usize];
            if n.count > 0 {
                for at in n.start..n.start + n.count {
                    let tri = bvh.tri(&self.tris, at);
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
                let (a, b) = (bvh.median[n.start as usize], bvh.median[n.start as usize + 1]);
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
        push_sphere_in(&[Some(self), self.detail.as_deref()][..], center, radius, max_normal_z)
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

/// Where box queries find their triangles: `CollisionWorld::cells_in` over several worlds, or
/// a `Region` of them.
trait Cells {
    fn cells(&self, min: Vec3, max: Vec3, f: impl FnMut(&Tri));
}

impl Cells for [Option<&CollisionWorld>] {
    fn cells(&self, min: Vec3, max: Vec3, mut f: impl FnMut(&Tri)) {
        for w in self.iter().flatten() {
            w.cells_in(min, max, &mut f);
        }
    }
}

/// The median-tree leaves of the movement sets (`with_clip`) that overlap a box, for several
/// box queries inside it (a Titan's body spheres and their pushes): those filter the leaves
/// instead of walking the trees. A leaf that overlaps a box inside the region's box overlaps
/// the region's too, so they visit exactly the leaves `cells_in` would, in the same order.
pub(crate) struct Region<'a> {
    lo: Vec3,
    hi: Vec3,
    sets: [(Option<&'a CollisionWorld>, SmallBuf<Node, 128>); 2],
}

impl Cells for Region<'_> {
    fn cells(&self, min: Vec3, max: Vec3, mut f: impl FnMut(&Tri)) {
        if !(min.cmpge(self.lo).all() && max.cmple(self.hi).all()) {
            // Outside the region: walk the trees.
            for (w, _) in &self.sets {
                if let Some(w) = w {
                    w.cells_in(min, max, &mut f);
                }
            }
            return;
        }
        for (w, leaves) in &self.sets {
            let Some(w) = w else { continue };
            let bvh = w.bvh();
            for leaf in leaves.as_slice() {
                if overlaps(leaf, min, max) {
                    for at in leaf.start..leaf.start + leaf.count {
                        f(bvh.tri(&w.tris, at));
                    }
                }
            }
        }
    }
}

impl Region<'_> {
    /// `CollisionWorld::raycast_moving` in the region's sets.
    pub(crate) fn raycast(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<Hit> {
        let [a, b] = &self.sets;
        let cast = |(w, leaves): &(Option<&CollisionWorld>, SmallBuf<Node, 128>)| {
            let w = (*w)?;
            let bvh = w.bvh();
            if !bvh.nodes.is_empty() && !bvh.median.is_empty() && dir.length() * max_t <= SHORT_RAY {
                w.raycast_replay(bvh, origin, dir, max_t, Some((self.lo, self.hi, leaves.as_slice())))
            } else {
                w.raycast_tris(origin, dir, max_t)
            }
        };
        nearer(cast(a), cast(b))
    }

    /// `CollisionWorld::push_sphere_moving` in the region's sets.
    pub(crate) fn push_sphere(&self, center: Vec3, radius: f32, max_normal_z: f32) -> (Vec3, Vec3) {
        push_sphere_in(self, center, radius, max_normal_z)
    }
}

impl CollisionWorld {
    /// The region `lo..hi` of this world and clip set `which`, for movement box queries in it.
    pub(crate) fn region_moving(&self, lo: Vec3, hi: Vec3, which: usize) -> Region<'_> {
        let sets = self.with_clip(which).map(|w| {
            let mut leaves = SmallBuf::new();
            if let Some(w) = w {
                w.leaves_in(lo, hi, &mut leaves);
            }
            (w, leaves)
        });
        Region { lo, hi, sets }
    }
}

/// `CollisionWorld::push_sphere` against several worlds at once.
fn push_sphere_in<C: Cells + ?Sized>(worlds: &C, center: Vec3, radius: f32, max_normal_z: f32) -> (Vec3, Vec3) {
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
        worlds.cells(c - r, c + r, &mut visit);
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
            w.cells_in(lo, hi, |tri| match sweep_box_tri_in(c0, half, d, tie, tri, lo, hi) {
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
fn sweep_box_tri_in(c: Vec3, e: Vec3, d: Vec3, tie: f32, t: &Tri, lo: Vec3, hi: Vec3) -> Sweep {
    if t.a.max(t.b).max(t.c).cmplt(lo).any() || t.a.min(t.b).min(t.c).cmpgt(hi).any() {
        return Sweep::Miss;
    }
    sweep_box_tri(c, e, d, tie, t)
}

/// Separating-axis sweep of a box (centre `c`, half extents `e`, moving by `d`) against a
/// triangle: the box's three axes, the triangle's normal and the nine edge cross products. An
/// edge-edge axis only takes over the entry from a face axis when it enters `tie` (0.01 units
/// travelled, as a fraction of the move) later.
fn sweep_box_tri(c: Vec3, e: Vec3, d: Vec3, tie: f32, t: &Tri) -> Sweep {
    let v = [t.a - c, t.b - c, t.c - c];
    let edges = [v[1] - v[0], v[2] - v[1], v[0] - v[2]];
    let (mut t_enter, mut t_exit) = (f32::NEG_INFINITY, f32::INFINITY);
    let mut n_enter = Vec3::ZERO;
    let mut face = true;
    let mut inside = true;
    let (mut depth, mut mtv) = (f32::INFINITY, Vec3::ZERO);
    // Separation along the unit axis `l`.
    let mut test = |l: Vec3, is_face: bool| -> bool {
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
    // An axis too short to have a direction separates nothing. (The box's own axes are unit
    // length already: normalizing them would change nothing.)
    let unit = |axis: Vec3| {
        let len2 = axis.length_squared();
        (len2 >= 1e-10).then(|| axis / len2.sqrt())
    };
    let n = (v[1] - v[0]).cross(v[2] - v[0]);
    if !(test(Vec3::X, true) && test(Vec3::Y, true) && test(Vec3::Z, true) && unit(n).is_none_or(|l| test(l, true))) {
        return Sweep::Miss;
    }
    for ed in edges {
        for ax in [Vec3::X, Vec3::Y, Vec3::Z] {
            if !unit(ax.cross(ed)).is_none_or(|l| test(l, false)) {
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
    fn parallel_split_level_matches_serial_splits() {
        // Pseudo-random triangles; the top levels split node by node (as before) and level by
        // level in parallel must give the same nodes and order.
        let mut seed = 0x1234_5678u32;
        let mut r = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 1000.0
        };
        let tris: Vec<Tri> = (0..5000).map(|_| {
            let a = Vec3::new(r(), r(), r());
            Tri { a, b: a + Vec3::new(r(), r(), r()) * 0.01, c: a + Vec3::new(r(), r(), r()) * 0.01, n: Vec3::Z }
        }).collect();
        let boxes: Vec<(Vec3, Vec3)> = tris.iter().map(|t| (t.a.min(t.b).min(t.c), t.a.max(t.b).max(t.c))).collect();
        let centroids: Vec<Vec3> = boxes.iter().map(|(lo, hi)| (*lo + *hi) * 0.5).collect();
        let ctx = BuildCtx { boxes: &boxes, centroids: &centroids };
        let run = |parallel: bool| {
            let mut order: Vec<u32> = (0..tris.len() as u32).collect();
            let (lo, hi) = ctx.bounds(&order);
            let mut nodes = vec![Node { min: lo, max: hi, start: 0, count: tris.len() as u32 }];
            let mut pending = vec![0usize];
            for _ in 0..8 {
                let splits: Vec<Option<usize>> = if parallel { ctx.split_level(&mut nodes, &mut order, &pending) } else { pending.iter().map(|&ni| ctx.split(&mut nodes, &mut order, ni)).collect() };
                pending = pending.iter().zip(splits).flat_map(|(&ni, s)| s.map_or(vec![ni], |l| vec![l, l + 1])).collect();
            }
            let flat: Vec<[u32; 8]> = nodes.iter().map(|n| [n.min.x, n.min.y, n.min.z, n.max.x, n.max.y, n.max.z].map(f32::to_bits).iter().copied().chain([n.start, n.count]).collect::<Vec<_>>().try_into().unwrap()).collect();
            (flat, order)
        };
        assert!(run(false) == run(true));
    }

    /// A world built to give the queries trouble: an axis-aligned grid of floors and walls
    /// (shared edges, exact ties, rays lying on box faces, hits that round to a hair in front
    /// of their box), duplicated and re-wound triangles, big triangles over small ones (the
    /// median tree's overlap) and random slivers. `at` is the grid's corner.
    fn awkward_world(seed: u32, random: usize, at: Vec3) -> CollisionWorld {
        let mut seed = seed;
        let mut r = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let mut w = CollisionWorld::default();
        for i in 0..12 {
            for j in 0..12 {
                let (x, y, z) = (at.x + i as f32 * 64.0, at.y + j as f32 * 64.0, at.z + ((i * 7 + j * 3) % 5) as f32 * 16.0);
                let [a, b, c, d] = [Vec3::new(x, y, z), Vec3::new(x + 64.0, y, z), Vec3::new(x + 64.0, y + 64.0, z), Vec3::new(x, y + 64.0, z)];
                w.add(a, b, c);
                w.add(a, c, d);
                if (i + j) % 3 == 0 {
                    w.add(a, b, c); // duplicate
                    w.add(c, a, b); // same triangle, other first vertex
                }
                if (i * j) % 4 == 1 {
                    let h = Vec3::Z * 96.0;
                    w.add(a, b, b + h);
                    w.add(a, b + h, a + h);
                    w.add(b, b + h, a); // overlaps the wall's first half
                    w.add(a + h, a, b + h); // the second half again
                }
                if (i + 2 * j) % 5 == 0 {
                    // A wall stacked many times over, so the copies span median-tree leaves.
                    let h = Vec3::Z * 80.0;
                    for k in 0..12 {
                        let [p, q, r] = [d, a, a + h];
                        match k % 3 {
                            0 => w.add(p, q, r),
                            1 => w.add(q, r, p),
                            _ => w.add(r, q, p),
                        }
                    }
                }
            }
        }
        for k in 0..random {
            let a = at + Vec3::new(r(), r(), r() * 0.2) * 800.0;
            let size = if k % 50 == 0 { 600.0 } else if k % 7 == 0 { 0.05 } else { 20.0 };
            let b = a + Vec3::new(r() - 0.5, r() - 0.5, r() - 0.5) * size;
            let c = if k % 11 == 0 { a + (b - a) * 0.5 + Vec3::splat(1e-3) } else { a + Vec3::new(r() - 0.5, r() - 0.5, r() - 0.5) * size };
            w.add(a, b, c);
        }
        w.finish();
        w
    }

    #[test]
    fn box_queries_visit_what_the_median_walk_did() {
        // The second world has enough median-tree leaves to bin its top splits on threads.
        for (seed, random) in [(7u32, 3000usize), (11, 300_000)] {
            let w = awkward_world(seed, random, Vec3::ZERO);
            let mut s = seed;
            let mut r = move || {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 8) as f32 / (1u32 << 24) as f32
            };
            // Triangles by their index in `w.tris`.
            let bvh = w.bvh();
            let index = |t: &Tri| match bvh.sorted.as_ptr_range().contains(&(t as *const Tri)) {
                true => bvh.order[unsafe { (t as *const Tri).offset_from(bvh.sorted.as_ptr()) } as usize] as isize,
                false => unsafe { (t as *const Tri).offset_from(w.tris.as_ptr()) },
            };
            let queries = if random > 10_000 { 300 } else { 3000 };
            for q in 0..queries {
                let c = if q % 3 == 0 { Vec3::new((r() * 12.0).floor() * 64.0, (r() * 12.0).floor() * 64.0, 16.0) } else { Vec3::new(r(), r(), r() * 0.3) * 800.0 };
                let e = Vec3::new(r(), r(), r()) * if q % 5 == 0 { 200.0 } else { 40.0 };
                let (mut a, mut b) = (Vec::new(), Vec::new());
                w.cells_in(c - e, c + e, |t| a.push(index(t)));
                w.cells_in_median(bvh, c - e, c + e, |t| b.push(index(t)));
                assert_eq!(a, b, "query {q} around {c}");
            }
        }
    }

    #[test]
    fn region_queries_match_the_world_queries() {
        // A region's pushes and rays give exactly what the world's own give, inside the
        // region's box and (searching the trees) outside it.
        let mut w = awkward_world(5, 4000, Vec3::ZERO);
        for t in awkward_world(9, 500, Vec3::new(30.0, 20.0, 8.0)).tris {
            w.add_clip(CLIP_TITAN, t.a, t.b, t.c);
        }
        w.finish();
        let mut s = 17u32;
        let mut r = move || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (s >> 8) as f32 / (1u32 << 24) as f32
        };
        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        let hit = |h: Option<Hit>| h.map(|h| bits(&[h.t, h.point.x, h.point.y, h.point.z, h.normal.x, h.normal.y, h.normal.z]));
        for q in 0..3000 {
            let c = Vec3::new(r(), r(), r() * 0.3) * 800.0;
            let e = Vec3::new(r(), r(), r()) * 150.0;
            let region = w.region_moving(c - e, c + e, CLIP_TITAN);
            for _ in 0..4 {
                let p = c + (Vec3::new(r(), r(), r()) * 2.0 - Vec3::ONE) * e * 1.3;
                let rad = 5.0 + r() * 60.0;
                let (a, an) = region.push_sphere(p, rad, 0.7);
                let (b, bn) = w.push_sphere_moving(p, rad, 0.7, CLIP_TITAN);
                assert_eq!(bits(&[a.x, a.y, a.z, an.x, an.y, an.z]), bits(&[b.x, b.y, b.z, bn.x, bn.y, bn.z]), "push {q} at {p}");
                let d = if q % 2 == 0 { -Vec3::Z } else { Vec3::new(r() - 0.5, r() - 0.5, r() - 0.5) };
                let len = r() * r() * 300.0;
                assert_eq!(hit(region.raycast(p, d, len)), hit(w.raycast_moving(p, d, len, CLIP_TITAN)), "ray {q} from {p} along {d}");
            }
        }
    }

    #[test]
    fn short_rays_match_the_median_walk() {
        // Near the origin (exact arithmetic, exact ties) and at map-like coordinates (rounding).
        for at in [Vec3::ZERO, Vec3::new(-3061.25, 1517.5, -211.0)] {
            short_rays_match_in(&awkward_world(3, 4000, at), at);
        }
    }

    fn short_rays_match_in(w: &CollisionWorld, at: Vec3) {
        let bvh = w.bvh();
        let mut s = 99u32;
        let mut r = move || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (s >> 8) as f32 / (1u32 << 24) as f32
        };
        let bits = |h: Option<Hit>| h.map(|h| [h.t, h.point.x, h.point.y, h.point.z, h.normal.x, h.normal.y, h.normal.z].map(f32::to_bits));
        let mut hits = 0;
        for q in 0..100_000 {
            // Origins on the grid's lines and corners, on its floors' heights, or anywhere.
            let mut o = Vec3::new(r(), r(), r() * 0.25) * 800.0;
            match q % 4 {
                0 => o = (o / 16.0).round() * 16.0,
                1 => o.x = (o.x / 64.0).round() * 64.0,
                _ => {}
            }
            let o = o + at;
            let axes = [Vec3::X, Vec3::Y, Vec3::Z, -Vec3::X, -Vec3::Y, -Vec3::Z, Vec3::new(0.0, 0.0, -1.0)];
            let d = match q % 3 {
                0 => axes[q / 3 % axes.len()],
                // Level, at the walls.
                1 => {
                    let yaw = r() * std::f32::consts::TAU;
                    Vec3::new(yaw.cos(), yaw.sin(), 0.0) * (0.5 + r())
                }
                _ => Vec3::new(r() - 0.5, r() - 0.5, r() - 0.5) * 2.0,
            };
            let max_t = [5.0, 50.0, 200.0, 1000.0][q % 4] * r();
            let a = w.raycast_replay(bvh, o, d, max_t, None);
            let b = w.raycast_median(bvh, o, d, max_t);
            assert_eq!(bits(a), bits(b), "ray {q} from {o} along {d} up to {max_t}");
            hits += a.is_some() as usize;
        }
        assert!(hits > 30_000, "too few hits to mean much: {hits}");
    }

    #[test]
    fn level_ray_at_a_box_top_passes_any_box() {
        // Why `raycast_replay` sends such rays to the median walk: z's slab is 0 * inf = NaN,
        // which glam's `max_element` / `min_element` pass on, so `ray_box` reaches a box far
        // off to the side.
        let (o, d) = (Vec3::new(0.0, 0.0, 10.0), Vec3::X);
        assert!(ray_box(o, Vec3::ONE / d, Vec3::new(500.0, 500.0, 0.0), Vec3::new(600.0, 600.0, 10.0), 100.0).is_some());
        assert!(ray_box(o, Vec3::ONE / d, Vec3::new(500.0, 500.0, 0.0), Vec3::new(600.0, 600.0, 10.5), 100.0).is_none());
    }

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
