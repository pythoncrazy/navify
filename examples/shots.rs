//! Headless screenshot driver: NAVIFY_SHOT=<route>[|setups] renders one PNG.
//! NAVIFY_URL/NAVIFY_USER/NAVIFY_PASS log in first when no session is saved.

use iced::{Font, Size, Theme, application, window};

use navify::app::App;

fn main() {
    let fonts: &[&[u8]] = &[
        include_bytes!("../assets/fonts/Figtree-Regular.ttf"),
        include_bytes!("../assets/fonts/Figtree-Medium.ttf"),
        include_bytes!("../assets/fonts/Figtree-Bold.ttf"),
        include_bytes!("../assets/fonts/Figtree-Black.ttf"),
        include_bytes!("../assets/fonts/lucide.ttf"),
    ];
    unsafe {
        std::env::set_var("ICED_TEST_BACKEND", "tiny-skia");
    }
    let app = application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .theme(|_: &App| Theme::Dark)
        .default_font(Font::with_name("Figtree"))
        .font(fonts[0])
        .font(fonts[1])
        .font(fonts[2])
        .font(fonts[3])
        .font(fonts[4])
        .window(window::Settings {
            size: Size::new(1280.0, 800.0),
            min_size: Some(Size::new(800.0, 600.0)),
            ..window::Settings::default()
        });
    let fs = iced_test::renderer::graphics::text::font_system();
    {
        let mut w = fs.write().expect("font system");
        for f in fonts {
            w.load_font(std::borrow::Cow::Borrowed(*f));
        }
    }
    let secs: u64 = std::env::var("NAVIFY_SHOT_WAIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let shot = iced_test::screenshot(
        &app,
        &Theme::Dark,
        Size::new(1280.0, 800.0),
        1.0,
        std::time::Duration::from_secs(secs),
    );
    let name = std::env::var("NAVIFY_SHOT")
        .unwrap_or_else(|_| "shot".into())
        .replace([':', '/', '|'], "_");
    let path = format!("/tmp/navify-shots/{name}.png");
    image::save_buffer(
        &path,
        shot.as_ref(),
        shot.size.width,
        shot.size.height,
        image::ExtendedColorType::Rgba8,
    )
    .expect("save shot");
    eprintln!("wrote {path}");
}
