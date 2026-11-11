//! Non-interactive outputs: a text disassembly listing, Graphviz DOT and Mermaid.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::analysis::{
    Access, Activation, EdgeKind, Gid, Model, ROOT, Severity, group_label, item_label,
};
use crate::listing::{self, Mode, Names};

/// Which groups to include in an output.
pub struct Filter {
    /// Restrict to these groups (from `--group`/`--depth`). `None` = everything.
    pub only: Option<BTreeSet<Gid>>,
    /// Include decorative groups with no triggers and no edges.
    pub all_groups: bool,
}

impl Filter {
    fn keep(&self, m: &Model, g: Gid) -> bool {
        if let Some(only) = &self.only
            && !only.contains(&g)
        {
            return false;
        }
        g == ROOT || self.all_groups || m.groups.get(&g).is_some_and(|i| i.has_logic())
    }
}

/// One trigger line of the listing (without indentation).
/// `same_tick` marks a TASM instruction with `~` (runs on the same tick as the previous one).
pub fn obj_line(m: &Model, i: usize, mode: Mode, names: &Names, same_tick: bool) -> String {
    let o = &m.objects[i];
    match mode {
        Mode::Tasm => {
            let ins = format!(
                "{}{}",
                if same_tick { "~" } else { "" },
                listing::tasm(m, i, names)
            );
            format!("{ins:<56} ; #{i} x={:.1}", o.config.pos.0)
        }
        _ => format!(
            "#{i:<6} x={:>8.1} y={:>7.1}  [{:<6}] {}",
            o.config.pos.0,
            o.config.pos.1,
            m.meta[i].activation.tag(),
            listing::line(m, i, mode, names, true)
        ),
    }
}

/// Triggers of a group, left to right. In tasm mode, a trigger at the same x as the one before it
/// gets TASM's "same tick" marker `~`.
pub fn group_lines(
    m: &Model,
    triggers: &[usize],
    mode: Mode,
    names: &Names,
) -> Vec<(usize, String)> {
    let mut trig = triggers.to_vec();
    trig.sort_by(|a, b| {
        m.objects[*a]
            .config
            .pos
            .0
            .total_cmp(&m.objects[*b].config.pos.0)
    });
    let mut prev_x = None;
    trig.iter()
        .map(|&t| {
            let x = m.objects[t].config.pos.0;
            let same = mode == Mode::Tasm && prev_x == Some(x);
            prev_x = Some(x);
            (t, obj_line(m, t, mode, names, same))
        })
        .collect()
}

/// Describe who targets group `g`, grouped by source.
fn callers(m: &Model, g: Gid) -> Vec<String> {
    let Some(info) = m.groups.get(&g) else {
        return vec![];
    };
    let mut out = Vec::new();
    for &ai in &info.incoming {
        let a = &m.actions[ai];
        let src: Vec<String> = m
            .trigger_sources(a.trigger)
            .into_iter()
            .map(group_label)
            .collect();
        let src = if src.is_empty() {
            "<never runs>".to_string()
        } else {
            src.join("|")
        };
        out.push(format!(
            "{src} via #{} {} ({})",
            a.trigger, m.meta[a.trigger].mnemonic, a.role
        ));
    }
    out
}

