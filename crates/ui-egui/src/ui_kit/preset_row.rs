//! A card's Preset row, for every kind of preset ([`resin_engine::PresetKind`]): a drop-down of
//! the user's presets (and None), then Duplicate (the copy icon) and Delete (the delete icon).
//! A double click on the drop-down renames the preset followed in place; Duplicate names a new
//! preset in the same place, from what is shown. Enter or a click elsewhere commits a name that
//! is free, Esc leaves it. Delete asks first.
//!
//! The row knows names only, never settings or commands: it returns a [`PresetAction`] for the
//! card to run as its kind's command (`<area>.preset.save`, …). Its state is per row, not per
//! kind ([`PresetRows`]), so two cards showing one kind's presets at once never share a name
//! being typed or a confirmation.

use std::collections::HashMap;

use egui::{Rect, Ui, vec2};
use resin_engine::geom::{MAX_PRESET_NAME, same_preset_name};

use super::{Action, Confirm, icon_button, row};
use crate::i18n;
use crate::theme::Tokens;
use crate::theme::metrics::{CONTROL_HEIGHT, SPACE_4};

/// What the user did with a preset row, for the card to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresetAction {
    /// Follow this preset, or none.
    Pick(Option<String>),
    /// A new preset of what is shown, called this, followed from now on.
    Save(String),
    Rename {
        from: String,
        to: String,
    },
    /// Confirmed.
    Delete(String),
}

/// One row's state between frames: the name being typed, the delete being confirmed.
#[derive(Clone, Debug, Default)]
pub struct PresetRowState {
    editing: Option<Editing>,
    confirm_delete: Option<String>,
    /// Where the drop-down was last drawn (tests click round it).
    pub(crate) combo: Option<Rect>,
}

#[derive(Clone, Debug)]
struct Editing {
    /// The preset being renamed; `None` names a new one.
    from: Option<String>,
    name: String,
    /// Just opened: focus the field and select its text.
    fresh: bool,
}

impl PresetRowState {
    /// Whether a name is being typed.
    pub fn editing(&self) -> bool {
        self.editing.is_some()
    }

    /// Whether a delete waits for confirmation.
    pub fn confirming(&self) -> bool {
        self.confirm_delete.is_some()
    }
}

/// Every preset row's state, by row (`"label"`, later `"slicing.tab"` and `"slicing.model"`).
#[derive(Debug, Default)]
pub struct PresetRows(HashMap<&'static str, PresetRowState>);

impl PresetRows {
    pub fn state(&mut self, row: &'static str) -> &mut PresetRowState {
        self.0.entry(row).or_default()
    }

    pub fn get(&self, row: &'static str) -> Option<&PresetRowState> {
        self.0.get(row)
    }

    /// Forget a row's name being typed and confirmation: its card closed.
    pub fn close(&mut self, row: &'static str) {
        self.0.remove(row);
    }
}

pub struct PresetRow<'a> {
    /// The row's own id, unique among rows shown together: its state's key and widget ids.
    pub row: &'static str,
    /// The row's label ("Preset").
    pub label: &'a str,
    /// What the kind's presets hold, on hovering the drop-down.
    pub tip: &'a str,
    /// The presets' names, as the library sorts them.
    pub names: &'a [String],
    /// The name the card's target follows, whether or not it is in `names`.
    pub followed: Option<&'a str>,
}

