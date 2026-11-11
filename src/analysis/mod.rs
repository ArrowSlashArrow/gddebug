//! Turns a flat list of `GDObject`s into a "program": triggers (instructions), groups (functions /
//! data), the edges between groups, item cross-references and diagnostics.
//!
//! Terminology used throughout:
//! * **action**: one thing a single trigger does to one group (spawn g5, move g7, ...).
//! * **edge**: actions aggregated at the group level (`from -> to` by `kind`). The source of an
//!   action is every group of the trigger if it is spawn-triggered (spawning any of its groups runs
//!   it), otherwise the pseudo-group [`ROOT`] (the level itself: x-position, touch, or player contact).
//!
//! Layout: the data types live here; [`build`] constructs a [`Model`] using [`classify()`](fn@classify) and the
//! property readers in [`props`]; [`lints`] adds diagnostics; [`queries`] answers questions
//! about the result.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use gdlib::cclocallevels::gdobj::{
    GDObject, ObjectProperties,
    ids::{metadata::GROUP_PROPERTY_IDS, objects as o, properties as p},
    structs::{
        GDValue, ItemType, MoveLock, MoveMode, MoveTarget, RotationMode, StopMode, TouchToggle,
    },
    {ColourTrigger, MoveTrigger, RotateTrigger},
};
use gdlib::cclocallevels::properties::{OBJECT_NAMES, PROPERTY_TABLE};
mod build;
mod classify;
mod lints;
mod props;
mod queries;

use classify::{classify, is_player_object, item_refs};
pub use props::{flag, float, fmt_value, get_group, property_name};
use props::{fmt_f, get_item, item_ns, mnemonic};

/// Group ID. Real groups are 1..=9999.
pub type Gid = i16;

/// Pseudo-group representing the level itself (player position, touch, contact).
pub const ROOT: Gid = 0;

pub fn group_label(g: Gid) -> String {
    if g == ROOT {
        "LEVEL".to_string()
    } else {
        format!("g{g}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EdgeKind {
    /// Runs the spawn-triggerable triggers in the target group.
    Spawn,
    ToggleOn,
    ToggleOff,
    /// Alternates on/off (touch trigger default mode).
    Toggle,
    Stop,
    Pause,
    Resume,
    Reset,
    /// Changes the objects of the target group (move, rotate, alpha, ...).
    Affect,
    /// Uses the target group as a parameter (center, pivot, destination, ...).
    Reference,
}

impl EdgeKind {
    pub const ALL: [EdgeKind; 10] = [
        Self::Spawn,
        Self::ToggleOn,
        Self::ToggleOff,
        Self::Toggle,
        Self::Stop,
        Self::Pause,
        Self::Resume,
        Self::Reset,
        Self::Affect,
        Self::Reference,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Spawn => "spawn",
            Self::ToggleOn => "toggle on",
            Self::ToggleOff => "toggle off",
            Self::Toggle => "toggle",
            Self::Stop => "stop",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Reset => "reset",
            Self::Affect => "affect",
            Self::Reference => "reference",
        }
    }

    /// RGB colour shared by the GUI and the DOT output.
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Spawn => (80, 200, 120),
            Self::ToggleOn | Self::Toggle => (90, 170, 255),
            Self::ToggleOff => (150, 120, 255),
            Self::Stop | Self::Pause | Self::Resume | Self::Reset => (240, 90, 90),
            Self::Affect => (240, 180, 60),
            Self::Reference => (150, 150, 150),
        }
    }
}

/// Flowchart role of a logic object, used to pick node shapes. Ordered by how much the role
/// matters to control flow: a group's shape comes from the highest-ranked role it contains.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FlowKind {
    /// Changes objects (move, rotate, alpha, colour, ...). Flowchart "process": rectangle.
    Process,
    /// Runs or toggles other groups (spawn, toggle, touch). "Predefined process": barred rectangle.
    Call,
    /// Sets or displays an item / collider. "Data" (input/output): parallelogram.
    Item,
    /// Waits for a condition, then spawns (count, collision, on death, time event, ...). "Delay": D shape.
    Event,
    /// Ends the level. "Terminator": pill.
    Terminal,
    /// Stops, pauses, resumes or resets groups. Circle.
    Stop,
    /// Picks one of several branches (random, item compare, instant count, ...). "Decision": rhombus.
    Decision,
}

impl FlowKind {
    /// Most significant first (legend order).
    pub const ALL: [FlowKind; 7] = [
        Self::Decision,
        Self::Stop,
        Self::Terminal,
        Self::Event,
        Self::Item,
        Self::Call,
        Self::Process,
    ];

