//! Read-only questions about a built [`Model`]: describing an object, summarising a group's
//! contents, and the neighbourhood of a group.

use super::*;

impl Model {
    /// One-line "disassembly" of a logic object: `MNEMONIC args`.
    pub fn describe(&self, idx: usize) -> String {
        let obj = &self.objects[idx];
        let m = &self.meta[idx];
        let mut parts: Vec<String> = vec![m.mnemonic.clone()];

        for &ai in &m.actions {
            let a = &self.actions[ai];
            parts.push(format!("{} {}", a.role, group_label(a.target)));
        }

        match obj.id {
            o::MOVE_TRIGGER => {
                if let Some(t) = MoveTrigger::from_object(obj) {
                    match &t.move_config {
                        MoveMode::Default(d) => {
                            let lock = |l: Option<MoveLock>| match l {
                                Some(MoveLock::Player) => "*player",
                                Some(MoveLock::Camera) => "*camera",
                                None => "",
                            };
                            parts.push(format!(
                                "by ({}{}, {}{})",
                                fmt_f(d.dx),
                                lock(d.x_lock),
                                fmt_f(d.dy),
                                lock(d.y_lock)
                            ));
                        }
                        MoveMode::Targeting(tm) => {
                            if let MoveTarget::Player1 | MoveTarget::Player2 = tm.target_group_id {
                                parts.push(format!("to {:?}", tm.target_group_id));
                            }
                            if let Some(axis) = tm.axis_only {
                                parts.push(format!("axis {axis:?}"));
                            }
                        }
                        MoveMode::Directional(dm) => {
                            if let MoveTarget::Player1 | MoveTarget::Player2 = dm.target_group_id {
                                parts.push(format!("toward {:?}", dm.target_group_id));
                            }
                            parts.push(format!("dist {}", dm.distance));
                        }
                    }
                    parts.push(format!("t={}", fmt_f(t.time)));
                    if t.easing.easing as i32 != 0 {
                        parts.push(format!("ease={:?}", t.easing.easing));
                    }
                    if t.silent {
                        parts.push("silent".into());
                    }
                }
            }
            o::ROTATE_TRIGGER => {
                if let Some(t) = RotateTrigger::from_object(obj) {
                    match &t.rotation_mode {
                        RotationMode::Default(r) => {
                            parts.push(format!("deg={}", fmt_f(r.degrees + 360.0 * r.x360 as f32)))
                        }
                        RotationMode::Aim(a) | RotationMode::Follow(a) => {
                            if let Some(pt) = a.player_target {
                                parts.push(format!("at {pt:?}"));
                            }
                            if a.rot_offset != 0.0 {
                                parts.push(format!("offset={}", fmt_f(a.rot_offset)));
                            }
                        }
                    }
                    parts.push(format!("t={}", fmt_f(t.move_time)));
                }
            }
            o::COLOR_TRIGGER => {
                if let Some(t) = ColourTrigger::from_object(obj) {
                    parts.push(format!(
                        "ch={} #{:02x}{:02x}{:02x} t={}",
                        i16::from(t.channel),
                        t.colour.red,
                        t.colour.green,
                        t.colour.blue,
                        fmt_f(t.fade_time)
                    ));
                }
            }
            o::PULSE_TRIGGER if !flag(obj, p::PULSE_GROUP) => {
                if let Some(c) = get_group(obj, p::TARGET_ITEM) {
                    parts.push(format!("ch={c}"));
                }
            }
            o::SPAWN_TRIGGER => {
                if let Some(d) = float(obj, p::SPAWN_DELAY).filter(|d| *d != 0.0) {
                    let var = float(obj, p::SPAWN_DELAY_VARIATION).filter(|v| *v != 0.0);
                    match var {
                        Some(v) => parts.push(format!("delay={}+-{}", fmt_f(d), fmt_f(v))),
                        None => parts.push(format!("delay={}", fmt_f(d))),
                    }
                }
                if flag(obj, p::SPAWN_ORDERED) {
                    parts.push("ordered".into());
                }
                if let Some(GDValue::SpawnRemapsList(r)) = obj.get_property(p::SPAWN_ID_REMAPS)
                    && !r.is_empty()
                {
                    let r: Vec<String> = r.iter().map(|(a, b)| format!("{a}->{b}")).collect();
                    parts.push(format!("remap[{}]", r.join(",")));
                }
            }
            o::RANDOM_TRIGGER | o::ADVANCED_RANDOM_TRIGGER => {}
            _ => {
                if let Some(t) = float(obj, p::DURATION_GROUP_TRIGGER_CHANCE).filter(|t| *t != 0.0)
                    && m.logic
                {
                    parts.push(format!("t={}", fmt_f(t)));
                }
            }
        }

        for (key, access) in &m.items {
            let verb = match access {
                Access::Read => "reads",
                Access::Write => "writes",
                Access::Define => "defines",
            };
            parts.push(format!("{verb} {}", item_label(*key)));
        }
        parts.join("  ")
    }

    /// "12x Default Block, 3x Spike, ..." for a set of objects.
    pub fn content_summary(&self, members: &[usize], limit: usize) -> String {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for &i in members {
            *counts.entry(self.meta[i].name.as_str()).or_default() += 1;
        }
        let mut counts: Vec<(&str, usize)> = counts.into_iter().collect();
        counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let mut parts: Vec<String> = counts
            .iter()
            .take(limit)
            .map(|(n, c)| format!("{c}x {n}"))
            .collect();
        if counts.len() > limit {
            parts.push(format!("+{} more kinds", counts.len() - limit));
        }
        parts.join(", ")
    }

    /// Groups within `depth` hops of `center` (both directions, all edge kinds).
    pub fn neighbourhood(&self, center: Gid, depth: usize) -> BTreeSet<Gid> {
        let mut seen = BTreeSet::from([center]);
        let mut frontier = vec![center];
        for _ in 0..depth {
            let mut next = Vec::new();
            for g in frontier {
                let Some(info) = self.groups.get(&g) else {
                    continue;
                };
                for &ei in info.out_edges.iter().chain(info.in_edges.iter()) {
                    let e = &self.edges[ei];
                    for n in [e.from, e.to] {
                        if seen.insert(n) {
                            next.push(n);
                        }
                    }
                }
            }
            frontier = next;
        }
        seen
    }
}
