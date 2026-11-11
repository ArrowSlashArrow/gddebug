//! Headless UI tests: the real `App::show` runs against an offscreen `egui::Context` with synthetic
//! input. No window is created; shapes are still tessellated so the drawing code is exercised.

use super::*;
use egui::{Event, Modifiers, PointerButton};
use std::time::Instant;

fn level(file: &str) -> Model {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../gdlib/test_gmds")
        .join(file);
    crate::load_level(&path, false).expect("load test level")
}

struct Harness {
    ctx: egui::Context,
    app: App,
    time: f64,
    modifiers: Modifiers,
}

impl Harness {
    fn new(m: Model) -> Self {
        let mut h = Self {
            ctx: egui::Context::default(),
            app: App::new(m),
            time: 0.0,
            modifiers: Modifiers::NONE,
        };
        h.frames(2); // first frames lay out panels and run "fit"
        h
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.time += 1.0 / 60.0;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1500.0, 900.0))),
            time: Some(self.time),
            modifiers: self.modifiers,
            events,
            ..Default::default()
        };
        let app = &mut self.app;
        let out = self.ctx.run(input, |ctx| app.show(ctx));
        let meshes = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        assert!(!meshes.is_empty(), "nothing was drawn");
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    fn click(&mut self, pos: Pos2) {
        let button = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.frame(vec![Event::PointerMoved(pos)]);
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
        self.frames(1);
    }

    fn graph_screen(&self, g: Gid) -> Pos2 {
        let a = &self.app;
        (a.g_rect.center().to_vec2() + a.g_pan + a.pos[&g] * a.g_zoom).to_pos2()
    }
}

#[test]
fn every_tab_renders_with_selections() {
    let mut h = Harness::new(level("Chompstep.gmd"));
    let trigger = h.app.m.entry_points[0];
    let item = h.app.m.items.keys().next().copied();
    for tab in [Tab::Graph, Tab::Map, Tab::Listing, Tab::Items] {
        h.app.tab = tab;
        h.frames(2);
        h.app.navigate(Nav::Group(12), true);
        h.app.navigate(Nav::Obj(trigger), true);
        if let Some(k) = item {
            h.app.navigate(Nav::Item(k), true);
        }
        h.app.navigate(Nav::Group(ROOT), true);
        h.frames(3);
    }
    // toggles in the graph toolbar, including the force layout
    h.app.tab = Tab::Graph;
    h.app.show_reference = true;
    h.app.show_isolated = true;
    h.app.relax = true;
    h.frames(5);
    // filter: numeric, object index, and free text
    for f in ["12", "#7587", "move"] {
        h.app.filter = f.into();
        h.frames(2);
    }
}

#[test]
fn clicking_a_graph_node_selects_it() {
    let mut h = Harness::new(level("Chompstep.gmd"));
    assert!(h.app.g_rect.is_positive(), "graph view has no area");
    h.app.center_on(Nav::Group(12));
    h.frames(1);
    let p = h.graph_screen(12);
    assert!(h.app.g_rect.contains(p));
    h.click(p);
    assert_eq!(h.app.sel.group, Some(12));
    // clicking empty background clears the group selection
    h.click(h.app.g_rect.left_bottom() + vec2(5.0, -5.0));
    assert_eq!(h.app.sel.group, None);
}

#[test]
fn double_click_jumps_to_map_and_back_works() {
    let mut h = Harness::new(level("Chompstep.gmd"));
    h.app.center_on(Nav::Group(12));
    h.frames(1);
    let p = h.graph_screen(12);
    h.frame(vec![Event::PointerMoved(p)]);
    for pressed in [true, false, true, false] {
        h.frame(vec![Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }]);
    }
    h.frames(2);
    assert!(
        h.app.tab == Tab::Map,
        "double-click should switch to the map"
    );
    assert_eq!(h.app.sel.group, Some(12));

    h.app.navigate(Nav::Group(14), false);
    assert_eq!(h.app.sel.group, Some(14));
    h.modifiers = Modifiers::ALT;
    h.frame(vec![Event::Key {
        key: egui::Key::ArrowLeft,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::ALT,
    }]);
    h.modifiers = Modifiers::NONE;
    assert_eq!(h.app.sel.group, Some(12), "Alt+Left should go back");
}

