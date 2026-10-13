#![cfg_attr(windows, windows_subsystem = "windows")]

use iced::{Font, Theme};

use navify::app::{App, Message};

#[cfg(target_os = "linux")]
mod linux {
    use wayland_client::protocol::wl_registry::{Event, WlRegistry};
    use wayland_client::{Connection, Dispatch, QueueHandle, globals::GlobalListContents};

    pub struct Probe;

    impl Dispatch<WlRegistry, GlobalListContents> for Probe {
        fn event(
            _: &mut Self,
            _: &WlRegistry,
            _: Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    pub fn decorations() -> Option<bool> {
        let c = Connection::connect_to_env().ok()?;
        let (globals, _) = wayland_client::globals::registry_queue_init::<Probe>(&c).ok()?;
        let has = |l: &[_]| {
            l.iter().any(|g: &wayland_client::globals::Global| {
                g.interface == "zxdg_decoration_manager_v1"
            })
        };
        Some(globals.contents().with_list(has))
    }

    pub fn install() -> std::io::Result<()> {
        let data = dirs::data_dir().ok_or(std::io::ErrorKind::NotFound)?;
        let exe = format!("Exec=\"{}\"", std::env::current_exe()?.display());
        let desktop = include_str!("../assets/navify.desktop").replace("Exec=navify", &exe);
        for (path, bytes) in [
            ("icons/hicolor/256x256/apps/navify.png", navify::app::ICON),
            (
                "icons/hicolor/scalable/apps/navify.svg",
                include_bytes!("../assets/navify.svg"),
            ),
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
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--install") {
        if let Err(e) = linux::install() {
            eprintln!("Cannot install the desktop entry: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    unsafe { std::env::set_var("TOKIO_WORKER_THREADS", "2") };
    #[cfg(target_os = "linux")]
    if ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .all(|v| std::env::var_os(v).is_some())
        && std::env::var_os("WAYLAND_SOCKET").is_none()
    {
        let ssd = linux::decorations();
        if ssd.is_none()
            || (ssd == Some(false) && navify::config::Config::load().settings.system_frame)
        {
            unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
        }
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
        .default_font(Font::with_name("Figtree"))
        .title(|a: &App, _| a.title());
    fonts.into_iter().fold(app, |app, f| app.font(f)).run()
}
