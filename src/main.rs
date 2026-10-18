#![cfg_attr(windows, windows_subsystem = "windows")]

use iced::{Font, Theme};

use navify::app::{App, Message};

#[cfg(target_os = "linux")]
mod linux {
    use wayland_client::globals::{Global, GlobalListContents as G};
    use wayland_client::protocol::wl_registry::{Event as E, WlRegistry as R};
    use wayland_client::{Connection as C, Dispatch, QueueHandle as Q};

    pub struct Probe;

    impl Dispatch<R, G> for Probe {
        fn event(_: &mut Self, _: &R, _: E, _: &G, _: &C, _: &Q<Self>) {}
    }

    pub fn x11() -> bool {
        let set = ["DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET"].map(std::env::var_os);
        let ssd = || {
            let c = C::connect_to_env().ok()?;
            let (globals, _) = wayland_client::globals::registry_queue_init::<Probe>(&c).ok()?;
            let has = |l: &[Global]| l.iter().any(|g| g.interface.starts_with("zxdg_decoration"));
            Some(globals.contents().with_list(has))
        };
        let frame = || navify::config::Config::load().settings.system_frame;
        set.map(|v| v.is_some()) == [true, true, false] && ssd().is_none_or(|s| !s && frame())
    }

    pub fn install() -> std::io::Result<()> {
        let data = navify::config::dir("data");
        let exe = std::env::var_os("APPIMAGE").map_or_else(std::env::current_exe, |p| Ok(p.into()));
        let exe = exe?
            .display()
            .to_string()
            .replace('\\', r"\\")
            .replace('"', r#"\""#);
        let exe = exe
            .replace('`', r"\`")
            .replace('$', r"\$")
            .replace('\\', r"\\");
        let exe = format!("Exec=\"{}\"", exe.replace('%', "%%"));
        let desktop = include_str!("../assets/navify.desktop").replace("Exec=navify", &exe);
        let svg = include_bytes!("../assets/navify.svg");
        for (path, bytes) in [
            ("icons/hicolor/256x256/apps/navify.png", navify::app::ICON),
            ("icons/hicolor/scalable/apps/navify.svg", &svg[..]),
            ("applications/navify.desktop", desktop.as_bytes()),
        ] {
            let path = data.join(path);
            std::fs::create_dir_all(path.parent().expect("the path has a parent"))?;
            std::fs::write(&path, bytes)?;
            println!("{}", path.display());
        }
        Ok(())
    }
}

fn view(app: &App, _: iced::window::Id) -> iced::Element<'_, Message> {
    app.view()
}

fn main() -> iced::Result {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 1);
        libc::mallopt(libc::M_MMAP_THRESHOLD, 128 * 1024);
    }
    let arg = std::env::args().nth(1);
    if matches!(arg.as_deref(), Some("-V" | "--version")) {
        println!("navify {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    if arg.as_deref() == Some("--install") {
        let r = linux::install().map_err(|e| eprintln!("Cannot install the desktop entry: {e}"));
        std::process::exit(r.is_err() as i32);
    }
    unsafe { std::env::set_var("TOKIO_WORKER_THREADS", "2") };
    #[cfg(target_os = "linux")]
    if linux::x11() {
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
    let fonts: [&[u8]; 6] = [
        include_bytes!("../assets/fonts/Figtree-Regular.ttf"),
        include_bytes!("../assets/fonts/Figtree-Medium.ttf"),
        include_bytes!("../assets/fonts/Figtree-Bold.ttf"),
        include_bytes!("../assets/fonts/Figtree-Black.ttf"),
        include_bytes!("../assets/fonts/lucide.ttf"),
        include_bytes!("../assets/fonts/phosphor-fill.ttf"),
    ];
    let app = iced::daemon(App::boot, App::update, view)
        .subscription(App::subscription)
        .scale_factor(|a: &App, _| a.scale())
        .theme(|_: &App, _| Theme::Dark)
        .style(|_, _| iced::theme::Style {
            background_color: navify::theme::PANEL,
            text_color: iced::Color::WHITE,
        })
        .default_font(Font::with_name("Figtree"))
        .title(|a: &App, _| a.title());
    fonts.into_iter().fold(app, |app, f| app.font(f)).run()
}
