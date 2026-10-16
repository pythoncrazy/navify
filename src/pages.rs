use std::ops::RangeInclusive;

use iced::font::Weight::{Black, Bold, Medium, Normal};
use iced::widget::{
    Column, Row, Space, button, column, container, hover, image, mouse_area, pick_list, row,
    scrollable, slider, stack, text, text_input, toggler,
};
use iced::{Center, Color, Fill, FillPortion, Padding, Right, color, padding};

use crate::api::{Album, Artist, Genre, Info, Lyrics, Playlist, Server, Song, Trio};
use crate::app::{App, Home, Lib, Menu, MenuKind, Message, act, later, set, task};
use crate::config::Settings;
use crate::theme::{
    self, ACCENT, BG, CLEAR, ELEVATED, SUB, TEXT, bold, ell, fill, icon as i, link_btn, sub, txt,
};
use crate::view::{
    El, SONGS_BG, art, artists_line, btn, cover, fmt_dur, fmt_long, fmt_n, focusable, go, icon_btn,
    liked_art, media, tool_btn,
};

type Cols = (f32, f32, f32, f32);
type Hit = (usize, usize);

pub const PAD: f32 = 24.0;
pub const GRID_Y: f32 = 184.0;
pub const DIR_Y: f32 = 90.0;
pub const BAR: f32 = 84.0;
pub const HEAD: f32 = 53.0;
const DISC_HEAD: f32 = 136.0 + 16.0 + HEAD;
const GREY: Color = color!(0x535353);
const LIKED: Color = color!(0x5038a0);
const ORDER: [&str; 6] = [
    "Custom order",
    "Title",
    "Artist",
    "Album",
    "Date added",
    "Duration",
];
pub const DOT: &str = " \u{2022} ";
const GROUPS: [(&str, &str); 4] = [
    ("all", "Popular releases"),
    ("album", "Albums"),
    ("single", "Singles and EPs"),
    ("compilation", "Compilations"),
];

fn head_color(app: &App, id: &str) -> Color {
    app.covers.color(id).unwrap_or(GREY)
}

pub fn art_px(app: &App) -> f32 {
    (128.0 + (app.main_w() - 600.0) * 104.0 / 424.0).clamp(128.0, 232.0)
}

pub fn banner_h(app: &App) -> f32 {
    (app.view_size().1 * 0.4).clamp(340.0, 500.0)
}

pub fn banner(app: &App, a: &Artist) -> Option<String> {
    let url = app.data::<String>(&a.banner_key())?;
    let tenths = (app.main_w() / banner_h(app) * 10.0).round();
    (app.cfg.settings.banners && !url.is_empty()).then(|| format!("b{tenths}:{url}"))
}

pub fn hero_h(app: &App) -> f32 {
    art_px(app) + PAD + 64.0
}

fn own_pl(app: &App) -> Option<&(Playlist, Vec<Song>)> {
    let pl = app.data::<(Playlist, Vec<Song>)>(&app.route);
    pl.filter(|(p, _)| app.owns(p))
}

pub fn list_y(app: &App) -> f32 {
    let tools = own_pl(app).is_some_and(|x| !x.1.is_empty());
    hero_h(app) + BAR + HEAD + [0.0, 53.0][usize::from(tools)]
}

pub fn add_songs(app: &App) -> impl Iterator<Item = &Song> {
    let q = app.pl_more.as_deref().unwrap_or("").trim();
    let hits = app
        .data::<Trio>(&format!("search:{q}"))
        .filter(|_| !q.is_empty());
    let recent = q.is_empty().then_some(&app.recent[..]);
    let all = hits.map(|t| &t.2[..]).or(recent).unwrap_or(&[]);
    all.iter().take(20)
}

fn find_input<'a>(q: &str, id: &'static str) -> iced::widget::TextInput<'a, Message> {
    let field = text_input("Search for songs", q).id(id);
    let field = field.on_input(|v| {
        task(move |a| {
            a.pl_more = Some(v.clone());
            let v = v.trim().to_string();
            let go = task(
                move |a| match a.pl_more.as_deref().map(str::trim) == Some(&v) {
                    true if !v.is_empty() => a.request(&format!("search:{v}"), 0),
                    _ => iced::Task::none(),
                },
            );
            later(0.25, go)
        })
    });
    let glass = text_input::Icon {
        font: theme::LUCIDE,
        code_point: i::SEARCH,
        size: Some(16.0.into()),
        spacing: 8.0,
        side: text_input::Side::Left,
    };
    field.icon(glass).font(theme::fw(Normal))
}

fn plural(n: u32, word: &str) -> String {
    let s = if n == 1 { "" } else { "s" };
    format!("{} {word}{s}", fmt_n(n as usize))
}

fn cols(app: &App, album: bool, date: bool, n: usize) -> Cols {
    let idx = 16.0 + 8.0 * n.max(1).ilog10().saturating_sub(1) as f32;
    let st = &app.cfg.settings;
    let extra = 106.0 * f32::from(st.ratings) + 80.0 * f32::from(st.plays);
    let w = app.main_w() - 2.0 * PAD - 184.0 - idx - extra;
    let (a, d) = (f32::from(album && w > 480.0), f32::from(date && w > 640.0));
    let w = w - 16.0 * (a + d);
    let fr = w / (6.0 + 4.0 * a + 3.0 * d);
    (
        w - fr * (4.0 * a + 3.0 * d),
        fr * 4.0 * a,
        fr * 3.0 * d,
        idx,
    )
}

fn blk<'a>(w: f32, h: f32, r: f32) -> El<'a> {
    let b = container(Space::new().width(w).height(h));
    b.style(fill(theme::CHIP, r)).into()
}

fn huge(t: &str, tw: f32) -> text::Text<'_> {
    let fits = |s: &f32| theme::width(t, *s, Black) <= tw;
    let size = [96.0, 72.0, 48.0].into_iter().find(fits).unwrap_or(32.0);
    ell(t, tw, size, Black).line_height(1.1)
}

pub fn grid_y(app: &App) -> f32 {
    match app.route.split(':').next() {
        Some("genre") => hero_h(app) + PAD,
        Some("artists") => GRID_Y - 40.0,
        _ => GRID_Y,
    }
}

fn genre_color(app: &App, name: &str) -> Color {
    let gs = app.data::<Vec<Genre>>("genres");
    let n = gs.and_then(|g| g.iter().position(|x| x.name == name));
    theme::hue(n.unwrap_or_else(|| name.bytes().map(usize::from).sum()))
}

fn fade<'a>(top: Color, h: f32, body: impl Into<El<'a>>) -> El<'a> {
    let g = container(Space::new()).width(Fill).height(h);
    let g = g.style(theme::down(&[(0.0, top), (1.0, theme::PANEL)]));
    stack![body.into()].push_under(g).into()
}

fn rule<'a>() -> El<'a> {
    let line = container(Space::new().width(Fill).height(1));
    line.style(fill(theme::tint(0.1), 0.0)).into()
}

fn play_btn<'a>(app: &App, px: f32, ctx: &str) -> El<'a> {
    let on = app.ctx == ctx && app.current_song().is_some();
    let g = [theme::fill::PLAY, theme::fill::PAUSE][usize::from(on && app.playing)];
    let b = theme::sq(theme::filled(g, px * 0.4).color(BG).center(), px);
    let m = match on {
        true => Message::Toggle,
        false => Message::PlayCtx(ctx.into(), 0),
    };
    b.style(theme::accent).on_press(m).into()
}

fn play_over<'a>(app: &App, base: El<'a>, top: El<'a>, ctx: &str) -> El<'a> {
    match app.playing && app.ctx == ctx {
        true => stack![base, top].into(),
        false => hover(base, top),
    }
}

fn heart_btn<'a>(id: &str, on: bool, px: f32) -> El<'a> {
    let b = theme::sq(theme::saved(on, px).center(), 32.0).style(theme::link(SUB));
    b.on_press(Message::Star(vec![id.into()], !on)).into()
}

fn bar_btn<'a>(c: char, m: Message) -> El<'a> {
    let b = theme::sq(i(c, 28.0).center(), 32.0);
    b.on_press(m).style(theme::link(SUB)).into()
}

fn dots<'a>(k: MenuKind) -> El<'a> {
    bar_btn(i::DOTS, Message::Menu(Box::new(k)))
}

fn heading<'a>(t: impl text::IntoFragment<'a>, all: Option<String>) -> El<'a> {
    let all = all.map(|r| link_btn(bold("Show all", 14.0), SUB, Message::Nav(r)));
    let r = row![bold(t, 24.0).width(Fill)].push(all).align_y(Center);
    container(r).padding([0.0, PAD]).into()
}

fn shelf<'a>(
    title: impl text::IntoFragment<'a>,
    all: Option<String>,
    cards: impl Iterator<Item = El<'a>>,
) -> El<'a> {
    let cards: Vec<El> = cards.collect();
    if cards.is_empty() {
        return Space::new().height(0).into();
    }
    let row = container(Row::with_children(cards)).padding([0, 12]);
    column![heading(title, all), row].spacing(8).into()
}

fn card<'a>(
    app: &'a App,
    cid: &str,
    title: &'a str,
    sub: String,
    route: String,
    k: MenuKind,
) -> El<'a> {
    let round = matches!(k, MenuKind::Artist(_));
    let cw = app.grid().1;
    let img = cw - 24.0;
    let art = cover(app, cid, img, round);
    let play = play_btn(app, 48.0, &route);
    let play = container(play).align_right(Fill).align_bottom(Fill);
    let over = |art| play_over(app, art, play.padding(8).into(), &route);
    let art = if round { art } else { over(art) };
    let tag = format!("card:{:p}", title.as_ptr());
    let title = ell(title, img, 16.0, Normal);
    let lines = column![title, ell(sub, img, 14.0, Normal).color(SUB)].spacing(4);
    let on = app.menu_on(&route) && app.hover.as_deref() == Some(tag.as_str());
    let idle = [theme::CLEAR, ELEVATED][usize::from(on)];
    let style = theme::btn([idle, ELEVATED, BG], [TEXT; 3], 6.0);
    let b = button(column![art, lines].spacing(8)).padding(12);
    let b = b.style(style).width(cw).height(cw + 52.0);
    let b = b.on_press(Message::Nav(route));
    let menu = task(move |a| {
        a.hover = Some(tag.clone());
        a.update(Message::Menu(Box::new(k.clone())))
    });
    mouse_area(b).on_right_press(menu).into()
}

fn album_card<'a>(app: &'a App, a: &'a Album, sub: String) -> El<'a> {
    let (r, k) = (
        format!("album:{}", a.id),
        MenuKind::Album(Box::new(a.clone())),
    );
    card(app, &a.cover, &a.name, sub, r, k)
}

fn artist_card<'a>(app: &'a App, a: &'a Artist) -> El<'a> {
    let (r, k) = (format!("artist:{}", a.id), MenuKind::Artist(a.clone()));
    card(app, &a.cover, &a.name, "Artist".into(), r, k)
}

fn playlist_card<'a>(app: &'a App, p: &'a Playlist) -> El<'a> {
    let (by, r) = (format!("By {}", p.owner), format!("playlist:{}", p.id));
    card(app, &p.cover, &p.name, by, r, MenuKind::Playlist(p.clone()))
}

fn table_head<'a>(app: &App, c: Cols) -> El<'a> {
    let h = |t: &'static str, w: f32| (w > 0.0).then(|| sub(t, 14.0).width(w));
    let half = c.0 / 2.0 - 8.0;
    let split = app.cfg.settings.compact && !app.route.starts_with("artist:");
    let title = match split {
        true => row![h("Title", half), h("Artist", half)].spacing(16),
        false => row![h("Title", c.0)],
    };
    let clock = container(i(i::CLOCK, 16.0).color(SUB)).align_right(120);
    let r = row![sub("#", 16.0).width(c.3).center(), title];
    let album = app.route.starts_with("album:");
    let second: Option<El> = match album {
        true => h("Plays", c.1).map(|t| container(t.width(100).align_x(Right)).width(c.1).into()),
        false => h("Album", c.1).map(Into::into),
    };
    let r = r.push(second).push(h("Date added", c.2));
    let star = container(i(i::STAR, 16.0).color(SUB)).width(90);
    let r = r.push(app.cfg.settings.ratings.then_some(star));
    let plays = sub("Plays", 14.0).width(64).align_x(Right);
    let r = r.push((app.cfg.settings.plays && !album).then_some(plays));
    let r = r.push(clock.padding(padding::right(52)));
    let r = r.spacing(16).padding([0, 16]).height(36).align_y(Center);
    let p = Padding::new(PAD).top(0).bottom(16);
    container(column![r, rule()]).padding(p).into()
}

pub fn date(s: &str) -> String {
    const M: &str = "JanFebMarAprMayJunJulAugSepOctNovDec";
    let d = s.get(..10).unwrap_or("").split('-');
    let d = d.filter_map(|x| x.parse().ok());
    match d.collect::<Vec<usize>>()[..] {
        [y, m @ 1..=12, day] => format!("{} {day}, {y}", &M[m * 3 - 3..m * 3]),
        _ => String::new(),
    }
}

