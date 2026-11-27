//! Hand-drawn retro controls: bevelled panels, buttons, title bar, mascot.

use crate::theme::{Os, Theme};
use gpui::{
    canvas, div, fill, point, prelude::*, px, quad, rgb, size, BorderStyle, Bounds, Div,
    FontWeight, Hsla, PathBuilder, Pixels, SharedString, Stateful, Window,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Bevel {
    /// Sticks out: buttons, dialogs.
    Raised,
    /// Pressed in: lists, fields.
    Sunken,
}

/// Paints one bevel layer: `tl` on the top and left edges, `br` on the
/// bottom and right, `inset` pixels in from the bounds.
fn edges(window: &mut Window, b: Bounds<Pixels>, inset: f32, w: f32, tl: Hsla, br: Hsla) {
    let (x, y) = (b.origin.x + px(inset), b.origin.y + px(inset));
    let (bw, bh) = (b.size.width - px(inset * 2.0), b.size.height - px(inset * 2.0));
    let w = px(w);
    window.paint_quad(fill(Bounds::new(point(x, y), size(bw, w)), tl));
    window.paint_quad(fill(Bounds::new(point(x, y), size(w, bh)), tl));
    window.paint_quad(fill(Bounds::new(point(x, y + bh - w), size(bw, w)), br));
    window.paint_quad(fill(Bounds::new(point(x + bw - w, y), size(w, bh)), br));
}

/// Draws a filled, bevelled box in the current look.
pub fn paint_bevel(t: &Theme, b: Bounds<Pixels>, kind: Bevel, radius: f32, window: &mut Window) {
    let fill_color = if kind == Bevel::Raised { t.face } else { t.panel };
    match t.os {
        Os::Windows => {
            window.paint_quad(fill(b, fill_color));
            if kind == Bevel::Raised {
                edges(window, b, 0.0, 1.0, t.hi, t.lo2);
                edges(window, b, 1.0, 1.0, t.hi2, t.lo);
            } else {
                edges(window, b, 0.0, 1.0, t.lo, t.hi);
                edges(window, b, 1.0, 1.0, t.lo2, t.hi2);
            }
        }
        Os::Linux => {
            window.paint_quad(fill(b, fill_color));
            if kind == Bevel::Raised {
                edges(window, b, 0.0, 2.0, t.hi, t.lo);
            } else {
                edges(window, b, 0.0, 2.0, t.lo, t.hi);
            }
        }
        Os::Mac => {
            window.paint_quad(quad(b, px(radius), fill_color, px(1.0), t.line, BorderStyle::Solid));
            if radius == 0.0 {
                if kind == Bevel::Raised {
                    edges(window, b, 1.0, 1.0, t.hi, t.lo);
                } else {
                    edges(window, b, 1.0, 1.0, t.lo, fill_color);
                }
            }
        }
    }
}

/// A bevelled container. Add children as usual; leave ~3px padding so they
/// do not cover the bevel.
pub fn panel(t: Theme, kind: Bevel) -> Div {
    panel_r(t, kind, 0.0)
}

fn panel_r(t: Theme, kind: Bevel, radius: f32) -> Div {
    div().relative().child(
        canvas(|_, _, _| (), move |b, _, window, _| paint_bevel(&t, b, kind, radius, window))
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
    )
}

/// A push button. `default` marks the main action with the look's outline.
pub fn button(id: impl Into<SharedString>, label: impl Into<SharedString>, t: Theme, default: bool) -> Stateful<Div> {
    sized_button(id, label, t, default, 1.0)
}

/// Like [`button`], scaled up for the one big action on a screen.
pub fn sized_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    t: Theme,
    default: bool,
    scale: f32,
) -> Stateful<Div> {
    let radius = if t.os == Os::Mac { 7.0 * scale } else { 0.0 };
    let face = panel_r(t, Bevel::Raised, radius)
        .h(px(32.0 * scale))
        .px(px(16.0 * scale))
        .text_size(px(14.0 * scale))
        .flex()
        .items_center()
        .justify_center()
        .font_weight(FontWeight::BOLD)
        .text_color(t.text)
        .child(div().relative().child(label.into()));

    let mut outer = div().flex_none();
    if default {
        outer = match t.os {
            Os::Windows => outer.border_1().border_color(t.lo2),
            Os::Mac => outer.p(px(2.0)).border_2().border_color(t.line).rounded(px(11.0)),
            Os::Linux => outer.p(px(3.0)).border_2().border_color(t.lo),
        };
    }
    outer
        .id(id.into())
        .cursor_pointer()
        .active(|s| s.opacity(0.75))
        .child(face)
}

