//! The three ways a trigger can be listed (listing tab, `--listing` CLI output, detailed graph
//! cards):
//! * [`Mode::Raw`]: what the trigger touches: `moves g50`, `changes colour 520`, `writes to t294`.
//! * [`Mode::Details`]: what it actually does: `move g50 by (10, 0) in 0.5s`, `t294 = c1 * 2`.
//! * [`Mode::Tasm`]: the trigger as a TASM instruction (see the tasm-lang repository), or
//!   `UNKNOWN object <id>` when TASM has no instruction that compiles to it.

use std::collections::HashSet;

use gdlib::cclocallevels::gdobj::{
    CollisionTrigger, ColourTrigger, CounterLabel, EventTrigger, InstantCollTrigger,
    ItemCompareTrigger, ItemEditTrigger, ObjectProperties, PersistentItemTrigger, PulseTrigger,
    RandomTrigger, SpawnTrigger, StopTrigger, TimeControlTrigger, TimeTrigger, ToggleTrigger,
    ids::objects as o,
    structs::{CompareOp, CompareOperand, Item, Op, PulseTarget, RoundMode, SignMode, StopMode},
};

use crate::analysis::{Access, EdgeKind, Gid, Model, ROOT, item_label};
use crate::decode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Mode {
    /// What each trigger touches (groups moved, items written, ...).
    Raw,
    /// What each trigger does (equations, conditions, distances, colours, ...).
    #[default]
    Details,
    /// Each group as a TASM routine.
    Tasm,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Raw, Mode::Details, Mode::Tasm];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Raw => "raw",
            Mode::Details => "details",
            Mode::Tasm => "tasm",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Mode::Raw => "what each trigger touches",
            Mode::Details => "what each trigger does",
            Mode::Tasm => "each group as a TASM routine",
        }
    }
}

/// TASM names for groups: groups that contain triggers become routines (`group_12`), every other
/// group is referenced by its literal (`g12`).
pub struct Names {
    routines: HashSet<Gid>,
}

impl Names {
    pub fn new(m: &Model) -> Self {
        Names {
            routines: m
                .real_groups()
                .filter(|g| !g.triggers.is_empty())
                .map(|g| g.id)
                .collect(),
        }
    }

    /// A group as an instruction argument.
    pub fn group(&self, g: Gid) -> String {
        if self.routines.contains(&g) {
            format!("group_{g}")
        } else {
            format!("g{g}")
        }
    }

    /// The routine label of a group (`group_12`, `_init` for the level).
    pub fn label(&self, g: Gid) -> String {
        if g == ROOT {
            "_init".into()
        } else {
            format!("group_{g}")
        }
    }
}

/// One trigger in the given mode, without position or index. `with_mnemonic` prefixes the trigger
/// name in the raw and details modes (the listing has room for it, graph cards show it by colour).
pub fn line(m: &Model, i: usize, mode: Mode, names: &Names, with_mnemonic: bool) -> String {
    let body = match mode {
        Mode::Raw => raw(m, i),
        Mode::Details => details(m, i),
        Mode::Tasm => return tasm(m, i, names),
    };
    match (body.is_empty(), with_mnemonic) {
        (true, true) => format!("{:<10} -", m.meta[i].mnemonic),
        // nothing to describe (e.g. a reverse trigger): its name says it all
        (true, false) => m.meta[i].mnemonic.clone(),
        (false, true) => format!("{:<10} {body}", m.meta[i].mnemonic),
        (false, false) => body,
    }
}

// ---------------------------------------------------------------- raw

/// Third-person verb for an action role, if it reads better as one ("move" -> "moves").
fn verb(role: &str) -> Option<&'static str> {
    Some(match role {
        "spawn" => "spawns",
        "move" => "moves",
        "rotate" => "rotates",
        "scale" => "scales",
        "fade" => "fades",
        "animate" => "animates",
        "pulse" => "pulses",
        "follow" => "moves",
        "toggle on" => "toggles on",
        "toggle off" => "toggles off",
        "stop" => "stops",
        "pause" => "pauses",
        "resume" => "resumes",
        "reset" => "resets",
        "link visible" => "links visibility of",
        _ => return None,
    })
}