    pub fn of(obj_id: i32) -> Self {
        match obj_id {
            o::RANDOM_TRIGGER
            | o::ADVANCED_RANDOM_TRIGGER
            | o::ITEM_COMPARE_TRIGGER
            | o::INSTANT_COUNT_TRIGGER
            | o::INSTANT_COLLISION_TRIGGER => Self::Decision,
            o::STOP_TRIGGER | o::RESET_TRIGGER => Self::Stop,
            o::END_TRIGGER => Self::Terminal,
            o::COUNT_TRIGGER
            | o::COLLISION_TRIGGER
            | o::ON_DEATH_TRIGGER
            | o::TIME_EVENT_TRIGGER
            | o::EVENT_TRIGGER
            | o::COLLISION_STATE_BLOCK
            | o::PLAYER_TOUCH_TOGGLE_BLOCK
            | o::CHECKPOINT => Self::Event,
            o::ITEM_EDIT_TRIGGER
            | o::PICKUP_TRIGGER
            | o::PERSISTENT_ITEM_SETUP_TRIGGER
            | o::TIME_TRIGGER
            | o::TIME_CONTROL_TRIGGER
            | o::COUNTER_LABEL
            | o::COLLISION_BLOCK => Self::Item,
            o::SPAWN_TRIGGER | o::TOGGLE_TRIGGER | o::TOUCH_TRIGGER => Self::Call,
            _ => Self::Process,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Decision => "decision / branch",
            Self::Stop => "stop / pause / reset",
            Self::Terminal => "end of level",
            Self::Event => "wait for event",
            Self::Item => "set item / data",
            Self::Call => "spawn / toggle",
            Self::Process => "process (move, colour, ...)",
        }
    }

    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Decision => (200, 120, 255),
            Self::Stop => (240, 90, 90),
            Self::Terminal => (230, 230, 230),
            Self::Event => (240, 150, 60),
            Self::Item => (60, 210, 200),
            Self::Call => (80, 200, 120),
            Self::Process => (100, 150, 240),
        }
    }
}

/// How a trigger gets run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// Not a logic object.
    None,
    /// Spawn-triggered: runs when one of its groups is spawned.
    Spawn,
    /// Touch-triggered: runs when the player touches it.
    Touch,
    /// Runs when the player passes its x-position.
    Position,
    /// Acts on player contact/input (toggle block, state block, checkpoint).
    Player,
}

impl Activation {
    pub fn tag(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Spawn => "spawn",
            Self::Touch => "touch",
            Self::Position => "x-pos",
            Self::Player => "player",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Action {
    pub trigger: usize,
    pub target: Gid,
    pub kind: EdgeKind,
    pub role: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemNs {
    Counter,
    Timer,
    Collision,
    Points,
    MainTime,
    Attempts,
}

pub type ItemKey = (ItemNs, i16);

pub fn item_label((ns, id): ItemKey) -> String {
    match ns {
        ItemNs::Counter => format!("c{id}"),
        ItemNs::Timer => format!("t{id}"),
        ItemNs::Collision => format!("coll{id}"),
        ItemNs::Points => "points".into(),
        ItemNs::MainTime => "MainTime".into(),
        ItemNs::Attempts => "attempts".into(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    /// A collision block declaring a collider ID.
    Define,
}

#[derive(Clone, Debug)]
pub struct ItemRef {
    pub obj: usize,
    pub access: Access,
}

#[derive(Clone, Debug, Default)]
pub struct GroupInfo {
    pub id: Gid,
    /// Indices of every object in this group.
    pub members: Vec<usize>,
    /// Subset of `members` that are logic objects.
    pub triggers: Vec<usize>,
    /// Actions that target this group.
    pub incoming: Vec<usize>,
    /// Edge indices.
    pub out_edges: Vec<usize>,
    pub in_edges: Vec<usize>,
    pub centroid: Option<(f32, f32)>,
}

impl GroupInfo {
    pub fn has_logic(&self) -> bool {
        !self.triggers.is_empty() || !self.out_edges.is_empty() || !self.in_edges.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: Gid,
    pub to: Gid,
    pub kind: EdgeKind,
    pub actions: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct ObjMeta {
    pub name: String,
    pub mnemonic: String,
    pub logic: bool,
    pub activation: Activation,
    pub actions: Vec<usize>,
    pub items: Vec<(ItemKey, Access)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warn,
}

#[derive(Clone, Debug)]
pub struct Lint {
    pub severity: Severity,
    pub group: Option<Gid>,
    pub obj: Option<usize>,
    pub msg: String,
}

pub struct Model {
    pub level_name: String,
    pub creator: String,
    pub objects: Vec<GDObject>,
    pub meta: Vec<ObjMeta>,
    pub actions: Vec<Action>,
    pub groups: BTreeMap<Gid, GroupInfo>,
    pub edges: Vec<Edge>,
    /// Logic objects that are not spawn-triggered, sorted by x.
    pub entry_points: Vec<usize>,
    pub items: BTreeMap<ItemKey, Vec<ItemRef>>,
    /// Groups reachable from [`ROOT`] through spawn edges.
    pub reachable: BTreeSet<Gid>,
    /// Spawn cycles (strongly connected components with a loop).
    pub cycles: Vec<Vec<Gid>>,
    pub lints: Vec<Lint>,
    pub has_remaps: bool,
    /// (min_x, min_y, max_x, max_y) of all objects.
    pub bounds: (f32, f32, f32, f32),
}
