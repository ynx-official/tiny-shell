use std::{collections::HashMap, sync::Arc};

use anyhow::{Context as _, Result, bail};
use gpui::RenderImage;
use image::{Frame, RgbaImage};

use crate::backend::remote_desktop::{DecodedFrame, FrameSize};

pub(crate) struct RemoteDesktopSurface {
    pub(crate) sequence: u64,
    pub(crate) size: FrameSize,
    pub(crate) image: Arc<RenderImage>,
}

struct RenderedImageHistory {
    current: Arc<RenderImage>,
    previous: Option<Arc<RenderImage>>,
}

#[derive(Default)]
pub(crate) struct RemoteDesktopSurfaceCache {
    surfaces: HashMap<String, RemoteDesktopSurface>,
    rendered_images: HashMap<String, RenderedImageHistory>,
    retired_images: Vec<Arc<RenderImage>>,
}

impl RemoteDesktopSurfaceCache {
    pub(crate) fn update(&mut self, tab_id: String, frame: DecodedFrame) -> Result<()> {
        let surface = surface_from_frame(frame)?;
        self.surfaces.insert(tab_id, surface);
        Ok(())
    }

    pub(crate) fn get(&self, tab_id: &str) -> Option<&RemoteDesktopSurface> {
        self.surfaces.get(tab_id)
    }

    /// Records the image that is about to be handed to GPUI for this render.
    /// Keeping the two most recently rendered images mirrors GPUI's video path
    /// and avoids evicting a texture still referenced by the previous scene.
    pub(crate) fn mark_rendered(&mut self, tab_id: &str) {
        let Some(image) = self
            .surfaces
            .get(tab_id)
            .map(|surface| surface.image.clone())
        else {
            return;
        };
        let Some(history) = self.rendered_images.get_mut(tab_id) else {
            self.rendered_images.insert(
                tab_id.to_string(),
                RenderedImageHistory {
                    current: image,
                    previous: None,
                },
            );
            return;
        };
        if history.current.id == image.id {
            return;
        }
        let previous = std::mem::replace(&mut history.current, image);
        if let Some(retired) = history.previous.replace(previous) {
            self.retired_images.push(retired);
        }
    }

    pub(crate) fn remove(&mut self, tab_id: &str) {
        self.surfaces.remove(tab_id);
        if let Some(history) = self.rendered_images.remove(tab_id) {
            self.retired_images.push(history.current);
            if let Some(previous) = history.previous {
                self.retired_images.push(previous);
            }
        }
    }

    /// Returns textures that are no longer referenced by either the current or
    /// immediately previous scene. GPUI's sprite atlas requires explicit
    /// eviction even after the corresponding `Arc<RenderImage>` is dropped.
    pub(crate) fn take_retired_images(&mut self) -> Vec<Arc<RenderImage>> {
        std::mem::take(&mut self.retired_images)
    }
}