fn raw(m: &Model, i: usize) -> String {
    let obj = &m.objects[i];
    let mut parts: Vec<String> = Vec::new();
    for &ai in &m.meta[i].actions {
        let a = &m.actions[ai];
        let g = format!("g{}", a.target);
        parts.push(match (verb(&a.role), a.kind) {
            (Some(v), _) => format!("{v} {g}"),
            (None, _) if a.role == "follow player y" => format!("moves {g} with player y"),
            // branches and listeners: what happens, then when
            (None, EdgeKind::Spawn) => format!("spawns {g} ({})", a.role),
            (None, EdgeKind::ToggleOn) => format!("toggles on {g} ({})", a.role),
            (None, EdgeKind::ToggleOff) => format!("toggles off {g} ({})", a.role),
            (None, EdgeKind::Toggle) => format!("toggles {g} ({})", a.role),
            (None, _) => format!("{}: {g}", a.role),
        });
    }
    // colour channels aren't groups, so they don't show up as actions
    match obj.id {
        o::COLOR_TRIGGER => {
            if let Some(t) = ColourTrigger::from_object(obj) {
                parts.insert(0, format!("changes colour {}", i16::from(t.channel)));
            }
        }
        o::PULSE_TRIGGER => {
            if let Some(PulseTrigger {
                pulse_target: PulseTarget::Channel(c),
                ..
            }) = PulseTrigger::from_object(obj)
            {
                parts.insert(0, format!("pulses colour {}", c.channel_id));
            }
        }
        _ => {}
    }
    // "writes to c4, reads c1, c3": one phrase per access kind, each item once
    for (access, verb) in [
        (Access::Write, "writes to"),
        (Access::Read, "reads"),
        (Access::Define, "is"),
    ] {
        let mut items: Vec<String> = Vec::new();
        for (key, a) in &m.meta[i].items {
            let name = item_label(*key);
            if *a == access && !items.contains(&name) {
                items.push(name);
            }
        }
        if !items.is_empty() {
            parts.push(format!("{verb} {}", items.join(", ")));
        }
    }
    parts.join(", ")
}

// ---------------------------------------------------------------- details

fn details(m: &Model, i: usize) -> String {
    decode::summary(&m.objects[i]).unwrap_or_else(|| {
        // no typed constructor: what the analysis knows, minus the mnemonic it starts with
        let d = m.describe(i);
        d.strip_prefix(m.meta[i].mnemonic.as_str())
            .map(|s| s.trim_start().to_string())
            .unwrap_or(d)
    })
}

// ---------------------------------------------------------------- tasm

/// An instruction being built: `NAME a, b | flag:v flag:v ; note`.
struct Instr {
    name: String,
    args: Vec<String>,
    flags: Vec<String>,
    notes: Vec<String>,
}

impl Instr {
    fn new(name: &str, args: Vec<String>) -> Self {
        Instr {
            name: name.into(),
            args,
            flags: vec![],
            notes: vec![],
        }
    }

    fn flag(&mut self, name: &str, value: impl std::fmt::Display) {
        self.flags.push(format!("{name}:{value}"));
    }

    fn note(&mut self, n: impl Into<String>) {
        self.notes.push(n.into());
    }

    fn render(self) -> String {
        let mut s = self.name;
        if !self.args.is_empty() {
            s += " ";
            s += &self.args.join(", ");
        }
        if !self.flags.is_empty() {
            s += " | ";
            s += &self.flags.join(" ");
        }
        if !self.notes.is_empty() {
            s += "  ; ";
            s += &self.notes.join("; ");
        }
        s
    }
}

/// Shortest exact form of a float (`0.0044`, `2`, `-1.5`).
fn fnum(f: f32) -> String {
    let s = format!("{f}");
    if s == "-0" { "0".into() } else { s }
}

fn t_item(i: Item) -> String {
    match i {
        Item::Counter(n) => format!("C{n}"),
        Item::Timer(n) => format!("T{n}"),
        Item::Points => "POINTS".into(),
        Item::MainTime => "MAINTIME".into(),
        Item::Attempts => "ATTEMPTS".into(),
    }
}

fn t_op(op: Op) -> &'static str {
    match op {
        Op::Set => "=",
        Op::Add => "+",
        Op::Sub => "-",
        Op::Mul => "*",
        Op::Div => "/",
    }
}

