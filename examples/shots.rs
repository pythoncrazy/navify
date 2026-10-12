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
        include_bytes!("../assets/fonts/phosphor-fill.ttf"),
    ];
    unsafe {
        std::env::set_var("ICED_TEST_BACKEND", "tiny-skia");
    }
    let app = application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .scale_factor(|a: &App| a.cfg.settings.zoom)
        .theme(|_: &App| Theme::Dark)
        .default_font(Font::with_name("Figtree"))
        .font(fonts[0])
        .font(fonts[1])
        .font(fonts[2])
        .font(fonts[3])
        .font(fonts[4])
        .font(fonts[5])
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
    use iced_test::emulator::{Emulator, Event, Mode};
    use navify::app::Message;
    use std::time::{Duration, Instant};
    let secs: u64 =
        std::env::var("NAVIFY_SHOT_WAIT").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
    let steps = std::env::var("NAVIFY_STEPS").unwrap_or_default();
    let mut steps = steps.split(';').map(str::trim).filter(|s| !s.is_empty());
    let (tx, mut rx) = iced_test::futures::futures::channel::mpsc::channel(100);
    let (w, h) = std::env::var("NAVIFY_SHOT_SIZE")
        .ok()
        .and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
        .unwrap_or((1280.0, 800.0));
    let mut emu = Emulator::new(tx, &app, Mode::Immediate, Size::new(w, h));
    emu.update(&app, Message::Resize(w, h));
    let mut next = Instant::now() + Duration::from_secs(secs);
    loop {
        while let Ok(e) = rx.try_recv() {
            if let Event::Action(a) = e {
                emu.perform(&app, a);
            }
        }
        if Instant::now() >= next {
            let Some(s) = steps.next() else { break };
            match s {
                "tab" => emu.update(&app, Message::FocusMove(false)),
                "shift-tab" => emu.update(&app, Message::FocusMove(true)),
                "up" => emu.update(&app, Message::Arrow(true)),
                "down" => emu.update(&app, Message::Arrow(false)),
                "enter" => emu.update(&app, Message::Enter),
                "escape" => emu.update(&app, Message::MenuClose),
                "select-all" => emu.update(&app, Message::SelectAll),
                _ => {
                    if let Some((x, y)) = s
                        .split_once('(')
                        .and_then(|(_, r)| r.trim_end_matches(')').split_once(','))
                        .and_then(|(x, y)| {
                            Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?))
                        })
                    {
                        let bits = (u64::from(x.to_bits()) << 32) | u64::from(y.to_bits());
                        navify::app::CURSOR.store(bits, std::sync::atomic::Ordering::Relaxed);
                    }
                    emu.run(&app, iced_test::Instruction::parse(s).expect("step"))
                }
            }
            next = Instant::now() + Duration::from_millis(700);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let shot = emu.screenshot(&app, &Theme::Dark, 1.0);
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
