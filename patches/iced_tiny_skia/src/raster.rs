use crate::core::image as raster;
use crate::core::{Rectangle, Size};
use crate::graphics;

use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::hash_map;

#[derive(Debug)]
pub struct Pipeline {
    cache: RefCell<Cache>,
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            cache: RefCell::new(Cache::default()),
        }
    }

    pub fn load(
        &self,
        handle: &raster::Handle,
    ) -> Result<raster::Allocation, raster::Error> {
        let mut cache = self.cache.borrow_mut();
        let image = cache.allocate(handle)?;

        #[allow(unsafe_code)]
        Ok(unsafe {
            raster::allocate(handle, Size::new(image.width, image.height))
        })
    }

    pub fn dimensions(&self, handle: &raster::Handle) -> Option<Size<u32>> {
        let mut cache = self.cache.borrow_mut();
        let image = cache.allocate(handle).ok()?;

        Some(Size::new(image.width, image.height))
    }

    pub fn draw(
        &mut self,
        handle: &raster::Handle,
        filter_method: raster::FilterMethod,
        bounds: Rectangle,
        opacity: f32,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        transform: tiny_skia::Transform,
        clip_mask: Option<&tiny_skia::Mask>,
    ) {
        let mut cache = self.cache.borrow_mut();

        let Ok(entry) = cache.allocate(handle) else {
            return;
        };

        // An upright image is scaled one time to its display size and
        // then copied to whole pixels. The edges are rounded, so the copy
        // stays inside the bounds of the image.
        let edge =
            |at: f32, scale: f32, move_by: f32| (at * scale + move_by).round();
        let (sx, sy, tx, ty) =
            (transform.sx, transform.sy, transform.tx, transform.ty);
        let (x, y) = (edge(bounds.x, sx, tx), edge(bounds.y, sy, ty));
        let width = edge(bounds.x + bounds.width, sx, tx) - x;
        let height = edge(bounds.y + bounds.height, sy, ty) - y;

        if !transform.has_skew()
            && filter_method == raster::FilterMethod::Linear
            && width >= 1.0
            && height >= 1.0
            && width as u32 <= entry.width
            && height as u32 <= entry.height
            && let Some(scaled) = entry.fit(handle, width as u32, height as u32)
        {
            let (x, y) = (x as i32, y as i32);

            if clip_mask.is_none() && opacity >= 1.0 {
                return crate::text::blit(pixels, x, y, scaled, 1.0);
            }

            return pixels.draw_pixmap(
                x,
                y,
                scaled,
                &tiny_skia::PixmapPaint {
                    opacity,
                    ..Default::default()
                },
                tiny_skia::Transform::identity(),
                clip_mask,
            );
        }

        entry.keep = true;

        let Some(image) = entry.image(handle) else {
            return;
        };

        let width_scale = bounds.width / image.width() as f32;
        let height_scale = bounds.height / image.height() as f32;

        let transform = transform.pre_scale(width_scale, height_scale);

        let quality = match filter_method {
            raster::FilterMethod::Linear => tiny_skia::FilterQuality::Bilinear,
            raster::FilterMethod::Nearest => tiny_skia::FilterQuality::Nearest,
        };

        pixels.draw_pixmap(
            (bounds.x / width_scale) as i32,
            (bounds.y / height_scale) as i32,
            image,
            &tiny_skia::PixmapPaint {
                quality,
                opacity,
                ..Default::default()
            },
            transform,
            clip_mask,
        );
    }

    pub fn trim_cache(&mut self) {
        self.cache.borrow_mut().trim();
    }
}

// Entries, copies, and full-size pixels that were not used in the last
// `AGE` frames are dropped. A partial redraw thus keeps the covers that
// are on screen but outside the damage area.
const AGE: u64 = 30;
const PIXELS: u64 = 1;

#[derive(Debug, Default)]
struct Cache {
    entries: FxHashMap<raster::Id, (u64, Option<Entry>)>,
    frame: u64,
}

