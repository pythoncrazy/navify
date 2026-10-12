use iced::widget::text::Wrapping;
use iced::widget::{
    Column, Row, Space, button, column, container, mouse_area, pick_list, row, scrollable, slider,
    text, text_input, toggler,
};
use iced::{Alignment, Color, Element, Fill, Font, Length, color};

use crate::api::{Album, Artist, Genre, Info, Lyrics, Playlist, Song};
use crate::app::{App, Home, Lib, Menu, MenuKind, Message, Pager, SearchRes, Set};
use crate::theme::{self, icon, icon as i};
use crate::view::{artists_line, btn, cover, fmt_long, icon_btn};

fn bold(t: impl ToString, s: f32) -> text::Text<'static> {
    theme::bold(t.to_string(), s)
}

fn med(t: impl ToString, s: f32) -> text::Text<'static> {
    theme::med(t.to_string(), s)
}

pub fn head_color(app: &App, id: &str) -> Color {
    app.covers.color.get(id).copied().unwrap_or(color!(0x503a55))
}

fn menu_item<'a>(ic: char, label: &str, msg: Message) -> Element<'a, Message> {
    btn(
        row![i(ic, 15.0).color(theme::SUB), text(label.to_string()).size(13)]
            .spacing(10)
            .align_y(Alignment::Center)
            .padding([6, 12])
            .width(210),
        msg,
    )
    .into()
}

fn menu_sub<'a>(ic: char, label: &str, sub: u8) -> Element<'a, Message> {
    btn(
        row![
            i(ic, 15.0).color(theme::SUB),
            text(label.to_string()).size(13),
            Space::new().width(Fill),
            i(icon::RIGHT, 12.0).color(theme::SUB),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding([6, 12])
        .width(210),
        Message::MenuSub(sub),
    )
    .into()
}

#[allow(clippy::too_many_arguments)]
fn track_row<'a>(
    app: &'a App,
    s: &'a Song,
    n: usize,
    songs: &'a [Song],
    source: &'a str,
    show_album: bool,
    show_date: bool,
    context_idx: Option<usize>,
) -> Element<'a, Message> {
    let cur = app.current_song().is_some_and(|c| c.id == s.id);
    let starred = app.starred.get(&s.id).copied().unwrap_or(s.starred);
    let sel = app.sel.as_deref() == Some(s.id.as_str());
    let idx: Element<Message> = if cur && app.playing {
        i(icon::EQ, 14.0).color(theme::ACCENT).center().width(24).into()
    } else {
        text(n + 1)
            .size(13)
            .color(if cur { theme::ACCENT } else { theme::SUB })
            .center()
            .width(24)
            .into()
    };
    let mut r = Row::new().spacing(8).align_y(Alignment::Center).padding([0, 8]);
    r = r.push(idx);
    r = r.push(
        column![
            text(s.title.clone()).size(14).wrapping(Wrapping::None).color(if cur {
                theme::ACCENT
            } else {
                theme::TEXT
            }),
            artists_line(&s.artists, 12.0),
        ]
        .spacing(2)
        .width(Length::FillPortion(4)),
    );
    if show_album {
        r = r.push(
            container(btn(
                theme::sub(s.album.clone(), 13.0),
                Message::Nav(format!("album:{}", s.album_id)),
            ))
            .width(Length::FillPortion(3)),
        );
    }
    if show_date {
        r = r.push(container(theme::sub(&s.created[..s.created.len().min(10)], 13.0)).width(90));
    }
    r = r.push(btn(
        i(icon::HEART, 15.0)
            .color(if starred { theme::ACCENT } else { theme::SUB })
            .center()
            .width(24),
        Message::Star(s.id.clone(), !starred),
    ));
    r = r.push(theme::sub(crate::view::fmt_dur(s.duration), 13.0).width(48));
    r = r.push(icon_btn(
        icon::DOTS,
        14.0,
        Message::Menu(Box::new(
            context_idx.map_or(MenuKind::Song(s.clone()), |i| MenuKind::SongIn(s.clone(), i)),
        )),
    ));
    let row_el: Element<Message> = mouse_area(
        button(container(r).height(56).width(Fill).center_y(56))
            .width(Fill)
            .style(theme::track(sel))
            .on_press(Message::Select(s.id.clone())),
    )
    .on_double_click(Message::Play(songs.to_vec(), n, source.to_string()))
    .on_right_press(Message::Menu(Box::new(
        context_idx.map_or(MenuKind::Song(s.clone()), |i| MenuKind::SongIn(s.clone(), i)),
    )))
    .into();
    row_el
}

pub fn sticky_track_header<'a>(app: &'a App) -> Option<Element<'a, Message>> {
    let off = app.scrolls.get(&app.route).map(|(o, _)| *o).unwrap_or(0.0);
    if off < 395.0 {
        return None;
    }
    let (show_album, show_date) = match app.route.split(':').next().unwrap_or("") {
        "album" | "liked" => (true, false),
        "playlist" => (true, true),
        _ => return None,
    };
    Some(
        container(track_header(show_album, show_date))
            .width(Fill)
            .padding(theme::pad(0.0, 16.0, 0.0, 16.0))
            .style(|_| container::Style::default().background(theme::PANEL))
            .into(),
    )
}

fn track_header<'a>(show_album: bool, show_date: bool) -> Element<'a, Message> {
    let mut r = Row::new().spacing(8).align_y(Alignment::Center).padding([0, 8]);
    r = r.push(text("#").size(12).color(theme::SUB).center().width(24));
    r = r.push(container(theme::sub("Title", 12.0)).width(Length::FillPortion(4)));
    if show_album {
        r = r.push(container(theme::sub("Album", 12.0)).width(Length::FillPortion(3)));
    }
    if show_date {
        r = r.push(container(theme::sub("Date added", 12.0)).width(90));
    }
    r = r.push(Space::new().width(24));
    r = r.push(container(i(icon::CLOCK, 13.0).color(theme::SUB).center()).width(48));
    r = r.push(Space::new().width(32));
    column![
        r.height(32),
        container(Space::new())
            .height(1)
            .style(|_| { container::Style::default().background(theme::tint(0.15)) })
    ]
    .into()
}

fn vlist<'a>(
    app: &'a App,
    songs: &'a [Song],
    source: &'a str,
    show_album: bool,
    show_date: bool,
    header_y: f32,
) -> Element<'a, Message> {
    let (off, view) = app.scrolls.get(&app.route).copied().unwrap_or((0.0, 900.0));
    let rel = (off - header_y).max(0.0);
    let lo = ((rel / 56.0) as usize).saturating_sub(6).min(songs.len());
    let hi = (((rel + view) / 56.0) as usize + 12).min(songs.len());
    let mut col = Column::new();
    col = col.push(Space::new().height(lo as f32 * 56.0));
    for i in lo..hi {
        let ctx = if app.route.starts_with("playlist:") { Some(i) } else { None };
        col = col.push(track_row(app, &songs[i], i, songs, source, show_album, show_date, ctx));
    }
    col = col.push(Space::new().height((songs.len() - hi) as f32 * 56.0));
    col.into()
}

