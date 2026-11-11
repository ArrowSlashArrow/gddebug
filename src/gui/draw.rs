//! Drawing primitives shared by the graph and the map: arrows, text on plates, non-overlapping
//! label placement, outlined level text, syntax-highlighted lines and filled polygons.

use super::*;

/// Draws an arrow. `emph` adds a soft glow in the arrow's colour plus a dark backing stroke, so
/// arrows that touch the selection stand out from everything behind them.
pub(super) fn arrow(
    painter: &egui::Painter,
    from: Pos2,
    to: Pos2,
    color: Color32,
    width: f32,
    head: f32,
    emph: bool,
) {
    let d = to - from;
    let len = d.length();
    if len < 1.0 {
        return;
    }
    let dir = d / len;
    let perp = vec2(-dir.y, dir.x);
    let base = to - dir * head;
    let tip = vec![to, base + perp * head * 0.45, base - perp * head * 0.45];
    if emph {
        painter.line_segment(
            [from, base],
            Stroke::new(width + 8.0, color.gamma_multiply(0.18)),
        );
        painter.line_segment(
            [from, base],
            Stroke::new(width + 3.0, Color32::from_black_alpha(220)),
        );
        painter.add(Shape::convex_polygon(
            tip.clone(),
            Color32::TRANSPARENT,
            Stroke::new(3.0, Color32::from_black_alpha(220)),
        ));
    }
    painter.line_segment([from, base], Stroke::new(width, color));
    painter.add(Shape::convex_polygon(tip, color, Stroke::NONE));
}

/// Arrow along a polyline; the head sits on the last segment. See [`arrow`] for `emph`.
pub(super) fn arrow_path(
    painter: &egui::Painter,
    pts: &[Pos2],
    color: Color32,
    width: f32,
    head: f32,
    emph: bool,
) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    let d = pts[n - 1] - pts[n - 2];
    let len = d.length();
    if len < 0.5 {
        return;
    }
    let dir = d / len;
    let head = head.min(len);
    let perp = vec2(-dir.y, dir.x);
    let base = pts[n - 1] - dir * head;
    let mut line = pts[..n - 1].to_vec();
    line.push(base);
    let tip = vec![
        pts[n - 1],
        base + perp * head * 0.45,
        base - perp * head * 0.45,
    ];
    if emph {
        painter.add(Shape::line(
            line.clone(),
            Stroke::new(width + 8.0, color.gamma_multiply(0.18)),
        ));
        painter.add(Shape::line(
            line.clone(),
            Stroke::new(width + 3.0, Color32::from_black_alpha(220)),
        ));
        painter.add(Shape::convex_polygon(
            tip.clone(),
            Color32::TRANSPARENT,
            Stroke::new(3.0, Color32::from_black_alpha(220)),
        ));
    }
    painter.add(Shape::line(line, Stroke::new(width, color)));
    painter.add(Shape::convex_polygon(tip, color, Stroke::NONE));
}

/// Text on a dark rounded plate so it reads over edges, nodes and level objects.
/// `border` outlines the plate (used for labels belonging to the selection).
pub(super) fn plate_text(
    painter: &egui::Painter,
    pos: Pos2,
    align: Align2,
    text: impl ToString,
    font: FontId,
    color: Color32,
    border: Option<Color32>,
) -> Rect {
    let galley = painter.layout_no_wrap(text.to_string(), font, color);
    let rect = align.anchor_size(pos, galley.size());
    let bg = rect.expand2(vec2(3.0, 1.0));
    painter.rect_filled(bg, 3.0, PLATE_BG);
    painter.rect_stroke(
        bg,
        3.0,
        Stroke::new(1.0, border.unwrap_or(Color32::from_white_alpha(18))),
    );
    painter.galley(rect.min, galley, color);
    rect
}

/// Draws a laid-out galley at `rect` on a plate (see [`plate_text`]).
pub(super) fn plate_galley(
    painter: &egui::Painter,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    color: Color32,
    border: Option<Color32>,
) {
    let bg = rect.expand2(vec2(3.0, 1.0));
    painter.rect_filled(bg, 3.0, PLATE_BG);
    painter.rect_stroke(
        bg,
        3.0,
        Stroke::new(1.0, border.unwrap_or(Color32::from_white_alpha(18))),
    );
    painter.galley(rect.min, galley, color);
}

/// Greedy label placement: a label is only drawn where it doesn't overlap one already drawn.
#[derive(Default)]
pub(super) struct LabelPlacer {
    cells: HashMap<(i32, i32), Vec<Rect>>,
}

impl LabelPlacer {
    pub(super) const CELL: f32 = 64.0;

