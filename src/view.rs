use iced::font::Weight::{Black, Medium, Normal};
use iced::widget::tooltip::Position::{Bottom, Left, Right, Top};
use iced::widget::{
    Row, Space, button, column, container, hover, image, mouse_area, row, scrollable, slider,
    stack, text, text_editor, text_input, tooltip,
};
use iced::{Background, Border, Center, Color, Element, Fill, FillPortion, color, padding};
use std::borrow::Cow;

use crate::api::Artist;
use crate::app::{App, Lib, MenuKind, Message, act, set, task};
use crate::pages;
use crate::theme::{
    self, ACCENT, BG, Btn, CHIP, ELEVATED, SUB, TEXT, Txt, bold, fill, icon as i, sub, txt,
};

pub const SCROLL: &str = "main-scroll";
pub const SONGS_BG: Color = color!(0x006450);
pub type El<'a> = Element<'a, Message>;

static CAPS: std::sync::LazyLock<[image::Handle; 4]> = std::sync::LazyLock::new(|| {
    [(0, 0), (15, 0), (0, 15), (15, 15)].map(|(fx, fy)| {
        let px = (0..256).flat_map(|i: u32| {
            let (x, y) = ((i % 16).abs_diff(fx) as f32, (i / 16).abs_diff(fy) as f32);
            let d = |a: f32, s: u32| 15.875 - a - s as f32 / 4.0;
            let out = (0..16).filter(|s| d(x, s % 4).hypot(d(y, s / 4)) > 16.0);
            [0, 0, 0, (out.count() * 255 / 16) as u8]
        });
        image::Handle::from_rgba(16, 16, px.collect::<Vec<u8>>())
    })
});

pub fn btn<'a>(c: impl Into<El<'a>>, msg: Message) -> Btn<'a, Message> {
    theme::link_btn(c, TEXT, msg)
}

pub fn go(kind: &str, id: &str) -> Message {
    match id.is_empty() {
        true => Message::Noop,
        false => Message::Nav(format!("{kind}:{id}")),
    }
}

pub fn icon_btn<'a>(c: char, size: f32, msg: Message) -> Btn<'a, Message> {
    let b = theme::sq(i(c, size).center(), size * 2.0);
    b.on_press(msg).style(theme::link(SUB))
}

pub fn tool_btn<'a>(c: char, lit: bool, msg: Message) -> Btn<'a, Message> {
    let b = theme::sq(i(c, 20.0).center(), 32.0);
    b.style(theme::link([SUB, TEXT][usize::from(lit)]))
        .on_press(msg)
}

pub fn queue_icon<'a>(s: f32, c: Color) -> El<'a> {
    let bar = move |h: f32| container(Space::new().width(s * 1.03).height(h));
    let rect = bar(s * 0.375).style(move |_| {
        let b = iced::border::rounded(s * 0.17).width(s * 0.11).color(c);
        container::Style::default().border(b)
    });
    let line = || bar(s * 0.125).style(fill(c, 0.0));
    column![rect, line(), line()].spacing(s * 0.19).into()
}

pub fn focusable<'a>(
    c: impl Into<El<'a>>,
    msg: Option<Message>,
    radius: f32,
    id: Option<&'static str>,
) -> El<'a> {
    let id = id.map(iced::advanced::widget::Id::from);
    Element::new(wrap::Wrap(
        c.into(),
        Some((msg, radius, id)),
        None,
        0,
        usize::MAX,
        (Vec::new(), 0.0),
    ))
}

pub fn live<'a>(
    clock: Box<dyn Fn() -> f32>,
    run: bool,
    build: impl Fn(f32) -> El<'a> + 'a,
) -> El<'a> {
    let p = clock();
    Element::new(wrap::Wrap(
        build(p),
        None,
        Some((clock, Box::new(build), run, p)),
        0,
        usize::MAX,
        (Vec::new(), 0.0),
    ))
}

pub fn smooth(c: El<'_>, on: bool, lyr: usize) -> El<'_> {
    Element::new(wrap::Wrap(
        c,
        None,
        None,
        usize::from(on),
        lyr,
        (Vec::new(), 0.0),
    ))
}

pub fn pane<'a>(c: impl Into<El<'a>>) -> El<'a> {
    Element::new(wrap::Wrap(
        c.into(),
        None,
        None,
        2,
        usize::MAX,
        (Vec::new(), 0.0),
    ))
}

pub fn hov<'a>(
    m: iced::widget::MouseArea<'a, Message>,
    tag: String,
    back: Option<&'static str>,
) -> iced::widget::MouseArea<'a, Message> {
    let t = tag.clone();
    m.on_enter(act(move |a| a.hover = Some(tag.clone())))
        .on_exit(act(move |a| {
            a.hover = a.hover.take().filter(|h| *h != t).or(back.map(Into::into))
        }))
}

pub fn album_of(s: &crate::api::Song) -> Message {
    match s.url.is_empty() {
        true => go("album", &s.album_id),
        false => Message::Nav("radio".into()),
    }
}

pub fn shade<'a>(g: char, r: f32, m: Message) -> El<'a> {
    let top = container(theme::filled(g, 20.0).color(TEXT)).center(48);
    let top = button(top.style(fill(BG.scale_alpha(0.5), r))).padding(0);
    top.style(theme::link(TEXT)).on_press(m).into()
}

pub fn vgap<'a>(c: impl Into<El<'a>>, lo: usize) -> El<'a> {
    Element::new(wrap::Wrap(
        c.into(),
        None,
        None,
        3 + lo,
        usize::MAX,
        (Vec::new(), 0.0),
    ))
}

pub fn back<'a>(stops: &[(f32, Color)], h: f32, c: impl Into<El<'a>>) -> El<'a> {
    Element::new(wrap::Wrap(
        c.into(),
        None,
        None,
        0,
        usize::MAX,
        (stops.to_vec(), h),
    ))
}

pub fn bands<'a>(stops: &[(f32, Color)]) -> El<'a> {
    back(stops, 0.0, Space::new().width(Fill).height(Fill))
}

