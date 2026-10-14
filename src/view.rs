use iced::font::Weight::{Black, Medium, Normal};
use iced::widget::tooltip::Position::{Bottom, Right, Top};
use iced::widget::{
    Row, Space, button, column, container, image, mouse_area, row, scrollable, slider, stack, text,
    text_input, tooltip,
};
use iced::{Background, Center, Color, Element, Fill, FillPortion, color, padding};

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

pub fn focusable<'a>(
    c: impl Into<El<'a>>,
    msg: Option<Message>,
    radius: f32,
    id: Option<&'static str>,
) -> El<'a> {
    use iced::advanced::renderer::{Quad, Style};
    use iced::advanced::widget::{Id, Operation, Tree, Widget, operation::Focusable, tree};
    use iced::advanced::{Clipboard, Layout, Renderer, Shell, layout};
    use iced::keyboard::{Event::KeyPressed, Key, key::Named};
    use iced::{Event, Length, Rectangle as Rect, Size, mouse::Cursor, mouse::Interaction};
    type R = iced::Renderer;
    type T = iced::Theme;
    struct Focus<'a>(El<'a>, Option<Message>, f32, Option<Id>);
    #[derive(Default)]
    struct Ring(bool);
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
    impl Widget<Message, T, R> for Focus<'_> {
        fn size(&self) -> Size<Length> {
            self.0.as_widget().size()
        }
        fn layout(&mut self, t: &mut Tree, r: &R, l: &layout::Limits) -> layout::Node {
            self.0.as_widget_mut().layout(&mut t.children[0], r, l)
        }
        fn draw(&self, t: &Tree, r: &mut R, th: &T, s: &Style, l: Layout, c: Cursor, v: &Rect) {
            self.0.as_widget().draw(&t.children[0], r, th, s, l, c, v);
            let (e, mut q) = (self.2.min(4.0), Quad::default());
            q.border = iced::border::rounded(self.2 + e).color(TEXT).width(2);
            q.bounds = l.bounds().expand(e);
            if t.state.downcast_ref::<Ring>().0 {
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
            op.focusable(self.3.as_ref(), l.bounds(), t.state.downcast_mut::<Ring>());
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
            let w = self.0.as_widget_mut();
            w.update(&mut t.children[0], e, l, c, r, cb, sh, v);
            let ring = t.state.downcast_mut::<Ring>();
            let key = |k| matches!(e, Event::Keyboard(KeyPressed { key, .. }) if *key == k);
            let go = key(Key::Named(Named::Enter)) || key(Key::Named(Named::Space));
            match e {
                _ if !ring.0 => return,
                Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => ring.0 = false,
                _ if key(Key::Named(Named::Escape)) => ring.0 = false,
                _ if go && !sh.is_event_captured() => {
                    self.1.iter().for_each(|m| sh.publish(m.clone()));
                }
                _ => return,
            }
            if matches!(e, Event::Keyboard(_)) {
                sh.capture_event();
            }
            sh.request_redraw();
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
    Element::new(Focus(c.into(), msg, radius, id.map(Id::from)))
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

pub fn media<'a>(art: El<'a>, title: &str, sub: &str, w: f32, fg: Color) -> Row<'a, Message> {
    let title = theme::ell(title, w, 16.0, Normal).color(fg);
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
        let t = theme::ell(&name, left, size, Normal).color(SUB);
        left -= theme::width(&name, size, Normal);
        let link = format!("artist:{}", a.id);
        r = r.push(match a.id.is_empty() {
            true => El::from(t),
            false => theme::link_btn(t, SUB, Message::Nav(link)).into(),
        });
    }
    r.into()
}

pub fn fmt_dur(s: f32) -> String {
    format!("{}:{:02}", s as u64 / 60, s as u64 % 60)
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
        s => format!("{} min {} sec", s / 60, s % 60),
    }
}

