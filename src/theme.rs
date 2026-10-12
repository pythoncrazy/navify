use iced::font::{Family, Weight};
use iced::widget::{button, container, scrollable, slider, text, text_input, toggler};
use iced::{Border, Color, Font, Shadow, Theme, color};

pub const BG: Color = color!(0x000000);
pub const PANEL: Color = color!(0x121212);
pub const ELEVATED: Color = color!(0x1f1f1f);
pub const MENU: Color = color!(0x282828);
pub const MENU_HOVER: Color = color!(0x3e3e3e);
pub const CHIP: Color = color!(0x2a2a2a);
pub const TEXT: Color = color!(0xffffff);
pub const SUB: Color = color!(0xb3b3b3);
pub const ACCENT: Color = color!(0x1ed760);
pub const TRACK: Color = color!(0x4d4d4d);
pub const FIELD: Color = color!(0x727272);
pub const SUB2: Color = color!(0xa7a7a7);
pub const ACCENT_H: Color = color!(0x3be477);
pub const ERR: Color = color!(0xf15e6c);
pub const SKEL: Color = color!(0x2a2a2a);
pub const GRAD_A: Color = color!(0x450af5);
pub const GRAD_B: Color = color!(0xc4efd9);

pub fn tint(alpha: f32) -> Color {
    Color { a: alpha, ..Color::WHITE }
}

pub fn hover(_c: Color) -> Color {
    Color { a: 0.1, ..Color::WHITE }
}

pub const LUCIDE: Font = Font::with_name("lucide");

pub fn fw(w: Weight) -> Font {
    Font { family: Family::Name("Figtree"), weight: w, ..Font::DEFAULT }
}

pub fn black<'a>(t: impl ToString, size: f32) -> text::Text<'a, Theme, iced::Renderer> {
    text(t.to_string()).size(size).font(fw(Weight::Black))
}

pub fn bold<'a>(t: impl ToString, size: f32) -> text::Text<'a, Theme, iced::Renderer> {
    text(t.to_string()).size(size).font(fw(Weight::Bold))
}

pub fn med<'a>(t: impl ToString, size: f32) -> text::Text<'a, Theme, iced::Renderer> {
    text(t.to_string()).size(size).font(fw(Weight::Medium))
}

pub fn sub2<'a>(t: impl ToString, size: f32) -> text::Text<'a, Theme, iced::Renderer> {
    text(t.to_string()).size(size).color(SUB2)
}

pub fn ic(c: char) -> char {
    c
}

pub mod icon {
    pub const PLAY: char = '\u{e13c}';
    pub const PAUSE: char = '\u{e12e}';
    pub const NEXT: char = '\u{e160}';
    pub const PREV: char = '\u{e15f}';
    pub const SHUFFLE: char = '\u{e15e}';
    pub const REPEAT: char = '\u{e146}';
    pub const REPEAT1: char = '\u{e1fd}';
    pub const HEART: char = '\u{e0f2}';
    pub const HOME: char = '\u{e0f5}';
    pub const SEARCH: char = '\u{e151}';
    pub const LIBRARY: char = '\u{e100}';
    pub const QUEUE: char = '\u{e2e0}';
    pub const EQ: char = '\u{e55a}';
    pub const LYRICS: char = '\u{e349}';
    pub const VOL2: char = '\u{e1ab}';
    pub const VOL1: char = '\u{e1aa}';
    pub const MUTE: char = '\u{e1ac}';
    pub const PLUS: char = '\u{e13d}';
    pub const GRID: char = '\u{e0ff}';
    pub const DOWNLOAD: char = '\u{e0b2}';
    pub const SETTINGS: char = '\u{e154}';
    pub const MUSIC: char = '\u{e122}';
    pub const ALBUM: char = '\u{e494}';
    pub const RADIO: char = '\u{e142}';
    pub const DOWN: char = '\u{e06d}';
    pub const UP: char = '\u{e070}';
    pub const LEFT: char = '\u{e06e}';
    pub const RIGHT: char = '\u{e06f}';
    pub const X: char = '\u{e1b2}';
    pub const CHECK: char = '\u{e06c}';
    pub const DOTS: char = '\u{e0b6}';
    pub const BACK: char = '\u{e048}';
    pub const FWD: char = '\u{e049}';
    pub const CLOCK: char = '\u{e087}';
    pub const LISTPLUS: char = '\u{e23f}';
    pub const USER: char = '\u{e19f}';
    pub const USERS: char = '\u{e1a4}';
    pub const LOGOUT: char = '\u{e10e}';
    pub const STAR: char = '\u{e176}';
    pub const FOLDER: char = '\u{e0d7}';
    pub const CIRPLAY: char = '\u{e080}';
    pub const CIRPAUSE: char = '\u{e07f}';
    pub const REFRESH: char = '\u{e145}';
    pub const PENCIL: char = '\u{e1f9}';
    pub const TRASH: char = '\u{e18e}';
    pub const INFO: char = '\u{e0f9}';
    pub const SPIN: char = '\u{e10a}';
    pub const ALERT: char = '\u{e077}';
    pub const PL: char = '\u{e12a}';
    pub const PR: char = '\u{e431}';
    pub const SERVER: char = '\u{e153}';
    pub const SPEED: char = '\u{e29a}';
    pub const FF: char = '\u{e0bd}';
    pub const RW: char = '\u{e147}';
    pub const IMAGE: char = '\u{e0f6}';
    pub const HIST: char = '\u{e1f5}';
    pub const LISTCHK: char = '\u{e1d0}';
    pub const MINUS: char = '\u{e11c}';
}

