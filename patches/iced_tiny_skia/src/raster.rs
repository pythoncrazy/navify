use crate::core::image as raster;
use crate::core::{Rectangle, Size};
use crate::graphics;

use rustc_hash::{FxHashMap, FxHashSet};
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
        // then copied to whole pixels. The cache keeps only that copy.
        let width = (bounds.width * transform.sx).round();
        let height = (bounds.height * transform.sy).round();

        if !transform.has_skew()
            && filter_method == raster::FilterMethod::Linear
            && width >= 1.0
            && height >= 1.0
            && let Some(scaled) = entry.fit(handle, width as u32, height as u32)
        {
            let x = (bounds.x * transform.sx + transform.tx).round() as i32;
            let y = (bounds.y * transform.sy + transform.ty).round() as i32;

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

#[derive(Debug, Default)]
struct Cache {
    entries: FxHashMap<raster::Id, Option<Entry>>,
    hits: FxHashSet<raster::Id>,
}

impl Cache {
    pub fn allocate(
        &mut self,
        handle: &raster::Handle,
    ) -> Result<&mut Entry, raster::Error> {
        let id = handle.id();

        if let hash_map::Entry::Vacant(entry) = self.entries.entry(id) {
            match decode(handle) {
                Ok((width, height, pixels)) => {
                    let _ = entry.insert(Some(Entry {
                        width,
                        height,
                        pixels,
                        scaled: Vec::new(),
                        keep: false,
                    }));
                }
                Err(raster::Error::Empty) => return Err(raster::Error::Empty),
                Err(error) => {
                    let _ = entry.insert(None);

                    return Err(error);
                }
            }
        }

        let _ = self.hits.insert(id);

        self.entries
            .get_mut(&id)
            .and_then(Option::as_mut)
            .ok_or(raster::Error::Empty)
    }

    fn trim(&mut self) {
        self.entries.retain(|key, _| self.hits.contains(key));
        self.hits.clear();
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
    scaled: Vec<tiny_skia::Pixmap>,
    keep: bool,
}

impl Entry {
    fn image(
        &mut self,
        handle: &raster::Handle,
    ) -> Option<tiny_skia::PixmapRef<'_>> {
        if self.pixels.is_empty() {
            self.pixels = decode(handle).ok()?.2;
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
        let size = |s: &tiny_skia::Pixmap| (s.width(), s.height());

        if let Some(at) =
            self.scaled.iter().position(|s| size(s) == (width, height))
        {
            return Some(self.scaled[at].as_ref());
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

        // Halve the image while it is two times too large, so each
        // target pixel gets the mean of its source pixels.
        let image = self.image(handle)?;
        let mut half: Option<tiny_skia::Pixmap> = None;

        loop {
            let from = half.as_ref().map_or(image, |h| h.as_ref());
            let (w, h) = (from.width() / 2, from.height() / 2);

            if w < width * 2 || h < height * 2 {
                break;
            }

            half = Some(resize(from, w, h)?);
        }

        let from = half.as_ref().map_or(image, |h| h.as_ref());
        let scaled = resize(from, width, height)?;

        self.scaled.truncate(2);
        self.scaled.insert(0, scaled);

        if !self.keep {
            self.pixels = Vec::new();
        }

        Some(self.scaled[0].as_ref())
    }
}
