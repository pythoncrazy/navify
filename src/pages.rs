use iced::font::Weight;
use iced::widget::{
    Column, Row, Space, button, column, container, hover, mouse_area, pick_list, row, scrollable,
    slider, stack, text, text_input, toggler,
};
use iced::{Alignment, Color, Element, Fill, color};

use crate::api::{Album, Artist, Genre, Info, Lyrics, Playlist, Song};
use crate::app::{App, Home, Lib, Menu, MenuKind, Message, Pager, SearchRes, Set};
use crate::theme::{self, fill, icon, icon as i};
use crate::view::{
    SONGS_BG, artists_line, btn, cover, fmt_dur, fmt_long, fmt_n, focusable, icon_btn, liked_art,
    pin_art,
};

type El<'a> = Element<'a, Message>;
type Cols = (f32, f32, f32);

pub const PAD: f32 = 24.0;
pub const ROW: f32 = 56.0;
pub const GRID_Y: f32 = 136.0;
pub const BAR: f32 = 104.0;
pub const HEAD: f32 = 45.0;
const GREY: Color = color!(0x535353);

pub fn head_color(app: &App, id: &str) -> Color {
    app.covers.color.get(id).copied().unwrap_or(GREY)
}

pub fn art_px(app: &App) -> f32 {
    (128.0 + (app.main_w() - 600.0) * 104.0 / 424.0).clamp(128.0, 232.0)
}

fn cols(app: &App, album: bool, date: bool) -> Cols {
    let w = app.main_w() - 2.0 * PAD - 32.0 - 16.0 - 16.0 - 120.0 - 16.0;
    let (album, date) = (album && w > 480.0, date && w > 640.0);
    let w = w - 16.0 * (album as u8 + date as u8) as f32;
    let fr = w / (6.0 + if album { 4.0 } else { 0.0 } + if date { 3.0 } else { 0.0 });
    let (a, d) = (if album { fr * 4.0 } else { 0.0 }, if date { fr * 3.0 } else { 0.0 });
    (w - a - d, a, d)
}

fn play_btn<'a>(app: &App, px: f32, ctx: &str, m: Message) -> El<'a> {
    let on = app.playing && app.ctx == ctx;
    button(
        theme::filled(if on { fill::PAUSE } else { fill::PLAY }, px * 0.4)
            .color(theme::BG)
            .center(),
    )
    .width(px)
    .height(px)
    .padding(0)
    .style(theme::accent)
    .on_press(if on { Message::Toggle } else { m })
    .into()
}

fn play_over<'a>(app: &App, base: El<'a>, top: El<'a>, ctx: &str) -> El<'a> {
    if app.playing && app.ctx == ctx { stack![base, top].into() } else { hover(base, top) }
}

fn heading<'a>(t: &str, all: Option<String>) -> El<'a> {
    let r = row![theme::bold(t, 24.0), Space::new().width(Fill)].align_y(Alignment::Center);
    container(r.push(all.map(|r| {
        button(theme::bold("Show all", 14.0))
            .padding(0)
            .style(theme::link(theme::SUB))
            .on_press(Message::Nav(r))
    })))
    .padding([0.0, PAD])
    .into()
}

fn sep<'a>() -> El<'a> {
    container(Space::new()).width(Fill).height(1).style(theme::fill(theme::tint(0.1), 0.0)).into()
}

fn table_head<'a>(c: Cols) -> El<'a> {
    let h = |t: &'static str, w: f32| (w > 0.0).then(|| theme::sub(t, 14.0).width(w));
    container(column![
        row![theme::sub("#", 16.0).width(16).center()]
            .push(h("Title", c.0))
            .push(h("Album", c.1))
            .push(h("Date added", c.2))
            .push(
                container(i(icon::CLOCK, 16.0).color(theme::SUB))
                    .width(120)
                    .align_x(Alignment::End)
                    .padding(theme::pad(0.0, 52.0, 0.0, 0.0))
            )
            .spacing(16)
            .padding([0, 16])
            .height(36)
            .align_y(Alignment::Center),
        sep(),
    ])
    .padding(theme::pad(0.0, PAD, 8.0, PAD))
    .into()
}

fn track<'a>(app: &'a App, s: &'a Song, n: usize, key: &'a str, c: Cols) -> El<'a> {
    let hk = format!("{key}#{n}");
    let hot = app.hover.as_deref() == Some(hk.as_str());
    let sel = app.sel.as_ref().is_some_and(|x| x.0 == key) && app.picked.contains(&n);
    let cur = app.current_song().is_some_and(|x| x.id == s.id);
    let starred = app.is_starred(&s.id, s.starred);
    let art = !key.starts_with("album:");
    let idx: El = match (hot, cur && app.playing) {
        (true, true) => {
            btn(theme::filled(fill::PAUSE, 16.0).center(), Message::Toggle).width(16).into()
        }
        (true, false) => {
            btn(theme::filled(fill::PLAY, 16.0).center(), Message::PlayAt(key.into(), n))
                .width(16)
                .into()
        }
        (false, true) => i(icon::EQ, 16.0).color(theme::ACCENT).width(16).center().into(),
        _ => theme::txt(n + 1, 16.0, Weight::Normal)
            .color(if cur { theme::ACCENT } else { theme::SUB })
            .width(16)
            .center()
            .into(),
    };
    let tw = c.0 - if art { 52.0 } else { 0.0 };
    let title = column![
        theme::ell(&s.title, tw, 16.0, Weight::Normal).color(if cur {
            theme::ACCENT
        } else {
            theme::TEXT
        }),
        artists_line(&s.artists, 14.0, tw),
    ]
    .spacing(2);
    let date = || {
        const M: [&str; 12] =
            ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        let d: Vec<usize> =
            s.created.get(..10).unwrap_or("").split('-').filter_map(|x| x.parse().ok()).collect();
        match d[..] {
            [y, m @ 1..=12, day] => format!("{} {day}, {y}", M[m - 1]),
            _ => String::new(),
        }
    };
    let heart = (hot || starred).then(|| {
        button(theme::heart(starred, 16.0).center())
            .width(32)
            .height(32)
            .padding(0)
            .style(theme::link(if starred { theme::ACCENT } else { theme::SUB }))
            .on_press(Message::Star(vec![s.id.clone()], !starred))
    });
    let last = row![
        container(heart).width(32),
        theme::sub(fmt_dur(s.duration), 14.0).width(56).align_x(Alignment::End),
        container(hot.then(|| icon_btn(icon::DOTS, 16.0, Message::RowMenu(key.into(), n))))
            .width(32),
    ]
    .align_y(Alignment::Center);
    let r = row![
        idx,
        row![art.then(|| cover(app, &s.cover, 128, 40.0, false)), title]
            .spacing(12)
            .align_y(Alignment::Center)
            .width(c.0)
    ]
    .push((c.1 > 0.0).then(|| {
        container(
            button(theme::ell(&s.album, c.1, 14.0, Weight::Normal))
                .padding(0)
                .style(theme::link(theme::SUB))
                .on_press(Message::Nav(format!("album:{}", s.album_id))),
        )
        .width(c.1)
    }))
    .push((c.2 > 0.0).then(|| theme::sub(date(), 14.0).width(c.2)))
    .push(last)
    .spacing(16)
    .padding([0, 16])
    .height(ROW)
    .align_y(Alignment::Center);
    let a = if sel {
        0.3
    } else if hot {
        0.1
    } else {
        0.0
    };
    mouse_area(container(r).style(theme::fill(theme::tint(a), 4.0)))
        .on_press(Message::Select(key.into(), n))
        .on_double_click(Message::PlayAt(key.into(), n))
        .on_right_press(Message::RowMenu(key.into(), n))
        .on_enter(Message::Hover(hk.clone(), true))
        .on_exit(Message::Hover(hk, false))
        .into()
}

