//! Typed decoding of objects through gdlib's `ObjectProperties::from_object`, plus a short
//! human-readable summary of what each trigger does ("t1 <= points ? g4 : g5").
//!
//! [`typed`] is what the inspector shows; [`summary`] is the one-line text drawn in the detailed
//! graph / map views.

use gdlib::cclocallevels::gdobj::{
    GDObject, ObjectProperties,
    constructors::{SpinningObjectConfig, TextObject},
    ids::objects as o,
    structs::{
        CompareOp, CompareOperand, InstantCountComparison, Item, MoveEasing, MoveLock, MoveMode,
        MoveTarget, Op, OptionalPlayerTarget, PickupTriggerMode, PulseMode, PulseTarget,
        RotationMode, RoundMode, SignMode, StopMode, TouchToggle,
    },
    *,
};

/// Name used for an item everywhere in the UI: `c3`, `t1`, `points`, `MainTime`, `attempts`.
pub fn item_name(i: Item) -> String {
    match i {
        Item::Counter(n) => format!("c{n}"),
        Item::Timer(n) => format!("t{n}"),
        Item::Points => "points".into(),
        Item::MainTime => "MainTime".into(),
        Item::Attempts => "attempts".into(),
    }
}

/// Short float: `2.4`, `1`, `-0.25`.
pub fn num(f: f32) -> String {
    let s = format!("{f:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

fn op_sym(op: Op) -> &'static str {
    match op {
        Op::Set => "=",
        Op::Add => "+",
        Op::Sub => "-",
        Op::Mul => "*",
        Op::Div => "/",
    }
}

fn cmp_sym(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Equals => "==",
        CompareOp::Greater => ">",
        CompareOp::GreaterOrEquals => ">=",
        CompareOp::Less => "<",
        CompareOp::LessOrEquals => "<=",
        CompareOp::NotEquals => "!=",
    }
}

/// Wraps `expr` in the rounding and sign functions GD applies (`abs(floor(expr))`).
fn wrap(expr: String, round: RoundMode, sign: SignMode) -> String {
    let e = match round {
        RoundMode::None => expr,
        RoundMode::Nearest => format!("round({expr})"),
        RoundMode::Floor => format!("floor({expr})"),
        RoundMode::Ceiling => format!("ceil({expr})"),
    };
    match sign {
        SignMode::None => e,
        SignMode::Absolute => format!("abs({e})"),
        SignMode::Negative => format!("-abs({e})"),
    }
}

/// Strips one pair of redundant outer parentheses.
fn bare(s: String) -> String {
    if s.starts_with('(') && s.ends_with(')') {
        let inner = &s[1..s.len() - 1];
        // only if the parentheses actually match each other
        let mut depth = 0;
        if inner.chars().all(|c| {
            depth += (c == '(') as i32 - (c == ')') as i32;
            depth >= 0
        }) {
            return inner.to_string();
        }
    }
    s
}

fn grp(g: i16) -> String {
    if g > 0 { format!("g{g}") } else { "-".into() }
}

fn ease(e: MoveEasing) -> String {
    if e == MoveEasing::None {
        String::new()
    } else {
        format!(" {e:?}")
    }
}

fn compare_operand(c: &CompareOperand) -> String {
    let literal = matches!(c.operand_item, Item::Counter(0));
    let mut e = if literal {
        num(c.modifier)
    } else {
        item_name(c.operand_item)
    };
    if !literal {
        let op = if c.mod_op == Op::Set {
            Op::Mul
        } else {
            c.mod_op
        };
        let identity = match op {
            Op::Mul | Op::Div => c.modifier == 1.0,
            _ => c.modifier == 0.0,
        };
        if !identity {
            e = format!("({e} {} {})", op_sym(op), num(c.modifier));
        }
    }
    let wrapped = wrap(e.clone(), c.rounding, c.sign);
    if wrapped == e { bare(e) } else { wrapped }
}