fn card<'a>(
    app: &App,
    cover_id: &str,
    title: &str,
    sub: &str,
    route: String,
    circle: bool,
    play: Option<Message>,
) -> Element<'a, Message> {
    let cover_el = cover(app, cover_id, 300, 156.0, circle);
    let cover_el: Element<Message> = match play {
        Some(msg) => iced::widget::hover(
            cover_el,
            container(btn(
                container(i(icon::PLAY, 18.0).color(theme::BG).center())
                    .width(48)
                    .height(48)
                    .center_x(48)
                    .center_y(48)
                    .style(|_| {
                        container::Style::default()
                            .background(theme::ACCENT)
                            .border(iced::Border::default().rounded(500.0))
                    }),
                msg,
            ))
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(8)
            .width(Fill)
            .height(Fill),
        ),
        None => cover_el,
    };
    button(
        column![
            cover_el,
            med(title.to_string(), 15.0).width(Fill).wrapping(Wrapping::None),
            theme::sub(sub.to_string(), 13.0).width(Fill),
        ]
        .spacing(8)
        .padding(12)
        .width(180),
    )
    .on_press(Message::Nav(route))
    .style(theme::card_hover)
    .into()
}

fn shelf<'a, T, I>(
    app: &'a App,
    title: &str,
    items: I,
    show_all: Option<String>,
    f: impl Fn(&'a App, &'a T) -> Element<'a, Message> + 'a,
) -> Element<'a, Message>
where
    I: Iterator<Item = &'a T> + Clone + 'a,
    T: 'a,
{
    if items.clone().next().is_none() {
        return Space::new().height(0).into();
    }
    let mut t = row![bold(title, 24.0), Space::new().width(Fill)].align_y(Alignment::Center);
    if let Some(r) = show_all {
        t = t.push(btn(theme::sub("Show all", 13.0), Message::Nav(r)));
    }
    column![
        t.padding([0, 12]),
        iced::widget::responsive(move |size| {
            let n = ((size.width / 196.0).floor() as usize).max(1);
            row(items.clone().take(n).map(|i| f(app, i)).collect::<Vec<Element<Message>>>()).into()
        }),
    ]
    .spacing(8)
    .into()
}

pub fn route<'a>(app: &'a App) -> Element<'a, Message> {
    let (kind, arg) = app.route.split_once(':').unwrap_or((app.route.as_str(), ""));
    match kind {
        "home" => home(app),
        "album" => album_page(app, arg),
        "artist" => artist_page(app, arg),
        "playlist" => playlist_page(app, arg),
        "liked" => liked_page(app),
        "search" => search_page(app, arg),
        "albums" => albums_grid(app, arg),
        "artists" => artists_grid(app),
        "songs" => songs_page(app, arg),
        "genres" => genres_page(app),
        "genre" => genre_page(app, arg),
        "folders" => folders_page(app),
        "folder" => folder_page(app, arg),
        "lyrics" => lyrics_page(app),
        "settings" => settings_page(app),
        "login" => login(app),
        _ => container(theme::sub("This page does not exist.", 14.0)).padding(40).into(),
    }
}

fn loading<'a>(app: &'a App, key: &str) -> Option<Element<'a, Message>> {
    if app.cache.contains_key(key) {
        return None;
    }
    let blk = |w: Length, h: f32| container(Space::new()).width(w).height(h).style(theme::skel);
    Some(
        column![
            blk(280.0.into(), 32.0),
            blk(Fill, 220.0),
            row![
                blk(168.0.into(), 240.0),
                blk(168.0.into(), 240.0),
                blk(168.0.into(), 240.0),
                blk(168.0.into(), 240.0),
                blk(168.0.into(), 240.0),
            ]
            .spacing(24),
            row![
                blk(168.0.into(), 240.0),
                blk(168.0.into(), 240.0),
                blk(168.0.into(), 240.0),
                blk(168.0.into(), 240.0),
                blk(168.0.into(), 240.0),
            ]
            .spacing(24),
        ]
        .spacing(24)
        .padding(24)
        .into(),
    )
}

fn home<'a>(app: &'a App) -> Element<'a, Message> {
    if let Some(l) = loading(app, "home") {
        return l;
    }
    let Some((_, v)) = app.cache.get("home") else { return Space::new().into() };
    let Some(h) = v.downcast_ref::<Home>() else { return Space::new().into() };
    let gc = h.recent.first().map(|a| head_color(app, &a.cover)).unwrap_or(color!(0x503a55));
    let hour = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 3600 + 16) % 24)
        .unwrap_or(18);
    let greet = match hour {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    };
    let mut tiles = Column::new().spacing(8);
    let mut rows = Vec::new();
    let mut row_items: Vec<Element<Message>> = Vec::new();
    for (n, a) in h.recent.iter().take(8).enumerate() {
        if n == 4 {
            rows.push(row(std::mem::take(&mut row_items)).spacing(8));
        }
        row_items.push(
            button(
                row![
                    cover(app, &a.cover, 300, 64.0, false),
                    container(med(a.name.clone(), 14.0).wrapping(Wrapping::None),)
                        .center_y(64)
                        .padding([0, 12]),
                ]
                .width(Length::FillPortion(1)),
            )
            .on_press(Message::Nav(format!("album:{}", a.id)))
            .style(theme::tile)
            .clip(true)
            .width(Length::FillPortion(1))
            .into(),
        );
    }
    rows.push(row(row_items).spacing(8));
    for r in rows {
        tiles = tiles.push(r);
    }
    let mut col = column![
        container(column![bold(greet, 30.0), tiles,])
            .padding(theme::pad(16.0, 12.0, 24.0, 12.0))
            .width(Fill)
            .style(theme::header(gc)),
    ]
    .spacing(24);
    let mk = |l: &'a Vec<Album>, title: &'a str| {
        shelf(app, title, l.iter(), Some("albums:newest".into()), |app, a| {
            card(
                app,
                &a.cover,
                &a.name,
                &a.artist,
                format!("album:{}", a.id),
                false,
                Some(Message::PlayCtx(format!("album:{}", a.id), false)),
            )
        })
    };
    col = col.push(mk(&h.recent, "Recently played"));
    col = col.push(mk(&h.added, "Recently added"));
    col = col.push(mk(&h.frequent, "Most played"));
    col = col.push(mk(&h.random, "Random picks"));
    col = col.push(shelf(app, "Your playlists", h.playlists.iter(), None, |app, p| {
        card(
            app,
            &p.cover,
            &p.name,
            "Playlist",
            format!("playlist:{}", p.id),
            false,
            Some(Message::PlayCtx(format!("playlist:{}", p.id), false)),
        )
    }));
    col.into()
}