fn vlist<'a>(app: &'a App, songs: &'a [Song], key: &'a str, c: Cols, y0: f32) -> El<'a> {
    let discs = key.starts_with("album:") && songs.iter().any(|s| s.disc != songs[0].disc);
    let rows: Vec<Result<usize, u32>> = songs
        .iter()
        .enumerate()
        .flat_map(|(n, s)| {
            (discs && (n == 0 || songs[n - 1].disc != s.disc))
                .then_some(Err(s.disc))
                .into_iter()
                .chain([Ok(n)])
        })
        .collect();
    let (off, view) = app.scroll();
    let lo = (((off - y0) / ROW).max(0.0) as usize).saturating_sub(6).min(rows.len());
    let hi = (lo + (view / ROW) as usize + 13).min(rows.len());
    container(
        Column::new()
            .push(Space::new().height(lo as f32 * ROW))
            .extend(rows[lo..hi].iter().map(|r| {
                match *r {
                    Ok(n) => track(app, &songs[n], n, key, c),
                    Err(d) => row![
                        i(icon::ALBUM, 16.0).color(theme::SUB).width(16).center(),
                        theme::bold(format!("Disc {d}"), 16.0).color(theme::SUB)
                    ]
                    .spacing(16)
                    .padding([0, 16])
                    .height(ROW)
                    .align_y(Alignment::Center)
                    .into(),
                }
            }))
            .push(Space::new().height((rows.len() - hi) as f32 * ROW)),
    )
    .padding(theme::pad(0.0, PAD, PAD, PAD))
    .into()
}

fn card<'a>(
    app: &App,
    cid: &str,
    title: &str,
    sub: String,
    route: String,
    round: bool,
    play: Option<Message>,
) -> El<'a> {
    let cw = app.grid().1;
    let img = cw - 24.0;
    let art = cover(app, cid, 300, img, round);
    let art: El = match play {
        Some(m) => play_over(
            app,
            art,
            container(play_btn(app, 48.0, &route, m))
                .align_right(Fill)
                .align_bottom(Fill)
                .padding(8)
                .into(),
            &route,
        ),
        None => art,
    };
    button(
        column![
            art,
            column![
                theme::ell(title, img, 16.0, Weight::Normal),
                theme::ell(&sub, img, 14.0, Weight::Normal).color(theme::SUB)
            ]
            .spacing(4)
        ]
        .spacing(8),
    )
    .padding(12)
    .width(cw)
    .height(cw + 52.0)
    .style(theme::card)
    .on_press(Message::Nav(route))
    .into()
}

fn album_card<'a>(app: &App, a: &Album, sub: String) -> El<'a> {
    let r = format!("album:{}", a.id);
    card(app, &a.cover, &a.name, sub, r.clone(), false, Some(Message::PlayCtx(r, 0)))
}

fn artist_card<'a>(app: &App, a: &Artist) -> El<'a> {
    card(app, &a.cover, &a.name, "Artist".into(), format!("artist:{}", a.id), true, None)
}

fn shelf<'a>(title: &str, all: Option<String>, cards: Vec<El<'a>>) -> El<'a> {
    if cards.is_empty() {
        return Space::new().into();
    }
    column![heading(title, all), container(Row::with_children(cards)).padding([0, 12])]
        .spacing(8)
        .into()
}

fn grid<'a>(
    app: &App,
    n: usize,
    cols: usize,
    row_h: f32,
    y0: f32,
    f: impl Fn(usize) -> El<'a>,
) -> El<'a> {
    let rows = n.div_ceil(cols);
    let (off, view) = app.scroll();
    let lo = (((off - y0) / row_h).max(0.0) as usize).saturating_sub(1).min(rows);
    let hi = (lo + (view / row_h) as usize + 3).min(rows);
    container(
        Column::new()
            .push(Space::new().height(lo as f32 * row_h))
            .extend(
                (lo..hi).map(|r| {
                    Row::with_children((r * cols..(r * cols + cols).min(n)).map(&f)).into()
                }),
            )
            .push(Space::new().height((rows - hi) as f32 * row_h)),
    )
    .padding(theme::pad(0.0, 12.0, PAD, 12.0))
    .into()
}

fn grid_head<'a>(title: &str, chips: Option<El<'a>>) -> El<'a> {
    container(column![theme::bold(title, 32.0)].push(chips).spacing(16))
        .padding(theme::pad(PAD, PAD, 0.0, PAD))
        .height(GRID_Y)
        .into()
}

fn state<'a>(app: &'a App, key: &str) -> Option<El<'a>> {
    if let Some(e) = app.data::<String>(key) {
        return Some(
            container(
                column![
                    theme::bold("Could not load this page.", 24.0),
                    theme::sub(e, 14.0),
                    button(theme::bold("Try again", 14.0))
                        .style(theme::pill)
                        .padding([8, 16])
                        .on_press(Message::Retry),
                ]
                .spacing(16)
                .align_x(Alignment::Center),
            )
            .center_x(Fill)
            .padding(80)
            .into(),
        );
    }
    if app.cache.contains_key(key) {
        return None;
    }
    let blk = |w: f32, h: f32, r: f32| {
        container(Space::new()).width(w).height(h).style(theme::fill(theme::CHIP, r))
    };
    let px = art_px(app);
    Some(
        column![
            row![
                blk(px, px, 6.0),
                column![blk(80.0, 14.0, 4.0), blk(360.0, 56.0, 4.0), blk(220.0, 14.0, 4.0)]
                    .spacing(12)
            ]
            .spacing(PAD)
            .align_y(Alignment::End),
            blk(56.0, 56.0, 28.0),
        ]
        .extend((0..6).map(|_| {
            row![
                blk(40.0, 40.0, 4.0),
                column![blk(240.0, 14.0, 4.0), blk(140.0, 12.0, 4.0)].spacing(8)
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
        }))
        .spacing(PAD)
        .padding(PAD)
        .into(),
    )
}

pub fn route<'a>(app: &'a App) -> El<'a> {
    let (kind, arg) = app.route.split_once(':').unwrap_or((app.route.as_str(), ""));
    if (!matches!(kind, "settings" | "lyrics" | "search" | "genres" | "login")
        || (kind == "search" && !arg.is_empty()))
        && let Some(s) = state(app, &app.route)
    {
        return s;
    }
    match kind {
        "home" => home(app),
        "album" | "playlist" | "liked" | "songs" => detail(app),
        "artist" => artist(app),
        "search" | "genres" => search(app, arg),
        "albums" | "genre" => albums(app, kind, arg),
        "artists" => artists(app),
        "folders" | "folder" => folders(app),
        "lyrics" => lyrics(app),
        "settings" => settings(app),
        _ => container(theme::sub("This page does not exist.", 14.0)).padding(40).into(),
    }
}

fn home<'a>(app: &'a App) -> El<'a> {
    let Some(h) = app.data::<Home>("home") else { return Space::new().into() };
    let pls = app.data::<Lib>("lib").map_or(&[][..], |l| &l.playlists[..]);
    let w = app.main_w() - 2.0 * PAD;
    let n = if w >= 720.0 { 4 } else { 2 };
    let tw = (w - 8.0 * (n - 1) as f32) / n as f32;
    let tile = |art: El<'a>, name: &str, route: String, play: Message| -> El<'a> {
        play_over(
            app,
            button(
                row![art, theme::ell(name, tw - 56.0 - 24.0, 14.0, Weight::Bold)]
                    .spacing(12)
                    .align_y(Alignment::Center),
            )
            .padding(0)
            .width(tw)
            .height(56)
            .clip(true)
            .style(theme::tile)
            .on_press(Message::Nav(route.clone()))
            .into(),
            container(play_btn(app, 32.0, &route, play))
                .align_right(Fill)
                .center_y(56)
                .padding([0, 8])
                .into(),
            &route,
        )
    };
    let tiles: Vec<El> = std::iter::once(tile(
        liked_art(56.0),
        "Liked Songs",
        "liked".into(),
        Message::PlayCtx("liked".into(), 0),
    ))
    .chain(pls.iter().take(3).map(|p| {
        let r = format!("playlist:{}", p.id);
        tile(cover(app, &p.cover, 128, 56.0, false), &p.name, r.clone(), Message::PlayCtx(r, 0))
    }))
    .chain(h.recent.iter().map(|a| {
        let r = format!("album:{}", a.id);
        tile(cover(app, &a.cover, 128, 56.0, false), &a.name, r.clone(), Message::PlayCtx(r, 0))
    }))
    .take(8)
    .collect();
    let mut tiles = tiles.into_iter();
    let grid = Column::with_children(
        std::iter::from_fn(|| {
            let r: Vec<El> = tiles.by_ref().take(n).collect();
            (!r.is_empty()).then(|| Row::with_children(r).spacing(8).into())
        })
        .collect::<Vec<El>>(),
    )
    .spacing(8);
    let k = app.grid().0;
    let albums = |l: &'a [Album]| -> Vec<El<'a>> {
        l.iter().take(k).map(|a| album_card(app, a, a.artist.clone())).collect()
    };
    let shelves = [
        ("Recently played", "recent", &h.recent),
        ("Recently added", "newest", &h.added),
        ("Most played", "frequent", &h.frequent),
        ("Random picks", "random", &h.random),
    ];
    let c = h.recent.first().map_or(GREY, |a| head_color(app, &a.cover));
    let height = 200.0 + 5.0 * (app.grid().1 + 100.0);
    container(
        column![container(grid).padding([0.0, PAD])]
            .extend(
                shelves
                    .into_iter()
                    .map(|(t, s, l)| shelf(t, Some(format!("albums:{s}")), albums(l))),
            )
            .push(shelf(
                "Your playlists",
                None,
                pls.iter()
                    .take(k)
                    .map(|p| {
                        let r = format!("playlist:{}", p.id);
                        card(
                            app,
                            &p.cover,
                            &p.name,
                            format!("By {}", p.owner),
                            r.clone(),
                            false,
                            Some(Message::PlayCtx(r, 0)),
                        )
                    })
                    .collect(),
            ))
            .spacing(40)
            .padding(theme::pad(PAD, 0.0, PAD, 0.0)),
    )
    .width(Fill)
    .style(theme::fade(theme::shade(c, 0.7), 332.0 / height))
    .into()
}

