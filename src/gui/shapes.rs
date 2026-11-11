//! Flowchart geometry: node outlines per [`FlowKind`], where edges leave them, and the
//! orthogonal, 45-degree-chamfered routes edges take between columns.

use super::*;

/// Outline (convex, clockwise on screen) of a flowchart node centred on `c` with half-size `h`,
/// plus decoration lines drawn on top. `None` = a group without logic (stored objects): cylinder.
pub(super) fn node_shape(kind: Option<FlowKind>, c: Pos2, h: Vec2) -> (Vec<Pos2>, Vec<Vec<Pos2>>) {
    let (l, r, t, b) = (c.x - h.x, c.x + h.x, c.y - h.y, c.y + h.y);
    let rect = vec![
        Pos2::new(l, t),
        Pos2::new(r, t),
        Pos2::new(r, b),
        Pos2::new(l, b),
    ];
    let steps = if h.y < 6.0 { 4 } else { 10 };
    match kind {
        Some(FlowKind::Process) => (rect, vec![]),
        Some(FlowKind::Call) => {
            let inset = (h.x * 0.12).min(10.0);
            (
                rect,
                vec![
                    vec![Pos2::new(l + inset, t), Pos2::new(l + inset, b)],
                    vec![Pos2::new(r - inset, t), Pos2::new(r - inset, b)],
                ],
            )
        }
        Some(FlowKind::Decision) => {
            let hy = h.y * 1.35;
            (
                vec![
                    Pos2::new(c.x, c.y - hy),
                    Pos2::new(c.x + h.x, c.y),
                    Pos2::new(c.x, c.y + hy),
                    Pos2::new(c.x - h.x, c.y),
                ],
                vec![],
            )
        }
        Some(FlowKind::Stop) => {
            let mut pts = Vec::new();
            let rad = h.y * 1.5;
            arc(
                &mut pts,
                c,
                vec2(rad, rad),
                0.0,
                360.0 - 360.0 / 32.0,
                if h.y < 6.0 { 12 } else { 31 },
            );
            (pts, vec![])
        }
        Some(FlowKind::Item) => {
            let s = h.y * 0.6;
            (
                vec![
                    Pos2::new(l + s, t),
                    Pos2::new(r + s, t),
                    Pos2::new(r - s, b),
                    Pos2::new(l - s, b),
                ],
                vec![],
            )
        }
        Some(FlowKind::Event) => {
            let mut pts = vec![Pos2::new(l, b), Pos2::new(l, t)];
            arc(
                &mut pts,
                Pos2::new(r - h.y, c.y),
                vec2(h.y, h.y),
                -90.0,
                90.0,
                steps,
            );
            (pts, vec![])
        }
        Some(FlowKind::Terminal) => {
            let mut pts = Vec::new();
            arc(
                &mut pts,
                Pos2::new(r - h.y, c.y),
                vec2(h.y, h.y),
                -90.0,
                90.0,
                steps,
            );
            arc(
                &mut pts,
                Pos2::new(l + h.y, c.y),
                vec2(h.y, h.y),
                90.0,
                270.0,
                steps,
            );
            (pts, vec![])
        }
        None => {
            // cylinder: elliptical caps top and bottom
            let ry = h.y * 0.3;
            let mut pts = Vec::new();
            arc(
                &mut pts,
                Pos2::new(c.x, t + ry),
                vec2(h.x, ry),
                180.0,
                360.0,
                steps,
            );
            arc(
                &mut pts,
                Pos2::new(c.x, b - ry),
                vec2(h.x, ry),
                0.0,
                180.0,
                steps,
            );
            let mut front = Vec::new();
            arc(
                &mut front,
                Pos2::new(c.x, t + ry),
                vec2(h.x, ry),
                0.0,
                180.0,
                steps,
            );
            (pts, vec![front])
        }
    }
}

pub(super) fn arc(out: &mut Vec<Pos2>, c: Pos2, r: Vec2, from_deg: f32, to_deg: f32, steps: usize) {
    for i in 0..=steps {
        let a = (from_deg + (to_deg - from_deg) * i as f32 / steps as f32).to_radians();
        out.push(c + vec2(a.cos() * r.x, a.sin() * r.y));
    }
}

/// Where an edge from the node at `c` toward `p` leaves that node's flowchart shape.
pub(super) fn shape_exit(kind: Option<FlowKind>, c: Pos2, p: Pos2, h: Vec2) -> Pos2 {
    let d = p - c;
    if d.length_sq() < 1e-6 {
        return c;
    }
    match kind {
        Some(FlowKind::Stop) => c + d.normalized() * (h.y * 1.5).min(d.length()),
        Some(FlowKind::Decision) => {
            let s = 1.0 / (d.x.abs() / h.x + d.y.abs() / (h.y * 1.35));
            c + d * s.min(1.0)
        }
        _ => box_exit(c, p, h),
    }
}

/// Point where the ray from `c` toward `p` leaves a box of half-size `h` around `c`.
pub(super) fn box_exit(c: Pos2, p: Pos2, h: Vec2) -> Pos2 {
    let d = p - c;
    if d.length_sq() < 1e-6 {
        return c;
    }
    let sx = if d.x.abs() > 1e-6 {
        h.x / d.x.abs()
    } else {
        f32::MAX
    };
    let sy = if d.y.abs() > 1e-6 {
        h.y / d.y.abs()
    } else {
        f32::MAX
    };
    c + d * sx.min(sy).min(1.0)
}

/// Edge route from a right-hand port `a` to a left-hand port `b` (screen space): horizontal out,
/// vertical in the corridor just before the target's column, horizontal in, with 45 degree
/// corners. Siblings share the vertical, so a branch reads as a trunk splitting up and down.
/// Edges going backwards loop around below.
pub(super) fn route(a: Pos2, b: Pos2, corridor: f32, off: f32) -> Vec<Pos2> {
    let ortho = if b.x - a.x >= corridor * 0.6 {
        let xv = (b.x - corridor * 0.5 + off).max(a.x + corridor * 0.25);
        vec![a, Pos2::new(xv, a.y), Pos2::new(xv, b.y), b]
    } else {
        let s = corridor * 0.35;
        let ym = if (b.y - a.y).abs() > corridor {
            (a.y + b.y) / 2.0
        } else {
            a.y.max(b.y) + corridor
        };
        vec![
            a,
            Pos2::new(a.x + s, a.y),
            Pos2::new(a.x + s, ym),
            Pos2::new(b.x - s, ym),
            Pos2::new(b.x - s, b.y),
            b,
        ]
    };
    chamfer(&ortho, corridor * 0.3)
}

/// Cuts every corner of an orthogonal polyline at 45 degrees.
pub(super) fn chamfer(pts: &[Pos2], c: f32) -> Vec<Pos2> {
    let mut p: Vec<Pos2> = Vec::with_capacity(pts.len());
    for q in pts {
        if p.last().is_none_or(|l: &Pos2| l.distance(*q) > 0.01) {
            p.push(*q);
        }
    }
    if p.len() < 3 {
        return p;
    }
    let mut out = vec![p[0]];
    for i in 1..p.len() - 1 {
        let (d1, d2) = (p[i] - p[i - 1], p[i + 1] - p[i]);
        if d1.normalized().dot(d2.normalized()) > 0.999 {
            continue; // collinear
        }
        let k = c.min(d1.length() / 2.0).min(d2.length() / 2.0);
        out.push(p[i] - d1.normalized() * k);
        out.push(p[i] + d2.normalized() * k);
    }
    out.push(p[p.len() - 1]);
    out
}
