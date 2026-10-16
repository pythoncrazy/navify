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
    Message::Nav(format!("{kind}:{id}"))
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

pub fn focusable<'a>(
    c: impl Into<El<'a>>,
    msg: Option<Message>,
    radius: f32,
    id: Option<&'static str>,
) -> El<'a> {
    let id = id.map(iced::advanced::widget::Id::from);
    Element::new(wrap::Wrap(c.into(), Some((msg, radius, id)), None, false))
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
        false,
    ))
}

pub fn smooth(c: El<'_>, on: bool) -> El<'_> {
    Element::new(wrap::Wrap(c, None, None, on))
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
        pub bool,
    );
    #[derive(Default)]
    struct Ring(bool, f32, Option<std::time::Instant>, Point, bool);
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
            self.0.as_widget().draw(&t.children[0], r, th, s, l, c, v);
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
            tree::State::new(Ring::default())
        }
        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.0)]
        }
        fn diff(&self, t: &mut Tree) {
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
                (Event::Mouse(CursorMoved { position: p }), None, None) => {
                    let bits = (p.x.to_bits() as u64) << 32 | p.y.to_bits() as u64;
                    crate::app::CURSOR.store(bits, Relaxed);
                }
                (Event::Keyboard(ModifiersChanged(m)), None, None) => {
                    crate::app::MODS.store(m.bits(), Relaxed);
                }
                _ => {}
            }
            let ring = t.state.downcast_mut::<Ring>();
            match e {
                Event::Keyboard(ModifiersChanged(m)) => ring.4 = !m.is_empty(),
                Event::Mouse(WheelScrolled {
                    delta: ScrollDelta::Lines { x, y },
                }) if self.3 && *x == 0.0 && !ring.4 && c.position().is_some() => {
                    ring.3 = c.position().expect("the cursor is on the window");
                    if ring.1 * y < 0.0 {
                        ring.1 = 0.0;
                    }
                    ring.1 += y * 60.0;
                    ring.2 = None;
                    sh.capture_event();
                    sh.request_redraw();
                    return;
                }
                _ => {}
            }
            let w = self.0.as_widget_mut();
            w.update(&mut t.children[0], e, l, c, r, cb, sh, v);
            let frame = std::time::Duration::from_millis(16);
            if let (Event::Window(RedrawRequested(now)), true) = (e, ring.1 != 0.0) {
                if let Some(due) = ring.2.map(|t| t + frame).filter(|d| d > now) {
                    return sh.request_redraw_at(due);
                }
                let dt = ring.2.map_or(1.0 / 60.0, |t| (*now - t).as_secs_f32());
                let step = match ring.1.abs() < 1.5 {
                    true => ring.1,
                    false => ring.1 * (1.0 - (-dt / 0.05).exp()),
                };
                (ring.1, ring.2) = (ring.1 - step, Some(*now));
                let free = !sh.is_event_captured();
                let delta = ScrollDelta::Pixels { x: 0.0, y: step };
                let wheel = Event::Mouse(WheelScrolled { delta });
                let at = Cursor::Available(ring.3);
                w.update(&mut t.children[0], &wheel, l, at, r, cb, sh, v);
                match free && !sh.is_event_captured() {
                    true => ring.1 = 0.0,
                    false if ring.1 != 0.0 => sh.request_redraw_at(*now + frame),
                    false => {}
                }
            }
            let Some((msg, _, _)) = &self.1 else { return };
            let key = |k| matches!(e, Event::Keyboard(KeyPressed { key, .. }) if *key == k);
            let go = key(Key::Named(Named::Enter)) || key(Key::Named(Named::Space));
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

