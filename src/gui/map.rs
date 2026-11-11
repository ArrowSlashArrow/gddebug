//! The Map tab: the level itself. Triggers get a monochrome fill and an outline in their
//! activation colour, text objects show their text, labels never overlap, and the selection draws
//! arrows to and from the groups it touches.

use super::*;

impl App {
    pub(super) fn map_view(&mut self, ui: &mut egui::Ui) -> Option<Nav> {
        let mut nav = None;
        ui.horizontal_wrapped(|ui| {
            mode_toggle(ui, &mut self.map_detail);
            ui.separator();
            ui.checkbox(&mut self.map_logic_only, "logic objects only");
            ui.checkbox(&mut self.map_labels, "labels");
            if ui.button("fit level").clicked() {
                self.m_fit = true;
            }
            ui.label(RichText::new("■ trigger").color(TRIGGER_FILL));
            ui.label(RichText::new("outline:").weak());
            for (a, name) in [
                (Activation::Spawn, "spawn"),
                (Activation::Position, "x-pos"),
                (Activation::Touch, "touch"),
                (Activation::Player, "player"),
                (Activation::None, "passive logic"),
            ] {
                ui.label(RichText::new(format!("■ {name}")).color(act_color(a)));
            }
            ui.label(RichText::new("■ selected group").color(Color32::YELLOW));
        });

        let (resp, painter) = ui.allocate_painter(canvas_size(ui), Sense::click_and_drag());
        let rect = resp.rect;
        let prev_rect = std::mem::replace(&mut self.m_rect, rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(18));
        let center = rect.center().to_vec2();

        if self.m_fit || (self.m_autofit && rect != prev_rect) {
            self.m_fit = false;
            self.m_autofit = true;
            let (x0, y0, x1, y1) = self.m.bounds;
            let size = vec2((x1 - x0).max(300.0) + 120.0, (y1 - y0).max(300.0) + 120.0);
            self.m_zoom = (rect.width() / size.x)
                .min(rect.height() / size.y)
                .clamp(0.002, 4.0);
            self.m_pan = -vec2((x0 + x1) / 2.0, -(y0 + y1) / 2.0) * self.m_zoom;
        }
        if resp.hovered() {
            let (scroll, zd) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (scroll * 0.002).exp() * zd;
            if (factor - 1.0).abs() > 1e-4
                && let Some(hp) = resp.hover_pos()
            {
                self.m_autofit = false;
                let world = (hp.to_vec2() - center - self.m_pan) / self.m_zoom;
                self.m_zoom = (self.m_zoom * factor).clamp(0.002, 20.0);
                self.m_pan = hp.to_vec2() - center - world * self.m_zoom;
            }
        }
        if resp.dragged() {
            self.m_pan += resp.drag_delta();
            self.m_autofit = false;
        }
        let (pan, zoom) = (self.m_pan, self.m_zoom);
        let to_screen = |x: f32, y: f32| (center + pan + vec2(x, -y) * zoom).to_pos2();

        // ground line at y = 0 and the start line at x = 0
        painter.line_segment(
            [
                Pos2::new(rect.left(), to_screen(0.0, 0.0).y),
                Pos2::new(rect.right(), to_screen(0.0, 0.0).y),
            ],
            Stroke::new(1.0, Color32::from_gray(60)),
        );
        painter.line_segment(
            [
                Pos2::new(to_screen(0.0, 0.0).x, rect.top()),
                Pos2::new(to_screen(0.0, 0.0).x, rect.bottom()),
            ],
            Stroke::new(1.0, Color32::from_gray(45)),
        );

        // 0 = normal, 1 = in selected group, 2 = uses selected item
        let mut highlight = vec![
            0u8;
            if self.sel.group.is_some() || self.sel.item.is_some() {
                self.m.objects.len()
            } else {
                0
            }
        ];
        if let Some(info) = self.sel.group.and_then(|g| self.m.groups.get(&g)) {
            for &i in &info.members {
                highlight[i] = 1;
            }
        }
        if let Some(refs) = self.sel.item.and_then(|k| self.m.items.get(&k)) {
            for r in refs {
                highlight[r.obj] = 2;
            }
        }

        // level of detail: when objects are only a few pixels big, many land on the same pixel.
        // keep one quad per 2px cell, letting more important objects (logic, highlighted) win.
        let base = (30.0 * zoom).max(2.0);
        let view = rect.expand(base);
        let lod = 30.0 * zoom < 3.0;
        let (gw, gh) = (
            (view.width() / 2.0) as usize + 1,
            (view.height() / 2.0) as usize + 1,
        );
        let mut occupied = if lod { vec![0u8; gw * gh] } else { Vec::new() };

        let mut mesh = egui::Mesh::default();
        let mut labels = Vec::new();
        let mut texts: Vec<(Pos2, &str, f32, Color32, f32)> = Vec::new();
        // triggers: monochrome body, outlined in their activation colour
        let outline_w = (2.5 * zoom.sqrt()).clamp(1.0, 3.0);
        for (i, o) in self.m.objects.iter().enumerate() {
            let meta = &self.m.meta[i];
            let hl = highlight.get(i).copied().unwrap_or(0);
            if self.map_logic_only && !meta.logic && hl == 0 {
                continue;
            }
            let sp = to_screen(o.config.pos.0, o.config.pos.1);
            if !view.contains(sp) {
                continue;
            }
            if lod {
                let priority = if hl != 0 {
                    3
                } else if meta.logic {
                    2
                } else {
                    1
                };
                let cell = ((sp.y - view.top()) / 2.0) as usize * gw
                    + ((sp.x - view.left()) / 2.0) as usize;
                if let Some(c) = occupied.get_mut(cell) {
                    if *c >= priority {
                        continue;
                    }
                    *c = priority;
                }
            }
            let (sx, sy) = o.config.scale;
            let size = vec2(
                (30.0 * sx.abs().max(0.1) * zoom).max(2.0),
                (30.0 * sy.abs().max(0.1) * zoom).max(2.0),
            );
            let r = Rect::from_center_size(sp, size);
            let color = if hl == 1 {
                Color32::YELLOW
            } else if hl == 2 {
                Color32::from_rgb(0, 230, 230)
            } else if meta.logic {
                TRIGGER_FILL
            } else if self.sel.group.is_some() {
                Color32::from_gray(45)
            } else {
                Color32::from_gray(85)
            };
            // text objects: draw the text itself once it is big enough to read
            if let Some(text) = self.texts.get(&i) {
                let font = 26.0 * sy.abs().max(0.1) * zoom;
                if font >= 5.0 {
                    let col = match hl {
                        1 => Color32::YELLOW,
                        2 => Color32::from_rgb(0, 230, 230),
                        _ => Color32::from_gray(if self.sel.group.is_some() { 120 } else { 235 }),
                    };
                    texts.push((
                        sp,
                        text.as_str(),
                        font.min(200.0),
                        col,
                        o.config.angle.to_radians(),
                    ));
                    continue;
                }
            }
            if meta.logic {
                let ac = trigger_shade(act_color(meta.activation), o.id);
                let ow = outline_w * trigger_variant(o.id).2;
                mesh.add_colored_rect(r.expand(ow), ac);
                mesh.add_colored_rect(r, color);
                if self.map_labels && zoom > if self.map_detail { 0.4 } else { 0.55 } {
                    let text = if self.map_detail {
                        self.summaries
                            .get(&i)
                            .cloned()
                            .unwrap_or_else(|| meta.mnemonic.clone())
                    } else {
                        meta.mnemonic.clone()
                    };
                    let priority = if self.sel.obj == Some(i) {
                        3
                    } else {
                        hl.min(1) * 2
                    };
                    labels.push((sp - vec2(0.0, size.y / 2.0 + ow + 2.0), text, ac, priority));
                }
            } else {
                mesh.add_colored_rect(r, color);
            }
        }
        painter.add(Shape::mesh(mesh));
        for (p, text, size, color, angle) in texts {
            halo_text(&painter, p, text, size, color, angle);
        }
        // labels never overlap: the selection's labels are placed first, the rest only where free
        labels.sort_by_key(|l| std::cmp::Reverse(l.3));
        let mut placed = LabelPlacer::default();
        let font = FontId::monospace(if self.map_detail { 11.0 } else { 10.0 });
        for (p, text, color, priority) in labels {
            let galley = painter.layout_no_wrap(text, font.clone(), color);
            let r = Align2::CENTER_BOTTOM.anchor_size(p, galley.size());
            if placed.try_place(r.expand2(vec2(3.0, 1.0))) || priority >= 3 {
                plate_galley(&painter, r, galley, color, (priority >= 2).then_some(color));
            }
        }

        // arrows touching the selection: from the selected trigger to its targets, from the triggers
        // that target the selected object's groups to it, and from callers into the selected group
        let pos_of = |i: usize| {
            to_screen(
                self.m.objects[i].config.pos.0,
                self.m.objects[i].config.pos.1,
            )
        };
        let centroid = |g: Gid| {
            self.m
                .groups
                .get(&g)
                .and_then(|i| i.centroid)
                .map(|(x, y)| to_screen(x, y))
        };
        let mut arrows: Vec<(Pos2, Pos2, EdgeKind, String)> = Vec::new();
        if let Some(o) = self.sel.obj {
            for &ai in &self.m.meta[o].actions {
                let a = &self.m.actions[ai];
                if let Some(b) = centroid(a.target) {
                    arrows.push((
                        pos_of(o),
                        b,
                        a.kind,
                        format!("{} {}", a.role, group_label(a.target)),
                    ));
                }
            }
            let mut incoming = 0;
            for &g in &self.m.objects[o].config.groups {
                let Some(info) = self.m.groups.get(&g) else {
                    continue;
                };
                for &ai in &info.incoming {
                    let a = &self.m.actions[ai];
                    if a.trigger != o && incoming < 300 {
                        incoming += 1;
                        arrows.push((
                            pos_of(a.trigger),
                            pos_of(o),
                            a.kind,
                            format!("{} {}", a.role, group_label(g)),
                        ));
                    }
                }
            }
        }
        if let Some(g) = self.sel.group
            && let Some(b) = centroid(g)
        {
            for &ai in &self.m.groups[&g].incoming {
                let a = &self.m.actions[ai];
                if Some(a.trigger) != self.sel.obj {
                    arrows.push((
                        pos_of(a.trigger),
                        b,
                        a.kind,
                        format!("{} {}", a.role, group_label(g)),
                    ));
                }
            }
        }
        for (a, b, kind, _) in &arrows {
            arrow(&painter, *a, *b, kind_color(*kind), 2.0, 12.0, true);
        }
        // selected object: glow ring on top of the arrows that start or end at it
        if let Some(o) = self.sel.obj {
            let sp = pos_of(o);
            let (sx, sy) = self.m.objects[o].config.scale;
            let r = Rect::from_center_size(
                sp,
                vec2(
                    (30.0 * sx.abs().max(0.1) * zoom).max(6.0),
                    (30.0 * sy.abs().max(0.1) * zoom).max(6.0),
                ),
            )
            .expand(4.0);
            for (w, a) in [(14.0, 0.08), (8.0, 0.18), (4.0, 0.4)] {
                painter.rect_stroke(r, 3.0, Stroke::new(w, Color32::WHITE.gamma_multiply(a)));
            }
            painter.rect_stroke(r, 3.0, Stroke::new(1.5, Color32::WHITE));
        }
        for (a, b, kind, label) in arrows {
            let c = kind_color(kind);
            plate_text(
                &painter,
                a.lerp(b, 0.5),
                Align2::CENTER_BOTTOM,
                label,
                FontId::monospace(11.0),
                c,
                Some(c),
            );
        }

        // picking
        let pick = |pp: Pos2| -> Option<usize> {
            let mut best: Option<(usize, f32)> = None;
            for (i, o) in self.m.objects.iter().enumerate() {
                if self.map_logic_only && !self.m.meta[i].logic {
                    continue;
                }
                let d = to_screen(o.config.pos.0, o.config.pos.1).distance(pp);
                let r = (15.0 * zoom).max(6.0);
                // prefer logic objects when stacked
                let d = if self.m.meta[i].logic { d - 2.0 } else { d };
                if d <= r && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((i, d));
                }
            }
            best.map(|b| b.0)
        };
        if resp.clicked()
            && let Some(pp) = resp.interact_pointer_pos()
        {
            match pick(pp) {
                Some(i) => nav = Some(Nav::Obj(i)),
                None => self.sel.obj = None,
            }
        }
        if let Some(hp) = resp.hover_pos()
            && let Some(i) = pick(hp)
        {
            let o = &self.m.objects[i];
            let mut text = format!(
                "#{i} {} (id {})  at ({:.0}, {:.0})",
                self.m.meta[i].name, o.id, o.config.pos.0, o.config.pos.1
            );
            if let Some(s) = self.summaries.get(&i) {
                text.push('\n');
                text.push_str(s);
            }
            if let Some(t) = self.texts.get(&i) {
                text.push_str(&format!("\ntext: {t:?}"));
            }
            if !o.config.groups.is_empty() {
                let gs: Vec<String> = o.config.groups.iter().map(|g| group_label(*g)).collect();
                text.push_str(&format!("\ngroups: {}", gs.join(" ")));
            }
            resp.on_hover_text_at_pointer(text);
        }
        nav
    }
}
