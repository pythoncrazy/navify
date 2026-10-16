use std::{cell::RefCell, collections::HashMap, rc::Rc};

use iced::font::Weight;
use iced::widget::button::{Status, Style as BtnStyle};
use iced::widget::{button, container, pick_list, scrollable, slider, text, text_input, toggler};
use iced::{Background, Border, Color, Element, Font, Shadow, Theme, color};

pub const BG: Color = color!(0x000000);
pub const PANEL: Color = color!(0x121212);
pub const ELEVATED: Color = color!(0x1f1f1f);
pub const MENU: Color = color!(0x282828);
pub const MENU_HOVER: Color = color!(0x3e3e3e);
pub const CHIP: Color = color!(0x2a2a2a);
pub const TEXT: Color = color!(0xffffff);
pub const SUB: Color = color!(0xb3b3b3);
pub const DIM: Color = color!(0x7c7c7c);
pub const ACCENT: Color = color!(0x1ed760);
pub const TRACK: Color = color!(0x4d4d4d);
pub const FIELD: Color = color!(0x727272);
pub const ERR: Color = color!(0xf3727f);
pub const RED: Color = color!(0xed2c3f);
pub const CLEAR: Color = Color::TRANSPARENT;
pub const LUCIDE: Font = Font::with_name("lucide");

pub type Txt<'a> = text::Text<'a, Theme, iced::Renderer>;
pub type Btn<'a, M> = button::Button<'a, M, Theme, iced::Renderer>;

pub fn tint(alpha: f32) -> Color {
    Color::WHITE.scale_alpha(alpha)
}

pub fn shade(c: Color, k: f32) -> Color {
    let d = c.r.max(c.g).max(c.b) * (1.0 - k);
    let [r, g, b] = [c.r, c.g, c.b].map(|v| (v - d).max(0.0));
    Color::from_rgba(r, g, b, c.a)
}

pub fn dark(c: Color, l: f32) -> Color {
    let k = (2.0 * l / (c.r.max(c.g).max(c.b) + c.r.min(c.g).min(c.b))).min(1.0);
    Color::from_rgb(c.r * k, c.g * k, c.b * k)
}

pub fn fw(weight: Weight) -> Font {
    Font {
        weight,
        ..Font::with_name("Figtree")
    }
}

pub fn txt<'a>(t: impl text::IntoFragment<'a>, size: f32) -> Txt<'a> {
    text(t).size(size).font(fw(Weight::Normal))
}

pub fn bold<'a>(t: impl text::IntoFragment<'a>, size: f32) -> Txt<'a> {
    txt(t, size).font(fw(Weight::Bold))
}

pub fn sub<'a>(t: impl text::IntoFragment<'a>, size: f32) -> Txt<'a> {
    txt(t, size).color(SUB)
}

pub fn icon<'a>(c: char, size: f32) -> Txt<'a> {
    text(c).font(LUCIDE).size(size)
}

pub fn filled<'a>(c: char, size: f32) -> Txt<'a> {
    text(c).font(Font::with_name("Phosphor-Fill")).size(size)
}

pub fn saved<'a>(on: bool, size: f32) -> Txt<'a> {
    match on {
        true => filled(fill::SAVED, size).color(ACCENT),
        false => icon(icon::SAVE, size),
    }
}

pub mod fill {
    pub const PLAY: char = '\u{e3d0}';
    pub const PAUSE: char = '\u{e39e}';
    pub const PREV: char = '\u{e5a4}';
    pub const NEXT: char = '\u{e5a6}';
    pub const HEART: char = '\u{e2a8}';
    pub const SAVED: char = '\u{e184}';
    pub const HOME: char = '\u{e2c2}';
    pub const CARET: char = '\u{e136}';
}

pub mod icon {
    pub const SHUFFLE: char = '\u{e15e}';
    pub const REPEAT: char = '\u{e146}';
    pub const REPEAT1: char = '\u{e1fd}';
    pub const SAVE: char = '\u{e081}';
    pub const SAVED: char = '\u{e226}';
    pub const HOME: char = '\u{e0f5}';
    pub const SEARCH: char = '\u{e151}';
    pub const FULL: char = '\u{e112}';
    pub const DEVICE: char = '\u{e210}';
    pub const WINDOW: char = '\u{e11a}';
    pub const LIBRARY: char = '\u{e100}';
    pub const SIDE: char = '\u{e12a}';
    pub const QUEUE: char = '\u{e2e0}';
    pub const EQ: char = '\u{e55a}';
    pub const LYRICS: char = '\u{e349}';
    pub const VOL2: char = '\u{e1ab}';
    pub const VOL1: char = '\u{e1aa}';
    pub const MUTE: char = '\u{e1ac}';
    pub const PLAY: char = '\u{e13c}';
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
    pub const CLOCK: char = '\u{e087}';
    pub const LISTPLUS: char = '\u{e23f}';
    pub const USER: char = '\u{e19f}';
    pub const STAR: char = '\u{e176}';
    pub const FOLDER: char = '\u{e0d7}';
    pub const PENCIL: char = '\u{e1f9}';
    pub const PR: char = '\u{e431}';
    pub const LIST: char = '\u{e106}';
    pub const GRID: char = '\u{e0ff}';
    pub const DOWN: char = '\u{e06d}';
    pub const UP: char = '\u{e070}';
    pub const INFO: char = '\u{e0f9}';
    pub const GLOBE: char = '\u{e0e8}';
    pub const LOCK: char = '\u{e10b}';
    pub const MINUS: char = '\u{e07e}';
    pub const COMPACT: char = '\u{e115}';
    pub const ARROW_DOWN: char = '\u{e042}';
    pub const ARROW_UP: char = '\u{e04a}';
    pub const BROWSE: char = '\u{e041}';
}

