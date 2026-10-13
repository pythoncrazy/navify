use iced::font::Weight;
use iced::widget::{
    Column, Space, button, column, container, image, mouse_area, row, scrollable, slider, stack,
    text, text_input, tooltip,
};
use iced::{Alignment, Color, Element, Fill, Length, padding};

use crate::api::Artist;
use crate::app::{App, Lib, MenuKind, Message, set};
use crate::pages;
use crate::theme::{self, fill, icon, icon as i};

pub const SCROLL: &str = "main-scroll";

pub fn btn<'a>(c: impl Into<Element<'a, Message>>, msg: Message) -> button::Button<'a, Message> {
    theme::link_btn(c, theme::TEXT, msg)
}

pub fn icon_btn<'a>(c: char, size: f32, msg: Message) -> button::Button<'a, Message> {
    theme::sq(i(c, size).center(), size * 2.0)
        .on_press(msg)
        .style(theme::link(theme::SUB))
}

pub fn focusable<'a>(
    c: impl Into<Element<'a, Message>>,
    msg: Option<Message>,
    radius: f32,
    id: Option<&'static str>,
) -> Element<'a, Message> {
    use iced::advanced::widget::{Id, Operation, Tree, Widget, operation::Focusable, tree};
    use iced::advanced::{Clipboard, Layout, Shell, layout, renderer};
    use iced::keyboard::{Event::KeyPressed, Key, key::Named};
    use iced::{Event, Rectangle, Size, mouse};
    struct Focus<'a>(Element<'a, Message>, Option<Message>, f32, Option<Id>);
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
    impl Widget<Message, iced::Theme, iced::Renderer> for Focus<'_> {
        fn size(&self) -> Size<Length> {
            self.0.as_widget().size()
        }
        fn layout(&mut self, t: &mut Tree, r: &iced::Renderer, l: &layout::Limits) -> layout::Node {
            self.0.as_widget_mut().layout(&mut t.children[0], r, l)
        }
        fn draw(
            &self,
            t: &Tree,
            r: &mut iced::Renderer,
            th: &iced::Theme,
            s: &renderer::Style,
            l: Layout<'_>,
            c: mouse::Cursor,
            v: &Rectangle,
        ) {
            use iced::advanced::Renderer;
            self.0.as_widget().draw(&t.children[0], r, th, s, l, c, v);
            let e = self.2.min(4.0);
            if t.state.downcast_ref::<Ring>().0 {
                r.fill_quad(
                    renderer::Quad {
                        bounds: l.bounds().expand(e),
                        border: iced::border::rounded(self.2 + e)
                            .color(Color::WHITE)
                            .width(2),
                        ..Default::default()
                    },
                    Color::TRANSPARENT,
                );
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
        fn operate(
            &mut self,
            t: &mut Tree,
            l: Layout<'_>,
            r: &iced::Renderer,
            op: &mut dyn Operation,
        ) {
            op.focusable(self.3.as_ref(), l.bounds(), t.state.downcast_mut::<Ring>());
            op.traverse(&mut |op| self.0.as_widget_mut().operate(&mut t.children[0], l, r, op));
        }
        fn update(
            &mut self,
            t: &mut Tree,
            e: &Event,
            l: Layout<'_>,
            c: mouse::Cursor,
            r: &iced::Renderer,
            cb: &mut dyn Clipboard,
            sh: &mut Shell<'_, Message>,
            v: &Rectangle,
        ) {
            self.0
                .as_widget_mut()
                .update(&mut t.children[0], e, l, c, r, cb, sh, v);
            let ring = t.state.downcast_mut::<Ring>();
            match e {
                _ if !ring.0 => return,
                Event::Mouse(mouse::Event::ButtonPressed(_)) => ring.0 = false,
                Event::Keyboard(KeyPressed { key, .. }) if *key == Key::Named(Named::Escape) => {
                    ring.0 = false;
                    sh.capture_event();
                }
                Event::Keyboard(KeyPressed { key, .. })
                    if matches!(key, Key::Named(Named::Enter | Named::Space))
                        && !sh.is_event_captured() =>
                {
                    if let Some(m) = &self.1 {
                        sh.publish(m.clone());
                    }
                    sh.capture_event();
                }
                _ => return,
            }
            sh.request_redraw();
        }
        fn mouse_interaction(
            &self,
            t: &Tree,
            l: Layout<'_>,
            c: mouse::Cursor,
            v: &Rectangle,
            r: &iced::Renderer,
        ) -> mouse::Interaction {
            self.0
                .as_widget()
                .mouse_interaction(&t.children[0], l, c, v, r)
        }
    }
    Element::new(Focus(c.into(), msg, radius, id.map(Id::from)))
}