mod wrap {
    use super::{El, Message, TEXT};
    use iced::advanced::renderer::{Quad, Style};
    use iced::advanced::widget::{Id, Operation, Tree, Widget, operation::Focusable, tree};
    use iced::advanced::{Clipboard, Layout, Renderer, Shell, layout};
    use iced::keyboard::{Event::KeyPressed, Event::ModifiersChanged, Key, key::Named};
    use iced::mouse::{Event::CursorMoved, Event::WheelScrolled, ScrollDelta};
    use iced::window::Event::RedrawRequested;
    use iced::{
        Color, Event, Length, Point, Rectangle as Rect, Size, mouse::Cursor, mouse::Interaction,
    };
    type R = iced::Renderer;
    type T = iced::Theme;
    type Live<'a> = (
        Box<dyn Fn() -> f32>,
        Box<dyn Fn(f32) -> El<'a> + 'a>,
        bool,
        f32,
    );
    type Focus = (Option<Message>, f32, Option<Id>);
    pub struct Wrap<'a>(
        pub El<'a>,
        pub Option<Focus>,
        pub Option<Live<'a>>,
        pub usize,
        pub usize,
        pub (Vec<(f32, Color)>, f32),
    );
    #[derive(Default)]
    struct Ring(
        bool,
        f32,
        Option<std::time::Instant>,
        Point,
        bool,
        usize,
        Option<f32>,
    );
    struct Scan(Option<(Rect, Rect, f32)>, Option<(f32, f32)>, Option<f32>);
    impl Operation for Scan {
        fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn Operation)) {
            if self.0.is_none() || self.1.is_none() && self.2.is_none() {
                f(self);
            }
        }
        fn scrollable(
            &mut self,
            id: Option<&Id>,
            b: Rect,
            c: Rect,
            v: iced::Vector,
            s: &mut dyn iced::advanced::widget::operation::Scrollable,
        ) {
            if id == Some(&Id::from(super::SCROLL)) {
                self.0 = Some((b, c, v.y));
                if let Some(y) = self.2 {
                    s.scroll_to(iced::widget::operation::AbsoluteOffset {
                        x: Some(0.0),
                        y: Some(y),
                    });
                }
            }
        }
        fn container(&mut self, id: Option<&Id>, b: Rect) {
            if id == Some(&Id::from("lyric")) {
                self.1 = Some((b.y, b.height));
            }
        }
    }
    impl Focusable for Ring {
        fn is_focused(&self) -> bool {
            self.0
        }
        fn focus(&mut self) {
            self.0 = true;
        }
        fn unfocus(&mut self) {
            self.0 = false;
        }
    }
    impl Widget<Message, T, R> for Wrap<'_> {
        fn size(&self) -> Size<Length> {
            self.0.as_widget().size()
        }
        fn layout(&mut self, t: &mut Tree, r: &R, l: &layout::Limits) -> layout::Node {
            self.0.as_widget_mut().layout(&mut t.children[0], r, l)
        }
        fn draw(&self, t: &Tree, r: &mut R, th: &T, s: &Style, l: Layout, c: Cursor, v: &Rect) {
            if !self.5.0.is_empty() {
                let mut b = l.bounds();
                if self.5.1 > 0.0 {
                    b.height = b.height.min(self.5.1);
                }
                let n = (b.height / 4.0).ceil().max(1.0);
                let at = |f: f32| {
                    let i = self
                        .5
                        .0
                        .iter()
                        .position(|s| s.0 >= f)
                        .unwrap_or(self.5.0.len() - 1);
                    let (a, z) = (self.5.0[i.saturating_sub(1)], self.5.0[i]);
                    let k = ((f - a.0) / (z.0 - a.0)).clamp(0.0, 1.0);
                    let m = |x: f32, y: f32| x + (y - x) * if k.is_nan() { 1.0 } else { k };
                    let (a, z) = (a.1, z.1);
                    Color::from_rgba(m(a.r, z.r), m(a.g, z.g), m(a.b, z.b), m(a.a, z.a))
                };
                if b.intersects(v) {
                    for i in 0..n as usize {
                        let y0 = (b.y + b.height * i as f32 / n).round();
                        let y1 = (b.y + b.height * (i + 1) as f32 / n).round();
                        let q = Rect::new(Point::new(b.x, y0), Size::new(b.width, y1 - y0));
                        let c = at((i as f32 + 0.5) / n);
                        if q.intersects(v) && c.a > 0.0 {
                            r.fill_quad(
                                Quad {
                                    bounds: q,
                                    ..Quad::default()
                                },
                                c,
                            );
                        }
                    }
                }
            }
            let w = self.0.as_widget();
            let c = if self.3 == 2 && t.state.downcast_ref::<Ring>().4 {
                Cursor::Unavailable
            } else {
                c
            };
            match l.bounds().intersection(v).filter(|_| self.3 == 2) {
                Some(mut b) => {
                    b.height -= (v.y * 0.618).fract() * 0.01;
                    let e = b.expand(1.0);
                    r.with_layer(b, |r| w.draw(&t.children[0], r, th, s, l, c, &e));
                }
                None => w.draw(&t.children[0], r, th, s, l, c, v),
            }
            if let Some((_, rad, _)) = self.1
                && t.state.downcast_ref::<Ring>().0
            {
                let (e, mut q) = (rad.min(4.0), Quad::default());
                q.border = iced::border::rounded(rad + e).color(TEXT).width(2);
                q.bounds = l.bounds().expand(e);
                r.fill_quad(q, Color::TRANSPARENT);
            }
        }
        fn tag(&self) -> tree::Tag {
            tree::Tag::of::<Ring>()
        }
        fn state(&self) -> tree::State {
            tree::State::new(Ring(
                false,
                if self.3 == 2 { f32::NAN } else { 0.0 },
                None,
                Point::ORIGIN,
                false,
                self.3.checked_sub(3).unwrap_or(usize::MAX),
                None,
            ))
        }
        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.0)]
        }
        fn diff(&self, t: &mut Tree) {
            if let Some(lo) = self.3.checked_sub(3) {
                let old = std::mem::replace(&mut t.state.downcast_mut::<Ring>().5, lo);
                let c = &mut t.children[0].children;
                let (m, d) = (c.len().saturating_sub(2), lo.abs_diff(old));
                if d > 0 && d < m {
                    match lo > old {
                        true => c[1..=m].rotate_left(d),
                        false => c[1..=m].rotate_right(d),
                    }
                }
            }
            t.diff_children(std::slice::from_ref(&self.0));
        }
        fn operate(&mut self, t: &mut Tree, l: Layout, r: &R, op: &mut dyn Operation) {
            if let Some((_, _, id)) = &self.1 {
                op.focusable(id.as_ref(), l.bounds(), t.state.downcast_mut::<Ring>());
            }
            op.traverse(&mut |op| self.0.as_widget_mut().operate(&mut t.children[0], l, r, op));
        }
        fn update(
            &mut self,
            t: &mut Tree,
            e: &Event,
            l: Layout,
            c: Cursor,
            r: &R,
            cb: &mut dyn Clipboard,
            sh: &mut Shell<Message>,
            v: &Rect,
        ) {
            if let (Event::Window(RedrawRequested(now)), Some((clock, build, true, last))) =
                (e, &mut self.2)
            {
                let p = clock();
                if p.floor() != last.floor() {
                    (self.0, *last) = (build(p), p);
                    t.diff_children(std::slice::from_ref(&self.0));
                    let size = l.bounds().size();
                    let limits = layout::Limits::new(size, size);
                    self.0
                        .as_widget_mut()
                        .layout(&mut t.children[0], r, &limits);
                }
                let wait = p.floor() + 1.15 - p;
                sh.request_redraw_at(*now + std::time::Duration::from_secs_f32(wait));
            }
            use std::sync::atomic::Ordering::Relaxed;
            match (e, &self.1, &self.2) {
                _ if self.3 >= 2 => {}
                (Event::Mouse(CursorMoved { position: p }), None, None) => {
                    let bits = (p.x.to_bits() as u64) << 32 | p.y.to_bits() as u64;
                    crate::app::CURSOR.store(bits, Relaxed);
                }
                (
                    Event::Mouse(
                        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)
                        | iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
                    ),
                    None,
                    None,
                ) => {
                    let down = matches!(e, Event::Mouse(iced::mouse::Event::ButtonPressed(_)));
                    crate::app::DOWN.store(down, Relaxed);
                }
                (Event::Keyboard(ModifiersChanged(m)), None, None) => {
                    crate::app::MODS.store(m.bits(), Relaxed);
                }
                _ => {}
            }
            let w = self.0.as_widget_mut();
            let c0 = &mut t.children[0];
            let scan = |w: &mut dyn Widget<Message, T, R>, c0: &mut Tree, a, b| {
                let mut s = Scan(None, a, b);
                w.operate(c0, l, r, &mut s);
                (s.0, s.1)
            };
            let frame = std::time::Duration::from_millis(16);
            let ring = t.state.downcast_mut::<Ring>();
            if self.3 == 2 {
                match e {
                    Event::Mouse(
                        CursorMoved { .. }
                        | iced::mouse::Event::ButtonPressed(_)
                        | iced::mouse::Event::ButtonReleased(_),
                    ) => {
                        (ring.2, ring.4) = (None, false);
                    }
                    Event::Window(RedrawRequested(now)) => {
                        let old = std::mem::replace(&mut ring.1, v.y);
                        if (v.y - old).abs() > 1e-3 {
                            let due = *now + std::time::Duration::from_millis(80);
                            ring.2 = Some(due);
                            sh.request_redraw_at(due);
                            if std::mem::replace(&mut ring.4, true) {
                                return;
                            }
                        } else if let Some(due) = ring.2.filter(|d| *d > *now) {
                            sh.request_redraw_at(due);
                            return;
                        } else {
                            (ring.2, ring.4) = (None, false);
                        }
                    }
                    _ => {}
                }
            }
            let c = if self.3 == 2 && ring.4 {
                Cursor::Unavailable
            } else {
                c
            };
            let root = self.3 <= 1 && self.1.is_none() && self.2.is_none() && self.5.0.is_empty();
            let mut pass = |w: &mut dyn Widget<Message, T, R>,
                            c0: &mut Tree,
                            e: &Event,
                            c: Cursor,
                            sh: &mut Shell<Message>,
                            glide: bool|
             -> bool {
                if !root {
                    w.update(c0, e, l, c, r, cb, sh, v);
                    return sh.is_event_captured();
                }
                let mut msgs = Vec::new();
                let mut sub = Shell::new(&mut msgs);
                w.update(c0, e, l, c, r, cb, &mut sub, v);
                let hit = sub.is_event_captured();
                if hit && !glide {
                    sh.capture_event();
                }
                if sub.is_layout_invalid() {
                    sh.invalidate_layout();
                }
                if sub.are_widgets_invalid() {
                    sh.invalidate_widgets();
                }
                if !glide {
                    sh.request_redraw_at(sub.redraw_request());
                }
                sh.request_input_method(sub.input_method());
                for m in msgs {
                    if !matches!(m, Message::Noop) {
                        sh.publish(m);
                    }
                }
                hit
            };
            if matches!(e, Event::Window(iced::window::Event::Resized(_))) && self.3 <= 1 {
                ring.5 = usize::MAX;
            }
            if self.4 >= usize::MAX - 1 && root {
                (ring.5, ring.6) = (self.4, None);
            } else if self.4 < usize::MAX - 1
                && let Event::Window(RedrawRequested(now)) = e
            {
                if ring.5 != self.4
                    && let (Some((b, ch, off)), Some((ly, lh))) = scan(w, c0, None, None)
                {
                    let old = std::mem::replace(&mut ring.5, self.4);
                    ring.1 = 0.0;
                    let max = (ch.height - b.height).max(0.0);
                    let y = (ly - ch.y + lh * 0.5 - b.height * 0.45).clamp(0.0, max);
                    let near = old == usize::MAX - 1 || (y - off).abs() < b.height;
                    match self.3 == 1 && old != usize::MAX && near && (y - off).abs() >= 1.0 {
                        true => {
                            ring.2 = ring.2.filter(|_| ring.6.is_some());
                            ring.6 = Some(y);
                        }
                        false => {
                            ring.6 = None;
                            scan(w, c0, None, Some(y));
                        }
                    }
                }
                if let Some(y) = ring.6 {
                    match ring.2.map(|t| t + frame).filter(|d| d > now) {
                        Some(due) => sh.request_redraw_at(due),
                        None => {
                            let off = scan(w, c0, Some((0.0, 0.0)), None).0.map_or(y, |x| x.2);
                            let dt = ring.2.map_or(1.0 / 60.0, |t| (*now - t).as_secs_f32());
                            let rem = y - off;
                            let next = match rem.abs() < 1.0 {
                                true => {
                                    ring.6 = None;
                                    y
                                }
                                false => {
                                    ring.2 = Some(*now);
                                    sh.request_redraw_at(*now + frame);
                                    off + rem * (1.0 - (-dt / 0.08).exp())
                                }
                            };
                            scan(w, c0, None, Some(next));
                        }
                    }
                }
            }
            if self.4 < usize::MAX - 1
                && matches!(e, Event::Mouse(WheelScrolled { .. }))
                && scan(w, c0, Some((0.0, 0.0)), None)
                    .0
                    .is_some_and(|x| c.is_over(x.0))
            {
                (ring.5, ring.6) = (usize::MAX - 1, None);
                sh.publish(super::act(|a| a.lyric = false));
            }
            match e {
                Event::Keyboard(ModifiersChanged(m)) if root => ring.4 = !m.is_empty(),
                Event::Mouse(iced::mouse::Event::ButtonPressed(_)) if root => {
                    (ring.1, ring.2) = (0.0, None);
                }
                Event::Mouse(WheelScrolled {
                    delta: ScrollDelta::Lines { x, y },
                }) if root
                    && *x == 0.0
                    && !ring.4
                    && c.position().is_some_and(|p| p.y < l.bounds().height - 88.0) =>
                {
                    if self.3 == 0 {
                        let delta = ScrollDelta::Pixels {
                            x: 0.0,
                            y: y * 112.0,
                        };
                        pass(w, c0, &Event::Mouse(WheelScrolled { delta }), c, sh, false);
                        return;
                    }
                    ring.3 = c.position().expect("the cursor is on the window");
                    if ring.1 * y < 0.0 {
                        (ring.1, ring.2) = (0.0, None);
                    }
                    ring.1 += y * 112.0;
                    if ring.2.is_none() {
                        let step = ring.1 * 0.35;
                        (ring.1, ring.2) = (ring.1 - step, Some(std::time::Instant::now()));
                        let delta = ScrollDelta::Pixels { x: 0.0, y: step };
                        let wheel = Event::Mouse(WheelScrolled { delta });
                        if !pass(w, c0, &wheel, Cursor::Available(ring.3), sh, false) {
                            (ring.1, ring.2) = (0.0, None);
                        }
                    } else {
                        sh.capture_event();
                    }
                    return;
                }
                _ => {}
            }
            if let (Event::Window(RedrawRequested(now)), true) = (e, root && ring.1 != 0.0) {
                match ring.2.map(|t| t + frame).filter(|d| d > now) {
                    Some(due) => sh.request_redraw_at(due),
                    None => {
                        let dt = ring.2.map_or(1.0 / 60.0, |t| (*now - t).as_secs_f32());
                        let step = match ring.1.abs() < 3.0 {
                            true => ring.1,
                            false => ring.1 * (1.0 - (-dt / 0.04).exp()),
                        };
                        (ring.1, ring.2) = (ring.1 - step, Some(*now));
                        let delta = ScrollDelta::Pixels { x: 0.0, y: step };
                        let wheel = Event::Mouse(WheelScrolled { delta });
                        let at = Cursor::Available(ring.3);
                        match pass(w, c0, &wheel, at, sh, true) {
                            false => (ring.1, ring.2) = (0.0, None),
                            true if ring.1 != 0.0 => sh.request_redraw_at(*now + frame),
                            true => ring.2 = None,
                        }
                    }
                }
            }
            let drag = self.4 < usize::MAX - 1
                && matches!(
                    e,
                    Event::Mouse(CursorMoved { .. } | iced::mouse::Event::ButtonPressed(_))
                );
            let pre = drag
                .then(|| scan(w, c0, Some((0.0, 0.0)), None).0.map(|x| x.2))
                .flatten();
            pass(w, c0, e, c, sh, false);
            if pre.is_some_and(|o| {
                scan(w, c0, Some((0.0, 0.0)), None)
                    .0
                    .is_some_and(|x| (x.2 - o).abs() > 0.5)
            }) {
                (ring.5, ring.6) = (usize::MAX - 1, None);
                sh.publish(super::act(|a| a.lyric = false));
            }
            let Some((msg, _, _)) = &self.1 else { return };
            let key = |k| matches!(e, Event::Keyboard(KeyPressed { key, .. }) if *key == k);
            let go = key(Key::Named(Named::Enter)) || key(Key::Named(Named::Space));
            let press = matches!(e, Event::Mouse(iced::mouse::Event::ButtonPressed(_)));
            let hit = press && c.is_over(l.bounds()) || go && ring.0;
            if hit {
                let d = c.position().map_or(iced::Vector::ZERO, |p| {
                    let (x, y) = crate::app::cursor();
                    iced::Vector::new(x - p.x, y - p.y)
                });
                *crate::app::ANCHOR.lock().expect("the anchor lock") = l.bounds() + d;
            }
            match e {
                _ if !ring.0 => return,
                Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => ring.0 = false,
                _ if key(Key::Named(Named::Escape)) => ring.0 = false,
                _ if go && !sh.is_event_captured() => {
                    msg.iter().for_each(|m| sh.publish(m.clone()));
                }
                _ => return,
            }
            if matches!(e, Event::Keyboard(_)) {
                sh.capture_event();
            }
            sh.request_redraw();
        }
        fn overlay<'b>(
            &'b mut self,
            t: &'b mut Tree,
            l: Layout<'b>,
            r: &R,
            v: &Rect,
            tr: iced::Vector,
        ) -> Option<iced::advanced::overlay::Element<'b, Message, T, R>> {
            self.0
                .as_widget_mut()
                .overlay(&mut t.children[0], l, r, v, tr)
        }
        fn mouse_interaction(
            &self,
            t: &Tree,
            l: Layout,
            c: Cursor,
            v: &Rect,
            r: &R,
        ) -> Interaction {
            if self.3 == 2 && t.state.downcast_ref::<Ring>().4 {
                return Interaction::None;
            }
            let w = self.0.as_widget();
            w.mouse_interaction(&t.children[0], l, c, v, r)
        }
    }
}