fn hero<'a>(app: &App, art: El<'a>, kind: &str, title: &str, meta: El<'a>, c: Color) -> El<'a> {
    let px = art_px(app);
    let tw = app.main_w() - 3.0 * PAD - px;
    let size = [96.0, 72.0, 48.0, 32.0]
        .into_iter()
        .find(|s| theme::width(title, *s, Weight::Black) <= tw)
        .unwrap_or(32.0);
    container(
        row![
            container(art).style(|_| container::Style {
                shadow: iced::Shadow {
                    color: Color { a: 0.5, ..theme::BG },
                    blur_radius: 60.0,
                    offset: iced::Vector::new(0.0, 4.0),
                },
                ..container::Style::default()
            }),
            column![
                theme::med(kind, 14.0),
                theme::ell(title, tw, size, Weight::Black).line_height(1.1),
                meta
            ]
            .spacing(8),
        ]
        .spacing(PAD)
        .align_y(Alignment::End),
    )
    .padding(PAD)
    .width(Fill)
    .height(px + 2.0 * PAD)
    .style(theme::grad(c, theme::shade(c, 0.5)))
    .into()
}

fn bar_btn<'a>(c: char, msg: Message) -> El<'a> {
    button(theme::icon(c, 28.0).center())
        .on_press(msg)
        .padding(0)
        .width(32)
        .height(32)
        .style(theme::link(theme::SUB))
        .into()
}

fn actions<'a>(app: &App, play: Message, shuffle: Message, extra: Vec<El<'a>>) -> El<'a> {
    container(
        row![play_btn(app, 56.0, &app.route, play), bar_btn(icon::SHUFFLE, shuffle)]
            .extend(extra)
            .spacing(PAD)
            .align_y(Alignment::Center),
    )
    .padding(PAD)
    .height(BAR)
    .into()
}

fn meta_line<'a>(lead: El<'a>, rest: String) -> El<'a> {
    row![lead, theme::sub(rest, 14.0)].align_y(Alignment::Center).into()
}

struct Detail<'a> {
    art: El<'a>,
    kind: &'static str,
    title: &'a str,
    meta: El<'a>,
    color: Color,
    songs: &'a [Song],
    extra: Vec<El<'a>>,
    cols: Cols,
}

fn detail_of<'a>(app: &'a App) -> Option<Detail<'a>> {
    let key = app.route.as_str();
    let px = art_px(app);
    let star = |id: &str, on: bool| -> El<'a> {
        let on = app.is_starred(id, on);
        button(theme::heart(on, 32.0).center())
            .padding(0)
            .style(theme::link(if on { theme::ACCENT } else { theme::SUB }))
            .on_press(Message::Star(vec![id.into()], !on))
            .into()
    };
    let dots = |k: MenuKind| bar_btn(icon::DOTS, Message::Menu(Box::new(k)));
    if let Some((a, songs)) = app.data::<(Album, Vec<Song>)>(key) {
        let artists: Vec<Artist> = if a.artists.is_empty() {
            vec![Artist { id: a.artist_id.clone(), name: a.artist.clone(), ..Artist::default() }]
        } else {
            a.artists.clone()
        };
        let lead = Row::with_children(artists.iter().enumerate().map(|(n, x)| {
            button(theme::bold(if n > 0 { format!(", {}", x.name) } else { x.name.clone() }, 14.0))
                .padding(0)
                .style(theme::link(theme::TEXT))
                .on_press(Message::Nav(format!("artist:{}", x.id)))
                .into()
        }));
        let lead = row![cover(app, &artists[0].id, 128, 24.0, true), lead]
            .spacing(8)
            .align_y(Alignment::Center);
        let year = if a.year > 0 { format!(" \u{2022} {}", a.year) } else { String::new() };
        return Some(Detail {
            art: cover(app, &a.cover, 300, px, false),
            kind: if a.song_count <= 6 && a.duration < 1800.0 { "EP" } else { "Album" },
            title: &a.name,
            meta: meta_line(
                lead.into(),
                format!("{year} \u{2022} {} songs, {}", a.song_count, fmt_long(a.duration)),
            ),
            color: head_color(app, &a.cover),
            songs,
            extra: vec![star(&a.id, a.starred), dots(MenuKind::Album(Box::new(a.clone())))],
            cols: cols(app, false, false),
        });
    }
    if let Some((p, songs)) = app.data::<(Playlist, Vec<Song>)>(key) {
        return Some(Detail {
            art: cover(app, &p.cover, 300, px, false),
            kind: if p.public { "Public Playlist" } else { "Playlist" },
            title: &p.name,
            meta: column![]
                .push((!p.comment.is_empty()).then(|| theme::sub(&p.comment, 14.0)))
                .push(meta_line(
                    theme::bold(&p.owner, 14.0).into(),
                    format!(" \u{2022} {} songs, {}", p.song_count, fmt_long(p.duration)),
                ))
                .spacing(8)
                .into(),
            color: head_color(app, &p.cover),
            songs,
            extra: vec![dots(MenuKind::Playlist(p.clone()))],
            cols: cols(app, true, true),
        });
    }
    let liked = key == "liked";
    let p =
        app.data::<Pager<Song>>(key).filter(|_| liked || key.split(':').next() == Some("songs"))?;
    let (user, native) = app
        .session
        .as_ref()
        .map_or((String::new(), false), |s| (s.server.user.clone(), s.server.native));
    let count =
        (liked || native).then(|| format!(" \u{2022} {} songs", fmt_n(p.total.max(p.items.len()))));
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
    let sort = (!liked && native).then(|| -> El<'a> {
        pick_list(
            SORTS.map(|(_, l)| l),
            SORTS.iter().find(|(k, _)| *k == cur).map(|(_, l)| *l),
            |l| {
                Message::Nav(format!(
                    "songs:{}",
                    SORTS.iter().find(|(_, x)| *x == l).expect("the label is a sort").0
                ))
            },
        )
        .text_size(14)
        .font(theme::fw(Weight::Normal))
        .padding([8, 12])
        .style(theme::pick_flat)
        .menu_style(theme::pick_menu)
        .into()
    });
    Some(Detail {
        art: if liked { liked_art(px) } else { pin_art(px, icon::MUSIC, SONGS_BG, theme::ACCENT) },
        kind: if liked { "Playlist" } else { "Library" },
        title: if liked { "Liked Songs" } else { "Songs" },
        meta: meta_line(theme::bold(user, 14.0).into(), count.unwrap_or_default()),
        color: if liked { color!(0x5038a0) } else { color!(0x1e6b52) },
        songs: &p.items,
        extra: sort.into_iter().flat_map(|s| [Space::new().width(Fill).into(), s]).collect(),
        cols: cols(app, true, true),
    })
}