impl PresetRow<'_> {
    pub fn show(self, ui: &mut Ui, t: &Tokens, state: &mut PresetRowState) -> Option<PresetAction> {
        let known = self.followed.and_then(|f| self.names.iter().find(|n| same_preset_name(n, f)));
        let mut action = None;
        let mut taken = false;
        row(ui, t, self.label, |ui| {
            ui.spacing_mut().item_spacing.x = SPACE_4;
            let width = (ui.available_width() - 2.0 * (CONTROL_HEIGHT + SPACE_4)).max(CONTROL_HEIGHT);
            match state.editing.take() {
                Some(mut editing) => {
                    let (done, is_taken) = self.name_field(ui, width, &mut editing, &mut action);
                    taken = is_taken;
                    if !done {
                        state.editing = Some(editing);
                    }
                }
                None => {
                    let combo = self.combo(ui, width, known, &mut action);
                    state.combo = Some(combo.rect);
                    if let Some(name) = known.filter(|_| combo.double_clicked()) {
                        egui::Popup::close_id(ui.ctx(), combo.id.with("popup"));
                        state.editing = Some(Editing { from: Some(name.clone()), name: name.clone(), fresh: true });
                        action = None;
                    }
                }
            }
            let (duplicate_tip, suggested) = match (known, self.followed) {
                (Some(name), _) => (i18n::tr("ui.preset.duplicate_tip"), self.free_copy_name(name)),
                (None, Some(missing)) => (i18n::tr("ui.preset.restore_tip"), missing.to_string()),
                (None, None) => (i18n::tr("ui.preset.save_tip"), String::new()),
            };
            if icon_button(ui, t, "copy", duplicate_tip, true).clicked() {
                state.editing = Some(Editing { from: None, name: suggested, fresh: true });
            }
            if icon_button(ui, t, "delete", i18n::tr("ui.preset.delete_tip"), known.is_some()).clicked() {
                state.confirm_delete = known.cloned();
            }
        });
        if taken {
            ui.colored_label(t.warning, i18n::tr("ui.preset.taken"));
        }
        if let Some(name) = state.confirm_delete.clone() {
            let message = i18n::format("ui.preset.delete_message", &[("name", &name)]);
            let confirm = Confirm {
                id: egui::Id::new(("preset_delete", self.row)),
                icon: "delete",
                title: i18n::tr("ui.preset.delete_title"),
                message: &message,
                ok: "ui.preset.delete_ok",
            };
            match confirm.show(ui.ctx(), t) {
                Action::Ok => {
                    state.confirm_delete = None;
                    action = Some(PresetAction::Delete(name));
                }
                Action::Cancel => state.confirm_delete = None,
                Action::Reset | Action::None => {}
            }
        }
        action
    }

    fn combo(&self, ui: &mut Ui, width: f32, known: Option<&String>, action: &mut Option<PresetAction>) -> egui::Response {
        let shown = match (known, self.followed) {
            (Some(name), _) => name.clone(),
            (None, Some(missing)) => i18n::format("ui.preset.missing", &[("name", &missing)]),
            (None, None) => i18n::tr("ui.preset.none").into(),
        };
        // Truncated to a child of its width: a long name shortens rather than pushing the icons
        // out of the card.
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        let response = ui
            .allocate_ui_with_layout(vec2(width, CONTROL_HEIGHT), layout, |ui| {
                egui::ComboBox::from_id_salt(("preset_row", self.row))
                    .selected_text(shown)
                    .width(width)
                    .truncate()
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(self.followed.is_none(), i18n::tr("ui.preset.none")).clicked() {
                            *action = Some(PresetAction::Pick(None));
                        }
                        for name in self.names {
                            if ui.selectable_label(known == Some(name), name).clicked() {
                                *action = Some(PresetAction::Pick(Some(name.clone())));
                            }
                        }
                    })
                    .response
            })
            .inner;
        let tip = if known.is_some() { format!("{}\n{}", self.tip, i18n::tr("ui.preset.rename_hint")) } else { self.tip.to_string() };
        response.on_hover_text(tip)
    }

    /// The field naming a new preset or renaming one; whether it is done with, and whether the
    /// name is another preset's.
    fn name_field(&self, ui: &mut Ui, width: f32, editing: &mut Editing, action: &mut Option<PresetAction>) -> (bool, bool) {
        let id = egui::Id::new(("preset_row_name", self.row));
        let mut output = egui::TextEdit::singleline(&mut editing.name)
            .id(id)
            .char_limit(MAX_PRESET_NAME)
            .desired_width(width)
            .min_size(vec2(width, CONTROL_HEIGHT))
            .vertical_align(egui::Align::Center)
            .show(ui);
        if editing.fresh {
            editing.fresh = false;
            output.response.request_focus();
            output.state.cursor.set_char_range(Some(egui::text::CCursorRange::select_all(&output.galley)));
            output.state.store(ui.ctx(), id);
        }
        let name = editing.name.trim().to_string();
        let taken = self.names.iter().any(|n| same_preset_name(n, &name) && !editing.from.as_deref().is_some_and(|f| same_preset_name(f, n)));
        if !output.response.lost_focus() {
            return (false, taken);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            return (true, false);
        }
        let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if name.is_empty() || taken {
            // Enter on a name that cannot be: keep typing. A click elsewhere: give up.
            if entered {
                output.response.request_focus();
            }
            return (!entered, taken && entered);
        }
        *action = match editing.from.clone() {
            Some(from) if from == name => None,
            Some(from) => Some(PresetAction::Rename { from, to: name }),
            None => Some(PresetAction::Save(name)),
        };
        (true, false)
    }

    /// "`name` copy", or "`name` copy 2" and on when that is taken, short enough to be a name.
    fn free_copy_name(&self, name: &str) -> String {
        let fit = |s: String| s.chars().take(MAX_PRESET_NAME).collect::<String>().trim_end().to_string();
        let base = i18n::format("ui.preset.copy_name", &[("name", &name)]);
        (1..=self.names.len() + 1)
            .map(|i| if i == 1 { fit(base.clone()) } else { fit(format!("{base} {i}")) })
            .find(|c| !self.names.iter().any(|n| same_preset_name(n, c)))
            .unwrap_or_else(|| fit(base))
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput};

    use super::*;

    fn key(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    fn click(at: Pos2) -> Vec<Event> {
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        vec![Event::PointerMoved(at), button(true), button(false)]
    }

    /// Two rows of one kind, drawn in one frame at `time`; what each returned.
    struct Two {
        ctx: egui::Context,
        rows: PresetRows,
        names: Vec<String>,
        followed: [Option<String>; 2],
        time: f64,
    }

    impl Two {
        fn new() -> Two {
            let names = vec!["Alpha".to_string(), "Beta".to_string(), "Beta copy".to_string()];
            Two { ctx: egui::Context::default(), rows: PresetRows::default(), names, followed: [Some("alpha".into()), None], time: 0.0 }
        }

        fn frame(&mut self, events: Vec<Event>) -> [Option<PresetAction>; 2] {
            self.time += 0.05;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let mut got = [None, None];
            let t = Tokens::for_dark(false);
            let (names, followed, rows) = (&self.names, &self.followed, &mut self.rows);
            let mut out = self.ctx.run_ui(raw, |ui| {
                ui.set_max_width(300.0);
                for (i, row) in ["a", "b"].into_iter().enumerate() {
                    let shown = PresetRow { row, label: "Preset", tip: "tip", names, followed: followed[i].as_deref() };
                    got[i] = shown.show(ui, &t, rows.state(row));
                }
            });
            out.textures_delta.clear();
            got
        }

        fn combo(&self, row: &'static str) -> Pos2 {
            self.rows.get(row).and_then(|s| s.combo).map(|r| r.center()).unwrap()
        }

        fn editing(&self, row: &'static str) -> bool {
            self.rows.get(row).is_some_and(PresetRowState::editing)
        }
    }

    #[test]
    fn a_double_click_renames_in_place_and_enter_commits() {
        let mut two = Two::new();
        two.frame(vec![]);
        let at = two.combo("a");
        two.frame(click(at));
        two.frame(click(at));
        assert!(two.editing("a") && !two.editing("b"), "one row of the kind edits, the other does not");
        two.frame(vec![]);
        two.frame(vec![Event::Text("Gamma".into())]);
        let got = two.frame(vec![key(Key::Enter)]);
        assert_eq!(got[0], Some(PresetAction::Rename { from: "Alpha".into(), to: "Gamma".into() }), "the selected name is replaced");
        assert!(!two.editing("a"));
    }

    #[test]
    fn duplicate_suggests_a_free_name_and_esc_leaves_it() {
        let mut two = Two::new();
        two.followed = [Some("Beta".into()), None];
        two.frame(vec![]);
        two.rows.state("a").editing = Some(Editing {
            from: None,
            name: PresetRow { row: "a", label: "", tip: "", names: &two.names, followed: None }.free_copy_name("Beta"),
            fresh: true,
        });
        assert_eq!(
            two.rows.state("a").editing.as_ref().map(|e| e.name.clone()),
            Some(format!("{} 2", i18n::format("ui.preset.copy_name", &[("name", &"Beta")])))
        );
        two.frame(vec![]);
        let got = two.frame(vec![key(Key::Enter)]);
        assert_eq!(got[0], Some(PresetAction::Save("Beta copy 2".into())));

        two.rows.state("a").editing = Some(Editing { from: None, name: "x".into(), fresh: true });
        two.frame(vec![]);
        let got = two.frame(vec![key(Key::Escape)]);
        assert_eq!((got[0].clone(), two.editing("a")), (None, false), "Esc: nothing saved");
    }

    #[test]
    fn a_taken_name_is_not_committed() {
        let mut two = Two::new();
        two.frame(vec![]);
        two.rows.state("a").editing = Some(Editing { from: Some("Alpha".into()), name: "beta".into(), fresh: true });
        two.frame(vec![]);
        let got = two.frame(vec![key(Key::Enter)]);
        assert_eq!((got[0].clone(), two.editing("a")), (None, true), "still typing");
        two.rows.state("a").editing = Some(Editing { from: Some("Alpha".into()), name: "ALPHA".into(), fresh: true });
        two.frame(vec![]);
        let got = two.frame(vec![key(Key::Enter)]);
        assert_eq!(got[0], Some(PresetAction::Rename { from: "Alpha".into(), to: "ALPHA".into() }), "its own name in another case");
    }

    #[test]
    fn a_long_name_keeps_the_drop_down_its_width() {
        let mut two = Two::new();
        two.frame(vec![]);
        let before = two.rows.get("b").and_then(|s| s.combo).unwrap();
        two.followed[1] = Some("a name no longer in the presets, long enough to overflow the row".into());
        two.frame(vec![]);
        assert_eq!(two.rows.get("b").and_then(|s| s.combo).map(|r| r.width()), Some(before.width()));
    }

    #[test]
    fn delete_asks_first_and_none_has_nothing_to_delete() {
        let mut two = Two::new();
        two.frame(vec![]);
        two.rows.state("b").confirm_delete = None;
        assert!(!two.rows.get("b").is_some_and(PresetRowState::confirming));
        two.rows.state("a").confirm_delete = Some("Alpha".into());
        two.frame(vec![]);
        let got = two.frame(vec![key(Key::Enter)]);
        assert_eq!(got[0], Some(PresetAction::Delete("Alpha".into())));
        assert!(!two.rows.get("a").is_some_and(PresetRowState::confirming));
        two.rows.close("a");
        assert!(two.rows.get("a").is_none());
    }
}
