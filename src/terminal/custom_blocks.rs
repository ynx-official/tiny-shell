use gpui::{Bounds, Hsla, Path, Pixels, Window, fill, point, px, size};

pub(crate) fn is_custom_block_supported(c: char) -> bool {
    match c as u32 {
        0x2580..=0x258F | 0x2590 | 0x2594..=0x259F => true, // Block Elements
        0x2500 | 0x2502 | 0x250C | 0x2510 | 0x2514 | 0x2518 | 0x251C | 0x2524 | 0x252C | 0x2534
        | 0x253C => true, // Light lines
        0x2501 | 0x2503 | 0x250F | 0x2513 | 0x2517 | 0x251B | 0x2523 | 0x252B | 0x2533 | 0x253B
        | 0x254B => true, // Heavy lines
        0xE0B0..=0xE0B6 => true,                            // Powerline
        _ => false,
    }
}

pub(crate) fn paint_custom_block(
    window: &mut Window,
    c: char,
    raw_x: f32,
    raw_y: f32,
    raw_w: f32,
    raw_h: f32,
    color: Hsla,
) -> bool {
    let scale = window.scale_factor();

    let x = px((raw_x * scale).round() / scale);
    let y = px((raw_y * scale).round() / scale);
    let w = px(((raw_x + raw_w) * scale).round() / scale) - x;
    let h = px(((raw_y + raw_h) * scale).round() / scale) - y;

    let mut painted = false;

    // Helper to paint a quad snapping its true physical boundaries to integer physical pixels
    let mut paint_quad = |qx: Pixels, qy: Pixels, qw: Pixels, qh: Pixels| {
        let l = px((qx.as_f32() * scale).round() / scale);
        let t = px((qy.as_f32() * scale).round() / scale);
        let r = px(((qx + qw).as_f32() * scale).round() / scale);
        let b = px(((qy + qh).as_f32() * scale).round() / scale);
        window.paint_quad(fill(Bounds::new(point(l, t), size(r - l, b - t)), color));
        painted = true;
    };

    let px_1 = px(1.0);
    let px_2 = px(2.0);

    // Box drawing thickness
    let light = (w * 0.1).max(px_1);
    let heavy = (w * 0.2).max(px_2);
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;

    match c as u32 {
        // --- Block Elements (U+2580..=U+259F) ---
        0x2580 => paint_quad(x, y, w, h / 2.0),
        0x2581 => paint_quad(x, y + h * 7.0 / 8.0, w, h / 8.0),
        0x2582 => paint_quad(x, y + h * 3.0 / 4.0, w, h / 4.0),
        0x2583 => paint_quad(x, y + h * 5.0 / 8.0, w, h * 3.0 / 8.0),
        0x2584 => paint_quad(x, y + h / 2.0, w, h / 2.0),
        0x2585 => paint_quad(x, y + h * 3.0 / 8.0, w, h * 5.0 / 8.0),
        0x2586 => paint_quad(x, y + h / 4.0, w, h * 3.0 / 4.0),
        0x2587 => paint_quad(x, y + h / 8.0, w, h * 7.0 / 8.0),
        0x2588 => paint_quad(x, y, w, h),
        0x2589 => paint_quad(x, y, w * 7.0 / 8.0, h),
        0x258A => paint_quad(x, y, w * 3.0 / 4.0, h),
        0x258B => paint_quad(x, y, w * 5.0 / 8.0, h),
        0x258C => paint_quad(x, y, w / 2.0, h),
        0x258D => paint_quad(x, y, w * 3.0 / 8.0, h),
        0x258E => paint_quad(x, y, w / 4.0, h),
        0x258F => paint_quad(x, y, w / 8.0, h),
        0x2590 => paint_quad(x + w / 2.0, y, w / 2.0, h),
        0x2594 => paint_quad(x, y, w, h / 8.0),
        0x2595 => paint_quad(x + w * 7.0 / 8.0, y, w / 8.0, h),
        0x2596 => paint_quad(x, y + h / 2.0, w / 2.0, h / 2.0),
        0x2597 => paint_quad(x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0),
        0x2598 => paint_quad(x, y, w / 2.0, h / 2.0),
        0x2599 => {
            paint_quad(x, y, w / 2.0, h);
            paint_quad(x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        }
        0x259A => {
            paint_quad(x, y, w / 2.0, h / 2.0);
            paint_quad(x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        }
        0x259B => {
            paint_quad(x, y, w, h / 2.0);
            paint_quad(x, y + h / 2.0, w / 2.0, h / 2.0);
        }
        0x259C => {
            paint_quad(x, y, w, h / 2.0);
            paint_quad(x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        }
        0x259D => paint_quad(x + w / 2.0, y, w / 2.0, h / 2.0),
        0x259E => {
            paint_quad(x + w / 2.0, y, w / 2.0, h / 2.0);
            paint_quad(x, y + h / 2.0, w / 2.0, h / 2.0);
        }
        0x259F => {
            paint_quad(x + w / 2.0, y, w / 2.0, h);
            paint_quad(x, y + h / 2.0, w / 2.0, h / 2.0);
        }

        // --- Basic Box Drawing (U+2500..=U+257F) ---
        // Light lines
        0x2500 => paint_quad(x, cy - light / 2.0, w, light), // Horizontal
        0x2502 => paint_quad(cx - light / 2.0, y, light, h), // Vertical
        0x250C => {
            // Down + Right
            paint_quad(
                cx - light / 2.0,
                cy - light / 2.0,
                light,
                h / 2.0 + light / 2.0,
            );
            paint_quad(
                cx - light / 2.0,
                cy - light / 2.0,
                w / 2.0 + light / 2.0,
                light,
            );
        }
        0x2510 => {
            // Down + Left
            paint_quad(
                cx - light / 2.0,
                cy - light / 2.0,
                light,
                h / 2.0 + light / 2.0,
            );
            paint_quad(x, cy - light / 2.0, w / 2.0 + light / 2.0, light);
        }
        0x2514 => {
            // Up + Right
            paint_quad(cx - light / 2.0, y, light, h / 2.0 + light / 2.0);
            paint_quad(
                cx - light / 2.0,
                cy - light / 2.0,
                w / 2.0 + light / 2.0,
                light,
            );
        }
        0x2518 => {
            // Up + Left
            paint_quad(cx - light / 2.0, y, light, h / 2.0 + light / 2.0);
            paint_quad(x, cy - light / 2.0, w / 2.0 + light / 2.0, light);
        }
        0x251C => {
            // Vertical + Right
            paint_quad(cx - light / 2.0, y, light, h);
            paint_quad(
                cx - light / 2.0,
                cy - light / 2.0,
                w / 2.0 + light / 2.0,
                light,
            );
        }
        0x2524 => {
            // Vertical + Left
            paint_quad(cx - light / 2.0, y, light, h);
            paint_quad(x, cy - light / 2.0, w / 2.0 + light / 2.0, light);
        }
        0x252C => {
            // Horizontal + Down
            paint_quad(x, cy - light / 2.0, w, light);
            paint_quad(
                cx - light / 2.0,
                cy - light / 2.0,
                light,
                h / 2.0 + light / 2.0,
            );
        }
        0x2534 => {
            // Horizontal + Up
            paint_quad(x, cy - light / 2.0, w, light);
            paint_quad(cx - light / 2.0, y, light, h / 2.0 + light / 2.0);
        }
        0x253C => {
            // Vertical + Horizontal (Cross)
            paint_quad(x, cy - light / 2.0, w, light);
            paint_quad(cx - light / 2.0, y, light, h);
        }

        // Heavy lines
        0x2501 => paint_quad(x, cy - heavy / 2.0, w, heavy), // Heavy Horizontal
        0x2503 => paint_quad(cx - heavy / 2.0, y, heavy, h), // Heavy Vertical
        0x250F => {
            // Heavy Down + Right
            paint_quad(
                cx - heavy / 2.0,
                cy - heavy / 2.0,
                heavy,
                h / 2.0 + heavy / 2.0,
            );
            paint_quad(
                cx - heavy / 2.0,
                cy - heavy / 2.0,
                w / 2.0 + heavy / 2.0,
                heavy,
            );
        }
        0x2513 => {
            // Heavy Down + Left
            paint_quad(
                cx - heavy / 2.0,
                cy - heavy / 2.0,
                heavy,
                h / 2.0 + heavy / 2.0,
            );
            paint_quad(x, cy - heavy / 2.0, w / 2.0 + heavy / 2.0, heavy);
        }
        0x2517 => {
            // Heavy Up + Right
            paint_quad(cx - heavy / 2.0, y, heavy, h / 2.0 + heavy / 2.0);
            paint_quad(
                cx - heavy / 2.0,
                cy - heavy / 2.0,
                w / 2.0 + heavy / 2.0,
                heavy,
            );
        }
        0x251B => {
            // Heavy Up + Left
            paint_quad(cx - heavy / 2.0, y, heavy, h / 2.0 + heavy / 2.0);
            paint_quad(x, cy - heavy / 2.0, w / 2.0 + heavy / 2.0, heavy);
        }
        0x2523 => {
            // Heavy Vertical + Right
            paint_quad(cx - heavy / 2.0, y, heavy, h);
            paint_quad(
                cx - heavy / 2.0,
                cy - heavy / 2.0,
                w / 2.0 + heavy / 2.0,
                heavy,
            );
        }
        0x252B => {
            // Heavy Vertical + Left
            paint_quad(cx - heavy / 2.0, y, heavy, h);
            paint_quad(x, cy - heavy / 2.0, w / 2.0 + heavy / 2.0, heavy);
        }
        0x2533 => {
            // Heavy Horizontal + Down
            paint_quad(x, cy - heavy / 2.0, w, heavy);
            paint_quad(
                cx - heavy / 2.0,
                cy - heavy / 2.0,
                heavy,
                h / 2.0 + heavy / 2.0,
            );
        }
        0x253B => {
            // Heavy Horizontal + Up
            paint_quad(x, cy - heavy / 2.0, w, heavy);
            paint_quad(cx - heavy / 2.0, y, heavy, h / 2.0 + heavy / 2.0);
        }
        0x254B => {
            // Heavy Vertical + Horizontal (Cross)
            paint_quad(x, cy - heavy / 2.0, w, heavy);
            paint_quad(cx - heavy / 2.0, y, heavy, h);
        }

        // --- Powerline ---
        0xE0B0 => {
            // Rightward Solid Arrow
            let mut path = Path::new(point(x, y));
            path.line_to(point(x + w, y + h / 2.0));
            path.line_to(point(x, y + h));
            path.line_to(point(x, y));
            window.paint_path(path, color);
            painted = true;
        }
        0xE0B2 => {
            // Leftward Solid Arrow
            let mut path = Path::new(point(x + w, y));
            path.line_to(point(x, y + h / 2.0));
            path.line_to(point(x + w, y + h));
            path.line_to(point(x + w, y));
            window.paint_path(path, color);
            painted = true;
        }
        0xE0B1 => {
            // Rightward Line Arrow
            let t = px(1.0);
            let mut p = Path::new(point(x, y));
            p.line_to(point(x + w, y + h / 2.0));
            p.line_to(point(x + w - t, y + h / 2.0));
            p.line_to(point(x, y + t));
            p.line_to(point(x, y));
            window.paint_path(p, color);
            let mut p2 = Path::new(point(x, y + h));
            p2.line_to(point(x + w, y + h / 2.0));
            p2.line_to(point(x + w - t, y + h / 2.0));
            p2.line_to(point(x, y + h - t));
            p2.line_to(point(x, y + h));
            window.paint_path(p2, color);
            painted = true;
        }
        0xE0B3 => {
            // Leftward Line Arrow
            let t = px(1.0);
            let mut p = Path::new(point(x + w, y));
            p.line_to(point(x, y + h / 2.0));
            p.line_to(point(x + t, y + h / 2.0));
            p.line_to(point(x + w, y + t));
            p.line_to(point(x + w, y));
            window.paint_path(p, color);
            let mut p2 = Path::new(point(x + w, y + h));
            p2.line_to(point(x, y + h / 2.0));
            p2.line_to(point(x + t, y + h / 2.0));
            p2.line_to(point(x + w, y + h - t));
            p2.line_to(point(x + w, y + h));
            window.paint_path(p2, color);
            painted = true;
        }

        // Half arches (Powerline rounded)
        0xE0B4 => {
            // Rightward Solid Semicircle
            let mut path = Path::new(point(x, y));
            path.curve_to(point(x, y + h), point(x + w * 2.0, y + h / 2.0));
            path.line_to(point(x, y));
            window.paint_path(path, color);
            painted = true;
        }
        0xE0B6 => {
            // Leftward Solid Semicircle
            let mut path = Path::new(point(x + w, y));
            path.curve_to(point(x + w, y + h), point(x - w, y + h / 2.0));
            path.line_to(point(x + w, y));
            window.paint_path(path, color);
            painted = true;
        }

        _ => {}
    }

    painted
}
