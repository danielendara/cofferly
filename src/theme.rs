use eframe::egui::{self, vec2, Color32, Stroke};

pub const ACCENT: Color32 = Color32::from_rgb(42, 157, 143); // Teal - modern, trustworthy
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(230, 245, 243); // Very light teal for subtle fills
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(33, 37, 41);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(100, 108, 117);
pub const POSITIVE: Color32 = Color32::from_rgb(40, 167, 69);
pub const NEGATIVE: Color32 = Color32::from_rgb(220, 53, 69);
pub const CARD_BG: Color32 = Color32::from_rgb(250, 250, 251); // Subtle off-white card background for separation
pub const BORDER: Color32 = Color32::from_rgb(222, 226, 230); // Visible but soft border
pub const FAINT_BG: Color32 = Color32::from_rgb(246, 247, 249); // Subtle stripe / alt row bg

pub fn balance_color(cents: i64) -> Color32 {
    cents_color(cents)
}

pub fn amount_color(cents: i64) -> Color32 {
    cents_color(cents)
}

fn cents_color(cents: i64) -> Color32 {
    if cents < 0 {
        NEGATIVE
    } else {
        POSITIVE
    }
}

pub fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();

    // Modern, clean spacing
    style.spacing.item_spacing = vec2(12.0, 8.0);
    style.spacing.button_padding = vec2(12.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(12);

    // Clean visuals
    style.visuals.widgets.active.bg_fill = ACCENT;
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);

    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(240, 240, 240);
    style.visuals.widgets.hovered.bg_fill = ACCENT_LIGHT;

    // Subtle panel / window look - lower contrast
    style.visuals.panel_fill = Color32::WHITE;
    style.visuals.window_fill = Color32::WHITE;
    style.visuals.window_stroke = Stroke::new(1.0, BORDER);
    style.visuals.faint_bg_color = FAINT_BG;

    ctx.set_global_style(style);
}

pub fn app_icon() -> egui::IconData {
    const SIZE: u32 = 64;
    let mut rgba = vec![0_u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let index = ((y * SIZE + x) * 4) as usize;
            let inside = rounded_rect(x, y, 5, 5, 54, 54, 12);

            if inside {
                rgba[index] = 31;
                rgba[index + 1] = 126;
                rgba[index + 2] = 108;
                rgba[index + 3] = 255;
            }

            if rounded_rect(x, y, 14, 18, 44, 30, 7) {
                rgba[index] = 246;
                rgba[index + 1] = 250;
                rgba[index + 2] = 248;
                rgba[index + 3] = 255;
            }

            if rounded_rect(x, y, 16, 25, 40, 24, 6) {
                rgba[index] = 227;
                rgba[index + 1] = 241;
                rgba[index + 2] = 236;
                rgba[index + 3] = 255;
            }

            if rounded_rect(x, y, 42, 30, 8, 8, 4) {
                rgba[index] = 31;
                rgba[index + 1] = 126;
                rgba[index + 2] = 108;
                rgba[index + 3] = 255;
            }

            if (20..=44).contains(&x) && (12..=16).contains(&y) {
                rgba[index] = 255;
                rgba[index + 1] = 214;
                rgba[index + 2] = 102;
                rgba[index + 3] = 255;
            }
        }
    }

    egui::IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    }
}

fn rounded_rect(x: u32, y: u32, left: u32, top: u32, width: u32, height: u32, radius: u32) -> bool {
    if x < left || y < top || x >= left + width || y >= top + height {
        return false;
    }

    let right = left + width - 1;
    let bottom = top + height - 1;
    let cx = if x < left + radius {
        left + radius
    } else if x > right - radius {
        right - radius
    } else {
        x
    };
    let cy = if y < top + radius {
        top + radius
    } else if y > bottom - radius {
        bottom - radius
    } else {
        y
    };

    let dx = i64::from(x) - i64::from(cx);
    let dy = i64::from(y) - i64::from(cy);
    dx * dx + dy * dy <= i64::from(radius) * i64::from(radius)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_icon_is_valid_rgba_square() {
        let icon = app_icon();
        assert_eq!(icon.width, 64);
        assert_eq!(icon.height, 64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
    }
}
