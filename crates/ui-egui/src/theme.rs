//! Colour tokens. Every colour the UI or the viewport uses comes from here, so a theme is one
//! place to change. The chrome's colours, sizes and egui style (`ui_*` fields, [`metrics`],
//! [`apply`]) are SolidCraft's. The design file (`design/resinsupport.figcraft`) owns the values
//! between `design-tokens` markers: `cargo xtask design-pull` writes them, and a hand edit there
//! fails `cargo xtask ci`.
//!
//! Chrome values and `visuals`/`apply` ported from SolidCraft (MIT OR Apache-2.0),
//! assets/styles/ui/palettes.rs and crates/ui-egui/src/theme.rs at 4bdffef0; `dialog_label`,
//! `form_primary_fill`, `SPACE_14` and `FORM_INSET` from crates/theme/src/palettes.rs at a34756830;
//! see ATTRIBUTION.md.

use egui::{Color32, Stroke};
use resin_engine::render::OverlayStyle;

/// Chrome dimensions, in points: the same in both themes.
pub mod metrics {
    // design-tokens: Metrics (values from design/tokens.json: edit the design file, then `cargo xtask design-pull`)
    pub const SPACE_2: f32 = 2.0;
    pub const SPACE_3: f32 = 3.0;
    pub const SPACE_4: f32 = 4.0;
    pub const SPACE_6: f32 = 6.0;
    pub const SPACE_8: f32 = 8.0;
    pub const SPACE_10: f32 = 10.0;
    pub const SPACE_12: f32 = 12.0;
    pub const SPACE_14: f32 = 14.0;
    pub const SPACE_16: f32 = 16.0;
    pub const SPACE_24: f32 = 24.0;
    pub const RADIUS_SM: u8 = 4;
    pub const RADIUS_MD: u8 = 6;
    pub const RADIUS_LG: u8 = 8;
    pub const RADIUS_XL: u8 = 10;
    pub const FONT_SMALL: f32 = 11.0;
    pub const FONT_CAPTION: f32 = 11.5;
    pub const FONT_BODY: f32 = 12.0;
    pub const FONT_LABEL: f32 = 12.5;
    pub const FONT_BUTTON: f32 = 13.0;
    pub const FONT_HEADING: f32 = 14.0;
    pub const CONTROL_HEIGHT: f32 = 28.0;
    pub const CONTROL_RADIUS: u8 = 6;
    /// A row's label column; rows of a wide card start from `FORM_LABEL_WIDTH + SPACE_24 +
    /// SPACE_10` (104 pt) instead ([`crate::ui_kit::row`]).
    pub const FORM_LABEL_WIDTH: f32 = 70.0;
    /// A settings window: SolidCraft's wide card.
    pub const FORM_WIDE_CARD_WIDTH: f32 = 520.0;
    pub const FORM_GAP_Y: f32 = 5.0;
    pub const FORM_HEADER_HEIGHT: f32 = 40.0;
    pub const FORM_FOOTER_HEIGHT: f32 = 48.0;
    pub const FORM_CARD_WIDTH: f32 = 264.0;
    /// A card whose tab strip carries five tabs, two-word names among them.
    pub const FORM_TABBED_CARD_WIDTH: f32 = 480.0;
    pub const FORM_BUTTON_HEIGHT: f32 = 28.0;
    pub const FORM_ICON_SIZE: f32 = 19.0;
    pub const FORM_CLOSE_SIZE: f32 = 26.0;
    /// A text field's text, from its left and right edges.
    pub const FORM_INSET: f32 = 9.0;
    /// A row of the Models card.
    pub const TREE_ROW_HEIGHT: f32 = 26.0;
    /// The Models card's width.
    pub const FORM_BROWSER_WIDTH: f32 = 188.0;
    // design-tokens: end
}

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub viewport_top: Color32,
    pub viewport_bottom: Color32,
    pub model: Color32,
    /// A model selected in Object mode.
    pub model_selected: Color32,
    /// Support tips: where one would go under the pointer, placed, and selected.
    pub support_preview: Color32,
    /// The selection box dragged rightwards (takes what is inside) and leftwards (what it touches).
    pub box_window: Color32,
    pub box_crossing: Color32,
    /// The outline of the cut at the shown layer, and the pixel grid of the layer view.
    pub cap_edge: Color32,
    pub layer_grid: Color32,
    pub support: Color32,
    pub support_selected: Color32,
    /// A support with no path to the plate.
    pub support_unreachable: Color32,
    /// In Support edition, faces looking down: the steeper toward the plate, the more of it.
    pub overhang: Color32,
    /// The build plate's surface, and the arrow at its front edge.
    pub plate: Color32,
    pub plate_arrow: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub axis_x: Color32,
    pub axis_y: Color32,
    pub axis_z: Color32,
    /// The gizmo: a handle under the pointer or being dragged, and the centre ball.
    pub gizmo_hot: Color32,
    pub gizmo_ball: Color32,
    pub gizmo_ball_edge: Color32,
    pub text_dim: Color32,
    pub error: Color32,
    /// A confirmation that undoes the user's own work.
    pub warning: Color32,
    /// Behind a modal dialog.
    pub backdrop: Color32,
    // ---- chrome (SolidCraft's palette) ----
    pub dark: bool,
    pub panel: Color32,
    pub panel_header: Color32,
    pub border: Color32,
    pub text: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    /// Text on `accent_soft` (a selected tab).
    pub selected_ink: Color32,
    pub selected_bg: Color32,
    pub hover: Color32,
    pub field: Color32,
    pub control_fill: Color32,
    pub control_border: Color32,
    pub on_accent: Color32,
    pub dialog_body: Color32,
    pub dialog_footer: Color32,
    pub dialog_rule: Color32,
    pub dialog_border: Color32,
    pub form_ink: Color32,
    pub form_dim: Color32,
    pub form_primary: Color32,
    /// A ticked checkbox.
    pub form_primary_fill: Color32,
    /// A form row's label.
    pub dialog_label: Color32,
    /// The application bar: always dark.
    pub app_bar: Color32,
    pub app_bar_text: Color32,
    pub app_bar_hover: Color32,
    pub app_bar_hover_button: Color32,
    pub toolbar: Color32,
}

