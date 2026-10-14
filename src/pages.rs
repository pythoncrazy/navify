use std::ops::RangeInclusive;

use iced::font::Weight::{Black, Bold, Medium, Normal};
use iced::widget::{
    Column, Row, Space, button, column, container, hover, image, mouse_area, pick_list, row,
    scrollable, slider, stack, text, text_input, toggler,
};
use iced::{Center, Color, Fill, Padding, Right, color, padding};

use crate::api::{Album, Artist, Genre, Info, Lyrics, Playlist, Server, Song};
use crate::app::{App, Home, Lib, Menu, MenuKind, Message, Pager, SearchRes, act, set, task};
use crate::config::Settings;
use crate::theme::{
    self, ACCENT, BG, CLEAR, ELEVATED, SUB, TEXT, bold, ell, fill, icon as i, link_btn, sub, txt,
};
use crate::view::{
    El, SONGS_BG, art, artists_line, btn, cover, fmt_dur, fmt_long, fmt_n, focusable, go, icon_btn,
    liked_art, media,
};

type Cols = (f32, f32, f32, f32);

pub const PAD: f32 = 24.0;
pub const GRID_Y: f32 = 136.0;
pub const BAR: f32 = 84.0;
pub const HEAD: f32 = 53.0;
const DISC_HEAD: f32 = 136.0 + 16.0 + HEAD;
const GREY: Color = color!(0x535353);
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

fn plural(n: u32, word: &str) -> String {
    let s = if n == 1 { "" } else { "s" };
    format!("{} {word}{s}", fmt_n(n as usize))
}

fn cols(app: &App, album: bool, date: bool, n: usize) -> Cols {
    let idx = 16.0 + 8.0 * n.max(1).ilog10().saturating_sub(1) as f32;
    let stars = if app.cfg.settings.ratings { 106.0 } else { 0.0 };
    let w = app.main_w() - 2.0 * PAD - 184.0 - idx - stars;
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
    let m = if on {
        Message::Toggle
    } else {
        Message::PlayCtx(ctx.into(), 0)
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

fn heading<'a>(t: &str, all: Option<String>) -> El<'a> {
    let all = all.map(|r| link_btn(bold("Show all", 14.0), SUB, Message::Nav(r)));
    let r = row![bold(t, 24.0).width(Fill)].push(all).align_y(Center);
    container(r).padding([0.0, PAD]).into()
}

fn shelf<'a>(title: &str, all: Option<String>, cards: impl Iterator<Item = El<'a>>) -> El<'a> {
    let cards: Vec<El> = cards.collect();
    if cards.is_empty() {
        return Space::new().into();
    }
    let row = container(Row::with_children(cards)).padding([0, 12]);
    column![heading(title, all), row].spacing(8).into()
}

fn card<'a>(app: &App, cid: &str, title: &str, sub: String, route: String, round: bool) -> El<'a> {
    let cw = app.grid().1;
    let img = cw - 24.0;
    let art = cover(app, cid, img, round);
    let play = play_btn(app, 48.0, &route);
    let play = container(play).align_right(Fill).align_bottom(Fill);
    let over = |art| play_over(app, art, play.padding(8).into(), &route);
    let art = if round { art } else { over(art) };
    let title = ell(title, img, 16.0, Normal);
    let lines = column![title, ell(&sub, img, 14.0, Normal).color(SUB)].spacing(4);
    let style = theme::btn([theme::CLEAR, ELEVATED, BG], [TEXT; 3], 6.0);
    let b = button(column![art, lines].spacing(8)).padding(12);
    let b = b.style(style).width(cw).height(cw + 52.0);
    b.on_press(Message::Nav(route)).into()
}

fn album_card<'a>(app: &App, a: &Album, sub: String) -> El<'a> {
    let r = format!("album:{}", a.id);
    card(app, &a.cover, &a.name, sub, r, false)
}

fn artist_card<'a>(app: &App, a: &Artist) -> El<'a> {
    let r = format!("artist:{}", a.id);
    card(app, &a.cover, &a.name, "Artist".into(), r, true)
}

fn playlist_card<'a>(app: &App, p: &Playlist) -> El<'a> {
    let (by, r) = (format!("By {}", p.owner), format!("playlist:{}", p.id));
    card(app, &p.cover, &p.name, by, r, false)
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
    let r = r.push(h("Album", c.1)).push(h("Date added", c.2));
    let star = container(i(i::STAR, 16.0).color(SUB)).width(90);
    let r = r.push(app.cfg.settings.ratings.then_some(star));
    let r = r.push(clock.padding(padding::right(52)));
    let r = r.spacing(16).padding([0, 16]).height(36).align_y(Center);
    let p = Padding::new(PAD).top(0).bottom(16);
    container(column![r, rule()]).padding(p).into()
}

fn date(s: &str) -> String {
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
    let plays = (s.play_count > 0).then(|| fmt_n(s.play_count as usize));
    let album = ell(&s.album, c.1, 14.0, Normal);
    let album = link_btn(album, SUB, go("album", &s.album_id));
    let second: El = match pop {
        true => sub(plays.unwrap_or_default(), 14.0).width(c.1).into(),
        false => container(album).width(c.1).into(),
    };
    let more = hot.then(|| icon_btn(i::DOTS, 16.0, Message::RowMenu(key.into(), n)));
    let heart = (hot || starred).then(|| heart_btn(&s.id, starred, 16.0));
    let dur = sub(fmt_dur(s.duration), 14.0).width(56).align_x(Right);
    let thumb = art.then(|| cover(app, &s.cover, 40.0, false));
    let name = row![thumb, title].spacing(12).align_y(Center).width(c.0);
    let added = (c.2 > 0.0).then(|| sub(date(&s.created), 14.0).width(c.2));
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
    let r = r.push(added).push(stars).push(tail.align_y(Center));
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

fn grid_head<'a>(title: &str, chips: Option<El<'a>>) -> El<'a> {
    let col = column![bold(title, 32.0)].push(chips).spacing(16);
    let p = Padding::new(PAD).bottom(0);
    container(col).padding(p).height(GRID_Y).into()
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