pub fn artists_line<'a>(artists: &[Artist], size: f32, w: f32) -> El<'a> {
    let mut left = w;
    let mut r = Row::new();
    for (n, a) in artists.iter().enumerate() {
        if left < size {
            break;
        }
        let name = format!("{}{}", if n > 0 { ", " } else { "" }, a.name);
        let used = theme::width(&name, size, Normal);
        let t = theme::ell(name, left, size, Normal).color(SUB);
        left -= used;
        let link = format!("artist:{}", a.id);
        r = r.push(match a.id.is_empty() {
            true => El::from(t),
            false => theme::link_btn(t, SUB, Message::Nav(link)).into(),
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
        k.center_y(24)
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
    let body = container(txt(label, 14.0)).padding([4, 8]);
    let body = body.style(fill(theme::MENU, 4.0));
    let delay = std::time::Duration::from_millis(200);
    tooltip(c, body, at).gap(8).delay(delay).into()
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
    let on = app.route.starts_with("search");
    let browse = match on {
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
    let all = all.on_press(if on { clear } else { browse.clone() });
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

type LibRow<'a> = (&'a String, &'a String, String, String, &'a String);

pub fn lib_rows(app: &App) -> (Vec<LibRow<'_>>, std::ops::Range<usize>) {
    let f = app.lib_filter;
    let q = app.lib_find.as_deref().unwrap_or("").to_lowercase();
    let hit = |name: &str| name.to_lowercase().contains(&q);
    let rows = app.data::<Lib>("lib").into_iter().flat_map(|l| {
        let pls = l.playlists.iter().filter(|_| f < 2).map(|p| {
            let s = format!("Playlist \u{2022} {}", p.owner);
            (&p.cover, &p.name, s, format!("playlist:{}", p.id), &p.owner)
        });
        let als = l.albums.iter().filter(|_| matches!(f, 0 | 2)).map(|a| {
            let s = format!("Album \u{2022} {}", a.artist);
            (&a.cover, &a.name, s, format!("album:{}", a.id), &a.artist)
        });
        let ars = l.artists.iter().filter(|_| matches!(f, 0 | 3)).map(|a| {
            let r = format!("artist:{}", a.id);
            (&a.cover, &a.name, "Artist".into(), r, &a.name)
        });
        pls.chain(als).chain(ars)
    });
    let mut rows: Vec<_> = rows.filter(|r| hit(r.1)).collect();
    match app.cfg.settings.lib_sort {
        1 => rows.sort_by_cached_key(|r| r.1.to_lowercase()),
        2 => rows.sort_by_cached_key(|r| (r.4.to_lowercase(), r.1.to_lowercase())),
        _ => {}
    }
    let pins = &app.cfg.settings.pins;
    rows.sort_by_key(|r| pins.iter().position(|p| *p == r.3).unwrap_or(usize::MAX));
    let k = PINS
        .iter()
        .filter(|p| p.0 >> f & 1 == 1 && hit(p.3))
        .count();
    let ((off, view), (_, cols, _, h)) = (app.lib_scroll, app.lib_layout());
    let lo = ((off / h) as usize).saturating_sub(4) * cols;
    let hi = lo + ((view.max(600.0) / h) as usize + 8) * cols;
    let n = rows.len();
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
    let hit = |name: &str| name.to_lowercase().contains(&q);
    let (sort, (view, cols, lw, rh)) = (app.cfg.settings.lib_sort, app.lib_layout());
    let px = [48.0, 48.0, lw - 8.0, lw - 24.0][view as usize];
    let item = |art: El<'a>, title: &'a str, sub: Cow<'a, str>, route: String| -> El<'a> {
        let same = |r: &str| r == route || r.starts_with(&format!("{route}:"));
        let on = !app.ctx.is_empty() && same(&app.ctx);
        let fg = if on { ACCENT } else { TEXT };
        let tag = format!("lib:{route}");
        let art = match route.as_str() {
            "folders" | "radio" => art,
            _ if app.hover.as_deref() != Some(tag.as_str()) => art,
            _ => {
                let live = on && app.playing;
                let g = [theme::fill::PLAY, theme::fill::PAUSE][usize::from(live)];
                let m = match live {
                    true => Message::Toggle,
                    false => Message::PlayCtx(route.clone(), 0),
                };
                let top: El = match view > 1 {
                    true => {
                        let b = theme::sq(theme::filled(g, 16.0).color(BG).center(), 40.0);
                        let b = b.style(theme::accent).on_press(m);
                        let b = container(b).align_right(px).align_bottom(px);
                        b.padding(8).into()
                    }
                    false => {
                        let r = [4.0, 24.0][usize::from(route.starts_with("artist:"))];
                        let top = container(theme::filled(g, 20.0).color(TEXT)).center(48);
                        let top = button(top.style(fill(BG.scale_alpha(0.5), r))).padding(0);
                        top.style(theme::link(TEXT)).on_press(m).into()
                    }
                };
                stack![art, top].into()
            }
        };
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
        let (enter, exit) = (tag.clone(), tag);
        mouse_area(b.on_press(Message::Nav(route)))
            .on_right_press(menu)
            .on_enter(act(move |a| a.hover = Some(enter.clone())))
            .on_exit(act(move |a| {
                if a.hover.as_ref() == Some(&exit) {
                    a.hover = Some("lib".into());
                }
            }))
            .into()
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
        let m = act(move |a| a.lib_filter = if a.lib_filter == n { 0 } else { n });
        theme::chip_btn(name, f == n, m)
    });
    let chips = Row::with_children(chips).spacing(8);
    let pins = PINS.iter().filter(|p| p.0 >> f & 1 == 1 && hit(p.3));
    let pins = pins.map(|&(_, c, green, t, r)| {
        let (bg, fg) = [(ELEVATED, SUB), (SONGS_BG, ACCENT)][usize::from(green)];
        let (liked, route) = (r == "liked", r.into());
        let pic = liked.then(|| liked_art(px));
        let pic = pic.unwrap_or_else(|| art(px, i(c, 0.0).color(fg), bg));
        item(
            pic,
            t,
            ["Library", "Playlist"][usize::from(liked)].into(),
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
    let rows = rows.map(|(c, name, s, r, _)| {
        let round = r.starts_with("artist:");
        item(cover(app, c, px, round), name, s.into(), r)
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
    let list = list.push(bottom);
    let chips = (!mini).then(|| container(chips).padding([0, 8]));
    let find = task(|a| {
        a.lib_find = a.lib_find.is_none().then(String::new);
        iced::widget::operation::focus("libfind")
    });
    let find = icon_btn(i::SEARCH, 16.0, find);
    let find = tip(find, "Search in Your Library", Bottom);
    let field = app.lib_find.as_ref().map(|v| {
        let style = theme::input([CHIP, CHIP], [CHIP, CHIP], [0.0, 0.0], 4.0);
        let t = text_input("Search in Your Library", v).id("libfind");
        let t = t.on_input(|v| act(move |a| a.lib_find = Some(v.clone())));
        let t = t.on_submit(Message::Noop);
        let t = t.style(style).font(theme::fw(Normal)).size(14);
        t.padding([6, 8]).width(Fill)
    });
    let icon = pages::LIB_VIEWS[view as usize];
    let order = row![txt(pages::LIB_SORTS[sort as usize], 14.0), i(icon, 16.0)].spacing(8);
    let menu = Message::Menu(Box::new(MenuKind::LibSort));
    let open = matches!(app.menu.as_ref().map(|m| &m.kind), Some(MenuKind::LibSort));
    let order = theme::link_btn(order.align_y(Center), [SUB, TEXT][usize::from(open)], menu);
    let order = order.padding([4, 8]);
    let order = tip(order, "Change the sort order", Bottom);
    let gap = field.is_none().then(|| Space::new().width(Fill));
    let tools = row![find].push(field).push(gap).push(order);
    let tools = tools.spacing(8).align_y(Center).width(Fill);
    let tools = (!mini).then(|| container(tools).padding([0, 4]));
    let list = scrollable(list).style(theme::scroll).height(Fill);
    let list = list.on_scroll(|v| Message::LibScrolled(v.absolute_offset().y, v.bounds().height));
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
    let side = |t: Txt<'static>, on: bool, label: &'static str, m: Message| {
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
        let home = if radio {
            "radio".into()
        } else {
            format!("album:{}", s.album_id)
        };
        let title = btn(title, Message::Nav(home));
        let label = ["Add to Liked Songs", "Remove from Liked Songs"][usize::from(on)];
        let like = Message::Star(vec![s.id.clone()], !on);
        let art = btn(cover(app, &s.cover, 56.0, false), Message::Panel(1));
        let lines = column![title, artists_line(&s.artists, 12.0, tw)].spacing(2);
        let heart = (!radio).then(|| side(theme::saved(on, 16.0), false, label, like));
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
        let seek = slider(0.0..=dur, pos, |v| act(move |a| a.seek_preview = Some(v)));
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
        side(dic(i::SHUFFLE), on, shuffle, Message::Shuffle),
        side(dfi(theme::fill::PREV), false, "Previous", Message::Prev),
        play,
        side(dfi(theme::fill::NEXT), false, "Next", Message::Next(true)),
        side(repeat_icon, rep > 0, repeat, act(App::repeat)),
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
    let words = [Message::Nav("lyrics".into()), Message::Back];
    let words = words[usize::from(lyrics)].clone();
    let mute = if app.muted { "Unmute" } else { "Mute" };
    let np = Message::Panel(1);
    let panels = row![
        song.map(|_| side(ic(i::PR), app.panel == 1, "Now playing view", np)),
        song.map(|_| side(ic(i::LYRICS), lyrics, "Lyrics", words)),
        side(ic(i::QUEUE), app.panel == 2, "Queue", Message::Panel(2)),
    ];
    let right = row![
        (!app.full).then_some(panels),
        device.then(|| side(
            ic(i::DEVICE),
            app.panel == 3,
            "Connect to a device",
            Message::Panel(3)
        )),
        side(vol_icon, false, mute, act(|a| a.muted = !a.muted)),
        volume,
        Space::new().width(8),
        match (app.full, song) {
            (false, None) => container(dic(i::FULL)).center(32).into(),
            (full, _) => side(
                ic([i::FULL, i::WINDOW][usize::from(full)]),
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
    let lib = app.data::<Lib>("lib");
    let pl = lib.and_then(|l| l.playlists.iter().find(|p| p.id == id));
    let act = |label: &'static str, bg: [Color; 3], fg: Color, x: u16, m: Option<Message>| {
        let b = button(bold(label, 16.0)).style(theme::solid(bg, fg));
        let b = b.padding([14, x]).on_press_maybe(m.clone());
        focusable(b, m, 500.0, Some("layer"))
    };
    let x = icon_btn(i::X, 16.0, Message::MenuClose);
    let close = focusable(x, Some(Message::MenuClose), 16.0, Some("layer"));
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
        let list = scrollable(list.padding(padding::right(16))).height(Fill);
        let head = row![bold("Keyboard Shortcuts", 32.0).width(Fill), close];
        let hint = row![
            txt("Press", 16.0),
            caps("shortcuts"),
            txt("to open or close this dialog.", 16.0)
        ];
        let top = column![head.align_y(Center), hint.spacing(6).align_y(Center)].spacing(12);
        let body = column![top, list.style(theme::scroll)].spacing(40);
        let h = (app.view_size().1 - 220.0).max(300.0);
        let body = container(body.padding(24)).width(500).height(h);
        return body.style(fill(ELEVATED, 4.0)).into();
    }
    if kind == 5 {
        let key = format!("song:{id}");
        let v = app.data::<serde_json::Value>(&key);
        let val = |k: &str| -> Option<String> {
            let x = v?.get(k)?;
            let s = match k {
                "size" => format!("{:.1} MB", x.as_f64()? / 1e6),
                "duration" => fmt_dur(x.as_f64()? as f32),
                "created" | "played" => pages::date(x.as_str()?),
                "playCount" => {
                    let local = app.plays.get(id).copied().unwrap_or(0) as u64;
                    fmt_n(local.max(x.as_u64()?) as usize)
                }
                "displayAlbumArtist" if v?.get("displayArtist") == Some(x) => return None,
                _ => x.as_str().map_or_else(|| x.to_string(), String::from),
            };
            (!s.is_empty() && s != "0").then_some(s)
        };
        let groups = [
            (
                "Artist",
                &[
                    ("Artist", "displayArtist", ""),
                    ("Album artist", "displayAlbumArtist", ""),
                ][..],
            ),
            (
                "Composition & Lyrics",
                &[("Composer", "displayComposer", "")],
            ),
            (
                "Your listening",
                &[
                    ("Plays", "playCount", ""),
                    ("Last played", "played", ""),
                    ("Added", "created", ""),
                ],
            ),
            (
                "Song",
                &[
                    ("Album", "album", ""),
                    ("Genre", "genre", ""),
                    ("Year", "year", ""),
                    ("Disc", "discNumber", ""),
                    ("Track", "track", ""),
                    ("Duration", "duration", ""),
                    ("BPM", "bpm", ""),
                    ("Comment", "comment", ""),
                ],
            ),
            (
                "File",
                &[
                    ("Format", "suffix", ""),
                    ("Bit rate", "bitRate", " kbps"),
                    ("Sample rate", "samplingRate", " Hz"),
                    ("Bit depth", "bitDepth", " bits"),
                    ("Channels", "channelCount", ""),
                    ("Size", "size", ""),
                    ("Track gain", "trackGain", " dB"),
                    ("Album gain", "albumGain", " dB"),
                    ("Path", "path", ""),
                ],
            ),
        ];
        let groups = groups.into_iter().filter_map(|(name, fields)| {
            let rows: Vec<El> = fields
                .iter()
                .filter_map(|&(label, k, unit)| {
                    let t = text(val(k)? + unit).size(16).font(theme::fw(Normal));
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
        let list = scrollable(list.padding([24, 24]).width(Fill))
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
        let line = container(Space::new().width(Fill).height(1)).style(fill(theme::tint(0.1), 0.0));
        let body = column![container(head).padding(24), line, list];
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
        t.font(theme::fw(Normal)).padding(10).size(14)
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
    let body = column![column![head, fields].spacing(24), save].spacing(8);
    let body = body.width(524).padding(24);
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
    let modal = app.modal.as_ref().map(|(id, kind)| {
        let card = container(modal(app, id, *kind)).center(Fill);
        let dim = if matches!(kind, 2 | 4) { 0.5 } else { 0.7 };
        stack![scrim(BG.scale_alpha(dim)), card]
    });
    let menu = app.menu.as_ref().map(|m| {
        let (ww, wh) = app.view_size();
        let (el, w, h) = pages::menu(app, m, 0);
        let x = if m.x + w + 8.0 > ww { m.x - w } else { m.x };
        let y = if m.y + h + 8.0 > wh { m.y - h } else { m.y };
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
            container(el).padding(padding::top(sy).left(sx.max(8.0)))
        });
        let at = padding::top(y).left(x);
        stack![scrim(theme::CLEAR), container(el).padding(at)].push(child)
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
        let body = container(body).style(theme::down(&bg));
        let top = [modal, menu]
            .into_iter()
            .flatten()
            .map(El::from)
            .chain(toast.map(El::from));
        return stack![body].extend(top).into();
    }
    let scroll = scrollable(pages::route(app)).id(SCROLL).on_scroll(|v| {
        let (b, c) = (v.bounds().height, v.content_bounds().height);
        Message::Scrolled(v.absolute_offset().y, b, c)
    });
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