pub fn art<'a>(px: f32, glyph: Txt<'a>, bg: impl Into<Background>) -> El<'a> {
    let r = if px < 100.0 { 4.0 } else { 6.0 };
    let g = container(glyph.size(px * 0.38).center()).center(px);
    g.style(fill(bg.into(), r)).into()
}

pub fn cover(app: &App, id: &str, px: f32, round: bool) -> El<'static> {
    let r = [[6.0, 4.0][usize::from(px < 100.0)], px / 2.0][usize::from(round)];
    let Some(h) = app.covers.get(id, app.bucket(px)) else {
        let g = i([i::MUSIC, i::USER][usize::from(round)], px * 0.38).color(SUB);
        let g = container(g.center()).center(px);
        return g.style(fill(ELEVATED, r)).into();
    };
    let img = image(h).width(px).height(px).border_radius(r);
    img.content_fit(iced::ContentFit::Cover).into()
}

pub fn liked_art<'a>(px: f32) -> El<'a> {
    let g = iced::gradient::Linear::new(2.4).add_stop(0.0, color!(0x450af5));
    let heart = theme::filled(theme::fill::HEART, 0.0).color(TEXT);
    art(px, heart, g.add_stop(1.0, color!(0xc4efd9)))
}

pub fn media<'a>(
    art: El<'a>,
    title: &'a str,
    sub: impl Into<Cow<'a, str>>,
    w: f32,
    fg: Color,
) -> Row<'a, Message> {
    let (title, sub) = (theme::ell(title, w, 16.0, Normal).color(fg), sub.into());
    let sub = (!sub.is_empty()).then(|| theme::ell(sub, w, 14.0, Normal).color(SUB));
    let lines = column![title].push(sub);
    let r = row![art, lines.spacing(2).width(Fill)];
    r.spacing(12).align_y(Center)
}

pub fn artists_line<'a>(artists: &'a [Artist], size: f32, w: f32) -> El<'a> {
    let mut left = w;
    let mut r = Row::new();
    for (n, a) in artists.iter().enumerate() {
        if left < size {
            break;
        }
        let name: Cow<'a, str> = match n {
            0 => (&a.name).into(),
            _ => format!(", {}", a.name).into(),
        };
        let used = (n + 1 < artists.len()).then(|| theme::width(&name, size, Normal));
        let t = theme::ell(name, left, size, Normal).color(SUB);
        left -= used.unwrap_or(0.0);
        r = r.push(match a.id.is_empty() {
            true => El::from(t),
            false => theme::link_btn(t, SUB, go("artist", &a.id)).into(),
        });
    }
    r.into()
}

pub fn fmt_dur(s: f32) -> String {
    let s = s as u64;
    match s / 3600 {
        0 => format!("{}:{:02}", s / 60, s % 60),
        h => format!("{h}:{:02}:{:02}", s / 60 % 60, s % 60),
    }
}

pub fn fmt_n(n: usize) -> String {
    let s = n.to_string();
    let comma = |i: usize| (i > 0 && (s.len() - i).is_multiple_of(3)).then_some(',');
    let digits = s.chars().enumerate();
    let digits = digits.flat_map(|(i, c)| comma(i).into_iter().chain([c]));
    digits.collect()
}

