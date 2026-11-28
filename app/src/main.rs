//! computie: the window, its screens and the glue between UI and scanner.

mod theme;
mod wheel;
mod widgets;

use computie_core::{
    default_roots, format_size, move_to_trash, scan_many, NodeId, ScanHandle, ScanOptions, Tree,
};
use gpui::{
    canvas, deferred, div, point, prelude::*, px, size, AnyElement, App, Application, Bounds,
    Context, Div, FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathPromptOptions, Pixels, Point, SharedString, Window, WindowAppearance, WindowBounds,
    WindowDecorations, WindowOptions,
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use theme::{Mode, Os, Theme, PALETTE};
use widgets::{button, mascot, panel, sized_button, title_bar, Bevel, Mood};

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Start,
    Scanning,
    Results,
    Review,
}

/// One entry in the "Or scan just:" dropdown.
#[derive(Clone)]
enum Place {
    Path(String, PathBuf),
    Choose,
}

struct Drag {
    id: NodeId,
    name: String,
    size: u64,
    color: u32,
    pos: Point<Pixels>,
}

struct Computie {
    theme: Theme,
    screen: Screen,
    home: Option<PathBuf>,
    places: Vec<Place>,
    place: usize,
    place_open: bool,

    scan: Option<ScanHandle>,
    tick: usize,

    tree: Option<Tree>,
    focus: NodeId,
    hover: Option<NodeId>,
    collected: Vec<NodeId>,
    slices: Rc<Vec<wheel::Slice>>,
    wheel_bounds: Rc<Cell<Bounds<Pixels>>>,
    drop_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Slice the mouse went down on, and where; becomes a drag after 6px.
    press: Option<(usize, Point<Pixels>)>,
    drag: Option<Drag>,
    messages: Vec<String>,
    /// Developer switch: after a scan, collect the top items and open the
    /// review screen, so that screen can be checked without a mouse.
    demo_review: bool,
}