pub fn cover(app: &App, id: &str, size: u32, px: f32, round: bool) -> Element<'static, Message> {
    let r = if px < 100.0 { 4.0 } else { 6.0 };
    let r = if round { px / 2.0 } else { r };
    match app.covers.get(id, size) {
        Some(h) => image(h)
            .width(px)
            .height(px)
            .content_fit(iced::ContentFit::Cover)
            .border_radius(r)
            .into(),
        None => container(
            i(if round { icon::USER } else { icon::MUSIC }, px * 0.35)
                .color(theme::SUB)
                .center(),
        )
        .center(px)
        .style(theme::fill(theme::ELEVATED, r))
        .into(),
    }
}

pub const SONGS_BG: Color = Color::from_rgb8(0x00, 0x64, 0x50);

pub fn pin_art<'a>(px: f32, c: char, bg: Color, fg: Color) -> Element<'a, Message> {
    container(i(c, px * 0.38).color(fg).center())
        .center(px)
        .style(theme::fill(bg, if px < 100.0 { 4.0 } else { 6.0 }))
        .into()
}

pub fn liked_art<'a>(px: f32) -> Element<'a, Message> {
    let heart = theme::filled(fill::HEART, px * 0.38).color(theme::TEXT);
    container(heart.center())
        .center(px)
        .style(theme::diagonal(if px < 100.0 { 4.0 } else { 6.0 }))
        .into()
}

pub fn artists_line<'a>(artists: &[Artist], size: f32, w: f32) -> Element<'a, Message> {
    let mut left = w;
    let mut r = iced::widget::Row::new();
    for (n, a) in artists.iter().enumerate() {
        if left < size {
            break;
        }
        let name = format!("{}{}", if n > 0 { ", " } else { "" }, a.name);
        let t = theme::ell(&name, left, size, Weight::Normal);
        left -= theme::width(&name, size, Weight::Normal);
        r = r.push(if a.id.is_empty() {
            Element::from(t.color(theme::SUB))
        } else {
            theme::link_btn(t, theme::SUB, Message::Nav(format!("artist:{}", a.id))).into()
        });
    }
    r.into()
}

pub fn fmt_dur(s: f32) -> String {
    let s = s as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn fmt_n(n: usize) -> String {
    let s = n.to_string();
    s.char_indices().fold(String::new(), |mut o, (i, c)| {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            o.push(',');
        }
        o.push(c);
        o
    })
}

pub fn fmt_long(s: f32) -> String {
    match s as u64 {
        s @ 3600.. => format!("{} hr {} min", s / 3600, s % 3600 / 60),
        s => format!("{} min {} sec", s / 60, s % 60),
    }
}

