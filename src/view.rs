use iced::font::Weight;
use iced::widget::{
    Column, Space, button, column, container, image, mouse_area, row, scrollable, slider, stack,
    text, text_input,
};
use iced::{Alignment, Color, Element, Fill, Length};

use crate::api::Artist;
use crate::app::{App, MenuKind, Message};
use crate::pages;
use crate::theme::{self, icon, icon as i};

pub const SCROLL: &str = "main-scroll";

pub fn btn<'a>(c: impl Into<Element<'a, Message>>, msg: Message) -> button::Button<'a, Message> {
    button(c).on_press(msg).padding(0).style(theme::ghost)
}

pub fn icon_btn<'a>(c: char, size: f32, msg: Message) -> button::Button<'a, Message> {
    button(i(c, size).center())
        .on_press(msg)
        .padding(0)
        .width(size * 2.0)
        .height(size * 2.0)
        .style(theme::link(theme::SUB))
}

pub fn cover(app: &App, id: &str, size: u32, px: f32, round: bool) -> Element<'static, Message> {
    let r = if round {
        px / 2.0
    } else if px < 100.0 {
        4.0
    } else {
        6.0
    };
    match app.covers.get(&(id.to_string(), size)) {
        Some(h) => image(h)
            .width(px)
            .height(px)
            .content_fit(iced::ContentFit::Cover)
            .border_radius(r)
            .into(),
        None => container(
            i(if round { icon::USER } else { icon::MUSIC }, px * 0.35).color(theme::SUB).center(),
        )
        .center(px)
        .style(theme::fill(theme::ELEVATED, r))
        .into(),
    }
}

pub fn liked_art<'a>(px: f32) -> Element<'a, Message> {
    container(i(icon::HEART, px * 0.38).color(theme::TEXT).center())
        .center(px)
        .style(theme::diagonal(if px < 100.0 { 4.0 } else { 6.0 }))
        .into()
}

pub fn artists_line<'a>(artists: &[Artist], size: f32, w: f32) -> Element<'a, Message> {
    let mut left = w;
    let mut r = iced::widget::Row::new();
    for (n, a) in artists.iter().enumerate() {
        let name = if n > 0 { format!(", {}", a.name) } else { a.name.clone() };
        if left < size {
            break;
        }
        let t = theme::ell(&name, left, size, Weight::Normal);
        left -= theme::width(&name, size, Weight::Normal);
        r = r.push(if a.id.is_empty() {
            Element::from(t.color(theme::SUB))
        } else {
            button(t)
                .padding(0)
                .style(theme::link(theme::SUB))
                .on_press(Message::Nav(format!("artist:{}", a.id)))
                .into()
        });
    }
    r.into()
}