fn by_date<'a>(v: impl Iterator<Item = &'a Album>) -> Vec<&'a Album> {
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
    let tool = |c: char, on: bool| {
        let b = theme::sq(i(c, 20.0).center(), 32.0);
        let b = b.style(theme::link(if on == grid_on { TEXT } else { SUB }));
        b.on_press(set(|s, b| s.disc_grid = b, on))
    };
    let arrow = i(if app.disc_old { i::UP } else { i::DOWN }, 16.0);
    let order = row![txt("Release date", 14.0), arrow].spacing(4);
    let order = link_btn(order.align_y(Center), SUB, Message::DiscOrder);
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
    let head = head.padding(Padding::new(PAD).bottom(0)).height(GRID_Y);
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
        let retry = theme::pill_btn("Try again", Message::Retry);
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
    Some(col.spacing(PAD).padding(PAD).into())
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
        "folders" | "folder" => Some(folders(app)),
        "lyrics" => Some(lyrics(app)),
        "settings" => Some(settings(app)),
        _ => Some(missing().into()),
    };
    page.unwrap_or_else(|| Space::new().into())
}

fn home(app: &App) -> Option<El<'_>> {
    let h = app.data::<Home>("home")?;
    let pls = app.data::<Lib>("lib").map_or(&[][..], |l| &l.playlists[..]);
    let w = app.main_w() - 2.0 * PAD;
    let n = if w >= 720.0 { 4 } else { 2 };
    let tw = (w - 8.0 * (n - 1) as f32) / n as f32;
    let tile = |art: El<'static>, name: &str, route: String| {
        let body = row![art, ell(name, tw - 80.0, 14.0, Bold)].spacing(12);
        let style = theme::btn([0.1, 0.2, 0.3].map(theme::tint), [TEXT; 3], 4.0);
        let b = button(body.align_y(Center)).padding(0).width(tw).height(56);
        let b = b.clip(true).style(style);
        let b = b.on_press(Message::Nav(route.clone()));
        let play = play_btn(app, 32.0, &route);
        let play = container(play).align_right(Fill).center_y(56);
        let play = play.padding([0, 8]);
        play_over(app, b.into(), play.into(), &route)
    };
    let pins = pls.iter().take(3);
    let pins = pins.map(|p| (&p.cover, &p.name, format!("playlist:{}", p.id)));
    let recent = h[0].iter();
    let recent = recent.map(|a| (&a.cover, &a.name, format!("album:{}", a.id)));
    let small = |c| cover(app, c, 56.0, false);
    let rest = pins.chain(recent).map(|(c, n, r)| tile(small(c), n, r));
    let liked = tile(liked_art(56.0), "Liked Songs", "liked".into());
    let mut tiles = std::iter::once(liked).chain(rest).take(8);
    let rows = std::iter::from_fn(|| {
        let r: Vec<El> = tiles.by_ref().take(n).collect();
        (!r.is_empty()).then(|| Row::with_children(r).spacing(8).into())
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
    let c = h[0].first().map_or(GREY, |a| head_color(app, &a.cover));
    let pls = pls.iter().take(k).map(|p| playlist_card(app, p));
    let pls = shelf("Your playlists", None, pls);
    let tiles = container(Column::from_vec(rows.collect()).spacing(8)).padding([0.0, PAD]);
    let col = column![tiles].extend(shelves).push(pls).spacing(40);
    Some(fade(theme::shade(c, 0.7), 332.0, col.padding([PAD, 0.0])))
}

struct Page<'a> {
    art: El<'a>,
    kind: &'static str,
    title: &'a str,
    meta: El<'a>,
    color: Color,
    extra: Vec<El<'a>>,
    banner: Option<&'a str>,
}