fn detail<'a>(app: &'a App) -> El<'a> {
    let Some(d) = detail_of(app) else { return Space::new().into() };
    let key = app.route.as_str();
    let h = BAR + HEAD + d.songs.len() as f32 * ROW + PAD;
    let y0 = art_px(app) + 2.0 * PAD + BAR + HEAD;
    column![
        hero(app, d.art, d.kind, d.title, d.meta, d.color),
        container(column![
            actions(app, Message::PlayAt(key.into(), 0), Message::PlayCtx(key.into(), 1), d.extra),
            table_head(d.cols),
            vlist(app, d.songs, key, d.cols, y0),
        ])
        .width(Fill)
        .style(theme::fade(theme::shade(d.color, 0.4), 232.0 / h)),
    ]
    .into()
}

pub fn sticky<'a>(app: &'a App) -> Option<El<'a>> {
    let off = app.scrolls.get(&app.route).map_or(0.0, |s| s.0);
    let d = detail_of(app)?;
    let top = art_px(app) + 2.0 * PAD;
    (off > top - 64.0).then(|| {
        column![
            container(
                row![
                    play_btn(app, 48.0, &app.route, Message::PlayAt(app.route.clone(), 0)),
                    theme::ell(d.title, app.main_w() - 120.0, 24.0, Weight::Bold)
                ]
                .spacing(8)
                .align_y(Alignment::Center)
            )
            .padding([0.0, PAD])
            .center_y(64)
            .width(Fill)
            .style(theme::fill(theme::shade(d.color, 0.5), 0.0)),
        ]
        .push(
            (off > top + BAR - 64.0)
                .then(|| container(table_head(d.cols)).style(theme::fill(color!(0x1a1a1a), 0.0))),
        )
        .into()
    })
}

fn artist<'a>(app: &'a App) -> El<'a> {
    let key = app.route.as_str();
    let Some((a, albums)) = app.data::<(Artist, Vec<Album>)>(key) else {
        return Space::new().into();
    };
    let id = &key["artist:".len()..];
    let top = app.data::<Vec<Song>>(&format!("top:{id}")).map_or(&[][..], |v| &v[..]);
    let info = app.data::<Info>(&format!("info:{id}"));
    let c = head_color(app, &a.cover);
    let counts = [(a.album_count, "album"), (a.song_count, "song")]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, w)| format!("{} {w}{}", fmt_n(*n as usize), if *n == 1 { "" } else { "s" }))
        .collect::<Vec<_>>()
        .join(" \u{2022} ");
    let hero = hero(
        app,
        cover(app, &a.cover, 300, art_px(app), true),
        "Artist",
        &a.name,
        theme::sub(counts, 14.0).color(theme::TEXT).into(),
        c,
    );
    let on = app.is_starred(&a.id, a.starred);
    let n = if app.popular_all { 10 } else { 5 }.min(top.len());
    let c3 = cols(app, false, false);
    let popular = (!top.is_empty()).then(|| {
        column![heading("Popular", None)]
            .extend(
                top.iter()
                    .take(n)
                    .enumerate()
                    .map(|(k, s)| container(track(app, s, k, key, c3)).padding([0.0, PAD]).into()),
            )
            .push((top.len() > 5).then(|| {
                container(
                    button(theme::bold(
                        if app.popular_all { "Show less" } else { "See more" },
                        14.0,
                    ))
                    .padding(16)
                    .style(theme::link(theme::SUB))
                    .on_press(Message::Popular),
                )
                .padding([0.0, PAD])
            }))
            .spacing(8)
    });
    let k = app.grid().0;
    let mut disc: Vec<&Album> = albums.iter().collect();
    disc.sort_by_key(|x| std::cmp::Reverse(x.year));
    let h = BAR + 600.0 + 2.0 * (app.grid().1 + 100.0);
    column![
        hero,
        container(
            column![
                actions(
                    app,
                    Message::PlayCtx(key.into(), 0),
                    Message::PlayCtx(key.into(), 1),
                    vec![
                        button(theme::bold(if on { "Following" } else { "Follow" }, 14.0))
                            .style(theme::pill)
                            .padding([7, 15])
                            .on_press(Message::Star(vec![a.id.clone()], !on))
                            .into(),
                        bar_btn(icon::DOTS, Message::Menu(Box::new(MenuKind::Artist(a.clone())))),
                    ],
                ),
                column![]
                    .push(popular)
                    .push((!disc.is_empty()).then(|| {
                        shelf(
                            "Discography",
                            None,
                            disc.iter()
                                .take(k)
                                .map(|x| {
                                    let kind = if x.song_count <= 6 && x.duration < 1800.0 {
                                        "EP"
                                    } else {
                                        "Album"
                                    };
                                    let year = if x.year > 0 {
                                        format!("{} \u{2022} ", x.year)
                                    } else {
                                        String::new()
                                    };
                                    album_card(app, x, format!("{year}{kind}"))
                                })
                                .collect(),
                        )
                    }))
                    .push(info.filter(|i| !i.similar.is_empty()).map(|i| {
                        shelf(
                            "Fans also like",
                            None,
                            i.similar.iter().take(k).map(|x| artist_card(app, x)).collect(),
                        )
                    }))
                    .push(info.filter(|i| !i.biography.is_empty()).map(|info| {
                        column![
                            heading("About", None),
                            container(
                                container(
                                    theme::txt(strip_html(&info.biography), 16.0, Weight::Normal)
                                        .color(theme::SUB)
                                )
                                .padding(PAD)
                                .max_width(672)
                                .style(theme::fill(theme::ELEVATED, 8.0))
                            )
                            .padding([0.0, PAD])
                        ]
                        .spacing(8)
                    }))
                    .spacing(40),
            ]
            .padding(theme::pad(0.0, 0.0, PAD * 2.0, 0.0)),
        )
        .width(Fill)
        .style(theme::fade(theme::shade(c, 0.4), 232.0 / h)),
    ]
    .into()
}

fn strip_html(s: &str) -> String {
    s.split('<')
        .enumerate()
        .map(|(n, p)| if n == 0 { p } else { p.split_once('>').map_or("", |x| x.1) })
        .collect::<String>()
        .trim()
        .to_string()
}

