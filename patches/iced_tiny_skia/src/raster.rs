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
            raster::allocate(handle, Size::new(image.width(), image.height()))
        })
    }

    pub fn dimensions(&self, handle: &raster::Handle) -> Option<Size<u32>> {
        let mut cache = self.cache.borrow_mut();
        let image = cache.allocate(handle).ok()?;

        Some(Size::new(image.width(), image.height()))
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

        if cache.allocate(handle).is_err() {
            return;
        }

        let entry = cache
            .entries
            .get_mut(&handle.id())
            .and_then(Option::as_mut)
            .expect("Image should be allocated");

        let image = tiny_skia::PixmapRef::from_bytes(
            bytemuck::cast_slice(&entry.pixels),
            entry.width,
            entry.height,
        )
        .expect("Build pixmap from image bytes");

        // An upright, opaque image with no clip mask is scaled one time
        // and then copied to whole pixels.
        let width = (bounds.width * transform.sx).round();
        let height = (bounds.height * transform.sy).round();

        if !transform.has_skew()
            && clip_mask.is_none()
            && opacity >= 1.0
            && filter_method == raster::FilterMethod::Linear
            && width >= 1.0
            && height >= 1.0
        {
            let x = (bounds.x * transform.sx + transform.tx).round() as i32;
            let y = (bounds.y * transform.sy + transform.ty).round() as i32;
            let (width, height) = (width as u32, height as u32);

            if (width, height) == (entry.width, entry.height) {
                return crate::text::blit(pixels, x, y, image, 1.0);
            }

            let found = entry
                .scaled
                .iter()
                .position(|s| (s.width(), s.height()) == (width, height));

            let found = found.or_else(|| {
                // Halve the image while it is two times too large, so
                // each target pixel gets the mean of its source pixels.
                let resize =
                    |from: tiny_skia::PixmapRef<'_>, w: u32, h: u32| {
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

                entry.scaled.truncate(2);
                entry.scaled.insert(0, scaled);

                Some(0)
            });

            if let Some(scaled) = found.and_then(|i| entry.scaled.get(i)) {
                return crate::text::blit(pixels, x, y, scaled.as_ref(), 1.0);
            }
        }

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
    ) -> Result<tiny_skia::PixmapRef<'_>, raster::Error> {
        let id = handle.id();

        if let hash_map::Entry::Vacant(entry) = self.entries.entry(id) {
            let image = match graphics::image::load(handle) {
                Ok(image) => image,
                Err(error) => {
                    let _ = entry.insert(None);

                    return Err(error);
                }
            };

            if image.width() == 0 || image.height() == 0 {
                return Err(raster::Error::Empty);
            }

            let mut buffer =
                vec![0u32; image.width() as usize * image.height() as usize];

            for (i, pixel) in image.pixels().enumerate() {
                let [r, g, b, a] = pixel.0;

                buffer[i] = bytemuck::cast(
                    tiny_skia::ColorU8::from_rgba(b, g, r, a).premultiply(),
                );
            }

            let _ = entry.insert(Some(Entry {
                width: image.width(),
                height: image.height(),
                pixels: buffer,
                scaled: Vec::new(),
            }));
        }

        let _ = self.hits.insert(id);

        Ok(self
            .entries
            .get(&id)
            .unwrap()
            .as_ref()
            .map(|entry| {
                tiny_skia::PixmapRef::from_bytes(
                    bytemuck::cast_slice(&entry.pixels),
                    entry.width,
                    entry.height,
                )
                .expect("Build pixmap from image bytes")
            })
            .expect("Image should be allocated"))
    }

    fn trim(&mut self) {
        self.entries.retain(|key, _| self.hits.contains(key));
        self.hits.clear();
    }
}

#[derive(Debug)]
struct Entry {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
    scaled: Vec<tiny_skia::Pixmap>,
}