    pub(super) fn cells(r: Rect) -> impl Iterator<Item = (i32, i32)> {
        let (x0, x1) = (
            (r.left() / Self::CELL).floor() as i32,
            (r.right() / Self::CELL).floor() as i32,
        );
        let (y0, y1) = (
            (r.top() / Self::CELL).floor() as i32,
            (r.bottom() / Self::CELL).floor() as i32,
        );
        (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
    }

    pub(super) fn try_place(&mut self, r: Rect) -> bool {
        if Self::cells(r).any(|c| {
            self.cells
                .get(&c)
                .is_some_and(|v| v.iter().any(|o| o.intersects(r)))
        }) {
            return false;
        }
        for c in Self::cells(r) {
            self.cells.entry(c).or_default().push(r);
        }
        true
    }
}

/// Font sizes level text is rasterised at. egui caches glyphs per integer pixel size and never
/// evicts them, so drawing text at every zoom-dependent size fills the font atlas, which then gets
/// re-uploaded (and eventually rebuilt from scratch): those were the lag spikes while zooming.
/// Text is rasterised at the nearest size from this short list and the mesh is scaled instead.
pub(super) const TEXT_RASTER: [f32; 12] = [
    8.0, 10.0, 12.0, 14.0, 16.0, 20.0, 24.0, 28.0, 32.0, 40.0, 48.0, 64.0,
];

/// Text with a dark outline (stroke), centred on `center` and rotated by `angle` radians.
/// Used for level text objects, where a plate would hide the level behind it.
pub(super) fn halo_text(
    painter: &egui::Painter,
    center: Pos2,
    text: &str,
    size: f32,
    color: Color32,
    angle: f32,
) {
    let raster = TEXT_RASTER
        .iter()
        .copied()
        .find(|s| *s >= size)
        .unwrap_or(64.0);
    let scale = size / raster;
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(raster), color);
    let ctx = painter.ctx();
    let mut tess = egui::epaint::Tessellator::new(
        ctx.pixels_per_point(),
        Default::default(),
        ctx.fonts(|f| f.font_image_size()),
        vec![],
    );
    let mut mesh = egui::Mesh::default();
    let w = (size * 0.08).clamp(1.0, 2.5) / scale;
    for off in [
        vec2(-w, 0.0),
        vec2(w, 0.0),
        vec2(0.0, -w),
        vec2(0.0, w),
        vec2(-w, -w),
        vec2(w, w),
        vec2(-w, w),
        vec2(w, -w),
    ] {
        let halo = egui::epaint::TextShape::new(Pos2::ZERO + off, galley.clone(), color)
            .with_override_text_color(Color32::BLACK);
        tess.tessellate_text(&halo, &mut mesh);
    }
    tess.tessellate_text(
        &egui::epaint::TextShape::new(Pos2::ZERO, galley.clone(), color),
        &mut mesh,
    );
    let mid = galley.size() / 2.0;
    let rot = egui::emath::Rot2::from_angle(angle);
    for v in &mut mesh.vertices {
        v.pos = center + rot * ((v.pos.to_vec2() - mid) * scale);
    }
    painter.add(Shape::mesh(mesh));
}

/// A listing line as a highlighted layout job. Keywords and plain text keep `base` (the row's or
/// trigger's own colour); groups, items, numbers, operators, flags and comments get their own.
/// Hex colours are drawn in themselves. `dim` darkens everything (unrelated graph cards).
pub(super) fn highlighted(
    text: &str,
    font: FontId,
    base: Color32,
    dim: bool,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for (r, t) in highlight::tokens(text) {
        let piece = &text[r];
        let rgb = match t {
            Tok::Colour => highlight::hex_rgb(piece).map(|(r, g, b)| {
                // keep dark colours readable on the dark background: brighten, keeping the hue
                let max = r.max(g).max(b);
                if max < 20 {
                    (150, 150, 150)
                } else if max < 190 {
                    let k = 190.0 / max as f32;
                    (
                        (r as f32 * k) as u8,
                        (g as f32 * k) as u8,
                        (b as f32 * k) as u8,
                    )
                } else {
                    (r, g, b)
                }
            }),
            _ => highlight::rgb(t),
        };
        let mut color = rgb.map_or(base, |(r, g, b)| Color32::from_rgb(r, g, b));
        if dim {
            color = shade(color, 0.5);
        }
        job.append(piece, 0.0, egui::TextFormat::simple(font.clone(), color));
    }
    job
}

/// Adds a convex polygon to a mesh as a triangle fan (cheap batching when zoomed far out).
pub(super) fn mesh_polygon(mesh: &mut egui::Mesh, pts: &[Pos2], color: Color32) {
    let base = mesh.vertices.len() as u32;
    for p in pts {
        mesh.colored_vertex(*p, color);
    }
    for i in 1..pts.len().saturating_sub(1) as u32 {
        mesh.add_triangle(base, base + i, base + i + 1);
    }
}