fn action_bar<'a>(
    _app: &'a App,
    play: Message,
    shuffle_msg: Message,
    extra: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut r = row![
        btn(
            container(i(icon::PLAY, 22.0).color(theme::BG).center())
                .width(56)
                .height(56)
                .center_x(56)
                .center_y(56)
                .style(|_| container::Style::default()
                    .background(theme::ACCENT)
                    .border(iced::Border::default().rounded(500.0))),
            play,
        ),
        icon_btn(icon::SHUFFLE, 22.0, shuffle_msg),
    ]
    .spacing(18)
    .align_y(Alignment::Center);
    for e in extra {
        r = r.push(e);
    }
    container(r.padding([16, 12])).into()
}

fn detail_head<'a>(
    app: &App,
    label: &str,
    cover_id: &str,
    title: &str,
    meta: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let size = if title.len() > 40 {
        42.0
    } else if title.len() > 20 {
        64.0
    } else {
        84.0
    };
    container(
        row![
            container(cover(app, cover_id, 640, 232.0, false)).style(|_| container::Style {
                shadow: iced::Shadow {
                    color: iced::Color { a: 0.6, ..theme::BG },
                    blur_radius: 24.0,
                    offset: iced::Vector::new(0.0, 8.0),
                },
                ..container::Style::default()
            }),
            column![
                Space::new().height(Fill),
                theme::sub(label, 13.0),
                theme::black(title.to_string(), size).wrapping(Wrapping::None),
                row(meta).spacing(4).align_y(Alignment::Center),
            ]
            .spacing(8),
        ]
        .spacing(20)
        .align_y(Alignment::End)
        .padding([24, 12])
        .height(300),
    )
    .width(Fill)
    .style(theme::header(head_color(app, cover_id)))
    .into()
}

fn album_page<'a>(app: &'a App, _id: &str) -> Element<'a, Message> {
    if let Some(l) = loading(app, &app.route) {
        return l;
    }
    let Some((_, v)) = app.cache.get(&app.route) else { return Space::new().into() };
    let Some((a, songs)) = v.downcast_ref::<(Album, Vec<Song>)>() else {
        return Space::new().into();
    };
    let label = if a.song_count <= 6 { "EP" } else { "Album" };
    let mut meta: Vec<Element<Message>> = Vec::new();
    meta.push(artists_line(&a.artists, 14.0));
    if a.year > 0 {
        meta.push(theme::sub(format!(" \u{2022} {}", a.year), 14.0).into());
    }
    meta.push(
        theme::sub(format!(" \u{2022} {} songs, {}", a.song_count, fmt_long(a.duration)), 14.0)
            .into(),
    );
    let head = detail_head(app, label, &a.cover, &a.name, meta);
    let bar = action_bar(
        app,
        Message::Play(songs.clone(), 0, a.name.clone()),
        Message::PlayCtx(app.route.clone(), true),
        vec![
            icon_btn(
                icon::HEART,
                22.0,
                Message::Star(a.id.clone(), !app.starred.get(&a.id).copied().unwrap_or(a.starred)),
            )
            .into(),
            icon_btn(icon::DOTS, 22.0, Message::Menu(Box::new(MenuKind::Album(a.clone())))).into(),
        ],
    );
    let discs = songs.iter().map(|s| s.disc).collect::<std::collections::BTreeSet<_>>();
    let multi_disc = discs.len() > 1;
    let mut list = Column::new();
    if !multi_disc {
        list = list.push(vlist(app, songs, &a.name, true, false, 430.0));
    } else {
        let (off, view) = app.scrolls.get(&app.route).copied().unwrap_or((0.0, 900.0));
        let mut y = 430.0f32;
        for d in discs {
            let ds: Vec<usize> =
                songs.iter().enumerate().filter(|(_, s)| s.disc == d).map(|(i, _)| i).collect();
            list = list.push(
                container(theme::sub(format!("Disc {d}"), 14.0))
                    .padding(theme::pad(16.0, 8.0, 8.0, 8.0)),
            );
            for &i in &ds {
                list = list.push(track_row(app, &songs[i], i, songs, &a.name, true, false, None));
            }
            y += (ds.len() as f32 + 1.0) * 56.0;
            let _ = (off, view, y);
        }
    }
    column![head, bar, track_header(true, false), list].into()
}

fn playlist_page<'a>(app: &'a App, _id: &str) -> Element<'a, Message> {
    if let Some(l) = loading(app, &app.route) {
        return l;
    }
    let Some((_, v)) = app.cache.get(&app.route) else { return Space::new().into() };
    let Some((p, songs)) = v.downcast_ref::<(Playlist, Vec<Song>)>() else {
        return Space::new().into();
    };
    let meta: Vec<Element<Message>> = vec![
        theme::sub(p.owner.clone(), 14.0).into(),
        theme::sub(
            format!(
                " \u{2022} {} songs, {}",
                p.song_count.max(songs.len() as u32),
                fmt_long(p.duration.max(songs.iter().map(|s| s.duration).sum()))
            ),
            14.0,
        )
        .into(),
    ];
    let head = detail_head(app, "Playlist", &p.cover, &p.name, meta);
    let bar = action_bar(
        app,
        Message::Play(songs.clone(), 0, p.name.clone()),
        Message::PlayCtx(app.route.clone(), true),
        vec![
            icon_btn(icon::DOTS, 22.0, Message::Menu(Box::new(MenuKind::Playlist(p.clone()))))
                .into(),
        ],
    );
    column![head, bar, track_header(true, true), vlist(app, songs, &p.name, true, true, 430.0)]
        .into()
}

