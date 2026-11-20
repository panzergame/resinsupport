// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-egui/src/ui_kit_guard_tests.rs
// (`all_ui_and_apps_use_ui_kit_controls`) at a34756830; see ATTRIBUTION.md. Reduced: no allowance
// list, no removed-implementation checks.

//! Feature modules and the app draw their controls with `ui_kit`, never with egui's own: one look
//! for every card, menu and list.

use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};

/// egui's controls the kit replaces, by type and by `Ui` method.
const RAW_TYPES: &[&str] = &["ComboBox", "Button", "Slider", "Checkbox", "RadioButton", "TextEdit", "DragValue", "ProgressBar"];
const RAW_METHODS: &[&str] = &[
    "checkbox",
    "button",
    "small_button",
    "selectable_label",
    "selectable_value",
    "radio",
    "radio_value",
    "toggle_value",
    "text_edit_singleline",
    "text_edit_multiline",
];

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test") || (a.path().is_ident("cfg") && a.parse_args::<syn::Path>().is_ok_and(|p| p.is_ident("test"))))
}

#[derive(Default)]
struct Guard {
    found: Vec<String>,
    /// Names a `use … as …` gave a raw control.
    aliases: Vec<String>,
}

impl<'ast> Visit<'ast> for Guard {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs = match item {
            syn::Item::Mod(i) => &i.attrs,
            syn::Item::Fn(i) => &i.attrs,
            _ => return visit::visit_item(self, item),
        };
        if !test_only(attrs) {
            visit::visit_item(self, item);
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        if RAW_METHODS.contains(&method.as_str()) {
            self.found.push(format!(".{method}"));
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref() {
            let names: Vec<String> = path.path.segments.iter().map(|s| s.ident.to_string()).collect();
            // Loading or storing an editor's state draws nothing.
            let state_access = names.last().is_some_and(|n| matches!(n.as_str(), "load_state" | "store_state"));
            let kit = names.iter().any(|n| n == "ui_kit");
            if !state_access && !kit && names.iter().any(|n| RAW_TYPES.contains(&n.as_str()) || self.aliases.contains(n)) {
                self.found.push(names.join("::"));
            }
        }
        visit::visit_expr_call(self, call);
    }
}

/// The raw controls `source` draws outside its tests.
fn violations(source: &str) -> Vec<String> {
    fn aliases(tree: &syn::UseTree, out: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(p) => aliases(&p.tree, out),
            syn::UseTree::Group(g) => g.items.iter().for_each(|tree| aliases(tree, out)),
            syn::UseTree::Rename(r) if RAW_TYPES.contains(&r.ident.to_string().as_str()) => out.push(r.rename.to_string()),
            _ => {}
        }
    }
    let ast = syn::parse_file(source).unwrap();
    let mut guard = Guard::default();
    for item in &ast.items {
        if let syn::Item::Use(u) = item {
            aliases(&u.tree, &mut guard.aliases);
        }
    }
    guard.visit_file(&ast);
    guard.found
}

/// Every `.rs` file under `dir`, but the kit's own (`ui_kit.rs`, `ui_kit/`) and this one.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
        if path.is_dir() {
            if name != "ui_kit" {
                sources(&path, out);
            }
        } else if name.ends_with(".rs") && !matches!(name.as_str(), "ui_kit.rs" | "ui_kit_guard_tests.rs") {
            out.push(path);
        }
    }
}

#[test]
fn raw_widgets_stay_in_the_kit() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    sources(&crate_dir.join("src"), &mut files);
    sources(&crate_dir.join("../../apps"), &mut files);
    assert!(files.len() > 10, "the sources were found");
    let failures: Vec<String> = files
        .iter()
        .filter_map(|path| {
            let found = violations(&std::fs::read_to_string(path).unwrap());
            (!found.is_empty()).then(|| format!("{}: {found:?}", path.display()))
        })
        .collect();
    assert!(failures.is_empty(), "draw these with ui_kit:\n{}", failures.join("\n"));
}

#[test]
fn the_guard_sees_raw_controls_and_aliases_but_not_tests_or_the_kit() {
    let source = r#"
        use egui::DragValue as Drag;
        fn card(ui: &mut egui::Ui, v: &mut f64, b: &mut bool) {
            ui.add(Drag::new(v));
            ui.checkbox(b, "x");
            egui::ComboBox::from_id_salt("a");
            ui_kit::number_field(ui, v, 0.0..=1.0, 0.1, "", 2);
            egui::TextEdit::load_state(ui.ctx(), ui.id());
        }
        #[cfg(test)]
        mod tests {
            fn t(ui: &mut egui::Ui) { ui.button("x"); }
        }
    "#;
    assert_eq!(violations(source), ["Drag::new", ".checkbox", "egui::ComboBox::from_id_salt"]);
}