pub fn fmt_long(s: f32) -> String {
    match s as u64 {
        s @ 3600.. => format!("{} hr {} min", s / 3600, s % 3600 / 60),
        s @ 60.. => format!("{} min {} sec", s / 60, s % 60),
        s => format!("{s} sec"),
    }
}

fn caps<'a>(combo: &str, size: f32, color: Color) -> Row<'a, Message> {
    let edge = Border::default()
        .color(color!(0x7c7c7c))
        .width(1)
        .rounded(4);
    let cap = |k: &str| -> El<'a> {
        let k = container(txt(k.to_string(), size).color(color)).padding([0, 6]);
        k.center_y(size + 12.0)
            .style(move |_| container::Style::default().border(edge))
            .into()
    };
    Row::with_children(combo.split('+').map(cap)).spacing(4)
}

pub fn tip<'a>(
    c: impl Into<El<'a>>,
    label: impl text::IntoFragment<'a>,
    at: tooltip::Position,
) -> El<'a> {
    let label = txt(label, 14.0).line_height(text::LineHeight::Absolute(20.0.into()));
    let body = container(label)
        .padding([4, 8])
        .style(fill(theme::MENU, 4.0));
    let delay = std::time::Duration::from_millis(200);
    tooltip(c, body, at).gap(8).padding(0).delay(delay).into()
}

fn top_bar(app: &App) -> El<'_> {
    let nav = |c: char, on: bool, label: &'static str, m: Message| {
        let b = theme::sq(i(c, 22.0).center(), 32.0);
        let b = b.style(theme::link(if on { SUB } else { theme::DIM }));
        tip(b.on_press_maybe(on.then_some(m)), label, Bottom)
    };
    let round = |c: Txt<'static>, label: &'static str, m: Message| {
        let b = button(container(c.center()).center(48)).padding(0);
        tip(b.style(theme::round(ELEVATED)).on_press(m), label, Bottom)
    };
    let user = app.session.as_ref().map_or("", |s| s.server.user.as_str());
    let avatar = button(pages::avatar(user, 32.0)).padding(8);
    let avatar = avatar.style(theme::round(ELEVATED));
    let home = match app.route == "home" {
        true => theme::filled(theme::fill::HOME, 24.0).color(TEXT),
        false => i(i::HOME, 24.0).color(SUB),
    };
    let glass = text_input::Icon {
        font: theme::LUCIDE,
        code_point: i::SEARCH,
        size: Some(22.0.into()),
        spacing: 12.0,
        side: text_input::Side::Left,
    };
    let style = theme::input([ELEVATED, CHIP], [BG, TEXT], [0.0, 2.0], 500.0);
    let hint = "What do you want to play?";
    let search = text_input(hint, &app.search_q);
    let search = search.id("search").on_input(Message::Search);
    let search = search.on_submit(Message::SearchGo(app.search_seq));
    let search = search.style(style).icon(glass).width(474).size(16);
    let search = search.font(theme::fw(Normal));
    let search = search.padding(iced::Padding::new(13.0).left(16.0).right(100.0));
    let search = search.line_height(text::LineHeight::Absolute(22.0.into()));
    let on = app.route == "search" && app.search_q.is_empty();
    let browse = match app.route.starts_with("search") || !app.search_q.is_empty() {
        true => Message::Noop,
        false => Message::Nav("search".into()),
    };
    let clear = Message::Search(String::new());
    let x = (!app.search_q.is_empty())
        .then(|| tip(icon_btn(i::X, 20.0, clear.clone()), "Clear", Bottom));
    let bar = container(Space::new().width(1).height(25)).style(fill(color!(0x7c7c7c), 0.0));
    let all = match on {
        true => theme::filled(theme::fill::BROWSE, 24.0),
        false => i(i::BROWSE, 24.0),
    };
    let all = button(all).padding(0);
    let all = all.style(theme::link([SUB, TEXT][usize::from(on)]));
    let all = all.on_press(if app.route.starts_with("search") {
        clear
    } else {
        Message::Nav("search".into())
    });
    let all = tip(all, "Browse", Bottom);
    let tail = row![x, bar, all].spacing(12).align_y(Center);
    let tail = container(tail)
        .padding(padding::right(16))
        .align_right(474)
        .center_y(48);
    let search = stack![mouse_area(search).on_release(browse)]
        .push(tail)
        .clip(true);
    let keys = (app.search_q.is_empty() && !app.binding("search").is_empty())
        .then(|| caps(&crate::app::key_label(app.binding("search")), 14.0, SUB));
    let x = 66.0 + theme::width(hint, 16.0, Normal);
    let search = hover(
        search,
        container(keys).padding(padding::left(x)).center_y(48),
    );
    let fwd = app.hi + 1 < app.history.len();
    let settings = Message::Nav("settings".into());
    let menu = Message::Menu(Box::new(MenuKind::User));
    let right = row![
        tip(icon_btn(i::SETTINGS, 18.0, settings), "Settings", Bottom),
        tip(avatar.on_press(menu), user, Bottom),
    ];
    let back = nav(i::LEFT, app.hi > 0, "Go back", Message::Back);
    let right = container(right.spacing(8).align_y(Center)).align_right(Fill);
    let left = row![back, nav(i::RIGHT, fwd, "Go forward", Message::Fwd)];
    let home = round(home, "Home", Message::Nav("home".into()));
    let bar = row![left.spacing(8).width(Fill), home, search, right];
    let bar = bar.spacing(8).align_y(Center);
    bar.padding([0, 8]).height(64).into()
}

type LibRow<'a> = (&'a String, &'a String, &'static str, &'a str, &'a String);

pub fn lib_rows(app: &App) -> (Vec<LibRow<'_>>, std::ops::Range<usize>) {
    let f = app.lib_filter;
    let q = app.lib_find.as_deref().unwrap_or("").to_lowercase();
    let hit = |name: &str| q.is_empty() || name.to_lowercase().contains(&q);
    let wait = app.pending.contains("home#0") || app.covers.wait && app.covers.empty();
    let rows = app
        .data::<Lib>("lib")
        .filter(|_| !wait)
        .into_iter()
        .flat_map(|l| {
            let pls = l
                .playlists
                .iter()
                .filter(|_| f < 2)
                .map(|p| (&p.cover, &p.name, "playlist", p.id.as_str(), &p.owner));
            let als = l
                .albums
                .iter()
                .filter(|_| matches!(f, 0 | 2))
                .map(|a| (&a.cover, &a.name, "album", a.id.as_str(), &a.artist));
            let ars = l
                .artists
                .iter()
                .filter(|_| matches!(f, 0 | 3))
                .map(|a| (&a.cover, &a.name, "artist", a.id.as_str(), &a.name));
            pls.chain(als).chain(ars)
        });
    let mut rows: Vec<_> = rows.filter(|r| hit(r.1)).collect();
    match app.cfg.settings.lib_sort {
        1 => rows.sort_by_cached_key(|r| r.1.to_lowercase()),
        2 => rows.sort_by_cached_key(|r| (r.4.to_lowercase(), r.1.to_lowercase())),
        _ => {}
    }
    let pins = &app.cfg.settings.pins;
    if !pins.is_empty() {
        let pos = |r: &LibRow| {
            pins.iter()
                .position(|p| p.split_once(':') == Some((r.2, r.3)))
        };
        rows.sort_by_key(|r| pos(r).unwrap_or(usize::MAX));
    }
    let k = PINS
        .iter()
        .filter(|p| p.0 >> f & 1 == 1 && hit(p.3))
        .count();
    let ((off, view), (_, cols, _, h)) = (app.lib_scroll, app.lib_layout());
    let n = rows.len();
    let vis = (view.max(600.0) / h) as usize;
    let r0 = ((off / h) as usize).min((n + k).div_ceil(cols).saturating_sub(vis));
    let lo = r0.saturating_sub(4) * cols;
    let hi = (r0 + vis + 8) * cols;
    (
        rows,
        lo.saturating_sub(k).min(n)..hi.saturating_sub(k).min(n),
    )
}

const PINS: [(u8, char, bool, &str, &str); 6] = [
    (3, i::MUSIC, false, "Liked Songs", "liked"),
    (1, i::MUSIC, true, "Songs", "songs"),
    (4, i::ALBUM, false, "All albums", "albums"),
    (8, i::USER, false, "All artists", "artists"),
    (1, i::FOLDER, false, "Folders", "folders"),
    (1, i::RADIO, false, "Radio stations", "radio"),
];