pub fn tip<'a>(c: impl Into<El<'a>>, label: impl ToString, at: tooltip::Position) -> El<'a> {
    let body = container(txt(label, 14.0)).padding([4, 8]);
    let body = body.style(fill(theme::MENU, 4.0));
    let delay = std::time::Duration::from_millis(200);
    tooltip(c, body, at).gap(8).delay(delay).into()
}

fn top_bar(app: &App) -> El<'_> {
    let nav = |c: char, on: bool, label: &str, m: Message| {
        let b = theme::sq(i(c, 22.0).center(), 32.0);
        let b = b.style(theme::link(if on { SUB } else { theme::DIM }));
        tip(b.on_press_maybe(on.then_some(m)), label, Bottom)
    };
    let round = |c: Txt<'static>, label: &str, m: Message| {
        let b = button(container(c.center()).center(48)).padding(0);
        tip(b.style(theme::round(ELEVATED)).on_press(m), label, Bottom)
    };
    let user = app.session.as_ref().map_or("", |s| s.server.user.as_str());
    let first = user.chars().next().map_or('?', |c| c.to_ascii_uppercase());
    let ring = fill(theme::color_of(&first.to_string()), 500.0);
    let avatar = container(bold(first, 14.0).color(BG).center()).center(32);
    let avatar = button(avatar.style(ring)).padding(8);
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
    let search = text_input("What do you want to play?", &app.search_q);
    let search = search.id("search").on_input(Message::Search);
    let search = search.on_submit(Message::SearchGo(app.search_seq));
    let search = search.style(style).icon(glass).width(474).size(16);
    let search = search.font(theme::fw(Normal)).padding([13, 16]);
    let search = search.line_height(text::LineHeight::Absolute(22.0.into()));
    let browse = match app.route.starts_with("search") {
        true => Message::Noop,
        false => Message::Nav("search".into()),
    };
    let search = mouse_area(search).on_release(browse);
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

pub fn lib_rows(app: &App) -> (usize, Vec<LibRow<'_>>, std::ops::Range<usize>) {
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
    let pins = [
        (f < 2, "Liked Songs"),
        (f == 0, "Songs"),
        (f == 2, "All albums"),
    ];
    let pins = pins
        .into_iter()
        .chain([(f == 3, "All artists"), (f == 0, "Folders")]);
    let k = pins.filter(|p| p.0 && hit(p.1)).count();
    let (off, view) = app.lib_scroll;
    let lo = ((off / 64.0) as usize)
        .saturating_sub(k + 4)
        .min(rows.len());
    let hi = (lo + (view.max(600.0) / 64.0) as usize + 8).min(rows.len());
    (k, rows, lo..hi)
}