fn item_edit(t: &ItemEditTrigger) -> String {
    let operands = match (t.operand1, t.operand2) {
        (None, None) => None,
        (Some(a), None) => Some(item_name(a)),
        (None, Some(b)) => Some(item_name(b)),
        (Some(a), Some(b)) => Some(format!(
            "({} {} {})",
            item_name(a),
            op_sym(t.id_op),
            item_name(b)
        )),
    };
    let value = match operands {
        None => num(t.modifier),
        Some(e) if t.modifier == 1.0 => e,
        Some(e) => format!(
            "{e} {} {}",
            if t.multiply_mod { "*" } else { "/" },
            num(t.modifier)
        ),
    };
    let value = wrap(value, t.id_rounding, t.id_sign);
    let target = item_name(t.target);
    if t.assign_op == Op::Set {
        format!("{target} = {}", bare(value))
    } else if t.result_rounding == RoundMode::None && t.result_sign == SignMode::None {
        format!("{target} {}= {}", op_sym(t.assign_op), bare(value))
    } else {
        let inner = format!("{target} {} {value}", op_sym(t.assign_op));
        format!(
            "{target} = {}",
            wrap(inner, t.result_rounding, t.result_sign)
        )
    }
}

fn spawn_or_toggle(activate: bool, g: i16) -> String {
    if activate {
        format!("spawn {}", grp(g))
    } else {
        format!("toggle off {}", grp(g))
    }
}

fn colliders(c1: i16, c2: i16, p1: bool, p2: bool, both: bool) -> String {
    let a = if both {
        "P1&P2".to_string()
    } else if p1 {
        "P1".into()
    } else if p2 {
        "P2".into()
    } else {
        format!("coll{c1}")
    };
    format!("{a} hits coll{c2}")
}

/// Compact single-line Debug of a struct without its type name: `strength: 1.0, duration: 0.5`.
fn compact(dbg: String) -> String {
    let body = match (dbg.find('{'), dbg.rfind('}')) {
        (Some(a), Some(b)) if a < b => dbg[a + 1..b].trim().to_string(),
        _ => dbg,
    };
    if body.chars().count() > 90 {
        body.chars().take(87).collect::<String>() + "..."
    } else {
        body
    }
}

/// Decodes `obj` with every gdlib constructor that implements `from_object`.
/// Returns the struct's name and its pretty-printed (`{:#?}`) contents.
pub fn typed(obj: &GDObject) -> Option<(&'static str, String)> {
    macro_rules! try_all {
        ($($t:ident),* $(,)?) => {
            $(
                if let Some(v) = $t::from_object(obj) {
                    return Some((stringify!($t), format!("{v:#?}")));
                }
            )*
        };
    }
    try_all!(
        MoveTrigger,
        RotateTrigger,
        ScaleTrigger,
        ColourTrigger,
        PulseTrigger,
        AlphaTrigger,
        ToggleTrigger,
        StopTrigger,
        GroupResetTrigger,
        SpawnTrigger,
        RandomTrigger,
        AdvancedRandomTrigger,
        ItemEditTrigger,
        ItemCompareTrigger,
        PersistentItemTrigger,
        PickupTrigger,
        InstantCountTrigger,
        CountTrigger,
        CounterLabel,
        CollisionBlock,
        CollisionTrigger,
        InstantCollTrigger,
        ToggleBlock,
        StateBlock,
        OnDeathTrigger,
        TimeTrigger,
        TimeControlTrigger,
        TimeEventTrigger,
        EventTrigger,
        TouchTrigger,
        FollowTrigger,
        FollowPlayerYTrigger,
        AnimateTrigger,
        LinkVisibleTrigger,
        UIConfigTrigger,
        ParticleSpawnTrigger,
        TeleportTrigger,
        EndTrigger,
        TimewarpTrigger,
        ShakeTrigger,
        BGSpeedTrigger,
        MGSpeedTrigger,
        PlayerControlTrigger,
        GravityTrigger,
        CameraZoomTrigger,
        CameraGuide,
        TransitionTrigger,
        MiddleGroundTrigger,
        MiddleGroundConfigTrigger,
        AreaStopTrigger,
        BPMGuide,
        RotateGameplayTrigger,
        StartposConfig,
        TextObject,
        GDPad,
        SpinningObjectConfig,
    );
    None
}