fn page<'a>(app: &App, p: Page<'a>, body: El<'a>) -> El<'a> {
    let px = art_px(app);
    let title = |tw: f32| {
        let fits = |s: &f32| theme::width(p.title, *s, Black) <= tw;
        let size = [96.0, 72.0, 48.0].into_iter().find(fits).unwrap_or(32.0);
        ell(p.title, tw, size, Black).line_height(1.1)
    };
    let mut shadow = theme::SHADOW;
    (shadow.blur_radius, shadow.offset) = (60.0, iced::Vector::new(0.0, 4.0));
    let r = [4.0, px / 2.0][usize::from(app.route.starts_with("artist:"))];
    let st = container::Style::default().shadow(shadow);
    let art = container(p.art).style(move |_| st.border(iced::border::rounded(r)));
    let bg = theme::down(&[(0.0, p.color), (1.0, theme::shade(p.color, 0.5))]);
    let hero: El = if let Some(url) = p.banner {
        let bh = app.main_w() / 3.0;
        let img = app.covers.get(url, 640);
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
        let kind = txt(p.kind, 14.0).font(theme::fw(Medium));
        let info = column![kind, title(app.main_w() - 3.0 * PAD - px), p.meta].spacing(8);
        let hero = row![art, info].spacing(PAD).align_y(iced::Bottom);
        let hero = container(hero).padding(PAD).width(Fill);
        hero.height(px + 2.0 * PAD).style(bg).into()
    };
    let shuffle = bar_btn(i::SHUFFLE, Message::PlayCtx(app.route.clone(), 1));
    let play = play_btn(app, 56.0, &app.route);
    let compact = app.cfg.settings.compact;
    let tables = ["album", "playlist", "liked", "songs"];
    let view = tables
        .contains(&app.route.split(':').next().unwrap_or(""))
        .then(|| {
            let b = theme::sq(i(i::LIST, 20.0).center(), 32.0);
            let b = b.style(theme::link([SUB, TEXT][usize::from(compact)]));
            b.on_press(set(|s, b| s.compact = b, !compact))
        });
    let bar = row![play, shuffle].extend(p.extra).push(view).spacing(PAD);
    let bar = container(bar.align_y(Center)).height(BAR);
    let bar = bar.padding(Padding::new(PAD).top(12).bottom(16));
    let rest = fade(theme::shade(p.color, 0.4), 232.0, column![bar, body]);
    column![hero, rest].into()
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
        let links = names.iter().enumerate().map(|(n, (id, name))| {
            let name = format!("{}{name}", if n > 0 { ", " } else { "" });
            link_btn(bold(name, 14.0), TEXT, go("artist", id)).into()
        });
        let face = cover(app, names[0].0, 24.0, true);
        let lead = row![face, Row::with_children(links)].spacing(8);
        let year = (a.year > 0).then(|| format!("{DOT}{}", a.year));
        let (year, len) = (year.unwrap_or_default(), fmt_long(a.duration));
        let rest = format!("{year}{DOT}{} songs, {len}", a.song_count);
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
        return Some((p, songs, cols(app, false, false, songs.len())));
    }
    if let Some((p, songs)) = app.data::<(Playlist, Vec<Song>)>(key) {
        let len = fmt_long(p.duration);
        let rest = format!("{DOT}{} songs, {len}", p.song_count);
        const ORDER: [&str; 6] = [
            "Custom order",
            "Title",
            "Artist",
            "Album",
            "Date added",
            "Duration",
        ];
        let find: El = match &app.pl_find {
            None => {
                let open = task(|a| {
                    a.pl_find = Some(String::new());
                    iced::widget::operation::focus("plfind")
                });
                let b = theme::sq(i(i::SEARCH, 20.0).center(), 32.0);
                b.style(theme::link(SUB)).on_press(open).into()
            }
            Some(q) => {
                let style = theme::input([ELEVATED; 2], [BG; 2], [0.0; 2], 4.0);
                let t = text_input("Search in playlist", q)
                    .id("plfind")
                    .width(220)
                    .size(14);
                let t = t.on_input(|v| act(move |a| a.pl_find = Some(v.clone())));
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
        let to = |l: &str| ORDER.iter().position(|x| *x == l).expect("an order label") as u8;
        let sort = pick_list(ORDER, Some(ORDER[app.pl_sort as usize]), move |l| {
            act(move |a| a.pl_sort = to(l))
        });
        let sort = sort.text_size(14).font(theme::fw(Normal)).padding([8, 12]);
        let sort = sort.style(theme::pick(true)).menu_style(theme::pick_menu);
        let comment = (!p.comment.is_empty()).then(|| sub(&p.comment, 14.0));
        let meta = column![comment, meta(bold(&p.owner, 14.0), rest)];
        let page = Page {
            art: cover(app, &p.cover, px, false),
            kind: ["Playlist", "Public Playlist"][usize::from(p.public)],
            title: &p.name,
            meta: meta.spacing(8).into(),
            color: head_color(app, &p.cover),
            extra: vec![
                dots(MenuKind::Playlist(p.clone())),
                Space::new().width(Fill).into(),
                find,
                sort.into(),
            ],
            banner: None,
        };
        let songs = app.songs_of(key).map_or(&songs[..], |x| x.0);
        return Some((page, songs, cols(app, true, true, songs.len())));
    }
    let liked = key == "liked";
    let p = app.data::<Pager<Song>>(key);
    let p = p.filter(|_| liked || key.split(':').next() == Some("songs"))?;
    let server = app.session.as_ref().map(|s| &s.server);
    let (user, native) = server.map_or(("", false), |s| (s.user.as_str(), s.native));
    let n = p.total.max(p.items.len());
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
    let (kind, title, color) = match liked {
        true => ("Playlist", "Liked Songs", color!(0x5038a0)),
        false => ("Library", "Songs", color!(0x1e6b52)),
    };
    let art = match liked {
        true => liked_art(px),
        false => art(px, i(i::MUSIC, 0.0).color(ACCENT), SONGS_BG),
    };
    let meta = meta(bold(user, 14.0), count.unwrap_or_default());
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
    Some((page, &p.items, cols(app, true, true, n)))
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
    let y0 = art_px(app) + 2.0 * PAD + BAR + HEAD;
    let body = column![table_head(app, c), vlist(app, songs, &app.route, c, y0)];
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
        let more = shelf(&format!("More by {}", a.artist), all, more);
        column![container(notes).padding([0.0, PAD]), more]
            .spacing(48)
            .padding(padding::bottom(PAD))
    });
    Some(page(app, p, body.push(foot).into()))
}

pub fn sticky<'a>(app: &'a App) -> Option<El<'a>> {
    let off = app.scrolls.get(&app.route).map_or(0.0, |s| s.0);
    let bar = |c: El<'a>, bg: Color| {
        let b = container(c).padding([0.0, PAD]).center_y(64).width(Fill);
        b.style(fill(bg, 0.0))
    };
    if app.route == "lyrics" && app.lyric.1.is_none() {
        let t = bold("Show the current line", 14.0);
        let b = button(t).padding([10, 20]).on_press(Message::Follow);
        let b = b.style(theme::btn([TEXT; 3], [BG; 3], 500.0));
        let b = container(b).center_x(Fill).align_bottom(Fill);
        return Some(b.padding(24).into());
    }
    if let Some((id, f, _, albums)) = disc_args(app) {
        let tools = disc_tools(app, albums, id, f);
        return (off > GRID_Y - 64.0).then(|| bar(tools, theme::PANEL).into());
    }
    let artist = app.data::<(Artist, Vec<Album>)>(&app.route);
    let (title, color, cols) = match artist {
        Some((a, _)) => (&a.name[..], head_color(app, &a.cover), None),
        None => detail_of(app).map(|(p, _, c)| (p.title, p.color, Some(c)))?,
    };
    let top = art_px(app) + 2.0 * PAD;
    let title = ell(title, app.main_w() - 120.0, 24.0, Bold);
    let play = play_btn(app, 48.0, &app.route);
    let head = row![play, title].spacing(8).align_y(Center);
    let head = bar(head.into(), theme::shade(color, 0.5));
    let cols = cols
        .filter(|_| off > top + BAR - 64.0)
        .map(|c| table_head(app, c));
    let cols = cols.map(|c| container(c).style(fill(color!(0x1a1a1a), 0.0)));
    (off > top - 64.0).then(|| column![head].push(cols).into())
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
    let c3 = cols(app, true, false, n);
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
        let m = Message::DiscChip(n as u8);
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
    let similar = shelf("Fans also like", None, similar.map(|x| artist_card(app, x)));
    let appears = app.data::<Vec<Album>>(&format!("appears:{id}"));
    let appears = appears.into_iter().flatten().take(k);
    let appears = appears.map(|x| album_card(app, x, year_kind(x)));
    let about = info.filter(|i| !i.biography.is_empty()).map(|info| {
        let bio = txt(strip_html(&info.biography), 16.0).color(SUB);
        let bio = container(bio).padding(PAD).max_width(672);
        let bio = container(bio.style(fill(ELEVATED, 8.0))).padding([0.0, PAD]);
        column![heading("About", None), bio].spacing(8)
    });
    let popular = (!top.is_empty()).then(|| popular.spacing(8));
    let discog = (!albums.is_empty()).then(|| discog.spacing(12));
    let body = column![popular, discog, similar, shelf("Appears On", None, appears)];
    let body = body.push(about).spacing(40);
    let body = body.padding(padding::bottom(PAD * 2.0));
    let banner = app.data::<String>(&a.banner_key()).map(String::as_str);
    let banner = banner.filter(|u| app.cfg.settings.banners && !u.is_empty());
    let follow = if on { "Following" } else { "Follow" };
    let follow = theme::pill_btn(follow, Message::Star(vec![a.id.clone()], !on));
    let p = Page {
        art: cover(app, &a.cover, art_px(app), true),
        kind: "Artist",
        title: &a.name,
        meta: sub(counts, 14.0).color(TEXT).into(),
        color: head_color(app, &a.cover),
        extra: vec![follow.into(), dots(MenuKind::Artist(a.clone()))],
        banner,
    };
    Some(page(app, p, body.into()))
}

