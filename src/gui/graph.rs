//! The Graph tab: draws the laid-out groups as flowchart nodes (or detailed cards), the LEVEL
//! timeline, routed edges with branch labels, badges and the legend, and handles pan / zoom /
//! drag / click / double-click.

use super::*;

impl App {
    pub(super) fn graph_view(&mut self, ui: &mut egui::Ui) -> Option<Nav> {
        let mut nav = None;
        ui.horizontal_wrapped(|ui| {
            mode_toggle(ui, &mut self.graph_detail);
            if self.graph_detail {
                ui.label(RichText::new("as").weak());
                listing_toggle(ui, &mut self.listing_mode);
            }
            ui.separator();
            ui.checkbox(&mut self.show_affect, "affect edges");
            ui.checkbox(&mut self.show_reference, "reference edges");
            ui.checkbox(&mut self.show_isolated, "groups without edges");
            let can_relax = self.nodes.len() <= MAX_RELAX_NODES;
            ui.add_enabled(
                can_relax,
                egui::Checkbox::new(&mut self.relax, "force layout"),
            )
            .on_disabled_hover_text(format!("too many nodes (> {MAX_RELAX_NODES})"));
            if ui.button("re-layout").clicked() {
                self.relax = false;
                self.layout_key = None;
            }
            if ui.button("fit").clicked() {
                self.g_fit = true;
            }
            ui.label(
                RichText::new("drag: pan / move node   scroll: zoom   double-click: show on map")
                    .weak(),
            );
        });
        self.ensure_layout();

        let visible = self.visible_nodes();
        let (resp, painter) = ui.allocate_painter(canvas_size(ui), Sense::click_and_drag());
        let rect = resp.rect;
        let prev_rect = std::mem::replace(&mut self.g_rect, rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(22));

        if self.g_fit || (self.g_autofit && rect != prev_rect) {
            self.g_fit = false;
            self.g_autofit = true;
            let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            for g in &visible {
                if let (Some(p), Some(h)) = (self.pos.get(g), self.half.get(g)) {
                    lo = lo.min(*p - *h);
                    hi = hi.max(*p + *h);
                }
            }
            if lo.x <= hi.x {
                // leave the legend's column free
                let legend = LEGEND_W.min(rect.width() * 0.3);
                let size = (hi - lo) + vec2(80.0, 80.0);
                self.g_zoom = ((rect.width() - legend) / size.x)
                    .min(rect.height() / size.y)
                    .clamp(0.02, 1.5);
                self.g_pan = -(lo + hi) / 2.0 * self.g_zoom + vec2(legend / 2.0, 0.0);
            }
        }

        if self.relax {
            self.structured = false;
            self.relax_step(&visible);
            ui.ctx().request_repaint();
        }

        let center = rect.center().to_vec2();
        // zoom around the pointer
        if resp.hovered() {
            let (scroll, zd) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (scroll * 0.002).exp() * zd;
            if (factor - 1.0).abs() > 1e-4
                && let Some(hp) = resp.hover_pos()
            {
                self.g_autofit = false;
                let world = (hp.to_vec2() - center - self.g_pan) / self.g_zoom;
                self.g_zoom = (self.g_zoom * factor).clamp(0.02, 6.0);
                self.g_pan = hp.to_vec2() - center - world * self.g_zoom;
            }
        }
        let hit = |app: &Self, pp: Pos2| -> Option<Gid> {
            let (pan, zoom) = (app.g_pan, app.g_zoom);
            visible.iter().rev().copied().find(|g| {
                let (Some(p), Some(h)) = (app.pos.get(g), app.half.get(g)) else {
                    return false;
                };
                Rect::from_center_size((center + pan + *p * zoom).to_pos2(), *h * 2.0 * zoom)
                    .contains(pp)
            })
        };

        if resp.drag_started()
            && let Some(pp) = resp.interact_pointer_pos()
        {
            self.g_drag = hit(self, pp);
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            match self.g_drag {
                Some(g) => {
                    let zoom = self.g_zoom;
                    if let Some(p) = self.pos.get_mut(&g) {
                        *p += d / zoom;
                    }
                }
                None => {
                    self.g_pan += d;
                    self.g_autofit = false;
                }
            }
        }
        if resp.drag_stopped() {
            self.g_drag = None;
        }
        if resp.clicked()
            && let Some(pp) = resp.interact_pointer_pos()
        {
            match hit(self, pp) {
                Some(g) => nav = Some(Nav::Group(g)),
                None => self.sel.group = None,
            }
        }
        if resp.double_clicked()
            && let Some(pp) = resp.interact_pointer_pos()
            && let Some(g) = hit(self, pp)
            && g != ROOT
        {
            self.tab = Tab::Map;
            self.navigate(Nav::Group(g), true);
        }

        let (pan, zoom) = (self.g_pan, self.g_zoom);
        let to_screen = |p: Vec2| (center + pan + p * zoom).to_pos2();
        let detail = self.graph_detail;
        let card = |g: Gid| detail && g != ROOT && self.node_lines.contains_key(&g);
        // where an edge leaves (`side` = 1, right) or enters (`side` = -1, left) a node
        let port = |g: Gid, side: f32, off: f32| -> Pos2 {
            let c = to_screen(self.pos[&g]);
            let h = self.half[&g] * zoom;
            let toward = c + vec2(side * 1e5, 0.0);
            let p = if card(g) {
                box_exit(c, toward, h)
            } else {
                shape_exit(self.flow.get(&g).copied(), c, toward, h)
            };
            p + vec2(0.0, off)
        };
        let sel = self.sel.group;
        let mut neighbours = BTreeSet::new();
        if let Some(s) = sel {
            for e in &self.m.edges {
                if self.edge_visible(e.kind) && (e.from == s || e.to == s) {
                    neighbours.insert(e.from);
                    neighbours.insert(e.to);
                }
            }
        }
        let clip = rect.expand(50.0);
        let lod = zoom < 0.3;
        let corridor = H_GAP * zoom;

        // the level timeline: LEVEL -> horizontal line -> one vertical bus per lane
        if self.structured && visible.contains(&ROOT) && !self.buses.is_empty() {
            let root_sel = sel == Some(ROOT);
            let mut col = Color32::from_gray(170);
            if sel.is_some() && !root_sel {
                col = col.gamma_multiply(0.25);
            }
            let ty = to_screen(vec2(0.0, self.timeline_y)).y;
            let start = port(ROOT, 1.0, 0.0);
            let mut lines = vec![vec![
                start,
                Pos2::new(to_screen(vec2(self.buses.last().unwrap().x, 0.0)).x, ty),
            ]];
            for b in &self.buses {
                let x = to_screen(vec2(b.x, 0.0)).x;
                lines.push(vec![
                    Pos2::new(x, ty),
                    Pos2::new(x, to_screen(vec2(0.0, b.bottom)).y),
                ]);
            }
            for l in lines {
                if root_sel {
                    painter.add(Shape::line(
                        l.clone(),
                        Stroke::new(10.0, col.gamma_multiply(0.18)),
                    ));
                    painter.add(Shape::line(
                        l.clone(),
                        Stroke::new(5.0, Color32::from_black_alpha(220)),
                    ));
                }
                painter.add(Shape::line(
                    l,
                    Stroke::new(if root_sel { 2.5 } else { 2.0 }, col),
                ));
            }
        }

        // edges: the ones touching the selection are drawn last (on top) with a glow
        // (anchor, alignment, text, colour)
        let mut edge_labels: Vec<(Pos2, Align2, String, Color32)> = Vec::new();
        for pass in [false, true] {
            for (ei, e) in self.m.edges.iter().enumerate() {
                if !self.edge_visible(e.kind)
                    || !visible.contains(&e.from)
                    || !visible.contains(&e.to)
                {
                    continue;
                }
                let incident = sel.is_some_and(|s| e.from == s || e.to == s);
                if incident != pass
                    || !self.pos.contains_key(&e.from)
                    || !self.pos.contains_key(&e.to)
                {
                    continue;
                }
                let mut color = kind_color(e.kind);
                if sel.is_some() && !incident {
                    color = color.gamma_multiply(0.18);
                }
                let width = if incident { 2.5 } else { 1.3 };
                let label = || {
                    let roles: BTreeSet<&str> = e
                        .actions
                        .iter()
                        .map(|&a| self.m.actions[a].role.as_str())
                        .collect();
                    let mut label = roles.into_iter().collect::<Vec<_>>().join(", ");
                    if label.len() > 40 {
                        label.truncate(37);
                        label.push_str("...");
                    }
                    label
                };
                if e.from == e.to {
                    let rad = 9.0 * zoom.max(0.5);
                    let c =
                        to_screen(self.pos[&e.from]) - vec2(0.0, self.half[&e.from].y * zoom + rad);
                    if !clip.contains(c) {
                        continue;
                    }
                    if incident {
                        painter.circle_stroke(
                            c,
                            rad,
                            Stroke::new(width + 8.0, color.gamma_multiply(0.18)),
                        );
                        painter.circle_stroke(
                            c,
                            rad,
                            Stroke::new(width + 3.0, Color32::from_black_alpha(220)),
                        );
                    }
                    painter.circle_stroke(c, rad, Stroke::new(width, color));
                    if incident && zoom > 0.45 {
                        edge_labels.push((
                            c - vec2(0.0, rad + 2.0),
                            Align2::CENTER_BOTTOM,
                            label(),
                            color,
                        ));
                    }
                    continue;
                }
                let off = self.edge_offset[ei] * zoom.max(0.3);
                // entries hang off the timeline bus to their left
                let bus = (e.from == ROOT
                    && self.structured
                    && self.tree_parent.get(&e.to) == Some(&ROOT))
                .then(|| self.buses.iter().rev().find(|b| b.x <= self.pos[&e.to].x))
                .flatten();
                let pts = match bus {
                    Some(b) => {
                        let end = port(e.to, -1.0, off);
                        vec![Pos2::new(to_screen(vec2(b.x, 0.0)).x, end.y), end]
                    }
                    None => route(port(e.from, 1.0, off), port(e.to, -1.0, off), corridor, off),
                };
                let bb = Rect::from_points(&pts);
                if !clip.intersects(bb) {
                    continue;
                }
                if lod && !incident {
                    painter.add(Shape::line(pts, Stroke::new(1.0, color)));
                } else {
                    // labels sit on the incoming segment, ending just before the arrow head
                    let end = pts[pts.len() - 1] - vec2((10.0 * zoom).clamp(5.0, 14.0) + 3.0, 2.0);
                    arrow_path(
                        &painter,
                        &pts,
                        color,
                        width,
                        (10.0 * zoom).clamp(5.0, 14.0),
                        incident,
                    );
                    // branches are always labelled so it's clear which way is which
                    let branch = e.actions.iter().any(|&a| {
                        FlowKind::of(self.m.objects[self.m.actions[a].trigger].id)
                            == FlowKind::Decision
                    });
                    if (incident || (branch && sel.is_none())) && zoom > 0.45 {
                        edge_labels.push((end, Align2::RIGHT_BOTTOM, label(), color));
                    }
                }
            }
        }

        // nodes: flowchart shapes (minimal) or cards listing what each trigger does (detailed)
        let font_size = (12.0 * zoom).clamp(6.0, 16.0);
        let mut node_mesh = egui::Mesh::default();
        let mut outlines = Vec::new();
        for &g in &visible {
            let (Some(p), Some(hw)) = (self.pos.get(&g), self.half.get(&g)) else {
                continue;
            };
            let c = to_screen(*p);
            let half = *hw * zoom;
            let r = Rect::from_center_size(c, half * 2.0);
            if !clip.intersects(r.expand(half.y)) {
                continue;
            }
            let info = &self.m.groups[&g];
            let kind = self.flow.get(&g).copied();
            let base = flow_color(kind);
            let dimmed = sel.is_some() && sel != Some(g) && !neighbours.contains(&g);
            let (fill, edge) = if dimmed {
                (shade(base, 0.12), shade(base, 0.35))
            } else {
                (shade(base, 0.3), base)
            };
            let stroke = if sel == Some(g) {
                Stroke::new(2.5, Color32::YELLOW)
            } else if neighbours.contains(&g) {
                Stroke::new(2.0, edge)
            } else {
                Stroke::new(1.2, edge)
            };
            let is_card = card(g);
            let (outline, deco) = if is_card {
                (
                    vec![
                        r.left_top(),
                        r.right_top(),
                        r.right_bottom(),
                        r.left_bottom(),
                    ],
                    vec![],
                )
            } else {
                node_shape(kind, c, half)
            };
            if lod {
                mesh_polygon(
                    &mut node_mesh,
                    &outline,
                    if dimmed { fill } else { shade(base, 0.6) },
                );
                if sel == Some(g) || neighbours.contains(&g) {
                    outlines.push((outline, stroke));
                }
                continue;
            }
            if sel == Some(g) {
                for (w, a) in [(16.0, 0.08), (9.0, 0.18), (4.5, 0.4)] {
                    painter.add(Shape::closed_line(
                        outline.clone(),
                        Stroke::new(w, Color32::YELLOW.gamma_multiply(a)),
                    ));
                }
            }
            let text_col = Color32::from_gray(if dimmed { 120 } else { 235 });
            if is_card {
                painter.rect(
                    r,
                    6.0 * zoom.min(1.0),
                    shade(base, if dimmed { 0.08 } else { 0.16 }),
                    stroke,
                );
                let hs = (12.0 * zoom).clamp(4.0, 16.0);
                let head_y = r.top() + HEADER_H / 2.0 * zoom;
                let (gl, gd) = node_shape(
                    kind,
                    Pos2::new(r.left() + 14.0 * zoom, head_y),
                    vec2(9.0, 4.5) * zoom,
                );
                painter.add(Shape::convex_polygon(gl, fill, Stroke::new(1.0, edge)));
                for d in gd {
                    painter.add(Shape::line(d, Stroke::new(1.0, edge)));
                }
                painter.line_segment(
                    [
                        Pos2::new(r.left(), r.top() + HEADER_H * zoom),
                        Pos2::new(r.right(), r.top() + HEADER_H * zoom),
                    ],
                    Stroke::new(1.0, edge.gamma_multiply(0.5)),
                );
                if 12.0 * zoom >= 4.5 {
                    painter.text(
                        Pos2::new(r.left() + 28.0 * zoom, head_y),
                        Align2::LEFT_CENTER,
                        format!(
                            "{}  {} obj  {} trg",
                            group_label(g),
                            info.members.len(),
                            info.triggers.len()
                        ),
                        FontId::monospace(hs),
                        text_col,
                    );
                }
                let bs = (DETAIL_FONT * zoom).clamp(4.0, 14.0);
                if DETAIL_FONT * zoom >= 4.5 {
                    let lines = &self.node_lines[&g];
                    for (i, (t, k, id)) in lines.iter().take(MAX_LINES).enumerate() {
                        let y = r.top() + (HEADER_H + 4.0 + (i as f32 + 0.5) * LINE_H) * zoom;
                        let col = trigger_shade(flow_color(Some(*k)), *id);
                        // trigger type tick: its shade and stroke width
                        let tick = 2.0 * trigger_variant(*id).2 * zoom.min(1.5);
                        painter.line_segment(
                            [
                                Pos2::new(r.left() + 5.0 * zoom, y - LINE_H * 0.35 * zoom),
                                Pos2::new(r.left() + 5.0 * zoom, y + LINE_H * 0.35 * zoom),
                            ],
                            Stroke::new(tick, if dimmed { shade(col, 0.5) } else { col }),
                        );
                        let galley =
                            painter.layout_job(highlighted(t, FontId::monospace(bs), col, dimmed));
                        painter.galley(
                            Pos2::new(r.left() + 10.0 * zoom, y - galley.size().y / 2.0),
                            galley,
                            col,
                        );
                    }
                    if lines.len() > MAX_LINES {
                        let y =
                            r.top() + (HEADER_H + 4.0 + (MAX_LINES as f32 + 0.5) * LINE_H) * zoom;
                        painter.text(
                            Pos2::new(r.left() + 10.0 * zoom, y),
                            Align2::LEFT_CENTER,
                            format!("+{} more", lines.len() - MAX_LINES),
                            FontId::proportional(bs),
                            shade(text_col, 0.7),
                        );
                    }
                }
            } else {
                painter.add(Shape::convex_polygon(outline, fill, stroke));
                for d in deco {
                    painter.add(Shape::line(d, stroke));
                }
                if zoom > 0.25 {
                    let (l1, l2) = if g == ROOT {
                        (
                            "LEVEL".to_string(),
                            format!("{} entries", self.m.entry_points.len()),
                        )
                    } else {
                        (
                            group_label(g),
                            format!("{} obj  {} trg", info.members.len(), info.triggers.len()),
                        )
                    };
                    let g1 = painter.layout_no_wrap(l1, FontId::monospace(font_size), text_col);
                    let g2 = painter.layout_no_wrap(
                        l2,
                        FontId::proportional(font_size * 0.8),
                        shade(text_col, 0.8),
                    );
                    let top = c.y - (g1.size().y + g2.size().y) / 2.0;
                    let r1 =
                        Rect::from_min_size(Pos2::new(c.x - g1.size().x / 2.0, top), g1.size());
                    let r2 = Rect::from_min_size(
                        Pos2::new(c.x - g2.size().x / 2.0, r1.bottom()),
                        g2.size(),
                    );
                    painter.rect_filled(
                        r1.union(r2).expand2(vec2(3.0, 0.5)),
                        3.0,
                        Color32::from_black_alpha(if dimmed { 90 } else { 160 }),
                    );
                    painter.galley(r1.min, g1, text_col);
                    painter.galley(r2.min, g2, text_col);
                }
            }
            if zoom > 0.25 {
                // badges: warning, spawn loop
                let br = (6.0 * zoom).clamp(3.5, 8.0);
                let mut bp = r.right_top() + vec2(-br, 0.0);
                if self.warn_groups.contains(&g) {
                    painter.circle(bp, br, WARN_COLOR, Stroke::new(1.0, Color32::BLACK));
                    painter.text(
                        bp,
                        Align2::CENTER_CENTER,
                        "!",
                        FontId::proportional(br * 1.7),
                        Color32::BLACK,
                    );
                    bp.x -= br * 2.4;
                }
                if self.cycle_groups.contains(&g) {
                    painter.circle(bp, br, CYCLE_COLOR, Stroke::new(1.0, Color32::BLACK));
                }
            }
        }

        painter.add(Shape::mesh(node_mesh));
        for (o, stroke) in outlines {
            painter.add(Shape::closed_line(o, stroke));
        }
        // labels of the selection's edges, on top of everything so nodes can't hide them
        let mut placer = LabelPlacer::default();
        for (p, align, text, color) in edge_labels {
            let galley = painter.layout_no_wrap(text, FontId::monospace(11.0), color);
            let r = align.anchor_size(p, galley.size());
            if placer.try_place(r.expand2(vec2(3.0, 1.0))) || sel.is_some() {
                plate_galley(&painter, r, galley, color, Some(color));
            }
        }

        self.graph_legend(&painter, rect);

        // hover tooltip
        if let Some(hp) = resp.hover_pos()
            && let Some(g) = hit(self, hp)
        {
            let lines = self.group_tooltip(g);
            resp.on_hover_ui_at_pointer(|ui| {
                for (i, l) in lines.iter().enumerate() {
                    if i == 0 {
                        ui.label(RichText::new(l).strong());
                    } else {
                        ui.label(RichText::new(l).monospace().size(11.0));
                    }
                }
            });
        }
        nav
    }

