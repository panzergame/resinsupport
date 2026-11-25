// `describe_for_accessibility` ported from SolidCraft (MIT OR Apache-2.0),
// crates/ui-kit/src/container.rs at a34756830; see ATTRIBUTION.md.

//! Tooltips. Feature code shows one only through [`Tip`], so how a tooltip looks and what it
//! tells a screen reader change in this one place (`raw_widgets_stay_in_the_kit`). egui 0.36's
//! `on_hover_text` only shows the popup, so a tip is given to the widget's accessibility node
//! here: as its description, and as its name when it has none.
//!
//! The idea comes from SolidCraft's `BackingTooltipExt` (same file); this one is ours, without
//! its blurred backing.

use egui::accesskit::Role;
use egui::{Response, WidgetText};

pub trait Tip: Sized {
    /// The tip while the control can act.
    fn tip(self, text: impl Into<WidgetText>) -> Self;
    /// The tip while the control is dimmed: why it can't act.
    fn disabled_tip(self, text: impl Into<WidgetText>) -> Self;
    /// The same tip whether or not the control can act.
    fn tip_always(self, text: impl Into<WidgetText>) -> Self;
}

impl Tip for Response {
    fn tip(self, text: impl Into<WidgetText>) -> Self {
        let text = text.into();
        if self.enabled() {
            describe_for_accessibility(&self, text.text());
        }
        self.on_hover_text(text)
    }

    fn disabled_tip(self, text: impl Into<WidgetText>) -> Self {
        let text = text.into();
        if !self.enabled() {
            describe_for_accessibility(&self, text.text());
        }
        self.on_disabled_hover_text(text)
    }

    fn tip_always(self, text: impl Into<WidgetText>) -> Self {
        let text = text.into();
        describe_for_accessibility(&self, text.text());
        self.on_hover_text(text.clone()).on_disabled_hover_text(text)
    }
}

/// The tip becomes the node's description, and its name when it has none (a label is named by its
/// text, a widget with a `labelled_by` relation by that). Does nothing while accesskit is off.
fn describe_for_accessibility(response: &Response, text: &str) {
    response.ctx.accesskit_node_builder(response.id, |node| {
        if node.description().is_none() {
            node.set_description(text);
        }
        if node.role() != Role::Label && node.labelled_by().is_empty() && node.label().is_none_or(str::is_empty) {
            node.set_label(text);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The accessibility nodes of one frame running `ui`.
    pub(crate) fn nodes(ui: impl FnMut(&mut egui::Ui)) -> Vec<egui::accesskit::Node> {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut ui = ui;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui_| ui(ui_));
        out.textures_delta.clear();
        out.platform_output.accesskit_update.map(|u| u.nodes.into_iter().map(|(_, n)| n).collect()).unwrap_or_default()
    }

    #[test]
    fn a_tip_names_and_describes_an_unnamed_control_and_only_describes_a_named_one() {
        let found = nodes(|ui| {
            let (_, bare) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
            bare.tip("Fit the view");
            let named = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click()).1;
            named.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Fit"));
            named.tip("Frame everything");
            ui.add_enabled_ui(false, |ui| {
                let (_, dimmed) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                dimmed.tip("Never shown").disabled_tip("Select a model first");
            });
        });
        let has = |label: &str, description: &str| found.iter().any(|n| n.label() == Some(label) && n.description() == Some(description));
        assert!(has("Fit the view", "Fit the view"));
        assert!(has("Fit", "Frame everything"));
        assert!(has("Select a model first", "Select a model first"));
        assert!(!found.iter().any(|n| n.description() == Some("Never shown")));
    }
}