fn track<'a>(app: &'a App, s: &'a Song, n: usize, key: &str, c: Cols) -> El<'a> {
    let pop = key.starts_with("artist:");
    let hk = format!("{key}#{n}");
    let hot = app.hover.as_deref() == Some(hk.as_str());
    let sel = app.sel.as_ref().is_some_and(|x| x.0 == key) && app.picked.contains(&n);
    let cur = app.current_song().is_some_and(|x| x.id == s.id);
    let starred = app.is_starred(&s.id, s.starred);
    let compact = app.cfg.settings.compact;
    let art = !key.starts_with("album:") && !compact;
    let fg = if cur { ACCENT } else { TEXT };
    let at = Message::PlayAt(key.into(), n);
    let play = if cur { Message::Toggle } else { at.clone() };
    let g = [theme::fill::PLAY, theme::fill::PAUSE][usize::from(cur && app.playing)];
    let num = txt(n + 1, 16.0).color(if cur { ACCENT } else { SUB });
    let idx: El = match (hot, cur && app.playing) {
        (true, _) => btn(theme::filled(g, 16.0).center(), play).width(c.3).into(),
        (false, true) => i(i::EQ, 16.0).color(ACCENT).width(c.3).center().into(),
        _ => num.width(c.3).center().into(),
    };
    let tw = c.0 - if art { 52.0 } else { 0.0 };
    let half = [tw, tw / 2.0 - 8.0][usize::from(compact && !pop)];
    let name = ell(&s.title, half, 16.0, Normal).color(fg);
    let by = (!pop).then(|| artists_line(&s.artists, 14.0, half));
    let title: El = match compact {
        true => row![name.width(half), by]
            .spacing(16)
            .align_y(Center)
            .into(),
        false => column![name].push(by).spacing(2).into(),
    };
    let count = app.plays.get(&s.id).copied().unwrap_or(0).max(s.play_count);
    let plays = || fmt_n(count as usize);
    let in_album = key.starts_with("album:");
    let album = ell(&s.album, c.1, 14.0, Normal);
    let album = link_btn(album, SUB, go("album", &s.album_id));
    let second: El = match (pop, in_album) {
        (true, _) => sub(plays(), 14.0).width(c.1).into(),
        (_, true) => container(sub(plays(), 14.0).width(100).align_x(Right))
            .width(c.1)
            .into(),
        _ => container(album).width(c.1).into(),
    };
    let more = hot.then(|| icon_btn(i::DOTS, 16.0, Message::RowMenu(key.into(), n)));
    let liked = key == "liked";
    let heart = (hot || starred && !liked).then(|| heart_btn(&s.id, starred, 16.0));
    let dur = sub(fmt_dur(s.duration), 14.0).width(56).align_x(Right);
    let thumb = art.then(|| cover(app, &s.cover, 40.0, false));
    let name = row![thumb, title].spacing(12).align_y(Center).width(c.0);
    let added = date([&s.created, &s.starred_at][usize::from(liked)]);
    let added = (c.2 > 0.0).then(|| sub(added, 14.0).width(c.2));
    let cur_rate = app.ratings.get(&s.id).copied().unwrap_or(s.user_rating);
    let stars = app.cfg.settings.ratings.then(|| {
        Row::with_children((1..=5u8).map(|k| {
            let m = Message::Rate(s.id.clone(), if k == cur_rate { 0 } else { k });
            let c = [[Color::TRANSPARENT, SUB], [ACCENT; 2]][usize::from(k <= cur_rate)];
            let b = theme::sq(i(i::STAR, 14.0).center(), 18.0);
            b.style(theme::link(c[usize::from(hot)])).on_press(m).into()
        }))
    });
    let tail = row![container(heart).width(32), dur, container(more).width(32)];
    let r = row![idx, name].push((c.1 > 0.0).then_some(second));
    let count = sub(plays(), 14.0).width(64).align_x(Right);
    let count = (app.cfg.settings.plays && !pop && !in_album).then_some(count);
    let r = r
        .push(added)
        .push(stars)
        .push(count)
        .push(tail.align_y(Center));
    let r = r
        .spacing(16)
        .padding([0, 16])
        .height(app.row_h())
        .align_y(Center);
    let a = [[0.0, 0.1], [0.3; 2]][usize::from(sel)][usize::from(hot)];
    let (k, enter) = (key.to_string(), hk.clone());
    let r = container(r);
    mouse_area(if a > 0.0 {
        r.style(fill(theme::tint(a), 4.0))
    } else {
        r
    })
    .on_press(act(move |a| a.pick(k.clone(), n)))
    .on_double_click(at)
    .on_right_press(Message::RowMenu(key.into(), n))
    .on_enter(act(move |a| a.hover = Some(enter.clone())))
    .on_exit(act(move |a| a.hover = a.hover.take().filter(|h| *h != hk)))
    .into()
}

fn span(top: f32, bottom: f32, row_h: f32, n: usize, extra: usize) -> (usize, usize) {
    let at = |y: f32| (y / row_h).max(0.0) as usize;
    let lo = at(top).saturating_sub(extra).min(n);
    (lo, (at(bottom) + extra + 1).clamp(lo, n))
}

fn gaps<'a>(
    lo: usize,
    hi: usize,
    n: usize,
    h: f32,
    items: impl Iterator<Item = El<'a>>,
) -> Column<'a, Message> {
    let col = Column::new().push(Space::new().height(lo as f32 * h));
    let col = col.extend(items);
    col.push(Space::new().height((n - hi) as f32 * h))
}

fn vlist<'a>(app: &'a App, songs: &'a [Song], key: &'a str, c: Cols, y0: f32) -> El<'a> {
    let discs = key.starts_with("album:") && songs.iter().any(|s| s.disc != songs[0].disc);
    let rows: Vec<Result<usize, u32>> = (0..songs.len())
        .flat_map(|n| {
            let head = discs && (n == 0 || songs[n - 1].disc != songs[n].disc);
            let head = head.then_some(Err(songs[n].disc));
            head.into_iter().chain([Ok(n)])
        })
        .collect();
    let (off, view) = app.scroll();
    let (lo, hi) = span(off - y0, off - y0 + view, app.row_h(), rows.len(), 6);
    let items = rows[lo..hi].iter().map(|r| match *r {
        Ok(n) => track(app, &songs[n], n, key, c),
        Err(d) => {
            let disc = i(i::ALBUM, 16.0).color(SUB).width(16).center();
            let r = row![disc, bold(format!("Disc {d}"), 16.0).color(SUB)].spacing(16);
            r.padding([0, 16])
                .height(app.row_h())
                .align_y(Center)
                .into()
        }
    });
    let col = gaps(lo, hi, rows.len(), app.row_h(), items);
    container(col).padding(Padding::new(PAD).top(0)).into()
}

fn grid<'a>(
    app: &App,
    n: usize,
    cols: usize,
    rh: f32,
    y0: f32,
    f: impl Fn(usize) -> El<'a>,
) -> El<'a> {
    let (rows, (off, view)) = (n.div_ceil(cols), app.scroll());
    let (lo, hi) = span(off - y0, off - y0 + view, rh, rows, 1);
    let line = |r: usize| Row::with_children((r * cols..(r * cols + cols).min(n)).map(&f));
    let col = gaps(lo, hi, rows, rh, (lo..hi).map(|r| line(r).into()));
    let p = Padding::new(12.0).top(0).bottom(PAD);
    container(col).padding(p).into()
}

fn kind(a: &Album) -> (&'static str, &'static str) {
    let t = |k: &str| a.release_types.iter().any(|x| x.eq_ignore_ascii_case(k));
    match () {
        _ if a.compilation || t("compilation") => ("Compilation", "compilation"),
        _ if t("single") => ("Single", "single"),
        _ if t("ep") => ("EP", "single"),
        _ => ("Album", "album"),
    }
}

fn year_kind(a: &Album) -> String {
    match a.year {
        0 => kind(a).0.into(),
        y => format!("{y}{DOT}{}", kind(a).0),
    }
}

pub fn by_date<'a>(v: impl Iterator<Item = &'a Album>) -> Vec<&'a Album> {
    let mut v: Vec<&Album> = v.collect();
    let day = |a: &&Album| a.release_date.clone().max(format!("{:04}", a.year));
    v.sort_by_cached_key(|a| std::cmp::Reverse(day(a)));
    v
}

pub fn disc_shelf<'a>(app: &App, albums: &'a [Album]) -> Vec<&'a Album> {
    let mut v = by_date(albums.iter());
    match app.disc_chip {
        0 if v.len() > 1 => v[1..].sort_by_key(|a| std::cmp::Reverse(a.play_count)),
        0 => {}
        n => v.retain(|a| kind(a).1 == GROUPS[n as usize].0),
    }
    v.truncate(app.grid().0);
    v
}

fn disc_args(app: &App) -> Option<(&str, &str, &Artist, &[Album])> {
    let arg = app.route.strip_prefix("discography:")?;
    let (id, f) = arg.split_once(':').unwrap_or((arg, "all"));
    let (a, albums) = app.data::<(Artist, Vec<Album>)>(&format!("artist:{id}"))?;
    Some((id, f, a, albums))
}

fn disc_rows<'a>(app: &'a App, albums: &'a [Album], f: &str) -> Vec<(f32, f32, &'a Album)> {
    let mut v = by_date(albums.iter().filter(|a| f == "all" || kind(a).1 == f));
    if app.disc_old {
        v.reverse();
    }
    let mut y = GRID_Y;
    let row = |a: &'a Album| {
        let songs = app.data::<(Album, Vec<Song>)>(&format!("album:{}", a.id));
        let n = songs.map_or(0, |x| x.1.len());
        let n = if n > 0 { n } else { a.song_count as usize };
        let h = DISC_HEAD + n as f32 * app.row_h() + 48.0;
        y += h;
        (y - h, h, a)
    };
    v.into_iter().map(row).collect()
}

pub fn disc_view(app: &App) -> Vec<&Album> {
    let Some((_, f, _, albums)) = disc_args(app) else {
        return Vec::new();
    };
    let rows = disc_rows(app, albums, f);
    let (off, view) = app.scroll();
    let (cols, cw) = app.grid();
    let (lo, hi) = span(off - GRID_Y, off + view - GRID_Y, cw + 52.0, rows.len(), 1);
    let (lo, hi) = ((lo * cols).min(rows.len()), (hi * cols).min(rows.len()));
    let near = |r: &&(f32, f32, &Album)| r.0 + r.1 > off - view && r.0 < off + 2.0 * view;
    match app.cfg.settings.disc_grid {
        true => rows[lo..hi].iter().map(|r| r.2).collect(),
        false => rows.iter().filter(near).map(|r| r.2).collect(),
    }
}

fn disc_tools<'a>(app: &'a App, albums: &[Album], id: &str, f: &str) -> El<'a> {
    let grid_on = app.cfg.settings.disc_grid;
    let has = |g: &str| g == "all" || albums.iter().any(|x| kind(x).1 == g);
    let chips = GROUPS.iter().filter(|(g, _)| has(g)).map(|(g, l)| {
        let label = if *g == "all" { "All" } else { l };
        theme::chip_btn(label, f == *g, go("discography", &format!("{id}:{g}")))
    });
    let tool = |c, on: bool| tool_btn(c, on == grid_on, set(|s, b| s.disc_grid = b, on));
    let arrow = i(if app.disc_old { i::UP } else { i::DOWN }, 16.0);
    let order = row![txt("Release date", 14.0), arrow].spacing(4);
    let order = link_btn(
        order.align_y(Center),
        SUB,
        task(|a| {
            a.disc_old = !a.disc_old;
            a.disc_load()
        }),
    );
    let chips = Row::with_children(chips).spacing(8);
    let r = row![chips, Space::new().width(Fill), order, tool(i::LIST, false)];
    let r = r.push(tool(i::GRID, true)).spacing(16);
    r.align_y(Center).into()
}

fn discography(app: &App) -> Option<El<'_>> {
    let (id, f, a, albums) = disc_args(app)?;
    let name = ell(&a.name, app.main_w() - 2.0 * PAD, 32.0, Bold);
    let name = link_btn(name, TEXT, go("artist", id));
    let head = container(column![name, disc_tools(app, albums, id, f)].spacing(16));
    let head = head
        .padding(Padding::new(PAD).top(72).bottom(0))
        .height(GRID_Y);
    let rows = disc_rows(app, albums, f);
    if app.cfg.settings.disc_grid {
        let (cols, cw) = app.grid();
        let card = |n: usize| album_card(app, rows[n].2, year_kind(rows[n].2));
        return Some(column![head, grid(app, rows.len(), cols, cw + 52.0, GRID_Y, card)].into());
    }
    let (c, (off, view)) = (cols(app, false, false, 0), app.scroll());
    let (mut gap, mut col) = (0.0, Column::new().push(head));
    for (y, h, al) in rows {
        if y + h < off - 200.0 || y > off + view + 200.0 {
            gap += h;
        } else {
            col = col.push(Space::new().height(gap));
            col = col.push(release(app, al, y, c));
            gap = 0.0;
        }
    }
    Some(col.push(Space::new().height(gap)).into())
}

fn release<'a>(app: &'a App, al: &'a Album, y: f32, c: Cols) -> El<'a> {
    let key = format!("album:{}", al.id);
    let songs = app.data::<(Album, Vec<Song>)>(&key).map(|x| &x.1[..]);
    let songs = songs.filter(|s| !s.is_empty());
    let n = songs.map_or(al.song_count as usize, <[Song]>::len);
    let ((off, view), top) = (app.scroll(), y + DISC_HEAD);
    let (lo, hi) = span(off - top, off + view - top, app.row_h(), n, 6);
    let year = (al.year > 0).then(|| al.year.to_string());
    let count = Some(plural(al.song_count, "song"));
    let meta = [Some(kind(al).0.to_string()), year, count];
    let meta = meta.into_iter().flatten().collect::<Vec<_>>().join(DOT);
    let name = ell(&al.name, app.main_w() - 3.0 * PAD - 136.0, 32.0, Bold);
    let on = app.is_starred(&al.id, al.starred);
    let open = Message::Nav(key.clone());
    let art = link_btn(cover(app, &al.cover, 136.0, false), TEXT, open.clone());
    let play = play_btn(app, 40.0, &key);
    let menu = dots(MenuKind::Album(Box::new(al.clone())));
    let acts = row![play, heart_btn(&al.id, on, 24.0), menu].spacing(16);
    let name = link_btn(name, TEXT, open);
    let info = column![name, sub(meta, 14.0), Space::new().height(Fill)];
    let acts = acts.align_y(Center);
    let head = row![art, info.push(acts).spacing(8).height(136)].spacing(PAD);
    let rows = (lo..hi).map(|k| -> El<'a> {
        let Some(s) = songs else {
            let r = row![blk(16.0, 14.0, 4.0), blk(240.0, 14.0, 4.0)].spacing(16);
            let r = container(r.align_y(Center)).padding([0.0, PAD + 16.0]);
            return r.center_y(app.row_h()).into();
        };
        let t = track(app, &s[k], k, &key, c);
        container(t).padding([0.0, PAD]).into()
    });
    let head = head.padding([0.0, PAD]).height(136);
    let head = column![head, Space::new().height(16)];
    let list = gaps(lo, hi, n, app.row_h(), rows).push(Space::new().height(48));
    head.push(table_head(app, c)).push(list).into()
}

fn state<'a>(app: &'a App, key: &str) -> Option<El<'a>> {
    if let Some(e) = app.data::<String>(key) {
        let retry = theme::pill_btn(bold("Try again", 14.0), task(App::retry));
        let col = column![bold("Could not load this page.", 24.0), sub(e, 14.0), retry];
        let col = container(col.spacing(16).align_x(Center));
        return Some(col.center_x(Fill).padding(80).into());
    }
    if app.cache.contains_key(key) {
        return None;
    }
    let px = art_px(app);
    let b = |w, h| blk(w, h, 4.0);
    let lines = column![b(80.0, 14.0), b(360.0, 56.0), b(220.0, 14.0)];
    let rows = (0..6).map(|_| {
        let lines = column![b(240.0, 14.0), b(140.0, 12.0)].spacing(8);
        row![b(40.0, 40.0), lines]
            .spacing(12)
            .align_y(Center)
            .into()
    });
    let hero = row![blk(px, px, 6.0), lines.spacing(12)].spacing(PAD);
    let col = column![hero.align_y(iced::Bottom), blk(56.0, 56.0, 28.0)].extend(rows);
    Some(col.spacing(PAD).padding(PAD).width(Fill).into())
}

pub fn route(app: &App) -> El<'_> {
    let r = app.route.as_str();
    let (kind, arg) = r.split_once(':').unwrap_or((r, ""));
    let plain = matches!(kind, "settings" | "lyrics" | "search" | "genres" | "login");
    if (!plain || (kind == "search" && !arg.is_empty()))
        && let Some(s) = state(app, &app.key_of(r))
    {
        return s;
    }
    let missing = || container(sub("This page does not exist.", 14.0)).padding(40);
    let page = match kind {
        "home" => home(app),
        "album" | "playlist" | "liked" | "songs" => detail(app),
        "artist" => artist(app),
        "discography" => discography(app),
        "search" | "genres" => search(app, arg),
        "albums" | "genre" | "artists" => Some(library(app, kind, arg)),
        "folders" | "folder" | "radio" => Some(folders(app)),
        "lyrics" => Some(lyrics(app)),
        "settings" => Some(settings(app)),
        "profile" => profile(app),
        _ => Some(missing().into()),
    };
    page.unwrap_or_else(|| Space::new().into())
}

