use image::{ImageFormat, RgbaImage};
use std::collections::BTreeMap;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const WINDOW_ICON: &[u8] = include_bytes!("../assets/icons/tiny-shell.png");
const WINDOWS_ICON: &[u8] = include_bytes!("../assets/icons/tiny-shell.ico");
const MACOS_ICON: &[u8] = include_bytes!("../assets/icons/tiny-shell.icns");

fn read_u32_be(bytes: &[u8]) -> u32 {
    assert!(bytes.len() >= 4);
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn visible_bounds(image: &RgbaImage) -> (u32, u32) {
    let mut left = image.width();
    let mut top = image.height();
    let mut right = 0;
    let mut bottom = 0;
    let mut visible = false;
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[3] >= 128 {
            visible = true;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    assert!(visible, "application icon must have visible artwork");
    (right - left + 1, bottom - top + 1)
}

fn assert_windows_occupancy(image: &RgbaImage) {
    assert_eq!(image.width(), image.height());
    let (width, height) = visible_bounds(image);
    assert!(
        width * 8 >= image.width() * 7,
        "{}px icon has only {width}px visible width; excessive padding makes the icon too small",
        image.width()
    );
    assert!(
        height * 100 >= image.height() * 92,
        "{}px icon has only {height}px visible height",
        image.height()
    );
    for (x, y) in [
        (0, 0),
        (image.width() - 1, 0),
        (0, image.height() - 1),
        (image.width() - 1, image.height() - 1),
    ] {
        assert_eq!(image.get_pixel(x, y)[3], 0, "corners must be transparent");
    }
}

#[test]
fn runtime_window_icon_uses_the_available_canvas() -> TestResult {
    let image = image::load_from_memory_with_format(WINDOW_ICON, ImageFormat::Png)?.into_rgba8();
    assert_eq!(image.dimensions(), (1024, 1024));
    assert_windows_occupancy(&image);
    Ok(())
}

#[test]
fn every_windows_ico_size_uses_the_available_canvas() -> TestResult {
    assert_eq!(&WINDOWS_ICON[..4], &[0, 0, 1, 0]);
    let count = u16::from_le_bytes([WINDOWS_ICON[4], WINDOWS_ICON[5]]) as usize;
    let mut sizes = Vec::new();
    assert!(6 + count * 16 <= WINDOWS_ICON.len());
    for index in 0..count {
        let entry = &WINDOWS_ICON[6 + index * 16..6 + (index + 1) * 16];
        let size = if entry[0] == 0 { 256 } else { entry[0] as u32 };
        assert_eq!(entry[0], entry[1]);
        let length = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
        let offset = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
        assert!(offset >= 6 + count * 16);
        assert!(offset + length <= WINDOWS_ICON.len());
        let image = image::load_from_memory_with_format(
            &WINDOWS_ICON[offset..offset + length],
            ImageFormat::Png,
        )?
        .into_rgba8();
        assert_eq!(image.dimensions(), (size, size));
        assert_windows_occupancy(&image);
        sizes.push(size);
    }
    assert_eq!(sizes, [16, 20, 24, 32, 40, 48, 64, 128, 256]);
    Ok(())
}

fn icns_entries() -> BTreeMap<[u8; 4], &'static [u8]> {
    assert_eq!(&MACOS_ICON[..4], b"icns");
    assert_eq!(read_u32_be(&MACOS_ICON[4..8]) as usize, MACOS_ICON.len());
    let mut entries = BTreeMap::new();
    let mut offset = 8;
    while offset < MACOS_ICON.len() {
        assert!(offset + 8 <= MACOS_ICON.len());
        let tag = [
            MACOS_ICON[offset],
            MACOS_ICON[offset + 1],
            MACOS_ICON[offset + 2],
            MACOS_ICON[offset + 3],
        ];
        let length = read_u32_be(&MACOS_ICON[offset + 4..offset + 8]) as usize;
        assert!(length >= 8 && offset + length <= MACOS_ICON.len());
        assert!(
            entries
                .insert(tag, &MACOS_ICON[offset + 8..offset + length])
                .is_none()
        );
        offset += length;
    }
    entries
}

fn decode_argb(payload: &[u8], size: usize) -> RgbaImage {
    assert!(
        payload.starts_with(b"ARGB"),
        "16/32px ICNS entries must use native ARGB encoding"
    );
    let mut offset = 4;
    let mut channels = Vec::new();
    for _ in 0..4 {
        let mut channel = Vec::new();
        while channel.len() < size * size {
            assert!(offset < payload.len());
            let control = payload[offset];
            offset += 1;
            if control < 128 {
                let count = usize::from(control) + 1;
                assert!(offset + count <= payload.len());
                channel.extend_from_slice(&payload[offset..offset + count]);
                offset += count;
            } else {
                let count = usize::from(control) - 125;
                assert!(offset < payload.len());
                channel.extend(std::iter::repeat_n(payload[offset], count));
                offset += 1;
            }
            assert!(channel.len() <= size * size);
        }
        channels.push(channel);
    }
    assert_eq!(offset, payload.len(), "unexpected trailing ARGB bytes");
    RgbaImage::from_fn(size as u32, size as u32, |x, y| {
        let index = y as usize * size + x as usize;
        image::Rgba([
            channels[1][index],
            channels[2][index],
            channels[3][index],
            channels[0][index],
        ])
    })
}

#[test]
fn macos_small_sizes_use_native_argb_encoding() -> TestResult {
    let entries = icns_entries();
    for (tag, size) in [(b"ic04", 16), (b"ic05", 32)] {
        let image = decode_argb(entries[tag], size);
        assert!(image.pixels().all(|pixel| pixel[3] == 255));
        if size == 32 {
            let retina_png =
                image::load_from_memory_with_format(entries[b"ic11"], ImageFormat::Png)?
                    .into_rgba8();
            assert_eq!(
                image, retina_png,
                "ARGB must preserve the PNG color channels"
            );
        }
    }
    Ok(())
}

#[test]
fn macos_canvas_is_full_bleed_for_system_masking() -> TestResult {
    let entries = icns_entries();
    for (tag, size) in [
        (b"ic11", 32),
        (b"ic12", 64),
        (b"ic07", 128),
        (b"ic13", 256),
        (b"ic08", 256),
        (b"ic14", 512),
        (b"ic09", 512),
        (b"ic10", 1024),
    ] {
        let image =
            image::load_from_memory_with_format(entries[tag], ImageFormat::Png)?.into_rgba8();
        assert_eq!(image.dimensions(), (size, size));
        assert!(
            image.pixels().all(|pixel| pixel[3] == 255),
            "macOS background must fill the canvas without a pre-masked inset tile"
        );
        for (x, y) in [(0, 0), (size - 1, 0), (0, size - 1), (size - 1, size - 1)] {
            let pixel = image.get_pixel(x, y);
            assert!(
                pixel[0] < 70 && pixel[1] < 70 && pixel[2] < 90,
                "macOS corners must be navy, not a baked checkerboard"
            );
        }
    }
    Ok(())
}
