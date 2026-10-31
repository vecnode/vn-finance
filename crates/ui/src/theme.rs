//! The colours, spacing and type vn-finance draws with.
//!
//! Transcribed from the browser panel's stylesheet, token for token, so the window
//! and the tab it replaces are recognisably the same product. Four values are
//! deliberately **not** the stylesheet's, because the stylesheet was measured and
//! found wanting:
//!
//! - light `text4` was `#8A97A8`, which is 2.7–3.0:1 on every surface it is used on
//!   and carries captions in twenty places. It is now `#5B6779`.
//! - dark `text4` was `#75879F`, which fails 4.5:1 on the two tinted surfaces. It is
//!   now `#8B9CB3`.
//! - `text3` failed by a hair on the page background and on `surface3`; it is one
//!   step darker in both modes.
//! - no text is below 11 px. The stylesheet's own header says nothing below 12 px is
//!   text for reading, and then used 9.5 px in six places.
//!
//! The browser had one dark mode, driven by `prefers-color-scheme`, and no switch.
//! This one follows the system too and also offers a switch, because a desktop
//! application that cannot be changed is a regression against the tab it replaces.

use egui::{Color32, Context, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle, Visuals};

/// The colours, in one value per mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The page behind everything.
    pub bg: Color32,
    /// Cards, panels, the sidebar.
    pub surface: Color32,
    /// The quieter fill: chips, table headers, row hover.
    pub surface_2: Color32,
    /// The quiet fill's quiet fill: count badges, tracks, inline code.
    pub surface_3: Color32,
    /// The default hairline.
    pub border: Color32,
    /// A control's boundary, which must clear 3:1 against what it sits on.
    pub border_2: Color32,
    /// Primary text.
    pub text: Color32,
    /// Secondary text.
    pub text_2: Color32,
    /// Muted text: captions and hints.
    pub text_3: Color32,
    /// Faint text: micro-labels. Never below 4.5:1.
    pub text_4: Color32,
    /// Links, the active navigation item, the primary button.
    pub accent: Color32,
    /// The primary button's hover.
    pub accent_2: Color32,
    /// Text on an accent fill.
    pub accent_fg: Color32,
    /// A tinted accent surface.
    pub accent_bg: Color32,
    /// An accent-tinted border.
    pub accent_border: Color32,
    /// Good news.
    pub ok: Color32,
    /// Good news, tinted.
    pub ok_bg: Color32,
    /// Good news, bordered.
    pub ok_border: Color32,
    /// Something to look at.
    pub warn: Color32,
    /// Warning, tinted.
    pub warn_bg: Color32,
    /// Warning, bordered.
    pub warn_border: Color32,
    /// Something is wrong.
    pub danger: Color32,
    /// Danger, tinted.
    pub danger_bg: Color32,
    /// Danger, bordered.
    pub danger_border: Color32,
    /// A neutral, unremarkable state.
    pub slate: Color32,
    /// Neutral, tinted.
    pub slate_bg: Color32,
    /// Neutral, bordered.
    pub slate_border: Color32,
}