fn liked_page<'a>(app: &'a App) -> Element<'a, Message> {
    if let Some(l) = loading(app, &app.route) {
        return l;
    }
    let Some((_, v)) = app.cache.get(&app.route) else { return Space::new().into() };
    let Some(p) = v.downcast_ref::<Pager<Song>>() else { return Space::new().into() };
    let songs = &p.items;
    let meta: Vec<Element<Message>> =
        vec![theme::sub(format!("{} songs", p.total.max(songs.len())), 14.0).into()];
    let head = container(
        row![
            container(i(icon::HEART, 90.0).color(iced::Color::WHITE).center())
                .width(232)
                .height(232)
                .center_x(232)
                .center_y(232)
                .style(|_| container::Style::default()
                    .background(iced::Gradient::Linear(
                        iced::gradient::Linear::new(iced::Radians(2.35))
                            .add_stop(0.0, theme::GRAD_A)
                            .add_stop(1.0, theme::GRAD_B)
                    ))
                    .border(iced::Border::default().rounded(6.0))),
            column![
                Space::new().height(Fill),
                theme::sub("Playlist", 13.0),
                text("Liked Songs")
                    .size(84.0)
                    .font(Font { weight: iced::font::Weight::Bold, ..Font::DEFAULT }),
                row(meta).spacing(4),
            ]
            .spacing(8),
        ]
        .spacing(20)
        .align_y(Alignment::End)
        .padding([24, 12])
        .height(300),
    )
    .width(Fill)
    .style(theme::header(color!(0x503a55)));
    let bar = action_bar(
        app,
        Message::Play(songs.clone(), 0, "Liked Songs".into()),
        Message::Noop,
        vec![],
    );
    column![
        head,
        bar,
        track_header(true, false),
        vlist(app, songs, "Liked Songs", true, false, 430.0)
    ]
    .into()
}

fn artist_page<'a>(app: &'a App, _id: &str) -> Element<'a, Message> {
    if let Some(l) = loading(app, &app.route) {
        return l;
    }
    let Some((_, v)) = app.cache.get(&app.route) else { return Space::new().into() };
    let Some((a, albums, top, info)) = v.downcast_ref::<(Artist, Vec<Album>, Vec<Song>, Info)>()
    else {
        return Space::new().into();
    };
    let meta: Vec<Element<Message>> = vec![
        theme::sub(format!("{} albums \u{2022} {} songs", a.album_count, a.song_count), 14.0)
            .into(),
    ];
    let head = container(
        column![
            Space::new().height(Fill),
            theme::sub("Artist", 14.0),
            theme::black(a.name.clone(), 84.0).wrapping(Wrapping::None),
            row(meta).spacing(4),
        ]
        .spacing(8)
        .padding([24, 12])
        .height(340),
    )
    .width(Fill)
    .style(theme::header(head_color(app, &a.cover)));
    let bar = action_bar(
        app,
        Message::PlayCtx(app.route.clone(), false),
        Message::PlayCtx(app.route.clone(), true),
        vec![
            icon_btn(
                icon::HEART,
                22.0,
                Message::Star(a.id.clone(), !app.starred.get(&a.id).copied().unwrap_or(a.starred)),
            )
            .into(),
            icon_btn(icon::DOTS, 22.0, Message::Menu(Box::new(MenuKind::Artist(a.clone())))).into(),
        ],
    );
    let mut col = column![head, bar].spacing(24);
    if !top.is_empty() {
        let n = if app.popular_all { 10.min(top.len()) } else { 5.min(top.len()) };
        let mut rows = Column::new();
        for (i, s) in top.iter().take(n).enumerate() {
            rows = rows.push(track_row(app, s, i, top, &a.name, true, false, None));
        }
        let mut t = column![container(bold("Popular", 24.0)).padding([0, 12]), rows];
        if !app.popular_all && top.len() > 5 {
            t = t.push(btn(theme::sub("See more", 13.0), Message::Popular).padding([8, 12]));
        }
        col = col.push(t);
    }
    col = col.push(shelf(app, "Discography", albums.iter(), None, |app, al| {
        card(
            app,
            &al.cover,
            &al.name,
            &al.year.to_string(),
            format!("album:{}", al.id),
            false,
            Some(Message::PlayCtx(format!("album:{}", al.id), false)),
        )
    }));
    if !info.similar.is_empty() {
        col = col.push(shelf(app, "Fans also like", info.similar.iter(), None, |app, s| {
            card(app, &s.cover, &s.name, "Artist", format!("artist:{}", s.id), true, None)
        }));
    }
    if !info.biography.is_empty() {
        col = col.push(
            container(
                column![bold("About", 20.0), theme::sub(strip_html(&info.biography), 14.0),]
                    .spacing(8),
            )
            .padding(theme::pad(0.0, 12.0, 24.0, 12.0)),
        );
    }
    col.into()
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn search_page<'a>(app: &'a App, q: &str) -> Element<'a, Message> {
    if q.is_empty() {
        let mut col = column![container(bold("Browse all", 24.0)).padding([16, 12])].spacing(0);
        if let Some((_, v)) = app.cache.get("genres")
            && let Some(gs) = v.downcast_ref::<Vec<Genre>>()
        {
            let mut grid = Column::new().spacing(12).padding(12);
            let mut r = Row::new().spacing(12);
            let mut n = 0;
            for g in gs.iter() {
                let c = genre_color(&g.name);
                r = r.push(btn(
                    container(bold(g.name.clone(), 18.0))
                        .width(176)
                        .height(96)
                        .padding(12)
                        .clip(true)
                        .style(move |_| {
                            container::Style::default()
                                .background(c)
                                .border(iced::Border::default().rounded(8.0))
                        }),
                    Message::Nav(format!("genre:{}", g.name)),
                ));
                n += 1;
                if n == 6 {
                    grid = grid.push(r);
                    r = Row::new().spacing(12);
                    n = 0;
                }
            }
            if n > 0 {
                grid = grid.push(r);
            }
            col = col.push(grid);
        }
        return col.into();
    }
    if let Some(l) = loading(app, &app.route) {
        return l;
    }
    let Some((_, v)) = app.cache.get(&app.route) else { return Space::new().into() };
    let Some(r) = v.downcast_ref::<SearchRes>() else { return Space::new().into() };
    let mut col = Column::new().spacing(24).padding([16, 12]);
    let mut top = Column::new().spacing(16);
    if let Some(best) = r.albums.first() {
        top = top.push(column![
            bold("Top result", 24.0),
            container(
                column![
                    cover(app, &best.cover, 300, 100.0, false),
                    bold(best.name.clone(), 28.0),
                    theme::sub(format!("Album \u{2022} {}", best.artist), 14.0),
                ]
                .spacing(10)
                .padding(20)
                .width(380),
            )
            .style(theme::elevated),
        ]);
    }
    let mut songs_col = column![bold("Songs", 24.0)].spacing(0);
    for (i, s) in r.songs.iter().take(4).enumerate() {
        songs_col = songs_col.push(track_row(app, s, i, &r.songs, "Search", false, false, None));
    }
    col = col.push(row![top.width(Fill), songs_col.width(Length::FillPortion(2))].spacing(24));
    if !r.artists.is_empty() {
        col = col.push(shelf(app, "Artists", r.artists.iter(), None, |app, a| {
            card(app, &a.cover, &a.name, "Artist", format!("artist:{}", a.id), true, None)
        }));
    }
    if !r.albums.is_empty() {
        col = col.push(shelf(app, "Albums", r.albums.iter(), None, |app, a| {
            card(
                app,
                &a.cover,
                &a.name,
                &a.artist,
                format!("album:{}", a.id),
                false,
                Some(Message::PlayCtx(format!("album:{}", a.id), false)),
            )
        }));
    }
    if let Some((_, lv)) = app.cache.get("lib")
        && let Some(l) = lv.downcast_ref::<Lib>()
    {
        let ql = q.to_lowercase();
        let pls = l.playlists.iter().filter(move |p| p.name.to_lowercase().contains(&ql));
        if pls.clone().next().is_some() {
            col = col.push(shelf(app, "Playlists", pls, None, |app, p| {
                card(
                    app,
                    &p.cover,
                    &p.name,
                    "Playlist",
                    format!("playlist:{}", p.id),
                    false,
                    Some(Message::PlayCtx(format!("playlist:{}", p.id), false)),
                )
            }));
        }
    }
    col.into()
}

