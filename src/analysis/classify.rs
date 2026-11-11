//! Per-object semantics: which groups a trigger acts on and how ([`classify()`](fn@classify)), and which items
//! it reads or writes ([`item_refs`]).

use super::*;

/// What this object does to which groups.
pub(super) fn classify(obj: &GDObject) -> Vec<(Gid, EdgeKind, String)> {
    use EdgeKind::*;
    let mut out: Vec<(Gid, EdgeKind, String)> = Vec::new();
    let mut push = |prop: u16, kind: EdgeKind, role: &str| {
        if let Some(g) = get_group(obj, prop) {
            out.push((g, kind, role.to_string()));
        }
    };
    // used by the 2.2 "listener" triggers: "activate group" spawns, otherwise the group is toggled off
    let act = if flag(obj, p::ACTIVATE_GROUP) {
        Spawn
    } else {
        ToggleOff
    };

    match obj.id {
        o::SPAWN_TRIGGER => push(p::TARGET_ITEM, Spawn, "spawn"),
        o::RANDOM_TRIGGER => {
            let chance = float(obj, p::DURATION_GROUP_TRIGGER_CHANCE).unwrap_or(0.0);
            push(p::TARGET_ITEM, Spawn, &format!("chance {}", fmt_f(chance)));
            push(p::TARGET_ITEM_2, Spawn, "otherwise");
        }
        o::ADVANCED_RANDOM_TRIGGER => {
            if let Some(GDValue::ProbabilitiesList(list)) =
                obj.get_property(p::RANDOM_PROBABILITIES_LIST)
            {
                let total: i64 = list.iter().map(|(_, w)| (*w).max(0) as i64).sum();
                for (g, w) in list.iter() {
                    if *g > 0 {
                        let pct = if total > 0 {
                            *w as f64 * 100.0 / total as f64
                        } else {
                            0.0
                        };
                        out.push((*g, Spawn, format!("weight {w} = {pct:.0}%")));
                    }
                }
            }
        }
        o::ITEM_COMPARE_TRIGGER | o::INSTANT_COLLISION_TRIGGER => {
            push(p::TARGET_ITEM, Spawn, "if true");
            push(p::TARGET_ITEM_2, Spawn, "if false");
        }
        o::COLLISION_STATE_BLOCK => {
            push(p::TARGET_ITEM, Spawn, "on enter");
            push(p::TARGET_ITEM_2, Spawn, "on exit");
        }
        o::TOGGLE_TRIGGER => {
            if flag(obj, p::ACTIVATE_GROUP) {
                push(p::TARGET_ITEM, ToggleOn, "toggle on");
            } else {
                push(p::TARGET_ITEM, ToggleOff, "toggle off");
            }
        }
        o::TOUCH_TRIGGER => {
            let kind = match obj.get_property(p::TOUCH_TOGGLE_ONOFF) {
                Some(GDValue::TouchToggle(TouchToggle::ToggleOn)) => ToggleOn,
                Some(GDValue::TouchToggle(TouchToggle::ToggleOff)) => ToggleOff,
                _ => Toggle,
            };
            push(p::TARGET_ITEM, kind, "on touch");
        }
        o::COLLISION_TRIGGER => {
            let role = if flag(obj, p::TRIGGER_ON_EXIT) {
                "on collision exit"
            } else {
                "on collision"
            };
            push(p::TARGET_ITEM, act, role);
        }
        o::ON_DEATH_TRIGGER => push(p::TARGET_ITEM, act, "on death"),
        o::COUNT_TRIGGER => push(p::TARGET_ITEM, act, "when count reached"),
        o::INSTANT_COUNT_TRIGGER => push(p::TARGET_ITEM, act, "if count matches"),
        o::PLAYER_TOUCH_TOGGLE_BLOCK => push(p::TARGET_ITEM, act, "on input inside"),
        o::TIME_TRIGGER => push(p::TARGET_ITEM, Spawn, "when timer hits target"),
        o::TIME_EVENT_TRIGGER => push(p::TARGET_ITEM, Spawn, "at time"),
        o::EVENT_TRIGGER => push(p::TARGET_ITEM, Spawn, "on event"),
        o::CHECKPOINT => {
            push(p::TARGET_ITEM, Spawn, "on respawn");
            push(p::TARGET_ITEM_2, Reference, "respawn position");
            push(p::CHECKPOINT_RESPAWN_ID, Reference, "respawn id");
        }
        o::END_TRIGGER => {
            push(p::TARGET_ITEM, Spawn, "on end");
            push(p::TARGET_ITEM_2, Reference, "end position");
        }
        o::STOP_TRIGGER => {
            let (kind, role) = match obj.get_property(p::STOP_MODE) {
                Some(GDValue::StopMode(StopMode::Pause)) => (Pause, "pause"),
                Some(GDValue::StopMode(StopMode::Resume)) => (Resume, "resume"),
                _ => (Stop, "stop"),
            };
            let role = if flag(obj, p::USE_CONTROL_ID) {
                format!("{role} (control id)")
            } else {
                role.into()
            };
            push(p::TARGET_ITEM, kind, &role);
        }
        o::RESET_TRIGGER => push(p::TARGET_ITEM, Reset, "reset"),
        o::MOVE_TRIGGER => {
            push(p::TARGET_ITEM, Affect, "move");
            if flag(obj, p::TARGET_MOVE_MODE) || flag(obj, p::DIRECTIONAL_MOVE_MODE) {
                push(p::TARGET_ITEM_2, Reference, "move toward");
            }
            push(p::CENTER_GROUP_ID, Reference, "center");
        }
        o::ROTATE_TRIGGER => {
            push(p::TARGET_ITEM, Affect, "rotate");
            push(p::TARGET_ITEM_2, Reference, "pivot");
            if flag(obj, p::TARGET_MOVE_MODE) || flag(obj, p::DIRECTIONAL_MOVE_MODE) {
                push(p::ROTATION_TARGET_ID, Reference, "aim at");
            }
            push(p::MINX_ID, Reference, "bound min x");
            push(p::MINY_ID, Reference, "bound min y");
            push(p::MAXX_ID, Reference, "bound max x");
            push(p::MAXY_ID, Reference, "bound max y");
        }
        o::SCALE_TRIGGER => {
            push(p::TARGET_ITEM, Affect, "scale");
            push(p::TARGET_ITEM_2, Reference, "center");
        }
        o::FOLLOW_TRIGGER => {
            push(p::TARGET_ITEM, Affect, "follow");
            push(p::TARGET_ITEM_2, Reference, "leader");
        }
        o::FOLLOW_PLAYER_Y_TRIGGER => push(p::TARGET_ITEM, Affect, "follow player y"),
        o::ALPHA_TRIGGER => push(p::TARGET_ITEM, Affect, "fade"),
        o::ANIMATE_TRIGGER => push(p::TARGET_ITEM, Affect, "animate"),
        o::LINK_VISIBLE_TRIGGER => push(p::TARGET_ITEM, Affect, "link visible"),
        o::PULSE_TRIGGER => {
            if flag(obj, p::PULSE_GROUP) {
                push(p::TARGET_ITEM, Affect, "pulse");
            }
        }
        o::SPAWN_PARTICLE_TRIGGER => {
            push(p::TARGET_ITEM, Reference, "particles");
            push(p::TARGET_ITEM_2, Reference, "spawn at");
        }
        o::TELEPORT_TRIGGER => push(p::TARGET_ITEM, Reference, "teleport to"),
        o::UI_TRIGGER => {
            push(p::TARGET_ITEM, Affect, "ui element");
            push(p::TARGET_ITEM_2, Reference, "ui reference");
        }
        // item triggers whose TARGET_ITEM is an item, not a group
        o::ITEM_EDIT_TRIGGER
        | o::PERSISTENT_ITEM_SETUP_TRIGGER
        | o::PICKUP_TRIGGER
        | o::COUNTER_LABEL => {}
        _ => {
            // unknown semantics: report every group-typed parameter as a plain reference
            for &prop in GROUP_PROPERTY_IDS.iter() {
                if prop == p::GROUPS || prop == p::PARENT_GROUPS {
                    continue;
                }
                let name = property_name(prop).to_lowercase();
                push(prop, Reference, &name);
            }
        }
    }
    out
}