/// One line describing what this object does, from its typed gdlib struct. `None` if gdlib has
/// no constructor for it.
pub fn summary(obj: &GDObject) -> Option<String> {
    let s = match obj.id {
        o::ITEM_EDIT_TRIGGER => item_edit(&ItemEditTrigger::from_object(obj)?),
        o::ITEM_COMPARE_TRIGGER => {
            let t = ItemCompareTrigger::from_object(obj)?;
            let tol = if t.tolerance != 0.0 {
                format!(" ±{}", num(t.tolerance))
            } else {
                String::new()
            };
            format!(
                "{} {} {}{tol} ? {} : {}",
                compare_operand(&t.lhs),
                cmp_sym(t.compare_op),
                compare_operand(&t.rhs),
                grp(t.true_id),
                grp(t.false_id)
            )
        }
        o::INSTANT_COUNT_TRIGGER => {
            let t = InstantCountTrigger::from_object(obj)?;
            let op = match t.comparison {
                InstantCountComparison::Equals => "==",
                InstantCountComparison::Larger => ">",
                InstantCountComparison::Smaller => "<",
            };
            format!(
                "c{} {op} {} ? {}",
                t.item_id,
                t.target_count,
                spawn_or_toggle(t.activate_group, t.target_group)
            )
        }
        o::COUNT_TRIGGER => {
            let t = CountTrigger::from_object(obj)?;
            let multi = if t.multi_activate { " (multi)" } else { "" };
            format!(
                "when c{} == {}: {}{multi}",
                t.item_id,
                t.target_count,
                spawn_or_toggle(t.activate_group, t.target_id)
            )
        }
        o::PICKUP_TRIGGER => {
            let t = PickupTrigger::from_object(obj)?;
            if t.override_value {
                format!("c{} = {}", t.item_id, t.count)
            } else {
                match t.mode {
                    PickupTriggerMode::Add => format!("c{} += {}", t.item_id, t.count),
                    PickupTriggerMode::Multiply => format!("c{} *= {}", t.item_id, num(t.modifier)),
                    PickupTriggerMode::Divide => format!("c{} /= {}", t.item_id, num(t.modifier)),
                }
            }
        }
        o::PERSISTENT_ITEM_SETUP_TRIGGER => {
            let t = PersistentItemTrigger::from_object(obj)?;
            let item = if t.target_all {
                "all items".to_string()
            } else {
                format!("{}{}", if t.timer { "t" } else { "c" }, t.item_id)
            };
            let what = match (t.persistent, t.reset) {
                (_, true) => "reset",
                (true, _) => "persist",
                _ => "unpersist",
            };
            format!("{what} {item}")
        }
        o::RANDOM_TRIGGER => {
            let t = RandomTrigger::from_object(obj)?;
            format!(
                "{}%: {} / else {}",
                num(t.chance),
                grp(t.target_group1),
                grp(t.target_group2)
            )
        }
        o::ADVANCED_RANDOM_TRIGGER => {
            let t = AdvancedRandomTrigger::from_object(obj)?;
            let total: i64 = t
                .probabilities
                .iter()
                .map(|(_, w)| (*w).max(0) as i64)
                .sum();
            let parts: Vec<String> = t
                .probabilities
                .iter()
                .map(|(g, w)| {
                    format!(
                        "{} {:.0}%",
                        grp(*g),
                        if total > 0 {
                            *w as f64 * 100.0 / total as f64
                        } else {
                            0.0
                        }
                    )
                })
                .collect();
            format!("pick: {}", parts.join(", "))
        }
        o::SPAWN_TRIGGER => {
            let t = SpawnTrigger::from_object(obj)?;
            let mut s = format!("spawn {}", grp(t.spawn_id));
            if t.delay != 0.0 {
                s += &format!(" after {}s", num(t.delay));
                if t.delay_variation != 0.0 {
                    s += &format!(" ±{}", num(t.delay_variation));
                }
            }
            if t.spawn_ordered {
                s += " ordered";
            }
            if !t.spawn_remaps.is_empty() {
                let r: Vec<String> = t
                    .spawn_remaps
                    .iter()
                    .map(|(a, b)| format!("{a}->{b}"))
                    .collect();
                s += &format!(" remap[{}]", r.join(","));
            }
            s
        }
        o::TOGGLE_TRIGGER => {
            let t = ToggleTrigger::from_object(obj)?;
            format!(
                "{} {}",
                grp(t.target_group),
                if t.activate_group { "on" } else { "off" }
            )
        }
        o::STOP_TRIGGER => {
            let t = StopTrigger::from_object(obj)?;
            let verb = match t.stop_mode {
                StopMode::Stop => "stop",
                StopMode::Pause => "pause",
                StopMode::Resume => "resume",
            };
            format!(
                "{verb} {}{}",
                grp(t.target_group),
                if t.use_control_id {
                    " (control id)"
                } else {
                    ""
                }
            )
        }
        o::RESET_TRIGGER => format!(
            "reset {}",
            grp(GroupResetTrigger::from_object(obj)?.target_group)
        ),
        o::ALPHA_TRIGGER => {
            let t = AlphaTrigger::from_object(obj)?;
            format!(
                "{} alpha -> {} in {}s",
                grp(t.target_group),
                num(t.opacity),
                num(t.fade_time)
            )
        }
        o::MOVE_TRIGGER => {
            let t = MoveTrigger::from_object(obj)?;
            let lock = |l: Option<MoveLock>| match l {
                Some(MoveLock::Player) => "*player",
                Some(MoveLock::Camera) => "*camera",
                None => "",
            };
            let what = match &t.move_config {
                MoveMode::Default(d) => format!(
                    "by ({}{}, {}{})",
                    num(d.dx),
                    lock(d.x_lock),
                    num(d.dy),
                    lock(d.y_lock)
                ),
                MoveMode::Targeting(tm) => match tm.target_group_id {
                    MoveTarget::Group(g) => format!("to g{g}"),
                    other => format!("to {other:?}"),
                },
                MoveMode::Directional(dm) => match dm.target_group_id {
                    MoveTarget::Group(g) => format!("toward g{g} by {}", dm.distance),
                    other => format!("toward {other:?} by {}", dm.distance),
                },
            };
            format!(
                "move {} {what} in {}s{}{}",
                grp(t.target_group),
                num(t.time),
                ease(t.easing.easing),
                if t.silent { " silent" } else { "" }
            )
        }
        o::ROTATE_TRIGGER => {
            let t = RotateTrigger::from_object(obj)?;
            let what = match &t.rotation_mode {
                RotationMode::Default(r) => {
                    format!("by {}°", num(r.degrees + 360.0 * r.x360 as f32))
                }
                RotationMode::Aim(a) => format!(
                    "aim at {}",
                    a.player_target
                        .map_or(grp(a.aim_target), |p| format!("{p:?}"))
                ),
                RotationMode::Follow(a) => format!(
                    "follow {}",
                    a.player_target
                        .map_or(grp(a.aim_target), |p| format!("{p:?}"))
                ),
            };
            format!(
                "rotate {} {what} around {} in {}s{}",
                grp(t.target_group),
                grp(t.center_group_id),
                num(t.move_time),
                ease(t.easing.easing)
            )
        }
        o::SCALE_TRIGGER => {
            let t = ScaleTrigger::from_object(obj)?;
            let c = &t.scale_config;
            format!(
                "scale {} x({}, {}) around {} in {}s",
                grp(t.target_group),
                num(c.x_scale),
                num(c.y_scale),
                grp(t.center_group_id),
                num(t.duration)
            )
        }
        o::COLOR_TRIGGER => {
            let t = ColourTrigger::from_object(obj)?;
            let src = match &t.copy_colour {
                Some(c) => format!("copy ch {}", i16::from(c.original_ch)),
                None => format!(
                    "#{:02x}{:02x}{:02x}",
                    t.colour.red, t.colour.green, t.colour.blue
                ),
            };
            format!(
                "ch {} = {src} in {}s",
                i16::from(t.channel),
                num(t.fade_time)
            )
        }
        o::PULSE_TRIGGER => {
            let t = PulseTrigger::from_object(obj)?;
            let target = match &t.pulse_target {
                PulseTarget::Group(g) => grp(g.group_id),
                PulseTarget::Channel(c) => format!("ch {}", c.channel_id),
            };
            let col = match &t.pulse_mode {
                PulseMode::Colour(c) => format!("#{:02x}{:02x}{:02x}", c.red, c.green, c.blue),
                PulseMode::HSV(h) => format!("hsv {}", h.hsv_config),
            };
            format!(
                "pulse {target} {col} {}/{}/{}s",
                num(t.pulse_fade_in_time),
                num(t.pulse_hold_time),
                num(t.pulse_fade_out_time)
            )
        }
        o::FOLLOW_TRIGGER => {
            let t = FollowTrigger::from_object(obj)?;
            format!(
                "{} follows {} x({}, {}) for {}s",
                grp(t.target_group),
                grp(t.follow_group),
                num(t.x_mod),
                num(t.y_mod),
                num(t.follow_time)
            )
        }
        o::FOLLOW_PLAYER_Y_TRIGGER => {
            let t = FollowPlayerYTrigger::from_object(obj)?;
            format!(
                "{} follows player y (speed {}, delay {}) for {}s",
                grp(t.target_group),
                num(t.speed),
                num(t.delay),
                num(t.move_time)
            )
        }
        o::ANIMATE_TRIGGER => {
            let t = AnimateTrigger::from_object(obj)?;
            format!("animate {} {:?}", grp(t.target_group), t.animation)
        }
        o::COLLISION_TRIGGER => {
            let t = CollisionTrigger::from_object(obj)?;
            let when = if t.on_trigger_exit {
                "stops hitting"
            } else {
                "hits"
            };
            let c = colliders(
                t.collider1,
                t.collider2,
                t.collide_player1,
                t.collide_player2,
                t.collide_both_players,
            )
            .replacen(" hits ", &format!(" {when} "), 1);
            format!(
                "when {c}: {}",
                spawn_or_toggle(t.activate_group, t.target_id)
            )
        }
        o::INSTANT_COLLISION_TRIGGER => {
            let t = InstantCollTrigger::from_object(obj)?;
            let c = colliders(
                t.collider1,
                t.collider2,
                t.collide_player1,
                t.collide_player2,
                t.collide_both_players,
            );
            format!("{c} ? {} : {}", grp(t.true_id), grp(t.false_id))
        }
        o::COLLISION_BLOCK => {
            let t = CollisionBlock::from_object(obj)?;
            format!(
                "collider coll{}{}",
                t.id,
                if t.dynamic { " (dynamic)" } else { "" }
            )
        }
        o::COLLISION_STATE_BLOCK => {
            let t = StateBlock::from_object(obj)?;
            format!(
                "enter: spawn {} / exit: spawn {}",
                grp(t.state_on),
                grp(t.state_off)
            )
        }
        o::PLAYER_TOUCH_TOGGLE_BLOCK => {
            let t = ToggleBlock::from_object(obj)?;
            format!(
                "on input: {}",
                spawn_or_toggle(t.activate_group, t.target_group)
            )
        }
        o::ON_DEATH_TRIGGER => {
            let t = OnDeathTrigger::from_object(obj)?;
            format!(
                "on death: {}",
                spawn_or_toggle(t.activate_group, t.target_group)
            )
        }
        o::TOUCH_TRIGGER => {
            let t = TouchTrigger::from_object(obj)?;
            let verb = match t.toggle {
                TouchToggle::None => "toggle",
                TouchToggle::ToggleOn => "toggle on",
                TouchToggle::ToggleOff => "toggle off",
            };
            let who = match t.target_player {
                OptionalPlayerTarget::None => "",
                OptionalPlayerTarget::Player1 => " (P1)",
                OptionalPlayerTarget::Player2 => " (P2)",
            };
            let hold = if t.hold_mode { " hold" } else { "" };
            format!("on touch{who}{hold}: {verb} {}", grp(t.target_group))
        }
        o::TIME_TRIGGER => {
            let t = TimeTrigger::from_object(obj)?;
            let mut s = format!("t{} from {}", t.timer_id, num(t.start_time));
            if t.time_mod != 1.0 {
                s += &format!(" x{}", num(t.time_mod));
            }
            if t.target_group > 0 || t.stop_time != 0.0 {
                s += &format!("; at {}: spawn {}", num(t.stop_time), grp(t.target_group));
            }
            if t.start_paused {
                s += " (paused)";
            }
            s
        }
        o::TIME_CONTROL_TRIGGER => {
            let t = TimeControlTrigger::from_object(obj)?;
            format!("{} t{}", if t.stop { "stop" } else { "start" }, t.id)
        }
        o::TIME_EVENT_TRIGGER => {
            let t = TimeEventTrigger::from_object(obj)?;
            format!(
                "when t{} hits {}: spawn {}{}",
                t.id,
                num(t.target_time),
                grp(t.target_group),
                if t.multi_activate { " (multi)" } else { "" }
            )
        }
        o::EVENT_TRIGGER => {
            let t = EventTrigger::from_object(obj)?;
            let ev: Vec<String> = t.events.iter().map(|e| format!("{e:?}")).collect();
            format!(
                "on {}: spawn {}",
                if ev.is_empty() {
                    "?".into()
                } else {
                    ev.join("|")
                },
                grp(t.target_group)
            )
        }
        o::COUNTER_LABEL => format!("shows {}", item_name(CounterLabel::from_object(obj)?.item)),
        o::END_TRIGGER => {
            let t = EndTrigger::from_object(obj)?;
            let mut s = "end level".to_string();
            if let Some(g) = t.spawn_id.filter(|g| *g > 0) {
                s += &format!(", spawn g{g}");
            }
            if t.instant {
                s += " (instant)";
            }
            s
        }
        o::TIMEWARP_TRIGGER => format!(
            "time x{}",
            num(TimewarpTrigger::from_object(obj)?.time_scale)
        ),
        o::LINK_VISIBLE_TRIGGER => format!(
            "link visibility of {}",
            grp(LinkVisibleTrigger::from_object(obj)?.target_group)
        ),
        o::SHAKE_TRIGGER => {
            let t = ShakeTrigger::from_object(obj)?;
            format!("shake {} for {}s", num(t.strength), num(t.duration))
        }
        _ => {
            let (_, dbg) = typed(obj)?;
            // the pretty form, folded onto one line
            compact(
                dbg.split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .replace("{ ", "{")
                    .replace(" }", "}"),
            )
        }
    };
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdlib::cclocallevels::gdobj::{meta::GDObjConfig, structs::CompareOperand};

    #[test]
    fn item_edit_reads_like_an_expression() {
        let t = ItemEditTrigger {
            operand1: Some(Item::Timer(1)),
            operand2: None,
            target: Item::MainTime,
            modifier: 2.4,
            assign_op: Op::Set,
            multiply_mod: false,
            id_op: Op::Add,
            id_rounding: RoundMode::Floor,
            result_rounding: RoundMode::None,
            id_sign: SignMode::Absolute,
            result_sign: SignMode::None,
        };
        let obj = GDObject::from_config(&GDObjConfig::new(), t);
        assert_eq!(summary(&obj).unwrap(), "MainTime = abs(floor(t1 / 2.4))");
    }

    #[test]
    fn item_compare_reads_like_a_condition() {
        let t = ItemCompareTrigger {
            true_id: 4,
            false_id: 5,
            lhs: Item::Timer(1).into(),
            rhs: Item::Points.into(),
            compare_op: CompareOp::LessOrEquals,
            tolerance: 0.0,
        };
        let obj = GDObject::from_config(&GDObjConfig::new(), t);
        assert_eq!(summary(&obj).unwrap(), "t1 <= points ? g4 : g5");
        let t = ItemCompareTrigger {
            rhs: CompareOperand::number_literal(3.0),
            ..ItemCompareTrigger::from_object(&obj).unwrap()
        };
        let obj = GDObject::from_config(&GDObjConfig::new(), t);
        assert_eq!(summary(&obj).unwrap(), "t1 <= 3 ? g4 : g5");
    }
}