fn genre_color(name: &str) -> Color {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write(name.as_bytes());
    let x = h.finish();
    const P: [u32; 10] = [
        0xe13300, 0x8d67ab, 0x1e3264, 0xe8115b, 0x477d95, 0xba5d07, 0x148a08, 0xd84000, 0x8d67ab,
        0x777777,
    ];
    let c = P[(x % P.len() as u64) as usize];
    Color::from_rgb(
        ((c >> 16) & 0xff) as f32 / 255.0,
        ((c >> 8) & 0xff) as f32 / 255.0,
        (c & 0xff) as f32 / 255.0,
    )
}

fn sort_bar<'a>(
    current: &str,
    opts: &'a [(&'a str, &'a str)],
    route: &str,
) -> Element<'a, Message> {
    let mut r = Row::new().spacing(6).align_y(Alignment::Center);
    for (k, name) in opts {
        r = r.push(
            button(text(name.to_string()).size(12))
                .on_press(Message::Nav(format!("{route}:{k}")))
                .style(theme::chip(current == *k))
                .padding([5, 10]),
        );
    }
    row![Space::new().width(Fill), r].padding([8, 12]).into()
}

fn grid<'a, T>(
    app: &'a App,
    items: &'a [T],
    card_h: f32,
    card_w: f32,
    f: impl Fn(&'a App, &'a T) -> Element<'a, Message> + 'a,
) -> Element<'a, Message> {
    let (off, view) = app.scrolls.get(&app.route).copied().unwrap_or((0.0, 900.0));
    iced::widget::responsive(move |size| {
        let cols = ((size.width / card_w).floor() as usize).max(1);
        let rows_total = items.len().div_ceil(cols);
        let first = ((off / card_h) as usize).saturating_sub(1);
        let last = (((off + view) / card_h) as usize + 2).min(rows_total);
        let mut col = Column::new();
        col = col.push(Space::new().height(first as f32 * card_h));
        for r in first..last {
            col = col.push(row(items[r * cols..(r * cols + cols).min(items.len())]
                .iter()
                .map(|x| f(app, x))
                .collect::<Vec<Element<Message>>>()));
        }
        col = col.push(Space::new().height((rows_total - last) as f32 * card_h));
        col.into()
    })
    .into()
}

fn albums_grid<'a>(app: &'a App, sort: &str) -> Element<'a, Message> {
    let cur = if sort.is_empty() { "newest" } else { sort };
    let mut col = column![
        container(bold("Albums", 24.0)).padding([16, 12]),
        sort_bar(
            cur,
            &[
                ("newest", "Recently added"),
                ("name", "Name"),
                ("artist", "Artist"),
                ("year", "Year"),
                ("frequent", "Most played"),
                ("recent", "Recently played"),
                ("random", "Random"),
                ("rating", "Rating"),
            ],
            "albums",
        ),
    ];
    if let Some(l) = loading(app, &app.route) {
        return l;
    }
    if let Some((_, v)) = app.cache.get(&app.route)
        && let Some(p) = v.downcast_ref::<Pager<Album>>()
    {
        col = col.push(grid(app, &p.items, 246.0, 196.0, |app, a| {
            card(
                app,
                &a.cover,
                &a.name,
                &a.artist,
                format!("album:{}", a.id),
                false,
                Some(Message::PlayCtx(format!("album:{}", a.id), false)),
            )
        }));
    }
    col.into()
}

fn artists_grid<'a>(app: &'a App) -> Element<'a, Message> {
    let mut col = column![container(bold("Artists", 24.0)).padding([16, 12])];
    if let Some(l) = loading(app, &app.route) {
        return l;
    }
    if let Some((_, v)) = app.cache.get(&app.route)
        && let Some(p) = v.downcast_ref::<Pager<Artist>>()
    {
        col = col.push(grid(app, &p.items, 246.0, 196.0, |app, a| {
            card(app, &a.cover, &a.name, "Artist", format!("artist:{}", a.id), true, None)
        }));
    }
    col.into()
}

fn songs_page<'a>(app: &'a App, _sort: &str) -> Element<'a, Message> {
    if let Some(l) = loading(app, &app.route) {
        return l;
    }
    let mut col = column![container(bold("Songs", 24.0)).padding([16, 12])];
    if let Some((_, v)) = app.cache.get(&app.route)
        && let Some(p) = v.downcast_ref::<Pager<Song>>()
    {
        col = col.push(track_header(true, false));
        col = col.push(vlist(app, &p.items, "Songs", true, false, 110.0));
    }
    col.into()
}

fn genres_page<'a>(app: &'a App) -> Element<'a, Message> {
    search_page(app, "")
}

fn genre_page<'a>(app: &'a App, name: &str) -> Element<'a, Message> {
    if let Some(l) = loading(app, &app.route) {
        return l;
    }
    let mut col = column![container(bold(name.to_string(), 24.0)).padding([16, 12])];
    if let Some((_, v)) = app.cache.get(&app.route)
        && let Some(p) = v.downcast_ref::<Pager<Album>>()
    {
        col = col.push(grid(app, &p.items, 246.0, 196.0, |app, a| {
            card(
                app,
                &a.cover,
                &a.name,
                &a.artist,
                format!("album:{}", a.id),
                false,
                Some(Message::PlayCtx(format!("album:{}", a.id), false)),
            )
        }));
    }
    col.into()
}

fn folders_page<'a>(app: &'a App) -> Element<'a, Message> {
    if let Some(l) = loading(app, &app.route) {
        return l;
    }
    let mut col = column![container(bold("Folders", 24.0)).padding([16, 12])];
    if let Some((_, v)) = app.cache.get(&app.route)
        && let Some(a) = v.downcast_ref::<Vec<Artist>>()
    {
        for x in a.iter() {
            col = col.push(
                btn(
                    row![i(icon::FOLDER, 18.0).color(theme::SUB), text(x.name.clone()).size(14),]
                        .spacing(12)
                        .padding(10)
                        .align_y(Alignment::Center),
                    Message::Nav(format!("folder:{}", x.id)),
                )
                .width(Fill),
            );
        }
    }
    col.into()
}