fn search<'a>(app: &'a App, q: &str) -> El<'a> {
    if q.is_empty() {
        let Some(gs) = app.data::<Vec<Genre>>("genres") else { return Space::new().into() };
        let cols = ((app.main_w() - 24.0) / 300.0).max(2.0) as usize;
        let tw = (app.main_w() - 24.0) / cols as f32 - 24.0;
        let th = (tw * 0.56).round();
        let tile = |g: &'a Genre| -> El<'a> {
            container(
                button(theme::bold(&g.name, 24.0).wrapping(text::Wrapping::WordOrGlyph))
                    .padding(16)
                    .width(tw)
                    .height(th)
                    .clip(true)
                    .style(move |_, _| button::Style {
                        background: Some(theme::color_of(&g.name).into()),
                        text_color: theme::TEXT,
                        border: iced::Border::default().rounded(8.0),
                        ..button::Style::default()
                    })
                    .on_press(Message::Nav(format!("genre:{}", g.name))),
            )
            .padding(12)
            .into()
        };
        return column![
            container(theme::bold("Browse all", 24.0)).padding(theme::pad(PAD, PAD, 4.0, PAD)),
            grid(app, gs.len(), cols, th + 24.0, 64.0, |n| tile(&gs[n])),
        ]
        .into();
    }
    let Some(r) = app.data::<SearchRes>(&app.route) else { return Space::new().into() };
    if r.artists.is_empty() && r.albums.is_empty() && r.songs.is_empty() {
        return container(
            column![
                theme::bold(format!("No results found for \u{201c}{q}\u{201d}"), 24.0),
                theme::sub("Check the spelling, or use fewer or different keywords.", 16.0),
            ]
            .spacing(16)
            .align_x(Alignment::Center),
        )
        .center_x(Fill)
        .padding(80)
        .into();
    }
    let w = app.main_w() - 3.0 * PAD;
    let lw = (w * 0.4).clamp(280.0, 420.0);
    let ql = q.to_lowercase();
    let best: Option<(&str, &str, String, String, bool)> = r
        .artists
        .iter()
        .find(|a| a.name.to_lowercase() == ql)
        .map(|a| {
            (
                a.cover.as_str(),
                a.name.as_str(),
                "Artist".to_string(),
                format!("artist:{}", a.id),
                true,
            )
        })
        .or_else(|| {
            r.albums.first().map(|a| {
                (
                    a.cover.as_str(),
                    a.name.as_str(),
                    format!("Album \u{2022} {}", a.artist),
                    format!("album:{}", a.id),
                    false,
                )
            })
        })
        .or_else(|| {
            r.artists.first().map(|a| {
                (
                    a.cover.as_str(),
                    a.name.as_str(),
                    "Artist".to_string(),
                    format!("artist:{}", a.id),
                    true,
                )
            })
        });
    let top: El = best.map_or_else(
        || Space::new().into(),
        |(cid, name, sub, route, round)| {
            column![
                theme::bold("Top result", 24.0),
                button(
                    column![
                        cover(app, cid, 300, 92.0, round),
                        theme::ell(name, lw - 40.0, 32.0, Weight::Bold),
                        theme::sub(sub, 14.0)
                    ]
                    .spacing(12)
                )
                .padding(20)
                .width(lw)
                .style(|_, s| button::Style {
                    background: Some(
                        if matches!(s, button::Status::Hovered) {
                            theme::MENU
                        } else {
                            color!(0x181818)
                        }
                        .into()
                    ),
                    text_color: theme::TEXT,
                    border: iced::Border::default().rounded(8.0),
                    ..button::Style::default()
                })
                .on_press(Message::Nav(route)),
            ]
            .spacing(8)
            .into()
        },
    );
    let sw = w - lw;
    let sc = (sw - 32.0 - 16.0 - 16.0 - 120.0 - 16.0, 0.0, 0.0);
    let k = app.grid().0;
    let pls: Vec<El> = app
        .data::<Lib>("lib")
        .map(|l| {
            l.playlists
                .iter()
                .filter(|p| p.name.to_lowercase().contains(&ql))
                .take(k)
                .map(|p| {
                    let r = format!("playlist:{}", p.id);
                    card(
                        app,
                        &p.cover,
                        &p.name,
                        format!("By {}", p.owner),
                        r.clone(),
                        false,
                        Some(Message::PlayCtx(r, 0)),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    column![
        container(
            row![
                top,
                column![theme::bold("Songs", 24.0)]
                    .extend(
                        r.songs
                            .iter()
                            .take(4)
                            .enumerate()
                            .map(|(n, s)| track(app, s, n, &app.route, sc))
                    )
                    .spacing(8)
                    .width(sw)
            ]
            .spacing(PAD)
        )
        .padding([0.0, PAD]),
        shelf("Artists", None, r.artists.iter().take(k).map(|a| artist_card(app, a)).collect()),
        shelf(
            "Albums",
            None,
            r.albums
                .iter()
                .take(k)
                .map(|a| album_card(app, a, format!("{} \u{2022} {}", a.year, a.artist)))
                .collect()
        ),
        shelf("Playlists", None, pls),
    ]
    .spacing(40)
    .padding(theme::pad(PAD, 0.0, PAD, 0.0))
    .into()
}

fn albums<'a>(app: &'a App, kind: &str, arg: &str) -> El<'a> {
    let rh = app.grid().1 + 52.0;
    let chips = (kind == "albums").then(|| {
        let cur = if arg.is_empty() { "newest" } else { arg };
        Row::with_children(
            [
                ("newest", "Recently added"),
                ("recent", "Recently played"),
                ("frequent", "Most played"),
                ("name", "Name"),
                ("artist", "Artist"),
                ("year", "Year"),
                ("random", "Random"),
            ]
            .map(|(k, name)| {
                button(theme::txt(name, 14.0, Weight::Normal))
                    .style(theme::chip(cur == k))
                    .padding([6, 12])
                    .on_press(Message::Nav(format!("albums:{k}")))
                    .into()
            }),
        )
        .spacing(8)
        .into()
    });
    let title = if kind == "genre" { arg } else { "Albums" };
    column![grid_head(title, chips)]
        .push(app.data::<Pager<Album>>(&app.route).map(|p| {
            grid(app, p.items.len(), app.grid().0, rh, GRID_Y, |n| {
                album_card(app, &p.items[n], p.items[n].artist.clone())
            })
        }))
        .into()
}

fn artists<'a>(app: &'a App) -> El<'a> {
    let rh = app.grid().1 + 52.0;
    column![grid_head("Artists", None)]
        .push(app.data::<Pager<Artist>>(&app.route).map(|p| {
            grid(app, p.items.len(), app.grid().0, rh, GRID_Y, |n| artist_card(app, &p.items[n]))
        }))
        .into()
}

fn folders<'a>(app: &'a App) -> El<'a> {
    let dir = |id: &str, name: &str| -> El<'a> {
        button(
            row![
                container(i(icon::FOLDER, 20.0).color(theme::SUB).center())
                    .center(40)
                    .style(theme::fill(theme::ELEVATED, 4.0)),
                theme::ell(name, app.main_w() - 2.0 * PAD - 80.0, 16.0, Weight::Normal)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([8, 16])
        .width(Fill)
        .style(theme::track(false))
        .on_press(Message::Nav(format!("folder:{id}")))
        .into()
    };
    let c = cols(app, false, false);
    let body: El = if let Some(a) = app.data::<Vec<Artist>>(&app.route) {
        Column::with_children(a.iter().map(|x| dir(&x.id, &x.name))).into()
    } else if let Some(v) = app.data::<Vec<Song>>(&app.route) {
        Column::with_children(v.iter().enumerate().map(|(n, s)| {
            if s.is_dir { dir(&s.id, &s.title) } else { track(app, s, n, &app.route, c) }
        }))
        .into()
    } else {
        Space::new().into()
    };
    column![
        container(theme::bold("Folders", 32.0)).padding(PAD),
        container(body).padding(theme::pad(0.0, PAD, PAD, PAD))
    ]
    .into()
}

fn lyrics<'a>(app: &'a App) -> El<'a> {
    let Some(song) = app.current_song() else {
        return container(theme::sub("Play a song to see its lyrics.", 16.0))
            .center_x(Fill)
            .padding(80)
            .into();
    };
    let key = format!("lyrics:{}", song.id);
    if let Some(s) = state(app, &key) {
        return s;
    }
    let c = head_color(app, &song.cover);
    let Some(l) =
        app.data::<Vec<Lyrics>>(&key).and_then(|v| v.iter().find(|l| !l.lines.is_empty()))
    else {
        return container(theme::bold("No lyrics are available for this song.", 24.0))
            .center_x(Fill)
            .padding(80)
            .height(app.view_size().1 - 160.0)
            .style(theme::fill(c, 0.0))
            .into();
    };
    let ms = (app.position * 1000.0) as i64;
    let end = (song.duration * 1000.0) as i64;
    let synced = l.synced
        && (end <= 0 || l.lines.iter().filter_map(|x| x.0).all(|t| t as i64 + l.offset <= end));
    let cur = l
        .lines
        .iter()
        .rposition(|(t, _)| t.is_some_and(|t| t as i64 + l.offset <= ms))
        .unwrap_or(0);
    let lines = l.lines.iter().enumerate().map(|(n, (t, line))| -> El<'a> {
        let colr = match n {
            _ if !synced => theme::TEXT,
            n if n == cur => theme::TEXT,
            n if n < cur => theme::tint(0.7),
            _ => Color { a: 0.8, ..theme::BG },
        };
        let txt = theme::bold(if line.is_empty() { "\u{266a}" } else { line }, 32.0);
        match t {
            Some(t) if synced => button(txt)
                .padding(0)
                .style(theme::link(colr))
                .on_press(Message::Seek((*t as i64 + l.offset).max(0) as f32 / 1000.0))
                .into(),
            _ => txt.color(colr).into(),
        }
    });
    container(Column::with_children(lines).spacing(16).max_width(900))
        .padding([48.0, PAD * 2.0])
        .width(Fill)
        .style(theme::fill(c, 0.0))
        .into()
}