fn glyphs(s: &str, size: f32, w: Weight) -> Rc<[(f32, usize)]> {
    use iced::advanced::graphics::text::{cosmic_text as ct, font_system, to_attributes};
    type Memo = HashMap<(String, u32, Weight), Rc<[(f32, usize)]>>;
    thread_local!(static MEMO: RefCell<Memo> = RefCell::default());
    MEMO.with_borrow_mut(|m| {
        if m.len() > 2048 {
            m.clear();
        }
        let key = (s.to_string(), size.to_bits(), w);
        let shape = || {
            let mut fs = font_system().write().expect("font system");
            let fs = fs.raw();
            let mut b = ct::Buffer::new(fs, ct::Metrics::new(size, size * 1.3));
            b.set_text(fs, s, &to_attributes(fw(w)), ct::Shaping::Advanced, None);
            let end = |g: &ct::LayoutGlyph| (g.x + g.w, g.end);
            let runs = b.layout_runs().flat_map(|r| r.glyphs.iter().map(end));
            runs.collect()
        };
        m.entry(key).or_insert_with(shape).clone()
    })
}

pub fn width(s: &str, size: f32, w: Weight) -> f32 {
    glyphs(s, size, w).last().map_or(0.0, |g| g.0)
}

pub fn ell<'a>(s: impl Into<std::borrow::Cow<'a, str>>, px: f32, size: f32, w: Weight) -> Txt<'a> {
    let (s, breaks) = (s.into(), ['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}']);
    let s = match s.contains(breaks) {
        true => s.replace(breaks, " ").into(),
        false => s,
    };
    let g = glyphs(&s, size, w);
    let room = px - width("\u{2026}", size, w);
    let cut = g.iter().take_while(|g| g.0 <= room).last();
    let t = match g.last().is_none_or(|g| g.0 <= px) {
        true => s,
        false => format!("{}\u{2026}", s[..cut.map_or(0, |g| g.1)].trim_end()).into(),
    };
    txt(t, size).font(fw(w)).wrapping(text::Wrapping::None)
}

pub fn btn(bg: [Color; 3], fg: [Color; 3], r: f32) -> impl Fn(&Theme, Status) -> BtnStyle {
    move |_, s| {
        let i = usize::from(s == Status::Hovered) + 2 * usize::from(s == Status::Pressed);
        let mut b = BtnStyle::default().with_background(bg[i]);
        (b.text_color, b.border) = (fg[i], Border::default().rounded(r));
        b
    }
}

pub fn link(c: Color) -> impl Fn(&Theme, Status) -> BtnStyle {
    let h = if c == ACCENT { color!(0x3be477) } else { TEXT };
    btn([CLEAR; 3], [c, h, h], 0.0)
}

pub fn track(a: f32) -> impl Fn(&Theme, Status) -> BtnStyle {
    btn([tint(a), tint(a.max(0.1)), tint(0.3)], [TEXT; 3], 4.0)
}

pub fn solid(c: [Color; 3], fg: Color) -> impl Fn(&Theme, Status) -> BtnStyle {
    btn(c, [fg; 3], 500.0)
}

pub fn accent(t: &Theme, s: Status) -> BtnStyle {
    solid([ACCENT, color!(0x3be477), color!(0x1abc54)], BG)(t, s)
}

pub fn round(c: Color) -> impl Fn(&Theme, Status) -> BtnStyle {
    btn([c, CHIP, CHIP], [SUB, TEXT, TEXT], 500.0)
}

pub fn sq<'a, M>(c: impl Into<Element<'a, M>>, px: f32) -> Btn<'a, M> {
    button(c).width(px).height(px).padding(0)
}