fn folder_page<'a>(app: &'a App, _id: &str) -> Element<'a, Message> {
    if let Some(l) = loading(app, &app.route) {
        return l;
    }
    let mut col = Column::new();
    if let Some((_, v)) = app.cache.get(&app.route)
        && let Some(songs) = v.downcast_ref::<Vec<Song>>()
    {
        for (idx, s) in songs.iter().enumerate() {
            if s.is_dir {
                col = col.push(
                    btn(
                        row![
                            i(icon::FOLDER, 18.0).color(theme::SUB),
                            text(s.title.clone()).size(14),
                        ]
                        .spacing(12)
                        .padding(10)
                        .align_y(Alignment::Center),
                        Message::Nav(format!("folder:{}", s.id)),
                    )
                    .width(Fill),
                );
            } else {
                col = col.push(track_row(app, s, idx, songs, "Folders", false, false, None));
            }
        }
    }
    col.into()
}

fn lyrics_page<'a>(app: &'a App) -> Element<'a, Message> {
    let Some(song) = app.current_song() else {
        return container(theme::sub("Nothing is playing.", 16.0)).padding(60).into();
    };
    let key = format!("lyrics:{}", song.id);
    if let Some(l) = loading(app, &key) {
        return l;
    }
    let c = head_color(app, &song.cover);
    let Some((_, v)) = app.cache.get(&key) else { return Space::new().into() };
    let Some(ly) = v.downcast_ref::<Vec<Lyrics>>() else { return Space::new().into() };
    let pos_ms = (app.position * 1000.0) as i64;
    let mut col = Column::new().spacing(8).padding([60, 40]).width(Fill);
    if ly.is_empty() || ly.iter().all(|l| l.lines.is_empty()) {
        return container(theme::sub("No lyrics are available for this song.", 16.0))
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .style(theme::header(c))
            .into();
    }
    let l = &ly[0];
    let cur = l
        .lines
        .iter()
        .enumerate()
        .filter(|(_, (t, _))| t.is_some_and(|t| (t as i64 + l.offset) <= pos_ms))
        .map(|(i, _)| i)
        .next_back()
        .unwrap_or(0);
    for (i, (t, line)) in l.lines.iter().enumerate() {
        let (colr, bolded) = if !l.synced {
            (theme::TEXT, false)
        } else if i < cur {
            (theme::tint(0.7), false)
        } else if i == cur {
            (theme::TEXT, true)
        } else {
            (Color { a: 0.55, ..theme::BG }, false)
        };
        let mut el = text(line.clone()).size(28).color(colr);
        if bolded || !l.synced {
            el = el.font(Font { weight: iced::font::Weight::Bold, ..Font::DEFAULT });
        }
        let seek_ms = t.map(|t| (t as i64 + l.offset).max(0) as f32 / 1000.0);
        let line: Element<Message> = match seek_ms {
            Some(ms) => btn(el, Message::Seek(ms)).into(),
            None => el.into(),
        };
        col = col.push(line);
    }
    container(col).style(theme::header(c)).into()
}

fn settings_page<'a>(app: &'a App) -> Element<'a, Message> {
    let st = &app.cfg.settings;
    let sct: fn(&'a str) -> Element<'a, Message> =
        |t| container(bold(t, 20.0)).padding(theme::pad(24.0, 12.0, 8.0, 12.0)).into();
    let row_s = |label: &str, c: Element<'a, Message>| {
        row![theme::sub(label.to_string(), 14.0).width(220), container(c).width(Fill),]
            .align_y(Alignment::Center)
            .padding([6, 12])
    };
    let pick = |opts: &'a [&'a str], cur: &'a str, f: fn(String) -> Message| {
        pick_list(
            opts.iter().map(|s| s.to_string()).collect::<Vec<String>>(),
            Some(cur.to_string()),
            f,
        )
        .text_size(13)
        .padding([6, 10])
    };
    let mut col = column![container(bold("Settings", 26.0)).padding([16, 12])];
    col = col.push(sct("Playback"));
    col = col.push(row_s(
        "ReplayGain",
        pick(&["off", "track", "album"], st.replaygain.as_str(), |o| Message::Set(Set::Rg(o)))
            .into(),
    ));
    col = col.push(row_s(
        "Preamp (dB)",
        row![
            slider(-15.0..=15.0, st.preamp_db, |v| Message::Set(Set::Preamp(v))).width(200),
            theme::sub(format!("{:.1}", st.preamp_db), 13.0).width(48),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .into(),
    ));
    col = col.push(row_s(
        "Crossfade (s)",
        row![
            slider(0.0..=12.0, st.crossfade_secs, |v| Message::Set(Set::Xfade(v))).width(200),
            theme::sub(format!("{:.0}", st.crossfade_secs), 13.0).width(48),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .into(),
    ));
    col = col.push(row_s(
        "Speed",
        row![
            slider(0.5..=2.0, st.speed, |v| Message::Set(Set::Speed(v))).width(200),
            theme::sub(format!("{:.2}x", st.speed), 13.0).width(48),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .into(),
    ));
    col = col.push(row_s(
        "Transcode format",
        pick(&["original", "mp3", "opus", "aac"], st.transcode_format.as_str(), |o| {
            Message::Set(Set::Trans(match o.as_str() {
                "mp3" => 1,
                "opus" => 2,
                "aac" => 3,
                _ => 0,
            }))
        })
        .into(),
    ));
    col = col.push(row_s(
        "Max bitrate",
        pick(
            &["unlimited", "320", "256", "192", "128", "96"],
            if st.transcode_max_bitrate == 0 { "unlimited" } else { "0" },
            |o| Message::Set(Set::Bitrate(o.parse().unwrap_or(0))),
        )
        .into(),
    ));
    col = col.push(sct("Library"));
    col = col.push(row_s(
        "Sync play queue",
        toggler(st.queue_sync).on_toggle(|b| Message::Set(Set::Qsync(b))).into(),
    ));
    col = col.push(sct("Integrations"));
    col = col.push(row_s(
        "Discord Rich Presence",
        toggler(st.discord).on_toggle(|b| Message::Set(Set::Discord(b))).into(),
    ));
    col = col.push(sct("Servers"));
    for s in &app.cfg.servers {
        col = col.push(
            row![
                column![text(s.name.clone()).size(14), theme::sub(s.user.clone(), 12.0),],
                Space::new().width(Fill),
                btn(theme::sub("Log out", 13.0), Message::LogOut),
            ]
            .padding([6, 12])
            .align_y(Alignment::Center),
        );
    }
    col = col.push(sct("Cache"));
    col = col.push(row_s(
        "Cover cache",
        row![
            theme::sub(format!("{} MB on disk", app.covers.bytes / 1048576), 13.0),
            Space::new().width(16),
            btn(theme::sub("Clear cache", 13.0), Message::ClearCache),
        ]
        .align_y(Alignment::Center)
        .into(),
    ));
    col = col.push(sct("About"));
    col = col.push(
        row![
            theme::sub(format!("navify {}", env!("CARGO_PKG_VERSION")), 13.0),
            Space::new().width(Fill),
            btn(theme::sub("Licenses", 13.0), Message::Noop),
        ]
        .padding(theme::pad(6.0, 12.0, 24.0, 12.0)),
    );
    col.into()
}

pub fn now_playing<'a>(app: &'a App) -> Element<'a, Message> {
    let Some(s) = app.current_song() else {
        return container(theme::sub("Nothing is playing.", 14.0)).padding(24).into();
    };
    let starred = app.starred.get(&s.id).copied().unwrap_or(s.starred);
    column![
        row![
            bold("Now playing", 16.0),
            Space::new().width(Fill),
            theme::sub(app.source.clone(), 12.0),
        ]
        .padding(theme::pad(16.0, 16.0, 8.0, 16.0))
        .align_y(Alignment::Center),
        scrollable(
            column![
                container(cover(app, &s.cover, 640, 280.0, false)).padding([8, 16]),
                row![
                    column![bold(s.title.clone(), 20.0), artists_line(&s.artists, 13.0),]
                        .spacing(4),
                    Space::new().width(Fill),
                    icon_btn(icon::HEART, 18.0, Message::Star(s.id.clone(), !starred)),
                ]
                .padding([8, 16])
                .align_y(Alignment::Center),
                container(
                    column![bold("About the artist", 15.0), theme::sub(s.artist.clone(), 13.0),]
                        .spacing(8)
                        .padding(16)
                        .width(Fill),
                )
                .style(theme::elevated)
                .padding([8, 16]),
            ]
            .spacing(8),
        )
        .style(theme::scroll),
    ]
    .into()
}