fn lib_panel(app: &App) -> El<'_> {
    let (w, f, mini) = (app.lib_w(), app.lib_filter, app.lib_mini());
    let q = app.lib_find.as_deref().unwrap_or("").to_lowercase();
    let hit = |name: &str| name.to_lowercase().contains(&q);
    let sort = app.cfg.settings.lib_sort;
    let item = |art: El<'static>, title: &str, sub: &str, route: String| -> El<'static> {
        let same = |r: &str| r == route || r.starts_with(&format!("{route}:"));
        let on = !app.ctx.is_empty() && same(&app.ctx);
        let fg = if on { ACCENT } else { TEXT };
        let row = |art| media(art, title, sub, w - 112.0, fg).into();
        let body = if mini { art } else { row(art) };
        let b = button(body).padding(8).width(Fill);
        let b = b.style(theme::track(same(&app.route)));
        b.on_press(Message::Nav(route)).into()
    };
    let head: El = if mini {
        let b = icon_btn(i::LIBRARY, 20.0, Message::LibCollapse);
        let b = tip(b, "Expand Your Library", Right);
        container(b).center_x(Fill).into()
    } else {
        let title = bold("Your Library", 16.0);
        let title = theme::link_btn(title, TEXT, Message::LibCollapse);
        let plus = row![i(i::PLUS, 16.0), bold("Create", 14.0)].spacing(8);
        let plus = button(container(plus.align_y(Center)).center_y(32)).padding([0, 16]);
        let plus = plus
            .style(theme::round(CHIP))
            .on_press(Message::NewPlaylist);
        let plus = tip(plus, "Create playlist", Bottom);
        let title = tip(title, "Collapse Your Library", Bottom);
        let r = row![title, Space::new().width(Fill), plus];
        r.align_y(Center).into()
    };
    let chips = [(1u8, "Playlists"), (2, "Albums"), (3, "Artists")].map(|(n, name)| {
        let m = act(move |a| a.lib_filter = if a.lib_filter == n { 0 } else { n });
        theme::chip_btn(name, f == n, m)
    });
    let chips = Row::with_children(chips).spacing(8);
    let (green, dim) = ((SONGS_BG, ACCENT), (ELEVATED, SUB));
    let pins = [
        (f == 0, i::MUSIC, green, "Songs", "songs"),
        (f == 2, i::ALBUM, dim, "All albums", "albums"),
        (f == 3, i::USER, dim, "All artists", "artists"),
        (f == 0, i::FOLDER, dim, "Folders", "folders"),
    ];
    let pins = pins.into_iter().filter(|p| p.0 && hit(p.3));
    let pins = pins.map(|(_, c, (bg, fg), t, r)| {
        let art = art(48.0, i(c, 0.0).color(fg), bg);
        item(art, t, "Library", r.into())
    });
    let (_, rows, span) = lib_rows(app);
    let gap = |n: usize| Space::new().height(n as f32 * 64.0);
    let (top, bottom) = (gap(span.start), gap(rows.len() - span.end));
    let rows = rows.into_iter().take(span.end).skip(span.start);
    let rows = rows.map(|(c, n, s, r, _)| {
        let round = r.starts_with("artist:");
        item(cover(app, c, 48.0, round), n, &s, r)
    });
    let liked = || item(liked_art(48.0), "Liked Songs", "Playlist", "liked".into());
    let liked = (f < 2 && hit("Liked Songs")).then(liked);
    let list = column![liked].extend(pins).push(top);
    let list = list.extend(rows).push(bottom);
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
        let t = t.style(style).font(theme::fw(Normal)).size(14);
        t.padding([6, 8]).width(Fill)
    });
    let names = ["Custom order", "Alphabetical", "Creator"];
    let order = row![txt(names[sort as usize], 14.0), i(i::LIST, 16.0)].spacing(8);
    let next = set(|s, n| s.lib_sort = n, (sort + 1) % 3);
    let order = theme::link_btn(order.align_y(Center), SUB, next).padding([4, 8]);
    let order = tip(order, "Change the sort order", Bottom);
    let gap = field.is_none().then(|| Space::new().width(Fill));
    let tools = row![find].push(field).push(gap).push(order);
    let tools = tools.spacing(8).align_y(Center).width(Fill);
    let tools = (!mini).then(|| container(tools).padding([0, 4]));
    let list = scrollable(list).style(theme::scroll).height(Fill);
    let list = list.on_scroll(|v| Message::LibScrolled(v.absolute_offset().y, v.bounds().height));
    let body = column![container(head).padding([4, 8]), chips, tools, list];
    let body = container(body.spacing(12).padding(8)).width(w);
    body.height(Fill).into()
}