pub fn link_btn<'a, M: Clone + 'a>(c: impl Into<Element<'a, M>>, fg: Color, m: M) -> Btn<'a, M> {
    button(c).padding(0).style(link(fg)).on_press(m)
}

pub fn pill_btn<'a, M: Clone + 'a>(label: impl Into<Element<'a, M>>, m: M) -> Btn<'a, M> {
    let style = |t: &Theme, s| {
        let edge = if s == Status::Active { FIELD } else { TEXT };
        let mut b = link(TEXT)(t, s);
        b.border = Border::default().color(edge).width(1).rounded(500);
        b
    };
    button(label).style(style).padding([7, 15]).on_press(m)
}

pub fn chip_btn<'a, M: Clone + 'a>(label: &'a str, on: bool, m: M) -> Element<'a, M> {
    let bg = [[CHIP, color!(0x333333), CHIP], [TEXT; 3]][usize::from(on)];
    let style = solid(bg, if on { BG } else { TEXT });
    let b = button(txt(label, 14.0)).on_press(m);
    b.style(style).padding([6, 12]).into()
}

pub fn fill(c: impl Into<Background>, r: f32) -> impl Fn(&Theme) -> container::Style {
    let s = container::Style::default().background(c);
    move |_| s.border(Border::default().rounded(r))
}

pub fn down(stops: &[(f32, Color)]) -> impl Fn(&Theme) -> container::Style + use<> {
    let g = iced::gradient::Linear::new(std::f32::consts::PI);
    fill(stops.iter().fold(g, |g, s| g.add_stop(s.0, s.1)), 0.0)
}

pub fn hue(n: usize) -> Color {
    const P: [u32; 12] = [
        0xdc148c, 0x006450, 0x8400e7, 0x1e3264, 0xe8115b, 0x27856a, 0x477d95, 0xe13300, 0xba5d07,
        0x8d67ab, 0x509bf5, 0x148a08,
    ];
    let [_, r, g, b] = P[n % P.len()].to_be_bytes();
    Color::from_rgb8(r, g, b)
}

pub fn input(
    bg: [Color; 2],
    edge: [Color; 2],
    w: [f32; 2],
    r: f32,
) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |t, s| {
        let focus = matches!(s, text_input::Status::Focused { .. });
        let hot = usize::from(focus || s == text_input::Status::Hovered);
        let border = Border::default().color(edge[hot]).rounded(r);
        let mut st = text_input::default(t, s);
        (st.background, st.border) = (bg[hot].into(), border.width(w[focus as usize]));
        (st.icon, st.placeholder, st.value, st.selection) = (SUB, SUB, TEXT, tint(0.3));
        st
    }
}

pub fn seek(t: &Theme, s: slider::Status) -> slider::Style {
    let hot = s != slider::Status::Active;
    let mut st = slider::default(t, s);
    st.rail.backgrounds = (if hot { ACCENT } else { TEXT }.into(), TRACK.into());
    st.handle.shape = slider::HandleShape::Circle { radius: 6.0 };
    st.handle.background = if hot { TEXT } else { CLEAR }.into();
    st
}

pub fn tog(t: &Theme, s: toggler::Status) -> toggler::Style {
    use toggler::Status::{Active as A, Hovered as H};
    let on = matches!(s, A { is_toggled: true } | H { is_toggled: true });
    let mut st = toggler::default(t, s);
    (st.background, st.foreground) = (if on { ACCENT } else { FIELD }.into(), TEXT.into());
    st.padding_ratio = 0.12;
    st
}

pub fn pick(flat: bool) -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style {
    move |t, s| {
        let hot = s != pick_list::Status::Active;
        let fg = if hot { TEXT } else { SUB };
        let bg = [[CHIP, MENU_HOVER], [CLEAR; 2]][flat as usize][hot as usize];
        let mut st = pick_list::default(t, s);
        (st.text_color, st.placeholder_color) = (if flat { fg } else { TEXT }, SUB);
        (st.handle_color, st.background) = (fg, bg.into());
        st.border = Border::default().rounded(if flat { 0 } else { 4 });
        st
    }
}

pub fn pick_menu(t: &Theme) -> iced::overlay::menu::Style {
    let mut st = iced::overlay::menu::default(t);
    (st.background, st.border) = (MENU.into(), Border::default().rounded(4));
    (st.text_color, st.selected_text_color) = (TEXT, TEXT);
    st.selected_background = MENU_HOVER.into();
    st
}

pub fn scroll(t: &Theme, s: scrollable::Status) -> scrollable::Style {
    let a = match s {
        scrollable::Status::Active { .. } => 0.0,
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered: false,
            ..
        } => 0.3,
        _ => 0.5,
    };
    let mut st = scrollable::default(t, s);
    for r in [&mut st.vertical_rail, &mut st.horizontal_rail] {
        (r.background, r.border) = (None, Border::default());
        (r.scroller.background, r.scroller.border) = (tint(a).into(), Border::default());
    }
    (st.auto_scroll.background, st.auto_scroll.icon) = (CLEAR.into(), CLEAR);
    (st.auto_scroll.border, st.auto_scroll.shadow) = (Border::default(), Shadow::default());
    st
}