pub fn text(m: &Model, f: &Filter, mode: Mode) -> String {
    let names = Names::new(m);
    let tasm = mode == Mode::Tasm;
    // in tasm mode everything that isn't an instruction or a routine label is a comment
    let c = if tasm { "  ; " } else { "  " };
    let mut s = String::new();
    let used = m.real_groups().filter(|g| !g.members.is_empty()).count();
    let _ = writeln!(
        s,
        "; gddbg disassembly of \"{}\" by {}",
        m.level_name, m.creator
    );
    let _ = writeln!(
        s,
        "; {} objects, {} logic objects, {} groups in use, {} group edges",
        m.objects.len(),
        m.trigger_count(),
        used,
        m.edges.len()
    );
    let _ = writeln!(
        s,
        "; activation tags: spawn = spawn-triggered, x-pos = player x-position, touch = touch-triggered, player = player contact/input"
    );
    let _ = writeln!(s, "; #N is the object's index in the level's object list");
    if tasm {
        let _ = writeln!(
            s,
            "; routines are named group_N; groups without triggers are referenced as gN"
        );
        let _ = writeln!(
            s,
            "; compares are written in their instant (I-prefixed) form: that is what a single compare trigger does"
        );
    }
    s.push('\n');

    // entry points
    let _ = writeln!(
        s,
        "; ===================== ENTRY POINTS (run by the level, sorted by x) ====================="
    );
    if tasm {
        let _ = writeln!(s, "_init:");
    }
    let mut any = false;
    for &i in &m.entry_points {
        if f.only.as_ref().is_some_and(|only| {
            !only.contains(&ROOT)
                && !m.meta[i]
                    .actions
                    .iter()
                    .any(|&a| only.contains(&m.actions[a].target))
        }) {
            continue;
        }
        any = true;
        let _ = writeln!(s, "  {}", obj_line(m, i, mode, &names, false));
    }
    if !any {
        let _ = writeln!(s, "  ; (none)");
    }

    // groups
    let _ = writeln!(s, "\n; ===================== GROUPS =====================");
    let mut hidden = 0usize;
    for info in m.real_groups() {
        if !f.keep(m, info.id) {
            hidden += 1;
            continue;
        }
        let g = info.id;
        let reach = if m.reachable.contains(&g) {
            ""
        } else if info
            .triggers
            .iter()
            .any(|&t| m.meta[t].activation == Activation::Spawn)
        {
            "  ; UNREACHABLE"
        } else {
            ""
        };
        let _ = writeln!(
            s,
            "\n{}:{:width$}; {} object(s), {} logic{reach}",
            if tasm { names.label(g) } else { group_label(g) },
            "",
            info.members.len(),
            info.triggers.len(),
            width = if tasm {
                3
            } else {
                8usize.saturating_sub(group_label(g).len() + 1)
            }
        );
        let c = callers(m, g);
        if c.is_empty() {
            let _ = writeln!(s, "  ; targeted by: nothing");
        } else {
            for line in c {
                let _ = writeln!(s, "  ; <- {line}");
            }
        }
        let non_logic: Vec<usize> = info
            .members
            .iter()
            .copied()
            .filter(|&i| !m.meta[i].logic)
            .collect();
        if !non_logic.is_empty() {
            let _ = writeln!(s, "  ; objects: {}", m.content_summary(&non_logic, 6));
        }
        for (_, line) in group_lines(m, &info.triggers, mode, &names) {
            let _ = writeln!(s, "  {line}");
        }
    }
    if hidden > 0 {
        let _ = writeln!(
            s,
            "\n; ({hidden} group(s) hidden: decorative only or outside --group focus; use --all-groups to show them)"
        );
    }

    // items
    if !m.items.is_empty() {
        let _ = writeln!(
            s,
            "\n; ===================== ITEMS / COLLIDERS ====================="
        );
        for (key, refs) in &m.items {
            let fmt = |acc: Access| {
                let v: Vec<String> = refs
                    .iter()
                    .filter(|r| r.access == acc)
                    .map(|r| format!("#{}", r.obj))
                    .collect();
                v.join(" ")
            };
            let (w, r, d) = (fmt(Access::Write), fmt(Access::Read), fmt(Access::Define));
            let mut line = format!("{c}{:<10}", item_label(*key));
            if !d.is_empty() {
                let _ = write!(line, " defined by: {d}");
            }
            if !w.is_empty() {
                let _ = write!(line, " written by: {w}");
            }
            if !r.is_empty() {
                let _ = write!(line, " read by: {r}");
            }
            let _ = writeln!(s, "{line}");
        }
    }

    // diagnostics
    let _ = writeln!(
        s,
        "\n; ===================== DIAGNOSTICS ====================="
    );
    if m.lints.is_empty() {
        let _ = writeln!(s, "  ; no issues found");
    }
    for l in &m.lints {
        let tag = match l.severity {
            Severity::Warn => "warn",
            Severity::Info => "info",
        };
        let _ = writeln!(s, "{c}[{tag}] {}", l.msg);
    }
    s
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Node label: group name, size, and the triggers inside it.
fn node_lines(m: &Model, g: Gid, max_triggers: usize) -> Vec<String> {
    let info = &m.groups[&g];
    if g == ROOT {
        return vec![
            "LEVEL".into(),
            format!("{} entry trigger(s)", m.entry_points.len()),
        ];
    }
    let mut lines = vec![format!("{}  ({} obj)", group_label(g), info.members.len())];
    let mut trig = info.triggers.clone();
    trig.sort_by(|a, b| {
        m.objects[*a]
            .config
            .pos
            .0
            .total_cmp(&m.objects[*b].config.pos.0)
    });
    for &t in trig.iter().take(max_triggers) {
        let mut d = m.describe(t);
        if d.len() > 60 {
            d.truncate(57);
            d.push_str("...");
        }
        lines.push(d);
    }
    if trig.len() > max_triggers {
        lines.push(format!("... +{} more", trig.len() - max_triggers));
    }
    let non_logic: Vec<usize> = info
        .members
        .iter()
        .copied()
        .filter(|&i| !m.meta[i].logic)
        .collect();
    if !non_logic.is_empty() {
        lines.push(m.content_summary(&non_logic, 3));
    }
    lines
}

/// Edges that pass the filter, merged per (from, to, kind) with a combined label.
fn visible_edges(m: &Model, f: &Filter) -> Vec<(Gid, Gid, EdgeKind, String)> {
    let mut out = Vec::new();
    for e in &m.edges {
        if !f.keep(m, e.from) || !f.keep(m, e.to) {
            continue;
        }
        let roles: BTreeSet<&str> = e
            .actions
            .iter()
            .map(|&a| m.actions[a].role.as_str())
            .collect();
        let mut label = roles.into_iter().collect::<Vec<_>>().join(", ");
        if e.actions.len() > 1 {
            let _ = write!(label, " x{}", e.actions.len());
        }
        out.push((e.from, e.to, e.kind, label));
    }
    out
}

pub fn dot(m: &Model, f: &Filter) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "// gddbg call graph of \"{}\" by {}",
        esc(&m.level_name),
        esc(&m.creator)
    );
    let _ = writeln!(s, "// render with: dot -Tsvg graph.dot -o graph.svg");
    let _ = writeln!(s, "digraph gd {{");
    let _ = writeln!(
        s,
        "  rankdir=LR; bgcolor=\"#1e1e1e\"; nodesep=0.3; ranksep=0.8;"
    );
    let _ = writeln!(
        s,
        "  node [shape=box, style=\"rounded,filled\", fontname=\"Consolas\", fontsize=10, fillcolor=\"#2d2d30\", fontcolor=\"#dddddd\", color=\"#666666\"];"
    );
    let _ = writeln!(
        s,
        "  edge [fontname=\"Consolas\", fontsize=9, fontcolor=\"#bbbbbb\"];"
    );

    let groups: Vec<Gid> = m.groups.keys().copied().filter(|g| f.keep(m, *g)).collect();
    for &g in &groups {
        let lines = node_lines(m, g, 12);
        let label = lines.iter().map(|l| esc(l)).collect::<Vec<_>>().join("\\l") + "\\l";
        let id = if g == ROOT {
            "LEVEL".to_string()
        } else {
            format!("g{g}")
        };
        let extra = if g == ROOT {
            ", shape=doubleoctagon, fillcolor=\"#1f4d2b\""
        } else if m
            .lints
            .iter()
            .any(|l| l.group == Some(g) && l.severity == Severity::Warn)
        {
            ", color=\"#e0a030\", penwidth=2"
        } else if m.cycles.iter().any(|c| c.contains(&g)) {
            ", color=\"#50c878\", penwidth=2"
        } else {
            ""
        };
        let _ = writeln!(s, "  {id} [label=\"{label}\"{extra}];");
    }

    for (from, to, kind, label) in visible_edges(m, f) {
        let (r, g, b) = kind.rgb();
        let style = match kind {
            EdgeKind::Affect => "dashed",
            EdgeKind::Reference => "dotted",
            _ => "solid",
        };
        let id = |g: Gid| {
            if g == ROOT {
                "LEVEL".to_string()
            } else {
                format!("g{g}")
            }
        };
        let _ = writeln!(
            s,
            "  {} -> {} [label=\"{}\", color=\"#{r:02x}{g:02x}{b:02x}\", style={style}];",
            id(from),
            id(to),
            esc(&format!("{}: {label}", kind.name()))
        );
    }

    // legend
    let _ = writeln!(
        s,
        "  subgraph cluster_legend {{ label=\"legend\"; fontcolor=\"#bbbbbb\"; color=\"#444444\";"
    );
    let mut seen = BTreeMap::new();
    for k in EdgeKind::ALL {
        seen.entry(k.rgb()).or_insert(k);
    }
    for (i, (_, k)) in seen.iter().enumerate() {
        let (r, g, b) = k.rgb();
        let _ = writeln!(
            s,
            "    lg{i}a [label=\"\", shape=point, color=\"#{r:02x}{g:02x}{b:02x}\"]; lg{i}b [label=\"{}\", shape=plaintext];",
            k.name()
        );
        let _ = writeln!(
            s,
            "    lg{i}a -> lg{i}b [color=\"#{r:02x}{g:02x}{b:02x}\"];"
        );
    }
    let _ = writeln!(s, "  }}");
    let _ = writeln!(s, "}}");
    s
}

pub fn mermaid(m: &Model, f: &Filter) -> String {
    let mut s = String::from("flowchart LR\n");
    let id = |g: Gid| {
        if g == ROOT {
            "LEVEL".to_string()
        } else {
            format!("g{g}")
        }
    };
    let clean = |t: &str| {
        t.replace('"', "'")
            .replace(['[', ']', '{', '}', '<', '>', '|'], " ")
    };
    for g in m.groups.keys().copied().filter(|g| f.keep(m, *g)) {
        let lines: Vec<String> = node_lines(m, g, 6).iter().map(|l| clean(l)).collect();
        if g == ROOT {
            let _ = writeln!(s, "  {}([\"{}\"])", id(g), lines.join("<br/>"));
        } else {
            let _ = writeln!(s, "  {}[\"{}\"]", id(g), lines.join("<br/>"));
        }
    }
    for (from, to, kind, label) in visible_edges(m, f) {
        let arrow = match kind {
            EdgeKind::Affect | EdgeKind::Reference => "-.->",
            _ => "-->",
        };
        let _ = writeln!(
            s,
            "  {} {arrow}|\"{}\"| {}",
            id(from),
            clean(&format!("{}: {label}", kind.name())),
            id(to)
        );
    }
    s
}
