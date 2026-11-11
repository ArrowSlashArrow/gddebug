//! Flow layout for the group graph.
//!
//! The graph is read left to right like a flowchart:
//! * A spanning forest is taken over the visible edges (BFS, so every group hangs under the
//!   shortest path that reaches it). Each group's children are ordered by the x position of the
//!   trigger that targets them, then by action order, so a decision's "true" branch sits above its
//!   "false" branch.
//! * Every tree is laid out tidily: depth = column, children stacked vertically and centred on
//!   their parent, so branches fan out above and below.
//! * Everything run directly by the level hangs off a **timeline**: LEVEL sits top-left, a
//!   horizontal line runs right from it, and drops into vertical "buses" from which each entry
//!   subtree branches off, in the order the player reaches them.
//! * Trees are packed into lanes (columns of trees) so the whole graph keeps a screen-like aspect.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use eframe::egui::{Vec2, vec2};

use crate::analysis::{Edge, Gid, Model, ROOT};

/// Horizontal gap between columns: the corridor edges branch in.
pub const H_GAP: f32 = 90.0;
/// Vertical gap between siblings.
pub const V_GAP: f32 = 22.0;
/// Extra vertical gap between trees stacked in one lane.
const BLOCK_GAP: f32 = 36.0;
/// Horizontal gap between lanes.
const LANE_GAP: f32 = 120.0;
/// Distance from a lane's LEVEL bus to the lane's first column.
pub const BUS_GAP: f32 = 60.0;
/// Width / height the packing aims for.
const ASPECT: f32 = 1.7;

/// A vertical LEVEL bus: entry subtrees branch off it to the right.
#[derive(Clone, Copy, Debug)]
pub struct Bus {
    pub x: f32,
    pub bottom: f32,
}

#[derive(Default)]
pub struct Layout {
    /// Node centres.
    pub pos: HashMap<Gid, Vec2>,
    /// Spanning-tree parent of every non-root node.
    pub parent: HashMap<Gid, Gid>,
    /// LEVEL buses, left to right. Empty if nothing is run by the level.
    pub buses: Vec<Bus>,
    /// y of the timeline joining LEVEL to the buses.
    pub timeline_y: f32,
}

/// Sort key of an edge among its source's out-edges: x of the earliest trigger, then action order.
fn edge_key(m: &Model, e: &Edge) -> (f32, usize) {
    let x = e
        .actions
        .iter()
        .map(|&a| m.objects[m.actions[a].trigger].config.pos.0)
        .fold(f32::MAX, f32::min);
    (x, e.actions.iter().copied().min().unwrap_or(0))
}

/// Where a packed block landed: (lane, x, y).
type Placement = Vec<(usize, f32, f32)>;

struct Block {
    root: Gid,
    /// Node offsets from the block's top-left corner.
    offs: Vec<(Gid, Vec2)>,
    size: Vec2,
}

