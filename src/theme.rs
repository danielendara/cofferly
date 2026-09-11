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
/// Left wallet picker width; cards stay 50px tall so 1280×800 still fits.
pub const WALLET_PICKER_WIDTH: f32 = 300.0;
pub const WALLET_CARD_HEIGHT: f32 = 50.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WalletCardChrome {
    pub fill: Color32,
    pub stroke: Stroke,
    pub focus_ring: Option<Stroke>,
}

/// Selected = inverted fill + thicker stroke. Focused = 3px ring that stays
/// visible on both white cards and the teal selected fill (egui's white
/// focus stroke vanishes on `CARD_BG`).
pub fn wallet_card_chrome(selected: bool, focused: bool) -> WalletCardChrome {
    let fill = if selected { ACCENT } else { CARD_BG };
    let stroke = if selected {
        Stroke::new(2.0, ACCENT_DARK)
    } else {
        Stroke::new(1.0, BORDER)
    };
    let focus_ring = if focused {
        Some(if selected {
            Stroke::new(3.0, Color32::WHITE)
        } else {
            Stroke::new(3.0, GOLD_DARK)
        })
    } else {
        None
    };
    WalletCardChrome {
        fill,
        stroke,
        focus_ring,
    }
}

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
    const APP_ICON_BYTES: &[u8] = include_bytes!("../assets/cofferly-app-icon.png");
    let rgba = image::load_from_memory(APP_ICON_BYTES)
        .expect("embedded Cofferly app icon should decode")
        .to_rgba8();
    let (width, height) = rgba.dimensions();

    egui::IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    }
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
        assert_eq!(icon.width, 512);
        assert_eq!(icon.height, 512);
        assert_eq!(icon.rgba.len(), 512 * 512 * 4);
        assert_eq!(icon.rgba[3], 0);
        assert_eq!(icon.rgba[(256 * 512 + 256) * 4 + 3], 255);
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

    #[test]
    fn selected_and_focused_wallet_cards_are_distinguishable() {
        let selected = wallet_card_chrome(true, false);
        let focused = wallet_card_chrome(false, true);
        let both = wallet_card_chrome(true, true);
        let idle = wallet_card_chrome(false, false);

        assert_eq!(selected.fill, ACCENT);
        assert_eq!(focused.fill, CARD_BG);
        assert!(selected.focus_ring.is_none());
        let focused_ring = focused.focus_ring.expect("focus has a ring");
        let both_ring = both.focus_ring.expect("selected+focused has a ring");
        assert_eq!(focused_ring.color, GOLD_DARK);
        assert_eq!(both_ring.color, Color32::WHITE);
        assert!(focused_ring.width > selected.stroke.width);
        assert!(selected.stroke.width > idle.stroke.width);
        assert_eq!(both.fill, selected.fill);
        assert_ne!(focused_ring.color, both_ring.color);
        assert!(contrast_ratio(GOLD_DARK, CARD_BG) >= 3.0);
        assert!(contrast_ratio(Color32::WHITE, ACCENT) >= 3.0);
    }

    #[test]
    fn wallet_card_metrics_fit_1280x800_sidebar() {
        assert_eq!(WALLET_PICKER_WIDTH, 300.0);
        assert_eq!(WALLET_CARD_HEIGHT, 50.0);
        let viewport_height = 800.0;
        let top_header = 56.0;
        let sidebar_margin = 22.0;
        let family_header = 48.0;
        let print_export = 29.0 * 4.0;
        let remaining =
            viewport_height - top_header - sidebar_margin - family_header - print_export;
        assert!(
            remaining > WALLET_CARD_HEIGHT * 3.0,
            "three 50px wallet cards must still fit a 1280×800 window"
        );
    }
}
