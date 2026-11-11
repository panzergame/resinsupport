//! Colour tokens. Every colour the UI or the viewport uses comes from here, so a theme is one
//! place to change. The chrome's colours, sizes and egui style (`ui_*` fields, [`metrics`],
//! [`apply`]) are SolidCraft's.
//!
//! Chrome values and `visuals`/`apply` ported from SolidCraft (MIT OR Apache-2.0),
//! assets/styles/ui/palettes.rs and crates/ui-egui/src/theme.rs at 4bdffef0; see ATTRIBUTION.md.

use egui::{Color32, Stroke};
use resin_engine::render::OverlayStyle;

/// Chrome dimensions, in points: the same in both themes.
pub mod metrics {
    pub const SPACE_2: f32 = 2.0;
    pub const SPACE_3: f32 = 3.0;
    pub const SPACE_4: f32 = 4.0;
    pub const SPACE_6: f32 = 6.0;
    pub const SPACE_8: f32 = 8.0;
    pub const SPACE_10: f32 = 10.0;
    pub const SPACE_12: f32 = 12.0;
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
    pub const FORM_LABEL_WIDTH: f32 = 70.0;
    /// Settings rows: SolidCraft widens its 104 pt manager basis by up to 1.6 times for long names.
    pub const FORM_WIDE_LABEL_WIDTH: f32 = 140.0;
    /// A settings window: SolidCraft's wide card.
    pub const FORM_WIDE_CARD_WIDTH: f32 = 520.0;
    pub const FORM_GAP_Y: f32 = 5.0;
    pub const FORM_HEADER_HEIGHT: f32 = 40.0;
    pub const FORM_FOOTER_HEIGHT: f32 = 48.0;
    pub const FORM_CARD_WIDTH: f32 = 264.0;
    /// A card whose tab strip carries two-word tab names.
    pub const FORM_TABBED_CARD_WIDTH: f32 = 400.0;
    pub const FORM_BUTTON_HEIGHT: f32 = 28.0;
    pub const FORM_ICON_SIZE: f32 = 19.0;
    pub const FORM_CLOSE_SIZE: f32 = 26.0;
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
                grid_minor: Color32::from_rgba_unmultiplied(0x9a, 0xa4, 0xb0, 0x30),
                grid_major: Color32::from_rgba_unmultiplied(0xb8, 0xc2, 0xce, 0x70),
                axis_x: Color32::from_rgb(0xe0, 0x4a, 0x4a),
                axis_y: Color32::from_rgb(0x6c, 0xc0, 0x3c),
                axis_z: Color32::from_rgb(0x3f, 0x7f, 0xe8),
                gizmo_hot: Color32::from_rgb(0xff, 0xd2, 0x4a),
                gizmo_ball: Color32::from_gray(235),
                gizmo_ball_edge: Color32::from_gray(60),
                text_dim: Color32::from_rgb(148, 155, 168),
                error: Color32::from_rgb(240, 100, 96),
                warning: Color32::from_rgb(236, 170, 70),
                backdrop: Color32::from_black_alpha(120),
                dark: true,
                panel: Color32::from_rgb(39, 43, 52),
                panel_header: Color32::from_rgb(47, 52, 62),
                border: Color32::from_rgb(63, 69, 82),
                text: Color32::from_rgb(222, 226, 232),
                accent: Color32::from_rgb(58, 136, 238),
                accent_soft: Color32::from_rgb(42, 70, 110),
                selected_ink: Color32::from_rgb(156, 199, 245),
                selected_bg: Color32::from_rgb(42, 70, 110),
                hover: Color32::from_rgb(54, 61, 74),
                field: Color32::from_rgb(30, 34, 41),
                control_fill: Color32::from_rgb(28, 31, 38),
                control_border: Color32::from_rgb(74, 82, 98),
                on_accent: Color32::WHITE,
                dialog_body: Color32::from_rgb(39, 43, 52),
                dialog_footer: Color32::from_rgb(47, 52, 62),
                dialog_rule: Color32::from_rgb(53, 59, 71),
                dialog_border: Color32::from_rgb(63, 69, 82),
                form_ink: Color32::from_rgb(222, 226, 232),
                form_dim: Color32::from_rgb(180, 187, 200),
                form_primary: Color32::from_rgb(58, 136, 238),
                app_bar: Color32::from_rgb(28, 31, 38),
                app_bar_text: Color32::from_rgb(230, 234, 242),
                app_bar_hover: Color32::from_rgba_unmultiplied(255, 255, 255, 28),
                app_bar_hover_button: Color32::from_rgba_unmultiplied(255, 255, 255, 30),
                toolbar: Color32::from_rgb(39, 43, 52),
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
                grid_minor: Color32::from_rgba_unmultiplied(0x40, 0x48, 0x54, 0x28),
                grid_major: Color32::from_rgba_unmultiplied(0x30, 0x38, 0x44, 0x60),
                axis_x: Color32::from_rgb(0xd0, 0x30, 0x30),
                axis_y: Color32::from_rgb(0x4a, 0xa0, 0x20),
                axis_z: Color32::from_rgb(0x20, 0x60, 0xd0),
                gizmo_hot: Color32::from_rgb(0xe0, 0xa0, 0x00),
                gizmo_ball: Color32::from_gray(250),
                gizmo_ball_edge: Color32::from_gray(50),
                text_dim: Color32::from_rgb(110, 116, 128),
                error: Color32::from_rgb(214, 54, 54),
                warning: Color32::from_rgb(222, 150, 30),
                backdrop: Color32::from_black_alpha(80),
                dark: false,
                panel: Color32::from_rgb(250, 251, 252),
                panel_header: Color32::from_rgb(236, 238, 242),
                border: Color32::from_rgb(208, 212, 220),
                text: Color32::from_rgb(38, 42, 50),
                accent: Color32::from_rgb(38, 120, 218),
                accent_soft: Color32::from_rgb(214, 230, 250),
                selected_ink: Color32::from_rgb(26, 80, 127),
                selected_bg: Color32::from_rgb(232, 241, 250),
                hover: Color32::from_rgb(226, 234, 246),
                field: Color32::WHITE,
                control_fill: Color32::WHITE,
                control_border: Color32::from_rgb(195, 203, 214),
                on_accent: Color32::WHITE,
                dialog_body: Color32::WHITE,
                dialog_footer: Color32::from_rgb(244, 246, 249),
                dialog_rule: Color32::from_rgb(230, 234, 240),
                dialog_border: Color32::from_rgb(213, 219, 227),
                form_ink: Color32::from_rgb(27, 36, 48),
                form_dim: Color32::from_rgb(74, 88, 104),
                form_primary: Color32::from_rgb(35, 110, 173),
                app_bar: Color32::from_rgb(52, 56, 64),
                app_bar_text: Color32::from_rgb(230, 234, 242),
                app_bar_hover: Color32::from_rgba_unmultiplied(255, 255, 255, 28),
                app_bar_hover_button: Color32::from_rgba_unmultiplied(255, 255, 255, 30),
                toolbar: Color32::from_rgb(246, 247, 249),
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