fn settings<'a>(app: &'a App) -> El<'a> {
    let st = &app.cfg.settings;
    let section =
        |t: &'a str| container(theme::bold(t, 16.0)).padding(theme::pad(PAD, 0.0, 8.0, 0.0));
    let line = |label: &'a str, c: El<'a>| -> El<'a> {
        row![theme::txt(label, 14.0, Weight::Normal).color(theme::SUB).width(Fill), c]
            .align_y(Alignment::Center)
            .height(48)
            .into()
    };
    fn pick<'a, T: Clone + 'a>(
        opts: impl IntoIterator<Item = (T, impl ToString)>,
        cur: impl PartialEq<T>,
        f: impl Fn(T) -> Message + 'a,
    ) -> El<'a> {
        let opts: Vec<(T, String)> = opts.into_iter().map(|(v, l)| (v, l.to_string())).collect();
        let on = opts.iter().find(|o| cur == o.0).map(|o| o.1.clone());
        let labels = opts.iter().map(|o| o.1.clone()).collect::<Vec<_>>();
        pick_list(labels, on, move |l| {
            f(opts.iter().find(|o| o.1 == l).expect("the label is an option").0.clone())
        })
        .placeholder("The saved value is not available")
        .text_size(14)
        .font(theme::fw(Weight::Normal))
        .padding([8, 12])
        .width(268)
        .style(theme::pick)
        .menu_style(theme::pick_menu)
        .into()
    }
    let num = |v: f32,
               r: std::ops::RangeInclusive<f32>,
               f: fn(f32) -> Message,
               label: String|
     -> El<'a> {
        row![
            slider(r, v, f).step(0.05f32).width(200).style(theme::seek),
            theme::sub(label, 14.0).width(56).align_x(Alignment::End)
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .into()
    };
    let tog = |v: bool, f: fn(bool) -> Message| -> El<'a> {
        toggler(v).on_toggle(f).style(theme::tog).size(20).into()
    };
    let device = pick(
        std::iter::once((String::new(), "System default".to_string()))
            .chain(app.data::<Vec<(String, String)>>("devices").into_iter().flatten().cloned()),
        st.device.as_str(),
        |v| Message::Set(Set::Device(v)),
    );
    let servers = app.cfg.servers.iter().map(|s| -> El<'a> {
        row![
            column![
                theme::txt(&s.name, 14.0, Weight::Normal),
                theme::sub(format!("{} \u{2022} {}", s.user, s.url), 14.0)
            ]
            .spacing(2)
            .width(Fill),
            (app.cfg.active.as_ref() == Some(&s.id)).then(|| {
                button(theme::bold("Log out", 14.0))
                    .style(theme::pill)
                    .padding([7, 15])
                    .on_press(Message::LogOut)
            }),
        ]
        .align_y(Alignment::Center)
        .height(56)
        .into()
    });
    container(
        column![
            theme::bold("Settings", 32.0),
            section("Playback"),
            line("Output device", device),
            line(
                "ReplayGain",
                pick(
                    [("off", "Off"), ("track", "Track"), ("album", "Album")],
                    &st.replaygain,
                    |v: &str| Message::Set(Set::Rg(v.into()))
                )
            ),
            line(
                "Preamp",
                num(
                    st.preamp_db,
                    -15.0..=15.0,
                    |v| Message::Set(Set::Preamp(v)),
                    format!("{:+.1} dB", st.preamp_db)
                )
            ),
            line(
                "Gain for tracks without ReplayGain",
                num(
                    st.fallback_db,
                    -15.0..=15.0,
                    |v| Message::Set(Set::Fallback(v)),
                    format!("{:+.1} dB", st.fallback_db)
                )
            ),
            line(
                "Crossfade",
                num(
                    st.crossfade_secs,
                    0.0..=12.0,
                    |v| Message::Set(Set::Xfade(v)),
                    format!("{:.0} s", st.crossfade_secs)
                )
            ),
            section("Streaming"),
            line(
                "Transcode format",
                pick(
                    [("", "Original"), ("mp3", "MP3"), ("opus", "Opus"), ("aac", "AAC")],
                    &st.transcode_format,
                    |v: &str| Message::Set(Set::Trans(v.into()))
                )
            ),
            line(
                "Maximum bitrate",
                pick(
                    [0, 320, 256, 192, 128, 96].map(|b| {
                        (b, if b == 0 { "No limit".into() } else { format!("{b} kbps") })
                    }),
                    st.transcode_max_bitrate,
                    |v| Message::Set(Set::Bitrate(v))
                )
            ),
            section("Library"),
            line(
                "Sync the play queue with the server",
                tog(st.queue_sync, |b| Message::Set(Set::Qsync(b)))
            ),
            line(
                "Send played tracks to the server (scrobble)",
                tog(st.scrobble, |b| Message::Set(Set::Scrobble(b)))
            ),
            section("Display"),
            line(
                "Zoom",
                pick(
                    [0.8, 0.9, 1.0, 1.1, 1.25, 1.5].map(|z| (z, format!("{:.0}%", z * 100.0))),
                    st.zoom,
                    |v| Message::Set(Set::Zoom(v))
                )
            ),
            section("Servers"),
        ]
        .extend(servers)
        .push(section("Storage"))
        .push(line(
            "Cover cache",
            row![
                theme::sub(
                    format!(
                        "{:.0} MB on disk, {:.1} MB in memory",
                        app.data::<u64>("disk").copied().unwrap_or(0) as f32 / 1_048_576.0,
                        app.covers.bytes as f32 / 1_048_576.0
                    ),
                    14.0
                ),
                button(theme::bold("Clear", 14.0))
                    .style(theme::pill)
                    .padding([7, 15])
                    .on_press(Message::ClearCache)
            ]
            .spacing(16)
            .align_y(Alignment::Center)
            .into(),
        ))
        .push(section("About"))
        .push(line("Version", theme::sub(env!("CARGO_PKG_VERSION"), 14.0).into()))
        .max_width(860),
    )
    .padding(PAD)
    .into()
}

pub fn now_playing<'a>(app: &'a App) -> El<'a> {
    let w = crate::app::RIGHT_W - 32.0;
    let Some(s) = app.current_song() else {
        return container(theme::sub("Nothing is playing.", 14.0)).padding(16).into();
    };
    let starred = app.is_starred(&s.id, s.starred);
    let next = app.queue.get(app.real_at(app.index + 1));
    column![
        row![
            theme::ell(&app.source, w - 32.0, 16.0, Weight::Bold).width(Fill),
            icon_btn(icon::X, 16.0, Message::Panel(1)),
        ]
        .align_y(Alignment::Center)
        .padding(theme::pad(8.0, 8.0, 8.0, 16.0)),
        scrollable(
            column![
                cover(app, &s.cover, 300, w, false),
                row![
                    column![
                        btn(
                            theme::ell(&s.title, w - 40.0, 24.0, Weight::Bold),
                            Message::Nav(format!("album:{}", s.album_id))
                        ),
                        artists_line(&s.artists, 16.0, w - 40.0),
                    ]
                    .spacing(4)
                    .width(Fill),
                    button(theme::heart(starred, 20.0).center())
                        .padding(0)
                        .width(32)
                        .style(theme::link(if starred { theme::ACCENT } else { theme::SUB }))
                        .on_press(Message::Star(vec![s.id.clone()], !starred)),
                ]
                .align_y(Alignment::Center),
                container(
                    column![
                        theme::bold("About the artist", 16.0),
                        button(theme::bold(s.artists.first().map_or(&s.artist, |a| &a.name), 16.0))
                            .padding(0)
                            .style(theme::link(theme::TEXT))
                            .on_press(Message::Nav(format!(
                                "artist:{}",
                                s.artists.first().map_or(&s.artist_id, |a| &a.id)
                            ))),
                    ]
                    .spacing(12)
                    .padding(16)
                    .width(Fill),
                )
                .style(theme::fill(theme::ELEVATED, 8.0)),
            ]
            .push(next.map(|n| {
                container(
                    column![
                        row![
                            theme::bold("Next in queue", 16.0).width(Fill),
                            button(theme::bold("Open queue", 14.0))
                                .padding(0)
                                .style(theme::link(theme::SUB))
                                .on_press(Message::Panel(2))
                        ],
                        queue_row(app, n, app.index + 1, w - 32.0),
                    ]
                    .spacing(12)
                    .padding(16)
                    .width(Fill),
                )
                .style(theme::fill(theme::ELEVATED, 8.0))
            }))
            .spacing(16)
            .padding(theme::pad(0.0, 16.0, 16.0, 16.0)),
        )
        .style(theme::scroll),
    ]
    .into()
}