pub fn home_tiles(app: &App) -> (usize, f32, f32, f32) {
    let w = app.main_w() - 80.0;
    let n = if w >= 720.0 { 4 } else { 2 };
    let fit = |gap: f32| (w - gap * (n - 1) as f32) / n as f32;
    let th = match fit(12.0) {
        t if t >= 400.0 => 80.0,
        t if t >= 280.0 => 64.0,
        _ => 48.0,
    };
    let gap = [8.0, 12.0][usize::from(th > 48.0)];
    (n, fit(gap), th, gap)
}

fn home<'a>(app: &'a App) -> Option<El<'a>> {
    let h = app.data::<Home>("home")?;
    let pls = app.data::<Lib>("lib").map_or(&[][..], |l| &l.playlists[..]);
    let (n, tw, th, gap) = home_tiles(app);
    let big = th > 48.0;
    let tile = |art: El<'a>, name: &'a str, route: String, cid: &str| -> El<'a> {
        let name = ell(name, tw - th - 56.0, [14.0, 16.0][usize::from(big)], Bold);
        let body = row![art, name].spacing([8, 16][usize::from(big)]);
        let style = theme::btn([0.1, 0.2, 0.3].map(theme::tint), [TEXT; 3], 4.0);
        let b = button(body.align_y(Center)).padding(0).width(tw).height(th);
        let b = b.clip(true).style(style);
        let b = b.on_press(Message::Nav(route.clone()));
        let play = play_btn(app, 32.0, &route);
        let play = container(play).align_right(Fill).center_y(th);
        let play = play.padding([0, 8]);
        let (on, off) = (format!("tile:{cid}"), format!("tile:{cid}"));
        mouse_area(play_over(app, b.into(), play.into(), &route))
            .on_enter(act(move |a| a.hover = Some(on.clone())))
            .on_exit(act(move |a| a.hover = a.hover.take().filter(|h| *h != off)))
            .into()
    };
    let pins = pls.iter().take(3);
    let pins = pins.map(|p| (&p.cover, &p.name, format!("playlist:{}", p.id)));
    let recent = h[0].iter();
    let recent = recent.map(|a| (&a.cover, &a.name, format!("album:{}", a.id)));
    let small = |c| cover(app, c, th, false);
    let rest = pins.chain(recent).map(|(c, n, r)| tile(small(c), n, r, c));
    let liked = tile(liked_art(th), "Liked Songs", "liked".into(), "");
    let mut tiles = std::iter::once(liked).chain(rest).take(8);
    let rows = std::iter::from_fn(|| {
        let r: Vec<El> = tiles.by_ref().take(n).collect();
        (!r.is_empty()).then(|| Row::with_children(r).spacing(gap).into())
    });
    let k = app.grid().0;
    let shelves = [
        ("Recently played", "recent"),
        ("Recently added", "newest"),
        ("Most played", "frequent"),
        ("Random picks", "random"),
    ];
    let shelves = shelves.into_iter().zip(h).map(|((t, s), l)| {
        let cards = l
            .iter()
            .take(k)
            .map(|a| album_card(app, a, a.artist.clone()));
        shelf(t, Some(format!("albums:{s}")), cards)
    });
    let c = match app.hover.as_deref().and_then(|h| h.strip_prefix("tile:")) {
        Some("") => LIKED,
        Some(id) => head_color(app, id),
        None => h[0].first().map_or(GREY, |a| head_color(app, &a.cover)),
    };
    let pls = pls.iter().take(k).map(|p| playlist_card(app, p));
    let pls = shelf("Your playlists", None, pls);
    let tiles = container(Column::from_vec(rows.collect()).spacing(gap)).padding([0.0, PAD]);
    let col = column![tiles].extend(shelves).push(pls).spacing(40);
    Some(fade(theme::dark(c, 0.08), 256.0, col.padding([PAD, 16.0])))
}

struct Page<'a> {
    art: El<'a>,
    kind: &'static str,
    title: &'a str,
    meta: El<'a>,
    color: Color,
    extra: Vec<El<'a>>,
    banner: Option<String>,
}

fn page<'a>(app: &App, p: Page<'a>, body: El<'a>) -> El<'a> {
    let px = art_px(app);
    let title = |tw: f32| huge(p.title, tw);
    let low = theme::dark(p.color, 0.2);
    let bg = theme::down(&[(0.0, p.color), (1.0, low)]);
    let hero: El = if let Some(key) = &p.banner {
        let bh = banner_h(app);
        let img = app.covers.get(key, 640);
        let img = img.map(|h| image(h).width(Fill).height(bh));
        let img = img.map(|i| i.content_fit(iced::ContentFit::Fill));
        let a = |x| BG.scale_alpha(x);
        let dim = container(Space::new()).width(Fill).height(bh);
        let dim = dim.style(theme::down(&[(0.0, CLEAR), (0.5, a(0.1)), (1.0, a(0.6))]));
        let text = column![title(app.main_w() - 2.0 * PAD), p.meta].spacing(8);
        let text = container(text).padding(PAD);
        let text = text.width(Fill).height(bh).align_bottom(bh);
        let back = container(Space::new()).width(Fill).height(bh).style(bg);
        stack![back].push(img).push(dim).push(text).into()
    } else {
        let id = app.route.strip_prefix("playlist:");
        let edit = id.filter(|id| app.playlist(id).is_some_and(|p| app.owns(p)));
        let edit = edit.map(|id| Message::Modal(id.into(), 1));
        let click = |e: El<'a>| -> El<'a> {
            let hand = iced::mouse::Interaction::Pointer;
            match edit.clone() {
                Some(m) => mouse_area(e).on_press(m).interaction(hand).into(),
                None => e,
            }
        };
        let kind = (!p.kind.is_empty()).then(|| txt(p.kind, 14.0).font(theme::fw(Medium)));
        let name = click(title(app.main_w() - 3.0 * PAD - px).into());
        let info = column![kind, name, p.meta].spacing(8);
        let hero = row![click(p.art), info].spacing(PAD).align_y(iced::Bottom);
        let hero = container(hero)
            .padding(Padding::new(PAD).top(64.0))
            .width(Fill);
        hero.height(hero_h(app)).style(bg).into()
    };
    let shuffle = bar_btn(i::SHUFFLE, Message::PlayCtx(app.route.clone(), 1));
    let play = play_btn(app, 56.0, &app.route);
    let compact = app.cfg.settings.compact;
    let tables = ["album", "playlist", "liked", "songs"];
    let view = tables
        .contains(&app.route.split(':').next().unwrap_or(""))
        .then(|| {
            let label = match app.route.starts_with("playlist:") {
                true => ORDER[usize::from(app.pl_sort & 15)],
                false => ["List", "Compact"][usize::from(compact)],
            };
            let c = row![
                txt(label, 14.0),
                i([i::LIST, i::COMPACT][usize::from(compact)], 16.0)
            ];
            link_btn(
                c.spacing(8).align_y(Center),
                SUB,
                Message::Menu(Box::new(MenuKind::View)),
            )
        });
    let empty = app.route == "profile"
        || app
            .data::<(Playlist, Vec<Song>)>(&app.route)
            .is_some_and(|x| x.1.is_empty())
        || app
            .data::<(Vec<Song>, usize)>(&app.route)
            .is_some_and(|x| x.1 == 0);
    let bar = Row::new().push((!empty).then_some(play));
    let bar = bar.push((!empty).then_some(shuffle));
    let bar = bar.extend(p.extra).push(view).spacing(PAD);
    let bar = container(bar.align_y(Center)).height(BAR);
    let bar = bar.padding(Padding::new(PAD).top(12).bottom(16));
    let top = Color::from_rgb(
        low.r * 0.7 + 0.3 * theme::PANEL.r,
        low.g * 0.7 + 0.3 * theme::PANEL.g,
        low.b * 0.7 + 0.3 * theme::PANEL.b,
    );
    let rest = fade(top, 173.0, column![bar, body]);
    column![hero, rest].into()
}

fn initial(name: &str) -> (char, Color) {
    let first = name.chars().next().map_or('?', |c| c.to_ascii_uppercase());
    (first, theme::hue(first as usize))
}

pub fn avatar<'a>(name: &str, px: f32) -> iced::widget::Container<'a, Message> {
    let (first, color) = initial(name);
    let c = container(bold(first, px * 0.42).color(BG).center()).center(px);
    c.style(fill(color, px / 2.0))
}

fn meta<'a>(lead: impl Into<El<'a>>, rest: String) -> El<'a> {
    row![lead.into(), sub(rest, 14.0)].align_y(Center).into()
}

fn detail_of(app: &App) -> Option<(Page<'_>, &[Song], Cols)> {
    let key = app.route.as_str();
    let px = art_px(app);
    if let Some((a, songs)) = app.data::<(Album, Vec<Song>)>(key) {
        let names = a.artists.iter().map(|x| (x.id.as_str(), x.name.as_str()));
        let mut names: Vec<_> = names.collect();
        if names.is_empty() {
            names.push((&a.artist_id, &a.artist));
        }
        let face = a.artists.first().map_or(&a.artist_id, |x| &x.cover);
        let links = names.iter().enumerate().map(|(n, (id, name))| {
            let name = format!("{}{name}", if n > 0 { ", " } else { "" });
            link_btn(bold(name, 14.0), TEXT, go("artist", id)).into()
        });
        let face = cover(app, face, 24.0, true);
        let lead = row![face, Row::with_children(links)].spacing(8);
        let year = (a.year > 0).then(|| format!("{DOT}{}", a.year));
        let (year, len) = (year.unwrap_or_default(), fmt_long(a.duration));
        let rest = format!("{year}{DOT}{}, {len}", plural(a.song_count, "song"));
        let heart = heart_btn(&a.id, app.is_starred(&a.id, a.starred), 32.0);
        let p = Page {
            art: cover(app, &a.cover, px, false),
            kind: kind(a).0,
            title: &a.name,
            meta: meta(lead.align_y(Center), rest),
            color: head_color(app, &a.cover),
            extra: vec![
                heart,
                dots(MenuKind::Album(Box::new(a.clone()))),
                Space::new().width(Fill).into(),
            ],
            banner: None,
        };
        let c = cols(app, true, false, songs.len());
        let w = c.0 + c.1 + 80.0 * f32::from(app.cfg.settings.plays);
        let fr = (app.main_w() - 2.0 * PAD - 80.0 - c.3) / 7.0;
        let c = match c.1 > 0.0 {
            true => (4.0 * fr, w - 4.0 * fr, c.2, c.3),
            false => c,
        };
        return Some((p, songs, c));
    }
    if let Some((p, songs)) = app.data::<(Playlist, Vec<Song>)>(key) {
        let len = fmt_long(p.duration);
        let rest = match p.song_count {
            0 => String::new(),
            n => format!("{DOT}{}, {len}", plural(n, "song")),
        };
        let find: El = match &app.pl_find {
            None => {
                let open = task(|a| {
                    a.pl_find = Some(String::new());
                    iced::widget::operation::focus("plfind")
                });
                tool_btn(i::SEARCH, false, open).into()
            }
            Some(q) => {
                let style = theme::input([ELEVATED; 2], [BG; 2], [0.0; 2], 4.0);
                let t = text_input("Search in playlist", q)
                    .id("plfind")
                    .width(220)
                    .size(14);
                let t = t.on_input(|v| act(move |a| a.pl_find = Some(v.clone())));
                let t = t.on_submit(Message::Noop);
                let glass = text_input::Icon {
                    font: theme::LUCIDE,
                    code_point: i::SEARCH,
                    size: Some(16.0.into()),
                    spacing: 8.0,
                    side: text_input::Side::Left,
                };
                let t = t.icon(glass);
                t.style(style)
                    .font(theme::fw(Normal))
                    .padding([8, 12])
                    .into()
            }
        };
        let some = !songs.is_empty();
        let comment = (!p.comment.is_empty()).then(|| sub(&p.comment, 14.0));
        let owner = row![avatar(&p.owner, 24.0), bold(&p.owner, 14.0)].spacing(8);
        let meta = column![comment, meta(owner.align_y(Center), rest)];
        let page = Page {
            art: cover(app, &p.cover, px, false),
            kind: ["Private Playlist", "Public Playlist"][usize::from(p.public)],
            title: &p.name,
            meta: meta.spacing(8).into(),
            color: head_color(app, &p.cover),
            extra: vec![
                dots(MenuKind::Playlist(p.clone())),
                Space::new().width(Fill).into(),
                some.then_some(find).into(),
            ],
            banner: None,
        };
        let songs = app.songs_of(key).map_or(&songs[..], |x| x.0);
        return Some((page, songs, cols(app, true, true, songs.len())));
    }
    let liked = key == "liked";
    let (items, total) = app
        .data::<(Vec<Song>, usize)>(key)
        .filter(|_| liked || key.split(':').next() == Some("songs"))?;
    let server = app.session.as_ref().map(|s| &s.server);
    let (user, native) = server.map_or(("", false), |s| (s.user.as_str(), s.native));
    let n = (*total).max(items.len());
    let count = (liked || native).then(|| format!("{DOT}{} songs", fmt_n(n)));
    const SORTS: [(&str, &str); 8] = [
        ("name", "Title"),
        ("artist", "Artist"),
        ("album", "Album"),
        ("newest", "Recently added"),
        ("recent", "Recently played"),
        ("frequent", "Most played"),
        ("year", "Year"),
        ("rating", "Rating"),
    ];
    let cur = key.strip_prefix("songs:").unwrap_or("name");
    let on = SORTS.iter().find(|(k, _)| *k == cur).map(|(_, l)| *l);
    let to = |l: &str| SORTS.iter().find(|(_, x)| *x == l).expect("a sort label").0;
    let sort = pick_list(SORTS.map(|(_, l)| l), on, move |l| go("songs", to(l)));
    let sort = sort.text_size(14).font(theme::fw(Normal)).padding([8, 12]);
    let sort = sort.style(theme::pick(true)).menu_style(theme::pick_menu);
    let sort = (!liked && native).then_some(sort);
    let heads = [
        ("Library", "Songs", color!(0x1e6b52)),
        ("Playlist", "Liked Songs", LIKED),
    ];
    let (kind, title, color) = heads[usize::from(liked)];
    let art = match liked {
        true => liked_art(px),
        false => art(px, i(i::MUSIC, 0.0).color(ACCENT), SONGS_BG),
    };
    let owner = row![avatar(user, 24.0), bold(user, 14.0)].spacing(8);
    let meta = meta(owner.align_y(Center), count.unwrap_or_default());
    let mut extra = vec![Space::new().width(Fill).into()];
    extra.extend(sort.map(Into::into));
    let page = Page {
        art,
        kind,
        title,
        meta,
        color,
        extra,
        banner: None,
    };
    Some((page, items, cols(app, true, true, n)))
}

