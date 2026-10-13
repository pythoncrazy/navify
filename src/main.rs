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

const ICON: &[u8] = include_bytes!("../assets/icons/hicolor/256x256/apps/navify.png");

#[cfg(target_os = "linux")]
fn install() -> std::io::Result<()> {
    let data = dirs::data_dir().ok_or(std::io::ErrorKind::NotFound)?;
    let exe = std::env::current_exe()?;
    let icons = data.join("icons/hicolor");
    let files = [
        (icons.join("256x256/apps/navify.png"), ICON.to_vec()),
        (icons.join("scalable/apps/navify.svg"), include_bytes!("../assets/navify.svg").to_vec()),
        (
            data.join("applications/navify.desktop"),
            include_str!("../assets/navify.desktop")
                .replace("Exec=navify", &format!("Exec=\"{}\"", exe.display()))
                .into_bytes(),
        ),
    ];
    for (path, bytes) in files {
        std::fs::create_dir_all(path.parent().expect("the path has a parent"))?;
        std::fs::write(&path, bytes)?;
        println!("{}", path.display());
    }
    Ok(())
}

fn main() -> iced::Result {
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--install") {
        if let Err(e) = install() {
            eprintln!("Cannot install the desktop entry: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let cfg = Config::load();
    #[cfg(target_os = "linux")]
    if cfg.settings.system_frame
        && std::env::var_os("DISPLAY").is_some()
        && std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|d| d == "GNOME"))
    {
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
    let icon = image::load_from_memory(ICON).expect("the bundled icon is a PNG").into_rgba8();
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
            icon: Some(
                window::icon::from_rgba(icon.to_vec(), icon.width(), icon.height())
                    .expect("the bundled icon has a valid size"),
            ),
            #[cfg(target_os = "linux")]
            platform_specific: window::settings::PlatformSpecific {
                application_id: "navify".into(),
                ..Default::default()
            },
            ..window::Settings::default()
        })
        .run()
}
