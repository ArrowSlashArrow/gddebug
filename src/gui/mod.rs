//! Interactive explorer (egui).
//!
//! Tabs:
//! * **Graph** ([`graph`]): groups as flowchart nodes, triggers' effects as routed edges, laid out
//!   as a left-to-right tree hanging off the LEVEL timeline ([`layout`], [`graph_state`]).
//!   Minimal mode draws shapes; detailed mode draws cards listing each group's triggers.
//! * **Map** ([`map`]): the level itself, with the selection's objects and arrows highlighted.
//! * **Listing** / **Items** ([`listing_tab`]): the disassembly and the item cross-reference.
//!
//! Around them: the side panels ([`panels`]), and shared helpers for colours ([`style`]),
//! drawing ([`draw`]), node shapes and edge routing ([`shapes`]) and toolbars ([`widgets`]).
//!
//! Everything lives on [`App`]; each view is an `impl App` block in its own module. Tests run
//! the real UI headlessly (`tests`); `snapshot` renders views to PNG for visual checks.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2, vec2,
};

use gdlib::cclocallevels::gdobj::{ObjectProperties, constructors::TextObject, ids::objects::TEXT};

use crate::analysis::{
    Access, Activation, EdgeKind, FlowKind, Gid, ItemKey, Model, ROOT, Severity, fmt_value,
    group_label, item_label, property_name,
};
use crate::decode;
use crate::highlight::{self, Tok};
use crate::listing::{self, Mode, Names};
use crate::report;

mod draw;
mod graph;
mod graph_state;
mod layout;
mod listing_tab;
mod map;
mod panels;
mod shapes;
mod style;
mod widgets;

#[cfg(test)]
mod snapshot;
#[cfg(test)]
mod tests;

use draw::*;
use graph_state::*;
use layout::{Bus, H_GAP};
use listing_tab::*;
use shapes::*;
use style::*;
use widgets::*;

const NODE_W: f32 = 120.0;

const NODE_H: f32 = 40.0;

/// Detailed graph cards: header height, line height and body font size (world units).
const HEADER_H: f32 = 30.0;

const LINE_H: f32 = 15.0;

const DETAIL_FONT: f32 = 11.0;

/// Approximate advance of one monospace character at `DETAIL_FONT`.
const CHAR_W: f32 = 7.0;

const MAX_LINES: usize = 10;

const MAX_CHARS: usize = 64;

/// Width reserved for the graph legend when fitting.
const LEGEND_W: f32 = 217.0;

const MAX_RELAX_NODES: usize = 1500;

pub fn run(model: Model, path: PathBuf) -> Result<(), String> {
    let title = format!("gddbg - {} ({})", model.level_name, path.display());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 900.0])
            .with_title(title),
        ..Default::default()
    };
    eframe::run_native(
        "gddbg",
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(model)))),
    )
    .map_err(|e| e.to_string())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Graph,
    Map,
    Listing,
    Items,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct Sel {
    group: Option<Gid>,
    obj: Option<usize>,
    item: Option<ItemKey>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Nav {
    Group(Gid),
    Obj(usize),
    Item(ItemKey),
}

struct Row {
    text: String,
    color: Color32,
    obj: Option<usize>,
    group: Option<Gid>,
}

pub struct App {
    m: Model,
    tab: Tab,
    sel: Sel,
    history: Vec<Sel>,
    filter: String,
    only_logic: bool,
    warn_groups: BTreeSet<Gid>,
    cycle_groups: BTreeSet<Gid>,
    /// Flowchart shape of each group: the most significant role among its triggers (None = no logic).
    flow: HashMap<Gid, FlowKind>,
    /// Decoded text of every text object, by object index.
    texts: HashMap<usize, String>,
    /// What each logic object does, one line (from gdlib's typed structs where possible).
    summaries: HashMap<usize, String>,
    /// Detailed graph: one line per trigger of each group, with the trigger's flow kind.
    node_lines: HashMap<Gid, Vec<(String, FlowKind, i32)>>,

    // graph
    /// Every group that has logic (candidates for "groups without edges").
    nodes: Vec<Gid>,
    pos: HashMap<Gid, Vec2>,
    /// Half size of every laid-out node, world units.
    half: HashMap<Gid, Vec2>,
    tree_parent: HashMap<Gid, Gid>,
    buses: Vec<Bus>,
    timeline_y: f32,
    /// False once the force layout has moved things: the timeline no longer lines up.
    structured: bool,
    /// Toggles the layout was computed for; `None` forces a re-layout.
    layout_key: Option<(bool, bool, bool, bool, Mode)>,
    graph_detail: bool,
    edge_offset: Vec<f32>,
    show_affect: bool,
    show_reference: bool,
    show_isolated: bool,
    relax: bool,
    g_pan: Vec2,
    g_zoom: f32,
    g_drag: Option<Gid>,
    g_fit: bool,
    /// After a fit, keep fitting when the view is resized until the user moves the view.
    g_autofit: bool,
    g_rect: Rect,

    // map
    m_pan: Vec2,
    m_zoom: f32,
    m_fit: bool,
    m_autofit: bool,
    m_rect: Rect,
    map_logic_only: bool,
    map_labels: bool,
    map_detail: bool,

    // listing
    /// How triggers are written in the listing tab and on detailed graph cards.
    listing_mode: Mode,
    /// Mode `rows` were built for.
    rows_mode: Mode,
    names: Names,
    rows: Vec<Row>,
    group_row: HashMap<Gid, usize>,
    listing_jump: Option<usize>,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show(ctx);
    }
}