fn finder<'a>(app: &'a App, p: &'a Playlist, empty: bool) -> El<'a> {
    if !empty {
        let m = (app.panel != 4).then_some(Message::Panel(4));
        let open = button(bold("Find more", 14.0))
            .padding(0)
            .style(theme::link(TEXT));
        return container(open.on_press_maybe(m))
            .padding([16.0, PAD])
            .into();
    }
    let q = app.pl_more.as_deref().unwrap_or("");
    let head = bold("Find songs for your playlist", 24.0);
    let style = theme::input([ELEVATED; 2], [ELEVATED; 2], [0.0; 2], 4.0);
    let field = find_input(q, "plmore").width(360).size(14).style(style);
    let q = q.trim();
    let hits = app
        .data::<Trio>(&format!("search:{q}"))
        .filter(|_| !q.is_empty());
    let w = (app.main_w() - 2.0 * PAD - 200.0) / 2.0;
    let rows = hits.into_iter().flat_map(|t| t.2.iter().take(10)).map(|s| {
        let art = cover(app, &s.cover, 40.0, false);
        let add = theme::pill_btn(
            bold("Add", 14.0),
            Message::PlAdd(p.id.clone(), vec![s.id.clone()]),
        );
        let album = container(theme::ell(&s.album, w - 16.0, 14.0, Normal).color(SUB)).width(w);
        let r = row![
            media(art, &s.title, &s.artist, w - 52.0, TEXT),
            album,
            Space::new().width(Fill),
            add
        ];
        container(r.spacing(12).align_y(Center))
            .padding([4.0, 0.0])
            .into()
    });
    let col = column![head, field.padding([10, 12])].spacing(16);
    let col = col.push(Column::with_children(rows));
    container(col.spacing(16)).padding([16.0, PAD]).into()
}

pub fn more_by<'a>(app: &'a App, a: &Album) -> Vec<&'a Album> {
    let key = format!("artist:{}", a.artist_id);
    let all = app
        .data::<(Artist, Vec<Album>)>(&key)
        .map_or(&[][..], |x| &x.1[..]);
    let mut v = by_date(all.iter().filter(|x| x.id != a.id));
    v.truncate(app.grid().0);
    v
}

fn detail(app: &App) -> Option<El<'_>> {
    let (p, songs, c) = detail_of(app)?;
    let y0 = list_y(app);
    let pl = own_pl(app);
    let empty = pl.is_some_and(|x| x.1.is_empty());
    let liked = app.route == "liked";
    let tools = pl.filter(|_| !empty).map(|(p, _)| {
        let pill = |c, label, on: bool, m| {
            let r = row![i(c, 16.0), bold(label, 14.0)]
                .spacing(8)
                .align_y(Center);
            let bg = [
                [theme::tint(0.1), theme::tint(0.2), theme::tint(0.2)],
                [TEXT; 3],
            ];
            let b = button(r).padding(Padding::new(7.0).left(12).right(16));
            b.style(theme::solid(
                bg[usize::from(on)],
                [TEXT, BG][usize::from(on)],
            ))
            .on_press(m)
        };
        let add = pill(i::PLUS, "Add", app.panel == 4, Message::Panel(4));
        let edit = pill(
            i::PENCIL,
            "Name & details",
            false,
            Message::Modal(p.id.clone(), 1),
        );
        container(row![add, edit].spacing(8)).padding(Padding::new(PAD).top(14).bottom(7))
    });
    let head = (!empty && !(liked && songs.is_empty())).then(|| table_head(app, c));
    let body = column![tools, head];
    let body = body.push(vlist(app, songs, &app.route, c, y0));
    let none: El = match liked {
        true => container(blank(
            None,
            bold("Songs that you like show here", 32.0),
            txt("To save a song, click the plus icon.", 16.0),
            "Find songs",
        ))
        .center_x(Fill)
        .padding(64)
        .into(),
        false => container(sub("There are no songs to show.", 16.0))
            .padding(PAD)
            .into(),
    };
    let body = body.push((songs.is_empty() && !empty).then_some(none));
    let body = body.push(pl.map(|(p, _)| finder(app, p, empty)));
    let foot = app.data::<(Album, Vec<Song>)>(&app.route).map(|(a, _)| {
        let day = Some(date(&a.release_date)).filter(|d| !d.is_empty());
        let day = day.or((a.year > 0).then(|| a.year.to_string()));
        let label =
            (!a.labels.is_empty()).then(|| format!("\u{2117} {} {}", a.year, a.labels.join(", ")));
        let notes = column![day.map(|d| sub(d, 14.0)), label.map(|l| sub(l, 11.0))].spacing(4);
        let more = more_by(app, a)
            .into_iter()
            .map(|x| album_card(app, x, year_kind(x)));
        let all = Some(format!("discography:{}", a.artist_id));
        let more = shelf(format!("More by {}", a.artist), all, more);
        column![container(notes).padding([0.0, PAD]), more]
            .spacing(48)
            .padding(padding::bottom(PAD))
    });
    Some(page(app, p, body.push(foot).into()))
}

fn blank<'a>(
    icon: Option<char>,
    title: text::Text<'a>,
    hint: text::Text<'a>,
    find: &'a str,
) -> El<'a> {
    let find = button(bold(find, 16.0)).padding([12, 32]);
    let find = find.style(theme::solid([TEXT, color!(0xf0f0f0), color!(0xb7b7b7)], BG));
    let find = find.on_press(Message::Nav("search".into()));
    let head = column![icon.map(|c| i(c, 40.0)), title].spacing(40);
    let col = column![
        head.align_x(Center),
        hint.align_x(Center),
        container(find).padding(padding::top([16, 32][usize::from(icon.is_some())]))
    ];
    col.spacing(8).align_x(Center).into()
}

pub fn sticky<'a>(app: &'a App) -> Option<El<'a>> {
    let off = app.scrolls.get(&app.route).map_or(0.0, |s| s.0);
    let bar = |c: El<'a>, bg: Color| {
        let b = container(c).padding([0.0, PAD]).center_y(64).width(Fill);
        b.style(fill(bg, 0.0))
    };
    if app.route == "lyrics" && app.cfg.settings.lyrics_follow && app.lyric.1.is_none() {
        let t = bold("Show the current line", 14.0);
        let b = button(t)
            .padding([10, 20])
            .on_press(act(|a| a.lyric = (usize::MAX, Some(0.0))));
        let b = b.style(theme::btn([TEXT; 3], [BG; 3], 500.0));
        let b = container(b).center_x(Fill).align_bottom(Fill);
        return Some(b.padding(24).into());
    }
    if let Some((id, f, _, albums)) = disc_args(app) {
        let tools = disc_tools(app, albums, id, f);
        return (off > GRID_Y - 64.0).then(|| bar(tools, theme::PANEL).into());
    }
    if let Some(g) = app.route.strip_prefix("genre:") {
        let t = ell(g, app.main_w() - 2.0 * PAD, 24.0, Bold);
        let bg = theme::shade(genre_color(app, g), 0.5);
        return (off > hero_h(app) - 64.0).then(|| bar(t.into(), bg).into());
    }
    if app.route == "profile" {
        let user = app.session.as_ref().map_or("", |s| s.server.user.as_str());
        let t = ell(user, app.main_w() - 2.0 * PAD, 24.0, Bold);
        let bg = theme::dark(initial(user).1, 0.2);
        return (off > hero_h(app) - 64.0).then(|| bar(t.into(), bg).into());
    }
    let artist = app.data::<(Artist, Vec<Album>)>(&app.route);
    let (title, color, cols, banner) = match artist {
        Some((a, _)) => {
            let b = banner(app, a);
            let color = head_color(app, b.as_deref().unwrap_or(&a.cover));
            (&a.name[..], color, None, b.is_some())
        }
        None => detail_of(app).map(|(p, _, c)| (p.title, p.color, Some(c), false))?,
    };
    let top = [hero_h(app), banner_h(app)][usize::from(banner)];
    let title = ell(title, app.main_w() - 120.0, 24.0, Bold);
    let play = play_btn(app, 48.0, &app.route);
    let head = row![play, title].spacing(8).align_y(Center);
    let head = bar(head.into(), theme::dark(color, 0.2));
    let cols = cols
        .filter(|_| off > list_y(app) - HEAD - 64.0)
        .map(|c| table_head(app, c));
    let cols = cols.map(|c| container(c).style(fill(color!(0x1a1a1a), 0.0)));
    (off > top - 64.0).then(|| column![head].push(cols).into())
}

fn plain(html: &str) -> String {
    let parts = html.split('<').enumerate();
    let parts = parts.map(|(n, p)| [p, p.split_once('>').map_or("", |x| x.1)][usize::from(n > 0)]);
    parts.collect::<String>().trim().to_string()
}

pub fn public(app: &App) -> impl Iterator<Item = &Playlist> {
    let pls = app.data::<Lib>("lib").map_or(&[][..], |l| &l.playlists[..]);
    pls.iter().filter(|p| p.public && app.owns(p))
}

fn profile(app: &App) -> Option<El<'_>> {
    let (artists, songs) = app.data::<(Vec<Artist>, Vec<Song>)>("profile")?;
    let user = app.session.as_ref().map_or("", |s| s.server.user.as_str());
    let k = app.grid().0;
    let head = |t, all| {
        let hint = container(sub("Only visible to you", 14.0)).padding([0.0, PAD]);
        column![heading(t, all), hint].spacing(4)
    };
    let cards = artists.iter().take(k).map(|a| artist_card(app, a));
    let cards = container(Row::with_children(cards)).padding([0, 12]);
    let top = (!artists.is_empty()).then(|| column![head("Top artists", None), cards].spacing(8));
    let c = cols(app, true, false, songs.len());
    let rows = songs.iter().enumerate();
    let rows = rows.map(|(n, s)| container(track(app, s, n, "profile", c)).padding([0.0, PAD]));
    let all = Some("songs:frequent".to_string());
    let tracks =
        (!songs.is_empty()).then(|| column![head("Top tracks", all)].extend(rows.map(Into::into)));
    let pls = public(app).collect::<Vec<_>>();
    let shelf = shelf(
        "Public Playlists",
        None,
        pls.iter().take(k).map(|p| playlist_card(app, p)),
    );
    let body = column![top, tracks.map(|t| t.spacing(8)), shelf].spacing(40);
    let p = Page {
        art: avatar(user, art_px(app)).into(),
        kind: "Profile",
        title: user,
        meta: sub(plural(pls.len() as u32, "Public Playlist"), 14.0)
            .color(TEXT)
            .into(),
        color: initial(user).1,
        extra: Vec::new(),
        banner: None,
    };
    Some(page(
        app,
        p,
        body.padding(padding::bottom(PAD * 2.0)).into(),
    ))
}

fn artist(app: &App) -> Option<El<'_>> {
    let key = app.route.as_str();
    let (a, albums) = app.data::<(Artist, Vec<Album>)>(key)?;
    let id = &key["artist:".len()..];
    let top = app.data::<Vec<Song>>(&format!("top:{id}"));
    let top = top.map_or(&[][..], |v| &v[..]);
    let info = app.data::<Info>(&format!("info:{id}"));
    let counts = [(a.album_count, "album"), (a.song_count, "song")];
    let counts = counts.into_iter().filter(|x| x.0 > 0);
    let counts = counts.map(|(n, w)| plural(n, w));
    let counts = counts.collect::<Vec<_>>().join(DOT);
    let on = app.is_starred(&a.id, a.starred);
    let all = app.popular_all;
    let n = if all { 10 } else { 5 }.min(top.len());
    let newest = by_date(albums.iter()).first().copied();
    let newest = newest.filter(|_| app.main_w() >= 1000.0 && !top.is_empty());
    let cut = (app.main_w() - 2.0 * PAD) * [0.0, 0.4][usize::from(newest.is_some())];
    let c = cols(app, false, false, n);
    let c3 = (
        c.0 + 80.0 * f32::from(app.cfg.settings.plays) - cut - 136.0,
        120.0,
        0.0,
        c.3,
    );
    let more = (top.len() > 5).then(|| {
        let label = bold(if all { "Show less" } else { "See more" }, 14.0);
        let b = button(label).padding(16).style(theme::link(SUB));
        let b = b.on_press(act(|a| a.popular_all = !a.popular_all));
        container(b).padding([0.0, PAD])
    });
    let rows = top.iter().take(n).enumerate();
    let rows = rows.map(|(k, s)| track(app, s, k, key, c3));
    let rows = rows.map(|t| container(t).padding([0.0, PAD]).into());
    let popular = column![heading("Popular", None)].extend(rows).push(more);
    let k = app.grid().0;
    let has = |g: &str| albums.iter().any(|x| kind(x).1 == g);
    let chips = GROUPS.iter().enumerate();
    let chips = chips.filter(|(n, (g, _))| *n == 0 || has(g));
    let chips = chips.map(|(n, (_, l))| {
        let m = task(move |a| {
            a.disc_chip = n as u8;
            a.covers_now()
        });
        theme::chip_btn(l, app.disc_chip as usize == n, m)
    });
    let latest = |n: usize, x: &Album| match (n, app.disc_chip) {
        (0, 0) => format!("Latest Release{DOT}{}", kind(x).0),
        _ => year_kind(x),
    };
    let shelf_cards = disc_shelf(app, albums).into_iter().enumerate();
    let shelf_cards = shelf_cards.map(|(n, x)| album_card(app, x, latest(n, x)));
    let all = format!("discography:{id}:{}", GROUPS[app.disc_chip as usize].0);
    let chips = container(Row::with_children(chips).spacing(8)).padding([0.0, PAD]);
    let discog = column![heading("Discography", Some(all)), chips];
    let discog = discog.push(container(Row::with_children(shelf_cards)).padding([0, 12]));
    let similar = info.map_or(&[][..], |i| &i.similar[..]).iter().take(k);
    let similar = shelf(
        "Similar artists",
        None,
        similar.map(|x| artist_card(app, x)),
    );
    let appears = app.data::<Vec<Album>>(&format!("appears:{id}"));
    let appears = appears.into_iter().flatten().take(k);
    let appears = appears.map(|x| album_card(app, x, year_kind(x)));
    let about = info.filter(|i| !i.biography.is_empty()).map(|info| {
        let bio = txt(plain(&info.biography), 16.0).color(SUB);
        let bio = container(bio).padding(PAD).max_width(672);
        let bio = container(bio.style(fill(ELEVATED, 8.0))).padding([0.0, PAD]);
        column![heading("About", None), bio].spacing(8)
    });
    let pick = newest.map(|x| {
        let px = app.grid().1 - 24.0;
        let tw = cut - PAD - px - 40.0;
        let meta = column![ell(&x.name, tw, 16.0, Bold), sub(year_kind(x), 14.0)].spacing(4);
        let body = row![cover(app, &x.cover, px, false), meta]
            .spacing(16)
            .align_y(Center);
        let b = button(body)
            .padding(12)
            .width(Fill)
            .style(theme::track(0.0));
        let b = container(b.on_press(go("album", &x.id))).padding([0.0, PAD - 12.0]);
        column![heading("Latest release", None), b]
            .spacing(8)
            .width(Fill)
    });
    let popular = (!top.is_empty()).then(|| {
        let left = popular.spacing(8).width(app.main_w() - cut);
        row![left].push(pick)
    });
    let discog = (!albums.is_empty()).then(|| discog.spacing(12));
    let body = column![popular, discog, similar, shelf("Appears On", None, appears)];
    let body = body.push(about).spacing(40);
    let body = body.padding(padding::bottom(PAD * 2.0));
    let banner = banner(app, a);
    let follow = if on { "Following" } else { "Follow" };
    let follow = theme::pill_btn(bold(follow, 14.0), Message::Star(vec![a.id.clone()], !on));
    let p = Page {
        art: cover(app, &a.cover, art_px(app), true),
        kind: "",
        title: &a.name,
        meta: sub(counts, 14.0).color(TEXT).into(),
        color: head_color(app, banner.as_deref().unwrap_or(&a.cover)),
        extra: vec![follow.into(), dots(MenuKind::Artist(a.clone()))],
        banner,
    };
    Some(page(app, p, body.into()))
}