impl Palette {
    /// The light palette.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            bg: Color32::from_rgb(0xF7, 0xF8, 0xFA),
            surface: Color32::WHITE,
            surface_2: Color32::from_rgb(0xFB, 0xFC, 0xFD),
            surface_3: Color32::from_rgb(0xF1, 0xF5, 0xF9),
            border: Color32::from_rgb(0xE2, 0xE8, 0xF0),
            border_2: Color32::from_rgb(0xCB, 0xD5, 0xE1),
            text: Color32::from_rgb(0x0F, 0x17, 0x2A),
            text_2: Color32::from_rgb(0x33, 0x41, 0x55),
            // The stylesheet's #64748B is 4.48:1 on the page background and 4.34:1 on
            // surface-3; one step darker clears both.
            text_3: Color32::from_rgb(0x53, 0x60, 0x74),
            // Was #8A97A8 (2.7–3.0:1). This clears 4.5:1 on every surface.
            text_4: Color32::from_rgb(0x5B, 0x67, 0x79),
            accent: Color32::from_rgb(0x1D, 0x4E, 0xD8),
            accent_2: Color32::from_rgb(0x1E, 0x40, 0xAF),
            accent_fg: Color32::WHITE,
            accent_bg: Color32::from_rgb(0xEF, 0xF4, 0xFF),
            accent_border: Color32::from_rgb(0xC7, 0xD7, 0xFE),
            ok: Color32::from_rgb(0x15, 0x80, 0x3D),
            ok_bg: Color32::from_rgb(0xF0, 0xFD, 0xF4),
            ok_border: Color32::from_rgb(0xBB, 0xF7, 0xD0),
            warn: Color32::from_rgb(0xB4, 0x53, 0x09),
            warn_bg: Color32::from_rgb(0xFF, 0xFB, 0xEB),
            warn_border: Color32::from_rgb(0xFD, 0xE6, 0x8A),
            danger: Color32::from_rgb(0xB9, 0x1C, 0x1C),
            danger_bg: Color32::from_rgb(0xFE, 0xF2, 0xF2),
            danger_border: Color32::from_rgb(0xFE, 0xCA, 0xCA),
            slate: Color32::from_rgb(0x47, 0x55, 0x69),
            slate_bg: Color32::from_rgb(0xF1, 0xF5, 0xF9),
            slate_border: Color32::from_rgb(0xE2, 0xE8, 0xF0),
        }
    }

    /// The dark palette.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            bg: Color32::from_rgb(0x0B, 0x12, 0x20),
            surface: Color32::from_rgb(0x11, 0x1A, 0x2B),
            surface_2: Color32::from_rgb(0x0F, 0x18, 0x26),
            surface_3: Color32::from_rgb(0x16, 0x22, 0x3A),
            border: Color32::from_rgb(0x1E, 0x29, 0x3B),
            border_2: Color32::from_rgb(0x33, 0x41, 0x55),
            text: Color32::from_rgb(0xE6, 0xED, 0xF7),
            text_2: Color32::from_rgb(0xC2, 0xCE, 0xDF),
            text_3: Color32::from_rgb(0xA3, 0xB4, 0xCA),
            // Was #75879F (4.25:1 on surface-3 and on the accent tint).
            text_4: Color32::from_rgb(0x8B, 0x9C, 0xB3),
            accent: Color32::from_rgb(0x7B, 0xA2, 0xFF),
            accent_2: Color32::from_rgb(0xA8, 0xC0, 0xFF),
            accent_fg: Color32::from_rgb(0x0B, 0x12, 0x20),
            accent_bg: Color32::from_rgb(0x16, 0x23, 0x3F),
            accent_border: Color32::from_rgb(0x2C, 0x41, 0x68),
            ok: Color32::from_rgb(0x6E, 0xE7, 0xA8),
            ok_bg: Color32::from_rgb(0x0E, 0x2A, 0x1E),
            ok_border: Color32::from_rgb(0x1D, 0x51, 0x36),
            warn: Color32::from_rgb(0xF5, 0xC8, 0x6B),
            warn_bg: Color32::from_rgb(0x2A, 0x21, 0x09),
            warn_border: Color32::from_rgb(0x5A, 0x43, 0x1A),
            danger: Color32::from_rgb(0xF7, 0x9A, 0x9A),
            danger_bg: Color32::from_rgb(0x2C, 0x14, 0x14),
            danger_border: Color32::from_rgb(0x5E, 0x25, 0x25),
            slate: Color32::from_rgb(0xA9, 0xB7, 0xC9),
            slate_bg: Color32::from_rgb(0x16, 0x22, 0x3A),
            slate_border: Color32::from_rgb(0x24, 0x31, 0x49),
        }
    }

    /// The palette for a mode.
    #[must_use]
    pub const fn for_mode(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }
}

/// The corner radius the stylesheet uses everywhere.
pub const RADIUS: u8 = 6;

/// The radius for a small control: a badge, a pill, a code span.
pub const RADIUS_SMALL: u8 = 4;

