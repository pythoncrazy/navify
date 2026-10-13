use image::ImageEncoder;
use tiny_skia::{Color, FillRule, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

const JOIN: f32 = 0.05;

fn arrow(s: f32) -> Vec<(f32, f32)> {
    let pts = [(3.0, 11.0), (22.0, 2.0), (13.0, 21.0), (11.0, 13.0)];
    let k = 0.54 / 19.0 * s;
    pts.iter()
        .map(|(x, y)| (s / 2.0 + (x - 13.1) * k, s / 2.0 + (y - 10.9) * k))
        .collect()
}

fn png(s: u32) -> Vec<u8> {
    let (f, id) = (s as f32, Transform::identity());
    let mut pm = Pixmap::new(s, s).expect("the size is not zero");
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(Color::from_rgba8(0x1e, 0xd7, 0x60, 0xff));
    let circle = PathBuilder::from_circle(f / 2.0, f / 2.0, f / 2.0).expect("a circle");
    pm.fill_path(&circle, &paint, FillRule::Winding, id, None);
    let pts = arrow(f);
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    pts[1..].iter().for_each(|p| pb.line_to(p.0, p.1));
    pb.close();
    let path = pb.finish().expect("the arrow has points");
    paint.set_color(Color::BLACK);
    pm.fill_path(&path, &paint, FillRule::Winding, id, None);
    let stroke = Stroke {
        width: JOIN * f,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pm.stroke_path(&path, &paint, &stroke, id, None);
    let px = pm.pixels().iter().map(|p| p.demultiply());
    let rgba: Vec<u8> = px
        .flat_map(|c| [c.red(), c.green(), c.blue(), c.alpha()])
        .collect();
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new(&mut out);
    enc.write_image(&rgba, s, s, image::ExtendedColorType::Rgba8)
        .expect("the PNG encodes");
    out
}

fn main() -> std::io::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let pts = arrow(1024.0)
        .iter()
        .map(|(x, y)| format!("{x:.1} {y:.1}"))
        .collect::<Vec<_>>();
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1024 1024\">\
         <circle cx=\"512\" cy=\"512\" r=\"512\" fill=\"#1ed760\"/><path d=\"M{}Z\" \
         fill=\"#000\" stroke=\"#000\" stroke-width=\"{:.1}\" stroke-linejoin=\"round\"/></svg>\n",
        pts.join("L"),
        JOIN * 1024.0
    );
    std::fs::write(root.join("navify.svg"), svg)?;
    let (mut head, mut body) = ([0u16, 1, 6].map(u16::to_le_bytes).concat(), Vec::new());
    for s in [16, 24, 32, 48, 64, 128, 256, 512] {
        let png = png(s);
        let dir = root.join(format!("icons/hicolor/{s}x{s}/apps"));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("navify.png"), &png)?;
        if s <= 256 && s != 128 {
            let b = (s % 256) as u8;
            let off = 6 + 16 * 6 + body.len() as u32;
            head.extend([b, b, 0, 0, 1, 0, 32, 0]);
            head.extend([png.len() as u32, off].map(u32::to_le_bytes).concat());
            body.extend(png);
        }
    }
    std::fs::write(root.join("navify.ico"), [head, body].concat())
}