fn arith_name(op: Op) -> &'static str {
    match op {
        Op::Set => "MOV",
        Op::Add => "ADD",
        Op::Sub => "SUB",
        Op::Mul => "MUL",
        Op::Div => "DIV",
    }
}

/// Round/sign flag value (`floor+`, `-`, `round`); `None` when neither is set.
fn t_mode((r, s): (RoundMode, SignMode)) -> Option<String> {
    let r = match r {
        RoundMode::None => "",
        RoundMode::Nearest => "round",
        RoundMode::Floor => "floor",
        RoundMode::Ceiling => "ceil",
    };
    let s = match s {
        SignMode::None => "",
        SignMode::Absolute => "+",
        SignMode::Negative => "-",
    };
    let v = format!("{r}{s}");
    (!v.is_empty()).then_some(v)
}

const NO_MODE: (RoundMode, SignMode) = (RoundMode::None, SignMode::None);
const FLOOR: (RoundMode, SignMode) = (RoundMode::Floor, SignMode::None);

fn item_edit(t: &ItemEditTrigger) -> Instr {
    let target = t_item(t.target);
    let operands: Vec<Item> = [t.operand1, t.operand2].into_iter().flatten().collect();
    let mut res = (t.id_rounding, t.id_sign);
    let mut fin = (t.result_rounding, t.result_sign);
    let modifier = fnum(t.modifier);
    let mut ins = match operands[..] {
        // target <op>= modifier
        [] => {
            let mut ins = Instr::new(arith_name(t.assign_op), vec![target, modifier]);
            if t.assign_op == Op::Div && fin == FLOOR {
                ins.name = "FLDIV".into();
                fin = NO_MODE;
            }
            ins
        }
        // target = a * mod (MOV / MUL / DIV with a number), or target <op>= a
        [a] if t.assign_op == Op::Set => {
            if t.modifier == 1.0 {
                Instr::new("MOV", vec![target, t_item(a)])
            } else {
                let mut ins = Instr::new(
                    if t.multiply_mod { "MUL" } else { "DIV" },
                    vec![target, t_item(a), modifier],
                );
                if !t.multiply_mod && res == FLOOR {
                    ins.name = "FLDIV".into();
                    res = NO_MODE;
                }
                ins
            }
        }
        [a] => {
            let mut ins = Instr::new(arith_name(t.assign_op), vec![target, t_item(a)]);
            if t.modifier != 1.0 {
                ins.flag("itemmod", &modifier);
                if !t.multiply_mod {
                    ins.flag("divmod", true);
                }
            }
            if t.assign_op == Op::Div && fin == FLOOR {
                ins.name = "FLDIV".into();
                fin = NO_MODE;
            }
            ins
        }
        // target = a <op> b
        [a, b, ..] => {
            let op = if t.id_op == Op::Set { Op::Add } else { t.id_op };
            let mut ins = Instr::new(arith_name(op), vec![target, t_item(a), t_item(b)]);
            if op == Op::Div && res == FLOOR {
                ins.name = "FLDIV".into();
                res = NO_MODE;
            }
            if t.assign_op != Op::Set {
                ins.flag("iter", t_op(t.assign_op));
            }
            if t.modifier != 1.0 {
                ins.flag("itemmod", &modifier);
                if !t.multiply_mod {
                    ins.flag("divmod", true);
                }
            }
            ins
        }
    };
    if let Some(v) = t_mode(res) {
        ins.flag("resmode", v);
    }
    if let Some(v) = t_mode(fin) {
        ins.flag("finmode", v);
    }
    if matches!(t.target, Item::MainTime | Item::Attempts) {
        ins.note("TASM can't write to this item");
    }
    ins
}

fn cmp_suffix(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Equals => "E",
        CompareOp::NotEquals => "NE",
        CompareOp::Less => "L",
        CompareOp::LessOrEquals => "LE",
        CompareOp::Greater => "G",
        CompareOp::GreaterOrEquals => "GE",
    }
}