pub fn queue_panel<'a>(app: &'a App) -> Element<'a, Message> {
    let mut col = column![
        row![
            bold("Queue", 16.0),
            Space::new().width(Fill),
            btn(theme::sub("Clear queue", 12.0), Message::QClear),
        ]
        .padding(theme::pad(16.0, 16.0, 8.0, 16.0))
        .align_y(Alignment::Center),
    ];
    let mut list = Column::new();
    if let Some(s) = app.current_song() {
        list = list.push(container(theme::sub("Now playing", 12.0)).padding([8, 16]));
        list = list.push(track_row(app, s, app.index, &app.queue, "Queue", false, false, None));
    }
    list = list
        .push(container(theme::sub("Next up", 12.0)).padding(theme::pad(12.0, 16.0, 4.0, 16.0)));
    for i in (app.index + 1)
        ..app.queue.len().max(app.shuffle.as_ref().map_or(0, |o| o.len()).max(app.queue.len()))
    {
        let ri = app.real_at(i);
        if ri >= app.queue.len() {
            break;
        }
        let s = &app.queue[ri];
        list = list.push(
            row![
                mouse_area(
                    row![
                        cover(app, &s.cover, 128, 40.0, false),
                        column![
                            text(s.title.clone()).size(13),
                            theme::sub(s.artist.clone(), 11.0),
                        ]
                        .spacing(2)
                        .width(Fill),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .padding([4, 12]),
                )
                .on_double_click(Message::PlayQueue(i)),
                icon_btn(icon::X, 13.0, Message::QDel(i)),
            ]
            .align_y(Alignment::Center),
        );
    }
    col = col.push(scrollable(list).style(theme::scroll));
    col.into()
}

