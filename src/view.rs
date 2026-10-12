use iced::widget::{
    Column, Row, Space, button, column, container, image, mouse_area, row, scrollable, slider,
    stack, text, text_input,
};
use iced::{Color, Element, Fill, Font, Length};

use crate::api::Artist;
use crate::app::{App, MenuKind, Message};
use crate::pages;
use crate::theme::{self, icon, icon as i};

pub const SCROLL: &str = "main-scroll";

pub fn btn<'a>(c: impl Into<Element<'a, Message>>, msg: Message) -> button::Button<'a, Message> {
    button(c).on_press(msg).padding(0).style(theme::ghost)
}

pub fn icon_btn<'a>(c: char, size: f32, msg: Message) -> button::Button<'a, Message> {
    btn(i(c, size).center(), msg).width(size * 2.2).height(size * 2.2)
}

pub fn cover(app: &App, id: &str, size: u32, px: f32, round: bool) -> Element<'static, Message> {
    let el: Element<Message> = match app.covers.get(&(id.to_string(), size)) {
        Some(h) if !id.is_empty() => {
            image(h).width(Length::Fixed(px)).height(Length::Fixed(px)).into()
        }
        _ => container(i(icon::IMAGE, px * 0.3).color(theme::SUB).center())
            .width(px)
            .height(px)
            .style(theme::elevated)
            .center_x(px)
            .center_y(px)
            .into(),
    };
    let r = if round { px / 2.0 } else { 6.0 };
    container(el)
        .clip(true)
        .style(move |_| container::Style::default().border(iced::Border::default().rounded(r)))
        .into()
}

pub fn artists_line<'a>(artists: &[Artist], size: f32) -> Element<'a, Message> {
    let mut r = Row::new().spacing(0);
    for (i, a) in artists.iter().enumerate() {
        if i > 0 {
            r = r.push(theme::sub(", ", size));
        }
        let link: Element<Message> = if a.id.is_empty() {
            theme::sub(a.name.clone(), size).into()
        } else {
            btn(theme::sub(a.name.clone(), size), Message::Nav(format!("artist:{}", a.id))).into()
        };
        r = r.push(link);
    }
    r.into()
}

fn head_title(app: &App) -> Option<(String, iced::Color, Option<Message>)> {
    let off = app.scrolls.get(&app.route).map(|(o, _)| *o).unwrap_or(0.0);
    if off < 260.0 {
        return None;
    }
    let (kind, id) = app.route.split_once(':').unwrap_or((app.route.as_str(), ""));
    let v = app.cache.get(&app.route).map(|(_, v)| v)?;
    match kind {
        "album" => {
            let (a, songs) = v.downcast_ref::<(crate::api::Album, Vec<crate::api::Song>)>()?;
            Some((
                a.name.clone(),
                pages_head_color(app, &a.cover),
                Some(Message::Play(songs.clone(), 0, format!("album:{id}"))),
            ))
        }
        "playlist" => {
            let (p, songs) =
                v.downcast_ref::<(crate::api::Playlist, Vec<crate::api::Song>)>()?;
            Some((
                p.name.clone(),
                pages_head_color(app, &p.cover),
                Some(Message::Play(songs.clone(), 0, format!("playlist:{id}"))),
            ))
        }
        "liked" => {
            let p = v.downcast_ref::<crate::app::Pager<crate::api::Song>>()?;
            Some((
                "Liked Songs".into(),
                Color::from_rgb8(0x50, 0x3a, 0x55),
                Some(Message::Play(p.items.clone(), 0, "liked".into())),
            ))
        }
        "artist" => {
            let (a, _, top, _) = v.downcast_ref::<(
                crate::api::Artist,
                Vec<crate::api::Album>,
                Vec<crate::api::Song>,
                crate::api::Info,
            )>()?;
            Some((
                a.name.clone(),
                Color::from_rgb8(0x50, 0x3a, 0x55),
                Some(Message::Play(top.clone(), 0, format!("artist:{id}"))),
            ))
        }
        _ => None,
    }
}