pub fn fmt_dur(s: f32) -> String {
    let s = s as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn fmt_long(s: f32) -> String {
    let s = s as u64;
    match s {
        3600.. => format!("{} hr {} min", s / 3600, s % 3600 / 60),
        _ => format!("{} min {} sec", s / 60, s % 60),
    }
}

fn top_bar<'a>(app: &'a App) -> Element<'a, Message> {
    let nav = |c: char, on: bool, m: Message| {
        button(i(c, 22.0).center())
            .width(32)
            .height(32)
            .padding(0)
            .style(theme::link(if on { theme::SUB } else { theme::DIM }))
            .on_press_maybe(on.then_some(m))
    };
    let initial = app
        .session
        .as_ref()
        .and_then(|s| s.server.user.chars().next())
        .map_or('?', |c| c.to_ascii_uppercase());
    let round = |c: Element<'a, Message>, m: Message| {
        button(container(c).center(48)).padding(0).style(theme::round(theme::ELEVATED)).on_press(m)
    };
    row![
        container(
            row![
                nav(icon::LEFT, app.hi > 0, Message::Back),
                nav(icon::RIGHT, app.hi + 1 < app.history.len(), Message::Fwd)
            ]
            .spacing(8)
        )
        .width(Fill),
        row![
            round(
                i(icon::HOME, 24.0)
                    .color(if app.route == "home" { theme::TEXT } else { theme::SUB })
                    .into(),
                Message::Nav("home".into())
            ),
            text_input("What do you want to play?", &app.search_q)
                .id("search")
                .on_input(Message::Search)
                .on_submit(Message::SearchGo(app.search_seq))
                .style(theme::search_field)
                .icon(text_input::Icon {
                    font: theme::LUCIDE,
                    code_point: icon::SEARCH,
                    size: Some(22.0.into()),
                    spacing: 12.0,
                    side: text_input::Side::Left,
                })
                .font(theme::fw(Weight::Normal))
                .padding([13, 16])
                .line_height(text::LineHeight::Absolute(22.0.into()))
                .width(474)
                .size(16),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        container(
            row![
                icon_btn(icon::SETTINGS, 18.0, Message::Nav("settings".into())),
                round(
                    container(theme::bold(initial, 14.0).color(theme::BG).center())
                        .center(32)
                        .style(theme::fill(theme::color_of(&initial.to_string()), 500.0))
                        .into(),
                    Message::Menu(Box::new(MenuKind::User))
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        )
        .width(Fill)
        .align_x(Alignment::End),
    ]
    .align_y(Alignment::Center)
    .padding([0, 8])
    .height(64)
    .into()
}

fn lib_panel<'a>(app: &'a App) -> Element<'a, Message> {
    let w = app.lib_w();
    let item = |art: Element<'a, Message>, title: &str, sub: String, route: String| {
        let on = app.route == route;
        let playing = !app.source.is_empty() && app.source == route;
        let body: Element<'a, Message> = if app.lib_collapsed {
            art
        } else {
            row![
                art,
                column![
                    theme::ell(title, w - 112.0, 16.0, Weight::Normal).color(if playing {
                        theme::ACCENT
                    } else {
                        theme::TEXT
                    }),
                    theme::ell(&sub, w - 112.0, 14.0, Weight::Normal).color(theme::SUB),
                ]
                .spacing(2),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
        };
        Element::from(
            button(body)
                .padding(8)
                .width(Fill)
                .style(theme::track(on))
                .on_press(Message::Nav(route)),
        )
    };
    let head: Element<'a, Message> = if app.lib_collapsed {
        container(icon_btn(icon::LIBRARY, 20.0, Message::LibCollapse)).center_x(Fill).into()
    } else {
        row![
            button(
                row![i(icon::LIBRARY, 22.0), theme::bold("Your Library", 16.0)]
                    .spacing(12)
                    .align_y(Alignment::Center)
            )
            .padding(0)
            .style(theme::link(theme::SUB))
            .on_press(Message::LibCollapse),
            Space::new().width(Fill),
            button(i(icon::PLUS, 18.0).center())
                .width(32)
                .height(32)
                .padding(0)
                .style(theme::round(theme::CHIP))
                .on_press(Message::NewPlaylist),
        ]
        .align_y(Alignment::Center)
        .into()
    };
    let chips = (!app.lib_collapsed).then(|| {
        container(
            iced::widget::Row::with_children(
                [(1u8, "Playlists"), (2, "Albums"), (3, "Artists")].map(|(n, name)| {
                    button(theme::txt(name, 14.0, Weight::Normal))
                        .on_press(Message::LibFilter(n))
                        .style(theme::chip(app.lib_filter == n))
                        .padding([6, 12])
                        .into()
                }),
            )
            .spacing(8),
        )
        .padding([0, 8])
    });
    let f = app.lib_filter;
    let mut list = Column::new();
    if f < 2 {
        list = list.push(item(liked_art(48.0), "Liked Songs", "Playlist".into(), "liked".into()));
    }
    if let Some(l) = app.data::<crate::app::Lib>("lib") {
        list = list
            .extend(l.playlists.iter().filter(|_| f < 2).map(|p| {
                item(
                    cover(app, &p.cover, 128, 48.0, false),
                    &p.name,
                    format!("Playlist \u{2022} {}", p.owner),
                    format!("playlist:{}", p.id),
                )
            }))
            .extend(l.albums.iter().filter(|_| matches!(f, 0 | 2)).map(|a| {
                item(
                    cover(app, &a.cover, 128, 48.0, false),
                    &a.name,
                    format!("Album \u{2022} {}", a.artist),
                    format!("album:{}", a.id),
                )
            }))
            .extend(l.artists.iter().filter(|_| f == 0 || f == 3).map(|a| {
                item(
                    cover(app, &a.cover, 128, 48.0, true),
                    &a.name,
                    "Artist".into(),
                    format!("artist:{}", a.id),
                )
            }));
    }
    container(
        column![
            container(head).padding([4, 8]),
            chips,
            scrollable(list).style(theme::scroll).height(Fill)
        ]
        .spacing(12)
        .padding(8),
    )
    .width(w)
    .height(Fill)
    .style(theme::panel)
    .into()
}