pub fn menu<'a>(app: &'a App, m: &'a Menu) -> Element<'a, Message> {
    let items: Vec<Element<Message>> = match &m.kind {
        MenuKind::Speed => [0.5, 0.75, 1.0, 1.25, 1.5, 2.0]
            .iter()
            .map(|v| {
                menu_item(
                    if (app.cfg.settings.speed - v).abs() < 0.01 {
                        icon::CHECK
                    } else {
                        icon::SPEED
                    },
                    &format!("{v:.2}x"),
                    Message::Set(Set::Speed(*v)),
                )
            })
            .collect(),
        MenuKind::User => {
            let mut v: Vec<Element<Message>> = vec![
                container(
                    column![
                        text(app.session.as_ref().map_or(String::new(), |s| s.server.name.clone()))
                            .size(14),
                        theme::sub(
                            app.session.as_ref().map_or(String::new(), |s| s.server.url.clone()),
                            11.0
                        ),
                    ]
                    .padding([8, 12]),
                )
                .into(),
            ];
            for s in &app.cfg.servers {
                if Some(&s.id) != app.cfg.active.as_ref() {
                    v.push(menu_item(
                        icon::SERVER,
                        &format!("Switch to {}", s.name),
                        Message::UseServer(s.id.clone()),
                    ));
                }
            }
            v.push(menu_item(icon::LOGOUT, "Log out", Message::LogOut));
            v
        }
        MenuKind::Song(s) | MenuKind::SongIn(s, _) => {
            let starred = app.starred.get(&s.id).copied().unwrap_or(s.starred);
            let mut v: Vec<Element<Message>> = vec![
                menu_item(icon::QUEUE, "Add to queue", Message::QEnd(vec![s.clone()])),
                menu_item(icon::LISTPLUS, "Play next", Message::QNext(vec![s.clone()])),
            ];
            if !s.artist_id.is_empty() {
                v.push(menu_item(
                    icon::USER,
                    "Go to artist",
                    Message::Nav(format!("artist:{}", s.artist_id)),
                ));
            }
            if !s.album_id.is_empty() {
                v.push(menu_item(
                    icon::ALBUM,
                    "Go to album",
                    Message::Nav(format!("album:{}", s.album_id)),
                ));
            }
            v.push(menu_item(
                icon::HEART,
                if starred { "Remove from Liked Songs" } else { "Save to Liked Songs" },
                Message::Star(s.id.clone(), !starred),
            ));
            v.push(menu_sub(icon::PLUS, "Add to playlist", 1));
            if let MenuKind::SongIn(_, i) = &m.kind {
                v.push(menu_item(icon::MINUS, "Remove from this playlist", Message::PlRemove(*i)));
            }
            v.push(menu_sub(icon::STAR, "Rate", 2));
            v.push(menu_item(icon::RADIO, "Start radio", Message::Radio(s.id.clone())));
            match m.sub {
                1 => {
                    let mut pl = vec![menu_item(
                        icon::PLUS,
                        "New playlist",
                        Message::Modal(String::new(), false),
                    )];
                    if let Some((_, lv)) = app.cache.get("lib")
                        && let Some(l) = lv.downcast_ref::<Lib>()
                    {
                        for p in &l.playlists {
                            pl.push(menu_item(
                                icon::MUSIC,
                                &p.name,
                                Message::PlAdd(p.id.clone(), vec![s.id.clone()]),
                            ));
                        }
                    }
                    return container(column(pl)).style(theme::menu).into();
                }
                2 => {
                    return container(
                        row((0..=5)
                            .map(|n| {
                                btn(
                                    i(icon::STAR, 16.0).color(theme::SUB).center(),
                                    Message::Rate(s.id.clone(), n),
                                )
                                .into()
                            })
                            .collect::<Vec<Element<Message>>>())
                        .padding(8),
                    )
                    .style(theme::menu)
                    .into();
                }
                _ => {}
            }
            v
        }
        MenuKind::Album(a) => vec![
            menu_item(icon::PLAY, "Play", Message::PlayCtx(format!("album:{}", a.id), false)),
            menu_item(icon::SHUFFLE, "Shuffle", Message::PlayCtx(format!("album:{}", a.id), true)),
            menu_item(icon::QUEUE, "Add to queue", Message::AlbumQueue(a.id.clone())),
            menu_item(
                icon::HEART,
                if app.starred.get(&a.id).copied().unwrap_or(a.starred) {
                    "Remove from Liked"
                } else {
                    "Like"
                },
                Message::Star(a.id.clone(), !app.starred.get(&a.id).copied().unwrap_or(a.starred)),
            ),
            menu_item(icon::USER, "Go to artist", Message::Nav(format!("artist:{}", a.artist_id))),
        ],
        MenuKind::Artist(a) => vec![
            menu_item(icon::PLAY, "Play", Message::PlayCtx(format!("artist:{}", a.id), false)),
            menu_item(icon::SHUFFLE, "Shuffle", Message::PlayCtx(format!("artist:{}", a.id), true)),
            menu_item(
                icon::HEART,
                if app.starred.get(&a.id).copied().unwrap_or(a.starred) {
                    "Unfollow"
                } else {
                    "Follow"
                },
                Message::Star(a.id.clone(), !app.starred.get(&a.id).copied().unwrap_or(a.starred)),
            ),
        ],
        MenuKind::Playlist(p) => vec![
            menu_item(icon::PLAY, "Play", Message::PlayCtx(format!("playlist:{}", p.id), false)),
            menu_item(
                icon::SHUFFLE,
                "Shuffle",
                Message::PlayCtx(format!("playlist:{}", p.id), true),
            ),
            menu_item(icon::PENCIL, "Rename", Message::Modal(p.id.clone(), true)),
            menu_item(icon::TRASH, "Delete", Message::PlDel(p.id.clone())),
        ],
    };
    container(column(items)).style(theme::menu).into()
}

pub fn login<'a>(app: &'a App) -> Element<'a, Message> {
    let l = &app.login;
    let mut col = Column::new().spacing(24).width(480);
    col = col.push(
        row![i(icon::MUSIC, 30.0).color(theme::ACCENT), theme::black("navify", 30.0)]
            .spacing(8)
            .align_y(Alignment::Center),
    );
    col = col.push(theme::black("Log in to navify", 44.0));
    col = col.push(
        container(Space::new())
            .height(1)
            .style(|_| container::Style::default().background(theme::tint(0.15))),
    );
    if !app.cfg.servers.is_empty() {
        let mut srvs = Column::new().spacing(2);
        for s in &app.cfg.servers {
            let initial = s.user.chars().next().unwrap_or('?').to_ascii_uppercase().to_string();
            srvs = srvs.push(
                button(
                    row![
                        container(
                            text(initial)
                                .size(16)
                                .font(theme::fw(iced::font::Weight::Bold))
                                .center(),
                        )
                        .width(40)
                        .height(40)
                        .center_x(40)
                        .center_y(40)
                        .style(|_| {
                            container::Style::default()
                                .background(theme::CHIP)
                                .border(iced::Border::default().rounded(500.0))
                        }),
                        column![
                            theme::med(s.name.clone(), 15.0),
                            theme::sub2(s.user.clone(), 13.0),
                        ]
                        .spacing(2),
                        Space::new().width(Fill),
                        icon_btn(icon::X, 14.0, Message::DropServer(s.id.clone())),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center)
                    .padding([8, 4]),
                )
                .on_press(Message::UseServer(s.id.clone()))
                .style(theme::menu_item)
                .width(Fill),
            );
        }
        col = col.push(srvs);
        col = col.push(
            container(Space::new())
                .height(1)
                .style(|_| container::Style::default().background(theme::tint(0.15))),
        );
    }
    for (idx, (label, v, secure)) in [
        (0u8, ("Server address", l.url.as_str(), false)),
        (1, ("Username", l.user.as_str(), false)),
        (2, ("Password", l.pass.as_str(), true)),
    ] {
        let mut f = text_input("", v)
            .on_input(move |s| Message::LField(idx, s))
            .on_submit(Message::LSubmit)
            .style(theme::field)
            .padding(14)
            .size(15);
        if secure {
            f = f.secure(true);
        }
        col = col.push(column![theme::bold(label, 14.0), f].spacing(8));
    }
    col = col.push(
        row![
            toggler(l.remember).on_toggle(Message::LRemember).style(theme::tog).size(18),
            theme::med("Remember password", 14.0),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    );
    if !l.error.is_empty() {
        col = col.push(text(l.error.clone()).size(13).color(theme::ERR));
    }
    col = col.push(
        button(theme::bold("Log In", 16.0).color(theme::BG).center().width(Fill))
            .style(theme::accent)
            .width(Fill)
            .padding(14)
            .on_press(Message::LSubmit),
    );
    if l.busy {
        col = col.push(theme::sub2("Signing in.", 13.0));
    }
    container(container(col).padding(40).width(560).style(|_| {
        container::Style::default()
            .background(theme::PANEL)
            .border(iced::Border::default().rounded(8.0))
    }))
    .width(Fill)
    .height(Fill)
    .center_x(Fill)
    .center_y(Fill)
    .style(|_| {
        container::Style::default().background(iced::Gradient::Linear(
            iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI * 1.5))
                .add_stop(0.0, theme::tint(0.1))
                .add_stop(1.0, theme::BG),
        ))
    })
    .into()
}
