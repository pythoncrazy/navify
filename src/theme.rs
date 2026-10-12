use std::{cell::RefCell, collections::HashMap, rc::Rc};

use iced::font::{Family, Weight};
use iced::overlay::menu;
use iced::widget::{button, container, pick_list, scrollable, slider, text, text_input, toggler};
use iced::{Border, Color, Font, Gradient, Radians, Shadow, Theme, color, gradient};

pub const BG: Color = color!(0x000000);
pub const PANEL: Color = color!(0x121212);
pub const ELEVATED: Color = color!(0x1f1f1f);
pub const MENU: Color = color!(0x282828);
pub const MENU_HOVER: Color = color!(0x3e3e3e);
pub const CHIP: Color = color!(0x2a2a2a);
pub const TEXT: Color = color!(0xffffff);
pub const SUB: Color = color!(0xb3b3b3);
pub const DIM: Color = color!(0x6a6a6a);
pub const ACCENT: Color = color!(0x1ed760);
pub const TRACK: Color = color!(0x4d4d4d);
pub const FIELD: Color = color!(0x727272);
pub const ERR: Color = color!(0xf15e6c);
pub const GRAD_A: Color = color!(0x450af5);
pub const GRAD_B: Color = color!(0xc4efd9);
pub const LUCIDE: Font = Font::with_name("lucide");
pub const FILL: Font = Font::with_name("Phosphor-Fill");

pub type Txt<'a> = text::Text<'a, Theme, iced::Renderer>;

pub fn tint(alpha: f32) -> Color {
    Color { a: alpha, ..Color::WHITE }
}

pub fn shade(c: Color, k: f32) -> Color {
    Color { r: c.r * k, g: c.g * k, b: c.b * k, a: c.a }
}

pub fn fw(w: Weight) -> Font {
    Font { family: Family::Name("Figtree"), weight: w, ..Font::DEFAULT }
}

pub fn txt<'a>(t: impl ToString, size: f32, w: Weight) -> Txt<'a> {
    text(t.to_string()).size(size).font(fw(w))
}

pub fn black<'a>(t: impl ToString, size: f32) -> Txt<'a> {
    txt(t, size, Weight::Black)
}

pub fn bold<'a>(t: impl ToString, size: f32) -> Txt<'a> {
    txt(t, size, Weight::Bold)
}

pub fn med<'a>(t: impl ToString, size: f32) -> Txt<'a> {
    txt(t, size, Weight::Medium)
}

pub fn sub<'a>(t: impl ToString, size: f32) -> Txt<'a> {
    txt(t, size, Weight::Normal).color(SUB)
}

pub fn icon<'a>(c: char, size: f32) -> Txt<'a> {
    text(c).font(LUCIDE).size(size)
}

pub fn filled<'a>(c: char, size: f32) -> Txt<'a> {
    text(c).font(FILL).size(size)
}

pub fn heart<'a>(on: bool, size: f32) -> Txt<'a> {
    if on { filled(fill::HEART, size) } else { icon(icon::HEART, size) }
}

pub mod fill {
    pub const PLAY: char = '\u{e3d0}';
    pub const PAUSE: char = '\u{e39e}';
    pub const PREV: char = '\u{e5a4}';
    pub const NEXT: char = '\u{e5a6}';
    pub const HEART: char = '\u{e2a8}';
    pub const HOME: char = '\u{e2c2}';
}

fn glyphs(s: &str, size: f32, w: Weight) -> Rc<[(f32, usize)]> {
    use iced::advanced::graphics::text::{cosmic_text as ct, font_system, to_attributes};
    type Memo = HashMap<(String, u32, Weight), Rc<[(f32, usize)]>>;
    thread_local!(static MEMO: RefCell<Memo> = RefCell::default());
    MEMO.with_borrow_mut(|m| {
        if m.len() > 2048 {
            m.clear();
        }
        m.entry((s.to_string(), size.to_bits(), w))
            .or_insert_with(|| {
                let Ok(mut fs) = font_system().write() else { return Rc::new([]) };
                let fs = fs.raw();
                let mut b = ct::Buffer::new(fs, ct::Metrics::new(size, size * 1.3));
                b.set_text(fs, s, &to_attributes(fw(w)), ct::Shaping::Advanced, None);
                b.layout_runs().flat_map(|r| r.glyphs.iter().map(|g| (g.x + g.w, g.end))).collect()
            })
            .clone()
    })
}

pub fn width(s: &str, size: f32, w: Weight) -> f32 {
    glyphs(s, size, w).last().map_or(0.0, |g| g.0)
}