fn pages_head_color(app: &App, id: &str) -> iced::Color {
    app.covers.color.get(id).copied().unwrap_or(Color::from_rgb8(0x50, 0x3a, 0x55))
}

fn top_bar<'a>(app: &'a App) -> Element<'a, Message> {
    let initial = app
        .session
        .as_ref()
        .and_then(|s| s.server.user.chars().next())
        .map(|c| c.to_ascii_uppercase().to_string())
        .unwrap_or_else(|| "?".into());
    let avatar = button(
        container(text(initial).size(14).center())
            .width(32)
            .height(32)
            .center_x(32)
            .center_y(32)
            .style(|_| {
                container::Style::default()
                    .background(theme::CHIP)
                    .border(iced::Border::default().rounded(500.0))
            }),
    )
    .padding(0)
    .style(theme::ghost)
    .on_press(Message::Menu(Box::new(MenuKind::User)));
    let back_on = app.hi > 0;
    let fwd_on = app.hi + 1 < app.history.len();
    let navb = |c: char, on: bool, m: Message| -> Element<'a, Message> {
        button(
            i(c, 16.0)
                .color(if on { theme::TEXT } else { Color::from_rgb8(0x6a, 0x6a, 0x6a) })
                .center(),
        )
        .width(32)
        .height(32)
        .padding(0)
        .style(theme::navbtn(on))
        .on_press(m)
        .into()
    };
    let mini = head_title(app);
    let mini2 = head_title(app);
    let mut mini_el: Element<'a, Message> = Space::new().into();
    if let Some((title, _c, play)) = mini {
        let pl: Element<'a, Message> = match play {
            Some(m) => button(
                i(if app.playing { icon::PAUSE } else { icon::PLAY }, 15.0)
                    .color(theme::BG)
                    .center(),
            )
            .width(34)
            .height(34)
            .padding(0)
            .style(|_, _| button::Style {
                background: Some(theme::ACCENT.into()),
                border: iced::Border::default().rounded(500.0),
                ..button::Style::default()
            })
            .on_press(m)
            .into(),
            None => Space::new().into(),
        };
        mini_el = row![pl, Space::new().width(12), theme::bold(title, 17.0)]
            .align_y(iced::Alignment::Center)
            .into();
    }
    let r = row![
        navb(icon::BACK, back_on, Message::Back),
        navb(icon::FWD, fwd_on, Message::Fwd),
        btn(
            container(i(icon::HOME, 20.0).center())
                .width(48)
                .height(48)
                .center_x(48)
                .center_y(48)
                .style(theme::elevated),
            Message::Nav("home".into())
        ),
        container(
            text_input("What do you want to play?", &app.search_q)
                .id("search")
                .on_input(Message::Search)
                .on_submit(Message::SearchGo(app.search_seq))
                .style(theme::search_field)
                .icon(text_input::Icon {
                    font: theme::LUCIDE,
                    code_point: icon::SEARCH,
                    size: Some(18.0.into()),
                    spacing: 10.0,
                    side: text_input::Side::Left,
                })
                .padding([0, 16])
                .width(360)
                .size(14)
        )
        .height(48)
        .center_y(48),
        mini_el,
        Space::new().width(Fill),
        icon_btn(icon::SETTINGS, 20.0, Message::Nav("settings".into())),
        avatar,
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .height(64);
    let hdr = container(r)
        .width(Fill)
        .padding(theme::pad(0.0, 16.0, 0.0, 16.0));
    if let Some((_, c, _)) = mini2 {
        hdr.style(move |_| container::Style::default().background(c)).into()
    } else {
        hdr.into()
    }
}

