//! What the graph shows and where: visible nodes and edges, detailed card contents, node sizes,
//! (re)computing the layout when a toggle changes, and the optional force-directed relaxation.

use super::*;

/// Spread parallel edges between the same pair of groups so they don't overlap.
pub(super) fn edge_offsets(m: &Model) -> Vec<f32> {
    let mut pairs: HashMap<(Gid, Gid), Vec<usize>> = HashMap::new();
    for (i, e) in m.edges.iter().enumerate() {
        pairs
            .entry((e.from.min(e.to), e.from.max(e.to)))
            .or_default()
            .push(i);
    }
    let mut out = vec![0.0; m.edges.len()];
    for list in pairs.values() {
        let n = list.len() as f32;
        for (k, &ei) in list.iter().enumerate() {
            let off = (k as f32 - (n - 1.0) / 2.0) * 10.0;
            // keep the offset on a consistent side relative to the canonical (min -> max) direction
            out[ei] = if m.edges[ei].from <= m.edges[ei].to {
                off
            } else {
                -off
            };
        }
    }
    out
}

/// Body lines of every detailed graph card: one per trigger (left to right), with its flow kind
/// and object id for colouring. TASM lines get `~` when they run on the same tick as the previous.
pub(super) fn card_lines(
    m: &Model,
    names: &Names,
    mode: Mode,
) -> HashMap<Gid, Vec<(String, FlowKind, i32)>> {
    let mut out = HashMap::new();
    for info in m.real_groups().filter(|g| !g.triggers.is_empty()) {
        let mut trig = info.triggers.clone();
        trig.sort_by(|a, b| {
            m.objects[*a]
                .config
                .pos
                .0
                .total_cmp(&m.objects[*b].config.pos.0)
        });
        let mut prev_x = None;
        let lines = trig
            .iter()
            .map(|&t| {
                let x = m.objects[t].config.pos.0;
                let same = mode == Mode::Tasm && prev_x == Some(x);
                prev_x = Some(x);
                let mut s = format!(
                    "{}{}",
                    if same { "~" } else { "" },
                    listing::line(m, t, mode, names, false)
                );
                if s.chars().count() > MAX_CHARS {
                    s = s.chars().take(MAX_CHARS - 3).collect::<String>() + "...";
                }
                (s, FlowKind::of(m.objects[t].id), m.objects[t].id)
            })
            .collect();
        out.insert(info.id, lines);
    }
    out
}

impl App {
    pub(super) fn edge_visible(&self, k: EdgeKind) -> bool {
        match k {
            EdgeKind::Affect => self.show_affect,
            EdgeKind::Reference => self.show_reference,
            _ => true,
        }
    }

    pub(super) fn visible_nodes(&self) -> BTreeSet<Gid> {
        let mut vis = BTreeSet::from([ROOT]);
        for e in &self.m.edges {
            if self.edge_visible(e.kind) {
                vis.insert(e.from);
                vis.insert(e.to);
            }
        }
        if self.show_isolated {
            vis.extend(self.nodes.iter().copied());
        }
        vis
    }

    /// World size of a graph node in the current mode.
    pub(super) fn node_size(&self, g: Gid) -> Vec2 {
        match self.node_lines.get(&g) {
            Some(lines) if self.graph_detail && g != ROOT => {
                let header = group_label(g).len() + 22;
                let chars = lines
                    .iter()
                    .take(MAX_LINES)
                    .map(|(t, ..)| t.chars().count())
                    .max()
                    .unwrap_or(0)
                    .max(header);
                let n = lines.len().min(MAX_LINES) + (lines.len() > MAX_LINES) as usize;
                vec2(
                    (chars as f32 * CHAR_W + 20.0).max(NODE_W),
                    HEADER_H + 8.0 + n as f32 * LINE_H,
                )
            }
            _ => vec2(NODE_W, NODE_H),
        }
    }

    /// Recomputes the graph layout when a toggle that affects it changed.
    pub(super) fn ensure_layout(&mut self) {
        let key = (
            self.show_affect,
            self.show_reference,
            self.show_isolated,
            self.graph_detail,
            self.listing_mode,
        );
        if self.layout_key == Some(key) {
            return;
        }
        if self.layout_key.is_none_or(|k| k.4 != key.4) {
            self.node_lines = card_lines(&self.m, &self.names, self.listing_mode);
        }
        let first = self.layout_key.is_none() && self.pos.is_empty();
        self.layout_key = Some(key);
        let visible = self.visible_nodes();
        let size: HashMap<Gid, Vec2> = visible.iter().map(|&g| (g, self.node_size(g))).collect();
        let l = layout::layout(&self.m, &visible, |e| self.edge_visible(e.kind), &size);
        self.half = size.into_iter().map(|(g, s)| (g, s / 2.0)).collect();
        self.pos = l.pos;
        self.tree_parent = l.parent;
        self.buses = l.buses;
        self.timeline_y = l.timeline_y;
        self.structured = true;
        // keep the selection in view across mode switches, otherwise show everything
        match self
            .sel
            .group
            .filter(|g| !first && self.pos.contains_key(g))
        {
            Some(g) => {
                self.g_pan = -self.pos[&g] * self.g_zoom;
                self.g_autofit = false;
            }
            None => self.g_fit = true,
        }
    }

    pub(super) fn relax_step(&mut self, visible: &BTreeSet<Gid>) {
        let ids: Vec<Gid> = visible
            .iter()
            .copied()
            .filter(|g| self.pos.contains_key(g))
            .collect();
        if ids.len() > MAX_RELAX_NODES {
            self.relax = false;
            return;
        }
        let idx: HashMap<Gid, usize> = ids.iter().enumerate().map(|(i, g)| (*g, i)).collect();
        let p: Vec<Vec2> = ids.iter().map(|g| self.pos[g]).collect();
        let mut disp = vec![Vec2::ZERO; ids.len()];
        let k = 170.0f32;
        for i in 0..p.len() {
            for j in (i + 1)..p.len() {
                let d = p[i] - p[j];
                let dist = d.length().max(1.0);
                let f = (k * k / dist).min(600.0);
                let v = d / dist * f;
                disp[i] += v;
                disp[j] -= v;
            }
        }
        for e in &self.m.edges {
            if !self.edge_visible(e.kind) || e.from == e.to {
                continue;
            }
            let (Some(&a), Some(&b)) = (idx.get(&e.from), idx.get(&e.to)) else {
                continue;
            };
            let d = p[b] - p[a];
            let dist = d.length().max(1.0);
            let v = d / dist * (dist * dist / k);
            disp[a] += v;
            disp[b] -= v;
        }
        for (i, g) in ids.iter().enumerate() {
            if Some(*g) == self.g_drag {
                continue;
            }
            let d = disp[i];
            let len = d.length();
            if len > 0.01 {
                *self.pos.get_mut(g).unwrap() += d / len * len.min(6.0);
            }
        }
    }
}