pub fn ell<'a>(s: &str, px: f32, size: f32, w: Weight) -> Txt<'a> {
    let g = glyphs(s, size, w);
    let t = if g.last().is_none_or(|g| g.0 <= px) {
        s.to_string()
    } else {
        let room = px - width("\u{2026}", size, w);
        let cut = g.iter().take_while(|g| g.0 <= room).last().map_or(0, |g| g.1);
        format!("{}\u{2026}", s[..cut].trim_end())
    };
    txt(t, size, w).wrapping(text::Wrapping::None)
}

pub mod icon {
    pub const PLAY: char = '\u{e13c}';
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
    pub const SETTINGS: char = '\u{e154}';
    pub const MUSIC: char = '\u{e122}';
    pub const ALBUM: char = '\u{e494}';
    pub const RADIO: char = '\u{e142}';
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
    pub const LOGOUT: char = '\u{e10e}';
    pub const STAR: char = '\u{e176}';
    pub const FOLDER: char = '\u{e0d7}';
    pub const PENCIL: char = '\u{e1f9}';
    pub const TRASH: char = '\u{e18e}';
    pub const PR: char = '\u{e431}';
    pub const SERVER: char = '\u{e153}';
}

fn bg(color: impl Into<iced::Background>, radius: f32) -> container::Style {
    container::Style::default().background(color).border(Border::default().rounded(radius))
}

pub fn panel(_: &Theme) -> container::Style {
    bg(PANEL, 8.0)
}

pub fn elevated(_: &Theme) -> container::Style {
    bg(ELEVATED, 8.0)
}

pub fn skel(_: &Theme) -> container::Style {
    bg(CHIP, 4.0)
}

pub fn fill(c: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| bg(c, radius)
}

pub fn menu(_: &Theme) -> container::Style {
    container::Style {
        shadow: Shadow { color: Color { a: 0.5, ..BG }, blur_radius: 24.0, ..Shadow::default() },
        ..bg(MENU, 4.0)
    }
}

pub fn grad(top: Color, bottom: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        bg(
            Gradient::Linear(
                gradient::Linear::new(Radians::PI).add_stop(0.0, top).add_stop(1.0, bottom),
            ),
            0.0,
        )
    }
}

pub fn fade(top: Color, stop: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        bg(
            Gradient::Linear(
                gradient::Linear::new(Radians::PI)
                    .add_stop(0.0, top)
                    .add_stop(stop.clamp(0.0, 1.0), PANEL)
                    .add_stop(1.0, PANEL),
            ),
            0.0,
        )
    }
}

pub fn color_of(s: &str) -> Color {
    const P: [Color; 12] = [
        color!(0xdc148c),
        color!(0x006450),
        color!(0x8400e7),
        color!(0x1e3264),
        color!(0xe8115b),
        color!(0x27856a),
        color!(0x477d95),
        color!(0xe13300),
        color!(0xba5d07),
        color!(0x8d67ab),
        color!(0x509bf5),
        color!(0x148a08),
    ];
    P[s.bytes().fold(7usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize)) % P.len()]
}

pub fn diagonal(radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        bg(
            Gradient::Linear(
                gradient::Linear::new(Radians(2.4)).add_stop(0.0, GRAD_A).add_stop(1.0, GRAD_B),
            ),
            radius,
        )
    }
}

pub fn search_field(_: &Theme, s: text_input::Status) -> text_input::Style {
    let hot = matches!(s, text_input::Status::Hovered | text_input::Status::Focused { .. });
    text_input::Style {
        background: if hot { color!(0x2a2a2a) } else { ELEVATED }.into(),
        border: Border {
            color: if matches!(s, text_input::Status::Focused { .. }) { TEXT } else { BG },
            width: if matches!(s, text_input::Status::Focused { .. }) { 2.0 } else { 0.0 },
            radius: 500.0.into(),
        },
        icon: SUB,
        placeholder: SUB,
        value: TEXT,
        selection: tint(0.3),
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
            width: if matches!(s, text_input::Status::Focused { .. }) { 2.0 } else { 1.0 },
            radius: 4.0.into(),
        },
        icon: SUB,
        placeholder: SUB,
        value: TEXT,
        selection: tint(0.3),
    }
}

pub fn modal_field(_: &Theme, s: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: MENU_HOVER.into(),
        border: Border {
            color: color!(0x535353),
            width: if matches!(s, text_input::Status::Focused { .. }) { 1.0 } else { 0.0 },
            radius: 4.0.into(),
        },
        icon: SUB,
        placeholder: SUB,
        value: TEXT,
        selection: tint(0.3),
    }
}

fn btn_style(bg: Color, fg: Color, radius: f32) -> button::Style {
    button::Style {
        background: Some(bg.into()),
        text_color: fg,
        border: Border::default().rounded(radius),
        ..button::Style::default()
    }
}

pub fn link(base: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, s| {
        let hot = matches!(s, button::Status::Hovered | button::Status::Pressed);
        btn_style(Color::TRANSPARENT, if hot { TEXT } else { base }, 0.0)
    }
}