/// `!(a op b)` as `a op' b`.
fn cmp_negate(op: CompareOp) -> CompareOp {
    match op {
        CompareOp::Equals => CompareOp::NotEquals,
        CompareOp::NotEquals => CompareOp::Equals,
        CompareOp::Less => CompareOp::GreaterOrEquals,
        CompareOp::LessOrEquals => CompareOp::Greater,
        CompareOp::Greater => CompareOp::LessOrEquals,
        CompareOp::GreaterOrEquals => CompareOp::Less,
    }
}

/// `a op b` as `b op' a`.
fn cmp_swap(op: CompareOp) -> CompareOp {
    match op {
        CompareOp::Less => CompareOp::Greater,
        CompareOp::LessOrEquals => CompareOp::GreaterOrEquals,
        CompareOp::Greater => CompareOp::Less,
        CompareOp::GreaterOrEquals => CompareOp::LessOrEquals,
        o => o,
    }
}

fn is_literal(c: &CompareOperand) -> bool {
    c.operand_item == Item::Counter(0)
}

/// One side of a compare: its argument, plus its `lmod`/`lmodop`/`lmode` (or `r...`) flags.
fn compare_side(c: &CompareOperand, side: char, ins: &mut Instr) -> String {
    let op = if c.mod_op == Op::Set {
        Op::Mul
    } else {
        c.mod_op
    };
    let arg = if is_literal(c) {
        // GD stores a number literal as the side's modifier
        if op != Op::Mul {
            ins.flag(&format!("{side}modop"), t_op(op));
        }
        fnum(c.modifier)
    } else {
        if c.modifier != 1.0 {
            ins.flag(&format!("{side}mod"), fnum(c.modifier));
        }
        if op != Op::Mul {
            ins.flag(&format!("{side}modop"), t_op(op));
        }
        t_item(c.operand_item)
    };
    if let Some(v) = t_mode((c.rounding, c.sign)) {
        ins.flag(&format!("{side}mode"), v);
    }
    arg
}

fn item_compare(t: &ItemCompareTrigger, names: &Names) -> Instr {
    let (mut lhs, mut rhs, mut op) = (t.lhs, t.rhs, t.compare_op);
    // TASM needs an item on the left
    if is_literal(&lhs) && !is_literal(&rhs) {
        std::mem::swap(&mut lhs, &mut rhs);
        op = cmp_swap(op);
    }
    let (targets, op) = match (t.true_id, t.false_id) {
        (a, b) if a > 0 && b > 0 => (vec![names.group(a), names.group(b)], op),
        (a, _) if a > 0 => (vec![names.group(a)], op),
        (_, b) if b > 0 => (vec![names.group(b)], cmp_negate(op)),
        _ => (vec![], op),
    };
    let fork = targets.len() == 2;
    let mut ins = Instr::new(
        &format!("I{}{}", if fork { "F" } else { "S" }, cmp_suffix(op)),
        targets,
    );
    let l = compare_side(&lhs, 'l', &mut ins);
    let r = compare_side(&rhs, 'r', &mut ins);
    ins.args.extend([l, r]);
    if t.true_id <= 0 && t.false_id <= 0 {
        ins.note("spawns nothing");
    }
    if is_literal(&lhs) {
        ins.note("compares two numbers");
    }
    if t.tolerance != 0.0 {
        ins.note(format!("tolerance {}", fnum(t.tolerance)));
    }
    ins
}

/// TASM hitboxes of a collision / instant collision trigger.
fn hitboxes(c1: i16, c2: i16, p1: bool, p2: bool, both: bool) -> [String; 2] {
    if both {
        return ["COLL_P1".into(), "COLL_P2".into()];
    }
    let a = match (p1, p2) {
        (true, true) => "COLL_P_ANY".to_string(),
        (true, false) => "COLL_P1".into(),
        (false, true) => "COLL_P2".into(),
        _ => format!("b{c1}"),
    };
    [a, format!("b{c2}")]
}