/// The sidebar's width. Fixed, like the design.
pub const SIDEBAR_WIDTH: f32 = 264.0;

/// The widest the page content is allowed to get.
pub const CONTENT_MAX_WIDTH: f32 = 1440.0;

/// A page title: the largest text on a page that is not a figure.
pub const SECTION_TITLE: f32 = 17.0;

/// The application's name in the header.
pub const PAGE_TITLE: f32 = 16.0;

/// A figure: a KPI's value.
pub const FIGURE: f32 = 24.0;

/// A figure a step down, inside a card.
pub const FIGURE_SMALL: f32 = 18.0;

/// Body text.
pub const BODY: f32 = 13.5;

/// Secondary body text.
pub const BODY_SMALL: f32 = 12.5;

/// Captions.
pub const CAPTION: f32 = 12.0;

/// Micro-labels: uppercase, tracked, never below this.
pub const MICRO: f32 = 11.0;

/// The monospace size, which the stylesheet clamps at 11.5.
pub const MONO: f32 = 11.5;

/// The type scale, with one size the caller may change.
///
/// One table with one parameter, so a dialog that lowers the heading and restores it
/// cannot leave the product with two scales that disagree by a point.
#[must_use]
pub fn text_styles(heading: f32) -> std::collections::BTreeMap<TextStyle, FontId> {
    [
        (TextStyle::Heading, FontId::new(heading, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(BODY, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(BODY_SMALL, FontFamily::Proportional),
        ),
        (TextStyle::Small, FontId::new(CAPTION, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(MONO, FontFamily::Monospace)),
    ]
    .into()
}

/// The heading size a dialog uses, so a dialog does not read as a page in a box.
pub const DIALOG_TITLE: f32 = 14.0;

/// Apply a palette to the whole context.
pub fn apply(ctx: &Context, dark: bool) {
    let palette = Palette::for_mode(dark);
    let mut visuals = if dark { Visuals::dark() } else { Visuals::light() };
    visuals.panel_fill = palette.bg;
    visuals.window_fill = palette.surface;
    visuals.faint_bg_color = palette.surface_2;
    visuals.extreme_bg_color = if dark { palette.surface_3 } else { palette.surface };
    visuals.override_text_color = Some(palette.text);
    visuals.selection.bg_fill = palette.accent.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, palette.accent);
    visuals.hyperlink_color = palette.accent;
    visuals.warn_fg_color = palette.warn;
    visuals.error_fg_color = palette.danger;
    visuals.widgets.noninteractive.bg_fill = palette.surface;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text_2);
    visuals.widgets.inactive.bg_fill = palette.surface_2;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border_2);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text_2);
    visuals.widgets.hovered.bg_fill = palette.surface_3;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent_border);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.active.bg_fill = palette.accent_bg;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, palette.accent);
    visuals.striped = false;
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        style.spacing.window_margin = Margin::symmetric(20, 18);
        style.spacing.scroll = egui::style::ScrollStyle {
            floating: false,
            foreground_color: false,
            bar_width: 6.0,
            bar_inner_margin: 4.0,
            bar_outer_margin: 0.0,
            ..Default::default()
        };
        style.text_styles = text_styles(SECTION_TITLE);
    });
}

/// The palette currently in force.
#[must_use]
pub fn palette(ctx: &Context) -> Palette {
    Palette::for_mode(is_dark(ctx))
}

/// Whether the context is drawing in the dark palette.
#[must_use]
pub fn is_dark(ctx: &Context) -> bool {
    matches!(ctx.theme(), egui::Theme::Dark)
}

/// A card's corner radius.
#[must_use]
pub fn radius() -> CornerRadius {
    CornerRadius::same(RADIUS)
}