impl Tokens {
    pub fn for_dark(dark: bool) -> Tokens {
        if dark {
            Tokens {
                viewport_top: Color32::from_rgb(0x3a, 0x3f, 0x47),
                viewport_bottom: Color32::from_rgb(0x1c, 0x1f, 0x24),
                model: Color32::from_rgb(0x7f, 0xa8, 0xd8),
                model_selected: Color32::from_rgb(0xc0, 0x8c, 0xf0),
                support_preview: Color32::from_rgb(0x5c, 0xe0, 0x8a),
                box_window: Color32::from_rgb(70, 150, 255),
                box_crossing: Color32::from_rgb(80, 200, 120),
                cap_edge: Color32::from_rgb(0x24, 0x3a, 0x5a),
                layer_grid: Color32::from_rgba_unmultiplied(0x6c, 0x7c, 0x94, 0xa0),
                support: Color32::from_rgb(0xe8, 0xe4, 0xd8),
                support_selected: Color32::from_rgb(0xff, 0x9a, 0x2e),
                support_unreachable: Color32::from_rgb(0xf0, 0x55, 0x55),
                overhang: Color32::from_rgb(0xff, 0x78, 0x1e),
                plate: Color32::from_rgb(0x42, 0x49, 0x55),
                plate_arrow: Color32::from_rgb(0x9a, 0xa6, 0xb6),
                grid_minor: Color32::from_rgba_unmultiplied(0x9a, 0xa4, 0xb0, 0x30),
                grid_major: Color32::from_rgba_unmultiplied(0xb8, 0xc2, 0xce, 0x70),
                axis_x: Color32::from_rgb(0xe0, 0x4a, 0x4a),
                axis_y: Color32::from_rgb(0x6c, 0xc0, 0x3c),
                axis_z: Color32::from_rgb(0x3f, 0x7f, 0xe8),
                gizmo_hot: Color32::from_rgb(0xff, 0xd2, 0x4a),
                gizmo_ball: Color32::from_gray(235),
                gizmo_ball_edge: Color32::from_gray(60),
                dark: true,
                // design-tokens: Colour/Dark (values from design/tokens.json: edit the design file, then `cargo xtask design-pull`)
                text_dim: Color32::from_rgb(0x94, 0x9b, 0xa8),
                error: Color32::from_rgb(0xf0, 0x64, 0x60),
                warning: Color32::from_rgb(0xec, 0xaa, 0x46),
                backdrop: Color32::from_rgba_unmultiplied(0x00, 0x00, 0x00, 0x78),
                panel: Color32::from_rgb(0x27, 0x2b, 0x34),
                panel_header: Color32::from_rgb(0x2f, 0x34, 0x3e),
                border: Color32::from_rgb(0x3f, 0x45, 0x52),
                text: Color32::from_rgb(0xde, 0xe2, 0xe8),
                accent: Color32::from_rgb(0x3a, 0x88, 0xee),
                accent_soft: Color32::from_rgb(0x2a, 0x46, 0x6e),
                selected_ink: Color32::from_rgb(0x9c, 0xc7, 0xf5),
                selected_bg: Color32::from_rgb(0x2a, 0x46, 0x6e),
                hover: Color32::from_rgb(0x36, 0x3d, 0x4a),
                field: Color32::from_rgb(0x1e, 0x22, 0x29),
                control_fill: Color32::from_rgb(0x1c, 0x1f, 0x26),
                control_border: Color32::from_rgb(0x6a, 0x75, 0x8c),
                on_accent: Color32::from_rgb(0xff, 0xff, 0xff),
                dialog_body: Color32::from_rgb(0x27, 0x2b, 0x34),
                dialog_footer: Color32::from_rgb(0x2f, 0x34, 0x3e),
                dialog_rule: Color32::from_rgb(0x35, 0x3b, 0x47),
                dialog_border: Color32::from_rgb(0x3f, 0x45, 0x52),
                form_ink: Color32::from_rgb(0xde, 0xe2, 0xe8),
                form_dim: Color32::from_rgb(0xb4, 0xbb, 0xc8),
                form_primary: Color32::from_rgb(0x86, 0xb6, 0xf5),
                form_primary_fill: Color32::from_rgb(0x14, 0x71, 0xeb),
                dialog_label: Color32::from_rgb(0xde, 0xe2, 0xe8),
                app_bar: Color32::from_rgb(0x1c, 0x1f, 0x26),
                app_bar_text: Color32::from_rgb(0xe6, 0xea, 0xf2),
                app_bar_hover: Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0x1c),
                app_bar_hover_button: Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0x1e),
                toolbar: Color32::from_rgb(0x27, 0x2b, 0x34),
                // design-tokens: end
            }
        } else {
            Tokens {
                viewport_top: Color32::from_rgb(0xe9, 0xec, 0xf1),
                viewport_bottom: Color32::from_rgb(0xb9, 0xc0, 0xca),
                model: Color32::from_rgb(0x4f, 0x7f, 0xb8),
                model_selected: Color32::from_rgb(0x9a, 0x50, 0xd8),
                support_preview: Color32::from_rgb(0x1f, 0xa8, 0x50),
                box_window: Color32::from_rgb(38, 120, 218),
                box_crossing: Color32::from_rgb(40, 160, 80),
                cap_edge: Color32::from_rgb(0x1a, 0x30, 0x52),
                layer_grid: Color32::from_rgba_unmultiplied(0x6c, 0x7c, 0x94, 0xa0),
                support: Color32::from_rgb(0xf4, 0xf1, 0xe8),
                support_selected: Color32::from_rgb(0xf0, 0x7a, 0x00),
                support_unreachable: Color32::from_rgb(0xd0, 0x2a, 0x2a),
                overhang: Color32::from_rgb(0xf0, 0x64, 0x00),
                plate: Color32::from_rgb(0xfb, 0xfc, 0xfd),
                plate_arrow: Color32::from_rgb(0x6e, 0x78, 0x86),
                grid_minor: Color32::from_rgba_unmultiplied(0x40, 0x48, 0x54, 0x28),
                grid_major: Color32::from_rgba_unmultiplied(0x30, 0x38, 0x44, 0x60),
                axis_x: Color32::from_rgb(0xd0, 0x30, 0x30),
                axis_y: Color32::from_rgb(0x4a, 0xa0, 0x20),
                axis_z: Color32::from_rgb(0x20, 0x60, 0xd0),
                gizmo_hot: Color32::from_rgb(0xe0, 0xa0, 0x00),
                gizmo_ball: Color32::from_gray(250),
                gizmo_ball_edge: Color32::from_gray(50),
                dark: false,
                // design-tokens: Colour/Light (values from design/tokens.json: edit the design file, then `cargo xtask design-pull`)
                text_dim: Color32::from_rgb(0x6b, 0x71, 0x7d),
                error: Color32::from_rgb(0xd6, 0x36, 0x36),
                warning: Color32::from_rgb(0x9e, 0x6b, 0x15),
                backdrop: Color32::from_rgba_unmultiplied(0x00, 0x00, 0x00, 0x50),
                panel: Color32::from_rgb(0xfa, 0xfb, 0xfc),
                panel_header: Color32::from_rgb(0xec, 0xee, 0xf2),
                border: Color32::from_rgb(0xd0, 0xd4, 0xdc),
                text: Color32::from_rgb(0x26, 0x2a, 0x32),
                accent: Color32::from_rgb(0x26, 0x78, 0xda),
                accent_soft: Color32::from_rgb(0xd6, 0xe6, 0xfa),
                selected_ink: Color32::from_rgb(0x1a, 0x50, 0x7f),
                selected_bg: Color32::from_rgb(0xe8, 0xf1, 0xfa),
                hover: Color32::from_rgb(0xe2, 0xea, 0xf6),
                field: Color32::from_rgb(0xff, 0xff, 0xff),
                control_fill: Color32::from_rgb(0xff, 0xff, 0xff),
                control_border: Color32::from_rgb(0x84, 0x95, 0xab),
                on_accent: Color32::from_rgb(0xff, 0xff, 0xff),
                dialog_body: Color32::from_rgb(0xff, 0xff, 0xff),
                dialog_footer: Color32::from_rgb(0xf4, 0xf6, 0xf9),
                dialog_rule: Color32::from_rgb(0xe6, 0xea, 0xf0),
                dialog_border: Color32::from_rgb(0xd5, 0xdb, 0xe3),
                form_ink: Color32::from_rgb(0x1b, 0x24, 0x30),
                form_dim: Color32::from_rgb(0x4a, 0x58, 0x68),
                form_primary: Color32::from_rgb(0x23, 0x6e, 0xad),
                form_primary_fill: Color32::from_rgb(0x23, 0x6e, 0xad),
                dialog_label: Color32::from_rgb(0x3a, 0x46, 0x56),
                app_bar: Color32::from_rgb(0x34, 0x38, 0x40),
                app_bar_text: Color32::from_rgb(0xe6, 0xea, 0xf2),
                app_bar_hover: Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0x1c),
                app_bar_hover_button: Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0x1e),
                toolbar: Color32::from_rgb(0xf6, 0xf7, 0xf9),
                // design-tokens: end
            }
        }
    }

    /// The floating cards' shadow.
    pub fn shell_shadow(&self) -> egui::Shadow {
        egui::Shadow { offset: [0, metrics::SPACE_2 as i8], blur: metrics::SPACE_8 as u8, spread: 0, color: self.app_bar.gamma_multiply(0.18) }
    }

    pub fn axes(&self) -> [Color32; 3] {
        [self.axis_x, self.axis_y, self.axis_z]
    }

    pub fn overlay(&self) -> OverlayStyle {
        OverlayStyle {
            minor: linear(self.grid_minor),
            major: linear(self.grid_major),
            x_axis: linear(self.axis_x),
            y_axis: linear(self.axis_y),
            z_axis: linear(self.axis_z),
        }
    }
}