pub fn browse(app: &App) -> (f32, usize, f32, f32) {
    let w = (app.main_w() - 24.0).min(1176.0);
    let cols = (w / 280.0).max(2.0) as usize;
    let tw = w / cols as f32 - 24.0;
    (w, cols, tw, (tw * 0.5625).round())
}

pub fn genre_cover<'a>(app: &'a App, name: &str) -> Option<&'a String> {
    let home = app.data::<Home>("home").into_iter().flatten();
    let lib = app.data::<Lib>("lib").into_iter().map(|l| &l.albums);
    home.chain(lib)
        .flatten()
        .find(|a| a.genre.eq_ignore_ascii_case(name))
        .map(|a| &a.cover)
}

fn search<'a>(app: &'a App, q: &str) -> Option<El<'a>> {
    if q.is_empty() {
        let gs = app.data::<Vec<Genre>>("genres")?;
        let (w, cols, tw, th) = browse(app);
        let tile = |n: usize| -> El<'a> {
            let g = &gs[n];
            let label = bold(&g.name, 24.0).wrapping(text::Wrapping::WordOrGlyph);
            let label = container(label).width(tw - 97.0);
            let b = button(label).padding(16).width(tw).height(th);
            let b = b.style(theme::btn([theme::hue(n); 3], [TEXT; 3], 8.0));
            let b = b.on_press(go("genre", &g.name));
            let s = (tw * 0.45).round();
            let art = genre_cover(app, &g.name)
                .and_then(|c| app.covers.get(c, app.bucket(s)))
                .map(|h| {
                    let img = image(h)
                        .width(tw * 0.3)
                        .height(tw * 0.3)
                        .scale(1.5_f32)
                        .border_radius(4);
                    let img = img.rotation(iced::Radians(25f32.to_radians()));
                    let p = Padding::new(0.0).left(tw * 0.7);
                    container(img).padding(p.top(th * 0.64 - tw * 0.15))
                });
            let edge = art.is_some().then(|| {
                let border = iced::Border::default()
                    .color(theme::PANEL)
                    .width(4)
                    .rounded(12);
                let f = container(Space::new()).width(tw + 8.0).height(th + 8.0);
                container(f.style(move |_| container::Style::default().border(border))).padding(8)
            });
            let tile = container(stack![b].push(art).clip(true)).padding(12);
            stack![tile].push(edge).into()
        };
        let title = container(bold("Browse all", 24.0));
        let title = title.padding(Padding::new(PAD).top(67).bottom(8));
        let col = column![title, grid(app, gs.len(), cols, th + 24.0, 104.0, tile)];
        return Some(container(col.width(w + 24.0)).center_x(Fill).into());
    }
    let (artists_res, albums_res, songs_res) = app.data::<Trio>(&app.route)?;
    if artists_res.is_empty() && albums_res.is_empty() && songs_res.is_empty() {
        let title = bold(format!("No results found for \u{201c}{q}\u{201d}"), 24.0);
        let hint = "Make sure that the spelling is correct. Use fewer words or different words.";
        let hint = sub(hint, 16.0);
        let col = column![title, hint].spacing(16).align_x(Center);
        return Some(container(col).center_x(Fill).padding(80).into());
    }
    let tab = app.search_tab;
    let chips = ["All", "Songs", "Albums", "Artists", "Playlists"]
        .into_iter()
        .enumerate();
    let chips = chips.map(|(n, l)| theme::chip_btn(l, tab == n, act(move |a| a.search_tab = n)));
    let x0 = ((app.main_w() - 1152.0) / 2.0).max(PAD);
    let chips = Row::with_children(chips).spacing(8);
    let chips = container(chips).padding(Padding::new(16.0).left(x0).right(PAD));
    let (k, cw) = app.grid();
    let tiles = |n, f: &dyn Fn(usize) -> El<'a>| grid(app, n, k, cw + 52.0, 64.0, f);
    let by = |a: &Album| match a.year {
        0 => a.artist.clone(),
        y => format!("{y}{DOT}{}", a.artist),
    };
    let ql = q.to_lowercase();
    let pls = app.data::<Lib>("lib").map_or(&[][..], |l| &l.playlists[..]);
    let pls: Vec<&Playlist> = pls
        .iter()
        .filter(|p| p.name.to_lowercase().contains(&ql))
        .collect();
    let body: El = match tab {
        1 => {
            let c = cols(app, true, false, songs_res.len());
            let rows = songs_res.iter().enumerate();
            let rows = rows.map(|(n, s)| track(app, s, n, &app.route, c));
            let rows = container(Column::with_children(rows)).padding([0.0, PAD]);
            column![table_head(app, c), rows].into()
        }
        2 => tiles(albums_res.len(), &|n| {
            album_card(app, &albums_res[n], by(&albums_res[n]))
        }),
        3 => tiles(artists_res.len(), &|n| artist_card(app, &artists_res[n])),
        4 => tiles(pls.len(), &|n| playlist_card(app, pls[n])),
        _ => {
            let (top, mix) = search_hits((artists_res, albums_res, songs_res), q);
            let rw = (app.main_w() - 2.0 * PAD).min(1152.0);
            let pick = |(k, i): Hit| match k {
                0 => {
                    let a = &artists_res[i];
                    let play = Message::PlayCtx(format!("artist:{}", a.id), 0);
                    let menu = Message::Menu(Box::new(MenuKind::Artist(a.clone())));
                    (
                        &a.cover,
                        &a.name,
                        "",
                        &a.id,
                        a.starred,
                        go("artist", &a.id),
                        play,
                        menu,
                    )
                }
                1 => {
                    let a = &albums_res[i];
                    let play = Message::PlayCtx(format!("album:{}", a.id), 0);
                    let menu = Message::Menu(Box::new(MenuKind::Album(Box::new(a.clone()))));
                    (
                        &a.cover,
                        &a.name,
                        &a.artist[..],
                        &a.id,
                        a.starred,
                        go("album", &a.id),
                        play,
                        menu,
                    )
                }
                _ => {
                    let s = &songs_res[i];
                    let (play, menu) = (
                        Message::PlayAt(app.route.clone(), i),
                        Message::RowMenu(app.route.clone(), i),
                    );
                    (
                        &s.cover,
                        &s.title,
                        &s.artist[..],
                        &s.id,
                        s.starred,
                        go("album", &s.album_id),
                        play,
                        menu,
                    )
                }
            };
            let save = |k: usize, id: &str, on: bool| -> El<'a> {
                let on = app.is_starred(id, on);
                match k {
                    0 => theme::pill_btn(
                        bold(["Follow", "Following"][usize::from(on)], 14.0),
                        Message::Star(vec![id.into()], !on),
                    )
                    .into(),
                    _ => heart_btn(id, on, 16.0),
                }
            };
            let kinds = ["Artist", "Album", "Song"];
            let top = top.map(|h| {
                let (cid, name, artist, id, on, nav, play, _) = pick(h);
                let line = match h.0 {
                    0 => kinds[0].to_string(),
                    k => format!("{}{DOT}{artist}", kinds[k]),
                };
                let tw = rw - 32.0 - 72.0 - 24.0 - 3.0 * 16.0 - 100.0 - 48.0;
                let text = column![ell(name, tw, 24.0, Bold), sub(line, 14.0)].spacing(2);
                let green = theme::filled(theme::fill::PLAY, 19.0).color(BG).center();
                let green = theme::sq(green, 48.0).style(theme::accent).on_press(play);
                let r = row![cover(app, cid, 72.0, h.0 == 0), text.width(Fill)].spacing(24);
                let r = r.push(row![save(h.0, id, on), green].spacing(16).align_y(Center));
                let style = theme::btn(
                    [theme::tint(0.1), theme::tint(0.2), theme::tint(0.1)],
                    [TEXT; 3],
                    6.0,
                );
                button(r.align_y(Center))
                    .padding(16)
                    .style(style)
                    .on_press(nav)
            });
            let tw = (rw - 28.0 - 120.0 - 32.0) * 6.0 / 7.0 - 126.0;
            let rows = mix.into_iter().enumerate().map(|(n, h)| -> El<'a> {
                let (cid, name, artist, id, on, nav, play, menu) = pick(h);
                let hk = format!("{}#m{n}", app.route);
                let hot = app.hover.as_deref() == Some(hk.as_str());
                let round = h.0 == 0;
                let bg = app
                    .covers
                    .color(cid)
                    .filter(|_| !round)
                    .unwrap_or(Color::TRANSPARENT);
                let thumb = container(cover(app, cid, 64.0, round))
                    .center_x(114)
                    .center_y(64);
                let thumb = thumb.style(fill(bg, 4.0));
                let shade = Color { a: 0.5, ..BG };
                let over = btn(
                    theme::filled(theme::fill::PLAY, 24.0).center(),
                    play.clone(),
                );
                let over = container(over.width(64).height(64))
                    .center_x(114)
                    .center_y(64);
                let over = container(over).style(fill(shade, if round { 32.0 } else { 4.0 }));
                let text = column![ell(name, tw, 16.0, Normal)];
                let text = text
                    .push((!artist.is_empty()).then(|| ell(artist, tw, 14.0, Normal).color(SUB)));
                let lead = row![hover(thumb, over), text.spacing(2)].spacing(12);
                let tag = container(bold(kinds[h.0], 12.0).color(SUB)).padding([0, 4]);
                let tag = tag.style(fill(theme::tint(0.1), 4.0));
                let dots = hot.then(|| icon_btn(i::DOTS, 16.0, menu.clone()));
                let tail = row![save(h.0, id, on)]
                    .push(dots)
                    .spacing(8)
                    .align_y(Center);
                let r = row![
                    lead.align_y(Center).width(FillPortion(6)),
                    container(tag).width(FillPortion(1)),
                    container(tail).align_right(120)
                ];
                let r = r.spacing(16).align_y(Center).height(80);
                let r = container(r.padding(Padding::new(8.0).left(12).right(16)));
                let r = if hot {
                    r.style(fill(theme::tint(0.1), 4.0))
                } else {
                    r
                };
                let m = mouse_area(r).on_right_press(menu).on_double_click(play);
                let m = if h.0 == 2 { m } else { m.on_press(nav) };
                let enter = hk.clone();
                m.on_enter(act(move |a| a.hover = Some(enter.clone())))
                    .on_exit(act(move |a| a.hover = a.hover.take().filter(|x| *x != hk)))
                    .into()
            });
            let col = column![top, Column::with_children(rows)]
                .spacing(16)
                .width(rw);
            container(col)
                .padding(Padding::new(PAD).top(0).left(x0))
                .into()
        }
    };
    let none = container(sub("There are no results to show.", 16.0)).padding([0.0, PAD]);
    let empty = [
        false,
        songs_res.is_empty(),
        albums_res.is_empty(),
        artists_res.is_empty(),
        pls.is_empty(),
    ][tab];
    Some(
        column![chips, if empty { none.into() } else { body }]
            .width(Fill)
            .into(),
    )
}

pub fn search_hits(t: (&[Artist], &[Album], &[Song]), q: &str) -> (Option<Hit>, Vec<Hit>) {
    let names: [Vec<&str>; 3] = [
        t.0.iter().map(|a| a.name.as_str()).collect(),
        t.1.iter().map(|a| a.name.as_str()).collect(),
        t.2.iter().map(|s| s.title.as_str()).collect(),
    ];
    let (ql, names) = (q.to_lowercase(), &names);
    let first = |f: &dyn Fn(String) -> bool| {
        (0..3).find_map(|k| {
            names[k]
                .iter()
                .position(|n| f(n.to_lowercase()))
                .map(|i| (k, i))
        })
    };
    let top = first(&|n| n == ql).or_else(|| first(&|n| n.starts_with(&ql)));
    let top = top.or_else(|| {
        [2, 1, 0]
            .into_iter()
            .find(|&k| !names[k].is_empty())
            .map(|k| (k, 0))
    });
    let take = |k: usize, r: std::ops::Range<usize>| {
        r.filter(move |&i| i < names[k].len()).map(move |i| (k, i))
    };
    let mix = take(2, 0..6)
        .chain(take(0, 0..2))
        .chain(take(1, 0..4))
        .chain(take(2, 6..10));
    (top, mix.filter(|h| Some(*h) != top).collect())
}

fn library<'a>(app: &'a App, kind: &str, arg: &'a str) -> El<'a> {
    const SORTS: [(&str, &str); 7] = [
        ("newest", "Recently added"),
        ("recent", "Recently played"),
        ("frequent", "Most played"),
        ("name", "Name"),
        ("artist", "Artist"),
        ("year", "Year"),
        ("random", "Random"),
    ];
    let cur = if arg.is_empty() { "newest" } else { arg };
    let chips = SORTS.map(|(k, name)| theme::chip_btn(name, cur == k, go("albums", k)));
    let chips = (kind == "albums").then(|| Row::with_children(chips).spacing(8));
    let title = match kind {
        "genre" => arg,
        "artists" => "Artists",
        _ => "Albums",
    };
    let (cols, cw) = app.grid();
    let g = |n, f: &dyn Fn(usize) -> El<'a>| grid(app, n, cols, cw + 52.0, grid_y(app), f);
    let artists = app.data::<(Vec<Artist>, usize)>(&app.route).map(|(v, _)| v);
    let artists = artists.map(|v| g(v.len(), &|n| artist_card(app, &v[n])));
    let albums = app.data::<(Vec<Album>, usize)>(&app.route).map(|(v, _)| v);
    let albums = albums.map(|v| g(v.len(), &|n| album_card(app, &v[n], v[n].artist.clone())));
    if kind == "genre" {
        let c = genre_color(app, arg);
        let head = container(huge(arg, app.main_w() - 2.0 * PAD)).padding(PAD);
        let head = head
            .width(Fill)
            .height(hero_h(app))
            .align_bottom(hero_h(app));
        let head = head.style(theme::down(&[(0.0, c), (1.0, theme::shade(c, 0.5))]));
        let body = column![Space::new().height(PAD)].push(albums).width(Fill);
        return column![head, fade(theme::shade(c, 0.4), 232.0, body)].into();
    }
    let head = column![bold(title, 32.0)].push(chips).spacing(16);
    let head = container(head)
        .padding(Padding::new(PAD).top(72).bottom(0))
        .height(grid_y(app));
    column![head, artists.or(albums)].into()
}

