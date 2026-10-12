#[test]
fn icon_fonts_have_every_icon() {
    let fs = iced_test::renderer::graphics::text::font_system();
    let mut w = fs.write().expect("font system");
    let theme = include_str!("../src/theme.rs");
    for (module, family, bytes) in [
        ("icon", "lucide", include_bytes!("../assets/fonts/lucide.ttf").as_slice()),
        ("fill", "Phosphor-Fill", include_bytes!("../assets/fonts/phosphor-fill.ttf").as_slice()),
    ] {
        w.load_font(std::borrow::Cow::Borrowed(bytes));
        let raw = w.raw();
        let face = raw
            .db()
            .faces()
            .find(|f| f.families.iter().any(|(n, _)| n == family))
            .map(|f| (f.id, f.weight))
            .expect("icon face");
        let font = raw.get_font(face.0, face.1).expect("icon font");
        let map = font.as_swash().charmap();
        let body = theme.split(&format!("pub mod {module} {{")).nth(1).expect("icon module");
        let missing: Vec<&str> = body[..body.find("\n}").expect("module end")]
            .lines()
            .filter_map(|l| l.trim().strip_prefix("pub const ")?.split_once(": char = '\\u{"))
            .filter(|(_, rest)| {
                let cp = u32::from_str_radix(rest.split('}').next().expect("code point"), 16);
                map.map(cp.expect("hex code point")) == 0
            })
            .map(|(name, _)| name)
            .collect();
        assert!(missing.is_empty(), "{family} has no glyph for {missing:?}");
    }
}