/// Item reads/writes of this object.
pub(super) fn item_refs(obj: &GDObject) -> Vec<(ItemKey, Access)> {
    use Access::*;
    let mut v = Vec::new();
    let mut add = |ns: ItemNs, id: Option<i16>, a: Access| {
        let special = matches!(ns, ItemNs::Points | ItemNs::MainTime | ItemNs::Attempts);
        match (special, id) {
            (true, _) => v.push(((ns, 0), a)),
            (false, Some(id)) => v.push(((ns, id), a)),
            _ => {}
        }
    };
    match obj.id {
        o::PICKUP_TRIGGER => add(ItemNs::Counter, get_item(obj, p::INPUT_ITEM_1), Write),
        o::ITEM_EDIT_TRIGGER => {
            add(
                item_ns(obj, p::TARGET_ITEM_TYPE),
                get_item(obj, p::TARGET_ITEM),
                Write,
            );
            if get_item(obj, p::INPUT_ITEM_1).is_some() {
                add(
                    item_ns(obj, p::FIRST_ITEM_TYPE),
                    get_item(obj, p::INPUT_ITEM_1),
                    Read,
                );
            }
            if get_item(obj, p::INPUT_ITEM_2).is_some() {
                add(
                    item_ns(obj, p::SECOND_ITEM_TYPE),
                    get_item(obj, p::INPUT_ITEM_2),
                    Read,
                );
            }
        }
        o::ITEM_COMPARE_TRIGGER => {
            add(
                item_ns(obj, p::FIRST_ITEM_TYPE),
                get_item(obj, p::INPUT_ITEM_1),
                Read,
            );
            add(
                item_ns(obj, p::SECOND_ITEM_TYPE),
                get_item(obj, p::INPUT_ITEM_2),
                Read,
            );
        }
        o::COUNT_TRIGGER | o::INSTANT_COUNT_TRIGGER => {
            add(ItemNs::Counter, get_item(obj, p::INPUT_ITEM_1), Read)
        }
        o::COUNTER_LABEL => {
            let ns = if flag(obj, p::IS_TIMER) {
                ItemNs::Timer
            } else {
                ItemNs::Counter
            };
            add(ns, get_item(obj, p::INPUT_ITEM_1), Read);
        }
        o::PERSISTENT_ITEM_SETUP_TRIGGER => {
            let ns = if flag(obj, p::TIMER) {
                ItemNs::Timer
            } else {
                ItemNs::Counter
            };
            add(ns, get_item(obj, p::TARGET_ITEM), Write);
        }
        o::TIME_TRIGGER | o::TIME_CONTROL_TRIGGER => {
            add(ItemNs::Timer, get_item(obj, p::INPUT_ITEM_1), Write)
        }
        o::TIME_EVENT_TRIGGER => add(ItemNs::Timer, get_item(obj, p::INPUT_ITEM_1), Read),
        o::COLLISION_BLOCK => add(ItemNs::Collision, get_item(obj, p::INPUT_ITEM_1), Define),
        o::COLLISION_TRIGGER | o::INSTANT_COLLISION_TRIGGER => {
            add(ItemNs::Collision, get_item(obj, p::INPUT_ITEM_1), Read);
            add(ItemNs::Collision, get_item(obj, p::INPUT_ITEM_2), Read);
        }
        _ => {}
    }
    v
}

pub(super) fn is_player_object(id: i32) -> bool {
    matches!(
        id,
        o::PLAYER_TOUCH_TOGGLE_BLOCK | o::COLLISION_STATE_BLOCK | o::CHECKPOINT
    )
}