pub fn layout(
    m: &Model,
    nodes: &BTreeSet<Gid>,
    edge_ok: impl Fn(&Edge) -> bool,
    size: &HashMap<Gid, Vec2>,
) -> Layout {
    let sz = |g: Gid| size.get(&g).copied().unwrap_or(vec2(120.0, 40.0));

    // ordered, de-duplicated successors
    let mut succ: HashMap<Gid, Vec<Gid>> = HashMap::new();
    let mut indeg: HashMap<Gid, usize> = HashMap::new();
    for &g in nodes {
        let Some(info) = m.groups.get(&g) else {
            continue;
        };
        let mut outs: Vec<((f32, usize), Gid)> = info
            .out_edges
            .iter()
            .map(|&ei| &m.edges[ei])
            .filter(|e| e.to != g && nodes.contains(&e.to) && edge_ok(e))
            .map(|e| (edge_key(m, e), e.to))
            .collect();
        outs.sort_by(|a, b| a.0.0.total_cmp(&b.0.0).then(a.0.1.cmp(&b.0.1)));
        let mut seen = HashSet::new();
        let list: Vec<Gid> = outs
            .into_iter()
            .map(|(_, t)| t)
            .filter(|t| seen.insert(*t))
            .collect();
        for t in &list {
            *indeg.entry(*t).or_default() += 1;
        }
        succ.insert(g, list);
    }

    // spanning forest by BFS
    let mut parent: HashMap<Gid, Gid> = HashMap::new();
    let mut children: HashMap<Gid, Vec<Gid>> = HashMap::new();
    let mut visited: HashSet<Gid> = HashSet::new();
    let mut bfs = |start: Gid, visited: &mut HashSet<Gid>| {
        visited.insert(start);
        let mut q = VecDeque::from([start]);
        while let Some(g) = q.pop_front() {
            for &t in succ.get(&g).map(|v| v.as_slice()).unwrap_or(&[]) {
                if visited.insert(t) {
                    parent.insert(t, g);
                    children.entry(g).or_default().push(t);
                    q.push_back(t);
                }
            }
        }
    };
    let has_root = nodes.contains(&ROOT);
    if has_root {
        bfs(ROOT, &mut visited);
    }
    let mut starts: Vec<Gid> = nodes.iter().copied().filter(|g| *g != ROOT).collect();
    starts.sort_by_key(|g| (indeg.get(g).copied().unwrap_or(0), *g));
    let mut other_roots = Vec::new();
    for s in starts {
        if !visited.contains(&s) {
            bfs(s, &mut visited);
            other_roots.push(s);
        }
    }

    // tidy layout of each tree
    let build_block = |root: Gid| -> Block {
        // BFS order with depths
        let mut order = vec![(root, 0usize)];
        let mut i = 0;
        while i < order.len() {
            let (g, d) = order[i];
            for &c in children.get(&g).map(|v| v.as_slice()).unwrap_or(&[]) {
                order.push((c, d + 1));
            }
            i += 1;
        }
        let depth_count = order.iter().map(|o| o.1).max().unwrap_or(0) + 1;
        let mut col_w = vec![0.0f32; depth_count];
        for &(g, d) in &order {
            col_w[d] = col_w[d].max(sz(g).x);
        }
        let mut col_x = vec![0.0f32; depth_count];
        for d in 1..depth_count {
            col_x[d] = col_x[d - 1] + col_w[d - 1] + H_GAP;
        }
        let kids = |g: Gid| children.get(&g).map(|v| v.as_slice()).unwrap_or(&[]);
        // extents bottom-up
        let mut ext: HashMap<Gid, f32> = HashMap::new();
        for &(g, _) in order.iter().rev() {
            let k = kids(g);
            let sum: f32 =
                k.iter().map(|c| ext[c]).sum::<f32>() + V_GAP * k.len().saturating_sub(1) as f32;
            ext.insert(g, sz(g).y.max(sum));
        }
        // tops top-down
        let mut top: HashMap<Gid, f32> = HashMap::from([(root, 0.0)]);
        for &(g, _) in &order {
            let k = kids(g);
            let sum: f32 =
                k.iter().map(|c| ext[c]).sum::<f32>() + V_GAP * k.len().saturating_sub(1) as f32;
            let mut y = top[&g] + (ext[&g] - sum) / 2.0;
            for c in k {
                top.insert(*c, y);
                y += ext[c] + V_GAP;
            }
        }
        // centres bottom-up: parents centred on their first and last child
        let mut cy: HashMap<Gid, f32> = HashMap::new();
        for &(g, _) in order.iter().rev() {
            let k = kids(g);
            let y = match (k.first(), k.last()) {
                (Some(a), Some(b)) => (cy[a] + cy[b]) / 2.0,
                _ => top[&g] + ext[&g] / 2.0,
            };
            cy.insert(g, y);
        }
        let offs = order
            .iter()
            .map(|&(g, d)| (g, vec2(col_x[d] + sz(g).x / 2.0, cy[&g])))
            .collect();
        Block {
            root,
            offs,
            size: vec2(col_x[depth_count - 1] + col_w[depth_count - 1], ext[&root]),
        }
    };

    let root_blocks: Vec<Block> = children
        .get(&ROOT)
        .map(|v| v.as_slice())
        .unwrap_or(&[])
        .iter()
        .map(|&g| build_block(g))
        .collect();
    let other_blocks: Vec<Block> = other_roots.iter().map(|&g| build_block(g)).collect();

    // pack into lanes, trying a few lane heights for the best aspect ratio
    let n_root = root_blocks.len();
    let blocks: Vec<&Block> = root_blocks.iter().chain(other_blocks.iter()).collect();
    let pack = |max_h: f32| -> (Placement, Vec2) {
        // (lane, x, y) of every block
        let mut placed = Vec::with_capacity(blocks.len());
        let (mut lane, mut x, mut y, mut lane_w, mut total_h) = (
            0usize,
            if n_root > 0 { BUS_GAP } else { 0.0 },
            0.0f32,
            0.0f32,
            0.0f32,
        );
        for (i, b) in blocks.iter().enumerate() {
            // other trees start in a fresh lane so they don't hang off a LEVEL bus
            let fresh = i == n_root && i > 0;
            if (y > 0.0 && y + b.size.y > max_h) || fresh {
                x += lane_w + LANE_GAP + if i < n_root { BUS_GAP } else { 0.0 };
                lane += 1;
                y = 0.0;
                lane_w = 0.0;
            }
            placed.push((lane, x, y));
            y += b.size.y + BLOCK_GAP;
            lane_w = lane_w.max(b.size.x);
            total_h = total_h.max(y);
        }
        (placed, vec2(x + lane_w, total_h))
    };
    let area: f32 = blocks
        .iter()
        .map(|b| (b.size.x + LANE_GAP) * (b.size.y + BLOCK_GAP))
        .sum();
    let tallest = blocks.iter().map(|b| b.size.y).fold(0.0, f32::max);
    let ideal = (area / ASPECT).sqrt().max(tallest);
    let mut best: Option<(f32, Placement)> = None;
    for k in 0..24 {
        let h = (ideal * 0.4 * 1.12f32.powi(k)).max(tallest);
        let (placed, dims) = pack(h);
        let score = ((dims.x.max(1.0) / dims.y.max(1.0)).ln() - ASPECT.ln()).abs();
        if best.as_ref().is_none_or(|(s, _)| score < *s) {
            best = Some((score, placed));
        }
    }
    let placed = best.map(|b| b.1).unwrap_or_default();

    let mut out = Layout {
        parent,
        timeline_y: -70.0,
        ..Default::default()
    };
    for (b, &(lane, x, y)) in blocks.iter().zip(&placed) {
        for &(g, off) in &b.offs {
            out.pos.insert(g, vec2(x, y) + off);
        }
        if out.parent.get(&b.root) == Some(&ROOT) {
            let root_y = out.pos[&b.root].y;
            match out.buses.get_mut(lane) {
                Some(bus) => bus.bottom = bus.bottom.max(root_y),
                None => out.buses.push(Bus {
                    x: x - BUS_GAP,
                    bottom: root_y,
                }),
            }
        }
    }
    if has_root {
        let first_bus = out.buses.first().map_or(0.0, |b| b.x);
        out.pos.insert(
            ROOT,
            vec2(first_bus - sz(ROOT).x / 2.0 - 30.0, out.timeline_y),
        );
    }
    out
}