fn strip_html(s: &str) -> String {
    let mut parts = s.split('<');
    let head = parts.next().unwrap_or("").to_string();
    let rest = parts.map(|p| p.split_once('>').map_or("", |x| x.1));
    (head + &rest.collect::<String>()).trim().to_string()
}

fn search<'a>(app: &'a App, q: &str) -> Option<El<'a>> {
    if q.is_empty() {
        let gs = app.data::<Vec<Genre>>("genres")?;
        let cols = ((app.main_w() - 24.0) / 300.0).max(2.0) as usize;
        let tw = (app.main_w() - 24.0) / cols as f32 - 24.0;
        let th = (tw * 0.56).round();
        let tile = |n: usize| -> El<'a> {
            let g = &gs[n];
            let label = bold(&g.name, 24.0).wrapping(text::Wrapping::WordOrGlyph);
            let b = button(label).padding(16).width(tw).height(th).clip(true);
            let b = b.style(theme::btn([theme::color_of(&g.name); 3], [TEXT; 3], 8.0));
            let b = b.on_press(go("genre", &g.name));
            container(b).padding(12).into()
        };
        let title = container(bold("Browse all", 24.0));
        let title = title.padding(Padding::new(PAD).bottom(4));
        return Some(column![title, grid(app, gs.len(), cols, th + 24.0, 64.0, tile)].into());
    }
    let r = app.data::<SearchRes>(&app.route)?;
    if r.artists.is_empty() && r.albums.is_empty() && r.songs.is_empty() {
        let title = bold(format!("No results found for \u{201c}{q}\u{201d}"), 24.0);
        let hint = "Check the spelling, or use fewer or different keywords.";
        let hint = sub(hint, 16.0);
        let col = column![title, hint].spacing(16).align_x(Center);
        return Some(container(col).center_x(Fill).padding(80).into());
    }
    let w = app.main_w() - 3.0 * PAD;
    let lw = (w * 0.4).clamp(280.0, 420.0);
    let ql = q.to_lowercase();
    let art = |a: &'a Artist| {
        (
            &a.cover,
            &a.name,
            "Artist".into(),
            go("artist", &a.id),
            true,
        )
    };
    let album = |x: &'a Album| {
        let s = format!("Album{DOT}{}", x.artist);
        (&x.cover, &x.name, s, go("album", &x.id), false)
    };
    let exact = r.artists.iter().find(|a| a.name.to_lowercase() == ql);
    let best = exact.map(art).or_else(|| r.albums.first().map(album));
    let best = best.or_else(|| r.artists.first().map(art)).or_else(|| {
        let s = r.songs.first()?;
        let sub = format!("Song{DOT}{}", s.artist);
        Some((&s.cover, &s.title, sub, go("album", &s.album_id), false))
    });
    let top = best.map(|(cid, name, s, to, round)| {
        let art = cover(app, cid, 92.0, round);
        let body = column![art, ell(name, lw - 40.0, 32.0, Bold), sub(s, 14.0)];
        let card = color!(0x181818);
        let style = theme::btn([card, theme::MENU, card], [TEXT; 3], 8.0);
        let b = button(body.spacing(12)).padding(20).width(lw).style(style);
        column![bold("Top result", 24.0), b.on_press(to)].spacing(8)
    });
    let sw = w - lw;
    let sc = (sw - 200.0, 0.0, 0.0, 16.0);
    let k = app.grid().0;
    let songs = r.songs.iter().take(4).enumerate();
    let songs = songs.map(|(n, s)| track(app, s, n, &app.route, sc));
    let songs = column![bold("Songs", 24.0)].extend(songs);
    let songs = songs.spacing(8).width(sw);
    let pls = app.data::<Lib>("lib").map_or(&[][..], |l| &l.playlists[..]);
    let pls = pls.iter().filter(|p| p.name.to_lowercase().contains(&ql));
    let pls = pls.take(k);
    let albums = r.albums.iter().take(k);
    let by = |a: &Album| match a.year {
        0 => a.artist.clone(),
        y => format!("{y}{DOT}{}", a.artist),
    };
    let albums = albums.map(|a| album_card(app, a, by(a)));
    let artists = r.artists.iter().take(k).map(|a| artist_card(app, a));
    let best = container(row![top, songs].spacing(PAD)).padding([0.0, PAD]);
    let col = column![
        best,
        shelf("Artists", None, artists),
        shelf("Albums", None, albums),
        shelf("Playlists", None, pls.map(|p| playlist_card(app, p))),
    ];
    Some(col.spacing(40).padding([PAD, 0.0]).into())
}

fn library<'a>(app: &'a App, kind: &str, arg: &str) -> El<'a> {
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
    let chips = (kind == "albums").then(|| Row::with_children(chips).spacing(8).into());
    let title = match kind {
        "genre" => arg,
        "artists" => "Artists",
        _ => "Albums",
    };
    let (cols, cw) = app.grid();
    let g = |n, f: &dyn Fn(usize) -> El<'a>| grid(app, n, cols, cw + 52.0, GRID_Y, f);
    let artists = app.data::<Pager<Artist>>(&app.route).map(|p| &p.items);
    let artists = artists.map(|v| g(v.len(), &|n| artist_card(app, &v[n])));
    let albums = app.data::<Pager<Album>>(&app.route).map(|p| &p.items);
    let albums = albums.map(|v| g(v.len(), &|n| album_card(app, &v[n], v[n].artist.clone())));
    column![grid_head(title, chips), artists.or(albums)].into()
}

