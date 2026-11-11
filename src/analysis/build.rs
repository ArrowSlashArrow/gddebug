//! Building the [`Model`]: metadata per object, actions, group edges, entry points, items, plus
//! reachability and spawn-loop detection.

use super::*;

impl Model {
    pub fn build(level_name: String, creator: String, objects: Vec<GDObject>) -> Self {
        let mut meta = Vec::with_capacity(objects.len());
        let mut actions = Vec::new();
        let mut items: BTreeMap<ItemKey, Vec<ItemRef>> = BTreeMap::new();
        let mut groups: BTreeMap<Gid, GroupInfo> = BTreeMap::new();
        let mut has_remaps = false;
        let mut bounds = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut sums: HashMap<Gid, (f64, f64, usize)> = HashMap::new();

        for (idx, obj) in objects.iter().enumerate() {
            let name = OBJECT_NAMES
                .get(&obj.id)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("Object {}", obj.id));
            let classified = classify(obj);
            let refs = item_refs(obj);
            let logic = !classified.is_empty()
                || !refs.is_empty()
                || name.contains("Trigger")
                || is_player_object(obj.id);

            let activation = if !logic || obj.id == o::COLLISION_BLOCK || obj.id == o::COUNTER_LABEL
            {
                Activation::None
            } else if obj.config.trigger_cfg.spawnable {
                Activation::Spawn
            } else if obj.config.trigger_cfg.touchable {
                Activation::Touch
            } else if is_player_object(obj.id) {
                Activation::Player
            } else {
                Activation::Position
            };

            if let Some(GDValue::SpawnRemapsList(r)) = obj.get_property(p::SPAWN_ID_REMAPS) {
                has_remaps |= !r.is_empty();
            }

            let mut my_actions = Vec::with_capacity(classified.len());
            for (target, kind, role) in classified {
                my_actions.push(actions.len());
                actions.push(Action {
                    trigger: idx,
                    target,
                    kind,
                    role,
                });
            }
            for (key, access) in &refs {
                items.entry(*key).or_default().push(ItemRef {
                    obj: idx,
                    access: *access,
                });
            }

            let (x, y) = obj.config.pos;
            bounds = (
                bounds.0.min(x),
                bounds.1.min(y),
                bounds.2.max(x),
                bounds.3.max(y),
            );
            for &g in &obj.config.groups {
                if g <= 0 {
                    continue;
                }
                let info = groups.entry(g).or_insert_with(|| GroupInfo {
                    id: g,
                    ..Default::default()
                });
                info.members.push(idx);
                if logic {
                    info.triggers.push(idx);
                }
                let s = sums.entry(g).or_insert((0.0, 0.0, 0));
                s.0 += x as f64;
                s.1 += y as f64;
                s.2 += 1;
            }

            meta.push(ObjMeta {
                mnemonic: mnemonic(&name),
                name,
                logic,
                activation,
                actions: my_actions,
                items: refs,
            });
        }
        if objects.is_empty() {
            bounds = (0.0, 0.0, 0.0, 0.0);
        }
        for (g, (sx, sy, n)) in sums {
            if let Some(info) = groups.get_mut(&g) {
                info.centroid = Some(((sx / n as f64) as f32, (sy / n as f64) as f32));
            }
        }

        // aggregate actions into group edges
        groups.entry(ROOT).or_insert_with(|| GroupInfo {
            id: ROOT,
            ..Default::default()
        });
        let mut edge_map: BTreeMap<(Gid, Gid, EdgeKind), Vec<usize>> = BTreeMap::new();
        for (ai, a) in actions.iter().enumerate() {
            groups
                .entry(a.target)
                .or_insert_with(|| GroupInfo {
                    id: a.target,
                    ..Default::default()
                })
                .incoming
                .push(ai);
            let trig = &objects[a.trigger];
            if meta[a.trigger].activation == Activation::Spawn {
                // a spawn trigger without groups can never run: no edge, a lint is raised below
                for &g in trig.config.groups.iter().filter(|g| **g > 0) {
                    edge_map.entry((g, a.target, a.kind)).or_default().push(ai);
                }
            } else {
                edge_map
                    .entry((ROOT, a.target, a.kind))
                    .or_default()
                    .push(ai);
            }
        }
        let mut edges = Vec::with_capacity(edge_map.len());
        for ((from, to, kind), acts) in edge_map {
            let ei = edges.len();
            groups
                .entry(from)
                .or_insert_with(|| GroupInfo {
                    id: from,
                    ..Default::default()
                })
                .out_edges
                .push(ei);
            groups
                .entry(to)
                .or_insert_with(|| GroupInfo {
                    id: to,
                    ..Default::default()
                })
                .in_edges
                .push(ei);
            edges.push(Edge {
                from,
                to,
                kind,
                actions: acts,
            });
        }