fn lib_panel<'a>(app: &'a App) -> El<'a> {
    let (w, f, mini) = (app.lib_w(), app.lib_filter, app.lib_mini());
    let q = app.lib_find.as_deref().unwrap_or("").to_lowercase();
    let hit = |name: &str| q.is_empty() || name.to_lowercase().contains(&q);
    let (sort, (view, cols, lw, rh)) = (app.cfg.settings.lib_sort, app.lib_layout());
    let px = [48.0, 48.0, lw - 8.0, lw - 24.0][view as usize];
    let item = |art: El<'a>, title: &'a str, sub: Cow<'a, str>, route: String| -> El<'a> {
        let same =
            |r: &str| r == route || r.strip_prefix(&route).is_some_and(|s| s.starts_with(':'));
        let on = app.current_song().is_some() && !app.ctx.is_empty() && same(&app.ctx);
        let fg = if on { ACCENT } else { TEXT };
        let tag = format!("lib:{route}");
        let top: El = match route.as_str() {
            "folders" | "radio" => Space::new().into(),
            _ if app.hover.as_deref() != Some(tag.as_str()) => Space::new().into(),
            _ => {
                let live = on && app.playing;
                let g = [theme::fill::PLAY, theme::fill::PAUSE][usize::from(live)];
                let m = match live {
                    true => Message::Toggle,
                    false => Message::PlayCtx(route.clone(), 0),
                };
                match view > 1 {
                    true => {
                        let b = theme::sq(theme::filled(g, 16.0).color(BG).center(), 40.0);
                        let b = b.style(theme::accent).on_press(m);
                        let b = container(b).align_right(px).align_bottom(px);
                        b.padding(8).into()
                    }
                    false => shade(g, [4.0, 24.0][usize::from(route.starts_with("artist:"))], m),
                }
            }
        };
        let art: El = stack![art, top].into();
        let live = (on && app.playing).then(|| i(i::VOL2, 16.0).color(ACCENT));
        let pinned = route == "liked" || app.cfg.settings.pins.contains(&route);
        let line = |w: f32, sub: Cow<'a, str>| {
            let pin = pinned.then(|| theme::filled(theme::fill::PIN, 12.0).color(ACCENT));
            let sub = theme::ell(sub, w - 16.0 * f32::from(u8::from(pinned)), 14.0, Normal);
            row![pin, sub.color(SUB)].spacing(4).align_y(Center)
        };
        let body = match (mini, view) {
            (true, _) | (_, 2) => art,
            (_, 0) => {
                let kind = sub.split(pages::DOT).next().unwrap_or_default().to_string();
                let name = theme::ell(title, w - 160.0, 14.0, Normal).color(fg);
                let kind = txt(format!("{}{kind}", pages::DOT), 14.0)
                    .color(SUB)
                    .width(Fill);
                row![name, kind].push(live).align_y(Center).into()
            }
            (_, 3) => {
                let name = theme::ell(title, px, 16.0, Normal).color(fg);
                column![art, column![name, line(px, sub)].spacing(2)]
                    .spacing(8)
                    .into()
            }
            _ => {
                let name = theme::ell(title, w - 112.0, 16.0, Normal).color(fg);
                let lines = column![name, line(w - 112.0, sub)].spacing(2).width(Fill);
                row![art, lines]
                    .spacing(12)
                    .align_y(Center)
                    .push(live)
                    .into()
            }
        };
        let b = button(body)
            .padding([8, 8, 4, 12][view as usize])
            .width(Fill);
        let [a, h] = match (same(&app.route), app.menu_on(&route)) {
            (true, _) => [0.1, 0.22],
            (_, true) => [0.1, 0.1],
            _ => [0.0, 0.05],
        };
        let t = theme::tint;
        let b = b.style(theme::btn([t(a), t(h), t(0.3)], [TEXT; 3], 4.0));
        let menu = Message::LibMenu(route.clone());
        let m = mouse_area(b.on_press(Message::Nav(route))).on_right_press(menu);
        hov(m, tag, Some("lib")).into()
    };
    let head: El = if mini {
        let b = icon_btn(i::LIBRARY, 20.0, task(App::lib_collapse));
        let b = tip(b, "Expand Your Library", Right);
        container(b).center_x(Fill).into()
    } else {
        let side = app
            .hover
            .as_ref()
            .filter(|h| h.starts_with("lib"))
            .map(|_| i(i::SIDE, 16.0));
        let title = row![side, bold("Your Library", 16.0)].spacing(12);
        let title = theme::link_btn(title.align_y(Center), TEXT, task(App::lib_collapse));
        let open = matches!(app.menu.as_ref().map(|m| &m.kind), Some(MenuKind::Create));
        let plus = row![
            i([i::PLUS, i::X][usize::from(open)], 16.0),
            bold("Create", 14.0)
        ];
        let plus = container(plus.spacing(8).align_y(Center)).center_y(36);
        let plus = button(plus).padding([0, 16]);
        let plus = plus
            .style(theme::btn([ELEVATED, CHIP, CHIP], [TEXT; 3], 500.0))
            .on_press(Message::Menu(Box::new(MenuKind::Create)));
        let plus = tip(plus, "Create a playlist", Bottom);
        let title = tip(title, "Collapse Your Library", Bottom);
        let r = row![title, Space::new().width(Fill), plus];
        r.align_y(Center).into()
    };
    let chips = [(1u8, "Playlists"), (2, "Albums"), (3, "Artists")].map(|(n, name)| {
        let m = task(move |a| {
            a.lib_filter = if a.lib_filter == n { 0 } else { n };
            a.lib_scroll.0 = 0.0;
            a.covers_now()
        });
        theme::chip_btn(name, f == n, m)
    });
    let chips = Row::with_children(chips).spacing(8);
    let user = app.session.as_ref().map_or("", |s| s.server.user.as_str());
    let pins = PINS.iter().filter(|p| p.0 >> f & 1 == 1 && hit(p.3));
    let pins = pins.map(|&(_, c, green, t, r)| {
        let (bg, fg) = [(ELEVATED, SUB), (SONGS_BG, ACCENT)][usize::from(green)];
        let (liked, route) = (r == "liked", r.into());
        let pic = liked.then(|| liked_art(px));
        let pic = pic.unwrap_or_else(|| art(px, i(c, 0.0).color(fg), bg));
        item(
            pic,
            t,
            match liked {
                true => format!("Playlist{}{user}", pages::DOT).into(),
                false => "Library".into(),
            },
            route,
        )
    });
    let (rows, span) = lib_rows(app);
    let pins: Vec<El> = pins.collect();
    let (k, n) = (pins.len(), rows.len());
    let (lo, hi) = (
        [span.start + k, 0][usize::from(span.start == 0)],
        span.end + k,
    );
    let gap = |r: usize| Space::new().height(r as f32 * rh);
    let (top, bottom) = (
        gap(lo / cols),
        gap((n + k).div_ceil(cols) - hi.div_ceil(cols)),
    );
    let rows = rows.into_iter().take(span.end).skip(span.start);
    let rows = rows.map(|(c, name, k, id, by)| {
        let s: Cow<str> = match k {
            "artist" => "Artist".into(),
            "album" => format!("Album \u{2022} {by}").into(),
            _ => format!("Playlist \u{2022} {by}").into(),
        };
        item(
            cover(app, c, px, k == "artist"),
            name,
            s,
            format!("{k}:{id}"),
        )
    });
    let mut tiles = pins.into_iter().filter(|_| lo == 0).chain(rows).peekable();
    let mut list = column![top];
    while tiles.peek().is_some() {
        let r = tiles
            .by_ref()
            .take(cols)
            .map(|t| container(t).width(Fill).into());
        let r: Vec<El> = r.collect();
        let pad = (r.len()..cols).map(|_| Space::new().width(Fill).into());
        list = list.push(Row::with_children(r).extend(pad));
    }
    let list = vgap(list.push(bottom), lo / cols);
    let chips = (!mini).then(|| container(chips).padding([0, 8]));
    let open = app.lib_find.is_none();
    let find = task(move |a| {
        a.lib_find = open.then(String::new);
        a.lib_scroll.0 = 0.0;
        let focus = open.then(|| iced::widget::operation::focus("libfind"));
        iced::Task::batch([a.covers_now()].into_iter().chain(focus))
    });
    let x = theme::sq(i(i::SEARCH, 20.0).center(), 32.0).style(theme::link(SUB));
    let x = x.on_press(find.clone());
    let field: El = match &app.lib_find {
        None => tip(x, "Search in Your Library", Bottom),
        Some(v) => {
            let style = theme::input([CHIP, CHIP], [CHIP, CHIP], [0.0, 0.0], 4.0);
            let t = text_input("Search in Your Library", v)
                .id("libfind")
                .size(14);
            let t = t.on_input(|v| {
                task(move |a| {
                    a.lib_find = Some(v.clone());
                    a.lib_scroll.0 = 0.0;
                    a.covers_now()
                })
            });
            let t = t
                .on_submit(Message::Noop)
                .style(style)
                .font(theme::fw(Normal));
            let t = t.line_height(text::LineHeight::Absolute(18.0.into()));
            let t = t
                .padding(iced::Padding::new(7.0).left(32).right(28))
                .width(Fill);
            let close = container(icon_btn(i::X, 14.0, find))
                .align_right(Fill)
                .center_y(Fill);
            container(stack![t, x, close]).max_width(188).into()
        }
    };
    let icon = pages::LIB_VIEWS[view as usize];
    let order = row![txt(pages::LIB_SORTS[sort as usize], 14.0), i(icon, 16.0)].spacing(8);
    let menu = Message::Menu(Box::new(MenuKind::LibSort));
    let open = matches!(app.menu.as_ref().map(|m| &m.kind), Some(MenuKind::LibSort));
    let order = theme::link_btn(order.align_y(Center), [SUB, TEXT][usize::from(open)], menu);
    let order = order.padding([4, 8]);
    let order = tip(order, "Change the sort order", Bottom);
    let gap = app.lib_find.is_none().then(|| Space::new().width(Fill));
    let tools = row![field].push(gap).push(order);
    let tools = tools.spacing(8).align_y(Center).width(Fill);
    let tools = (!mini).then(|| container(tools).padding([0, 8]));
    let (old_y, old_h) = app.lib_scroll;
    let list = scrollable(pane(list)).style(theme::scroll).height(Fill);
    let list = list.on_scroll(move |v| {
        let (y, h) = (v.absolute_offset().y, v.bounds().height);
        match (y / rh) as usize != (old_y / rh) as usize || (h - old_h).abs() > 0.5 {
            true => Message::LibScrolled(y, h),
            false => Message::Noop,
        }
    });
    let head = container(head).padding(iced::Padding::new(8.0).bottom(4));
    let body = column![head, chips, column![tools, list].spacing(8)];
    let body = container(body.spacing(12).padding(8)).width(w);
    let on = act(|a| {
        let h = a.hover.take().filter(|h| h.starts_with("lib"));
        a.hover = h.or(Some("lib".into()));
    });
    let off = act(|a| a.hover = a.hover.take().filter(|h| !h.starts_with("lib")));
    mouse_area(body.height(Fill))
        .on_enter(on)
        .on_exit(off)
        .into()
}