fn lib_row<'a>(
    app: &App,
    cover_id: &str,
    title: &str,
    sub: String,
    route: &str,
    circle: bool,
) -> Element<'a, Message> {
    if app.lib_collapsed {
        btn(cover(app, cover_id, 128, 48.0, circle), Message::Nav(route.into())).into()
    } else {
        btn(
            row![
                cover(app, cover_id, 128, 48.0, circle),
                column![
                    text(title.to_string()).size(14).color(if app.route == route {
                        theme::ACCENT
                    } else {
                        theme::TEXT
                    }),
                    theme::sub(sub, 12.0),
                ]
                .spacing(2),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center)
            .padding(8)
            .width(Fill),
            Message::Nav(route.into()),
        )
        .into()
    }
}

fn lib_panel<'a>(app: &'a App) -> Element<'a, Message> {
    let w = if app.lib_collapsed { 72.0 } else { 300.0 };
    let head = row![
        if app.lib_collapsed {
            Element::from(Space::new().width(0))
        } else {
            row![
                i(icon::LIBRARY, 20.0).color(theme::SUB),
                text("Your Library")
                    .size(15)
                    .font(Font { weight: iced::font::Weight::Bold, ..Font::DEFAULT }),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center)
            .into()
        },
        Space::new().width(Fill),
        icon_btn(icon::PLUS, 18.0, Message::NewPlaylist),
        icon_btn(icon::PL, 16.0, Message::LibCollapse),
    ]
    .align_y(iced::Alignment::Center)
    .padding([8, 10]);
    let mut chips = Row::new().spacing(6);
    if !app.lib_collapsed {
        for (n, name) in [(1u8, "Playlists"), (2, "Albums"), (3, "Artists")] {
            chips = chips.push(
                button(text(name).size(13))
                    .on_press(Message::LibFilter(n))
                    .style(theme::chip(app.lib_filter == n))
                    .padding([6, 12]),
            );
        }
    }
    let mut list = Column::new().spacing(2);
    list = list.push(lib_row(app, "", "Liked Songs", "Playlist".into(), "liked", false));
    if let Some((_, v)) = app.cache.get("lib")
        && let Some(l) = v.downcast_ref::<crate::app::Lib>()
    {
        if matches!(app.lib_filter, 0 | 1) {
            for p in &l.playlists {
                list = list.push(lib_row(
                    app,
                    &p.cover,
                    &p.name,
                    format!("Playlist \u{2022} {}", p.owner),
                    &format!("playlist:{}", p.id),
                    false,
                ));
            }
        }
        if matches!(app.lib_filter, 0 | 2) {
            for a in &l.albums {
                list = list.push(lib_row(
                    app,
                    &a.cover,
                    &a.name,
                    format!("Album \u{2022} {}", a.artist),
                    &format!("album:{}", a.id),
                    false,
                ));
            }
        }
        if matches!(app.lib_filter, 0 | 3) {
            for a in &l.artists {
                list = list.push(lib_row(
                    app,
                    &a.cover,
                    &a.name,
                    "Artist".into(),
                    &format!("artist:{}", a.id),
                    true,
                ));
            }
        }
    }
    container(
        column![head, chips.padding([0, 10]), scrollable(list.padding(8)).style(theme::scroll)]
            .spacing(4),
    )
    .width(w)
    .height(Fill)
    .style(theme::panel)
    .into()
}

