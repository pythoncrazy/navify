#[test]
fn icon_fonts_have_every_icon() {
    let fs = iced_test::renderer::graphics::text::font_system();
    let mut w = fs.write().expect("font system");
    let theme = include_str!("../src/theme.rs");
    let lucide = include_bytes!("../assets/fonts/lucide.ttf").as_slice();
    let fill = include_bytes!("../assets/fonts/phosphor-fill.ttf").as_slice();
    for (module, family, bytes) in [("icon", "lucide", lucide), ("fill", "Phosphor-Fill", fill)] {
        w.load_font(bytes.into());
        let raw = w.raw();
        let db = raw.db();
        let face = db
            .faces()
            .find(|f| f.families.iter().any(|(n, _)| n == family));
        let (id, weight) = face.map(|f| (f.id, f.weight)).expect("icon face");
        let font = raw.get_font(id, weight).expect("icon font");
        let map = font.as_swash().charmap();
        let head = format!("pub mod {module} {{");
        let body = theme.split_once(&*head).expect("icon module").1;
        let body = body.split_once("\n}").expect("module end").0;
        let cps = body
            .split("'\\u{")
            .skip(1)
            .filter_map(|r| r.split_once('}'));
        let cps: Vec<&str> = cps.map(|(cp, _)| cp).collect();
        let hex = |cp: &&&str| u32::from_str_radix(cp, 16).expect("hex code point");
        let missing: Vec<_> = cps.iter().filter(|cp| map.map(hex(cp)) == 0).collect();
        assert!(!cps.is_empty(), "{module} has no icons");
        assert!(missing.is_empty(), "{family} has no glyph for {missing:?}");
    }
}