/// The window's own title bar, drawn by us so it matches the look.
pub fn title_bar(t: Theme, title: &'static str) -> Div {
    let glyph_box = move |id: &'static str, glyph: &'static str| {
        panel(t, Bevel::Raised)
            .w(px(18.0))
            .h(px(16.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(10.0))
            .text_color(t.text)
            .child(div().relative().child(glyph))
            .id(id)
            .cursor_pointer()
    };
    let bar = div()
        .flex_none()
        .flex()
        .items_center()
        .font_weight(FontWeight::BOLD)
        .text_size(px(13.0))
        .on_mouse_down(gpui::MouseButton::Left, |_, window, _| window.start_window_move());

    match t.os {
        Os::Windows => bar
            .h(px(24.0))
            .px(px(4.0))
            .gap(px(3.0))
            .bg(t.title)
            .text_color(t.title_text)
            .child(div().flex_1().child(title))
            .child(glyph_box("tb-min", "_").on_click(|_, window, _| window.minimize_window()))
            .child(glyph_box("tb-max", "□").on_click(|_, window, _| window.zoom_window()))
            .child(glyph_box("tb-close", "×").on_click(|_, _, cx| cx.quit())),
        Os::Mac => {
            let stripes = canvas(|_, _, _| (), move |b, _, window, _| {
                let mut y = b.origin.y + px(4.0);
                while y < b.origin.y + b.size.height - px(4.0) {
                    let line = Bounds::new(point(b.origin.x, y), size(b.size.width, px(1.0)));
                    window.paint_quad(fill(line, t.lo));
                    y += px(2.0);
                }
            })
            .absolute()
            .top_0()
            .left(px(6.0))
            .right(px(6.0))
            .h_full();
            let close = div()
                .relative()
                .mx(px(8.0))
                .p(px(2.0))
                .bg(t.face)
                .child(div().size(px(11.0)).border_1().border_color(t.line).bg(t.face))
                .id("tb-close")
                .cursor_pointer()
                .on_click(|_, _, cx| cx.quit());
            bar.relative()
                .h(px(22.0))
                .bg(t.face)
                .border_b_1()
                .border_color(t.line)
                .text_color(t.title_text)
                .child(stripes)
                .child(close)
                .child(div().flex_1())
                .child(div().relative().px(px(10.0)).bg(t.face).child(title))
                .child(div().flex_1())
                .child(div().w(px(31.0)))
        }
        Os::Linux => {
            let cde_box = move |id: &'static str, w: f32, h: f32| {
                let mark = canvas(|_, _, _| (), move |b, _, window, _| {
                    let inner = Bounds::centered_at(b.center(), size(px(w), px(h)));
                    window.paint_quad(fill(inner, t.title));
                    edges(window, inner, 0.0, 1.0, t.hi, t.lo);
                })
                .absolute()
                .top_0()
                .left_0()
                .size_full();
                div()
                    .relative()
                    .size(px(26.0))
                    .bg(t.title)
                    .child(canvas(|_, _, _| (), move |b, _, window, _| edges(window, b, 0.0, 2.0, t.hi, t.lo))
                        .absolute().top_0().left_0().size_full())
                    .child(mark)
                    .id(id)
                    .cursor_pointer()
            };
            bar.h(px(26.0))
                .bg(t.title)
                .text_color(t.title_text)
                .child(cde_box("tb-close", 12.0, 4.0).on_click(|_, _, cx| cx.quit()))
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(canvas(|_, _, _| (), move |b, _, window, _| edges(window, b, 0.0, 2.0, t.hi, t.lo))
                            .absolute().top_0().left_0().size_full())
                        .child(div().relative().child(title)),
                )
                .child(cde_box("tb-min", 4.0, 4.0).on_click(|_, window, _| window.minimize_window()))
                .child(cde_box("tb-max", 12.0, 12.0).on_click(|_, window, _| window.zoom_window()))
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Mood {
    Happy,
    Looking,
}

/// computie itself: a beige retro computer with a face on its screen.
pub fn mascot(side: f32, mood: Mood) -> impl IntoElement {
    canvas(|_, _, _| (), move |b, _, window, _| {
        let u = side / 64.0;
        let at = |x: f32, y: f32| point(b.origin.x + px(x * u), b.origin.y + px(y * u));
        let rect = |x: f32, y: f32, w: f32, h: f32| Bounds::new(at(x, y), size(px(w * u), px(h * u)));
        let ink: Hsla = rgb(0x1E1B16).into();
        let beige: Hsla = rgb(0xE9E3D2).into();
        let green: Hsla = rgb(0x7DF0A0).into();
        let stroke = px((2.0 * u).max(1.0));

        window.paint_quad(quad(rect(8.0, 6.0, 48.0, 38.0), px(7.0 * u), beige, stroke, ink, BorderStyle::Solid));
        window.paint_quad(quad(rect(14.0, 12.0, 36.0, 24.0), px(4.0 * u), rgb(0x1B2030), stroke, ink, BorderStyle::Solid));
        window.paint_quad(quad(rect(25.0, 44.0, 14.0, 5.0), px(0.0), rgb(0xCFC8B4), stroke, ink, BorderStyle::Solid));
        window.paint_quad(quad(rect(10.0, 49.0, 44.0, 9.0), px(3.0 * u), beige, stroke, ink, BorderStyle::Solid));
        window.paint_quad(fill(rect(36.0, 52.5, 10.0, 2.0), ink));
        window.paint_quad(quad(rect(15.4, 51.9, 3.2, 3.2), px(1.6 * u), rgb(0x19B394), px(0.0), ink, BorderStyle::Solid));

        let eye_shift = if mood == Mood::Looking { 4.0 } else { 0.0 };
        for x in [24.0, 36.0] {
            window.paint_quad(quad(rect(x + eye_shift, 17.0, 4.0, 7.0), px(2.0 * u), green, px(0.0), ink, BorderStyle::Solid));
        }
        let mut mouth = PathBuilder::stroke(px((2.6 * u).max(1.0)));
        if mood == Mood::Happy {
            mouth.move_to(at(23.0, 27.0));
            mouth.curve_to(at(41.0, 27.0), at(32.0, 34.0));
        } else {
            mouth.move_to(at(33.0, 29.5));
            mouth.line_to(at(39.0, 29.5));
        }
        if let Ok(path) = mouth.build() {
            window.paint_path(path, green);
        }
    })
    .w(px(side))
    .h(px(side))
    .flex_none()
}
