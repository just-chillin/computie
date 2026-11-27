//! The ring chart: layout, hit-testing and painting.
//!
//! Angles are in radians, 0 at twelve o'clock, growing clockwise. Radii are
//! fractions of the wheel's radius, so the wheel scales with its box.

use crate::theme::{ink, tint, Theme, PALETTE};
use computie_core::{Kind, NodeId, Tree};
use gpui::{
    point, px, quad, size, App, BorderStyle, Bounds, PathBuilder, Pixels, Point, SharedString,
    TextRun, Window,
};
use std::f32::consts::{FRAC_PI_2, TAU};

/// Inner and outer edge of each ring.
const RINGS: [(f32, f32); 3] = [(0.39, 0.62), (0.63, 0.81), (0.82, 0.97)];
const HOLE: f32 = 0.37;
const TINTS: [f32; 3] = [0.0, 0.28, 0.5];
/// Slices thinner than this (about 1 degree) are left out.
const MIN_SPAN: f32 = 0.02;

#[derive(Clone)]
pub struct Slice {
    pub id: NodeId,
    pub depth: usize,
    pub a0: f32,
    pub a1: f32,
    /// Palette colour of the top-level ancestor, so a branch keeps one hue.
    pub base: u32,
    pub name: String,
    pub openable: bool,
}

/// Lays out up to three levels below `focus`.
pub fn layout(tree: &Tree, focus: NodeId) -> Vec<Slice> {
    let mut out = Vec::new();
    place(tree, focus, 0.0, TAU, 0, 0, &mut out);
    out
}

fn place(tree: &Tree, parent: NodeId, a0: f32, a1: f32, depth: usize, base: u32, out: &mut Vec<Slice>) {
    let total = tree.node(parent).size;
    if depth >= RINGS.len() || total == 0 {
        return;
    }
    let mut a = a0;
    for (i, (id, node)) in tree.children(parent).enumerate() {
        let span = (a1 - a0) * (node.size as f64 / total as f64) as f32;
        if span < MIN_SPAN {
            // Children are sorted biggest first, so the rest are thinner still.
            break;
        }
        let base = if depth == 0 { PALETTE[i % PALETTE.len()] } else { base };
        out.push(Slice {
            id,
            depth,
            a0: a,
            a1: a + span,
            base,
            name: node.name.clone(),
            openable: node.kind == Kind::Dir && !node.children.is_empty(),
        });
        place(tree, id, a, a + span, depth + 1, base, out);
        a += span;
    }
}

pub enum Hit {
    Slice(usize),
    Center,
    Nothing,
}

fn geometry(b: Bounds<Pixels>) -> (Point<Pixels>, f32) {
    let radius = f32::from(b.size.width.min(b.size.height)) / 2.0 - 2.0;
    (b.center(), radius.max(10.0))
}

/// What is under the pointer at `pos` (window coordinates)?
pub fn hit(slices: &[Slice], b: Bounds<Pixels>, pos: Point<Pixels>) -> Hit {
    let (c, radius) = geometry(b);
    let (dx, dy) = (f32::from(pos.x - c.x), f32::from(pos.y - c.y));
    let r = (dx * dx + dy * dy).sqrt() / radius;
    if r <= HOLE {
        return Hit::Center;
    }
    let angle = (dy.atan2(dx) + FRAC_PI_2).rem_euclid(TAU);
    for (i, s) in slices.iter().enumerate() {
        let (r0, r1) = RINGS[s.depth];
        if r >= r0 && r <= r1 && angle >= s.a0 && angle < s.a1 {
            return Hit::Slice(i);
        }
    }
    Hit::Nothing
}

pub struct CenterText {
    pub name: SharedString,
    pub size: SharedString,
    pub hint: SharedString,
}