pub fn fmt_dur(s: f32) -> String {
    let s = s as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn fmt_long(s: f32) -> String {
    let s = s as u64;
    if s >= 3600 {
        format!("{} hr {} min", s / 3600, s % 3600 / 60)
    } else {
        format!("{} min {} sec", s / 60, s % 60)
    }
}

fn player_bar<'a>(app: &'a App) -> Element<'a, Message> {
    let song = app.current_song();
    let left: Element<Message> = match song {
        Some(s) => row![
            cover(app, &s.cover, 128, 56.0, false),
            column![
                btn(text(s.title.clone()).size(13), Message::Nav(format!("album:{}", s.album_id))),
                artists_line(&s.artists, 11.0),
            ]
            .spacing(2),
            icon_btn(
                icon::HEART,
                16.0,
                Message::Star(s.id.clone(), !app.starred.get(&s.id).copied().unwrap_or(s.starred)),
            )
            .style(move |_, _| button::Style {
                text_color: if app.starred.get(&s.id).copied().unwrap_or(s.starred) {
                    theme::ACCENT
                } else {
                    theme::SUB
                },
                ..button::Style::default()
            }),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center)
        .into(),
        None => Space::new().width(0).into(),
    };
    let dur = song.map_or(0.0, |s| s.duration).max(1.0);
    let pos = app.position.min(dur);
    let center = column![
        row![
            btn(
                i(icon::SHUFFLE, 16.0)
                    .color(if app.shuffle.is_some() { theme::ACCENT } else { theme::SUB })
                    .center(),
                Message::Shuffle
            ),
            icon_btn(icon::PREV, 18.0, Message::Prev),
            btn(
                container(
                    i(if app.playing { icon::PAUSE } else { icon::PLAY }, 16.0)
                        .color(theme::BG)
                        .center()
                )
                .width(32)
                .height(32)
                .center_x(32)
                .center_y(32)
                .style(|_| container::Style::default()
                    .background(theme::TEXT)
                    .border(iced::Border::default().rounded(500.0))),
                Message::Toggle,
            ),
            icon_btn(icon::NEXT, 18.0, Message::Next(true)),
            btn(
                column![
                    i(if app.repeat == 2 { icon::REPEAT1 } else { icon::REPEAT }, 16.0)
                        .color(if app.repeat > 0 { theme::ACCENT } else { theme::SUB })
                        .center(),
                    if app.repeat > 0 {
                        text("\u{2022}").size(8).color(theme::ACCENT).center()
                    } else {
                        text(" ").size(8)
                    },
                ]
                .spacing(0),
                Message::Repeat,
            ),
        ]
        .spacing(16)
        .align_y(iced::Alignment::Center),
        row![
            theme::sub(fmt_dur(pos), 11.0),
            container(
                slider(0.0..=dur, app.seek_preview.unwrap_or(pos), Message::SeekPreview)
                    .on_release(Message::SeekRelease)
                    .style(theme::seek)
            )
            .width(Fill)
            .center_y(28),
            theme::sub(fmt_dur(dur), 11.0),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    ]
    .align_x(iced::Alignment::Center)
    .width(Fill);
    let vol_icon = if app.muted || app.volume == 0.0 {
        icon::MUTE
    } else if app.volume < 0.5 {
        icon::VOL1
    } else {
        icon::VOL2
    };
    let right = row![
        icon_btn(icon::PR, 16.0, Message::Panel(1)),
        icon_btn(icon::LYRICS, 16.0, Message::Nav("lyrics".into())),
        icon_btn(icon::QUEUE, 16.0, Message::Panel(2)),
        btn(
            i(icon::SPEED, 16.0).color(theme::SUB).center(),
            Message::Menu(Box::new(MenuKind::Speed))
        ),
        icon_btn(vol_icon, 16.0, Message::Mute),
        container(
            slider(0.0..=1.0, if app.muted { 0.0 } else { app.volume }, Message::Vol)
                .width(93)
                .style(theme::seek)
        )
        .center_y(28),
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center);
    container(
        row![
            container(left).width(Fill).align_x(iced::Alignment::Start),
            center.width(Fill),
            container(right).width(Fill).align_x(iced::Alignment::End),
        ]
        .align_y(iced::Alignment::Center)
        .padding([0, 16]),
    )
    .height(72)
    .center_y(72)
    .into()
}

fn right_panel<'a>(app: &'a App) -> Option<Element<'a, Message>> {
    if app.panel == 0 {
        return None;
    }
    let content: Element<Message> =
        if app.panel == 1 { pages::now_playing(app) } else { pages::queue_panel(app) };
    Some(container(content).width(320).height(Fill).style(theme::panel).into())
}