        let mut entry_points: Vec<usize> = (0..objects.len())
            .filter(|&i| {
                meta[i].logic && !matches!(meta[i].activation, Activation::Spawn | Activation::None)
            })
            .collect();
        entry_points.sort_by(|a, b| {
            objects[*a]
                .config
                .pos
                .0
                .total_cmp(&objects[*b].config.pos.0)
        });

        let mut model = Model {
            level_name,
            creator,
            objects,
            meta,
            actions,
            groups,
            edges,
            entry_points,
            items,
            reachable: BTreeSet::new(),
            cycles: Vec::new(),
            lints: Vec::new(),
            has_remaps,
            bounds,
        };
        model.reachable = model.compute_reachable();
        model.cycles = model.compute_cycles();
        model.lints = model.compute_lints();
        model
    }

    pub fn trigger_count(&self) -> usize {
        self.meta.iter().filter(|m| m.logic).count()
    }

    /// Real groups (excludes ROOT).
    pub fn real_groups(&self) -> impl Iterator<Item = &GroupInfo> {
        self.groups.values().filter(|g| g.id != ROOT)
    }

    /// Groups that spawn-trigger objects in `g`'s triggers would be run by. Source groups of a trigger.
    pub fn trigger_sources(&self, obj: usize) -> Vec<Gid> {
        if self.meta[obj].activation == Activation::Spawn {
            self.objects[obj]
                .config
                .groups
                .iter()
                .copied()
                .filter(|g| *g > 0)
                .collect()
        } else {
            vec![ROOT]
        }
    }

    pub(super) fn compute_reachable(&self) -> BTreeSet<Gid> {
        let mut seen = BTreeSet::from([ROOT]);
        let mut queue = VecDeque::from([ROOT]);
        while let Some(g) = queue.pop_front() {
            let Some(info) = self.groups.get(&g) else {
                continue;
            };
            for &ei in &info.out_edges {
                let e = &self.edges[ei];
                if e.kind == EdgeKind::Spawn && seen.insert(e.to) {
                    queue.push_back(e.to);
                }
            }
        }
        seen
    }

    /// Iterative Tarjan SCC over spawn edges between real groups.
    pub(super) fn compute_cycles(&self) -> Vec<Vec<Gid>> {
        let nodes: Vec<Gid> = self.groups.keys().copied().filter(|g| *g != ROOT).collect();
        let pos: HashMap<Gid, usize> = nodes.iter().enumerate().map(|(i, g)| (*g, i)).collect();
        let succ: Vec<Vec<usize>> = nodes
            .iter()
            .map(|g| {
                let mut s: Vec<usize> = self.groups[g]
                    .out_edges
                    .iter()
                    .map(|&ei| &self.edges[ei])
                    .filter(|e| e.kind == EdgeKind::Spawn && e.to != ROOT)
                    .filter_map(|e| pos.get(&e.to).copied())
                    .collect();
                s.dedup();
                s
            })
            .collect();

        let n = nodes.len();
        let mut index = vec![usize::MAX; n];
        let mut low = vec![0usize; n];
        let mut on_stack = vec![false; n];
        let mut stack = Vec::new();
        let mut counter = 0usize;
        let mut sccs = Vec::new();

        for start in 0..n {
            if index[start] != usize::MAX {
                continue;
            }
            // (node, next successor position)
            let mut call: Vec<(usize, usize)> = vec![(start, 0)];
            index[start] = counter;
            low[start] = counter;
            counter += 1;
            stack.push(start);
            on_stack[start] = true;

            while let Some(&mut (v, ref mut i)) = call.last_mut() {
                if *i < succ[v].len() {
                    let w = succ[v][*i];
                    *i += 1;
                    if index[w] == usize::MAX {
                        index[w] = counter;
                        low[w] = counter;
                        counter += 1;
                        stack.push(w);
                        on_stack[w] = true;
                        call.push((w, 0));
                    } else if on_stack[w] {
                        low[v] = low[v].min(index[w]);
                    }
                } else {
                    call.pop();
                    if let Some(&(parent, _)) = call.last() {
                        low[parent] = low[parent].min(low[v]);
                    }
                    if low[v] == index[v] {
                        let mut comp = Vec::new();
                        loop {
                            let w = stack.pop().unwrap();
                            on_stack[w] = false;
                            comp.push(nodes[w]);
                            if w == v {
                                break;
                            }
                        }
                        let self_loop = comp.len() == 1 && succ[v].contains(&v);
                        if comp.len() > 1 || self_loop {
                            comp.sort();
                            sccs.push(comp);
                        }
                    }
                }
            }
        }
        sccs.sort();
        sccs
    }
}