fn folders<'a>(app: &'a App) -> El<'a> {
    let (w, rh) = (app.main_w() - 2.0 * PAD - 80.0, app.row_h());
    let dir = |c: char, name: &'a str, url: &'a str, m: Message| -> El<'a> {
        let fg = if app.current_song().is_some_and(|s| s.url == url) {
            ACCENT
        } else {
            TEXT
        };
        let ic = container(i(c, 20.0).color(SUB).center()).center(rh - 16.0);
        let row = media(ic.style(fill(ELEVATED, 4.0)).into(), name, url, w, fg);
        let b = button(row).padding([8, 16]).width(Fill).height(rh);
        b.style(theme::track(0.0)).on_press(m).into()
    };
    let artists = app
        .data::<Vec<Artist>>(&app.route)
        .map_or(&[][..], |v| &v[..]);
    let radio = app
        .data::<Vec<Song>>(&app.route)
        .map(|v| ("Radio stations", &v[..]));
    let dir_data = app.data::<(String, Vec<Song>)>(&app.route);
    let dir_data = dir_data.map(|(n, v)| (n.as_str(), &v[..])).or(radio);
    let (name, songs) = dir_data.unwrap_or(("Folders", &[][..]));
    let c = cols(app, false, false, songs.len());
    let (n, (off, view)) = (artists.len() + songs.len(), app.scroll());
    let (lo, hi) = span(off - DIR_Y, off - DIR_Y + view, rh, n, 6);
    let items = (lo..hi).map(|k| match k.checked_sub(artists.len()) {
        None => dir(
            i::FOLDER,
            &artists[k].name,
            "",
            go("folder", &artists[k].id),
        ),
        Some(j) if songs[j].is_dir => {
            dir(i::FOLDER, &songs[j].title, "", go("folder", &songs[j].id))
        }
        Some(j) if !songs[j].url.is_empty() => {
            let s = &songs[j];
            dir(
                i::RADIO,
                &s.title,
                &s.url,
                Message::Play(vec![s.clone()], 0, s.title.clone()),
            )
        }
        Some(j) => track(app, &songs[j], j, &app.route, c),
    });
    let list = gaps(lo, hi, n, rh, items);
    let title = ell(name, app.main_w() - 2.0 * PAD, 32.0, Bold);
    let title = container(title).padding(PAD).height(DIR_Y);
    column![title, container(list).padding(Padding::new(PAD).top(0))].into()
}

fn lyrics(app: &App) -> El<'_> {
    let Some(song) = app.current_song() else {
        let t = sub("Play a song to see its lyrics.", 16.0);
        return container(t).center_x(Fill).padding(80).into();
    };
    let key = format!("lyrics:{}", song.id);
    if let Some(s) = state(app, &key) {
        return s;
    }
    let bg = fill(head_color(app, &song.cover), 0.0);
    let Some((l, cur, synced)) = lyric_at(app, song) else {
        let t = bold("No lyrics are available for this song.", 24.0);
        let t = container(t).center_x(Fill).padding(80);
        return t.height(app.scroll().1).style(bg).into();
    };
    let lag = (app.cfg.settings.lyrics_offset * 1000.0) as i64;
    let at = |t: u64| t as i64 - l.offset + lag;
    use iced::alignment::Horizontal as H;
    let align = [H::Left, H::Center, H::Right][usize::from(app.cfg.settings.lyrics_align.min(2))];
    let lines = l.lines.iter().enumerate().map(|(n, (t, line))| -> El<'_> {
        let fg = match n {
            _ if !synced || Some(n) == cur => TEXT,
            n if cur.is_some_and(|c| n < c) => theme::tint(0.7),
            _ => BG.scale_alpha(0.8),
        };
        let size = app.cfg.settings.lyrics_size;
        let words = bold(if line.is_empty() { "\u{266a}" } else { line }, size);
        let words = words.align_x(align);
        let el: El<'_> = match t {
            Some(t) if synced => {
                let seek = Message::Seek(at(*t).max(0) as f32 / 1000.0);
                link_btn(words, fg, seek).into()
            }
            _ => words.color(fg).into(),
        };
        match n == cur.unwrap_or(0) {
            true => container(el).id("lyric").into(),
            false => el,
        }
    });
    let col = Column::with_children(lines)
        .spacing(16)
        .max_width(900)
        .align_x(align);
    let pad = Padding::new(PAD * 2.0).top(48).bottom(app.scroll().1 * 0.6);
    container(col).padding(pad).width(Fill).style(bg).into()
}

pub fn lyric_line(app: &App) -> Option<usize> {
    Some(lyric_at(app, app.current_song()?)?.1.unwrap_or(0))
}

fn lyric_at<'a>(app: &'a App, song: &Song) -> Option<(&'a Lyrics, Option<usize>, bool)> {
    let all = app.data::<Vec<Lyrics>>(&format!("lyrics:{}", song.id))?;
    let any = |synced: bool| {
        all.iter()
            .find(move |l| !l.lines.is_empty() && l.synced >= synced)
    };
    let l = any(true).or_else(|| any(false))?;
    let ms = (app.position * 1000.0) as i64;
    let end = (song.duration * 1000.0) as i64;
    let lag = (app.cfg.settings.lyrics_offset * 1000.0) as i64;
    let at = |t: u64| t as i64 - l.offset + lag;
    let fits = l
        .lines
        .iter()
        .filter_map(|x| x.0)
        .all(|t| at(t) <= end + 1000);
    let synced = l.synced && (end <= 0 || fits);
    let past = |x: &(Option<u64>, String)| x.0.is_some_and(|t| at(t) <= ms);
    let cur = l.lines.iter().rposition(past).filter(|_| synced);
    Some((l, cur, synced))
}

fn pick<'a, T: Clone + Send + Sync + 'static>(
    opts: Vec<(T, String)>,
    cur: impl PartialEq<T>,
    f: fn(&mut Settings, T),
) -> El<'a> {
    let on = opts.iter().find(|o| cur == o.0).map(|o| o.1.clone());
    let labels: Vec<String> = opts.iter().map(|o| o.1.clone()).collect();
    let find = move |l: String| opts.iter().find(|o| o.1 == l).expect("an option").0.clone();
    let p = pick_list(labels, on, move |l| set(f, find(l)));
    let p = p.placeholder("The saved value is not available");
    let p = p.text_size(14).font(theme::fw(Normal)).padding([7, 12]);
    let p = p.width(271).style(theme::pick(false));
    p.menu_style(theme::pick_menu).into()
}

fn devices(app: &App) -> Vec<(String, String)> {
    let found = app.data::<Vec<(String, String)>>("devices");
    let found = found.into_iter().flatten().cloned();
    std::iter::once((String::new(), "System default".into()))
        .chain(found)
        .collect()
}

fn settings<'a>(app: &'a App) -> El<'a> {
    let st = &app.cfg.settings;
    let opts = |v: &[&str]| v.chunks(2).map(|p| (p[0].into(), p[1].into())).collect();
    let num = |v: f32, r: RangeInclusive<f32>, f: fn(&mut Settings, f32), label: String| {
        let step = [0.1f32, 1.0][usize::from(*r.start() >= 1.0)];
        let s = slider(r, v, move |x| set(f, x)).step(step);
        let label = sub(label, 14.0).width(56).align_x(Right);
        let r = row![s.width(200).style(theme::seek), label];
        El::from(r.spacing(12).align_y(Center))
    };
    let db = |v: f32, f| num(v, -15.0..=15.0, f, format!("{v:+.1} dB"));
    let tog = |v: bool, f: fn(&mut Settings, bool)| -> El<'_> {
        let t = toggler(v).on_toggle(move |b| set(f, b));
        t.style(theme::tog).size(20).into()
    };
    let head = |t: &'static str| container(bold(t, 16.0)).padding(padding::top(PAD).bottom(8));
    fn line<'a>(label: impl text::IntoFragment<'a>, c: El<'a>) -> El<'a> {
        let label = txt(label, 14.0).color(SUB).width(Fill).height(24);
        row![label.align_y(Center), c]
            .align_y(Center)
            .padding([4, 0])
            .into()
    }
    let mut rates = [0, 320, 256, 192, 128, 96].map(|b| (b, format!("{b} kbps")));
    rates[0].1 = "No limit".into();
    let zooms = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5].map(|z| (z, format!("{:.0}%", z * 100.0)));
    let gains = opts(&["off", "Off", "track", "Track", "album", "Album"]);
    let formats = opts(&["", "Original", "mp3", "MP3", "opus", "Opus", "aac", "AAC"]);
    let (tf, br) = (st.transcode_format.as_str(), st.transcode_max_bitrate);
    let (x, secs) = (st.crossfade_secs, format!("{:.0} s", st.crossfade_secs));
    let device = pick(devices(app), st.device.as_str(), |s, v| s.device = v);
    let gain = pick(gains, st.replaygain.as_str(), |s, v| s.replaygain = v);
    let preamp = db(st.preamp_db, |s, v| s.preamp_db = v);
    let fallback = db(st.fallback_db, |s, v| s.fallback_db = v);
    let fade = num(x, 0.0..=12.0, |s, v| s.crossfade_secs = v, secs);
    let (pct, step) = (st.scrobble_pct, st.seek_secs);
    let (lag, size, secs) = (st.lyrics_offset, st.lyrics_size, st.scrobble_secs);
    let (l1, l2, l3) = (
        format!("{pct:.0}%"),
        format!("{step:.0} s"),
        format!("{secs:.0} s"),
    );
    let pct = match st.scrobble_by_time {
        true => num(secs, 10.0..=600.0, |s, v| s.scrobble_secs = v, l3),
        false => num(pct, 10.0..=100.0, |s, v| s.scrobble_pct = v, l1),
    };
    let by = [
        (false, "Part of the track".into()),
        (true, "Time played".into()),
    ];
    let by = pick(by.into(), st.scrobble_by_time, |s, v| {
        s.scrobble_by_time = v
    });
    let modes = [
        (0, "Play now"),
        (3, "Add next"),
        (2, "Add to the end"),
        (1, "Shuffle"),
    ];
    let modes = modes.map(|(m, l)| (m, l.to_string())).into();
    let mode = pick(modes, st.play_mode, |s, v| s.play_mode = v);
    let skip = text_input("live, remix, intro", &st.queue_skip);
    let skip = skip
        .on_input(|v| set(|s, v| s.queue_skip = v, v))
        .width(271);
    let skip = skip.style(theme::input([ELEVATED; 2], [GREY; 2], [0.0, 1.0], 4.0));
    let skip = skip
        .font(theme::fw(Normal))
        .padding([7, 12])
        .size(14)
        .into();
    let step = num(step, 1.0..=30.0, |s, v| s.seek_secs = v, l2);
    let vs = st.vol_step;
    let vol_step = num(vs, 1.0..=25.0, |s, v| s.vol_step = v, format!("{vs:.0}%"));
    let aligns = [
        (0, "Left".into()),
        (1, "Center".into()),
        (2, "Right".into()),
    ];
    let align = pick(aligns.into(), st.lyrics_align, |s, v| s.lyrics_align = v);
    let (l1, l2) = (format!("{lag:+.1} s"), format!("{size:.0} px"));
    let lag = num(lag, -5.0..=5.0, |s, v| s.lyrics_offset = v, l1);
    let size = num(size, 20.0..=48.0, |s, v| s.lyrics_size = v, l2);
    let format = pick(formats, tf, |s, v| s.transcode_format = v);
    let rate = pick(rates.into(), br, |s, v| s.transcode_max_bitrate = v);
    let banners_label =
        "Show artist banners from TheAudioDB (this sends artist names to TheAudioDB)";
    let zoom = pick(zooms.into(), st.zoom, |s, v| s.zoom = v);
    let frame_label = "Use the system title bar on Wayland desktops that do not draw one";
    let frame_label = format!("{frame_label} (restart to apply)");
    let tray_label = "Keep Navify in the system tray when you close the window";
    let none = [" (no system tray found)", ""][usize::from(app.tray.is_some())];
    let tray_label = format!("{tray_label}{none}");
    macro_rules! on {
        ($label:expr, $f:ident) => {
            line($label, tog(st.$f, |s, b| s.$f = b))
        };
    }
    let server = |s: &'a Server| -> El<'a> {
        let active = app.cfg.active.as_ref() == Some(&s.id);
        let who = sub(format!("{}{DOT}{}", s.user, s.url), 14.0);
        let info = column![txt(&s.name, 14.0), who].spacing(2).width(Fill);
        let out = active.then(|| theme::pill_btn(bold("Log out", 14.0), Message::LogOut));
        row![info, out].align_y(Center).height(56).into()
    };
    let mb = |b: u64| b as f32 / 1_048_576.0;
    let disk = mb(app.data::<u64>("disk").copied().unwrap_or(0));
    let memory = mb(app.covers.bytes as u64);
    let cache = format!("{disk:.1} MB on disk, {memory:.1} MB in memory");
    let cache = sub(cache, 14.0);
    let clear = theme::pill_btn(bold("Clear", 14.0), Message::ClearCache);
    let cache = row![cache, clear].spacing(16).align_y(Center).into();
    let version = sub(env!("CARGO_PKG_VERSION"), 14.0).into();
    let col = column![
        bold("Settings", 32.0),
        head("Playback"),
        line("Output device", device),
        line("ReplayGain", gain),
        line("Preamp", preamp),
        line("Gain for tracks without ReplayGain", fallback),
        on!("Limit the gain to stop clipping", rg_clip),
        line("Crossfade", fade),
        line("Seek step for keys and media controls", step),
        line("Volume step for keys and the mouse wheel", vol_step),
        line("Action of the play buttons and double-click", mode),
        head("Queue"),
        on!("Ask before you replace the queue", confirm_queue),
        line("Do not add songs with these words (use commas)", skip),
        head("Streaming"),
        line("Transcode format", format),
        line("Maximum bitrate", rate),
        head("Library"),
        on!("Sync the play queue with the server", queue_sync),
        on!("Send played tracks to the server (scrobble)", scrobble),
        line("Scrobble by", by),
        line("Scrobble after", pct),
        head("Display"),
        line("Zoom", zoom),
        on!(banners_label, banners),
        on!("Show the rating column", ratings),
        on!("Show the play count column", plays),
        on!("Smooth scrolling", smooth),
        on!("Scroll the lyrics with the track", lyrics_follow),
        line("Lyrics offset (positive shows the lines later)", lag),
        line("Lyrics text size", size),
        line("Lyrics alignment", align),
        cfg!(target_os = "linux").then(|| on!(frame_label, system_frame)),
        on!(tray_label, tray),
        on!("Start minimized", start_min),
        on!("Show a notification when a new track starts", notify),
        on!("Keep the computer awake while music plays", awake),
        on!("Hide in the tray when you minimize the window", min_tray),
        head("Keyboard shortcuts"),
    ];
    let col = col.extend(crate::app::KEYS.iter().map(|k| {
        let on = app.recording == Some(k.0);
        let label = match crate::app::key_label(app.binding(k.0)) {
            _ if on => "Press a key".to_string(),
            c if c.is_empty() => "Not set".to_string(),
            c => c,
        };
        let bg = [
            [ELEVATED, theme::MENU_HOVER, theme::MENU_HOVER],
            [ACCENT; 3],
        ][usize::from(on)];
        let b = button(bold(label, 12.0)).padding([6, 12]);
        let b = b.style(theme::solid(bg, [TEXT, BG][usize::from(on)]));
        line(
            k.1,
            b.on_press(act(move |a| a.recording = (!on).then_some(k.0)))
                .into(),
        )
    }));
    let reset = theme::pill_btn(bold("Reset", 14.0), set(|s, ()| s.keys.clear(), ()));
    let col = col.push(line("Use the default shortcuts", reset.into()));
    let col = col.push(head("Servers"));
    let col = col.extend(app.cfg.servers.iter().map(server));
    let col = col.push(head("Storage"));
    let col = col.push(line("Cover cache", cache));
    let copy = task(|a| {
        let json = serde_json::to_string_pretty(&a.cfg.settings).expect("settings are JSON");
        iced::Task::batch([
            iced::clipboard::write(json),
            a.toast("Copied the settings to the clipboard."),
        ])
    });
    let paste = task(|_| {
        iced::clipboard::read().map(|t| {
            task(move |a| {
                let new = serde_json::from_str::<Settings>(t.as_deref().unwrap_or_default());
                let new = new.map_err(|e| e.to_string());
                match new.and_then(|n| n.check().map(|()| n).map_err(String::from)) {
                    Ok(mut new) => {
                        let s = &a.cfg.settings;
                        (new.window, new.device) = (s.window, s.device.clone());
                        a.volume = new.volume;
                        let old = std::mem::replace(&mut a.cfg.settings, new);
                        iced::Task::batch([
                            a.settings(old),
                            a.toast("Applied the settings from the clipboard."),
                        ])
                    }
                    Err(e) => a.toast(&format!(
                        "The clipboard does not contain Navify settings. {e}"
                    )),
                }
            })
        })
    });
    let copy = theme::pill_btn(bold("Copy", 14.0), copy);
    let paste = theme::pill_btn(bold("Paste", 14.0), paste);
    let io = row![copy, paste].spacing(8).into();
    let col = col.push(line(
        "Copy the settings to the clipboard, or paste them",
        io,
    ));
    let col = col.push(head("About"));
    let col = col.push(line("Version", version)).max_width(836);
    container(col).padding(32).center_x(Fill).into()
}