fn menu_overlay<'a>(app: &'a App) -> Option<Element<'a, Message>> {
    let m = app.menu.as_ref()?;
    let menu_el = pages::menu(app, m);
    let pos_layer = column![
        Space::new().height(m.y.max(0.0)),
        row![Space::new().width(m.x.max(0.0)), menu_el],
    ];
    Some(
        stack![
            mouse_area(Space::new().width(Fill).height(Fill))
                .on_press(Message::MenuClose)
                .on_right_press(Message::MenuClose),
            pos_layer,
        ]
        .into(),
    )
}

fn modal_overlay(app: &App) -> Option<Element<'static, Message>> {
    let (id, rename) = app.modal.as_ref()?;
    let title = if *rename { "Rename playlist" } else { "New playlist" };
    let id = id.clone();
    Some(
        stack![
            mouse_area(Space::new().width(Fill).height(Fill)).on_press(Message::Noop),
            container(
                column![
                    text(title)
                        .size(20)
                        .font(Font { weight: iced::font::Weight::Bold, ..Font::DEFAULT }),
                    text_input("Playlist name", &app.new_pl)
                        .on_input(|v| Message::LField(3, v))
                        .on_submit(if *rename {
                            Message::Rename(id.clone(), app.new_pl.clone())
                        } else {
                            Message::PlNew(app.new_pl.clone())
                        })
                        .style(theme::field)
                        .padding(10),
                    row![
                        Space::new().width(Fill),
                        btn(theme::sub("Cancel", 14.0), Message::Noop),
                        button(text("Save").size(14).color(theme::BG))
                            .style(theme::accent)
                            .padding([8, 20])
                            .on_press(if *rename {
                                Message::Rename(id, app.new_pl.clone())
                            } else {
                                Message::PlNew(app.new_pl.clone())
                            }),
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
                ]
                .spacing(16)
                .width(360)
                .padding(20),
            )
            .style(theme::menu),
        ]
        .into(),
    )
}

fn toast_overlay(app: &App) -> Option<Element<'static, Message>> {
    let (_, t) = app.toast.as_ref()?;
    Some(
        container(container(text(t.clone()).size(13)).padding([10, 20]).style(theme::toast))
            .width(Fill)
            .height(Fill)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::End)
            .padding([0, 0])
            .into(),
    )
}

pub fn view(app: &App) -> Element<'_, Message> {
    if app.session.is_none() {
        return pages::login(app);
    }
    let mut main_stack = stack![scrollable(pages::route(app))
        .id(SCROLL)
        .on_scroll(|v| {
            let o = v.absolute_offset();
            Message::Scrolled(o.y, v.bounds().height, v.content_bounds().height)
        })
        .style(theme::scroll)];
    if let Some(h) = pages::sticky_track_header(app) {
        main_stack = main_stack.push(h);
    }
    let main = column![
        top_bar(app),
        container(main_stack).padding(theme::pad(0.0, 16.0, 16.0, 16.0)),
    ];
    let layout = column![
        {
            let mut r =
                row![lib_panel(app), container(main).width(Fill).height(Fill).style(theme::panel)]
                    .spacing(8)
                    .height(Fill);
            if let Some(p) = right_panel(app) {
                r = r.push(p);
            }
            r
        },
        player_bar(app),
    ]
    .spacing(8)
    .padding(8);
    let base: Element<Message> =
        container(layout).style(|_| container::Style::default().background(theme::BG)).into();
    let mut layers = vec![base];
    if let Some(m) = modal_overlay(app) {
        layers.push(m);
    }
    if let Some(m) = menu_overlay(app) {
        layers.push(m);
    }
    if let Some(t) = toast_overlay(app) {
        layers.push(t);
    }
    stack(layers).into()
}