/// Straight (not premultiplied) linear RGBA, as the viewport shaders take colours.
pub fn linear(c: Color32) -> [f32; 4] {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    [lin(r), lin(g), lin(b), f32::from(a) / 255.0]
}

/// egui's own widgets in the chrome's colours.
fn visuals(dark: bool) -> egui::Visuals {
    use metrics::*;
    let t = Tokens::for_dark(dark);
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.window_shadow = t.shell_shadow();
    v.popup_shadow = t.shell_shadow();
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.panel_header;
    v.override_text_color = Some(t.text);
    v.weak_text_color = Some(t.text_dim);
    v.widgets.inactive.weak_bg_fill = t.panel_header;
    v.widgets.inactive.bg_fill = t.panel_header;
    v.widgets.active.weak_bg_fill = t.accent_soft;
    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_fill = t.hover;
    // egui uses the open-state fill for a movable window's active title bar.
    v.widgets.open.weak_bg_fill = t.panel_header;
    for state in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        state.bg_stroke = Stroke::new(1.0, t.control_border);
    }
    v.window_corner_radius = egui::CornerRadius::same(RADIUS_MD);
    v.menu_corner_radius = egui::CornerRadius::same(RADIUS_MD);
    // Set every interaction state so hovering or focusing cannot restore rounded corners.
    for widget in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.active, &mut v.widgets.open] {
        widget.corner_radius = egui::CornerRadius::same(RADIUS_SM / 2);
    }
    v.widgets.hovered.corner_radius = egui::CornerRadius::same(RADIUS_SM.saturating_sub(1));
    v
}