impl Cache {
    pub fn allocate(
        &mut self,
        handle: &raster::Handle,
    ) -> Result<&mut Entry, raster::Error> {
        let frame = self.frame;

        if let hash_map::Entry::Vacant(entry) = self.entries.entry(handle.id())
        {
            match decode(handle) {
                Ok((width, height, pixels)) => {
                    let _ = entry.insert((
                        frame,
                        Some(Entry {
                            width,
                            height,
                            pixels,
                            scaled: Vec::new(),
                            keep: false,
                            used: frame,
                            frame,
                        }),
                    ));
                }
                Err(raster::Error::Empty) => return Err(raster::Error::Empty),
                Err(error) => {
                    let _ = entry.insert((frame, None));

                    return Err(error);
                }
            }
        }

        let (seen, entry) = self
            .entries
            .get_mut(&handle.id())
            .expect("Image should be allocated");

        *seen = frame;

        let entry = entry.as_mut().ok_or(raster::Error::Empty)?;
        entry.frame = frame;

        Ok(entry)
    }

    fn trim(&mut self) {
        let frame = self.frame;

        self.entries.retain(|_, (seen, _)| *seen + AGE > frame);

        for entry in self.entries.values_mut().filter_map(|e| e.1.as_mut()) {
            entry.scaled.retain(|s| s.1 + AGE > frame);

            if !entry.keep && entry.used + PIXELS <= frame {
                entry.pixels = Vec::new();
            }
        }

        self.frame += 1;
    }
}

fn decode(
    handle: &raster::Handle,
) -> Result<(u32, u32, Vec<u32>), raster::Error> {
    let image = graphics::image::load(handle)?;

    if image.width() == 0 || image.height() == 0 {
        return Err(raster::Error::Empty);
    }

    let pixels = image
        .pixels()
        .map(|pixel| {
            let [r, g, b, a] = pixel.0;

            bytemuck::cast(
                tiny_skia::ColorU8::from_rgba(b, g, r, a).premultiply(),
            )
        })
        .collect();

    Ok((image.width(), image.height(), pixels))
}

#[derive(Debug)]
struct Entry {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
    scaled: Vec<(tiny_skia::Pixmap, u64)>,
    keep: bool,
    used: u64,
    frame: u64,
}

impl Entry {
    fn image(
        &mut self,
        handle: &raster::Handle,
    ) -> Option<tiny_skia::PixmapRef<'_>> {
        self.used = self.frame;

        if self.pixels.is_empty() {
            let (width, height, pixels) = decode(handle).ok()?;

            if (width, height) != (self.width, self.height) {
                self.scaled.clear();
            }

            (self.width, self.height, self.pixels) = (width, height, pixels);
        }

        tiny_skia::PixmapRef::from_bytes(
            bytemuck::cast_slice(&self.pixels),
            self.width,
            self.height,
        )
    }

    fn fit(
        &mut self,
        handle: &raster::Handle,
        width: u32,
        height: u32,
    ) -> Option<tiny_skia::PixmapRef<'_>> {
        let frame = self.frame;
        let size = |s: &tiny_skia::Pixmap| (s.width(), s.height());

        if let Some(at) = self
            .scaled
            .iter()
            .position(|s| size(&s.0) == (width, height))
        {
            self.scaled[at].1 = frame;

            return Some(self.scaled[at].0.as_ref());
        }

        let resize = |from: tiny_skia::PixmapRef<'_>, w: u32, h: u32| {
            let mut to = tiny_skia::Pixmap::new(w, h)?;

            to.draw_pixmap(
                0,
                0,
                from,
                &tiny_skia::PixmapPaint {
                    quality: tiny_skia::FilterQuality::Bilinear,
                    ..Default::default()
                },
                tiny_skia::Transform::from_scale(
                    w as f32 / from.width() as f32,
                    h as f32 / from.height() as f32,
                ),
                None,
            );

            Some(to)
        };

        // Halve the image while the half is not smaller than the target,
        // so the last bilinear step reduces by two times at most.
        let image = self.image(handle)?;
        let mut half: Option<tiny_skia::Pixmap> = None;

        loop {
            let from = half.as_ref().map_or(image, |h| h.as_ref());
            let (w, h) = (from.width() / 2, from.height() / 2);

            if w < width || h < height {
                break;
            }

            half = Some(resize(from, w, h)?);
        }

        let from = half.as_ref().map_or(image, |h| h.as_ref());
        let scaled = resize(from, width, height)?;

        self.scaled.truncate(2);
        self.scaled.insert(0, (scaled, frame));

        Some(self.scaled[0].0.as_ref())
    }
}