fn folders(app: &App) -> El<'_> {
    let w = app.main_w() - 2.0 * PAD - 80.0;
    let dir = |id: &str, name: &str| -> El<'_> {
        let ic = container(i(i::FOLDER, 20.0).color(SUB).center()).center(40);
        let row = media(ic.style(fill(ELEVATED, 4.0)).into(), name, "", w, TEXT);
        let b = button(row).padding([8, 16]).width(Fill);
        let b = b.style(theme::track(false));
        b.on_press(go("folder", id)).into()
    };
    let artists = app.data::<Vec<Artist>>(&app.route).into_iter().flatten();
    let artists = artists.map(|x| dir(&x.id, &x.name));
    let songs = app.data::<Vec<Song>>(&app.route);
    let songs = songs.map_or(&[][..], |v| &v[..]);
    let c = cols(app, false, false, songs.len());
    let songs = songs.iter().enumerate().map(|(n, s)| match s.is_dir {
        true => dir(&s.id, &s.title),
        false => track(app, s, n, &app.route, c),
    });
    let list = Column::new().extend(artists).extend(songs);
    let title = container(bold("Folders", 32.0)).padding(PAD);
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
        return t.height(app.view_size().1 - 160.0).style(bg).into();
    };
    let lag = (app.cfg.settings.lyrics_offset * 1000.0) as i64;
    let at = |t: u64| t as i64 + l.offset + lag;
    let lines = l.lines.iter().enumerate().map(|(n, (t, line))| -> El<'_> {
        let fg = match n {
            _ if !synced || n == cur => TEXT,
            n if n < cur => theme::tint(0.7),
            _ => BG.scale_alpha(0.8),
        };
        let size = app.cfg.settings.lyrics_size;
        let words = bold(if line.is_empty() { "\u{266a}" } else { line }, size);
        match t {
            Some(t) if synced => {
                let seek = Message::Seek(at(*t).max(0) as f32 / 1000.0);
                link_btn(words, fg, seek).into()
            }
            _ => words.color(fg).into(),
        }
    });
    let col = Column::with_children(lines).spacing(16).max_width(900);
    let pad = Padding::new(PAD * 2.0).top(48).bottom(app.scroll().1 * 0.6);
    container(col).padding(pad).width(Fill).style(bg).into()
}

pub fn lyric_scroll(app: &App) -> Option<(usize, f32)> {
    let (l, cur, _) = lyric_at(app, app.current_song()?)?;
    let w = (app.main_w() - 4.0 * PAD).min(900.0);
    let rows = |s: &str| (s.chars().count() as f32 * 17.6 / w).ceil().max(1.0);
    let ys = l.lines[..cur].iter().map(|x| rows(&x.1) * 41.6 + 16.0);
    Some((
        cur,
        (48.0 + ys.sum::<f32>() - app.scroll().1 * 0.3).max(0.0),
    ))
}

fn lyric_at<'a>(app: &'a App, song: &Song) -> Option<(&'a Lyrics, usize, bool)> {
    let all = app.data::<Vec<Lyrics>>(&format!("lyrics:{}", song.id))?;
    let l = all.iter().find(|l| !l.lines.is_empty())?;
    let ms = (app.position * 1000.0) as i64;
    let end = (song.duration * 1000.0) as i64;
    let lag = (app.cfg.settings.lyrics_offset * 1000.0) as i64;
    let at = |t: u64| t as i64 + l.offset + lag;
    let fits = l.lines.iter().filter_map(|x| x.0).all(|t| at(t) <= end);
    let synced = l.synced && (end <= 0 || fits);
    let past = |x: &(Option<u64>, String)| x.0.is_some_and(|t| at(t) <= ms);
    let cur = l.lines.iter().rposition(past).filter(|_| synced);
    Some((l, cur.unwrap_or(0), synced))
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
    let p = p.text_size(14).font(theme::fw(Normal)).padding([8, 12]);
    let p = p.width(268).style(theme::pick(false));
    p.menu_style(theme::pick_menu).into()
}