/// Paints the wheel. `dim[i]` fades slice `i` (used while hovering another).
pub fn paint(
    slices: &[Slice],
    dim: &[bool],
    center: &CenterText,
    t: &Theme,
    b: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let (c, radius) = geometry(b);
    let at = |r: f32, a: f32| {
        let s = a - FRAC_PI_2;
        point(c.x + px(radius * r * s.cos()), c.y + px(radius * r * s.sin()))
    };
    let disc = |r: f32| Bounds::centered_at(c, size(px(radius * r * 2.0), px(radius * r * 2.0)));

    // The well the wheel sits in.
    window.paint_quad(quad(disc(1.0), px(radius), t.panel, px(1.0), t.line, BorderStyle::Solid));

    for (i, s) in slices.iter().enumerate() {
        let (r0, r1) = RINGS[s.depth];
        let steps = (((s.a1 - s.a0) / 0.04).ceil() as usize).max(2);
        // GPUI has no ready-made ring segment, so the outline is traced as
        // many short straight lines: out along the outer edge, back along the
        // inner one.
        let mut outline = Vec::with_capacity(steps * 2 + 2);
        for k in 0..=steps {
            outline.push(at(r1, s.a0 + (s.a1 - s.a0) * k as f32 / steps as f32));
        }
        for k in (0..=steps).rev() {
            outline.push(at(r0, s.a0 + (s.a1 - s.a0) * k as f32 / steps as f32));
        }
        let mut color = tint(s.base, TINTS[s.depth]);
        let mut edge = ink();
        if dim.get(i).copied().unwrap_or(false) {
            color.a = 0.3;
            edge.a = 0.3;
        }
        let mut body = PathBuilder::fill();
        let mut border = PathBuilder::stroke(px(1.0));
        for (k, p) in outline.iter().enumerate() {
            if k == 0 {
                body.move_to(*p);
                border.move_to(*p);
            } else {
                body.line_to(*p);
                border.line_to(*p);
            }
        }
        body.close();
        border.close();
        if let Ok(path) = body.build() {
            window.paint_path(path, color);
        }
        if let Ok(path) = border.build() {
            window.paint_path(path, edge);
        }
    }

    // Labels go on last so no slice paints over them.
    let font = window.text_style().font();
    let run = |len: usize, color: gpui::Hsla| TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line_height = px(15.0);
    let mut placed: Vec<Bounds<Pixels>> = Vec::new();
    for (i, s) in slices.iter().enumerate() {
        let (r0, r1) = RINGS[s.depth];
        let mid = (s.a0 + s.a1) / 2.0;
        let arc = (s.a1 - s.a0) * radius * (r0 + r1) / 2.0;
        let thick = radius * (r1 - r0);
        // Text is horizontal, the slice is not. Near the top and bottom of
        // the wheel a slice runs sideways (room = its arc); near the left and
        // right it runs up and down (room = the ring's thickness).
        let (sin, cos) = (mid.sin().abs(), mid.cos().abs());
        let room_w = (arc * cos).max(thick * sin) - 10.0;
        let room_h = (arc * sin).max(thick * cos);
        if room_h < 18.0 || room_w < 26.0 {
            continue;
        }
        let mut color = ink();
        if dim.get(i).copied().unwrap_or(false) {
            color.a = 0.3;
        }
        let mut text = s.name.clone();
        loop {
            let shaped = window.text_system().shape_line(
                SharedString::from(text.clone()),
                px(12.0),
                &[run(text.len(), color)],
                None,
            );
            if f32::from(shaped.width) <= room_w {
                let p = at((r0 + r1) / 2.0, mid);
                let origin = point(p.x - shaped.width / 2.0, p.y - line_height / 2.0);
                let rect = Bounds::new(origin, size(shaped.width, line_height));
                // Never let two labels overlap; the earlier (bigger) one wins.
                if !placed.iter().any(|other: &Bounds<Pixels>| other.intersects(&rect)) {
                    shaped.paint(origin, line_height, window, cx).ok();
                    placed.push(rect);
                }
                break;
            }
            // Too wide: drop characters and end with an ellipsis.
            let kept: String = text.trim_end_matches('…').chars().collect::<Vec<_>>()
                [..text.trim_end_matches('…').chars().count().saturating_sub(1)]
                .iter()
                .collect();
            if kept.chars().count() < 3 {
                break;
            }
            text = format!("{kept}…");
        }
    }

    // The hole: where you are, and how big it is.
    window.paint_quad(quad(disc(HOLE), px(radius * HOLE), t.face, px(1.0), t.line, BorderStyle::Solid));
    let lines = [
        (&center.name, px(13.0), t.text, -22.0),
        (&center.size, px(22.0), t.text, 0.0),
        (&center.hint, px(12.0), t.muted, 22.0),
    ];
    for (text, font_size, color, dy) in lines {
        if text.is_empty() {
            continue;
        }
        let max_w = radius * HOLE * 1.7;
        let mut text = text.to_string();
        loop {
            let shaped = window.text_system().shape_line(
                SharedString::from(text.clone()),
                font_size,
                &[run(text.len(), color)],
                None,
            );
            if f32::from(shaped.width) <= max_w || text.chars().count() <= 4 {
                let lh = font_size * 1.3;
                let origin = point(c.x - shaped.width / 2.0, c.y + px(dy) - lh / 2.0);
                shaped.paint(origin, lh, window, cx).ok();
                break;
            }
            let n = text.trim_end_matches('…').chars().count().saturating_sub(1);
            text = format!("{}…", text.chars().take(n).collect::<String>());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use computie_core::{scan, ScanOptions};
    use std::fs;

    fn sample() -> (tempfile::TempDir, Tree) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("big")).unwrap();
        fs::create_dir(dir.path().join("small")).unwrap();
        fs::write(dir.path().join("big/a.bin"), vec![1u8; 600_000]).unwrap();
        fs::write(dir.path().join("small/b.bin"), vec![1u8; 200_000]).unwrap();
        let tree = scan(dir.path(), ScanOptions::default()).join();
        (dir, tree)
    }

    fn square() -> Bounds<Pixels> {
        Bounds::new(point(px(0.0), px(0.0)), size(px(400.0), px(400.0)))
    }

    #[test]
    fn slices_fill_the_circle_biggest_first() {
        let (_dir, tree) = sample();
        let slices = layout(&tree, Tree::ROOT);
        let ring: Vec<_> = slices.iter().filter(|s| s.depth == 0).collect();
        assert_eq!(ring.len(), 2);
        assert_eq!(ring[0].name, "big");
        assert!(ring[0].a0.abs() < 1e-4);
        assert!(ring[0].a1 > ring[1].a1 - ring[1].a0, "big should be the wider slice");
        assert!((ring[1].a1 - TAU).abs() < 0.05);
        assert!(ring[0].openable);
    }

    #[test]
    fn hit_finds_center_slice_and_nothing() {
        let (_dir, tree) = sample();
        let slices = layout(&tree, Tree::ROOT);
        let b = square();
        assert!(matches!(hit(&slices, b, point(px(200.0), px(200.0))), Hit::Center));
        // Just right of twelve o'clock, in the first ring: the biggest slice.
        match hit(&slices, b, point(px(210.0), px(100.0))) {
            Hit::Slice(i) => assert_eq!(slices[i].name, "big"),
            _ => panic!("expected the big slice"),
        }
        // Just left of twelve o'clock is the end of the circle: the small one.
        match hit(&slices, b, point(px(190.0), px(100.0))) {
            Hit::Slice(i) => assert_eq!(slices[i].name, "small"),
            _ => panic!("expected the small slice"),
        }
        assert!(matches!(hit(&slices, b, point(px(2.0), px(2.0))), Hit::Nothing));
    }
}
