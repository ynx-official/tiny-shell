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

#[derive(Default)]
pub(crate) struct RemoteDesktopSurfaceCache {
    surfaces: HashMap<String, RemoteDesktopSurface>,
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

    pub(crate) fn remove(&mut self, tab_id: &str) {
        self.surfaces.remove(tab_id);
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

    // GPUI RenderImage consumes BGRA bytes. Strip native row padding while
    // retaining channel order so the compositor can upload the frame directly.
    let packed_bytes = row_bytes
        .checked_mul(frame.size.height as usize)
        .context("RDP packed frame size overflowed")?;
    let mut pixels = Vec::with_capacity(packed_bytes);
    for row in frame.pixels[..total_bytes].chunks_exact(stride) {
        pixels.extend_from_slice(&row[..row_bytes]);
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
            Some([first_row, second_row].concat().as_slice())
        );
    }
}