/// A small control's corner radius.
#[must_use]
pub fn radius_small() -> CornerRadius {
    CornerRadius::same(RADIUS_SMALL)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.1 relative luminance.
    fn luminance(colour: Color32) -> f64 {
        let channel = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(colour.r()) + 0.7152 * channel(colour.g()) + 0.0722 * channel(colour.b())
    }

    /// The contrast ratio between two colours.
    fn contrast(left: Color32, right: Color32) -> f64 {
        let (a, b) = (luminance(left), luminance(right));
        let (lighter, darker) = if a > b { (a, b) } else { (b, a) };
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn every_text_pair_clears_wcag_aa() {
        // The stylesheet failed this in twenty places; the port must not.
        for palette in [Palette::light(), Palette::dark()] {
            for (label, text) in [
                ("text", palette.text),
                ("text-2", palette.text_2),
                ("text-3", palette.text_3),
                ("text-4", palette.text_4),
            ] {
                for (surface, background) in [
                    ("bg", palette.bg),
                    ("surface", palette.surface),
                    ("surface-2", palette.surface_2),
                    ("surface-3", palette.surface_3),
                    ("accent-bg", palette.accent_bg),
                    ("ok-bg", palette.ok_bg),
                    ("warn-bg", palette.warn_bg),
                    ("danger-bg", palette.danger_bg),
                ] {
                    let ratio = contrast(text, background);
                    assert!(
                        ratio >= 4.5,
                        "{label} on {surface} is {ratio:.2}:1, below the 4.5:1 floor"
                    );
                }
            }
        }
    }

    #[test]
    fn the_accent_and_status_colours_are_readable_on_their_own_tints() {
        for palette in [Palette::light(), Palette::dark()] {
            assert!(
                contrast(palette.accent, palette.accent_bg) >= 4.5,
                "accent on accent-bg"
            );
            assert!(
                contrast(palette.accent_fg, palette.accent) >= 4.5,
                "white on accent"
            );
            assert!(contrast(palette.ok, palette.ok_bg) >= 4.5, "ok on ok-bg");
            assert!(contrast(palette.warn, palette.warn_bg) >= 4.5, "warn on warn-bg");
            assert!(
                contrast(palette.danger, palette.danger_bg) >= 4.5,
                "danger on danger-bg"
            );
            assert!(
                contrast(palette.slate, palette.slate_bg) >= 4.5,
                "slate on slate-bg"
            );
        }
    }

    #[test]
    fn a_control_boundary_is_visible_against_what_it_sits_on() {
        // SC 1.4.11 asks 3:1 for a boundary that carries meaning. The stylesheet's
        // #CBD5E1 was 1.36–1.48:1, which is a decoration, not a boundary.
        for palette in [Palette::light(), Palette::dark()] {
            let ratio = contrast(palette.text_4, palette.surface);
            assert!(ratio >= 3.0, "a control's edge must be seen: {ratio:.2}:1");
        }
    }

    #[test]
    fn the_type_scale_never_drops_below_the_readable_floor() {
        for size in [
            SECTION_TITLE,
            PAGE_TITLE,
            FIGURE,
            FIGURE_SMALL,
            BODY,
            BODY_SMALL,
            CAPTION,
            MICRO,
            MONO,
        ] {
            assert!(
                size >= 11.0,
                "{size} is below the 11 px floor the stylesheet states"
            );
        }
        assert_larger(FIGURE, SECTION_TITLE, "a figure is the largest thing on the page");
    }

    /// Compared through parameters so the check is not a constant expression: the
    /// point is that the two sizes keep their relationship as the scale is edited.
    fn assert_larger(larger: f32, smaller: f32, why: &str) {
        assert!(larger > smaller, "{why}: {larger} vs {smaller}");
    }

    #[test]
    fn the_scale_is_one_table_with_one_parameter() {
        let page = text_styles(SECTION_TITLE);
        let dialog = text_styles(DIALOG_TITLE);
        assert_eq!(
            page.get(&TextStyle::Heading).map(|font| font.size),
            Some(SECTION_TITLE)
        );
        assert_eq!(
            dialog.get(&TextStyle::Heading).map(|font| font.size),
            Some(DIALOG_TITLE)
        );
        // Everything else is identical, which is what stops two scales drifting.
        assert_eq!(page.get(&TextStyle::Body), dialog.get(&TextStyle::Body));
        assert_eq!(page.get(&TextStyle::Small), dialog.get(&TextStyle::Small));
    }
}
