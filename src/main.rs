use iced::{Font, Size, Theme, application, window};

use navify::app::App;
use navify::config::Config;

struct Rt(tokio::runtime::Runtime);

impl iced::Executor for Rt {
    fn new() -> std::io::Result<Self> {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().map(Rt)
    }

    fn spawn(&self, f: impl Future<Output = ()> + Send + 'static) {
        drop(self.0.spawn(f));
    }

    fn block_on<T>(&self, f: impl Future<Output = T>) -> T {
        self.0.block_on(f)
    }

    fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        let _g = self.0.enter();
        f()
    }
}

fn main() -> iced::Result {
    let cfg = Config::load();
    let ((w, h), s) = (cfg.settings.window, cfg.settings.zoom);
    application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .scale_factor(App::scale)
        .executor::<Rt>()
        .theme(|_: &App| Theme::Dark)
        .default_font(Font::with_name("Figtree"))
        .font(include_bytes!("../assets/fonts/Figtree-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Figtree-Medium.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Figtree-Bold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Figtree-Black.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/lucide.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/phosphor-fill.ttf").as_slice())
        .title(App::title)
        .window(window::Settings {
            size: Size::new(w.max(800.0), h.max(600.0)) * (1.0 / s),
            min_size: Some(Size::new(800.0, 600.0)),
            ..window::Settings::default()
        })
        .run()
}