fn top_bar<'a>(app: &'a App) -> Element<'a, Message> {
    use tooltip::Position::Bottom;
    let nav = |c: char, on: bool, label: &str, m: Message| {
        tip(
            theme::sq(i(c, 22.0).center(), 32.0)
                .style(theme::link(if on { theme::SUB } else { theme::DIM }))
                .on_press_maybe(on.then_some(m)),
            label,
            Bottom,
        )
    };
    let fwd = app.hi + 1 < app.history.len();
    let initial = app
        .session
        .as_ref()
        .and_then(|s| s.server.user.chars().next())
        .map_or('?', |c| c.to_ascii_uppercase());
    let round = |c: Element<'a, Message>, m: Message| {
        button(container(c).center(48))
            .padding(0)
            .style(theme::round(theme::ELEVATED))
            .on_press(m)
    };
    row![
        container(
            row![
                nav(icon::LEFT, app.hi > 0, "Go back", Message::Back),
                nav(icon::RIGHT, fwd, "Go forward", Message::Fwd)
            ]
            .spacing(8)
        )
        .width(Fill),
        row![
            tip(
                round(
                    if app.route == "home" {
                        theme::filled(fill::HOME, 24.0).color(theme::TEXT)
                    } else {
                        i(icon::HOME, 24.0).color(theme::SUB)
                    }
                    .into(),
                    Message::Nav("home".into())
                ),
                "Home",
                Bottom
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
                tip(
                    icon_btn(icon::SETTINGS, 18.0, Message::Nav("settings".into())),
                    "Settings",
                    Bottom
                ),
                tip(
                    round(
                        container(theme::bold(initial, 14.0).color(theme::BG).center())
                            .center(32)
                            .style(theme::fill(theme::color_of(&initial.to_string()), 500.0))
                            .into(),
                        Message::Menu(Box::new(MenuKind::User))
                    ),
                    app.session.as_ref().map_or("", |s| s.server.user.as_str()),
                    Bottom
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
    let item = |art: Element<'a, Message>, title: &str, sub: &str, route: String| {
        let same = |r: &str| r == route || r.starts_with(&format!("{route}:"));
        let (on, playing) = (same(&app.route), !app.ctx.is_empty() && same(&app.ctx));
        let fg = if playing { theme::ACCENT } else { theme::TEXT };
        let body: Element<'a, Message> = if app.cfg.settings.lib_collapsed {
            art
        } else {
            row![
                art,
                column![
                    theme::ell(title, w - 112.0, 16.0, Weight::Normal).color(fg),
                    theme::ell(sub, w - 112.0, 14.0, Weight::Normal).color(theme::SUB),
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
    let head: Element<'a, Message> = if app.cfg.settings.lib_collapsed {
        container(tip(
            icon_btn(icon::LIBRARY, 20.0, Message::LibCollapse),
            "Expand Your Library",
            tooltip::Position::Right,
        ))
        .center_x(Fill)
        .into()
    } else {
        row![
            tip(
                theme::link_btn(
                    row![i(icon::LIBRARY, 22.0), theme::bold("Your Library", 16.0)]
                        .spacing(12)
                        .align_y(Alignment::Center),
                    theme::SUB,
                    Message::LibCollapse
                ),
                "Collapse Your Library",
                tooltip::Position::Bottom,
            ),
            Space::new().width(Fill),
            tip(
                theme::sq(i(icon::PLUS, 18.0).center(), 32.0)
                    .style(theme::round(theme::CHIP))
                    .on_press(Message::NewPlaylist),
                "Create playlist",
                tooltip::Position::Bottom,
            ),
        ]
        .align_y(Alignment::Center)
        .into()
    };
    let chips = (!app.cfg.settings.lib_collapsed).then(|| {
        container(
            iced::widget::Row::with_children(
                [(1u8, "Playlists"), (2, "Albums"), (3, "Artists")].map(|(n, name)| {
                    theme::chip_btn(name, app.lib_filter == n, Message::LibFilter(n))
                }),
            )
            .spacing(8),
        )
        .padding([0, 8])
    });
    let f = app.lib_filter;
    let (green, dim) = ((SONGS_BG, theme::ACCENT), (theme::ELEVATED, theme::SUB));
    let pins = [
        (f == 0, icon::MUSIC, green, "Songs", "songs"),
        (f == 2, icon::ALBUM, dim, "All albums", "albums"),
        (f == 3, icon::USER, dim, "All artists", "artists"),
        (f == 0, icon::FOLDER, dim, "Folders", "folders"),
    ]
    .into_iter()
    .filter(|p| p.0)
    .map(|(_, c, (bg, fg), t, r)| item(pin_art(48.0, c, bg, fg), t, "Library", r.into()));
    let rows = app.data::<Lib>("lib").into_iter().flat_map(|l| {
        let pls = l.playlists.iter().filter(|_| f < 2).map(|p| {
            let s = format!("Playlist \u{2022} {}", p.owner);
            (&p.cover, &p.name, s, format!("playlist:{}", p.id), false)
        });
        let als = l.albums.iter().filter(|_| matches!(f, 0 | 2)).map(|a| {
            let s = format!("Album \u{2022} {}", a.artist);
            (&a.cover, &a.name, s, format!("album:{}", a.id), false)
        });
        let ars = l.artists.iter().filter(|_| matches!(f, 0 | 3)).map(|a| {
            let r = format!("artist:{}", a.id);
            (&a.cover, &a.name, "Artist".into(), r, true)
        });
        pls.chain(als).chain(ars)
    });
    let list = Column::new()
        .push((f < 2).then(|| item(liked_art(48.0), "Liked Songs", "Playlist", "liked".into())))
        .extend(pins)
        .extend(rows.map(|(c, n, s, r, round)| item(cover(app, c, 128, 48.0, round), n, &s, r)));
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

pub fn tip<'a>(
    c: impl Into<Element<'a, Message>>,
    label: impl ToString,
    at: tooltip::Position,
) -> Element<'a, Message> {
    tooltip(
        c,
        container(theme::txt(label, 14.0, Weight::Normal))
            .padding([4, 8])
            .style(theme::menu),
        at,
    )
    .gap(8)
    .delay(std::time::Duration::from_millis(200))
    .into()
}

fn player_bar<'a>(app: &'a App) -> Element<'a, Message> {
    use tooltip::Position::Top;
    let song = app.current_song();
    let side = |t: theme::Txt<'a>, on: bool, label: &str, m: Message| -> Element<'a, Message> {
        let dot = container(Space::new())
            .width(4)
            .height(4)
            .style(theme::fill(theme::ACCENT, 2.0));
        tip(
            theme::sq(
                stack![container(t).center(Fill)]
                    .push(on.then(|| container(dot).center_x(Fill).align_bottom(Fill))),
                32.0,
            )
            .style(theme::link(if on { theme::ACCENT } else { theme::SUB }))
            .on_press(m),
            label,
            Top,
        )
    };
    let left = song.map(|s| {
        let starred = app.is_starred(&s.id, s.starred);
        row![
            btn(cover(app, &s.cover, 128, 56.0, false), Message::Panel(1)),
            column![
                btn(
                    theme::ell(&s.title, 200.0, 14.0, Weight::Normal),
                    Message::Nav(format!("album:{}", s.album_id))
                ),
                artists_line(&s.artists, 12.0, 200.0),
            ]
            .spacing(2),
            side(
                theme::heart(starred, 16.0).color_maybe(starred.then_some(theme::ACCENT)),
                false,
                ["Add to Liked Songs", "Remove from Liked Songs"][usize::from(starred)],
                Message::Star(vec![s.id.clone()], !starred)
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
    });
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
            theme::sub(fmt_dur(pos), 12.0)
                .width(48)
                .align_x(Alignment::End),
            slider(0.0..=dur, pos, Message::SeekPreview)
                .on_release(Message::SeekRelease)
                .style(theme::seek),
            button(theme::txt(
                if app.cfg.settings.remaining {
                    format!("-{}", fmt_dur(dur - pos))
                } else {
                    fmt_dur(dur)
                },
                12.0,
                Weight::Normal
            ))
            .width(48)
            .padding(0)
            .style(theme::link(theme::SUB))
            .on_press(set(|s, b| s.remaining = b, !app.cfg.settings.remaining)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
    };
    let playing = usize::from(app.playing);
    let glyph = theme::filled([fill::PLAY, fill::PAUSE][playing], 16.0);
    let bg = song.map_or(theme::DIM, |_| theme::TEXT);
    let play = theme::sq(glyph.color(theme::BG).center(), 32.0)
        .style(theme::solid([bg; 3], theme::BG))
        .on_press(Message::Toggle);
    let (ic, fi) = (|c| i(c, 16.0), |c| theme::filled(c, 16.0));
    let (on, repeat) = (app.shuffle.is_some(), usize::from(app.repeat));
    let center = column![
        row![
            side(
                ic(icon::SHUFFLE),
                on,
                ["Enable shuffle", "Disable shuffle"][usize::from(on)],
                Message::Shuffle
            ),
            side(fi(fill::PREV), false, "Previous", Message::Prev),
            tip(play, ["Play", "Pause"][playing], Top),
            side(fi(fill::NEXT), false, "Next", Message::Next(true)),
            side(
                ic([icon::REPEAT, icon::REPEAT, icon::REPEAT1][repeat]),
                repeat > 0,
                ["Enable repeat", "Enable repeat one", "Disable repeat"][repeat],
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
    let vol = if app.muted { 0.0 } else { app.volume };
    let vol_icon = match vol {
        0.0 => icon::MUTE,
        v if v < 0.5 => icon::VOL1,
        _ => icon::VOL2,
    };
    let right = row![
        side(
            ic(icon::PR),
            app.panel == 1,
            "Now playing view",
            Message::Panel(1)
        ),
        side(
            ic(icon::LYRICS),
            app.route == "lyrics",
            "Lyrics",
            if app.route == "lyrics" {
                Message::Back
            } else {
                Message::Nav("lyrics".into())
            }
        ),
        side(ic(icon::QUEUE), app.panel == 2, "Queue", Message::Panel(2)),
        tip(
            button(theme::bold(format!("{}x", app.cfg.settings.speed), 12.0).center())
                .width(40)
                .height(32)
                .padding(0)
                .style(theme::link(if app.cfg.settings.speed == 1.0 {
                    theme::SUB
                } else {
                    theme::ACCENT
                }))
                .on_press(Message::Menu(Box::new(MenuKind::Speed))),
            "Change speed",
            Top
        ),
        side(
            ic(vol_icon),
            false,
            if app.muted { "Unmute" } else { "Mute" },
            Message::Mute
        ),
        mouse_area(
            slider(0.0..=1.0, vol, Message::Vol)
                .step(0.01f32)
                .width(93)
                .style(theme::seek)
        )
        .on_scroll(move |d| {
            let step = match d {
                iced::mouse::ScrollDelta::Lines { y, .. } => y.clamp(-1.0, 1.0) / 10.0,
                iced::mouse::ScrollDelta::Pixels { y, .. } => y.clamp(-120.0, 120.0) / 1200.0,
            };
            Message::Vol((vol + step).clamp(0.0, 1.0))
        }),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    row![
        container(left).width(Length::FillPortion(3)),
        container(center).center_x(Length::FillPortion(4)),
        container(right)
            .width(Length::FillPortion(3))
            .align_x(Alignment::End),
    ]
    .align_y(Alignment::Center)
    .padding([0, 8])
    .height(72)
    .into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    if app.session.is_none() {
        return pages::login(app);
    }
    let scrim = |c: Color| {
        mouse_area(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::fill(c, 0.0)),
        )
        .on_press(Message::MenuClose)
        .on_right_press(Message::MenuClose)
    };
    let modal = app.modal.as_ref().map(|(id, kind)| {
        let pl = app
            .data::<Lib>("lib")
            .and_then(|l| l.playlists.iter().find(|p| &p.id == id));
        let act = |b: button::Button<'static, Message>, m: Option<Message>| {
            focusable(b.on_press_maybe(m.clone()), m, 500.0, Some("layer"))
        };
        let card: Element<'_, Message> = if *kind == 1 {
            let named = !app.new_pl.trim().is_empty();
            let ok = named
                .then(|| Message::Edit(id.clone(), app.new_pl.clone(), app.new_pl_desc.clone()));
            let input = |hint: &'static str, v: &str, n: u8| {
                text_input(hint, v)
                    .id("layer")
                    .on_input(move |v| Message::LField(n, v))
                    .on_submit_maybe(ok.clone())
                    .style(theme::modal_field)
                    .font(theme::fw(Weight::Normal))
                    .padding(10)
                    .size(14)
            };
            container(
                column![
                    row![
                        theme::bold("Edit details", 24.0).width(Fill),
                        focusable(
                            icon_btn(icon::X, 16.0, Message::MenuClose),
                            Some(Message::MenuClose),
                            16.0,
                            Some("layer"),
                        )
                    ]
                    .align_y(Alignment::Center),
                    row![
                        cover(app, pl.map_or("", |p| p.cover.as_str()), 300, 180.0, false),
                        column![
                            column![input("Add a name", &app.new_pl, 3)]
                                .push((!named).then(|| {
                                    theme::txt("Enter a name.", 12.0, Weight::Normal)
                                        .color(theme::ERR)
                                }))
                                .spacing(6),
                            input("Add an optional description", &app.new_pl_desc, 4),
                        ]
                        .spacing(16),
                    ]
                    .spacing(16),
                    container(act(
                        button(theme::bold("Save", 16.0))
                            .style(theme::white)
                            .padding([12, 32]),
                        ok
                    ))
                    .align_right(Fill),
                ]
                .spacing(24)
                .width(524)
                .padding(24),
            )
            .style(|t| theme::menu(t).border(iced::border::rounded(8)))
            .into()
        } else {
            container(
                column![
                    theme::bold("Delete from Your Library?", 24.0).color(theme::BG),
                    text::Rich::<(), Message>::with_spans(vec![
                        text::Span::new("This will delete "),
                        text::Span::new(pl.map_or("", |p| p.name.as_str()))
                            .font(theme::fw(Weight::Bold)),
                        text::Span::new(" from "),
                        text::Span::new("Your Library.").font(theme::fw(Weight::Bold)),
                    ])
                    .size(14)
                    .font(theme::fw(Weight::Normal))
                    .color(theme::BG),
                    row![
                        Space::new().width(Fill),
                        act(
                            button(theme::bold("Cancel", 16.0))
                                .style(theme::solid([Color::TRANSPARENT; 3], theme::BG))
                                .padding([12, 16]),
                            Some(Message::MenuClose)
                        ),
                        act(
                            button(theme::bold("Delete", 16.0))
                                .style(theme::danger)
                                .padding([12, 32]),
                            Some(Message::PlDel(id.clone()))
                        ),
                    ]
                    .spacing(16)
                    .align_y(Alignment::Center)
                    .padding(padding::top(16)),
                ]
                .spacing(8)
                .width(420)
                .padding(32),
            )
            .style(theme::fill(theme::TEXT, 8.0))
            .into()
        };
        stack![
            scrim(theme::BG.scale_alpha(0.7)),
            container(card).center(Fill)
        ]
    });
    let menu = app.menu.as_ref().map(|m| {
        let (el, w, h) = pages::menu(app, m);
        let (ww, wh) = app.view_size();
        let (x, y) = match m.kind {
            MenuKind::Speed => (m.x - 20.0, wh - 64.0 - h),
            _ => (
                if m.x + w + 8.0 > ww { m.x - w } else { m.x },
                if m.y + h + 8.0 > wh { m.y - h } else { m.y },
            ),
        };
        let (x, y) = (x.clamp(8.0, ww - w - 8.0), y.clamp(8.0, wh - h - 8.0));
        stack![
            scrim(Color::TRANSPARENT),
            column![Space::new().height(y), row![Space::new().width(x), el]]
        ]
    });
    let toast = app.toast.as_ref().map(|(_, t)| {
        container(
            container(theme::med(t, 14.0).color(theme::BG))
                .padding([12, 16])
                .style(theme::fill(theme::TEXT, 8.0)),
        )
        .center_x(Fill)
        .height(Fill)
        .align_y(Alignment::End)
        .padding(padding::bottom(104))
    });
    let scroll = scrollable(pages::route(app))
        .id(SCROLL)
        .on_scroll(|v| {
            Message::Scrolled(
                v.absolute_offset().y,
                v.bounds().height,
                v.content_bounds().height,
            )
        })
        .style(theme::scroll)
        .height(Fill);
    let main = container(stack![scroll].push(pages::sticky(app)))
        .width(Fill)
        .height(Fill)
        .clip(true)
        .style(theme::panel);
    let grip = |n| {
        mouse_area(Space::new().width(8).height(Fill))
            .interaction(iced::mouse::Interaction::ResizingHorizontally)
            .on_press(Message::Drag(Some(n)))
    };
    let panels = row![lib_panel(app), grip(0), main]
        .push((app.panel > 0).then(|| grip(1)))
        .push((app.panel > 0).then(|| {
            container(if app.panel == 1 {
                pages::now_playing(app)
            } else {
                pages::queue_panel(app)
            })
            .width(app.right_w())
            .height(Fill)
            .style(theme::panel)
        }))
        .height(Fill);
    let base = container(column![
        top_bar(app),
        container(panels).padding([0, 8]),
        player_bar(app)
    ])
    .style(theme::fill(theme::BG, 0.0));
    stack![base, modal, menu, toast].into()
}