fn player_bar(app: &App) -> El<'_> {
    let song = app.current_song();
    let side = |t: El<'static>, on: bool, label: &'static str, m: Message| {
        let dot = container(Space::new().width(4).height(4)).style(fill(ACCENT, 2.0));
        let dot = on.then(|| container(dot).center_x(Fill).align_bottom(Fill));
        let b = theme::sq(stack![container(t).center(Fill)].push(dot), 32.0);
        let b = b.style(theme::link(if on { ACCENT } else { SUB }));
        tip(b.on_press(m), label, Top)
    };
    let room = (app.view_size().0 - 16.0) * 0.3 - 8.0;
    let tw = (room - 112.0).min(200.0);
    let left = song.filter(|_| !app.full).map(|s| {
        let on = app.is_starred(&s.id, s.starred);
        let title = theme::ell(&s.title, tw, 14.0, Normal);
        let radio = !s.url.is_empty();
        let title = btn(title, album_of(s));
        let label = ["Add to Liked Songs", "Remove from Liked Songs"][usize::from(on)];
        let like = Message::Star(vec![s.id.clone()], !on);
        let art = btn(cover(app, &s.cover, 56.0, false), Message::Panel(1));
        let lines = column![title, artists_line(&s.artists, 12.0, tw)].spacing(2);
        let heart = (!radio).then(|| side(theme::saved(on, 16.0).into(), false, label, like));
        row![art, lines, heart].spacing(12).align_y(Center)
    });
    let dur = song.map_or(0.0, |s| s.duration).max(1.0);
    let rest = app.cfg.settings.remaining;
    let bar = move |pos: f32| -> El<'_> {
        let pos = pos.min(dur);
        let total = if rest {
            format!("-{}", fmt_dur(dur - pos))
        } else {
            fmt_dur(dur)
        };
        let total = button(txt(total, 12.0)).width(48).padding(0);
        let total = total.style(theme::link(SUB));
        let total = total.on_press(set(|s, b| s.remaining = b, !rest));
        let seek = slider(0.0..=dur, pos, |v| {
            task(
                move |a| match crate::app::DOWN.load(std::sync::atomic::Ordering::Relaxed) {
                    true => {
                        a.seek_preview = Some(v);
                        iced::Task::none()
                    }
                    false => a.update(Message::Seek(v)),
                },
            )
        });
        let seek = seek
            .on_release(task(|a| {
                iced::Task::batch(a.seek_preview.take().map(|v| a.update(Message::Seek(v))))
            }))
            .style(theme::seek);
        let at = sub(fmt_dur(pos), 12.0).width(48).align_x(iced::Right);
        row![at, seek, total].spacing(8).align_y(Center).into()
    };
    let run = app.playing && !app.hidden && app.seek_preview.is_none();
    let progress: El = match song.filter(|s| s.url.is_empty()) {
        None => {
            let rail = Space::new().width(Fill).height(4);
            let rail = container(rail).style(fill(theme::TRACK, 2.0));
            let none = || sub("-:--", 12.0).width(48);
            let row = row![none().align_x(iced::Right), rail, none()].spacing(8);
            container(row.align_y(Center)).center_y(16).into()
        }
        Some(_) => live(app.clock(), run, bar),
    };
    let playing = usize::from(app.playing);
    let glyph = theme::filled([theme::fill::PLAY, theme::fill::PAUSE][playing], 16.0);
    let bg = song.map_or(TEXT.scale_alpha(0.3), |_| TEXT);
    let play = theme::sq(glyph.color(BG).center(), 32.0).style(theme::solid([bg; 3], BG));
    let play = play.on_press(Message::Toggle);
    let play = tip(play, ["Play", "Pause"][playing], Top);
    let (ic, fi) = (|c| i(c, 16.0), |c| theme::filled(c, 19.0));
    let dim = song.is_none().then_some(SUB.scale_alpha(0.3));
    let (dic, dfi) = (|c| ic(c).color_maybe(dim), |c| fi(c).color_maybe(dim));
    let (on, rep) = (app.shuffle.is_some(), usize::from(app.repeat));
    let shuffle = ["Enable shuffle", "Disable shuffle"][usize::from(on)];
    let repeat = ["Enable repeat", "Enable repeat one", "Disable repeat"][rep];
    let repeat_icon = dic([i::REPEAT, i::REPEAT, i::REPEAT1][rep]);
    let controls = row![
        side(dic(i::SHUFFLE).into(), on, shuffle, Message::Shuffle),
        side(
            dfi(theme::fill::PREV).into(),
            false,
            "Previous",
            Message::Prev
        ),
        play,
        side(
            dfi(theme::fill::NEXT).into(),
            false,
            "Next",
            Message::Next(true)
        ),
        side(repeat_icon.into(), rep > 0, repeat, act(App::repeat)),
    ];
    let center = column![controls.spacing(8).align_y(Center), progress];
    let center = center.spacing(8).align_x(Center).max_width(722);
    let vol = if app.muted { 0.0 } else { app.volume };
    let level = usize::from(vol > 0.0) + usize::from(vol >= 0.5);
    let vol_icon = ic([i::MUTE, i::VOL1, i::VOL2][level]);
    let lyrics = app.route == "lyrics";
    let fixed = [216.0, 108.0][usize::from(app.full)];
    let device = room - fixed >= 56.0;
    let vw = (room - fixed + [36.0, 0.0][usize::from(device)]).min(93.0);
    let volume = slider(0.0..=1.0, vol, Message::Vol).step(0.01f32).width(vw);
    let volume = mouse_area(volume.style(theme::seek)).on_scroll(|d| match d {
        iced::mouse::ScrollDelta::Lines { y, .. } => Message::Step(true, y.clamp(-1.0, 1.0)),
        iced::mouse::ScrollDelta::Pixels { y, .. } => {
            Message::Step(true, y.clamp(-60.0, 60.0) / 60.0)
        }
    });
    let words = match (lyrics, app.hi > 0) {
        (false, _) => Message::Nav("lyrics".into()),
        (true, true) => Message::Back,
        (true, false) => Message::Nav("home".into()),
    };
    let mute = if app.muted { "Unmute" } else { "Mute" };
    let np = Message::Panel(1);
    let hot = app.hover.as_deref() == Some("queue");
    let panels = row![
        song.map(|_| side(ic(i::PR).into(), app.panel == 1, "Now playing view", np)),
        song.map(|_| side(ic(i::LYRICS).into(), lyrics, "Lyrics", words)),
        hov(
            mouse_area(side(
                queue_icon(
                    14.5,
                    [[SUB, TEXT][usize::from(hot)], ACCENT][usize::from(app.panel == 2)]
                ),
                app.panel == 2,
                "Queue",
                Message::Panel(2)
            )),
            "queue".into(),
            None
        ),
    ];
    let right = row![
        (!app.full).then_some(panels),
        device.then(|| side(
            ic(i::DEVICE).into(),
            app.panel == 3,
            "Connect to a device",
            Message::Panel(3)
        )),
        side(vol_icon.into(), false, mute, act(|a| a.muted = !a.muted)),
        volume,
        Space::new().width(8),
        match (app.full, song) {
            (false, None) => container(dic(i::FULL)).center(32).into(),
            (full, _) => side(
                ic([i::FULL, i::WINDOW][usize::from(full)]).into(),
                false,
                ["Full screen", "Exit full screen"][usize::from(full)],
                task(move |a| a.set_full(!full)),
            ),
        },
    ];
    let right = container(right.align_y(Center)).align_right(FillPortion(3));
    let left = container(left)
        .padding(padding::left(8))
        .width(FillPortion(3));
    let bar = row![left, container(center).center_x(FillPortion(4)), right];
    bar.align_y(Center).padding([0, 8]).height(72).into()
}