impl Computie {
    fn new(theme: Theme) -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        let mut places: Vec<Place> = default_roots()
            .into_iter()
            .map(|p| Place::Path(format!("Disk {}", p.display()), p))
            .collect();
        if let Some(home) = &home {
            places.push(Place::Path("Home folder".into(), home.clone()));
        }
        places.push(Place::Choose);
        Self {
            theme,
            screen: Screen::Start,
            home,
            places,
            place: 0,
            place_open: false,
            scan: None,
            tick: 0,
            tree: None,
            focus: Tree::ROOT,
            hover: None,
            collected: Vec::new(),
            slices: Rc::new(Vec::new()),
            wheel_bounds: Rc::new(Cell::new(Bounds::default())),
            drop_bounds: Rc::new(Cell::new(Bounds::default())),
            press: None,
            drag: None,
            messages: Vec::new(),
            demo_review: false,
        }
    }

    fn start_scan(&mut self, roots: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.scan = Some(scan_many(roots, ScanOptions::default()));
        self.screen = Screen::Scanning;
        self.place_open = false;
        self.collected.clear();
        self.messages.clear();
        // The scan runs on its own threads. This task only wakes up ten times
        // a second to refresh the numbers and to notice when it is done.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            let done = this.update(cx, |this, cx| this.poll_scan(cx)).unwrap_or(true);
            if done {
                break;
            }
        })
        .detach();
        cx.notify();
    }

    /// Returns true once there is nothing left to wait for.
    fn poll_scan(&mut self, cx: &mut Context<Self>) -> bool {
        self.tick += 1;
        cx.notify();
        let finished = self.scan.as_ref().map(|s| s.is_finished()).unwrap_or(true);
        if !finished {
            return false;
        }
        if let Some(handle) = self.scan.take() {
            let cancelled = handle.progress.is_cancelled();
            let tree = handle.join();
            if cancelled {
                self.screen = Screen::Start;
            } else {
                self.tree = Some(tree);
                self.focus = Tree::ROOT;
                self.hover = None;
                self.screen = Screen::Results;
                if self.demo_review {
                    if let Some(tree) = &self.tree {
                        self.collected = tree.children(Tree::ROOT).skip(3).take(3).map(|(id, _)| id).collect();
                    }
                    self.trash_collected();
                    self.screen = Screen::Review;
                }
            }
        }
        true
    }

    fn scan_place(&mut self, cx: &mut Context<Self>) {
        match self.places[self.place].clone() {
            Place::Path(_, path) => self.start_scan(vec![path], cx),
            Place::Choose => {
                let picked = cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some("Scan".into()),
                });
                cx.spawn(async move |this, cx| {
                    if let Ok(Ok(Some(paths))) = picked.await {
                        this.update(cx, |this, cx| this.start_scan(paths, cx)).ok();
                    }
                })
                .detach();
            }
        }
    }

    fn toggle_collected(&mut self, id: NodeId) {
        if let Some(i) = self.collected.iter().position(|&c| c == id) {
            self.collected.remove(i);
        } else {
            self.collected.push(id);
        }
    }

    fn collected_size(&self) -> u64 {
        let Some(tree) = &self.tree else { return 0 };
        self.collected.iter().map(|&id| tree.node(id).size).sum()
    }

    fn trash_collected(&mut self) {
        let Some(tree) = self.tree.as_mut() else { return };
        self.messages.clear();
        let mut kept = Vec::new();
        for id in std::mem::take(&mut self.collected) {
            let path = tree.path(id);
            match move_to_trash(&path, self.home.as_deref()) {
                Ok(()) => tree.remove(id),
                Err(err) => {
                    self.messages.push(err.to_string());
                    kept.push(id);
                }
            }
        }
        self.collected = kept;
        if self.collected.is_empty() {
            self.screen = Screen::Results;
        }
    }

    /// True when `a` is `b`, or one sits above the other in the tree.
    fn related(tree: &Tree, a: NodeId, b: NodeId) -> bool {
        let above = |mut low: NodeId, high: NodeId| loop {
            if low == high {
                return true;
            }
            match tree.node(low).parent {
                Some(p) => low = p,
                None => return false,
            }
        };
        above(a, b) || above(b, a)
    }

    // ---- mouse on the results screen -------------------------------------

    fn mouse_down(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        match wheel::hit(&self.slices, self.wheel_bounds.get(), ev.position) {
            wheel::Hit::Slice(i) => self.press = Some((i, ev.position)),
            wheel::Hit::Center => {
                if let Some(parent) = self.tree.as_ref().and_then(|t| t.node(self.focus).parent) {
                    self.focus = parent;
                    self.hover = None;
                    cx.notify();
                }
            }
            wheel::Hit::Nothing => {}
        }
    }

    fn mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if let Some(drag) = &mut self.drag {
            drag.pos = ev.position;
            cx.notify();
            return;
        }
        if let Some((i, from)) = self.press {
            let (dx, dy) = (f32::from(ev.position.x - from.x), f32::from(ev.position.y - from.y));
            if dx * dx + dy * dy > 36.0 {
                if let (Some(s), Some(tree)) = (self.slices.get(i), &self.tree) {
                    self.drag = Some(Drag {
                        id: s.id,
                        name: s.name.clone(),
                        size: tree.node(s.id).size,
                        color: s.base,
                        pos: ev.position,
                    });
                    cx.notify();
                }
            }
            return;
        }
        let hover = match wheel::hit(&self.slices, self.wheel_bounds.get(), ev.position) {
            wheel::Hit::Slice(i) => Some(self.slices[i].id),
            _ if self.wheel_bounds.get().contains(&ev.position) => None,
            _ => return,
        };
        if hover != self.hover {
            self.hover = hover;
            cx.notify();
        }
    }

    fn mouse_up(&mut self, ev: &MouseUpEvent, cx: &mut Context<Self>) {
        let press = self.press.take();
        if let Some(drag) = self.drag.take() {
            if self.drop_bounds.get().contains(&ev.position) && !self.collected.contains(&drag.id) {
                self.collected.push(drag.id);
            }
            cx.notify();
            return;
        }
        // A press and release on the same slice is a click: open that folder.
        if let (Some((i, _)), wheel::Hit::Slice(j)) =
            (press, wheel::hit(&self.slices, self.wheel_bounds.get(), ev.position))
        {
            if i == j && self.slices[i].openable {
                self.focus = self.slices[i].id;
                self.hover = None;
                cx.notify();
            }
        }
    }

    // ---- screens -----------------------------------------------------------

    fn header(&self, extra: Vec<AnyElement>, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let mode_label = if t.mode == Mode::Dark { "Light" } else { "Dark" };
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .py(px(6.0))
            .border_b_1()
            .border_color(t.lo)
            .child(mascot(28.0, Mood::Happy))
            .child(div().font_weight(FontWeight::BOLD).text_size(px(16.0)).child("computie"))
            .child(div().flex_1())
            .children(extra)
            .child(button("mode", mode_label, t, false).on_click(cx.listener(|this, _, _, cx| {
                this.theme = this.theme.toggled();
                cx.notify();
            })))
    }

    fn start_screen(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let place_label = |p: &Place| match p {
            Place::Path(label, _) => label.clone(),
            Place::Choose => "Choose a folder…".to_string(),
        };
        let mut picker = panel(t, Bevel::Sunken)
            .w(px(280.0))
            .h(px(32.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .child(div().relative().flex_1().truncate().child(place_label(&self.places[self.place])))
            .child(div().relative().child("▾"))
            .id("place")
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.place_open = !this.place_open;
                cx.notify();
            }));
        if self.place_open {
            let mut list = panel(t, Bevel::Raised)
                .absolute()
                .top(px(32.0))
                .left_0()
                .w(px(280.0))
                .p(px(3.0))
                .flex()
                .flex_col();
            for (i, place) in self.places.iter().enumerate() {
                let selected = i == self.place;
                list = list.child(
                    div()
                        .relative()
                        .px(px(8.0))
                        .py(px(5.0))
                        .when(selected, |d| d.bg(t.sel).text_color(t.title_text))
                        .child(place_label(place))
                        .id(("place-row", i))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.place = i;
                            this.place_open = false;
                            cx.notify();
                        })),
                );
            }
            picker = picker.child(deferred(list));
        }

        let roots: Vec<PathBuf> = default_roots();
        let disks = if roots.len() == 1 { "1 disk".to_string() } else { format!("{} disks", roots.len()) };

        div()
            .flex_1()
            .flex()
            .flex_col()
            .child(self.header(Vec::new(), cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(18.0))
                    .child(mascot(150.0, Mood::Happy))
                    .child(div().text_size(px(40.0)).font_weight(FontWeight::BOLD).child("Where did my space go?"))
                    .child(
                        div()
                            .text_color(t.muted)
                            .text_size(px(16.0))
                            .child("One click and computie maps every disk on this computer."),
                    )
                    .child(
                        sized_button("scan-all", "Scan everything", t, true, 1.6).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.start_scan(default_roots(), cx);
                            },
                        )),
                    )
                    .child(div().text_color(t.muted).text_size(px(13.0)).child(disks))
                    .child(
                        div()
                            .mt(px(14.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(div().font_weight(FontWeight::BOLD).child("Or scan just:"))
                            .child(picker)
                            .child(button("scan-one", "Scan", t, false).on_click(cx.listener(|this, _, _, cx| {
                                this.scan_place(cx);
                            }))),
                    ),
            )
            .child(
                panel(t, Bevel::Raised).flex_none().mx(px(6.0)).mb(px(6.0)).px(px(10.0)).py(px(5.0)).child(
                    div()
                        .relative()
                        .text_size(px(13.0))
                        .child("Scanning only looks. Nothing is removed without your OK, and removed items go to the Trash."),
                ),
            )
    }

    fn scanning_screen(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let (files, bytes, current) = match &self.scan {
            Some(s) => (s.progress.files(), s.progress.bytes(), s.progress.current()),
            None => (0, 0, PathBuf::new()),
        };
        // We do not know the total in advance, so the bar is a block that
        // slides back and forth instead of a percentage.
        let tick = self.tick;
        let sel = t.sel;
        let bar = panel(t, Bevel::Sunken).w(px(420.0)).h(px(22.0)).child(
            canvas(|_, _, _| (), move |b, _, window, _| {
                let blocks = 24usize;
                let head = tick % (blocks * 2);
                let head = if head >= blocks { blocks * 2 - 1 - head } else { head };
                let bw = (f32::from(b.size.width) - 8.0) / blocks as f32;
                for k in 0..6usize {
                    let Some(i) = head.checked_sub(k) else { break };
                    let x = b.origin.x + px(4.0 + i as f32 * bw);
                    let block = Bounds::new(point(x, b.origin.y + px(4.0)), size(px(bw - 3.0), b.size.height - px(8.0)));
                    window.paint_quad(gpui::fill(block, sel));
                }
            })
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        );
        div()
            .flex_1()
            .flex()
            .flex_col()
            .child(self.header(Vec::new(), cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(14.0))
                    .child(mascot(110.0, Mood::Looking))
                    .child(div().text_size(px(36.0)).font_weight(FontWeight::BOLD).child(format_size(bytes)))
                    .child(div().text_color(t.muted).child("found so far"))
                    .child(div().text_size(px(24.0)).font_weight(FontWeight::BOLD).child("Peeking into every folder…"))
                    .child(bar)
                    .child(div().child(format!("{} files", group_digits(files))))
                    .child(
                        div()
                            .max_w(px(600.0))
                            .truncate()
                            .text_color(t.muted)
                            .text_size(px(12.0))
                            .child(current.display().to_string()),
                    )
                    .child(button("cancel", "Cancel", t, false).on_click(cx.listener(|this, _, _, _| {
                        if let Some(scan) = &this.scan {
                            scan.progress.cancel();
                        }
                    }))),
            )
    }

    fn results_screen(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let Some(tree) = &self.tree else { return div() };
        let focus = self.focus;
        let node = tree.node(focus);

        // Breadcrumb: the chain of folders from the top down to here.
        let mut chain = vec![focus];
        while let Some(parent) = tree.node(*chain.last().unwrap()).parent {
            chain.push(parent);
        }
        chain.reverse();
        let mut crumbs = panel(t, Bevel::Sunken).flex_1().h(px(32.0)).px(px(8.0)).flex().items_center().gap(px(6.0)).overflow_hidden();
        for (i, &id) in chain.iter().enumerate() {
            let last = i + 1 == chain.len();
            if i > 0 {
                crumbs = crumbs.child(div().relative().text_color(t.muted).child("▸"));
            }
            crumbs = crumbs.child(
                div()
                    .relative()
                    .when(last, |d| d.font_weight(FontWeight::BOLD))
                    .child(short_name(&tree.node(id).name))
                    .id(("crumb", i))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.focus = id;
                        this.hover = None;
                        cx.notify();
                    })),
            );
        }
        let rescan = button("rescan", "New scan", t, false)
            .on_click(cx.listener(|this, _, _, cx| {
                this.screen = Screen::Start;
                cx.notify();
            }))
            .into_any_element();

        // The wheel.
        let slices = Rc::new(wheel::layout(tree, focus));
        self.slices = slices.clone();
        let dim: Vec<bool> = match self.hover {
            Some(h) => slices.iter().map(|s| !Self::related(tree, s.id, h)).collect(),
            None => vec![false; slices.len()],
        };
        let shown = self.hover.map(|h| tree.node(h)).unwrap_or(node);
        let hint = match self.hover {
            Some(_) if node.size > 0 => format!("{:.0}% of this view", 100.0 * shown.size as f64 / node.size as f64),
            Some(_) => String::new(),
            None if node.parent.is_some() => "Click to go up".to_string(),
            None => format!("{} files", group_digits(node.files)),
        };
        let center = wheel::CenterText {
            name: short_name(&shown.name).into(),
            size: format_size(shown.size).into(),
            hint: hint.into(),
        };
        let wheel_bounds = self.wheel_bounds.clone();
        let wheel_canvas = canvas(|_, _, _| (), move |b, _, window, cx| {
            wheel_bounds.set(b);
            wheel::paint(&slices, &dim, &center, &t, b, window, cx);
        })
        .size_full();

        // The collector: drop target for dragged slices.
        let dragging = self.drag.is_some();
        let over = self.drag.as_ref().map(|d| self.drop_bounds.get().contains(&d.pos)).unwrap_or(false);
        let count = self.collected.len();
        let collector_hint = if over {
            "Let go to add it".to_string()
        } else if dragging {
            "Drop the slice here".to_string()
        } else if count == 0 {
            "Drag slices here, or use + in the list".to_string()
        } else {
            format!("{} {} ready to review", count, if count == 1 { "item" } else { "items" })
        };
        let drop_bounds = self.drop_bounds.clone();
        let collector = panel(t, Bevel::Sunken)
            .flex_none()
            .w(px(460.0))
            .p(px(10.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .when(dragging, |d| d.border_2().border_color(if over { t.sel } else { t.muted }))
            .child(canvas(move |b, _, _| drop_bounds.set(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            .child(
                div()
                    .relative()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(FontWeight::BOLD).child(format!(
                        "Collector · {}",
                        if count == 0 { "empty".to_string() } else { format_size(self.collected_size()) }
                    )))
                    .child(div().text_size(px(13.0)).text_color(t.muted).child(collector_hint)),
            )
            .child(button("review", "Review…", t, false).on_click(cx.listener(|this, _, _, cx| {
                if !this.collected.is_empty() {
                    this.screen = Screen::Review;
                    cx.notify();
                }
            })));

        // The list beside the wheel: same items, biggest first.
        let biggest = tree.children(focus).next().map(|(_, n)| n.size).unwrap_or(1).max(1);
        let mut list = panel(t, Bevel::Sunken).flex_1().p(px(3.0)).flex().flex_col().overflow_hidden();
        let mut rows = div().relative().flex().flex_col();
        for (i, (id, child)) in tree.children(focus).take(200).enumerate() {
            let color = theme::tint(PALETTE[i % PALETTE.len()], 0.0);
            let is_in = self.collected.contains(&id);
            let hovered = self.hover.map(|h| Self::related(tree, h, id) && h != focus).unwrap_or(false);
            let openable = !child.children.is_empty();
            let pct = (child.size as f32 / biggest as f32).clamp(0.02, 1.0);
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(6.0))
                    .py(px(4.0))
                    .when(hovered, |d| d.bg(t.track))
                    .child(div().flex_none().size(px(12.0)).bg(color).border_1().border_color(t.line))
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.0))
                                    .child(div().flex_1().truncate().font_weight(FontWeight::BOLD).child(short_name(&child.name)))
                                    .child(div().flex_none().child(format_size(child.size))),
                            )
                            .child(div().h(px(5.0)).bg(t.track).child(div().h_full().w(gpui::relative(pct)).bg(color))),
                    )
                    .id(("row", id as usize))
                    .when(openable, |d| d.cursor_pointer())
                    .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                        this.hover = if *inside { Some(id) } else { None };
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if openable {
                            this.focus = id;
                            this.hover = None;
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .flex_none()
                            .w(px(24.0))
                            .h(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .font_weight(FontWeight::BOLD)
                            .child(if is_in { "✓" } else { "+" })
                            .id(("collect", id as usize))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_collected(id);
                                cx.notify();
                            })),
                    ),
            );
        }
        list = list.child(rows.id("rows").overflow_y_scroll().size_full());

        let unreadable = if node.errors > 0 {
            format!(" · {} could not be read", group_digits(node.errors as u64))
        } else {
            String::new()
        };

        div()
            .flex_1()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(self.header(vec![crumbs.into_any_element(), rescan], cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, ev, _, cx| this.mouse_down(ev, cx)))
                    .on_mouse_move(cx.listener(|this, ev, _, cx| this.mouse_move(ev, cx)))
                    .on_mouse_up(MouseButton::Left, cx.listener(|this, ev, _, cx| this.mouse_up(ev, cx)))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.0))
                            .p(px(12.0))
                            .overflow_hidden()
                            .child(div().flex_1().w_full().overflow_hidden().child(wheel_canvas))
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(13.0))
                                    .text_color(t.muted)
                                    .child("Click a slice to open it. Drag a slice to the collector to get rid of it."),
                            )
                            .child(collector),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(340.0))
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .p(px(12.0))
                            .overflow_hidden()
                            .child(div().text_size(px(22.0)).font_weight(FontWeight::BOLD).truncate().child(short_name(&node.name)))
                            .child(div().text_size(px(13.0)).text_color(t.muted).child(format!(
                                "{} · {} items{}",
                                format_size(node.size),
                                group_digits(node.children.len() as u64),
                                unreadable
                            )))
                            .child(list),
                    ),
            )
    }

    fn review_screen(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let Some(tree) = &self.tree else { return div() };
        let mut list = panel(t, Bevel::Sunken).p(px(3.0)).flex().flex_col();
        for (i, &id) in self.collected.iter().enumerate() {
            let node = tree.node(id);
            list = list.child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(8.0))
                    .py(px(6.0))
                    .when(i > 0, |d| d.border_t_1().border_color(t.track))
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::BOLD).truncate().child(short_name(&node.name)))
                            .child(div().text_size(px(12.0)).text_color(t.muted).truncate().child(tree.path(id).display().to_string())),
                    )
                    .child(div().flex_none().child(format_size(node.size)))
                    .child(
                        div()
                            .flex_none()
                            .w(px(24.0))
                            .text_center()
                            .font_weight(FontWeight::BOLD)
                            .child("×")
                            .id(("uncollect", i))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_collected(id);
                                if this.collected.is_empty() {
                                    this.screen = Screen::Results;
                                }
                                cx.notify();
                            })),
                    ),
            );
        }
        let mut problems = div().flex().flex_col().gap(px(4.0));
        for message in &self.messages {
            problems = problems.child(div().text_size(px(13.0)).text_color(t.warn).child(message.clone()));
        }
        let count = self.collected.len();
        let back = button("back", "Back to map", t, false)
            .on_click(cx.listener(|this, _, _, cx| {
                this.screen = Screen::Results;
                cx.notify();
            }))
            .into_any_element();

        div()
            .flex_1()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(self.header(vec![back], cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .gap(px(24.0))
                    .p(px(24.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(10.0))
                            .overflow_hidden()
                            .child(div().text_size(px(30.0)).font_weight(FontWeight::BOLD).child("The collector"))
                            .child(
                                div()
                                    .text_color(t.muted)
                                    .child("Check the list before anything moves. Nothing here has been touched yet."),
                            )
                            .child(list)
                            .child(problems),
                    )
                    .child(
                        panel(t, Bevel::Raised).flex_none().w(px(320.0)).p(px(18.0)).child(
                            div()
                                .relative()
                                .flex()
                                .flex_col()
                                .gap(px(12.0))
                                .child(div().text_color(t.muted).child("You’ll free up"))
                                .child(div().text_size(px(40.0)).font_weight(FontWeight::BOLD).child(format_size(self.collected_size())))
                                .child(div().text_size(px(17.0)).font_weight(FontWeight::BOLD).child(format!(
                                    "Move {} {} to the Trash?",
                                    count,
                                    if count == 1 { "item" } else { "items" }
                                )))
                                .child(div().text_size(px(13.0)).text_color(t.muted).child(
                                    "They stay in the Trash until you empty it, so you can put them back.",
                                ))
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .items_center()
                                        .gap(px(12.0))
                                        .child(button("cancel", "Cancel", t, false).on_click(cx.listener(|this, _, _, cx| {
                                            this.screen = Screen::Results;
                                            cx.notify();
                                        })))
                                        .child(button("trash", "Move to Trash", t, true).on_click(cx.listener(
                                            |this, _, _, cx| {
                                                this.trash_collected();
                                                cx.notify();
                                            },
                                        ))),
                                ),
                        ),
                    ),
            )
    }
}

