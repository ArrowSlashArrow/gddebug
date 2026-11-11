//! The Listing tab (the disassembly as clickable, highlighted rows, in raw / details / tasm mode)
//! and the Items tab (who reads and writes each counter, timer and collision ID).

use super::*;

pub(super) fn build_listing(
    m: &Model,
    mode: Mode,
    names: &Names,
) -> (Vec<Row>, HashMap<Gid, usize>) {
    let mut rows = Vec::new();
    let mut group_row = HashMap::new();
    let comment = Color32::from_gray(130);
    let header = Color32::from_rgb(230, 200, 90);
    let tasm = mode == Mode::Tasm;

    rows.push(Row {
        text: "; ======== ENTRY POINTS (run by the level, sorted by x) ========".into(),
        color: comment,
        obj: None,
        group: None,
    });
    group_row.insert(ROOT, 0);
    if tasm {
        rows.push(Row {
            text: "_init:".into(),
            color: header,
            obj: None,
            group: Some(ROOT),
        });
    }
    for &i in &m.entry_points {
        rows.push(Row {
            text: format!("  {}", report::obj_line(m, i, mode, names, false)),
            color: act_color(m.meta[i].activation),
            obj: Some(i),
            group: None,
        });
    }
    rows.push(Row {
        text: String::new(),
        color: comment,
        obj: None,
        group: None,
    });
    rows.push(Row {
        text: "; ======== GROUPS ========".into(),
        color: comment,
        obj: None,
        group: None,
    });
    for info in m.real_groups().filter(|g| g.has_logic()) {
        let g = info.id;
        rows.push(Row {
            text: String::new(),
            color: comment,
            obj: None,
            group: None,
        });
        group_row.insert(g, rows.len());
        rows.push(Row {
            text: format!(
                "{}:   ; {} object(s), {} logic",
                if tasm { names.label(g) } else { group_label(g) },
                info.members.len(),
                info.triggers.len()
            ),
            color: header,
            obj: None,
            group: Some(g),
        });
        for &ai in &info.incoming {
            let a = &m.actions[ai];
            let src: Vec<String> = m
                .trigger_sources(a.trigger)
                .into_iter()
                .map(group_label)
                .collect();
            rows.push(Row {
                text: format!(
                    "  ; <- {} via #{} {} ({})",
                    src.join("|"),
                    a.trigger,
                    m.meta[a.trigger].mnemonic,
                    a.role
                ),
                color: kind_color(a.kind),
                obj: Some(a.trigger),
                group: None,
            });
        }
        let deco: Vec<usize> = info
            .members
            .iter()
            .copied()
            .filter(|&i| !m.meta[i].logic)
            .collect();
        if !deco.is_empty() {
            rows.push(Row {
                text: format!("  ; objects: {}", m.content_summary(&deco, 6)),
                color: comment,
                obj: None,
                group: Some(g),
            });
        }
        for (t, line) in report::group_lines(m, &info.triggers, mode, names) {
            rows.push(Row {
                text: format!("  {line}"),
                color: act_color(m.meta[t].activation),
                obj: Some(t),
                group: None,
            });
        }
    }
    (rows, group_row)
}

impl App {
    pub(super) fn listing_view(&mut self, ui: &mut egui::Ui) -> Option<Nav> {
        let mut nav = None;
        ui.horizontal_wrapped(|ui| {
            listing_toggle(ui, &mut self.listing_mode);
        });
        if self.rows_mode != self.listing_mode {
            self.rows_mode = self.listing_mode;
            (self.rows, self.group_row) = build_listing(&self.m, self.listing_mode, &self.names);
            // stay on the selection
            self.listing_jump = match (self.sel.obj, self.sel.group) {
                (Some(i), _) => self.rows.iter().position(|r| r.obj == Some(i)),
                (None, Some(g)) => self.group_row.get(&g).copied(),
                _ => None,
            };
        }
        let needle = self.filter.trim().to_lowercase();
        let rows: Vec<usize> = if needle.is_empty() {
            (0..self.rows.len()).collect()
        } else {
            (0..self.rows.len())
                .filter(|&i| self.rows[i].text.to_lowercase().contains(&needle))
                .collect()
        };
        ui.label(
            RichText::new(format!(
                "{} line(s){}",
                rows.len(),
                if needle.is_empty() {
                    ""
                } else {
                    " matching the filter"
                }
            ))
            .weak(),
        );
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
        let mut area = egui::ScrollArea::both().auto_shrink([false, false]);
        if let Some(target) = self.listing_jump.take()
            && let Some(pos) = rows.iter().position(|&r| r == target)
        {
            area = area.vertical_scroll_offset((pos as f32 * row_h - 60.0).max(0.0));
        }
        area.show_rows(ui, row_h, rows.len(), |ui, range| {
            for &ri in &rows[range] {
                let row = &self.rows[ri];
                let selected = (row.obj.is_some() && row.obj == self.sel.obj)
                    || (row.group.is_some() && row.obj.is_none() && row.group == self.sel.group);
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                let text = highlighted(&row.text, font, row.color, false);
                if row.obj.is_none() && row.group.is_none() {
                    ui.label(text);
                } else if ui.selectable_label(selected, text).clicked() {
                    nav = row.obj.map(Nav::Obj).or(row.group.map(Nav::Group));
                }
            }
        });
        nav
    }

    pub(super) fn items_view(&mut self, ui: &mut egui::Ui) -> Option<Nav> {
        let mut nav = None;
        if self.m.items.is_empty() {
            ui.label("This level does not use any counters, timers or collision IDs.");
            return None;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("items")
                    .striped(true)
                    .num_columns(4)
                    .show(ui, |ui| {
                        ui.strong("item");
                        ui.strong("defined by");
                        ui.strong("written by");
                        ui.strong("read by");
                        ui.end_row();
                        for (key, refs) in &self.m.items {
                            if ui
                                .selectable_label(
                                    self.sel.item == Some(*key),
                                    RichText::new(item_label(*key)).monospace(),
                                )
                                .clicked()
                            {
                                nav = Some(Nav::Item(*key));
                            }
                            for acc in [Access::Define, Access::Write, Access::Read] {
                                ui.horizontal_wrapped(|ui| {
                                    for r in refs.iter().filter(|r| r.access == acc).take(30) {
                                        if ui
                                            .link(format!(
                                                "#{} {}",
                                                r.obj, self.m.meta[r.obj].mnemonic
                                            ))
                                            .clicked()
                                        {
                                            nav = Some(Nav::Obj(r.obj));
                                        }
                                    }
                                    let n = refs.iter().filter(|r| r.access == acc).count();
                                    if n > 30 {
                                        ui.label(format!("+{}", n - 30));
                                    }
                                });
                            }
                            ui.end_row();
                        }
                    });
            });
        nav
    }
}