fn modal<'a>(app: &'a App, id: &str, kind: u8) -> El<'a> {
    let pl = app.playlist(id);
    let act = |label: &'static str, bg: [Color; 3], fg: Color, x: u16, m: Option<Message>| {
        let b = button(bold(label, 16.0)).style(theme::solid(bg, fg));
        let b = b.padding([14, x]).on_press_maybe(m.clone());
        focusable(b, m, 500.0, Some("layer"))
    };
    let shut = |b: Btn<'a, Message>| {
        let m = Some(Message::MenuClose);
        focusable(b.on_press(Message::MenuClose), m, 16.0, Some("layer"))
    };
    let close = shut(theme::sq(i(i::X, 20.0).center(), 32.0).style(theme::link(SUB)));
    let style = fill(theme::MENU, 8.0);
    if kind == 3 {
        let caps = |id: &str| {
            let c = crate::app::key_label(app.binding(id));
            caps(if c.is_empty() { "Not set" } else { &c }, 16.0, TEXT)
        };
        let line = |k: &crate::app::KeyAction| -> El<'a> {
            let what = txt(k.1, 16.0).width(Fill);
            row![what, caps(k.0)].align_y(Center).height(32).into()
        };
        let mut keys = crate::app::KEYS.iter();
        let groups = crate::app::KEY_GROUPS.iter().map(|&(name, n)| {
            let rows = iced::widget::Column::with_children(keys.by_ref().take(n).map(line));
            column![bold(name, 24.0), rows.spacing(16)]
                .spacing(16)
                .into()
        });
        let list = iced::widget::Column::with_children(groups.collect::<Vec<El>>()).spacing(24);
        let list = scrollable(pane(list.padding(padding::right(16)))).height(Fill);
        let head = row![bold("Keyboard Shortcuts", 32.0)];
        let hint = row![
            txt("Press", 16.0),
            caps("shortcuts"),
            txt("to open or close this dialog.", 16.0)
        ];
        let top = column![
            head.align_y(Center).height(42),
            hint.spacing(6).align_y(Center)
        ]
        .spacing(12);
        let top = container(top).padding(iced::Padding::new(24.0).top(23).bottom(18));
        let list = container(list.style(theme::scroll)).padding(iced::Padding::new(24.0).top(22));
        let x = shut(theme::sq(i(i::X, 32.0).center(), 32.0).style(theme::link(SUB)));
        let x = container(x)
            .align_right(Fill)
            .padding(iced::Padding::ZERO.top(16).right(17));
        let body = stack![column![top, pages::rule(), list], x];
        let h = (app.view_size().1 - 218.0).max(300.0);
        let body = container(body).width(500).height(h);
        return body.style(fill(ELEVATED, 4.0)).into();
    }
    if kind == 6 {
        let Some((a, _)) = app.data::<(Artist, Vec<crate::api::Album>)>(&format!("artist:{id}"))
        else {
            return Space::new().into();
        };
        let bio = app.data::<crate::api::Info>(&format!("info:{id}"));
        let bio = bio.map_or(String::new(), |i| pages::plain(&i.biography));
        let pic = app
            .covers
            .get(&format!("b18:{}", app.artist_cover(id)), 640);
        let pic = pic.map(|h| {
            let img = image(h).width(Fill).height(432);
            let img = img.border_radius(iced::border::Radius::new(8.0).bottom(0.0));
            img.content_fit(iced::ContentFit::Fill)
        });
        let dark = BG.scale_alpha(0.7);
        let x = theme::sq(i(i::X, 16.0).center(), 32.0);
        let x = shut(x.style(theme::solid([dark, BG.scale_alpha(0.8), dark], TEXT)));
        let top = stack![
            container(pic).width(Fill).height(432),
            container(x).align_right(Fill).padding(16)
        ];
        let stats = [
            (a.album_count, "Albums"),
            (a.song_count, "Songs"),
            (a.play_count, "Plays"),
        ];
        let stats = stats
            .into_iter()
            .filter(|s| s.0 > 0)
            .map(|(n, l)| column![bold(fmt_n(n as usize), 32.0), sub(l, 14.0)].into());
        let stats = iced::widget::Column::with_children(stats.collect::<Vec<El>>()).spacing(40);
        let body = row![stats.width(180), txt(bio, 16.0).color(SUB).width(Fill)].padding(40);
        let body = scrollable(pane(column![top, body])).style(theme::scroll);
        let h = app.view_size().1 * 0.7;
        return container(body)
            .width(768)
            .height(h)
            .style(fill(theme::PANEL, 8.0))
            .into();
    }
    if kind == 5 {
        let key = format!("song:{id}");
        let v = app.data::<serde_json::Value>(&key);
        let val = |k: &str| -> Option<String> {
            let x = v?.get(k)?;
            let s = match k {
                "size" => format!("{:.1} MB", x.as_f64()? / 1e6),
                "duration" => fmt_dur(x.as_f64()? as f32),
                "created" | "played" => pages::date(x.as_str()?, false),
                "playCount" => {
                    let local = app.plays.get(id).copied().unwrap_or(0) as u64;
                    fmt_n(local.max(x.as_u64()?) as usize)
                }
                "displayAlbumArtist" if v?.get("displayArtist") == Some(x) => return None,
                _ => x.as_str().map_or_else(|| x.to_string(), String::from),
            };
            let unit = match k {
                "bitRate" => " kbps",
                "samplingRate" => " Hz",
                "bitDepth" => " bits",
                "trackGain" | "albumGain" => " dB",
                _ => "",
            };
            (!s.is_empty() && s != "0").then(|| s + unit)
        };
        let groups = [
            (
                "Artist",
                "Artist displayArtist,Album artist displayAlbumArtist",
            ),
            ("Composition & Lyrics", "Composer displayComposer"),
            (
                "Your listening",
                "Plays playCount,Last played played,Added created",
            ),
            (
                "Song",
                "Album album,Genre genre,Year year,Disc discNumber,Track track,\
                Duration duration,BPM bpm,Comment comment",
            ),
            (
                "File",
                "Format suffix,Bit rate bitRate,Sample rate samplingRate,\
                Bit depth bitDepth,Channels channelCount,Size size,\
                Track gain trackGain,Album gain albumGain,Path path",
            ),
        ];
        let groups = groups.into_iter().filter_map(|(name, fields)| {
            let rows: Vec<El> = fields
                .split(',')
                .filter_map(|f| {
                    let (label, k) = f.rsplit_once(' ').expect("label and key");
                    let t = text(val(k)?).size(16).font(theme::fw(Normal));
                    let t = t.wrapping(text::Wrapping::WordOrGlyph);
                    Some(column![t, sub(label, 14.0)].spacing(2).into())
                })
                .collect();
            let rows = (!rows.is_empty()).then(|| iced::widget::Column::with_children(rows));
            rows.map(|r| column![bold(name, 20.0), r.spacing(16)].spacing(16).into())
        });
        let note = match app.data::<String>(&key) {
            Some(e) => Some(sub(e.clone(), 14.0)),
            None => v.is_none().then(|| sub("Loading the song details.", 14.0)),
        };
        let list = iced::widget::Column::with_children(groups.collect::<Vec<El>>())
            .push(note)
            .spacing(32);
        let list = scrollable(pane(list.padding([24, 24]).width(Fill)))
            .height(Fill)
            .style(theme::scroll);
        let title = v
            .and_then(|v| v["title"].as_str())
            .unwrap_or_default()
            .to_string();
        let head = row![bold("Song details", 24.0).width(Fill), close].align_y(Center);
        let head = column![
            head,
            theme::ell(title, 392.0, 16.0, iced::font::Weight::Bold)
        ];
        let body = column![container(head).padding(24), pages::rule(), list];
        let body = container(body).width(440).height(542);
        return body.style(fill(theme::PANEL, 8.0)).into();
    }
    if kind == 2 || kind == 4 {
        let name = pl.map_or("", |p| p.name.as_str()).to_string();
        let b = theme::fw(iced::font::Weight::Bold);
        let red = [color!(0xe91429), color!(0xd81326), color!(0xa60e1d)];
        let green = [ACCENT, color!(0x3be477), color!(0x1abc54)];
        let n = app.queue.len();
        let (title, label, ok, bg, fg, spans) = match kind {
            2 => (
                "Delete from Your Library?",
                "Delete",
                Message::PlDel(id.into()),
                red,
                TEXT,
                vec![
                    text::Span::new("This will delete "),
                    text::Span::new(name).font(b),
                    text::Span::new(" from "),
                    text::Span::new("Your Library.").font(b),
                ],
            ),
            _ => (
                "Replace the queue?",
                "Replace",
                Message::Confirm,
                green,
                BG,
                vec![text::Span::new(format!(
                    "This removes the {n} songs that are in the queue now."
                ))],
            ),
        };
        let msg = text::Rich::<(), Message>::with_spans(spans).size(14);
        let msg = msg.font(theme::fw(Normal)).color(BG);
        let close = Some(Message::MenuClose);
        let cancel = act("Cancel", [theme::CLEAR; 3], BG, 0, close);
        let ok = act(label, bg, fg, 32, Some(ok));
        let buttons = row![Space::new().width(Fill), cancel, ok].spacing(16);
        let buttons = buttons.align_y(Center).padding(padding::top(24));
        let title = bold(title, 24.0).color(BG);
        let body = column![title, msg, buttons].spacing(8);
        let body = body.width(420).padding(32);
        return container(body).style(fill(TEXT, 8.0)).into();
    }
    let named = !app.new_pl.trim().is_empty();
    let (name, desc) = (app.new_pl.clone(), app.new_pl_desc.text());
    let public = app.new_pl_public;
    let ok = named.then(|| Message::Edit(id.into(), name, desc, public));
    let input = |hint: &'static str, v: &str, n: u8| {
        let edge = [color!(0x535353); 2];
        let style = theme::input([theme::MENU_HOVER; 2], edge, [0.0, 1.0], 4.0);
        let t = text_input(hint, v).id("layer");
        let t = t.on_input(move |v| Message::LField(n, v));
        let t = t.on_submit_maybe(ok.clone()).style(style);
        t.font(theme::fw(Normal)).padding(11).size(14)
    };
    let error = (!named).then(|| txt("Enter a name.", 12.0).color(theme::ERR));
    let name = column![input("Add a name", &app.new_pl, 3)];
    let name = name.push(error).spacing(6);
    let desc = text_editor(&app.new_pl_desc).id("layer");
    let desc = desc.placeholder("Add an optional description");
    let desc = desc
        .on_action(Message::Desc)
        .height(124)
        .padding(8)
        .size(14);
    let desc = desc.font(theme::fw(Normal)).style(|t, s| {
        let edge = color!(0x535353);
        let w = f32::from(u8::from(matches!(s, text_editor::Status::Focused { .. })));
        let mut st = text_editor::default(t, s);
        (st.background, st.placeholder, st.value) = (theme::MENU_HOVER.into(), SUB, TEXT);
        (st.border, st.selection) = (
            Border::default().color(edge).width(w).rounded(4.0),
            theme::tint(0.3),
        );
        st
    });
    let art = cover(app, pl.map_or("", |p| p.cover.as_str()), 180.0, false);
    let white = [TEXT, color!(0xf0f0f0), color!(0xf0f0f0)];
    let (c, label) = [(i::GLOBE, "Make public"), (i::LOCK, "Make private")][usize::from(public)];
    let flip = crate::app::act(|a| a.new_pl_public ^= true);
    let flip = theme::pill_btn(row![i(c, 16.0), bold(label, 14.0)].spacing(8), flip);
    let flip = flip.padding(iced::Padding::new(6.0).left(11).right(15));
    let save = row![
        flip,
        Space::new().width(Fill),
        act("Save", white, BG, 32, ok)
    ];
    let save = save.align_y(Center);
    let head = row![bold("Edit details", 24.0).width(Fill), close].align_y(Center);
    let fields = row![art, column![name, desc].spacing(16)].spacing(16);
    let fields = fields.padding(padding::right(9));
    let body = column![
        column![head, fields].spacing(24),
        save.padding(padding::right(9))
    ];
    let body = body
        .spacing(8)
        .width(524)
        .padding(iced::Padding::new(24.0).top(22).right(15));
    container(body).style(style).into()
}