fn queue_row<'a>(app: &'a App, s: &'a Song, n: usize, w: f32) -> El<'a> {
    let cur = n == app.index;
    mouse_area(
        container(
            row![
                cover(app, &s.cover, 128, 48.0, false),
                column![
                    theme::ell(&s.title, w - 100.0, 16.0, Weight::Normal).color(if cur {
                        theme::ACCENT
                    } else {
                        theme::TEXT
                    }),
                    theme::ell(&s.artist, w - 100.0, 14.0, Weight::Normal).color(theme::SUB),
                ]
                .spacing(2)
                .width(Fill),
            ]
            .push((!cur).then(|| icon_btn(icon::X, 14.0, Message::QDel(n))))
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding(8)
        .style(theme::fill(Color::TRANSPARENT, 4.0)),
    )
    .on_double_click(Message::PlayQueue(n))
    .on_right_press(Message::Menu(Box::new(MenuKind::Song(vec![s.clone()]))))
    .into()
}

pub fn queue_panel<'a>(app: &'a App) -> El<'a> {
    let w = crate::app::RIGHT_W - 16.0;
    let next: Vec<El> = (app.index + 1..app.queue.len())
        .take(100)
        .filter_map(|n| app.queue.get(app.real_at(n)).map(|s| queue_row(app, s, n, w)))
        .collect();
    let more = app.queue.len().saturating_sub(app.index + 101);
    let title = |t: String| {
        container(theme::ell(&t, w - 16.0, 16.0, Weight::Bold))
            .padding(theme::pad(16.0, 8.0, 8.0, 8.0))
    };
    column![
        row![
            theme::bold("Queue", 16.0).width(Fill),
            (!app.queue.is_empty()).then(|| button(theme::bold("Clear queue", 14.0))
                .padding(0)
                .style(theme::link(theme::SUB))
                .on_press(Message::QClear)),
            icon_btn(icon::X, 16.0, Message::Panel(2)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding(theme::pad(8.0, 8.0, 8.0, 16.0)),
        scrollable(
            column![]
                .push(
                    app.current_song().map(|s| column![
                        title("Now playing".into()),
                        queue_row(app, s, app.index, w)
                    ])
                )
                .push((!next.is_empty()).then(|| title(if app.source.is_empty() {
                    "Next up".into()
                } else {
                    format!("Next from: {}", app.source)
                })))
                .extend(next)
                .push(
                    (more > 0).then(
                        || container(theme::sub(format!("{more} more songs"), 14.0)).padding(8)
                    )
                )
                .push(
                    app.queue.is_empty().then(|| container(theme::sub(
                        "The queue is empty.",
                        14.0
                    ))
                    .padding(8))
                )
                .padding(theme::pad(0.0, 8.0, 8.0, 8.0)),
        )
        .style(theme::scroll),
    ]
    .into()
}

pub const SPEED_H: f32 = 436.0;

pub fn menu<'a>(app: &'a App, m: &'a Menu) -> (El<'a>, f32, f32) {
    const W: f32 = 260.0;
    let entry = |r: El<'a>, msg: Message| -> El<'a> {
        focusable(
            button(r)
                .padding([0, 12])
                .height(40)
                .width(Fill)
                .style(theme::menu_item)
                .on_press(msg.clone()),
            Some(msg),
            2.0,
            Some("layer"),
        )
    };
    let item = |ic: char, label: String, msg: Message| -> El<'a> {
        entry(
            row![i(ic, 16.0).color(theme::SUB), theme::ell(&label, W - 64.0, 14.0, Weight::Normal)]
                .spacing(12)
                .align_y(Alignment::Center)
                .into(),
            msg,
        )
    };
    let more = |ic: char, label: &str, sub: u8| -> El<'a> {
        entry(
            row![
                i(ic, 16.0).color(theme::SUB),
                theme::txt(label, 14.0, Weight::Normal).width(Fill),
                i(icon::RIGHT, 16.0)
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
            Message::MenuSub(sub),
        )
    };
    let star = |id: &str, on: bool, yes: &str, no: &str| {
        let on = app.is_starred(id, on);
        item(icon::HEART, if on { yes } else { no }.into(), Message::Star(vec![id.into()], !on))
    };
    let mine = |p: &Playlist| app.session.as_ref().is_some_and(|s| s.server.user == p.owner);
    let ctx = |kind: &str, id: &str| {
        let r = format!("{kind}:{id}");
        [
            item(icon::PLAY, "Play".into(), Message::PlayCtx(r.clone(), 0)),
            item(icon::SHUFFLE, "Shuffle".into(), Message::PlayCtx(r.clone(), 1)),
            item(icon::LISTPLUS, "Add to queue".into(), Message::PlayCtx(r, 2)),
        ]
    };
    let (items, seps): (Vec<El>, usize) = match &m.kind {
        MenuKind::Speed => {
            let head = container(theme::bold("Playback speed", 12.0).color(theme::SUB))
                .padding([0, 12])
                .center_y(40);
            let list = Column::with_children((5..=35u8).map(|n| {
                let v = f32::from(n) / 10.0;
                let on = app.cfg.settings.speed == v;
                entry(
                    row![
                        theme::txt(format!("{v}x"), 14.0, Weight::Normal)
                            .color(if on { theme::ACCENT } else { theme::TEXT })
                            .width(Fill)
                    ]
                    .push(on.then(|| i(icon::CHECK, 16.0).color(theme::ACCENT)))
                    .align_y(Alignment::Center)
                    .into(),
                    Message::Set(Set::Speed(v)),
                )
            }));
            return (
                container(column![
                    head,
                    scrollable(list).id("speed").style(theme::scroll).height(Fill)
                ])
                .padding(4)
                .width(160)
                .height(SPEED_H)
                .style(theme::menu)
                .into(),
                160.0,
                SPEED_H,
            );
        }
        MenuKind::User => {
            let s = app.session.as_ref();
            let head: El = container(
                column![
                    theme::bold(s.map_or("", |s| s.server.user.as_str()), 14.0),
                    theme::ell(
                        s.map_or("", |s| s.server.url.as_str()),
                        W - 32.0,
                        12.0,
                        Weight::Normal
                    )
                    .color(theme::SUB),
                ]
                .spacing(2),
            )
            .padding([0, 12])
            .center_y(40)
            .into();
            let v: Vec<El> = std::iter::once(head)
                .chain(
                    app.cfg.servers.iter().filter(|s| app.cfg.active.as_ref() != Some(&s.id)).map(
                        |s| {
                            item(
                                icon::SERVER,
                                format!("Switch to {}", s.name),
                                Message::UseServer(s.id.clone()),
                            )
                        },
                    ),
                )
                .chain([
                    item(icon::SETTINGS, "Settings".into(), Message::Nav("settings".into())),
                    sep(),
                    item(icon::LOGOUT, "Log out".into(), Message::LogOut),
                ])
                .collect();
            (v, 1)
        }
        MenuKind::Song(s) | MenuKind::SongIn(s, _) if m.sub == 1 => {
            let n = app.data::<Lib>("lib").map_or(0, |l| l.playlists.len()) + 1;
            let name = if let [x] = &s[..] { x.title.clone() } else { format!("My Playlist #{n}") };
            let pl: Vec<El> = std::iter::once(item(
                icon::PLUS,
                "New playlist".into(),
                Message::PlNew(name, s.iter().map(|s| s.id.clone()).collect()),
            ))
            .chain([sep()])
            .chain(app.data::<Lib>("lib").into_iter().flat_map(|l| {
                l.playlists.iter().filter(|p| mine(p)).map(|p| {
                    item(
                        icon::MUSIC,
                        p.name.clone(),
                        Message::PlAdd(p.id.clone(), s.iter().map(|s| s.id.clone()).collect()),
                    )
                })
            }))
            .collect();
            let h = ((pl.len() - 1) as f32 * 40.0 + 9.0 + 8.0).min(400.0);
            return (
                container(scrollable(Column::with_children(pl)).style(theme::scroll))
                    .padding(4)
                    .width(W)
                    .height(h)
                    .style(theme::menu)
                    .into(),
                W,
                h,
            );
        }
        MenuKind::Song(v) | MenuKind::SongIn(v, _) if m.sub == 2 && v.len() == 1 => {
            let s = &v[0];
            let cur = app.ratings.get(&s.id).copied().unwrap_or(s.user_rating);
            let stars = Row::with_children((1..=5u8).map(|n| {
                let m = Message::Rate(s.id.clone(), if n == cur { 0 } else { n });
                focusable(
                    button(i(icon::STAR, 20.0).center())
                        .width(36)
                        .height(36)
                        .padding(0)
                        .style(theme::link(if n <= cur { theme::ACCENT } else { theme::SUB }))
                        .on_press(m.clone()),
                    Some(m),
                    18.0,
                    Some("layer"),
                )
            }));
            return (container(stars).padding(6).style(theme::menu).into(), 192.0, 48.0);
        }
        MenuKind::Song(songs) | MenuKind::SongIn(songs, _) => {
            let on = songs.iter().all(|s| app.is_starred(&s.id, s.starred));
            let mut v = vec![
                item(icon::LISTPLUS, "Add to queue".into(), Message::QEnd(songs.clone())),
                item(icon::QUEUE, "Play next".into(), Message::QNext(songs.clone())),
                sep(),
                more(icon::PLUS, "Add to playlist", 1),
            ];
            if let MenuKind::SongIn(_, n) = &m.kind {
                v.push(item(
                    icon::TRASH,
                    "Remove from this playlist".into(),
                    Message::PlRemove(n.clone()),
                ));
            }
            v.push(item(
                icon::HEART,
                if on { "Remove from your Liked Songs" } else { "Save to your Liked Songs" }.into(),
                Message::Star(songs.iter().map(|s| s.id.clone()).collect(), !on),
            ));
            if let [s] = &songs[..] {
                v.extend([
                    more(icon::STAR, "Rate", 2),
                    sep(),
                    item(icon::RADIO, "Go to song radio".into(), Message::Radio(s.id.clone())),
                ]);
                v.extend((!s.artist_id.is_empty()).then(|| {
                    item(
                        icon::USER,
                        "Go to artist".into(),
                        Message::Nav(format!("artist:{}", s.artist_id)),
                    )
                }));
                v.extend((!s.album_id.is_empty()).then(|| {
                    item(
                        icon::ALBUM,
                        "Go to album".into(),
                        Message::Nav(format!("album:{}", s.album_id)),
                    )
                }));
            }
            let seps = if songs.len() == 1 { 2 } else { 1 };
            (v, seps)
        }
        MenuKind::Album(a) => (
            ctx("album", &a.id)
                .into_iter()
                .chain([
                    star(&a.id, a.starred, "Remove from Your Library", "Save to Your Library"),
                    item(
                        icon::USER,
                        "Go to artist".into(),
                        Message::Nav(format!("artist:{}", a.artist_id)),
                    ),
                ])
                .collect(),
            0,
        ),
        MenuKind::Artist(a) => (
            ctx("artist", &a.id)
                .into_iter()
                .chain([
                    star(&a.id, a.starred, "Unfollow", "Follow"),
                    item(icon::RADIO, "Go to artist radio".into(), Message::Radio(a.id.clone())),
                ])
                .collect(),
            0,
        ),
        MenuKind::Playlist(p) if mine(p) => (
            ctx("playlist", &p.id)
                .into_iter()
                .chain([
                    item(icon::PENCIL, "Edit details".into(), Message::Modal(p.id.clone(), 1)),
                    sep(),
                    item(icon::TRASH, "Delete".into(), Message::Modal(p.id.clone(), 2)),
                ])
                .collect(),
            1,
        ),
        MenuKind::Playlist(p) => (ctx("playlist", &p.id).into(), 0),
    };
    let h = (items.len() - seps) as f32 * 40.0 + seps as f32 + 8.0;
    (container(Column::with_children(items)).padding(4).width(W).style(theme::menu).into(), W, h)
}