fn settings(app: &App) -> El<'_> {
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
    let head = |t: &str| container(bold(t, 16.0)).padding(padding::top(PAD).bottom(8));
    let line = |label: &str, c: El<'static>| -> El<'_> {
        let label = txt(label, 14.0).color(SUB).width(Fill);
        row![label, c].align_y(Center).height(48).into()
    };
    let devices = app.data::<Vec<(String, String)>>("devices");
    let default = (String::new(), "System default".to_string());
    let devices = devices.into_iter().flatten().cloned();
    let devices = std::iter::once(default).chain(devices).collect();
    let mut rates = [0, 320, 256, 192, 128, 96].map(|b| (b, format!("{b} kbps")));
    rates[0].1 = "No limit".into();
    let zooms = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5].map(|z| (z, format!("{:.0}%", z * 100.0)));
    let gains = opts(&["off", "Off", "track", "Track", "album", "Album"]);
    let formats = opts(&["", "Original", "mp3", "MP3", "opus", "Opus", "aac", "AAC"]);
    let (tf, br) = (st.transcode_format.as_str(), st.transcode_max_bitrate);
    let (x, secs) = (st.crossfade_secs, format!("{:.0} s", st.crossfade_secs));
    let device = pick(devices, st.device.as_str(), |s, v| s.device = v);
    let gain = pick(gains, st.replaygain.as_str(), |s, v| s.replaygain = v);
    let preamp = db(st.preamp_db, |s, v| s.preamp_db = v);
    let fallback = db(st.fallback_db, |s, v| s.fallback_db = v);
    let fade = num(x, 0.0..=12.0, |s, v| s.crossfade_secs = v, secs);
    let clip = tog(st.rg_clip, |s, b| s.rg_clip = b);
    let (pct, step) = (st.scrobble_pct, st.seek_secs);
    let (lag, size) = (st.lyrics_offset, st.lyrics_size);
    let (l1, l2) = (format!("{pct:.0}%"), format!("{step:.0} s"));
    let secs = st.scrobble_secs;
    let pct = match st.scrobble_by_time {
        true => num(
            secs,
            10.0..=600.0,
            |s, v| s.scrobble_secs = v,
            format!("{secs:.0} s"),
        ),
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
    let ratings = tog(st.ratings, |s, b| s.ratings = b);
    let confirm = tog(st.confirm_queue, |s, b| s.confirm_queue = b);
    let skip = text_input("live, remix, intro", &st.queue_skip);
    let skip = skip
        .on_input(|v| set(|s, v| s.queue_skip = v, v))
        .width(268);
    let skip = skip.style(theme::input([ELEVATED; 2], [GREY; 2], [0.0, 1.0], 4.0));
    let skip = skip
        .font(theme::fw(Normal))
        .padding([8, 12])
        .size(14)
        .into();
    let follow = tog(st.lyrics_follow, |s, b| s.lyrics_follow = b);
    let start_min = tog(st.start_min, |s, b| s.start_min = b);
    let min_tray = tog(st.min_tray, |s, b| s.min_tray = b);
    let step = num(step, 1.0..=30.0, |s, v| s.seek_secs = v, l2);
    let (l1, l2) = (format!("{lag:+.1} s"), format!("{size:.0} px"));
    let lag = num(lag, -5.0..=5.0, |s, v| s.lyrics_offset = v, l1);
    let size = num(size, 20.0..=48.0, |s, v| s.lyrics_size = v, l2);
    let format = pick(formats, tf, |s, v| s.transcode_format = v);
    let rate = pick(rates.into(), br, |s, v| s.transcode_max_bitrate = v);
    let sync = tog(st.queue_sync, |s, b| s.queue_sync = b);
    let scrobble = tog(st.scrobble, |s, b| s.scrobble = b);
    let banners = tog(st.banners, |s, b| s.banners = b);
    let banners_label =
        "Show artist banners from TheAudioDB (this sends artist names to TheAudioDB)";
    let zoom = pick(zooms.into(), st.zoom, |s, v| s.zoom = v);
    let frame = tog(st.system_frame, |s, b| s.system_frame = b);
    let frame_label = "Use the system title bar on Wayland desktops that do not draw one";
    let frame_label = format!("{frame_label} (restart to apply)");
    let frame = cfg!(target_os = "linux").then(|| line(&frame_label, frame));
    let tray_label = "Keep Navify in the system tray when you close the window";
    let none = [" (no system tray found)", ""][usize::from(app.tray.is_some())];
    let tray_label = format!("{tray_label}{none}");
    let tray = line(&tray_label, tog(st.tray, |s, b| s.tray = b));
    let server = |s: &Server| -> El<'_> {
        let active = app.cfg.active.as_ref() == Some(&s.id);
        let who = sub(format!("{}{DOT}{}", s.user, s.url), 14.0);
        let info = column![txt(&s.name, 14.0), who].spacing(2).width(Fill);
        let out = active.then(|| theme::pill_btn("Log out", Message::LogOut));
        row![info, out].align_y(Center).height(56).into()
    };
    let mb = |b: u64| b as f32 / 1_048_576.0;
    let disk = mb(app.data::<u64>("disk").copied().unwrap_or(0));
    let memory = mb(app.covers.bytes as u64);
    let cache = format!("{disk:.0} MB on disk, {memory:.1} MB in memory");
    let cache = sub(cache, 14.0);
    let clear = theme::pill_btn("Clear", Message::ClearCache);
    let cache = row![cache, clear].spacing(16).align_y(Center).into();
    let version = sub(env!("CARGO_PKG_VERSION"), 14.0).into();
    let col = column![
        bold("Settings", 32.0),
        head("Playback"),
        line("Output device", device),
        line("ReplayGain", gain),
        line("Preamp", preamp),
        line("Gain for tracks without ReplayGain", fallback),
        line("Limit the gain to stop clipping", clip),
        line("Crossfade", fade),
        line("Seek step for keys and media controls", step),
        line("Action of the play buttons and double-click", mode),
        head("Queue"),
        line("Ask before you replace the queue", confirm),
        line(
            "Do not add songs with these words (separate with commas)",
            skip
        ),
        head("Streaming"),
        line("Transcode format", format),
        line("Maximum bitrate", rate),
        head("Library"),
        line("Sync the play queue with the server", sync),
        line("Send played tracks to the server (scrobble)", scrobble),
        line("Scrobble by", by),
        line("Scrobble after", pct),
        head("Display"),
        line("Zoom", zoom),
        line(banners_label, banners),
        line("Show the rating column", ratings),
        line("Scroll the lyrics with the track", follow),
        line("Lyrics offset (positive shows the lines later)", lag),
        line("Lyrics text size", size),
    ];
    let col = col.push(frame).push(tray);
    let col = col.push(line("Start minimized", start_min));
    let notify = tog(st.notify, |s, b| s.notify = b);
    let col = col.push(line("Show a notification when a new track starts", notify));
    let awake = tog(st.awake, |s, b| s.awake = b);
    let col = col.push(line("Keep the computer awake while music plays", awake));
    let col = col.push(line(
        "Hide Navify in the tray when you minimize the window",
        min_tray,
    ));
    let col = col.push(head("Keyboard shortcuts"));
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
            b.on_press(Message::Record((!on).then_some(k.0))).into(),
        )
    }));
    let reset = theme::pill_btn("Reset", set(|s, ()| s.keys.clear(), ()));
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
                match serde_json::from_str::<Settings>(t.as_deref().unwrap_or_default()) {
                    Ok(mut new) => {
                        let s = &a.cfg.settings;
                        (new.window, new.device) = (s.window, s.device.clone());
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
    let copy = theme::pill_btn("Copy", copy);
    let paste = theme::pill_btn("Paste", paste);
    let io = row![copy, paste].spacing(8).into();
    let col = col.push(line(
        "Copy the settings to the clipboard, or paste them from it",
        io,
    ));
    let col = col.push(head("About"));
    let col = col.push(line("Version", version)).max_width(860);
    container(col).padding(PAD).into()
}

pub fn now_playing<'a>(app: &'a App) -> El<'a> {
    let w = app.right_w() - 32.0;
    let Some(s) = app.current_song() else {
        let t = sub("Nothing is playing.", 14.0);
        return container(t).padding(16).into();
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
    let title = btn(title, go("album", &s.album_id));
    let next = app.queue.get(app.real_at(app.index + 1)).map(|n| {
        let open = link_btn(bold("Open queue", 14.0), SUB, Message::Panel(2));
        let head = row![bold("Next in queue", 16.0).width(Fill), open];
        card(column![head, queue_row(app, n, app.index + 1, w - 32.0)])
    });
    let lines = column![title, artists_line(&s.artists, 16.0, w - 40.0)].spacing(4);
    let info = row![lines.width(Fill), heart_btn(&s.id, starred, 20.0)].align_y(Center);
    let about = card(column![bold("About the artist", 16.0), artist]);
    let preview = lyric_at(app, s).map(|(l, cur, _)| {
        let lines = l.lines[cur..]
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
    let head = panel_head(ell(&app.source, app.right_w() - 64.0, 16.0, Bold), 1, None);
    column![head, scrollable(body).style(theme::scroll)].into()
}

fn panel_head<'a>(title: impl Into<El<'a>>, n: u8, extra: Option<El<'a>>) -> El<'a> {
    let title = container(title).width(Fill);
    let x = icon_btn(i::X, 16.0, Message::Panel(n));
    let r = row![title].push(extra).push(x).spacing(8).align_y(Center);
    r.padding(Padding::new(8.0).left(16)).into()
}

fn queue_row<'a>(app: &'a App, s: &'a Song, n: usize, w: f32) -> El<'a> {
    let cur = n == app.index;
    let art = cover(app, &s.cover, 48.0, false);
    let x = (!cur).then(|| icon_btn(i::X, 14.0, Message::QDel(n)));
    let fg = if cur { ACCENT } else { TEXT };
    let r = media(art, &s.title, &s.artist, w - 100.0, fg).push(x);
    let menu = Message::Menu(Box::new(MenuKind::Song(vec![s.clone()])));
    let r = mouse_area(container(r).padding(8)).on_double_click(Message::PlayQueue(n));
    r.on_right_press(menu).into()
}

pub fn queue_panel(app: &App) -> El<'_> {
    let w = app.right_w() - 16.0;
    let next = (app.index + 1..app.queue.len()).take(100);
    let next = next.filter_map(|n| app.queue.get(app.real_at(n)).map(|s| (n, s)));
    let next: Vec<El> = next.map(|(n, s)| queue_row(app, s, n, w)).collect();
    let more = app.queue.len().saturating_sub(app.index + 101);
    let title = |t: String| {
        let t = container(ell(&t, w - 16.0, 16.0, Bold));
        t.padding(Padding::new(8.0).top(16))
    };
    let note = |t: String| container(sub(t, 14.0)).padding(8);
    let from = format!("Next from: {}", app.source);
    let from = [&from, "Next up"][usize::from(app.source.is_empty())].to_string();
    let now = app.current_song().map(|s| queue_row(app, s, app.index, w));
    let now = now.map(|r| column![title("Now playing".into()), r]);
    let list = column![now, (!next.is_empty()).then(|| title(from))].extend(next);
    let list = list.push((more > 0).then(|| note(format!("{more} more songs"))));
    let empty = app.queue.is_empty();
    let list = list.push(empty.then(|| note("The queue is empty.".into())));
    let tab = |t: &'static str, on: bool| {
        let edge = container(Space::new().width(Fill).height(2));
        let edge = edge.style(fill([CLEAR, ACCENT][usize::from(on)], 1.0));
        let b = column![bold(t, 16.0).wrapping(text::Wrapping::None), edge].spacing(4);
        let b = button(b)
            .padding(0)
            .style(theme::link([SUB, TEXT][usize::from(on)]));
        b.on_press(act(move |a| a.recent_tab = t != "Queue"))
    };
    let past = app.recent.iter().map(|s| {
        let play = Message::Play(vec![s.clone()], 0, "Recently played".into());
        let r = media(
            cover(app, &s.cover, 48.0, false),
            &s.title,
            &s.artist,
            w - 100.0,
            TEXT,
        );
        let menu = Message::Menu(Box::new(MenuKind::Song(vec![s.clone()])));
        let r = mouse_area(container(r).padding(8)).on_double_click(play);
        r.on_right_press(menu).into()
    });
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
    let clear = link_btn(bold("Clear queue", 14.0), SUB, Message::QClear);
    let clear = (!empty && !app.recent_tab).then(|| clear.into());
    let head = panel_head(tabs.spacing(24), 2, clear);
    if app.recent_tab {
        return column![head, scrollable(past).style(theme::scroll)].into();
    }
    let list = list.padding(Padding::new(8.0).top(0));
    column![head, scrollable(list).style(theme::scroll)].into()
}

