use image::ImageEncoder;
use tiny_skia::{Color, FillRule, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

const ARROW: [(f32, f32); 4] = [(3.0, 11.0), (22.0, 2.0), (13.0, 21.0), (11.0, 13.0)];
const CENTER: (f32, f32) = (13.1, 10.9);
const SPAN: f32 = 0.54 / 19.0;
const JOIN: f32 = 0.05;

fn arrow(s: f32) -> Vec<(f32, f32)> {
    ARROW
        .iter()
        .map(|(x, y)| (s / 2.0 + (x - CENTER.0) * SPAN * s, s / 2.0 + (y - CENTER.1) * SPAN * s))
        .collect()
}

fn render(s: u32) -> Vec<u8> {
    let f = s as f32;
    let mut pm = Pixmap::new(s, s).expect("the size is not zero");
    let mut paint = Paint { anti_alias: true, ..Paint::default() };
    paint.set_color(Color::from_rgba8(0x1e, 0xd7, 0x60, 0xff));
    let circle =
        PathBuilder::from_circle(f / 2.0, f / 2.0, f / 2.0).expect("the radius is positive");
    pm.fill_path(&circle, &paint, FillRule::Winding, Transform::identity(), None);
    let mut pb = PathBuilder::new();
    let pts = arrow(f);
    pb.move_to(pts[0].0, pts[0].1);
    pts[1..].iter().for_each(|p| pb.line_to(p.0, p.1));
    pb.close();
    let path = pb.finish().expect("the arrow has points");
    paint.set_color(Color::BLACK);
    pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    let stroke = Stroke { width: JOIN * f, line_join: LineJoin::Round, ..Stroke::default() };
    pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    let rgba: Vec<u8> = pm
        .pixels()
        .iter()
        .map(|p| p.demultiply())
        .flat_map(|c| [c.red(), c.green(), c.blue(), c.alpha()])
        .collect();
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&rgba, s, s, image::ExtendedColorType::Rgba8)
        .expect("the PNG encodes");
    png
}

fn main() -> std::io::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let pts = arrow(1024.0);
    let d = pts
        .iter()
        .enumerate()
        .map(|(n, (x, y))| format!("{}{x:.1} {y:.1}", if n == 0 { "M" } else { "L" }))
        .collect::<String>();
    std::fs::write(
        root.join("navify.svg"),
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1024 1024\"><circle cx=\"512\" cy=\"512\" r=\"512\" fill=\"#1ed760\"/><path d=\"{d}Z\" fill=\"#000\" stroke=\"#000\" stroke-width=\"{:.1}\" stroke-linejoin=\"round\"/></svg>\n",
            JOIN * 1024.0
        ),
    )?;
    let mut ico = Vec::new();
    for s in [16, 24, 32, 48, 64, 128, 256, 512] {
        let png = render(s);
        let dir = root.join(format!("icons/hicolor/{s}x{s}/apps"));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("navify.png"), &png)?;
        if matches!(s, 16 | 24 | 32 | 48 | 64 | 256) {
            ico.push((s, png));
        }
    }
    let mut out =
        [0u16, 1, ico.len() as u16].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
    let mut off = 6 + 16 * ico.len() as u32;
    for (s, png) in &ico {
        let b = (*s % 256) as u8;
        out.extend([b, b, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((png.len() as u32).to_le_bytes());
        out.extend(off.to_le_bytes());
        off += png.len() as u32;
    }
    ico.iter().for_each(|(_, png)| out.extend(png));
    std::fs::write(root.join("navify.ico"), out)
}