pub fn ghost(t: &Theme, s: button::Status) -> button::Style {
    link(TEXT)(t, s)
}

pub fn track(sel: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, s| {
        let a = match s {
            button::Status::Pressed => 0.3,
            _ if sel => 0.3,
            button::Status::Hovered => 0.1,
            _ => 0.0,
        };
        btn_style(tint(a), TEXT, 4.0)
    }
}

pub fn chip(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, s| match (on, s) {
        (true, _) => btn_style(TEXT, BG, 500.0),
        (false, button::Status::Hovered) => btn_style(color!(0x333333), TEXT, 500.0),
        _ => btn_style(CHIP, TEXT, 500.0),
    }
}

pub fn accent(_: &Theme, s: button::Status) -> button::Style {
    let c = match s {
        button::Status::Hovered => color!(0x3be477),
        button::Status::Pressed => color!(0x1abc54),
        _ => ACCENT,
    };
    btn_style(c, BG, 500.0)
}

pub fn white(_: &Theme, s: button::Status) -> button::Style {
    let hot = matches!(s, button::Status::Hovered | button::Status::Pressed);
    btn_style(if hot { color!(0xf0f0f0) } else { TEXT }, BG, 500.0)
}

pub fn pill(_: &Theme, s: button::Status) -> button::Style {
    let hot = matches!(s, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        border: Border { color: if hot { TEXT } else { FIELD }, width: 1.0, radius: 500.0.into() },
        ..btn_style(Color::TRANSPARENT, TEXT, 500.0)
    }
}

pub fn round(c: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, s| {
        let hot = matches!(s, button::Status::Hovered | button::Status::Pressed);
        btn_style(if hot { color!(0x2a2a2a) } else { c }, if hot { TEXT } else { SUB }, 500.0)
    }
}

pub fn menu_item(_: &Theme, s: button::Status) -> button::Style {
    let hot = matches!(s, button::Status::Hovered | button::Status::Pressed);
    btn_style(if hot { MENU_HOVER } else { Color::TRANSPARENT }, TEXT, 2.0)
}

pub fn card(_: &Theme, s: button::Status) -> button::Style {
    let c = match s {
        button::Status::Hovered => ELEVATED,
        button::Status::Pressed => BG,
        _ => Color::TRANSPARENT,
    };
    btn_style(c, TEXT, 6.0)
}

pub fn tile(_: &Theme, s: button::Status) -> button::Style {
    let a = match s {
        button::Status::Hovered => 0.2,
        button::Status::Pressed => 0.3,
        _ => 0.1,
    };
    btn_style(tint(a), TEXT, 4.0)
}

pub fn seek(_: &Theme, s: slider::Status) -> slider::Style {
    let hot = matches!(s, slider::Status::Hovered | slider::Status::Dragged);
    slider::Style {
        rail: slider::Rail {
            backgrounds: (if hot { ACCENT } else { TEXT }.into(), TRACK.into()),
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

pub fn pick(_: &Theme, s: pick_list::Status) -> pick_list::Style {
    let hot = !matches!(s, pick_list::Status::Active);
    pick_list::Style {
        text_color: TEXT,
        placeholder_color: SUB,
        handle_color: if hot { TEXT } else { SUB },
        background: if hot { MENU_HOVER } else { CHIP }.into(),
        border: Border::default().rounded(4),
    }
}

pub fn pick_flat(_: &Theme, s: pick_list::Status) -> pick_list::Style {
    let hot = !matches!(s, pick_list::Status::Active);
    pick_list::Style {
        text_color: if hot { TEXT } else { SUB },
        placeholder_color: SUB,
        handle_color: if hot { TEXT } else { SUB },
        background: Color::TRANSPARENT.into(),
        border: Border::default(),
    }
}

pub fn pick_menu(_: &Theme) -> menu::Style {
    menu::Style {
        background: MENU.into(),
        border: Border::default().rounded(4),
        text_color: TEXT,
        selected_text_color: TEXT,
        selected_background: MENU_HOVER.into(),
        shadow: Shadow { color: Color { a: 0.5, ..BG }, blur_radius: 24.0, ..Shadow::default() },
    }
}

pub fn scroll(_: &Theme, s: scrollable::Status) -> scrollable::Style {
    let a = match s {
        scrollable::Status::Active { .. } => 0.0,
        scrollable::Status::Hovered { is_vertical_scrollbar_hovered: false, .. } => 0.3,
        _ => 0.5,
    };
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller { background: tint(a).into(), border: Border::default() },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Color::TRANSPARENT.into(),
            border: Border::default(),
            shadow: Shadow::default(),
            icon: Color::TRANSPARENT,
        },
    }
}

pub fn pad(top: f32, right: f32, bottom: f32, left: f32) -> iced::Padding {
    iced::Padding { top, right, bottom, left }
}