type Item = Option<(char, String, Message, bool)>;

pub fn menu<'a>(app: &'a App, m: &'a Menu) -> (El<'a>, f32, f32) {
    const W: f32 = 260.0;
    let entry = |r: Row<'a, Message>, msg: Message| -> El<'a> {
        let hover = [theme::CLEAR, theme::MENU_HOVER, theme::MENU_HOVER];
        let b = button(container(r.spacing(12).align_y(Center)).center_y(Fill));
        let b = b.padding([0, 12]).height(40).width(Fill);
        let b = b.style(theme::btn(hover, [TEXT; 3], 2.0));
        focusable(b.on_press(msg.clone()), Some(msg), 2.0, Some("layer"))
    };
    let boxed = |el: El<'a>, w: f32, h: f32, pad: u16| {
        let c = container(el).padding(pad).width(w).height(h);
        (c.style(theme::menu).into(), w, h)
    };
    let it = |c: char, l: &str, m: Message| Some((c, l.to_string(), m, false));
    let open = |n: u8, c: char, l: &str| {
        let m = act(move |a| a.menu.iter_mut().for_each(|m| m.sub = n));
        Some((c, l.to_string(), m, true))
    };
    let user = app.session.as_ref().map(|s| &s.server);
    let mine = |p: &Playlist| user.is_some_and(|s| s.user == p.owner);
    let star = |id: &str, server: bool, yes: &str, no: &str| {
        let k = usize::from(app.is_starred(id, server));
        it(
            [i::SAVE, i::SAVED][k],
            [no, yes][k],
            Message::Star(vec![id.into()], k == 0),
        )
    };
    let ctx = |kind: &str, id: &str| {
        let r = format!("{kind}:{id}");
        let p = |n| Message::PlayCtx(r.clone(), n);
        vec![
            it(i::PLAY, "Play", p(0)),
            it(i::SHUFFLE, "Shuffle", p(1)),
            it(i::LISTPLUS, "Add to queue", p(2)),
        ]
    };
    let ids = |s: &[Song]| s.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
    let mut head: Option<El> = None;
    let list: Vec<Item> = match &m.kind {
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
        MenuKind::Device => {
            head = Some(
                container(bold("Select an output device", 14.0))
                    .padding([0, 12])
                    .center_y(40)
                    .into(),
            );
            let found = app
                .data::<Vec<(String, String)>>("devices")
                .into_iter()
                .flatten();
            let all =
                std::iter::once((String::new(), "System default".into())).chain(found.cloned());
            all.map(|(id, name)| {
                let c = [i::DEVICE, i::CHECK][usize::from(id == app.cfg.settings.device)];
                it(c, &name, set(|s, v| s.device = v, id))
            })
            .collect()
        }
        MenuKind::User => {
            let url = ell(user.map_or("", |s| &s.url), W - 32.0, 12.0, Normal);
            let name = bold(user.map_or("", |s| &s.user), 14.0);
            let who = column![name, url.color(SUB)].spacing(2);
            head = Some(container(who).padding([0, 12]).center_y(40).into());
            let active = |s: &&Server| app.cfg.active.as_ref() != Some(&s.id);
            let others = app.cfg.servers.iter().filter(active).map(|s| {
                let label = format!("Switch to {}", s.name);
                it(i::SERVER, &label, Message::UseServer(s.id.clone()))
            });
            let settings = it(i::SETTINGS, "Settings", Message::Nav("settings".into()));
            let out = it(i::LOGOUT, "Log out", Message::LogOut);
            others.chain([settings, None, out]).collect()
        }
        MenuKind::Song(s) | MenuKind::SongIn(s, _) if m.sub == 1 => {
            let lib = app.data::<Lib>("lib");
            let n = lib.map_or(0, |l| l.playlists.len()) + 1;
            let name = match &s[..] {
                [x] => x.title.clone(),
                _ => format!("My Playlist #{n}"),
            };
            let pls = lib.into_iter().flat_map(|l| l.playlists.iter());
            let pls = pls.filter(|p| mine(p));
            let add = |p: &Playlist| it(i::MUSIC, &p.name, Message::PlAdd(p.id.clone(), ids(s)));
            let new = it(i::PLUS, "New playlist", Message::PlNew(name, ids(s)));
            [new, None].into_iter().chain(pls.map(add)).collect()
        }
        MenuKind::Song(songs) | MenuKind::SongIn(songs, _) => {
            let on = songs.iter().all(|s| app.is_starred(&s.id, s.starred));
            let like = ["Save to your Liked Songs", "Remove from your Liked Songs"];
            let q = |next| Message::Queue(songs.clone(), next);
            let mut v = vec![
                it(i::LISTPLUS, "Add to queue", q(false)),
                it(i::QUEUE, "Play next", q(true)),
                None,
                open(1, i::PLUS, "Add to playlist"),
            ];
            if let MenuKind::SongIn(_, n) = &m.kind {
                let rm = Message::PlRemove(n.clone());
                v.push(it(i::TRASH, "Remove from this playlist", rm));
            }
            let (star, k) = (Message::Star(ids(songs), !on), usize::from(on));
            v.push(it([i::SAVE, i::SAVED][k], like[k], star));
            if let [s] = &songs[..] {
                let radio = Message::Radio(s.id.clone());
                let radio = it(i::RADIO, "Go to song radio", radio);
                v.extend([open(2, i::STAR, "Rate"), None, radio]);
                let links = [("artist", &s.artist_id), ("album", &s.album_id)];
                let links = links.into_iter().filter(|(_, id)| !id.is_empty());
                v.extend(links.map(|(k, id)| {
                    let c = if k == "artist" { i::USER } else { i::ALBUM };
                    it(c, &format!("Go to {k}"), go(k, id))
                }));
            }
            v
        }
        MenuKind::Album(a) => {
            let mut v = ctx("album", &a.id);
            let save = ["Remove from Your Library", "Save to Your Library"];
            v.push(star(&a.id, a.starred, save[0], save[1]));
            let artist = it(i::USER, "Go to artist", go("artist", &a.artist_id));
            v.extend((!a.artist_id.is_empty()).then_some(artist));
            v
        }
        MenuKind::Artist(a) => {
            let mut v = ctx("artist", &a.id);
            v.push(star(&a.id, a.starred, "Unfollow", "Follow"));
            let radio = Message::Radio(a.id.clone());
            v.push(it(i::RADIO, "Go to artist radio", radio));
            v
        }
        MenuKind::Playlist(p) => {
            let mut v = ctx("playlist", &p.id);
            let edit = it(i::PENCIL, "Edit details", Message::Modal(p.id.clone(), 1));
            let del = it(i::TRASH, "Delete", Message::Modal(p.id.clone(), 2));
            v.extend(mine(p).then_some([edit, None, del]).into_iter().flatten());
            v
        }
    };
    let seps = list.iter().filter(|x| x.is_none()).count();
    let n = list.len() - seps + usize::from(head.is_some());
    let h = (n as f32 * 40.0 + seps as f32 + 8.0).min(400.0);
    let rows = list.into_iter().map(|x| match x {
        Some((c, l, msg, sub)) => {
            let label = ell(&l, W - if sub { 92.0 } else { 64.0 }, 14.0, Normal);
            let arrow = sub.then(|| i(i::RIGHT, 16.0));
            let r = row![i(c, 16.0).color(SUB), label.width(Fill)];
            entry(r.push(arrow), msg)
        }
        None => rule(),
    });
    let list = scrollable(Column::new().push(head).extend(rows));
    boxed(list.style(theme::scroll).into(), W, h, 4)
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
        let first = s.user.chars().next().unwrap_or('?').to_ascii_uppercase();
        let avatar = container(bold(first, 16.0).color(BG).center()).center(40);
        let avatar = avatar.style(fill(theme::color_of(&first.to_string()), 20.0));
        let drop = Message::DropServer(s.id.clone());
        let drop = focusable(icon_btn(i::X, 14.0, drop.clone()), Some(drop), 14.0, None);
        let body = media(avatar.into(), &s.name, &s.user, W - 120.0, TEXT).push(drop);
        let pick = Message::UseServer(s.id.clone());
        let b = button(body).padding(8).width(Fill);
        let b = b.style(theme::track(false));
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
    let label = bold(if l.busy { "Logging in" } else { "Log In" }, 16.0).color(BG);
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
