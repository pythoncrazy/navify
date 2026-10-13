#![cfg_attr(windows, windows_subsystem = "windows")]

use iced::{Font, Theme};

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

#[cfg(target_os = "linux")]
struct Probe;

#[cfg(target_os = "linux")]
impl
    wayland_client::Dispatch<
        wayland_client::protocol::wl_registry::WlRegistry,
        wayland_client::globals::GlobalListContents,
    > for Probe
{
    fn event(
        _: &mut Self, _: &wayland_client::protocol::wl_registry::WlRegistry,
        _: wayland_client::protocol::wl_registry::Event, _: &wayland_client::globals::GlobalListContents,
        _: &wayland_client::Connection, _: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

#[cfg(target_os = "linux")]
fn install() -> std::io::Result<()> {
    let data = dirs::data_dir().ok_or(std::io::ErrorKind::NotFound)?;
    let exe = std::env::current_exe()?;
    let icons = data.join("icons/hicolor");
    let files = [
        (icons.join("256x256/apps/navify.png"), navify::app::ICON.to_vec()),
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

fn view(app: &App, _: iced::window::Id) -> iced::Element<'_, navify::app::Message> {
    app.view()
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
    if ["DISPLAY", "WAYLAND_DISPLAY"].iter().all(|v| std::env::var_os(v).is_some())
        && std::env::var_os("WAYLAND_SOCKET").is_none()
    {
        use wayland_client::{Connection, globals::registry_queue_init};
        let ssd = Connection::connect_to_env().ok().and_then(|c| {
            let (globals, _) = registry_queue_init::<Probe>(&c).ok()?;
            Some(globals.contents().with_list(|l| l.iter().any(|g| g.interface == "zxdg_decoration_manager_v1")))
        });
        if ssd.is_none() || (ssd == Some(false) && cfg.settings.system_frame) {
            unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
        }
    }
    iced::daemon(App::boot, App::update, view)
        .subscription(App::subscription)
        .scale_factor(|a: &App, _| a.scale())
        .executor::<Rt>()
        .theme(|_: &App, _| Theme::Dark)
        .default_font(Font::with_name("Figtree"))
        .font(include_bytes!("../assets/fonts/Figtree-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Figtree-Medium.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Figtree-Bold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Figtree-Black.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/lucide.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/phosphor-fill.ttf").as_slice())
        .title(|a: &App, _| a.title())
        .run()
}