pub fn icon<'a>(c: char, size: f32) -> text::Text<'a, Theme, iced::Renderer> {
    text(c).font(LUCIDE).size(size)
}

pub fn sub<'a>(t: impl ToString, size: f32) -> text::Text<'a, Theme, iced::Renderer> {
    text(t.to_string()).size(size).color(SUB)
}

fn bg(color: Color, radius: f32) -> container::Style {
    container::Style::default().background(color).border(Border::default().rounded(radius))
}

pub fn panel(_: &Theme) -> container::Style {
    bg(PANEL, 8.0)
}

pub fn elevated(_: &Theme) -> container::Style {
    bg(ELEVATED, 8.0)
}

pub fn card(_: &Theme) -> container::Style {
    container::Style::default().border(Border::default().rounded(6.0))
}

pub fn menu(_: &Theme) -> container::Style {
    container::Style {
        background: Some(MENU.into()),
        border: Border::default().rounded(4.0),
        shadow: Shadow { color: Color { a: 0.6, ..BG }, blur_radius: 16.0, ..Shadow::default() },
        ..container::Style::default()
    }
}

pub fn toast(_: &Theme) -> container::Style {
    bg(MENU, 500.0)
}

pub fn header(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        container::Style::default().background(iced::Gradient::Linear(
            iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI * 1.5))
                .add_stop(0.0, color)
                .add_stop(1.0, PANEL),
        ))
    }
}

pub fn search_field(_: &Theme, _s: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: ELEVATED.into(),
        border: Border::default().rounded(500.0),
        icon: SUB,
        placeholder: SUB,
        value: TEXT,
        selection: ACCENT,
    }
}

pub fn field(_: &Theme, s: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: PANEL.into(),
        border: Border {
            color: match s {
                text_input::Status::Focused { .. } | text_input::Status::Hovered => TEXT,
                _ => FIELD,
            },
            width: match s {
                text_input::Status::Focused { .. } => 2.0,
                _ => 1.0,
            },
            radius: 4.0.into(),
        },
        icon: SUB,
        placeholder: SUB,
        value: TEXT,
        selection: ACCENT,
    }
}

pub fn ghost(_: &Theme, s: button::Status) -> button::Style {
    button::Style {
        background: Some(match s {
            button::Status::Hovered | button::Status::Pressed => tint(0.1).into(),
            _ => Color::TRANSPARENT.into(),
        }),
        text_color: TEXT,
        ..button::Style::default()
    }
}

pub fn row(_: &Theme, s: button::Status) -> button::Style {
    button::Style {
        background: Some(match s {
            button::Status::Hovered => tint(0.1).into(),
            button::Status::Pressed => tint(0.3).into(),
            _ => Color::TRANSPARENT.into(),
        }),
        text_color: TEXT,
        ..button::Style::default()
    }
}