fn player_bar(app: &App) -> El<'_> {
    let song = app.current_song();
    let side = |t: Txt<'static>, on: bool, label: &str, m: Message| {
        let dot = container(Space::new().width(4).height(4)).style(fill(ACCENT, 2.0));
        let dot = on.then(|| container(dot).center_x(Fill).align_bottom(Fill));
        let b = theme::sq(stack![container(t).center(Fill)].push(dot), 32.0);
        let b = b.style(theme::link(if on { ACCENT } else { SUB }));
        tip(b.on_press(m), label, Top)
    };
    let left = song.filter(|_| !app.full).map(|s| {
        let on = app.is_starred(&s.id, s.starred);
        let title = theme::ell(&s.title, 200.0, 14.0, Normal);
        let title = btn(title, Message::Nav(format!("album:{}", s.album_id)));
        let label = ["Add to Liked Songs", "Remove from Liked Songs"][usize::from(on)];
        let like = Message::Star(vec![s.id.clone()], !on);
        let art = btn(cover(app, &s.cover, 56.0, false), Message::Panel(1));
        let lines = column![title, artists_line(&s.artists, 12.0, 200.0)].spacing(2);
        let heart = side(theme::saved(on, 16.0), false, label, like);
        row![art, lines, heart].spacing(12).align_y(Center)
    });
    let dur = song.map_or(0.0, |s| s.duration).max(1.0);
    let pos = app.seek_preview.unwrap_or(app.position.min(dur));
    let rest = app.cfg.settings.remaining;
    let left_dur = format!("-{}", fmt_dur(dur - pos));
    let total = if rest { left_dur } else { fmt_dur(dur) };
    let total = button(txt(total, 12.0)).width(48).padding(0);
    let total = total.style(theme::link(SUB));
    let total = total.on_press(set(|s, b| s.remaining = b, !rest));
    let seek = slider(0.0..=dur, pos, |v| act(move |a| a.seek_preview = Some(v)));
    let seek = seek.on_release(Message::SeekRelease).style(theme::seek);
    let at = sub(fmt_dur(pos), 12.0).width(48).align_x(iced::Right);
    let progress: El = match song {
        None => {
            let rail = Space::new().width(Fill).height(4);
            let rail = container(rail).style(fill(theme::TRACK, 2.0));
            let none = || sub("-:--", 12.0).width(48);
            let row = row![none().align_x(iced::Right), rail, none()].spacing(8);
            container(row.align_y(Center)).center_y(16).into()
        }
        Some(_) => row![at, seek, total].spacing(8).align_y(Center).into(),
    };
    let playing = usize::from(app.playing);
    let glyph = theme::filled([theme::fill::PLAY, theme::fill::PAUSE][playing], 16.0);
    let bg = song.map_or(theme::DIM, |_| TEXT);
    let play = theme::sq(glyph.color(BG).center(), 32.0).style(theme::solid([bg; 3], BG));
    let play = play.on_press(Message::Toggle);
    let play = tip(play, ["Play", "Pause"][playing], Top);
    let (ic, fi) = (|c| i(c, 16.0), |c| theme::filled(c, 16.0));
    let (on, rep) = (app.shuffle.is_some(), usize::from(app.repeat));
    let shuffle = ["Enable shuffle", "Disable shuffle"][usize::from(on)];
    let repeat = ["Enable repeat", "Enable repeat one", "Disable repeat"][rep];
    let repeat_icon = ic([i::REPEAT, i::REPEAT, i::REPEAT1][rep]);
    let controls = row![
        side(ic(i::SHUFFLE), on, shuffle, Message::Shuffle),
        side(fi(theme::fill::PREV), false, "Previous", Message::Prev),
        play,
        side(fi(theme::fill::NEXT), false, "Next", Message::Next(true)),
        side(repeat_icon, rep > 0, repeat, Message::Repeat),
    ];
    let center = column![controls.spacing(8).align_y(Center), progress];
    let center = center.spacing(4).align_x(Center).max_width(722);
    let vol = if app.muted { 0.0 } else { app.volume };
    let level = usize::from(vol > 0.0) + usize::from(vol >= 0.5);
    let vol_icon = ic([i::MUTE, i::VOL1, i::VOL2][level]);
    let lyrics = app.route == "lyrics";
    let volume = slider(0.0..=1.0, vol, Message::Vol).step(0.01f32).width(93);
    let volume = mouse_area(volume.style(theme::seek)).on_scroll(move |d| {
        let step = match d {
            iced::mouse::ScrollDelta::Lines { y, .. } => y.clamp(-1.0, 1.0) / 10.0,
            iced::mouse::ScrollDelta::Pixels { y, .. } => y.clamp(-120.0, 120.0) / 1200.0,
        };
        Message::Vol((vol + step).clamp(0.0, 1.0))
    });
    let words = [Message::Nav("lyrics".into()), Message::Back];
    let words = words[usize::from(lyrics)].clone();
    let mute = if app.muted { "Unmute" } else { "Mute" };
    let np = Message::Panel(1);
    let panels = row![
        side(ic(i::PR), app.panel == 1, "Now playing view", np),
        side(ic(i::LYRICS), lyrics, "Lyrics", words),
        side(ic(i::QUEUE), app.panel == 2, "Queue", Message::Panel(2)),
    ];
    let right = row![
        (!app.full).then(|| panels.spacing(4)),
        side(
            ic(i::DEVICE),
            false,
            "Connect to a device",
            Message::Menu(Box::new(MenuKind::Device))
        ),
        side(vol_icon, false, mute, Message::Mute),
        volume,
        (app.full || app.current_song().is_some()).then(|| side(
            ic([i::FULL, i::WINDOW][usize::from(app.full)]),
            false,
            ["Full screen", "Exit full screen"][usize::from(app.full)],
            Message::Full(!app.full),
        )),
    ];
    let right = container(right.spacing(4).align_y(Center)).align_right(FillPortion(3));
    let left = container(left).width(FillPortion(3));
    let bar = row![left, container(center).center_x(FillPortion(4)), right];
    bar.align_y(Center).padding([0, 8]).height(72).into()
}

