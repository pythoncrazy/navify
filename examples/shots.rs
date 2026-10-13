use std::time::{Duration, Instant};

use iced::{Font, Size, Theme, application};
use iced_test::emulator::{Emulator, Event, Mode};
use navify::app::{App, Message};

fn main() {
    unsafe { std::env::set_var("ICED_TEST_BACKEND", "tiny-skia") };
    let fonts = iced_test::renderer::graphics::text::font_system();
    let dir = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts"));
    for f in dir.expect("font directory").flatten().map(|e| e.path()) {
        if f.extension().is_some_and(|x| x == "ttf") {
            let bytes = std::fs::read(f).expect("font file");
            fonts.write().expect("font system").load_font(bytes.into());
        }
    }
    let app = application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .default_font(Font::with_name("Figtree"));
    let var = |k| std::env::var(k).unwrap_or_default();
    let size = var("NAVIFY_SHOT_SIZE");
    let (w, h) = size.split_once('x').map_or((1280.0, 800.0), |(w, h)| {
        (w.parse().expect("width"), h.parse().expect("height"))
    });
    let mut cfg = navify::config::Config::load();
    cfg.settings.window = (w, h);
    cfg.save().expect("save the configuration");
    let s = cfg.settings.zoom;
    let (tx, mut rx) = iced_test::futures::futures::channel::mpsc::channel(100);
    let mut emu = Emulator::new(tx, &app, Mode::Immediate, Size::new(w / s, h / s));
    emu.update(&app, Message::Resize(w / s, h / s));
    let steps = var("NAVIFY_STEPS");
    let mut steps = steps.split(';').map(str::trim).filter(|s| !s.is_empty());
    let wait = var("NAVIFY_SHOT_WAIT").parse().unwrap_or(12);
    let mut next = Instant::now() + Duration::from_secs(wait);
    loop {
        while let Ok(Event::Action(a)) = rx.try_recv() {
            emu.perform(&app, a);
        }
        if Instant::now() >= next {
            let Some(step) = steps.next() else { break };
            match step {
                "tab" | "shift-tab" => emu.update(&app, Message::FocusMove(step == "shift-tab")),
                "up" | "down" => emu.update(&app, Message::Arrow(step == "up")),
                "enter" => emu.update(&app, Message::Enter),
                "escape" => emu.update(&app, Message::MenuClose),
                "select-all" => emu.update(&app, Message::SelectAll),
                _ => {
                    let nums = step.split(['(', ',', ')']).map(str::trim);
                    if let [x, y] = nums.filter_map(|t| t.parse().ok()).collect::<Vec<f32>>()[..] {
                        let bits = (u64::from(x.to_bits()) << 32) | u64::from(y.to_bits());
                        navify::app::CURSOR.store(bits, std::sync::atomic::Ordering::Relaxed);
                    }
                    emu.run(&app, iced_test::Instruction::parse(step).expect("step"));
                }
            }
            next = Instant::now() + Duration::from_millis(700);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let shot = emu.screenshot(&app, &Theme::Dark, s);
    let name = std::env::var("NAVIFY_SHOT").unwrap_or("shot".into());
    let name = name.replace([':', '/', '|'], "_");
    let path = format!("target/shots/{name}.png");
    let (sw, sh, rgba) = (
        shot.size.width,
        shot.size.height,
        image::ExtendedColorType::Rgba8,
    );
    image::save_buffer(&path, shot.as_ref(), sw, sh, rgba).expect("save shot");
    eprintln!("wrote {path}");
}