impl Render for Computie {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let screen = match self.screen {
            Screen::Start => self.start_screen(cx),
            Screen::Scanning => self.scanning_screen(cx),
            Screen::Results => self.results_screen(cx),
            Screen::Review => self.review_screen(cx),
        };
        let chip = self.drag.as_ref().map(|d| {
            deferred(
                panel(t, Bevel::Raised)
                    .absolute()
                    .left(d.pos.x + px(12.0))
                    .top(d.pos.y + px(12.0))
                    .px(px(10.0))
                    .py(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().relative().size(px(12.0)).bg(theme::tint(d.color, 0.0)).border_1().border_color(t.line))
                    .child(div().relative().font_weight(FontWeight::BOLD).child(short_name(&d.name)))
                    .child(div().relative().child(format_size(d.size))),
            )
        });
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(t.face)
            .text_color(t.text)
            .text_size(px(14.0))
            .child(title_bar(t, "computie"))
            .child(screen)
            .children(chip)
    }
}

/// Last part of a path-like name, so "/home/sam" shows as "sam".
fn short_name(name: &str) -> SharedString {
    let trimmed = name.trim_end_matches(['/', '\\']);
    let short = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    if short.is_empty() { name.to_string().into() } else { short.to_string().into() }
}

/// 1284907 -> "1,284,907"
fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn main() {
    // `computie <folder>` skips the start screen and scans that folder.
    let auto_scan = std::env::args().nth(1).map(PathBuf::from);
    // Developer switches for trying the other looks on one machine.
    let os = match std::env::var("COMPUTIE_STYLE").as_deref() {
        Ok("mac") => Os::Mac,
        Ok("windows") => Os::Windows,
        Ok("linux") => Os::Linux,
        _ => Theme::native_os(),
    };
    let forced_mode = match std::env::var("COMPUTIE_MODE").as_deref() {
        Ok("dark") => Some(Mode::Dark),
        Ok("light") => Some(Mode::Light),
        _ => None,
    };

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: None,
                window_decorations: Some(WindowDecorations::Client),
                window_min_size: Some(size(px(860.0), px(600.0))),
                app_id: Some("computie".into()),
                ..Default::default()
            },
            move |window, cx| {
                let system_dark = matches!(
                    window.appearance(),
                    WindowAppearance::Dark | WindowAppearance::VibrantDark
                );
                let mode = forced_mode.unwrap_or(if system_dark { Mode::Dark } else { Mode::Light });
                cx.new(|cx| {
                    let mut app = Computie::new(Theme::new(os, mode));
                    if let Some(path) = auto_scan {
                        app.start_scan(vec![path], cx);
                    }
                    app.demo_review = std::env::var_os("COMPUTIE_DEMO_REVIEW").is_some();
                    app
                })
            },
        )
        .unwrap();
        cx.on_window_closed(|cx| cx.quit()).detach();
        cx.activate(true);
    });
}
