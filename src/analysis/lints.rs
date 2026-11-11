//! Diagnostics: triggers that can never run, targets that don't exist, empty groups, ...

use super::*;

impl Model {
    pub(super) fn compute_lints(&self) -> Vec<Lint> {
        let mut lints = Vec::new();
        let remap_note = if self.has_remaps {
            " (level uses spawn remaps, so this may be a false positive)"
        } else {
            ""
        };

        for (i, m) in self.meta.iter().enumerate() {
            if m.activation == Activation::Spawn
                && !self.objects[i].config.groups.iter().any(|g| *g > 0)
            {
                lints.push(Lint {
                    severity: Severity::Warn,
                    group: None,
                    obj: Some(i),
                    msg: format!(
                        "#{i} {} is spawn-triggered but has no groups, so it can never run",
                        m.mnemonic
                    ),
                });
            }
        }

        for info in self.real_groups() {
            let g = info.id;
            // targets with nothing in them
            let random_only = info.incoming.iter().all(|&a| {
                matches!(
                    self.objects[self.actions[a].trigger].id,
                    o::RANDOM_TRIGGER | o::ADVANCED_RANDOM_TRIGGER
                )
            });
            if info.members.is_empty() && !info.incoming.is_empty() && random_only {
                // common idiom: an empty group as the "do nothing" outcome of a random trigger
                lints.push(Lint {
                    severity: Severity::Info,
                    group: Some(g),
                    obj: None,
                    msg: format!(
                        "{} is empty and used as a \"do nothing\" branch by {} random trigger(s)",
                        group_label(g),
                        info.incoming.len()
                    ),
                });
            } else if info.members.is_empty() && !info.incoming.is_empty() {
                let kinds: BTreeSet<&str> = info
                    .incoming
                    .iter()
                    .map(|&a| self.actions[a].kind.name())
                    .collect();
                lints.push(Lint {
                    severity: Severity::Warn,
                    group: Some(g),
                    obj: None,
                    msg: format!(
                        "{} is targeted ({}) by {} trigger(s) but contains no objects{remap_note}",
                        group_label(g),
                        kinds.into_iter().collect::<Vec<_>>().join(", "),
                        info.incoming.len()
                    ),
                });
            }
            // spawn triggers that never get spawned
            let spawn_triggers = info
                .triggers
                .iter()
                .filter(|&&t| self.meta[t].activation == Activation::Spawn)
                .count();
            if spawn_triggers > 0 && !self.reachable.contains(&g) {
                let callers: BTreeSet<Gid> = info
                    .in_edges
                    .iter()
                    .map(|&e| &self.edges[e])
                    .filter(|e| e.kind == EdgeKind::Spawn)
                    .map(|e| e.from)
                    .collect();
                let why = if callers.is_empty() {
                    "nothing spawns it".to_string()
                } else {
                    format!(
                        "only spawned from unreachable group(s) {}",
                        callers
                            .iter()
                            .map(|g| group_label(*g))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                lints.push(Lint {
                    severity: Severity::Warn,
                    group: Some(g),
                    obj: None,
                    msg: format!("{} has {spawn_triggers} spawn-triggered trigger(s) that never run: {why}{remap_note}", group_label(g)),
                });
            }
        }

        for cycle in &self.cycles {
            let names: Vec<String> = cycle.iter().map(|g| group_label(*g)).collect();
            lints.push(Lint {
                severity: Severity::Info,
                group: cycle.first().copied(),
                obj: None,
                msg: format!(
                    "spawn loop: {} (fine if intentional, e.g. a delayed tick loop)",
                    names.join(" <-> ")
                ),
            });
        }
        lints.sort_by(|a, b| b.severity.cmp(&a.severity));
        lints
    }
}