pub fn view(app: &App) -> El<'_> {
    if app.session.is_none() {
        return pages::login(app);
    }
    let scrim = |c: Color| {
        let s = container(Space::new()).width(Fill).height(Fill);
        let s = mouse_area(s.style(fill(c, 0.0))).on_press(Message::MenuClose);
        let s = s.on_right_press(Message::MenuClose);
        s.interaction(iced::mouse::Interaction::Idle)
    };
    let shield = |e| {
        mouse_area(e)
            .on_press(Message::Noop)
            .on_right_press(Message::Noop)
            .interaction(iced::mouse::Interaction::Idle)
    };
    let modal = app.modal.as_ref().map(|(id, kind)| {
        let card = container(shield(modal(app, id, *kind))).center(Fill);
        let dim = if matches!(kind, 2 | 4) { 0.5 } else { 0.7 };
        stack![scrim(BG.scale_alpha(dim)), card]
    });
    let menu = app.menu.as_ref().map(|m| {
        let (ww, wh) = app.view_size();
        let (el, w, h) = pages::menu(app, m, 0);
        let x = if m.x + w + 8.0 > ww {
            m.x + m.dx - w
        } else {
            m.x
        };
        let y = if m.y + h + 8.0 > wh {
            m.y - 2.0 * m.dy - h
        } else {
            m.y
        };
        let (x, y) = (x.min(ww - w - 8.0).max(8.0), y.min(wh - h - 8.0).max(8.0));
        let child = (m.sub > 0).then(|| {
            let (el, sw, sh) = pages::menu(app, m, m.sub);
            let sx = if x + w + sw + 4.0 > ww {
                x - sw + 4.0
            } else {
                x + w - 4.0
            };
            let sy = y + ((m.sub_y - y - 4.0) / 40.0).floor() * 40.0;
            let sy = sy.min(wh - sh - 8.0).max(8.0);
            container(shield(el)).padding(padding::top(sy).left(sx.max(8.0)))
        });
        let at = padding::top(y).left(x);
        stack![scrim(theme::CLEAR), container(shield(el)).padding(at)].push(child)
    });
    let toast = app.toast.as_ref().map(|(_, t)| {
        let t = txt(t, 14.0).font(theme::fw(Medium)).color(BG);
        let t = container(t).padding([12, 16]).style(fill(TEXT, 8.0));
        let t = container(t).center_x(Fill).align_bottom(Fill);
        t.padding(padding::bottom(104))
    });
    if app.full {
        let s = app.current_song();
        let top = s.and_then(|s| app.covers.color(&s.cover));
        let w = app.view_size().0 - 428.0;
        let info = s.map(|s| {
            let lines = column![theme::ell(&s.title, w, 64.0, Black)];
            let lines = lines.push(artists_line(&s.artists, 24.0, w)).spacing(8);
            row![cover(app, &s.cover, 300.0, false), lines]
                .spacing(32)
                .align_y(iced::Bottom)
        });
        let from = s.map(|_| column![sub("Playing from", 12.0), bold(&app.source, 14.0)]);
        let body = column![from.map(|f| f.spacing(4)), Space::new().height(Fill)].push(info);
        let body = column![body.padding(48).height(Fill), player_bar(app)];
        let bg = [(0.0, top.unwrap_or(ELEVATED)), (1.0, BG)];
        let body = back(&bg, 0.0, body);
        let top = [modal, menu]
            .into_iter()
            .flatten()
            .map(El::from)
            .chain(toast.map(El::from));
        return stack![body].extend(top).into();
    }
    let mut scroll = scrollable(pane(pages::route(app))).id(SCROLL);
    if !matches!(app.route.as_str(), "lyrics" | "settings") {
        let ((old_y, old_h), step) = (app.scroll(), app.row_h());
        let (t0, t1) = (pages::grid_y(app) - 120.0, pages::hero_h(app) - 24.0);
        let (t2, t3) = (
            pages::banner_h(app) - 24.0,
            pages::list_y(app) - pages::HEAD - 64.0,
        );
        let key = move |y: f32| ((y / step) as usize, y > t0, y > t1, y > t2, y > t3);
        scroll = scroll.on_scroll(move |v| {
            let (y, b, c) = (
                v.absolute_offset().y,
                v.bounds().height,
                v.content_bounds().height,
            );
            match key(y) != key(old_y) || (b - old_h).abs() > 0.5 {
                true => Message::Scrolled(y, b, c),
                false => Message::Noop,
            }
        });
    }
    let scroll = scroll.style(theme::scroll).height(Fill);
    let main = stack![scroll].extend(pages::sticky(app));
    let main = container(main).width(Fill).height(Fill).clip(true);
    let gap = || container(Space::new().width(8).height(Fill)).style(fill(BG, 0.0));
    let grip = |n| {
        let g = mouse_area(gap());
        let g = g.interaction(iced::mouse::Interaction::ResizingHorizontally);
        g.on_press(Message::Drag(Some(n)))
    };
    let right: El = if app.panel > 0 {
        let p = [
            pages::now_playing,
            pages::queue_panel,
            pages::device_panel,
            pages::add_panel,
        ][app.panel as usize - 1](app);
        row![grip(1), container(p).width(app.right_w()).height(Fill)].into()
    } else {
        let show = tip(
            icon_btn(i::LEFT, 16.0, Message::Panel(1)),
            "Show Now Playing view",
            Left,
        );
        row![gap(), container(show).center_x(40).center_y(Fill)].into()
    };
    let panels = row![gap(), lib_panel(app), grip(0), main].push(right);
    let black = |e| container(e).style(fill(BG, 0.0));
    let bottom = container(player_bar(app)).center_y(88);
    let (top, bottom) = (black(top_bar(app)), black(bottom.into()));
    let base = column![top, panels.push(gap()).height(Fill), bottom];
    let caps = |w: f32| {
        let cap = |k: usize| image(CAPS[k].clone()).width(8).height(8);
        let r = |a, b| row![cap(a), Space::new().width(Fill), cap(b)];
        column![r(0, 1), Space::new().height(Fill), r(2, 3)].width(w)
    };
    let gap = || Space::new().width(8);
    let right = row![
        gap(),
        caps([40.0, app.right_w()][usize::from(app.panel > 0)])
    ];
    let caps = row![gap(), caps(app.lib_w()), gap(), caps(app.main_w())].push(right);
    let pad = |h| Space::new().height(h);
    let caps = column![pad(64), caps.height(Fill), pad(88)];
    let top = [modal, menu]
        .into_iter()
        .flatten()
        .map(El::from)
        .chain(toast.map(El::from));
    stack![base, caps].extend(top).into()
}