fn surface_from_frame(frame: DecodedFrame) -> Result<RemoteDesktopSurface> {
    let row_bytes = (frame.size.width as usize)
        .checked_mul(4)
        .context("RDP frame row size overflowed")?;
    let stride = frame.size.stride as usize;
    let total_bytes = stride
        .checked_mul(frame.size.height as usize)
        .context("RDP frame buffer size overflowed")?;
    if frame.pixels.len() < total_bytes {
        bail!(
            "RDP frame buffer is incomplete: expected {total_bytes}, got {}",
            frame.pixels.len()
        );
    }

    // GPUI RenderImage consumes BGRA bytes. FreeRDP's fourth byte is padding in
    // several decoded desktop paths, while GPUI treats it as alpha. Normalize
    // the inherently opaque desktop image so valid frames cannot disappear.
    // Strip native row padding while retaining the B/G/R channel order.
    let packed_bytes = row_bytes
        .checked_mul(frame.size.height as usize)
        .context("RDP packed frame size overflowed")?;
    let mut pixels = if stride == row_bytes && frame.pixels.len() == packed_bytes {
        frame.pixels
    } else {
        let mut pixels = Vec::with_capacity(packed_bytes);
        for row in frame.pixels[..total_bytes].chunks_exact(stride) {
            pixels.extend_from_slice(&row[..row_bytes]);
        }
        pixels
    };
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = u8::MAX;
    }
    let buffer = RgbaImage::from_raw(frame.size.width, frame.size.height, pixels)
        .context("RDP frame dimensions do not match its pixel buffer")?;
    let image = Arc::new(RenderImage::new(vec![Frame::new(buffer)]));
    Ok(RemoteDesktopSurface {
        sequence: frame.sequence,
        size: frame.size,
        image,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_conversion_removes_native_row_padding() {
        let size = FrameSize::new(2, 2, 12).unwrap();
        let first_row = [1, 2, 3, 4, 5, 6, 7, 8];
        let second_row = [9, 10, 11, 12, 13, 14, 15, 16];
        let mut pixels = Vec::new();
        pixels.extend_from_slice(&first_row);
        pixels.extend_from_slice(&[99; 4]);
        pixels.extend_from_slice(&second_row);
        pixels.extend_from_slice(&[88; 4]);

        let surface = surface_from_frame(DecodedFrame::new(7, size, pixels).unwrap()).unwrap();

        assert_eq!(surface.sequence, 7);
        assert_eq!(surface.size, size);
        assert_eq!(
            surface.image.as_bytes(0),
            Some(
                [
                    [1, 2, 3, 255, 5, 6, 7, 255],
                    [9, 10, 11, 255, 13, 14, 15, 255],
                ]
                .concat()
                .as_slice()
            )
        );
    }

    #[test]
    fn surface_conversion_makes_tightly_packed_desktop_pixels_opaque() {
        let size = FrameSize::new(2, 1, 8).unwrap();
        let pixels = vec![10, 20, 30, 0, 40, 50, 60, 17];

        let surface = surface_from_frame(DecodedFrame::new(3, size, pixels).unwrap()).unwrap();

        assert_eq!(
            surface.image.as_bytes(0),
            Some([10, 20, 30, 255, 40, 50, 60, 255].as_slice())
        );
    }

    #[test]
    fn surface_cache_keeps_only_current_and_previous_gpu_images() {
        let mut cache = RemoteDesktopSurfaceCache::default();
        let size = FrameSize::new(1, 1, 4).unwrap();

        cache
            .update(
                "rdp-tab".to_string(),
                DecodedFrame::new(1, size, vec![1, 2, 3, 0]).unwrap(),
            )
            .unwrap();
        let first_id = cache.get("rdp-tab").unwrap().image.id;
        cache.mark_rendered("rdp-tab");

        // Frames that are replaced before GPUI paints them never enter the
        // sprite atlas and therefore must not advance the rendered history.
        let mut unrendered_id = None;
        for sequence in 2..=3 {
            cache
                .update(
                    "rdp-tab".to_string(),
                    DecodedFrame::new(sequence, size, vec![1, 2, 3, 0]).unwrap(),
                )
                .unwrap();
            if sequence == 2 {
                unrendered_id = cache.get("rdp-tab").map(|surface| surface.image.id);
            }
        }
        assert!(cache.take_retired_images().is_empty());

        cache.mark_rendered("rdp-tab");
        assert!(cache.take_retired_images().is_empty());

        cache
            .update(
                "rdp-tab".to_string(),
                DecodedFrame::new(4, size, vec![1, 2, 3, 0]).unwrap(),
            )
            .unwrap();
        cache.mark_rendered("rdp-tab");

        let retired = cache.take_retired_images();
        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0].id, first_id);
        assert_ne!(Some(retired[0].id), unrendered_id);
        assert_eq!(
            cache.get("rdp-tab").map(|surface| surface.sequence),
            Some(4)
        );

        cache.remove("rdp-tab");
        let removed = cache.take_retired_images();
        assert_eq!(removed.len(), 2);
        assert_ne!(removed[0].id, removed[1].id);
        assert!(removed.iter().all(|image| Some(image.id) != unrendered_id));
        assert!(cache.get("rdp-tab").is_none());
    }
}