    pub(super) fn graph_legend(&self, painter: &egui::Painter, rect: Rect) {
        let edge_kinds: Vec<EdgeKind> = {
            let mut seen = BTreeSet::new();
            EdgeKind::ALL
                .into_iter()
                .filter(|k| self.edge_visible(*k) && seen.insert(k.rgb()))
                .collect()
        };
        let row = 16.0;
        let rows = edge_kinds.len() + FlowKind::ALL.len() + 5;
        let panel = Rect::from_min_size(
            rect.left_top() + vec2(6.0, 6.0),
            vec2(LEGEND_W - 12.0, rows as f32 * row + 8.0),
        );
        painter.rect_filled(panel, 4.0, PLATE_BG);
        painter.rect_stroke(panel, 4.0, Stroke::new(1.0, Color32::from_white_alpha(25)));
        let (x0, mut y) = (panel.left() + 8.0, panel.top() + 4.0);
        let label_col = Color32::from_gray(210);
        let text_at = |y: f32, s: &str| {
            painter.text(
                Pos2::new(x0 + 30.0, y + row / 2.0),
                Align2::LEFT_CENTER,
                s,
                FontId::proportional(12.0),
                label_col,
            );
        };
        for k in edge_kinds {
            let label = match k {
                EdgeKind::ToggleOn => "toggle on / toggle",
                EdgeKind::Stop => "stop / pause / resume / reset",
                other => other.name(),
            };
            painter.line_segment(
                [
                    Pos2::new(x0, y + row / 2.0),
                    Pos2::new(x0 + 22.0, y + row / 2.0),
                ],
                Stroke::new(3.0, kind_color(k)),
            );
            text_at(y, label);
            y += row;
        }
        painter.line_segment(
            [
                Pos2::new(x0, y + row / 2.0),
                Pos2::new(x0 + 22.0, y + row / 2.0),
            ],
            Stroke::new(2.0, Color32::from_gray(170)),
        );
        text_at(y, "level timeline (in x order)");
        y += row * 1.5;
        let shapes = FlowKind::ALL
            .iter()
            .map(|k| (Some(*k), k.name()))
            .chain([(None, "no logic (objects only)")]);
        for (k, name) in shapes {
            let c = flow_color(k);
            let (o, deco) = node_shape(k, Pos2::new(x0 + 11.0, y + row / 2.0), vec2(10.0, 4.5));
            painter.add(Shape::convex_polygon(o, shade(c, 0.3), Stroke::new(1.2, c)));
            for d in deco {
                painter.add(Shape::line(d, Stroke::new(1.2, c)));
            }
            text_at(y, name);
            y += row;
        }
        let badge = Pos2::new(x0 + 11.0, y + row / 2.0);
        painter.circle(badge, 5.0, WARN_COLOR, Stroke::new(1.0, Color32::BLACK));
        painter.text(
            badge,
            Align2::CENTER_CENTER,
            "!",
            FontId::proportional(9.0),
            Color32::BLACK,
        );
        text_at(y, "has warnings");
        y += row;
        painter.circle(
            Pos2::new(x0 + 11.0, y + row / 2.0),
            5.0,
            CYCLE_COLOR,
            Stroke::new(1.0, Color32::BLACK),
        );
        text_at(y, "part of a spawn loop");
    }

    pub(super) fn group_tooltip(&self, g: Gid) -> Vec<String> {
        let info = &self.m.groups[&g];
        if g == ROOT {
            return vec![
                "LEVEL: triggers run by player position / touch / contact".into(),
                format!("{} entry point(s)", self.m.entry_points.len()),
            ];
        }
        let role = self.flow.get(&g).map_or("no logic", |k| k.name());
        let mut lines = vec![format!(
            "{}: {} object(s), {} logic  [{role}]",
            group_label(g),
            info.members.len(),
            info.triggers.len()
        )];
        for &t in info.triggers.iter().take(10) {
            lines.push(format!("#{t} {}", self.m.describe(t)));
        }
        if info.triggers.len() > 10 {
            lines.push(format!("... +{} more", info.triggers.len() - 10));
        }
        lines
    }
}