#[test]
fn clicking_on_the_map_selects_an_object() {
    let mut h = Harness::new(level("Chompstep.gmd"));
    h.app.tab = Tab::Map;
    h.frames(2);
    assert!(h.app.m_rect.is_positive(), "map view has no area");
    let target = 7587; // MOVE trigger in g12
    h.app.center_on(Nav::Obj(target));
    h.frames(1);
    h.click(h.app.m_rect.center());
    let picked = h.app.sel.obj.expect("nothing picked");
    assert_eq!(
        h.app.m.objects[picked].config.pos,
        h.app.m.objects[target].config.pos
    );
    assert!(
        h.app.m.meta[picked].logic,
        "logic objects should win when stacked"
    );
}

#[test]
fn listing_row_click_selects_trigger() {
    let mut h = Harness::new(level("advrand test.gmd"));
    h.app.tab = Tab::Listing;
    h.frames(2);
    // row 1 is the first entry point (the advanced random trigger, object #0); rows are laid out top-down
    let row_h = h.ctx.style().text_styles[&egui::TextStyle::Monospace].size + 4.0;
    let central = h.ctx.available_rect();
    let mut hit = false;
    for i in 0..12 {
        let p = Pos2::new(
            central.left() + 60.0,
            central.top() + 30.0 + i as f32 * row_h,
        );
        h.click(p);
        if h.app.sel.obj == Some(0) {
            hit = true;
            break;
        }
    }
    assert!(hit, "clicking the entry-point row should select object #0");
}

/// Detailed cards are sized from `CHAR_W`; if it's narrower than the font, lines overflow.
#[test]
fn card_width_estimate_fits_the_font() {
    let ctx = egui::Context::default();
    let _ = ctx.run(Default::default(), |_| {});
    let line = "x".repeat(MAX_CHARS);
    let w = ctx.fonts(|f| {
        f.layout_no_wrap(line, FontId::monospace(DETAIL_FONT), Color32::WHITE)
            .size()
            .x
    });
    assert!(
        w <= MAX_CHARS as f32 * CHAR_W,
        "{MAX_CHARS} chars are {w} wide, cards allow {}",
        MAX_CHARS as f32 * CHAR_W
    );
}

/// Zooming over level text used to rasterise glyphs at every zoom-dependent pixel size; the
/// font atlas then kept doubling (full re-upload each time) until egui rebuilt it: lag spikes.
#[test]
fn zooming_over_text_keeps_the_font_atlas_small() {
    let mut h = Harness::new(level("Chompstep.gmd"));
    h.app.tab = Tab::Map;
    h.frames(2);
    let i = *h.app.texts.keys().min().unwrap();
    let before = h.ctx.fonts(|f| f.font_image_size());
    for f in 0..300 {
        h.app.m_zoom = 0.5 * 1.01f32.powi(f);
        h.app.center_on(Nav::Obj(i));
        h.frames(1);
    }
    let (after, fill) = h
        .ctx
        .fonts(|f| (f.font_image_size(), f.font_atlas_fill_ratio()));
    println!("atlas {before:?} -> {after:?}, {:.0}% full", fill * 100.0);
    assert!(
        after[0] * after[1] <= 4 * before[0] * before[1],
        "font atlas grew from {before:?} to {after:?}"
    );
    assert!(fill < 0.6, "font atlas {:.0}% full", fill * 100.0);
}

/// 790k objects: build + frame timings. `cargo test --release -- --ignored --nocapture`
#[test]
#[ignore]
fn big_level_frame_times() {
    let t = Instant::now();
    let m = level("big.gmd");
    let load = t.elapsed();
    let t = Instant::now();
    let mut h = Harness::new(m);
    let build = t.elapsed();
    for tab in [Tab::Graph, Tab::Map, Tab::Listing, Tab::Items] {
        h.app.tab = tab;
        h.frames(1);
        let t = Instant::now();
        h.frames(5);
        println!(
            "{:?}: {:.1} ms/frame",
            ["graph", "map", "listing", "items"][tab as usize],
            t.elapsed().as_secs_f64() * 1000.0 / 5.0
        );
    }
    println!("load+analysis {load:?}, gui setup + 2 frames {build:?}");
}