/// The trigger as one TASM instruction.
pub fn tasm(m: &Model, i: usize, names: &Names) -> String {
    let obj = &m.objects[i];
    let unknown = || format!("UNKNOWN object {}", obj.id);
    let g = |g: i16| names.group(g);
    let ins = match obj.id {
        o::ITEM_EDIT_TRIGGER => ItemEditTrigger::from_object(obj).map(|t| item_edit(&t)),
        o::ITEM_COMPARE_TRIGGER => {
            ItemCompareTrigger::from_object(obj).map(|t| item_compare(&t, names))
        }
        o::RANDOM_TRIGGER => {
            RandomTrigger::from_object(obj).map(|t| match (t.target_group1, t.target_group2) {
                (a, b) if a > 0 && b > 0 => Instr::new("IFRAND", vec![g(a), g(b), fnum(t.chance)]),
                (a, _) if a > 0 => Instr::new("ISRAND", vec![g(a), fnum(t.chance)]),
                (_, b) => Instr::new("ISRAND", vec![g(b), fnum(100.0 - t.chance)]),
            })
        }
        o::SPAWN_TRIGGER => SpawnTrigger::from_object(obj).map(|t| {
            let mut ins = Instr::new("SPAWN", vec![g(t.spawn_id)]);
            if (t.delay - 0.0044).abs() > 1e-6 {
                ins.flag("delay", fnum(t.delay));
            }
            if !t.spawn_ordered {
                ins.flag("ordered", false);
            }
            if t.reset_remap {
                ins.flag("noremap", true);
            }
            if !t.spawn_remaps.is_empty() {
                let r: Vec<String> = t
                    .spawn_remaps
                    .iter()
                    .map(|(a, b)| format!("{a}={b}"))
                    .collect();
                ins.flags.push(format!("remap: {{{}}}", r.join(", ")));
            }
            if t.delay_variation != 0.0 {
                ins.note(format!("delay varies by {}", fnum(t.delay_variation)));
            }
            ins
        }),
        o::STOP_TRIGGER => StopTrigger::from_object(obj).map(|t| {
            let name = match t.stop_mode {
                StopMode::Stop => "KILL",
                StopMode::Pause => "PAUSE",
                StopMode::Resume => "RESUME",
            };
            Instr::new(name, vec![g(t.target_group)])
        }),
        o::TOGGLE_TRIGGER => ToggleTrigger::from_object(obj).map(|t| {
            Instr::new(
                if t.activate_group {
                    "TOGGLEON"
                } else {
                    "TOGGLEOFF"
                },
                vec![g(t.target_group)],
            )
        }),
        o::TIME_CONTROL_TRIGGER => TimeControlTrigger::from_object(obj).map(|t| {
            Instr::new(
                if t.stop { "TSTOP" } else { "TSTART" },
                vec![format!("T{}", t.id)],
            )
        }),
        o::TIME_TRIGGER => TimeTrigger::from_object(obj).map(|t| {
            let mut ins = Instr::new(
                "TSPAWN",
                vec![
                    format!("T{}", t.timer_id),
                    fnum(t.start_time),
                    fnum(t.stop_time),
                    g(t.target_group),
                ],
            );
            if t.pause_when_reached {
                ins.flag("tstop", true);
            }
            if t.time_mod != 1.0 {
                ins.flag("tmod", fnum(t.time_mod));
            }
            if t.start_paused {
                ins.flag("tpaused", true);
            }
            if t.dont_override {
                ins.flag("nover", true);
            }
            if t.ignore_timewarp {
                ins.note("ignores timewarp");
            }
            ins
        }),
        o::PERSISTENT_ITEM_SETUP_TRIGGER => PersistentItemTrigger::from_object(obj).and_then(|t| {
            let item = if t.timer {
                format!("T{}", t.item_id)
            } else {
                format!("C{}", t.item_id)
            };
            Some(match (t.target_all, t.persistent, t.reset) {
                (true, _, true) => Instr::new("RPERSALL", vec![]),
                (true, false, _) => Instr::new("UNPERSALL", vec![]),
                // "make every item persistent" has no instruction
                (true, true, false) => return None,
                (false, true, _) => Instr::new("PERS", vec![item]),
                (false, false, _) => Instr::new("UNPERS", vec![item]),
            })
        }),
        o::COUNTER_LABEL => {
            CounterLabel::from_object(obj).map(|t| Instr::new("DISPLAY", vec![t_item(t.item)]))
        }
        o::INSTANT_COLLISION_TRIGGER => InstantCollTrigger::from_object(obj).map(|t| {
            let [a, b] = hitboxes(
                t.collider1,
                t.collider2,
                t.collide_player1,
                t.collide_player2,
                t.collide_both_players,
            );
            Instr::new("INSTCOLL", vec![g(t.true_id), g(t.false_id), a, b])
        }),
        o::COLLISION_TRIGGER => CollisionTrigger::from_object(obj).map(|t| {
            let [a, b] = hitboxes(
                t.collider1,
                t.collider2,
                t.collide_player1,
                t.collide_player2,
                t.collide_both_players,
            );
            let mut ins = Instr::new("COLL", vec![g(t.target_id), a, b]);
            if !t.activate_group {
                ins.note("toggles the group off instead of spawning it");
            }
            if t.on_trigger_exit {
                ins.note("fires when the hitboxes stop colliding");
            }
            ins
        }),
        o::EVENT_TRIGGER => EventTrigger::from_object(obj).and_then(|t| {
            let (first, rest) = t.events.split_first()?;
            let mut ins = Instr::new("EVENT", vec![g(t.target_group), first.to_num().to_string()]);
            if t.extra_id != 0 {
                ins.flag("extra1", t.extra_id);
            }
            if t.extra_id2 as i32 != 0 {
                ins.flag("extra2", t.extra_id2 as i32);
            }
            if !rest.is_empty() {
                let r: Vec<String> = rest.iter().map(|e| e.to_num().to_string()).collect();
                ins.note(format!("also listens for events {}", r.join(", ")));
            }
            Some(ins)
        }),
        _ => None,
    };
    ins.map_or_else(unknown, Instr::render)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdlib::cclocallevels::gdobj::{GDObject, meta::GDObjConfig};

    fn edit(t: ItemEditTrigger) -> String {
        item_edit(
            &ItemEditTrigger::from_object(&GDObject::from_config(&GDObjConfig::new(), t)).unwrap(),
        )
        .render()
    }

    fn base() -> ItemEditTrigger {
        ItemEditTrigger {
            operand1: None,
            operand2: None,
            target: Item::Counter(1),
            modifier: 1.0,
            assign_op: Op::Set,
            multiply_mod: true,
            id_op: Op::Add,
            id_rounding: RoundMode::None,
            result_rounding: RoundMode::None,
            id_sign: SignMode::None,
            result_sign: SignMode::None,
        }
    }

    #[test]
    fn item_edits_become_arithmetic() {
        assert_eq!(
            edit(ItemEditTrigger {
                modifier: 5.0,
                assign_op: Op::Add,
                ..base()
            }),
            "ADD C1, 5"
        );
        assert_eq!(
            edit(ItemEditTrigger {
                modifier: 0.0,
                ..base()
            }),
            "MOV C1, 0"
        );
        assert_eq!(
            edit(ItemEditTrigger {
                operand1: Some(Item::Counter(2)),
                assign_op: Op::Sub,
                ..base()
            }),
            "SUB C1, C2"
        );
        assert_eq!(
            edit(ItemEditTrigger {
                operand1: Some(Item::Counter(2)),
                operand2: Some(Item::Timer(3)),
                id_op: Op::Mul,
                ..base()
            }),
            "MUL C1, C2, T3"
        );
        assert_eq!(
            edit(ItemEditTrigger {
                operand1: Some(Item::Timer(1)),
                target: Item::Points,
                modifier: 2.4,
                multiply_mod: false,
                id_rounding: RoundMode::Floor,
                ..base()
            }),
            "FLDIV POINTS, T1, 2.4"
        );
        assert_eq!(
            edit(ItemEditTrigger {
                operand1: Some(Item::Counter(1)),
                id_sign: SignMode::Absolute,
                ..base()
            }),
            "MOV C1, C1 | resmode:+"
        );
    }

    #[test]
    fn compares_become_instant_branches() {
        let names = Names {
            routines: HashSet::from([4]),
        };
        let t = ItemCompareTrigger {
            true_id: 4,
            false_id: 5,
            lhs: Item::Timer(1).into(),
            rhs: CompareOperand::number_literal(3.0),
            compare_op: CompareOp::LessOrEquals,
            tolerance: 0.0,
        };
        assert_eq!(item_compare(&t, &names).render(), "IFLE group_4, g5, T1, 3");
        let t = ItemCompareTrigger { true_id: 0, ..t };
        assert_eq!(item_compare(&t, &names).render(), "ISG g5, T1, 3");
    }
}