pub fn login<'a>(app: &'a App) -> El<'a> {
    let l = &app.login;
    const W: f32 = 324.0;
    let field = |label: &'a str, v: &'a str, n: u8, secure: bool| -> El<'a> {
        column![
            theme::bold(label, 14.0),
            text_input("", v)
                .id(format!("login{n}"))
                .on_input(move |s| Message::LField(n, s))
                .on_submit(Message::LSubmit)
                .secure(secure)
                .style(theme::field)
                .font(theme::fw(Weight::Normal))
                .padding(13)
                .line_height(text::LineHeight::Absolute(20.0.into()))
                .size(16)
        ]
        .spacing(8)
        .into()
    };
    let saved = app.cfg.servers.iter().map(|s| -> El<'a> {
        let initial = s.user.chars().next().unwrap_or('?').to_ascii_uppercase();
        let row = button(
            row![
                container(theme::bold(initial, 16.0).color(theme::BG).center())
                    .center(40)
                    .style(theme::fill(theme::color_of(&initial.to_string()), 20.0)),
                column![
                    theme::ell(&s.name, W - 120.0, 16.0, Weight::Bold),
                    theme::ell(&s.user, W - 120.0, 14.0, Weight::Normal).color(theme::SUB)
                ]
                .spacing(2)
                .width(Fill),
                focusable(
                    icon_btn(icon::X, 14.0, Message::DropServer(s.id.clone())),
                    Some(Message::DropServer(s.id.clone())),
                    14.0,
                    None,
                ),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding(8)
        .width(Fill)
        .style(theme::track(false))
        .on_press(Message::UseServer(s.id.clone()));
        focusable(row, Some(Message::UseServer(s.id.clone())), 4.0, None)
    });
    let form = column![
        container(i(icon::MUSIC, 36.0).color(theme::TEXT)).padding(8),
        theme::bold("Log in to navify", 48.0).align_x(Alignment::Center).line_height(1.1),
    ]
    .push((!l.error.is_empty()).then(|| {
        container(
            row![i(icon::X, 16.0), theme::txt(&l.error, 14.0, Weight::Normal)]
                .spacing(12)
                .align_y(Alignment::Center),
        )
        .padding(12)
        .width(Fill)
        .style(theme::fill(color!(0xe91429), 4.0))
    }))
    .push(
        (!app.cfg.servers.is_empty())
            .then(|| column![theme::bold("Saved servers", 14.0)].extend(saved).spacing(4).width(W)),
    )
    .push((!app.cfg.servers.is_empty()).then(sep))
    .push(
        column![
            field("Server address", &l.url, 0, false),
            field("Username", &l.user, 1, false),
            field("Password", &l.pass, 2, true),
            focusable(
                btn(
                    row![
                        toggler(l.remember)
                            .on_toggle(Message::LRemember)
                            .style(theme::tog)
                            .size(20),
                        theme::txt("Remember password", 14.0, Weight::Normal),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                    Message::LRemember(!l.remember),
                ),
                Some(Message::LRemember(!l.remember)),
                4.0,
                None,
            ),
            focusable(
                button(
                    theme::bold(if l.busy { "Logging in" } else { "Log In" }, 16.0)
                        .color(theme::BG)
                        .center()
                        .width(Fill)
                )
                .style(theme::accent)
                .width(Fill)
                .padding(14)
                .on_press_maybe((!l.busy).then_some(Message::LSubmit)),
                (!l.busy).then_some(Message::LSubmit),
                500.0,
                None,
            ),
        ]
        .spacing(16)
        .width(W),
    )
    .spacing(32)
    .align_x(Alignment::Center);
    container(
        scrollable(
            container(
                container(form)
                    .padding([40, 32])
                    .width((app.view_size().0 - 64.0).clamp(W + 64.0, 734.0))
                    .center_x(Fill)
                    .max_width(734)
                    .style(theme::panel),
            )
            .center_x(Fill)
            .padding([32, 32]),
        )
        .style(theme::scroll)
        .height(Fill),
    )
    .height(Fill)
    .style(theme::grad(color!(0x2a2a2a), theme::BG))
    .into()
}
