use eframe::egui::{self, vec2, Color32, Stroke};

// Warm neutrals echo the lock-screen artwork; teal keeps the product calm and
// trustworthy. Every foreground/background pair here meets WCAG AA for normal
// text, so meaning never depends on color alone.
pub const ACCENT: Color32 = Color32::from_rgb(31, 122, 112);
pub const ACCENT_DARK: Color32 = Color32::from_rgb(21, 92, 85);
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(228, 243, 239);
pub const GOLD: Color32 = Color32::from_rgb(224, 157, 55);
pub const GOLD_LIGHT: Color32 = Color32::from_rgb(253, 241, 214);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(34, 51, 59);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(83, 101, 109);
pub const POSITIVE: Color32 = Color32::from_rgb(24, 121, 78);
pub const NEGATIVE: Color32 = Color32::from_rgb(174, 55, 63);
pub const CARD_BG: Color32 = Color32::WHITE;
pub const BORDER: Color32 = Color32::from_rgb(216, 222, 220);
pub const FAINT_BG: Color32 = Color32::from_rgb(246, 248, 247);
pub const APP_BG: Color32 = Color32::from_rgb(250, 248, 244);

pub fn balance_color(cents: i64) -> Color32 {
    cents_color(cents)
}

pub fn amount_color(cents: i64) -> Color32 {
    cents_color(cents)
}

fn cents_color(cents: i64) -> Color32 {
    if cents < 0 {
        NEGATIVE
    } else if cents > 0 {
        POSITIVE
    } else {
        TEXT_PRIMARY
    }
}

pub fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();

    style.spacing.item_spacing = vec2(12.0, 10.0);
    style.spacing.button_padding = vec2(14.0, 9.0);
    style.spacing.window_margin = egui::Margin::same(18);
    style.spacing.interact_size = vec2(40.0, 36.0);

    // Clean visuals
    style.visuals.widgets.active.bg_fill = ACCENT;
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);

    style.visuals.widgets.inactive.bg_fill = CARD_BG;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.hovered.bg_fill = ACCENT_LIGHT;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, ACCENT_DARK);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
    style.visuals.extreme_bg_color = Color32::WHITE;
    style.visuals.override_text_color = Some(TEXT_PRIMARY);

    style.visuals.panel_fill = APP_BG;
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

    #[test]
    fn money_colors_distinguish_positive_negative_and_zero() {
        assert_eq!(amount_color(1), POSITIVE);
        assert_eq!(amount_color(-1), NEGATIVE);
        assert_eq!(amount_color(0), TEXT_PRIMARY);
        assert_eq!(balance_color(0), TEXT_PRIMARY);
    }
}
