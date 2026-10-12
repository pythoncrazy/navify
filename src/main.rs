use iced::{Font, Size, Theme, application, window};

use navify::app::App;
use navify::config::Config;

fn main() -> iced::Result {
    let cfg = Config::load();
    let (w, h) = cfg.settings.window;
    application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .theme(|_: &App| Theme::Dark)
        .default_font(Font::with_name("Figtree"))
        .font(include_bytes!("../assets/fonts/Figtree-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Figtree-Medium.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Figtree-Bold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Figtree-Black.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/lucide.ttf").as_slice())
        .title(App::title)
        .window(window::Settings {
            size: Size::new(w.max(800.0), h.max(600.0)),
            min_size: Some(Size::new(800.0, 600.0)),
            ..window::Settings::default()
        })
        .run()
}
