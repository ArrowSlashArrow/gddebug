//! Side panels: the group / entry point / diagnostics browser on the left and the inspector on the
//! right (decoded trigger, its targets and callers, raw properties).

use super::*;

impl App {
    pub(super) fn left_panel(&mut self, ui: &mut egui::Ui) -> Option<Nav> {
        let mut nav = None;
        let m = &self.m;
        ui.heading(&m.level_name);
        ui.label(RichText::new(format!("by {}", m.creator)).weak());
        ui.label(format!(
            "{} objects · {} logic · {} groups · {} edges",
            m.objects.len(),
            m.trigger_count(),
            m.real_groups().filter(|g| !g.members.is_empty()).count(),
            m.edges.len()
        ));
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("filter");
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("group #, #obj, or text")
                    .desired_width(f32::INFINITY),
            );
        });
        ui.checkbox(&mut self.only_logic, "only groups with logic");

        let m = &self.m;
        let f = self.filter.trim();
        if let Some(idx) = f.strip_prefix('#').and_then(|s| s.parse::<usize>().ok())
            && idx < m.objects.len()
            && ui
                .button(format!("go to object #{idx} ({})", m.meta[idx].name))
                .clicked()
        {
            nav = Some(Nav::Obj(idx));
        }

        let numeric = f.chars().all(|c| c.is_ascii_digit())
            || f.strip_prefix('g')
                .is_some_and(|r| r.chars().all(|c| c.is_ascii_digit()));
        let digits = f.trim_start_matches('g');
        let list: Vec<Gid> = m
            .real_groups()
            .filter(|g| !self.only_logic || g.has_logic())
            .filter(|g| !numeric || digits.is_empty() || g.id.to_string().starts_with(digits))
            .map(|g| g.id)
            .collect();

        egui::CollapsingHeader::new(format!("Groups ({})", list.len()))
            .default_open(true)
            .show(ui, |ui| {
                let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
                egui::ScrollArea::vertical()
                    .id_salt("groups")
                    .max_height(320.0)
                    .show_rows(ui, row_h, list.len(), |ui, range| {
                        for &g in &list[range] {
                            let info = &m.groups[&g];
                            let mut text = RichText::new(format!(
                                "{:<6} {:>5} obj {:>4} trg  in {:<3} out {}",
                                group_label(g),
                                info.members.len(),
                                info.triggers.len(),
                                info.in_edges.len(),
                                info.out_edges.len()
                            ))
                            .monospace();
                            if self.warn_groups.contains(&g) {
                                text = text.color(Color32::from_rgb(230, 170, 60));
                            }
                            if ui
                                .selectable_label(self.sel.group == Some(g), text)
                                .clicked()
                            {
                                nav = Some(Nav::Group(g));
                            }
                        }
                    });
            });

        if !numeric && !f.starts_with('#') && !f.is_empty() {
            let needle = f.to_lowercase();
            let hits: Vec<usize> = (0..m.objects.len())
                .filter(|&i| {
                    m.meta[i].logic
                        && (m.meta[i].name.to_lowercase().contains(&needle)
                            || m.meta[i].mnemonic.to_lowercase().contains(&needle))
                })
                .take(500)
                .collect();
            egui::CollapsingHeader::new(format!(
                "Matching triggers ({}{})",
                hits.len(),
                if hits.len() == 500 { "+" } else { "" }
            ))
            .default_open(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("hits")
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for i in hits {
                            if ui
                                .selectable_label(
                                    self.sel.obj == Some(i),
                                    RichText::new(format!("#{i} {}", m.describe(i)))
                                        .monospace()
                                        .size(11.0),
                                )
                                .clicked()
                            {
                                nav = Some(Nav::Obj(i));
                            }
                        }
                    });
            });
        }

        egui::CollapsingHeader::new(format!("Entry points ({})", m.entry_points.len())).show(
            ui,
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("entries")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for &i in &m.entry_points {
                            let o = &m.objects[i];
                            let text = RichText::new(format!(
                                "x={:<7.0} {}",
                                o.config.pos.0,
                                m.describe(i)
                            ))
                            .monospace()
                            .size(11.0)
                            .color(act_color(m.meta[i].activation));
                            if ui.selectable_label(self.sel.obj == Some(i), text).clicked() {
                                nav = Some(Nav::Obj(i));
                            }
                        }
                    });
            },
        );

        egui::CollapsingHeader::new(format!("Diagnostics ({})", m.lints.len()))
            .default_open(!m.lints.is_empty())
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("lints")
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for l in &m.lints {
                            let color = match l.severity {
                                Severity::Warn => Color32::from_rgb(230, 170, 60),
                                Severity::Info => Color32::from_rgb(120, 190, 230),
                            };
                            let resp = ui.add(
                                egui::Label::new(RichText::new(&l.msg).color(color).size(12.0))
                                    .wrap()
                                    .sense(Sense::click()),
                            );
                            if resp.clicked() {
                                nav = l.obj.map(Nav::Obj).or(l.group.map(Nav::Group));
                            }
                            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                        }
                    });
            });
        nav
    }

    pub(super) fn inspector(&self, ui: &mut egui::Ui) -> Option<Nav> {
        let mut nav = None;
        let m = &self.m;
        let group_links = |ui: &mut egui::Ui, groups: &[Gid], nav: &mut Option<Nav>| {
            ui.horizontal_wrapped(|ui| {
                for &g in groups {
                    if ui.link(RichText::new(group_label(g)).monospace()).clicked() {
                        *nav = Some(Nav::Group(g));
                    }
                }
            });
        };

        if self.sel.obj.is_none() && self.sel.group.is_none() && self.sel.item.is_none() {
            ui.label(RichText::new("Click a group or object to inspect it.").weak());
            ui.add_space(8.0);
            ui.label("Tips:");
            ui.label("• Alt+Left or mouse back button: go back");
            ui.label("• type a group number or #index in the filter");
            ui.label("• double-click a graph node to see it on the map");
            return None;
        }

        if let Some(i) = self.sel.obj {
            let o = &m.objects[i];
            let meta = &m.meta[i];
            ui.heading(format!("#{i} {}", meta.name));
            egui::Grid::new("objinfo").num_columns(2).show(ui, |ui| {
                ui.label("object id");
                ui.monospace(o.id.to_string());
                ui.end_row();
                ui.label("position");
                ui.monospace(format!("({:.1}, {:.1})", o.config.pos.0, o.config.pos.1));
                ui.end_row();
                if o.config.angle != 0.0 || o.config.scale != (1.0, 1.0) {
                    ui.label("angle / scale");
                    ui.monospace(format!(
                        "{:.1}° / ({:.2}, {:.2})",
                        o.config.angle, o.config.scale.0, o.config.scale.1
                    ));
                    ui.end_row();
                }
                if meta.logic {
                    ui.label("runs on");
                    ui.label(
                        RichText::new(match meta.activation {
                            Activation::Spawn => "spawn of any of its groups",
                            Activation::Touch => "player touch",
                            Activation::Position => "player x-position",
                            Activation::Player => "player contact / input",
                            Activation::None => "(passive)",
                        })
                        .color(act_color(meta.activation)),
                    );
                    ui.end_row();
                    if o.config.trigger_cfg.multitriggerable {
                        ui.label("multi-trigger");
                        ui.label("yes");
                        ui.end_row();
                    }
                }
            });
            ui.label("groups:");
            if o.config.groups.is_empty() {
                ui.label(RichText::new("(none)").weak());
            } else {
                group_links(ui, &o.config.groups, &mut nav);
            }
            if let Some(s) = self.summaries.get(&i) {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(s)
                        .monospace()
                        .size(13.0)
                        .color(Color32::from_rgb(220, 220, 160)),
                );
            }
            if let Some(t) = self.texts.get(&i) {
                ui.label("text:");
                ui.label(RichText::new(t).monospace());
            }
            if !meta.actions.is_empty() {
                ui.label("targets:");
                for &ai in &meta.actions {
                    let a = &m.actions[ai];
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{:<10}", a.kind.name()))
                                .monospace()
                                .color(kind_color(a.kind)),
                        );
                        if ui
                            .link(RichText::new(group_label(a.target)).monospace())
                            .clicked()
                        {
                            nav = Some(Nav::Group(a.target));
                        }
                        ui.label(RichText::new(&a.role).weak());
                    });
                }
            }
            if !meta.items.is_empty() {
                ui.label("items:");
                ui.horizontal_wrapped(|ui| {
                    for (key, access) in &meta.items {
                        let verb = match access {
                            Access::Read => "reads",
                            Access::Write => "writes",
                            Access::Define => "defines",
                        };
                        ui.label(verb);
                        if ui
                            .link(RichText::new(item_label(*key)).monospace())
                            .clicked()
                        {
                            nav = Some(Nav::Item(*key));
                        }
                    }
                });
            }
            // what gdlib makes of it: the typed struct from the matching `from_object`
            match decode::typed(o) {
                Some((name, dbg)) => {
                    egui::CollapsingHeader::new(RichText::new(format!("gdlib: {name}")).strong())
                        .id_salt("typed")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(dbg).monospace().size(11.5)).wrap(),
                            );
                        });
                }
                None => {
                    ui.label(
                        RichText::new("gdlib has no typed constructor for this object").weak(),
                    );
                }
            }
            egui::CollapsingHeader::new(format!("raw properties ({})", o.get_properties().len()))
                .id_salt("rawprops")
                .show(ui, |ui| {
                    egui::Grid::new("props")
                        .striped(true)
                        .num_columns(3)
                        .show(ui, |ui| {
                            for (k, v) in o.get_properties() {
                                ui.monospace(k.to_string());
                                ui.label(property_name(*k));
                                ui.monospace(fmt_value(v));
                                ui.end_row();
                            }
                        });
                });
            ui.separator();
        }

        if let Some(g) = self.sel.group
            && let Some(info) = m.groups.get(&g)
        {
            ui.heading(if g == ROOT {
                "LEVEL (entry points)".to_string()
            } else {
                group_label(g)
            });
            if g != ROOT {
                let reach = if m.reachable.contains(&g) {
                    RichText::new("reachable from level start")
                        .color(Color32::from_rgb(80, 200, 120))
                } else {
                    RichText::new("not reachable from level start")
                        .color(Color32::from_rgb(230, 170, 60))
                };
                ui.label(format!(
                    "{} object(s), {} logic",
                    info.members.len(),
                    info.triggers.len()
                ));
                ui.label(reach);
            }
            for l in m.lints.iter().filter(|l| l.group == Some(g)) {
                ui.label(
                    RichText::new(&l.msg)
                        .color(Color32::from_rgb(230, 170, 60))
                        .size(12.0),
                );
            }

            if !info.incoming.is_empty() {
                egui::CollapsingHeader::new(format!("targeted by ({})", info.incoming.len()))
                    .id_salt("incoming")
                    .default_open(true)
                    .show(ui, |ui| {
                        for &ai in info.incoming.iter().take(300) {
                            let a = &m.actions[ai];
                            ui.horizontal_wrapped(|ui| {
                                group_links(ui, &m.trigger_sources(a.trigger), &mut nav);
                                ui.label(RichText::new("via").weak());
                                if ui
                                    .link(
                                        RichText::new(format!(
                                            "#{} {}",
                                            a.trigger, m.meta[a.trigger].mnemonic
                                        ))
                                        .monospace(),
                                    )
                                    .clicked()
                                {
                                    nav = Some(Nav::Obj(a.trigger));
                                }
                                ui.label(RichText::new(&a.role).color(kind_color(a.kind)));
                            });
                        }
                    });
            }
            if !info.out_edges.is_empty() {
                egui::CollapsingHeader::new(format!("targets ({})", info.out_edges.len()))
                    .id_salt("outgoing")
                    .default_open(true)
                    .show(ui, |ui| {
                        for &ei in &info.out_edges {
                            let e = &m.edges[ei];
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{:<10}", e.kind.name()))
                                        .monospace()
                                        .color(kind_color(e.kind)),
                                );
                                if ui
                                    .link(RichText::new(group_label(e.to)).monospace())
                                    .clicked()
                                {
                                    nav = Some(Nav::Group(e.to));
                                }
                                if e.actions.len() > 1 {
                                    ui.label(RichText::new(format!("x{}", e.actions.len())).weak());
                                }
                            });
                        }
                    });
            }
            let triggers: &[usize] = if g == ROOT {
                &m.entry_points
            } else {
                &info.triggers
            };
            if !triggers.is_empty() {
                egui::CollapsingHeader::new(format!("logic ({})", triggers.len()))
                    .id_salt("logic")
                    .default_open(true)
                    .show(ui, |ui| {
                        for &t in triggers.iter().take(500) {
                            let text = RichText::new(format!("#{t} {}", m.describe(t)))
                                .monospace()
                                .size(11.0)
                                .color(act_color(m.meta[t].activation));
                            if ui.selectable_label(self.sel.obj == Some(t), text).clicked() {
                                nav = Some(Nav::Obj(t));
                            }
                        }
                        if triggers.len() > 500 {
                            ui.label(format!("+{} more", triggers.len() - 500));
                        }
                    });
            }
            let deco: Vec<usize> = info
                .members
                .iter()
                .copied()
                .filter(|&i| !m.meta[i].logic)
                .collect();
            if !deco.is_empty() {
                egui::CollapsingHeader::new(format!("objects ({})", deco.len()))
                    .id_salt("deco")
                    .show(ui, |ui| {
                        ui.label(RichText::new(m.content_summary(&deco, 12)).weak());
                        for &i in deco.iter().take(300) {
                            let o = &m.objects[i];
                            if ui
                                .selectable_label(
                                    self.sel.obj == Some(i),
                                    RichText::new(format!(
                                        "#{i} {} ({:.0}, {:.0})",
                                        m.meta[i].name, o.config.pos.0, o.config.pos.1
                                    ))
                                    .size(11.0),
                                )
                                .clicked()
                            {
                                nav = Some(Nav::Obj(i));
                            }
                        }
                        if deco.len() > 300 {
                            ui.label(format!("+{} more", deco.len() - 300));
                        }
                    });
            }
            ui.separator();
        }

        if let Some(key) = self.sel.item
            && let Some(refs) = m.items.get(&key)
        {
            ui.heading(item_label(key));
            for r in refs {
                ui.horizontal(|ui| {
                    let verb = match r.access {
                        Access::Read => "read by",
                        Access::Write => "written by",
                        Access::Define => "defined by",
                    };
                    ui.label(verb);
                    if ui
                        .link(
                            RichText::new(format!("#{} {}", r.obj, m.meta[r.obj].mnemonic))
                                .monospace(),
                        )
                        .clicked()
                    {
                        nav = Some(Nav::Obj(r.obj));
                    }
                });
            }
        }
        nav
    }
}