/// Give egui the chrome's style, for both themes: the window follows the system's.
pub fn apply(ctx: &egui::Context) {
    for (theme, dark) in [(egui::Theme::Dark, true), (egui::Theme::Light, false)] {
        ctx.set_visuals_of(theme, visuals(dark));
        ctx.style_mut_of(theme, |s| {
            s.spacing.item_spacing = egui::vec2(metrics::SPACE_6, metrics::SPACE_4);
            s.spacing.button_padding = egui::vec2(metrics::SPACE_6, metrics::SPACE_3);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2's relative luminance of an opaque colour.
    fn luminance(c: Color32) -> f64 {
        let channel = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.039_28 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (hi, lo) = (luminance(a).max(luminance(b)), luminance(a).min(luminance(b)));
        (hi + 0.05) / (lo + 0.05)
    }

    /// Text reaches 4.5:1 on what it is drawn on, and a control's edge or fill 3:1, in both
    /// themes (WCAG 1.4.3 and 1.4.11; `.agents/skills/resinsupport-ui/SKILL.md`, Accessibility).
    /// A new ink or surface adds its pairs here.
    #[test]
    fn text_and_controls_meet_contrast_in_both_themes() {
        type Pair = (&'static str, fn(&Tokens) -> Color32, fn(&Tokens) -> Color32);
        let text: &[Pair] = &[
            ("text on panel", |t| t.text, |t| t.panel),
            ("text on toolbar", |t| t.text, |t| t.toolbar),
            ("text_dim on panel", |t| t.text_dim, |t| t.panel),
            ("text_dim on toolbar", |t| t.text_dim, |t| t.toolbar),
            ("form_ink on dialog_body", |t| t.form_ink, |t| t.dialog_body),
            ("form_dim on dialog_body", |t| t.form_dim, |t| t.dialog_body),
            ("dialog_label on dialog_body", |t| t.dialog_label, |t| t.dialog_body),
            ("form_primary on dialog_body", |t| t.form_primary, |t| t.dialog_body),
            ("form_primary on selected_bg", |t| t.form_primary, |t| t.selected_bg),
            ("on_accent on form_primary_fill", |t| t.on_accent, |t| t.form_primary_fill),
            ("app_bar_text on app_bar", |t| t.app_bar_text, |t| t.app_bar),
            ("error on dialog_body", |t| t.error, |t| t.dialog_body),
            ("warning on dialog_body", |t| t.warning, |t| t.dialog_body),
            ("selected_ink on selected_bg", |t| t.selected_ink, |t| t.selected_bg),
            ("selected_ink on accent_soft", |t| t.selected_ink, |t| t.accent_soft),
        ];
        let controls: &[Pair] = &[
            ("control_border on field", |t| t.control_border, |t| t.field),
            ("control_border on dialog_body", |t| t.control_border, |t| t.dialog_body),
            ("control_border on control_fill", |t| t.control_border, |t| t.control_fill),
            ("form_primary_fill on dialog_body", |t| t.form_primary_fill, |t| t.dialog_body),
            ("accent on dialog_body", |t| t.accent, |t| t.dialog_body),
        ];
        let mut low = Vec::new();
        for dark in [false, true] {
            let t = Tokens::for_dark(dark);
            for (pairs, need) in [(text, 4.5), (controls, 3.0)] {
                for (name, ink, ground) in pairs {
                    let ratio = contrast(ink(&t), ground(&t));
                    if ratio < need {
                        low.push(format!("{} {name}: {ratio:.2} (needs {need})", if dark { "dark" } else { "light" }));
                    }
                }
            }
        }
        assert!(low.is_empty(), "change these in the design file, then `cargo xtask design-pull`:\n{}", low.join("\n"));
    }

    #[test]
    fn contrast_is_wcags() {
        assert!((contrast(Color32::WHITE, Color32::BLACK) - 21.0).abs() < 1e-9);
        assert!((contrast(Color32::from_gray(0x77), Color32::WHITE) - 4.48).abs() < 0.01);
    }
}