pub fn chip(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style {
        background: Some(if on { TEXT.into() } else { CHIP.into() }),
        text_color: if on { BG } else { TEXT },
        border: Border::default().rounded(500.0),
        ..button::Style::default()
    }
}

pub fn accent(_: &Theme, s: button::Status) -> button::Style {
    button::Style {
        background: Some(match s {
            button::Status::Hovered => ACCENT_H.into(),
            button::Status::Pressed => color!(0x1abc54).into(),
            _ => ACCENT.into(),
        }),
        text_color: BG,
        border: Border::default().rounded(500.0),
        ..button::Style::default()
    }
}

pub fn circle(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style {
        background: Some(if on { ELEVATED.into() } else { Color::TRANSPARENT.into() }),
        text_color: TEXT,
        border: Border::default().rounded(500.0),
        ..button::Style::default()
    }
}

pub fn scroll(_: &Theme, _: scrollable::Status) -> scrollable::Style {
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: TRACK.into(),
                border: Border::default().rounded(4.0),
            },
        },
        horizontal_rail: scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: TRACK.into(),
                border: Border::default().rounded(4.0),
            },
        },
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: iced::Color::TRANSPARENT.into(),
            border: Border::default(),
            shadow: Shadow::default(),
            icon: iced::Color::TRANSPARENT,
        },
    }
}

pub fn pad(top: f32, right: f32, bottom: f32, left: f32) -> iced::Padding {
    iced::Padding::new(0.0).top(top).right(right).bottom(bottom).left(left)
}

pub fn tog(_: &Theme, s: toggler::Status) -> toggler::Style {
    let on = matches!(
        s,
        toggler::Status::Active { is_toggled: true }
            | toggler::Status::Hovered { is_toggled: true }
    );
    toggler::Style {
        background: if on { ACCENT } else { FIELD }.into(),
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        foreground: TEXT.into(),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: None,
        border_radius: None,
        padding_ratio: 0.12,
    }
}

pub fn skel(_: &Theme) -> container::Style {
    container::Style::default().background(SKEL).border(Border::default().rounded(4.0))
}

pub fn navbtn(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style {
        background: Some(BG.into()),
        text_color: if on { TEXT } else { color!(0x6a6a6a) },
        border: Border::default().rounded(500.0),
        ..button::Style::default()
    }
}

pub fn menu_item(_: &Theme, s: button::Status) -> button::Style {
    button::Style {
        background: Some(match s {
            button::Status::Hovered => MENU_HOVER.into(),
            _ => Color::TRANSPARENT.into(),
        }),
        text_color: TEXT,
        ..button::Style::default()
    }
}

pub fn card_hover(_: &Theme, s: button::Status) -> button::Style {
    button::Style {
        background: Some(match s {
            button::Status::Hovered => ELEVATED.into(),
            button::Status::Pressed => color!(0x2a2a2a).into(),
            _ => Color::TRANSPARENT.into(),
        }),
        text_color: TEXT,
        border: Border::default().rounded(6.0),
        ..button::Style::default()
    }
}

pub fn track(sel: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, s| button::Style {
        background: Some(match s {
            button::Status::Pressed => tint(0.3).into(),
            button::Status::Hovered => tint(0.1).into(),
            _ if sel => tint(0.3).into(),
            _ => Color::TRANSPARENT.into(),
        }),
        text_color: TEXT,
        border: Border::default().rounded(4.0),
        ..button::Style::default()
    }
}

pub fn seek(_: &Theme, s: slider::Status) -> slider::Style {
    let hot = matches!(s, slider::Status::Hovered | slider::Status::Dragged);
    slider::Style {
        rail: slider::Rail {
            backgrounds: (TRACK.into(), if hot { ACCENT } else { TEXT }.into()),
            width: 4.0,
            border: Border::default().rounded(2.0),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 6.0 },
            background: if hot { TEXT } else { Color::TRANSPARENT }.into(),
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
        },
    }
}

pub fn tile(_: &Theme, s: button::Status) -> button::Style {
    button::Style {
        background: Some(match s {
            button::Status::Hovered => tint(0.2).into(),
            button::Status::Pressed => tint(0.3).into(),
            _ => tint(0.07).into(),
        }),
        text_color: TEXT,
        border: Border::default().rounded(4.0),
        ..button::Style::default()
    }
}