impl App {
    fn new(m: Model) -> Self {
        let nodes: Vec<Gid> = m
            .groups
            .values()
            .filter(|g| g.id == ROOT || g.has_logic())
            .map(|g| g.id)
            .collect();
        let edge_offset = edge_offsets(&m);
        let names = Names::new(&m);
        let (rows, group_row) = build_listing(&m, Mode::default(), &names);
        let warn_groups = m
            .lints
            .iter()
            .filter(|l| l.severity == Severity::Warn)
            .filter_map(|l| l.group)
            .collect();
        let cycle_groups = m.cycles.iter().flatten().copied().collect();
        let mut flow: HashMap<Gid, FlowKind> = HashMap::new();
        for info in m.real_groups() {
            if let Some(k) = info
                .triggers
                .iter()
                .map(|&t| FlowKind::of(m.objects[t].id))
                .max()
            {
                flow.insert(info.id, k);
            }
        }
        flow.insert(ROOT, FlowKind::Terminal);
        let summaries: HashMap<usize, String> = (0..m.objects.len())
            .filter(|&i| m.meta[i].logic)
            .map(|i| {
                (
                    i,
                    decode::summary(&m.objects[i]).unwrap_or_else(|| m.describe(i)),
                )
            })
            .collect();
        let node_lines = card_lines(&m, &names, Mode::default());
        let texts = m
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.id == TEXT)
            .filter_map(|(i, o)| TextObject::from_object(o).map(|t| (i, t.text)))
            .filter(|(_, t)| !t.trim().is_empty())
            .collect();
        Self {
            m,
            tab: Tab::Graph,
            sel: Sel::default(),
            history: Vec::new(),
            filter: String::new(),
            only_logic: true,
            warn_groups,
            cycle_groups,
            flow,
            texts,
            summaries,
            node_lines,
            nodes,
            pos: HashMap::new(),
            half: HashMap::new(),
            tree_parent: HashMap::new(),
            buses: Vec::new(),
            timeline_y: 0.0,
            structured: true,
            layout_key: None,
            graph_detail: false,
            edge_offset,
            show_affect: true,
            show_reference: false,
            show_isolated: false,
            relax: false,
            g_pan: Vec2::ZERO,
            g_zoom: 1.0,
            g_drag: None,
            g_fit: true,
            g_autofit: false,
            g_rect: Rect::NOTHING,
            m_pan: Vec2::ZERO,
            m_zoom: 1.0,
            m_fit: true,
            m_autofit: false,
            m_rect: Rect::NOTHING,
            map_logic_only: false,
            map_labels: true,
            map_detail: false,
            rows,
            group_row,
            listing_jump: None,
            listing_mode: Mode::default(),
            rows_mode: Mode::default(),
            names,
        }
    }

    fn navigate(&mut self, nav: Nav, center: bool) {
        let before = self.sel;
        match nav {
            Nav::Group(g) => {
                self.sel.group = Some(g);
                if let Some(o) = self.sel.obj
                    && !self.m.objects[o].config.groups.contains(&g)
                {
                    self.sel.obj = None;
                }
            }
            Nav::Obj(i) => self.sel.obj = Some(i),
            Nav::Item(k) => self.sel.item = Some(k),
        }
        if self.sel != before {
            self.history.push(before);
            if self.history.len() > 500 {
                self.history.remove(0);
            }
        }
        if center {
            self.center_on(nav);
        }
    }

    fn center_on(&mut self, nav: Nav) {
        self.g_autofit = false;
        self.m_autofit = false;
        match nav {
            Nav::Group(g) => {
                if let Some(p) = self.pos.get(&g) {
                    self.g_pan = -*p * self.g_zoom;
                }
                if let Some((x, y)) = self.m.groups.get(&g).and_then(|i| i.centroid) {
                    self.m_pan = -vec2(x, -y) * self.m_zoom;
                }
                self.listing_jump = self.group_row.get(&g).copied();
            }
            Nav::Obj(i) => {
                let (x, y) = self.m.objects[i].config.pos;
                if self.m_zoom < 0.4 {
                    self.m_zoom = 1.0;
                }
                self.m_pan = -vec2(x, -y) * self.m_zoom;
                if let Some(g) = self.m.trigger_sources(i).first()
                    && let Some(p) = self.pos.get(g)
                {
                    self.g_pan = -*p * self.g_zoom;
                }
                self.listing_jump = self.rows.iter().position(|r| r.obj == Some(i));
            }
            Nav::Item(_) => {}
        }
    }

    fn back(&mut self) {
        if let Some(prev) = self.history.pop() {
            self.sel = prev;
        }
    }

    /// The whole UI for one frame. Kept separate from `eframe::App` so it can run headless in tests.
    fn show(&mut self, ctx: &egui::Context) {
        let back = ctx.input(|i| {
            (i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft))
                || i.pointer.button_pressed(egui::PointerButton::Extra1)
        });
        if back {
            self.back();
        }

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.history.is_empty(), egui::Button::new("< back"))
                    .clicked()
                {
                    self.back();
                }
                ui.separator();
                for (tab, name) in [
                    (Tab::Graph, "Graph"),
                    (Tab::Map, "Map"),
                    (Tab::Listing, "Listing"),
                    (Tab::Items, "Items"),
                ] {
                    ui.selectable_value(&mut self.tab, tab, name);
                }
                ui.separator();
                let mut sel = Vec::new();
                if let Some(g) = self.sel.group {
                    sel.push(group_label(g));
                }
                if let Some(o) = self.sel.obj {
                    sel.push(format!("#{o} {}", self.m.meta[o].mnemonic));
                }
                if let Some(k) = self.sel.item {
                    sel.push(item_label(k));
                }
                if !sel.is_empty() {
                    ui.label(RichText::new(format!("selected: {}", sel.join("  ·  "))).weak());
                }
            });
        });

        let mut nav: Option<(Nav, bool)> = None;
        egui::SidePanel::left("left")
            .resizable(true)
            .default_width(330.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("leftscroll")
                    .show(ui, |ui| {
                        if let Some(n) = self.left_panel(ui) {
                            nav = Some((n, true));
                        }
                    });
            });
        egui::SidePanel::right("right")
            .resizable(true)
            .default_width(400.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("rightscroll")
                    .show(ui, |ui| {
                        if let Some(n) = self.inspector(ui) {
                            nav = Some((n, true));
                        }
                    });
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            // selecting inside the active view shouldn't yank that same view around
            let n = match self.tab {
                Tab::Graph => self.graph_view(ui),
                Tab::Map => self.map_view(ui),
                Tab::Listing => self.listing_view(ui),
                Tab::Items => self.items_view(ui),
            };
            if let Some(n) = n {
                nav = Some((n, false));
            }
        });

        if let Some((n, center)) = nav {
            self.navigate(n, center);
            if !center {
                // still keep the other views in sync, just not the one that was clicked
                let (gp, mp) = (self.g_pan, self.m_pan);
                self.center_on(n);
                match self.tab {
                    Tab::Graph => self.g_pan = gp,
                    Tab::Map => self.m_pan = mp,
                    Tab::Listing => self.listing_jump = None,
                    Tab::Items => {}
                }
            }
        }
    }
}