pub fn now_playing<'a>(app: &'a App) -> El<'a> {
    let w = app.right_w() - 32.0;
    let Some(s) = app.current_song() else {
        let go = button(bold("Search", 14.0))
            .padding([7, 16])
            .style(theme::accent);
        let col = column![
            bold("Find something to play", 24.0),
            go.on_press(Message::Nav("search".into()))
        ];
        return container(col.spacing(24).align_x(Center))
            .center(Fill)
            .into();
    };
    let starred = app.is_starred(&s.id, s.starred);
    let card = |c: Column<'a, Message>| {
        let c = container(c.spacing(12).padding(16).width(Fill));
        c.style(fill(ELEVATED, 8.0))
    };
    let lead = s.artists.first();
    let artist = bold(lead.map_or(&s.artist, |a| &a.name), 16.0);
    let artist = btn(artist, go("artist", lead.map_or(&s.artist_id, |a| &a.id)));
    let title = ell(&s.title, w - 40.0, 24.0, Bold);
    let radio = !s.url.is_empty();
    let home = if radio {
        Message::Nav("radio".into())
    } else {
        go("album", &s.album_id)
    };
    let title = btn(title, home);
    let next = app.queue.get(app.real_at(app.index + 1)).map(|n| {
        let open = link_btn(bold("Open queue", 14.0), SUB, Message::Panel(2));
        let head = row![bold("Next in queue", 16.0).width(Fill), open];
        card(column![
            head,
            queue_row(app, n, Some(app.index + 1), w - 32.0)
        ])
    });
    let lines = column![title, artists_line(&s.artists, 16.0, w - 40.0)].spacing(4);
    let heart = (!radio).then(|| heart_btn(&s.id, starred, 20.0));
    let info = row![lines.width(Fill), heart].align_y(Center);
    let about = app.lead_artist().map(|id| {
        let head = || container(bold("About the artist", 16.0)).padding(16);
        let pic = app
            .covers
            .get(&format!("w:{}", app.artist_cover(id)), app.bucket(w));
        let pic = pic.map(|h| {
            let r = iced::border::Radius::new(8.0).bottom(0.0);
            let img = image(h).width(Fill).height(w * 0.6).border_radius(r);
            let img = img.content_fit(iced::ContentFit::Cover);
            let shade = head().width(Fill).style(|_| {
                let g = iced::gradient::Linear::new(std::f32::consts::PI);
                let g = g.add_stop(0.0, Color::BLACK.scale_alpha(0.5));
                container::Style::default().background(g.add_stop(1.0, Color::TRANSPARENT))
            });
            stack![img, shade]
        });
        let bio = app
            .data::<Info>(&format!("info:{id}"))
            .map(|i| plain(&i.biography));
        let bio = bio.filter(|b| !b.is_empty()).map(|b| {
            container(txt(b, 14.0).color(SUB))
                .max_height(58.0)
                .clip(true)
        });
        let body = column![artist].push(bio).spacing(12).width(Fill);
        let body = body.padding(Padding::new(16.0).top([0.0, 16.0][usize::from(pic.is_some())]));
        let top = pic.map_or_else(|| El::from(head()), El::from);
        let c = container(column![top, body]);
        c.style(fill(ELEVATED, 8.0))
    });
    let preview = lyric_at(app, s).map(|(l, cur, _)| {
        let lines = l.lines[cur.unwrap_or(0)..]
            .iter()
            .filter(|x| !x.1.is_empty())
            .take(3)
            .zip([1.0, 1.0, 0.4]);
        let lines =
            lines.map(|((_, t), a)| ell(t, w - 32.0, 20.0, Bold).color(TEXT.scale_alpha(a)));
        let body = column![bold("Lyrics preview", 16.0)].extend(lines.map(Into::into));
        let body = container(body.spacing(8).padding(16).width(Fill));
        let bg = head_color(app, &s.cover);
        let b = button(body)
            .padding(0)
            .style(theme::btn([bg; 3], [TEXT; 3], 8.0));
        b.on_press(Message::Nav("lyrics".into()))
    });
    let art = cover(app, &s.cover, w, false);
    let body = column![art, info].push(preview).push(about).push(next);
    let body = body.spacing(16).padding(Padding::new(16.0).top(0));
    let src = [&app.source, &s.album][usize::from(app.source.is_empty())];
    let head = panel_head(ell(src, app.right_w() - 64.0, 16.0, Bold), 1);
    column![head, scrollable(body).style(theme::scroll)].into()
}

fn panel_head<'a>(title: impl Into<El<'a>>, n: u8) -> El<'a> {
    let title = container(title).width(Fill);
    let x = icon_btn(i::X, 16.0, Message::Panel(n));
    let r = row![title, x].spacing(8).align_y(Center);
    r.padding(Padding::new(8.0).left(16)).into()
}

fn queue_row<'a>(app: &'a App, s: &'a Song, n: Option<usize>, w: f32) -> El<'a> {
    let cur = n == Some(app.index);
    let art = cover(app, &s.cover, 48.0, false);
    let x = n
        .filter(|_| !cur)
        .map(|n| icon_btn(i::X, 14.0, Message::QDel(n)));
    let fg = if cur { ACCENT } else { TEXT };
    let r = media(art, &s.title, &s.artist, w - 100.0, fg).push(x);
    let menu = match s.url.is_empty() {
        true => Message::Menu(Box::new(MenuKind::Song(vec![s.clone()]))),
        false => Message::Noop,
    };
    let play = || Message::Play(vec![s.clone()], 0, "Recently played".into());
    let movable = n.filter(|&n| n > app.index);
    let to = app.qmove.filter(|&(f, t)| f != t && Some(t) == movable);
    let line = |on: bool| {
        let l = container(Space::new().width(Fill).height(2));
        on.then(|| l.style(fill(ACCENT, 1.0)))
    };
    let (up, down) = to.map_or((false, false), |(f, t)| (t < f, t > f));
    let pad = Padding::new(8.0)
        .top(8.0 - 2.0 * f32::from(up))
        .bottom(8.0 - 2.0 * f32::from(down));
    let r = column![line(up), container(r).padding(pad), line(down)];
    let r = mouse_area(r).on_double_click(n.map_or_else(play, Message::PlayQueue));
    let r = match movable {
        Some(n) => r
            .on_press(act(move |a| a.qmove = Some((n, n))))
            .on_enter(act(move |a| {
                if let Some(m) = &mut a.qmove {
                    m.1 = n;
                }
            })),
        None => r,
    };
    r.on_right_press(menu).into()
}

pub fn device_panel(app: &App) -> El<'_> {
    let cur = &app.cfg.settings.device;
    let (on, others): (Vec<_>, Vec<_>) = devices(app).into_iter().partition(|d| &d.0 == cur);
    let name = on.into_iter().next().map_or_else(|| cur.clone(), |d| d.1);
    let card = row![
        i(i::DEVICE, 24.0).color(ACCENT),
        txt(name, 16.0).color(ACCENT)
    ];
    let card = container(card.spacing(12).align_y(Center))
        .padding(20)
        .width(Fill);
    let card = container(card.style(fill(ELEVATED, 8.0))).padding([0, 8]);
    let title = ["Select another device", "No other devices found"][usize::from(others.is_empty())];
    let title = container(bold(title, 16.0)).padding(Padding::new(20.0).top(32).bottom(8));
    let rows = others.into_iter().map(|(id, name)| {
        let r = row![i(i::DEVICE, 24.0), txt(name, 16.0)]
            .spacing(12)
            .align_y(Center);
        let b = button(r)
            .padding([12, 20])
            .width(Fill)
            .style(theme::track(0.0));
        b.on_press(set(|s, v| s.device = v, id)).into()
    });
    let list = column![card, title]
        .extend(rows)
        .padding(padding::bottom(16));
    let head = panel_head(bold("Connect", 16.0), 3);
    column![head, scrollable(list).style(theme::scroll)].into()
}

pub fn add_panel(app: &App) -> El<'_> {
    let head = panel_head(bold("Add to playlist", 16.0), 4);
    let Some((p, have)) = own_pl(app) else {
        return head;
    };
    let q = app.pl_more.as_deref().unwrap_or("");
    let style = theme::input([ELEVATED; 2], [ELEVATED; 2], [0.0; 2], 500.0);
    let field = find_input(q, "pladd")
        .size(16)
        .padding(Padding::new(12.0).left(16));
    let field = container(field.style(style)).padding([8, 16]);
    let label =
        (q.trim().is_empty() && !app.recent.is_empty()).then(|| sub("Recently played", 14.0));
    let label = label.map(|l| container(l).padding(Padding::new(16.0).bottom(8)));
    let w = app.right_w() - 152.0;
    let rows = add_songs(app).map(|s| {
        let on = have.iter().any(|x| x.id == s.id);
        let add = theme::sq(theme::saved(on, 16.0).center(), 32.0).style(theme::link(SUB));
        let add =
            add.on_press_maybe((!on).then(|| Message::PlAdd(p.id.clone(), vec![s.id.clone()])));
        let art = cover(app, &s.cover, 48.0, false);
        let r = row![media(art, &s.title, &s.artist, w, TEXT), add];
        container(r.spacing(12).align_y(Center))
            .padding([8, 16])
            .into()
    });
    let list = column![label, Column::with_children(rows)].padding(padding::bottom(16));
    column![head, field, scrollable(list).style(theme::scroll)].into()
}

pub fn queue_panel(app: &App) -> El<'_> {
    let w = app.right_w() - 16.0;
    let next = (app.index + 1..app.queue.len()).take(100);
    let next = next.filter_map(|n| app.queue.get(app.real_at(n)).map(|s| (n, s)));
    let next: Vec<El> = next.map(|(n, s)| queue_row(app, s, Some(n), w)).collect();
    let more = app.queue.len().saturating_sub(app.index + 101);
    let title = |t: String| {
        let t = container(ell(t, w - 16.0, 16.0, Bold));
        t.padding(Padding::new(8.0).top(16))
    };
    let note = |t: String| container(sub(t, 14.0)).padding(8);
    let from = match app.source.is_empty() {
        true => "Next in queue".into(),
        false => format!("Next from: {}", app.source),
    };
    let now = app
        .current_song()
        .map(|s| queue_row(app, s, Some(app.index), w));
    let now = now.map(|r| column![title("Now playing".into()), r]);
    let clear = link_btn(bold("Clear queue", 14.0), SUB, task(App::q_clear));
    let from = row![ell(from, w - 116.0, 16.0, Bold).width(Fill), clear].align_y(Center);
    let from = container(from).padding(Padding::new(8.0).top(16));
    let list = column![now, (!next.is_empty()).then_some(from)].extend(next);
    let list = list.push((more > 0).then(|| note(format!("{more} more songs"))));
    let tab = |t: &'static str, on: bool| {
        let edge = container(Space::new().width(Fill).height(2));
        let edge = edge.style(fill([CLEAR, ACCENT][usize::from(on)], 1.0));
        let b = column![bold(t, 16.0).wrapping(text::Wrapping::None), edge].spacing(4);
        let b = button(b)
            .padding(0)
            .style(theme::link([SUB, TEXT][usize::from(on)]));
        b.on_press(act(move |a| a.recent_tab = t != "Queue"))
    };
    let past = app.recent.iter().map(|s| queue_row(app, s, None, w));
    let past = Column::with_children(past).padding(Padding::new(8.0).top(0));
    let past = past.push(
        app.recent
            .is_empty()
            .then(|| note("You did not play a track.".into())),
    );
    let tabs = row![
        tab("Queue", !app.recent_tab),
        tab("Recently played", app.recent_tab)
    ];
    let head = panel_head(tabs.spacing(24), 2);
    if app.recent_tab {
        return column![head, scrollable(past).style(theme::scroll)].into();
    }
    if app.queue.is_empty() {
        let hint = "Click \"Add to queue\" in the menu of a song to show it here.";
        let body = blank(
            Some(i::QUEUE),
            bold("Add to your queue", 24.0),
            sub(hint, 16.0),
            "Find something to play",
        );
        let body = container(body).padding([0, 22]).center(Fill);
        return column![head, body].into();
    }
    let list = list.padding(Padding::new(8.0).top(0));
    column![head, scrollable(list).style(theme::scroll)].into()
}

type Item = Option<(char, String, Message, bool)>;

