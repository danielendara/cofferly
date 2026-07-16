use eframe::egui::{self, vec2, Color32, Stroke};

// Warm neutrals echo the lock-screen artwork; teal keeps the product calm and
// trustworthy. Every foreground/background pair here meets WCAG AA for normal
// text, so meaning never depends on color alone.
pub const ACCENT: Color32 = Color32::from_rgb(31, 122, 112);
pub const ACCENT_DARK: Color32 = Color32::from_rgb(21, 92, 85);
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(228, 243, 239);
pub const GOLD: Color32 = Color32::from_rgb(224, 157, 55);
pub const GOLD_DARK: Color32 = Color32::from_rgb(176, 105, 18);
pub const GOLD_LIGHT: Color32 = Color32::from_rgb(253, 241, 214);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(34, 51, 59);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(83, 101, 109);
pub const LOCK_TEXT_SECONDARY: Color32 = Color32::from_rgb(64, 80, 88);
pub const POSITIVE: Color32 = Color32::from_rgb(24, 121, 78);
pub const NEGATIVE: Color32 = Color32::from_rgb(174, 55, 63);
/// Soft green wash for success status chips (meets WCAG AA with POSITIVE/ACCENT_DARK text).
pub const SUCCESS_LIGHT: Color32 = Color32::from_rgb(228, 243, 239);
/// Soft red wash for error status chips (meets WCAG AA with NEGATIVE text).
pub const ERROR_LIGHT: Color32 = Color32::from_rgb(252, 232, 233);
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
    // Cofferly uses a deliberately light, warm palette. Pinning the egui theme
    // prevents unconfigured system-dark defaults from leaking into light cards.
    ctx.set_theme(egui::Theme::Light);
    let mut style = (*ctx.global_style()).clone();

    style.spacing.item_spacing = vec2(12.0, 10.0);
    style.spacing.button_padding = vec2(14.0, 9.0);
    style.spacing.window_margin = egui::Margin::same(18);
    style.spacing.interact_size = vec2(40.0, 36.0);

    // Clean visuals
    style.visuals.widgets.active.bg_fill = ACCENT;
    style.visuals.widgets.active.weak_bg_fill = ACCENT;
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);

    style.visuals.widgets.inactive.bg_fill = CARD_BG;
    style.visuals.widgets.inactive.weak_bg_fill = CARD_BG;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.hovered.bg_fill = ACCENT_LIGHT;
    style.visuals.widgets.hovered.weak_bg_fill = ACCENT_LIGHT;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, ACCENT_DARK);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.open.bg_fill = ACCENT_LIGHT;
    style.visuals.widgets.open.weak_bg_fill = ACCENT_LIGHT;
    style.visuals.widgets.open.bg_stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.open.fg_stroke = Stroke::new(1.0, ACCENT_DARK);
    style.visuals.widgets.open.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.noninteractive.bg_fill = CARD_BG;
    style.visuals.widgets.noninteractive.weak_bg_fill = FAINT_BG;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    style.visuals.extreme_bg_color = Color32::WHITE;
    // Explicit RichText colors (such as white labels on primary actions) must
    // remain authoritative across light and dark operating-system themes.
    style.visuals.override_text_color = None;

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

    fn contrast_ratio(foreground: Color32, background: Color32) -> f32 {
        fn luminance(color: Color32) -> f32 {
            fn linear_channel(channel: u8) -> f32 {
                let channel = f32::from(channel) / 255.0;
                if channel <= 0.04045 {
                    channel / 12.92
                } else {
                    ((channel + 0.055) / 1.055).powf(2.4)
                }
            }

            0.2126 * linear_channel(color.r())
                + 0.7152 * linear_channel(color.g())
                + 0.0722 * linear_channel(color.b())
        }

        let foreground = luminance(foreground);
        let background = luminance(background);
        (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05)
    }

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

    #[test]
    fn lock_screen_colors_keep_small_text_and_controls_high_contrast() {
        // Small helper text and the primary action target AAA contrast. The
        // coin outline meets the 3:1 non-text UI-component requirement.
        assert!(contrast_ratio(LOCK_TEXT_SECONDARY, CARD_BG) >= 7.0);
        assert!(contrast_ratio(LOCK_TEXT_SECONDARY, APP_BG) >= 7.0);
        assert!(contrast_ratio(Color32::WHITE, ACCENT_DARK) >= 7.0);
        assert!(contrast_ratio(GOLD_DARK, CARD_BG) >= 3.0);
    }

    #[test]
    fn parent_screen_widget_states_do_not_inherit_dark_system_colors() {
        let ctx = egui::Context::default();
        ctx.set_theme(egui::Theme::Dark);
        configure_style(&ctx);
        let style = ctx.global_style();

        assert_eq!(ctx.theme(), egui::Theme::Light);
        assert_eq!(style.visuals.override_text_color, None);
        assert_eq!(style.visuals.widgets.inactive.weak_bg_fill, CARD_BG);
        assert_eq!(style.visuals.widgets.hovered.weak_bg_fill, ACCENT_LIGHT);
        assert_eq!(style.visuals.widgets.active.weak_bg_fill, ACCENT);
        assert_eq!(style.visuals.selection.stroke.color, Color32::WHITE);
    }
}