fn modal(app: &App, id: &str, kind: u8) -> El<'static> {
    let lib = app.data::<Lib>("lib");
    let pl = lib.and_then(|l| l.playlists.iter().find(|p| p.id == id));
    let act = |label: &'static str, bg: [Color; 3], fg: Color, x: u16, m: Option<Message>| {
        let b = button(bold(label, 16.0)).style(theme::solid(bg, fg));
        let b = b.padding([12, x]).on_press_maybe(m.clone());
        focusable(b, m, 500.0, Some("layer"))
    };
    let x = icon_btn(i::X, 16.0, Message::MenuClose);
    let close = focusable(x, Some(Message::MenuClose), 16.0, Some("layer"));
    let style = fill(theme::MENU, 8.0);
    if kind == 3 {
        let chip = |k: &str| {
            let k = container(bold(k.to_string(), 12.0)).padding([4, 8]);
            k.style(fill(ELEVATED, 4.0)).into()
        };
        let line = |k: &crate::app::KeyAction| {
            let c = crate::app::key_label(app.binding(k.0));
            let c = if c.is_empty() { "Not set".into() } else { c };
            let chips = Row::with_children(c.split('+').map(chip));
            let what = txt(k.1, 14.0).color(SUB).width(Fill);
            row![what, chips.spacing(4)]
                .align_y(Center)
                .height(40)
                .into()
        };
        let list = iced::widget::Column::with_children(crate::app::KEYS.iter().map(line));
        let list = scrollable(list.padding(padding::right(16)))
            .height(440)
            .style(theme::scroll);
        let head = row![bold("Keyboard shortcuts", 24.0).width(Fill), close];
        let hint = sub("You can change the shortcuts in Settings.", 14.0);
        let body = column![head.align_y(Center), hint, list].spacing(16);
        return container(body.width(480).padding(24)).style(style).into();
    }
    if kind == 2 || kind == 4 {
        let name = pl.map_or("", |p| p.name.as_str()).to_string();
        let b = theme::fw(iced::font::Weight::Bold);
        let red = [theme::RED, color!(0xd81326), color!(0xa60e1d)];
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
        let cancel = act("Cancel", [theme::CLEAR; 3], BG, 16, close);
        let ok = act(label, bg, fg, 32, Some(ok));
        let buttons = row![Space::new().width(Fill), cancel, ok].spacing(16);
        let buttons = buttons.align_y(Center).padding(padding::top(16));
        let title = bold(title, 24.0).color(BG);
        let body = column![title, msg, buttons].spacing(8);
        let body = body.width(420).padding(32);
        return container(body).style(fill(TEXT, 8.0)).into();
    }
    let named = !app.new_pl.trim().is_empty();
    let (name, desc) = (app.new_pl.clone(), app.new_pl_desc.clone());
    let ok = named.then(|| Message::Edit(id.into(), name, desc));
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
    let desc = input("Add an optional description", &app.new_pl_desc, 4);
    let art = cover(app, pl.map_or("", |p| p.cover.as_str()), 180.0, false);
    let white = [TEXT, color!(0xf0f0f0), color!(0xf0f0f0)];
    let save = container(act("Save", white, BG, 32, ok)).align_right(Fill);
    let head = row![bold("Edit details", 24.0).width(Fill), close].align_y(Center);
    let fields = row![art, column![name, desc].spacing(16)].spacing(16);
    let body = column![head, fields, save].spacing(24);
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
        s.on_right_press(Message::MenuClose)
    };
    let modal = app.modal.as_ref().map(|(id, kind)| {
        let card = container(modal(app, id, *kind)).center(Fill);
        stack![scrim(BG.scale_alpha(0.7)), card]
    });
    let menu = app.menu.as_ref().map(|m| {
        let (el, w, h) = pages::menu(app, m);
        let (ww, wh) = app.view_size();
        let x = if m.x + w + 8.0 > ww { m.x - w } else { m.x };
        let y = if m.y + h + 8.0 > wh { m.y - h } else { m.y };
        let (x, y) = (x.min(ww - w - 8.0).max(8.0), y.min(wh - h - 8.0).max(8.0));
        let at = padding::top(y).left(x);
        stack![scrim(theme::CLEAR), container(el).padding(at)]
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
        let from = s.map(|_| column![sub("Playing from", 12.0), bold(app.source.clone(), 14.0)]);
        let body = column![from.map(|f| f.spacing(4)), Space::new().height(Fill)].push(info);
        let body = column![body.padding(48).height(Fill), player_bar(app)];
        let bg = [(0.0, top.unwrap_or(ELEVATED)), (1.0, BG)];
        let body = container(body).style(theme::down(&bg));
        return stack![body, modal, menu, toast].into();
    }
    let scroll = scrollable(pages::route(app)).id(SCROLL).on_scroll(|v| {
        let (b, c) = (v.bounds().height, v.content_bounds().height);
        Message::Scrolled(v.absolute_offset().y, b, c)
    });
    let scroll = scroll.style(theme::scroll).height(Fill);
    let main = stack![scroll].push(pages::sticky(app));
    let main = container(main).width(Fill).height(Fill).clip(true);
    let gap = || container(Space::new().width(8).height(Fill)).style(fill(BG, 0.0));
    let grip = |n| {
        let g = mouse_area(gap());
        let g = g.interaction(iced::mouse::Interaction::ResizingHorizontally);
        g.on_press(Message::Drag(Some(n)))
    };
    let right = (app.panel > 0).then(|| {
        let p = [pages::queue_panel, pages::now_playing][usize::from(app.panel == 1)](app);
        let p = container(p).width(app.right_w()).height(Fill);
        row![grip(1), p]
    });
    let panels = row![gap(), lib_panel(app), grip(0), main].push(right);
    let black = |e| container(e).style(fill(BG, 0.0));
    let (top, bottom) = (black(top_bar(app)), black(player_bar(app)));
    let base = column![top, panels.push(gap()).height(Fill), bottom];
    let caps = |w: f32| {
        let cap = |k: usize| image(CAPS[k].clone()).width(8).height(8);
        let r = |a, b| row![cap(a), Space::new().width(Fill), cap(b)];
        column![r(0, 1), Space::new().height(Fill), r(2, 3)].width(w)
    };
    let gap = || Space::new().width(8);
    let right = (app.panel > 0).then(|| row![gap(), caps(app.right_w())]);
    let caps = row![gap(), caps(app.lib_w()), gap(), caps(app.main_w())].push(right);
    let pad = |h| Space::new().height(h);
    let caps = column![pad(64), caps.height(Fill), pad(72)];
    stack![base, caps, modal, menu, toast].into()
}