pub fn menu<'a>(app: &'a App, m: &'a Menu) -> (El<'a>, f32, f32) {
    let w = if matches!(m.kind, MenuKind::User) {
        328.0
    } else {
        250.0
    };
    let entry = |r: Row<'a, Message>, msg: Message| -> El<'a> {
        let hover = [theme::CLEAR, theme::MENU_HOVER, theme::MENU_HOVER];
        let b = button(container(r.spacing(12).align_y(Center)).center_y(Fill));
        let b = b.padding([0, 12]).height(40).width(Fill);
        let b = b.style(theme::btn(hover, [theme::tint(0.9); 3], 2.0));
        focusable(b.on_press(msg.clone()), Some(msg), 2.0, Some("layer"))
    };
    let boxed = |el: El<'a>, w: f32, h: f32, pad: u16| {
        let c = container(el).padding(pad).width(w).height(h);
        let r = if matches!(m.kind, MenuKind::Create) {
            8.0
        } else {
            4.0
        };
        (c.style(fill(theme::MENU, r)).into(), w, h)
    };
    let it = |c: char, l: &str, m: Message| Some((c, l.to_string(), m, false));
    let open = |n: u8, c: char, l: &str| {
        let m = act(move |a| {
            let (x, y) = crate::app::cursor();
            a.menu
                .iter_mut()
                .for_each(|m| (m.sub, m.x, m.y) = (n, x, y));
        });
        Some((c, l.to_string(), m, true))
    };
    let user = app.session.as_ref().map(|s| &s.server);
    let mine = |p: &Playlist| app.owns(p);
    let star = |id: &str, server: bool, yes: &str, no: &str| {
        let k = usize::from(app.is_starred(id, server));
        let m = Message::Star(vec![id.into()], k == 0);
        it([i::SAVE, i::SAVED][k], [no, yes][k], m)
    };
    let queue = |r: &str| it(i::LISTPLUS, "Add to queue", Message::PlayCtx(r.into(), 2));
    let to_pl = |r: String, skip: &str| -> Vec<Item> {
        let pls = app.data::<Lib>("lib").into_iter();
        let pls = pls.flat_map(|l| l.playlists.iter());
        let add = |p: &Playlist| it(i::MUSIC, &p.name, Message::AddCtx(r.clone(), p.id.clone()));
        let new = it(
            i::PLUS,
            "New playlist",
            Message::AddCtx(r.clone(), String::new()),
        );
        let pls = pls.filter(|p| mine(p) && p.id != skip).map(add);
        [new, None].into_iter().chain(pls).collect()
    };
    let ids = |s: &[Song]| s.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
    let mut head: Option<El> = None;
    let list: Vec<Item> = match &m.kind {
        MenuKind::Create => {
            let art = container(i(i::MUSIC, 24.0)).center(48);
            let art = art.style(fill(theme::tint(0.1), 24.0));
            let label = column![
                bold("Playlist", 16.0),
                sub("Create a playlist with songs", 14.0)
            ];
            let r = row![art, label.spacing(2)].spacing(12).align_y(Center);
            let msg = Message::PlNew(String::new(), Vec::new());
            let hover = [theme::CLEAR, theme::tint(0.1), theme::tint(0.1)];
            let b = button(r).padding(8).width(Fill);
            let b = b
                .style(theme::btn(hover, [TEXT; 3], 6.0))
                .on_press(msg.clone());
            return boxed(focusable(b, Some(msg), 6.0, Some("layer")), 367.0, 72.0, 4);
        }
        MenuKind::View => {
            let (key, desc, compact) = (
                app.pl_sort & 15,
                app.pl_sort & 16 != 0,
                app.cfg.settings.compact,
            );
            let head = |t: &'a str| -> El<'a> {
                let t = bold(t, 12.0).color(theme::tint(0.7));
                container(t)
                    .padding(Padding::new(12.0).top(14))
                    .height(41)
                    .into()
            };
            let item = |l: &'a str, c: Option<char>, on: bool, tail: char, msg: Message| {
                let fg = if on { ACCENT } else { theme::tint(0.9) };
                let r = Row::new().push(c.map(|c| i(c, 16.0).color(fg)));
                let r = r.push(txt(l, 14.0).color(fg).width(Fill));
                entry(r.push(on.then(|| i(tail, 16.0).color(fg))), msg)
            };
            let sorts = app.route.starts_with("playlist:").then(|| {
                let arrow = [i::ARROW_UP, i::ARROW_DOWN][usize::from(desc)];
                let rows = ORDER.iter().zip(0u8..).map(|(l, n)| {
                    let next = if n == key {
                        app.pl_sort ^ 16
                    } else {
                        n | (16 * u8::from(n == 4))
                    };
                    item(l, None, n == key, arrow, act(move |a| a.pl_sort = next))
                });
                column![head("Sort by")].extend(rows)
            });
            let views = [("Compact", i::COMPACT, true), ("List", i::LIST, false)];
            let views = views.map(|(l, c, v)| {
                item(
                    l,
                    Some(c),
                    compact == v,
                    i::CHECK,
                    set(|s, b| s.compact = b, v),
                )
            });
            let n = sorts.as_ref().map_or(0, |_| ORDER.len());
            let h = 41.0 * (1 + usize::from(n > 0)) as f32 + 40.0 * (n + 2) as f32 + 8.0;
            let col = Column::new()
                .push(sorts)
                .push(head("View as"))
                .extend(views);
            return boxed(col.into(), 160.0, h, 4);
        }
        MenuKind::Song(v) | MenuKind::SongIn(v, _) if m.sub == 2 && v.len() == 1 => {
            let id = &v[0].id;
            let cur = app.ratings.get(id).copied().unwrap_or(v[0].user_rating);
            let stars = (1..=5u8).map(|n| {
                let m = Message::Rate(id.clone(), if n == cur { 0 } else { n });
                let b = theme::sq(i(i::STAR, 20.0).center(), 36.0);
                let b = b.style(theme::link(if n <= cur { ACCENT } else { SUB }));
                focusable(b.on_press(m.clone()), Some(m), 18.0, Some("layer"))
            });
            return boxed(Row::with_children(stars).into(), 192.0, 48.0, 6);
        }
        MenuKind::User => {
            let url = ell(user.map_or("", |s| &s.url), w - 32.0, 12.0, Normal);
            let name = bold(user.map_or("", |s| &s.user), 14.0);
            let who = column![name, url.color(SUB)].spacing(2);
            head = Some(container(who).padding([0, 12]).center_y(40).into());
            let active = |s: &&Server| app.cfg.active.as_ref() != Some(&s.id);
            let others = app.cfg.servers.iter().filter(active).map(|s| {
                let label = format!("Switch to {}", s.name);
                it(' ', &label, Message::UseServer(s.id.clone()))
            });
            let profile = it(' ', "Profile", Message::Nav("profile".into()));
            let settings = it(' ', "Settings", Message::Nav("settings".into()));
            let out = it(' ', "Log out", Message::LogOut);
            others.chain([profile, settings, out]).collect()
        }
        MenuKind::Song(s) | MenuKind::SongIn(s, _) if m.sub == 1 => {
            let name = match &s[..] {
                [x] => x.title.clone(),
                _ => String::new(),
            };
            let pls = app
                .data::<Lib>("lib")
                .into_iter()
                .flat_map(|l| l.playlists.iter());
            let pls = pls.filter(|p| mine(p));
            let add = |p: &Playlist| it(i::MUSIC, &p.name, Message::PlAdd(p.id.clone(), ids(s)));
            let new = it(i::PLUS, "New playlist", Message::PlNew(name, ids(s)));
            [new, None].into_iter().chain(pls.map(add)).collect()
        }
        MenuKind::Song(songs) | MenuKind::SongIn(songs, _) => {
            let on = songs.iter().all(|s| app.is_starred(&s.id, s.starred));
            let like = ["Add to Liked Songs", "Remove from your Liked Songs"];
            let q = |next| Message::Queue(songs.clone(), next);
            let mut v = vec![open(1, i::PLUS, "Add to playlist")];
            if let MenuKind::SongIn(_, n) = &m.kind {
                let rm = Message::PlRemove(n.clone());
                v.push(it(i::TRASH, "Remove from this playlist", rm));
            }
            let (star, k) = (Message::Star(ids(songs), !on), usize::from(on));
            v.push(it(['\0', i::SAVED][k], like[k], star));
            v.push(it(i::LISTPLUS, "Add to queue", q(false)));
            v.push(it(i::QUEUE, "Play next", q(true)));
            if let [s] = &songs[..] {
                let radio = Message::Radio(s.id.clone());
                v.extend([None, it(i::RADIO, "Go to song radio", radio)]);
                let links = [("artist", &s.artist_id), ("album", &s.album_id)];
                let links = links
                    .into_iter()
                    .filter(|(k, id)| !id.is_empty() && app.route != format!("{k}:{id}"));
                v.extend(links.map(|(k, id)| {
                    let c = if k == "artist" { i::USER } else { i::ALBUM };
                    it(c, &format!("Go to {k}"), go(k, id))
                }));
                let info = Message::Modal(s.id.clone(), 5);
                v.extend([it(i::INFO, "Show song details", info), None]);
                v.push(open(2, i::STAR, "Rate"));
            }
            v
        }
        MenuKind::Album(a) if m.sub == 1 => to_pl(format!("album:{}", a.id), ""),
        MenuKind::Album(a) => {
            let save = ["Remove from Your Library", "Add to Your Library"];
            let r = format!("album:{}", a.id);
            let mut v = vec![star(&a.id, a.starred, save[0], save[1]), queue(&r)];
            let art = Some(&a.artist_id).filter(|id| !id.is_empty());
            let radio =
                art.map(|id| it(i::RADIO, "Go to artist radio", Message::Radio(id.clone())));
            let art = art.filter(|id| app.route != format!("artist:{id}"));
            v.extend(
                radio
                    .into_iter()
                    .chain(art.map(|id| it(i::USER, "Go to artist", go("artist", id)))),
            );
            v.extend([None, open(1, i::PLUS, "Add to playlist")]);
            v
        }
        MenuKind::Artist(a) => {
            let radio = Message::Radio(a.id.clone());
            let follow = star(&a.id, a.starred, "Unfollow", "Follow");
            vec![follow, it(i::RADIO, "Go to artist radio", radio)]
        }
        MenuKind::Playlist(p) if m.sub == 1 => to_pl(format!("playlist:{}", p.id), &p.id),
        MenuKind::Playlist(p) => {
            let mut v = vec![queue(&format!("playlist:{}", p.id)), None];
            if mine(p) {
                let modal = |n| Message::Modal(p.id.clone(), n);
                let (c, l) =
                    [(i::GLOBE, "Make public"), (i::LOCK, "Make private")][usize::from(p.public)];
                let flip =
                    Message::Edit(p.id.clone(), p.name.clone(), p.comment.clone(), !p.public);
                let new = Message::PlNew(String::new(), Vec::new());
                v.extend([
                    it(i::PENCIL, "Edit details", modal(1)),
                    it(i::MINUS, "Delete", modal(2)),
                    None,
                    it(i::MUSIC, "Create playlist", new),
                    None,
                    it(c, l, flip),
                ]);
            }
            v.push(open(1, i::PLUS, "Add to other playlist"));
            v
        }
    };
    let seps = list.iter().filter(|x| x.is_none()).count();
    let n = list.len() - seps + usize::from(head.is_some());
    let h = (n as f32 * 40.0 + seps as f32 + 8.0).min(400.0);
    let rows = list.into_iter().map(|x| match x {
        Some((c, l, msg, sub)) => {
            let room =
                w - 36.0 - 28.0 * f32::from(u8::from(sub)) - 28.0 * f32::from(u8::from(c != ' '));
            let label = ell(l, room, 14.0, Normal);
            let arrow = sub.then(|| theme::filled(theme::fill::PLAY, 12.0));
            let icon: Option<El> = match c {
                ' ' => None,
                '\0' => Some(crate::view::liked_art(16.0)),
                c => Some(i(c, 16.0).into()),
            };
            let r = Row::new().push(icon).push(label.width(Fill));
            entry(r.push(arrow), msg)
        }
        None => rule(),
    });
    let list = scrollable(Column::new().push(head).extend(rows));
    boxed(list.style(theme::scroll).into(), w, h, 4)
}

pub fn login(app: &App) -> El<'_> {
    let l = &app.login;
    const W: f32 = 324.0;
    let field = |label: &'static str, v: &str, n: u8| -> El<'_> {
        let style = theme::input([theme::PANEL; 2], [theme::FIELD, TEXT], [1.0, 2.0], 4.0);
        let input = text_input("", v).id(format!("login{n}"));
        let input = input.on_input(move |s| Message::LField(n, s));
        let input = input.on_submit(Message::LSubmit).secure(n == 2);
        let input = input.style(style).font(theme::fw(Normal));
        let input = input.padding(13).size(16);
        let input = input.line_height(text::LineHeight::Absolute(20.0.into()));
        column![bold(label, 14.0), input].spacing(8).into()
    };
    let saved = app.cfg.servers.iter().map(|s| -> El<'_> {
        let avatar = avatar(&s.user, 40.0);
        let drop = Message::DropServer(s.id.clone());
        let drop = focusable(icon_btn(i::X, 14.0, drop.clone()), Some(drop), 14.0, None);
        let body = media(avatar.into(), &s.name, &s.user, W - 120.0, TEXT).push(drop);
        let pick = Message::UseServer(s.id.clone());
        let b = button(body).padding(8).width(Fill);
        let b = b.style(theme::track(0.0));
        focusable(b.on_press(pick.clone()), Some(pick), 4.0, None)
    });
    let error = (!l.error.is_empty()).then(|| {
        let r = row![i(i::X, 16.0), txt(&l.error, 14.0)].spacing(12);
        let r = container(r.align_y(Center)).padding(12).width(Fill);
        r.style(fill(theme::RED, 4.0))
    });
    let has = !app.cfg.servers.is_empty();
    let saved = column![bold("Saved servers", 14.0)].extend(saved);
    let saved = saved.spacing(4).width(W);
    let flip = || act(|a| a.login.remember = !a.login.remember);
    let remember = toggler(l.remember).on_toggle(|b| act(move |a| a.login.remember = b));
    let remember = remember.style(theme::tog).size(20);
    let remember = row![remember, txt("Remember password", 14.0)].spacing(12);
    let remember = btn(remember.align_y(Center), flip());
    let go = (!l.busy).then_some(Message::LSubmit);
    let label = bold(if l.busy { "Wait…" } else { "Log In" }, 16.0).color(BG);
    let submit = button(label.center().width(Fill)).style(theme::accent);
    let submit = submit.width(Fill).padding(14).on_press_maybe(go.clone());
    let form = column![
        field("Server address", &l.url, 0),
        field("Username", &l.user, 1),
        field("Password", &l.pass, 2),
        focusable(remember, Some(flip()), 4.0, None),
        focusable(submit, go, 500.0, None),
    ];
    let title = bold("Log in to navify", 48.0).line_height(1.1);
    let title = title.align_x(Center);
    let logo = container(i(i::MUSIC, 36.0)).padding(8);
    let card = column![logo, title, error, has.then_some(saved), has.then(rule)];
    let card = card.push(form.spacing(16).width(W)).spacing(32);
    let width = (app.view_size().0 - 64.0).clamp(W + 64.0, 734.0);
    let card = container(card.align_x(Center)).padding([40, 32]);
    let card = card.width(width).center_x(Fill);
    let card = card.max_width(734).style(fill(theme::PANEL, 8.0));
    let page = container(card).center_x(Fill).padding(32);
    let bg = theme::down(&[(0.0, color!(0x2a2a2a)), (1.0, BG)]);
    let page = scrollable(page).style(theme::scroll).height(Fill);
    container(page).height(Fill).style(bg).into()
}