fn player_bar<'a>(app: &'a App) -> Element<'a, Message> {
    let song = app.current_song();
    let dim = if song.is_some() { theme::SUB } else { theme::DIM };
    let ctl = |c: char, on: bool, m: Message| -> Element<'a, Message> {
        let dot = text(if on { "\u{2022}" } else { " " }).size(8).color(theme::ACCENT);
        button(
            column![i(c, 16.0).color(if on { theme::ACCENT } else { dim }), dot]
                .align_x(Alignment::Center),
        )
        .padding(theme::pad(8.0, 8.0, 0.0, 8.0))
        .style(theme::ghost)
        .on_press(m)
        .into()
    };
    let side = |c: char, on: bool, m: Message| -> Element<'a, Message> {
        button(i(c, 16.0).center())
            .width(32)
            .height(32)
            .padding(0)
            .style(theme::link(if on { theme::ACCENT } else { theme::SUB }))
            .on_press(m)
            .into()
    };
    let left: Element<Message> = song.map_or_else(
        || Space::new().into(),
        |s| {
            let starred = app.is_starred(&s.id, s.starred);
            row![
                cover(app, &s.cover, 128, 56.0, false),
                column![
                    btn(
                        theme::ell(&s.title, 200.0, 14.0, Weight::Normal),
                        Message::Nav(format!("album:{}", s.album_id))
                    ),
                    artists_line(&s.artists, 12.0, 200.0),
                ]
                .spacing(2),
                side(icon::HEART, starred, Message::Star(s.id.clone(), !starred)),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
        },
    );
    let dur = song.map_or(0.0, |s| s.duration).max(1.0);
    let pos = app.seek_preview.unwrap_or(app.position.min(dur));
    let progress: Element<Message> = match song {
        None => container(
            container(Space::new().width(Fill).height(4)).style(theme::fill(theme::TRACK, 2.0)),
        )
        .center_y(16)
        .padding([0, 48])
        .into(),
        Some(_) => row![
            theme::sub(fmt_dur(pos), 12.0).width(40).align_x(Alignment::End),
            slider(0.0..=dur, pos, Message::SeekPreview)
                .on_release(Message::SeekRelease)
                .style(theme::seek),
            theme::sub(fmt_dur(dur), 12.0).width(40),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
    };
    let play = button(
        i(if app.playing { icon::PAUSE } else { icon::PLAY }, 16.0).color(theme::BG).center(),
    )
    .width(32)
    .height(32)
    .padding(0)
    .style(move |_, _| button::Style {
        background: Some(if song.is_some() { theme::TEXT } else { theme::DIM }.into()),
        border: iced::Border::default().rounded(500.0),
        ..button::Style::default()
    })
    .on_press(Message::Toggle);
    let center = column![
        row![
            ctl(icon::SHUFFLE, app.shuffle.is_some(), Message::Shuffle),
            side(icon::PREV, false, Message::Prev),
            play,
            side(icon::NEXT, false, Message::Next(true)),
            ctl(
                if app.repeat == 2 { icon::REPEAT1 } else { icon::REPEAT },
                app.repeat > 0,
                Message::Repeat
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        progress,
    ]
    .spacing(4)
    .align_x(Alignment::Center)
    .max_width(722);
    let vol_icon = match app.volume {
        _ if app.muted => icon::MUTE,
        0.0 => icon::MUTE,
        v if v < 0.5 => icon::VOL1,
        _ => icon::VOL2,
    };
    let right = row![
        side(icon::PR, app.panel == 1, Message::Panel(1)),
        side(icon::LYRICS, app.route == "lyrics", Message::Nav("lyrics".into())),
        side(icon::QUEUE, app.panel == 2, Message::Panel(2)),
        side(icon::SPEED, false, Message::Menu(Box::new(MenuKind::Speed))),
        side(vol_icon, false, Message::Mute),
        slider(0.0..=1.0, if app.muted { 0.0 } else { app.volume }, Message::Vol)
            .width(93)
            .style(theme::seek),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    row![
        container(left).width(Length::FillPortion(3)),
        container(center).center_x(Length::FillPortion(4)),
        container(right).width(Length::FillPortion(3)).align_x(Alignment::End),
    ]
    .align_y(Alignment::Center)
    .padding([0, 8])
    .height(72)
    .into()
}

fn overlays<'a>(app: &'a App) -> Vec<Element<'a, Message>> {
    let scrim = |c: Color| {
        mouse_area(container(Space::new()).width(Fill).height(Fill).style(theme::fill(c, 0.0)))
            .on_press(Message::MenuClose)
            .on_right_press(Message::MenuClose)
    };
    let mut layers = Vec::new();
    if let Some((id, rename)) = &app.modal {
        let save = if *rename {
            Message::Rename(id.clone(), app.new_pl.clone())
        } else {
            Message::PlNew(app.new_pl.clone())
        };
        layers.push(
            stack![
                scrim(Color { a: 0.7, ..theme::BG }),
                container(
                    container(
                        column![
                            theme::bold(
                                if *rename { "Rename playlist" } else { "New playlist" },
                                24.0
                            ),
                            text_input("Playlist name", &app.new_pl)
                                .on_input(|v| Message::LField(3, v))
                                .on_submit(save.clone())
                                .style(theme::field)
                                .font(theme::fw(Weight::Normal))
                                .padding(12)
                                .size(14),
                            row![
                                Space::new().width(Fill),
                                button(theme::bold("Cancel", 14.0))
                                    .style(theme::link(theme::SUB))
                                    .padding([12, 16])
                                    .on_press(Message::MenuClose),
                                button(theme::bold("Save", 16.0))
                                    .style(|_, _| button::Style {
                                        background: Some(theme::TEXT.into()),
                                        text_color: theme::BG,
                                        border: iced::Border::default().rounded(500.0),
                                        ..button::Style::default()
                                    })
                                    .padding([12, 32])
                                    .on_press(save),
                            ]
                            .spacing(8)
                            .align_y(Alignment::Center),
                        ]
                        .spacing(24)
                        .width(480)
                        .padding(24),
                    )
                    .style(|_| container::Style {
                        border: iced::Border::default().rounded(8.0),
                        ..theme::menu(&iced::Theme::Dark)
                    })
                )
                .center(Fill),
            ]
            .into(),
        );
    }
    if let Some(m) = &app.menu {
        let (el, w, h) = pages::menu(app, m);
        let (ww, wh) = app.cfg.settings.window;
        let (x, y) = (m.x.min(ww - w - 8.0).max(0.0), m.y.min(wh - h - 8.0).max(0.0));
        layers.push(
            stack![
                scrim(Color::TRANSPARENT),
                column![Space::new().height(y), row![Space::new().width(x), el]]
            ]
            .into(),
        );
    }
    if let Some((_, t)) = &app.toast {
        layers.push(
            container(
                container(theme::med(t, 14.0).color(theme::BG))
                    .padding([12, 16])
                    .style(theme::fill(theme::TEXT, 8.0)),
            )
            .center_x(Fill)
            .height(Fill)
            .align_y(Alignment::End)
            .padding(theme::pad(0.0, 0.0, 104.0, 0.0))
            .into(),
        );
    }
    layers
}

pub fn view(app: &App) -> Element<'_, Message> {
    if app.session.is_none() {
        return pages::login(app);
    }
    let scroll = scrollable(pages::route(app))
        .id(SCROLL)
        .on_scroll(|v| {
            Message::Scrolled(v.absolute_offset().y, v.bounds().height, v.content_bounds().height)
        })
        .style(theme::scroll)
        .height(Fill);
    let main = container(stack![scroll].push(pages::sticky(app)))
        .width(Fill)
        .height(Fill)
        .clip(true)
        .style(theme::panel);
    let panels = row![lib_panel(app), main]
        .push((app.panel > 0).then(|| {
            container(if app.panel == 1 {
                pages::now_playing(app)
            } else {
                pages::queue_panel(app)
            })
            .width(crate::app::RIGHT_W)
            .height(Fill)
            .style(theme::panel)
        }))
        .spacing(8)
        .height(Fill);
    let base = container(column![top_bar(app), container(panels).padding([0, 8]), player_bar(app)])
        .style(theme::fill(theme::BG, 0.0));
    stack(std::iter::once(base.into()).chain(overlays(app))).into()
}
