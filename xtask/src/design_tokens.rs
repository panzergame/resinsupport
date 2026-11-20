//! The design file's variables own the UI's colours and sizes. `design/tokens.json` (FigCraft's
//! `variable.export` of `design/resinsupport.figcraft`) is written into the marked regions of the
//! code: a region opens with `// design-tokens: <Collection>[/<Mode>]` and closes with
//! `// design-tokens: end`, and each token line in it takes its value from the variable of the
//! same name. The code names the tokens and the design file gives their values, so a name the
//! code uses that the design file lacks refuses the whole generation: nothing is written.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value as Json;

pub const TOKENS: &str = "design/tokens.json";
/// The files holding regions.
pub const FILES: &[&str] = &["crates/ui-egui/src/theme.rs", "crates/ui-egui/src/icons.rs"];
const BEGIN: &str = "// design-tokens: ";
const END: &str = "// design-tokens: end";
/// The mode a single-mode collection (Metrics) is given in the design file.
pub const SINGLE_MODE: &str = "Value";
/// The icon roles' third mode: the colour the SVG masters write for the role, so an icon exported
/// from the design file maps back to its roles.
pub const SOURCE_MODE: &str = "Source";
/// `line` is `currentColor` in the masters, and its light colour is `ink`'s: in the design file
/// its Source colour is this one instead, so the two stay apart.
pub const LINE_SOURCE: [u8; 4] = [0x01, 0x01, 0x01, 0xff];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Colour([u8; 4]),
    Number(f64),
}

/// A token as the code holds it: its collection, name, and value in each mode.
#[derive(Clone, Debug)]
pub struct CodeToken {
    pub collection: String,
    pub name: String,
    pub values: Vec<(String, Value)>,
    /// A metric's Rust type (`f32`, `u8`), for its variable's scope.
    pub ty: Option<String>,
    /// An icon role's colour in the masters (`#rrggbb`).
    pub source: Option<String>,
}

/// A line of a region that carries a token.
enum Line {
    /// `name: Color32::…,`
    Colour { indent: String, name: String, value: [u8; 4] },
    /// `pub const NAME: f32 = 2.0;`
    Metric { indent: String, name: String, ty: String, value: f64 },
    /// `("ink", "#34465b", [r, g, b], [r, g, b]),`: the Light and Dark colours of an icon role.
    Role { indent: String, name: String, source: String, light: [u8; 3], dark: [u8; 3] },
}

fn parse_int(s: &str) -> Option<u32> {
    let s = s.trim();
    match s.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

fn bytes<const N: usize>(list: &str) -> Option<[u8; N]> {
    let parts: Vec<u8> = list.split(',').map(|p| parse_int(p).and_then(|v| u8::try_from(v).ok())).collect::<Option<_>>()?;
    parts.try_into().ok()
}

/// The colour an egui `Color32` expression spells, in the forms the theme uses.
pub fn parse_colour(expr: &str) -> Option<[u8; 4]> {
    let expr = expr.trim();
    match expr {
        "Color32::WHITE" => return Some([255, 255, 255, 255]),
        "Color32::BLACK" => return Some([0, 0, 0, 255]),
        "Color32::TRANSPARENT" => return Some([0, 0, 0, 0]),
        _ => {}
    }
    let (ctor, args) = expr.strip_prefix("Color32::")?.split_once('(')?;
    let args = args.strip_suffix(')')?;
    match ctor {
        "from_rgb" => bytes::<3>(args).map(|[r, g, b]| [r, g, b, 255]),
        "from_rgba_unmultiplied" => bytes::<4>(args),
        "from_gray" => bytes::<1>(args).map(|[l]| [l, l, l, 255]),
        // Black premultiplied is black unmultiplied.
        "from_black_alpha" => bytes::<1>(args).map(|[a]| [0, 0, 0, a]),
        _ => None,
    }
}

fn format_colour(c: [u8; 4]) -> String {
    let [r, g, b, a] = c;
    if a == 255 {
        format!("Color32::from_rgb(0x{r:02x}, 0x{g:02x}, 0x{b:02x})")
    } else {
        format!("Color32::from_rgba_unmultiplied(0x{r:02x}, 0x{g:02x}, 0x{b:02x}, 0x{a:02x})")
    }
}

fn format_bytes(c: [u8; 3]) -> String {
    format!("[0x{:02x}, 0x{:02x}, 0x{:02x}]", c[0], c[1], c[2])
}

fn parse_line(line: &str) -> Option<Line> {
    let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    let body = line.trim();
    if let Some(rest) = body.strip_prefix("pub const ") {
        let (name, rest) = rest.split_once(':')?;
        let (ty, value) = rest.split_once('=')?;
        let value: f64 = value.trim().strip_suffix(';')?.trim().parse().ok()?;
        return Some(Line::Metric { indent, name: name.trim().into(), ty: ty.trim().into(), value });
    }
    if let Some(rest) = body.strip_prefix("(\"") {
        let (name, rest) = rest.split_once('"')?;
        let rest = rest.trim_start().strip_prefix(',')?.trim_start().strip_prefix('"')?;
        let (source, rest) = rest.split_once('"')?;
        let rest = rest.trim_start().strip_prefix(',')?.trim_start().strip_prefix('[')?;
        let (light, rest) = rest.split_once(']')?;
        let rest = rest.trim_start().strip_prefix(',')?.trim_start().strip_prefix('[')?;
        let (dark, rest) = rest.split_once(']')?;
        if rest.trim() != ")," {
            return None;
        }
        return Some(Line::Role { indent, name: name.into(), source: source.into(), light: bytes(light)?, dark: bytes(dark)? });
    }
    let (name, expr) = body.split_once(':')?;
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let value = parse_colour(expr.trim().strip_suffix(',')?)?;
    Some(Line::Colour { indent, name: name.into(), value })
}

/// A region's collection and mode, from its opening line.
fn region_spec(line: &str) -> Option<(String, Option<String>)> {
    let spec = line.trim().strip_prefix(BEGIN)?.split_whitespace().next()?;
    if spec == "end" {
        return None;
    }
    Some(match spec.split_once('/') {
        Some((c, m)) => (c.to_string(), Some(m.to_string())),
        None => (spec.to_string(), None),
    })
}

/// Walk `text`'s regions: `each` sees every token line with its region's collection and mode, and
/// returns the line to write instead (or `None` to keep it). Lines that are not tokens (comments,
/// blank lines) stay as they are; a line that looks like neither is a problem.
fn walk(shown: &str, text: &str, mut each: impl FnMut(&str, Option<&str>, &Line) -> Option<String>, problems: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut region: Option<(String, Option<String>)> = None;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        let trimmed = line.trim();
        let content = line.strip_suffix('\n').unwrap_or(line);
        if trimmed.starts_with(BEGIN) {
            if trimmed == END {
                if region.take().is_none() {
                    problems.push(format!("{shown}:{}: a region closes that never opened", i + 1));
                }
            } else if region.is_some() {
                problems.push(format!("{shown}:{}: a region opens inside another", i + 1));
            } else {
                region = region_spec(trimmed);
            }
            out.push_str(line);
            continue;
        }
        let Some((collection, mode)) = &region else {
            out.push_str(line);
            continue;
        };
        if trimmed.is_empty() || trimmed.starts_with("//") {
            out.push_str(line);
            continue;
        }
        match parse_line(content) {
            Some(parsed) => match each(collection, mode.as_deref(), &parsed) {
                Some(new) => {
                    out.push_str(&new);
                    if line.ends_with('\n') {
                        out.push('\n');
                    }
                }
                None => out.push_str(line),
            },
            None => {
                problems.push(format!("{shown}:{}: not a token line: `{trimmed}`", i + 1));
                out.push_str(line);
            }
        }
    }
    if region.is_some() {
        problems.push(format!("{shown}: a region never closes"));
    }
    out
}

/// Every token the code holds, with its current values.
pub fn code_tokens(root: &Path) -> Result<Vec<CodeToken>, Vec<String>> {
    let mut problems = Vec::new();
    let mut tokens: Vec<CodeToken> = Vec::new();
    for file in FILES {
        let text = std::fs::read_to_string(root.join(file)).map_err(|e| vec![format!("{file}: {e}")])?;
        let mut found = Vec::new();
        walk(
            file,
            &text,
            |collection, mode, line| {
                let mut add = |name: &str, mode: &str, value: Value, ty: Option<String>, source: Option<String>| match tokens
                    .iter_mut()
                    .find(|t| t.collection == collection && t.name == name)
                {
                    Some(t) => t.values.push((mode.to_string(), value)),
                    None => tokens.push(CodeToken {
                        collection: collection.to_string(),
                        name: name.to_string(),
                        values: vec![(mode.to_string(), value)],
                        ty,
                        source,
                    }),
                };
                match line {
                    Line::Colour { name, value, .. } => match mode {
                        Some(m) => add(name, m, Value::Colour(*value), None, None),
                        None => found.push(format!("{file}: the {collection} region of `{name}` names no mode")),
                    },
                    Line::Metric { name, ty, value, .. } => add(name, SINGLE_MODE, Value::Number(*value), Some(ty.clone()), None),
                    Line::Role { name, source, light, dark, .. } => {
                        let [r, g, b] = *light;
                        add(name, "Light", Value::Colour([r, g, b, 255]), None, Some(source.clone()));
                        let [r, g, b] = *dark;
                        add(name, "Dark", Value::Colour([r, g, b, 255]), None, None);
                        let src = if name == "line" { Some(LINE_SOURCE) } else { hex(source) };
                        match src {
                            Some(c) => add(name, SOURCE_MODE, Value::Colour(c), None, None),
                            None => found.push(format!("{file}: icon role `{name}`: `{source}` is not #rrggbb")),
                        }
                    }
                }
                None
            },
            &mut problems,
        );
        problems.extend(found);
    }
    if problems.is_empty() { Ok(tokens) } else { Err(problems) }
}

pub fn hex(s: &str) -> Option<[u8; 4]> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 && s.len() != 8 {
        return None;
    }
    let byte = |i: usize| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?, if s.len() == 8 { byte(6)? } else { 255 }])
}

pub fn hex_of(c: [u8; 4]) -> String {
    if c[3] == 255 { format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]) } else { format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]) }
}

/// The design file's variables: (collection, name) → value in each mode, by mode name.
pub struct Variables(pub BTreeMap<(String, String), BTreeMap<String, Value>>);

impl Variables {
    pub fn get(&self, collection: &str, name: &str, mode: &str) -> Option<Value> {
        self.0.get(&(collection.to_string(), name.to_string())).and_then(|m| m.get(mode)).copied()
    }
}

fn json_colour(v: &Json) -> Option<[u8; 4]> {
    let channel = |k: &str| {
        let x = v.get(k)?.as_f64()?;
        (x.is_finite() && (0.0..=1.0).contains(&x)).then(|| (x * 255.0).round() as u8)
    };
    Some([channel("r")?, channel("g")?, channel("b")?, v.get("a").map_or(Some(255), |_| channel("a"))?])
}

/// Read FigCraft's local variables JSON (`variableCollections` and `variables`, under `meta` or
/// at the top level). Aliases are followed to the value they show.
pub fn read_variables(text: &str) -> Result<Variables, String> {
    let json: Json = serde_json::from_str(text).map_err(|e| format!("{TOKENS}: {e}"))?;
    let meta = json.get("meta").unwrap_or(&json);
    let collections = meta.get("variableCollections").and_then(Json::as_object).ok_or(format!("{TOKENS}: no variableCollections"))?;
    let variables = meta.get("variables").and_then(Json::as_object).ok_or(format!("{TOKENS}: no variables"))?;
    // collection id → (name, mode id → mode name, default mode id)
    let mut cols: BTreeMap<&str, (&str, BTreeMap<&str, &str>, &str)> = BTreeMap::new();
    for (id, c) in collections {
        let name = c.get("name").and_then(Json::as_str).unwrap_or_default();
        let modes = c
            .get("modes")
            .and_then(Json::as_array)
            .map(|ms| ms.iter().filter_map(|m| Some((m.get("modeId")?.as_str()?, m.get("name")?.as_str()?))).collect())
            .unwrap_or_default();
        let default = c.get("defaultModeId").and_then(Json::as_str).unwrap_or_default();
        cols.insert(id.as_str(), (name, modes, default));
    }
    // A value in a mode, aliases followed (`depth` stops a loop).
    fn resolve<'a>(
        variables: &'a serde_json::Map<String, Json>,
        cols: &BTreeMap<&str, (&str, BTreeMap<&str, &str>, &str)>,
        var: &'a Json,
        mode_name: &str,
        depth: u32,
    ) -> Option<Value> {
        let col = cols.get(var.get("variableCollectionId")?.as_str()?)?;
        let mode_id = col.1.iter().find(|(_, n)| **n == mode_name).map(|(id, _)| *id).unwrap_or(col.2);
        let v = var.get("valuesByMode")?.get(mode_id)?;
        if v.get("type").and_then(Json::as_str) == Some("VARIABLE_ALIAS") {
            if depth > 16 {
                return None;
            }
            let target = variables.get(v.get("id")?.as_str()?)?;
            return resolve(variables, cols, target, mode_name, depth + 1);
        }
        match var.get("resolvedType")?.as_str()? {
            "COLOR" => json_colour(v).map(Value::Colour),
            "FLOAT" => v.as_f64().filter(|x| x.is_finite()).map(Value::Number),
            _ => None,
        }
    }
    let mut out = BTreeMap::new();
    for var in variables.values() {
        let Some(col) = var.get("variableCollectionId").and_then(Json::as_str).and_then(|id| cols.get(id)) else { continue };
        let Some(name) = var.get("name").and_then(Json::as_str) else { continue };
        let values: BTreeMap<String, Value> =
            col.1.values().filter_map(|mode| resolve(variables, &cols, var, mode, 0).map(|v| (mode.to_string(), v))).collect();
        out.insert((col.0.to_string(), name.to_string()), values);
    }
    Ok(Variables(out))
}

fn lookup_colour(vars: &Variables, file: &str, collection: &str, name: &str, mode: &str) -> Result<[u8; 4], String> {
    match vars.get(collection, name, mode) {
        Some(Value::Colour(c)) => Ok(c),
        Some(Value::Number(_)) => Err(format!("{TOKENS}: {collection} `{name}` is a number, {file} wants a colour")),
        None => Err(format!("{TOKENS} has no {collection} variable `{name}` in mode {mode}, which {file} uses")),
    }
}

fn lookup_metric(vars: &Variables, file: &str, collection: &str, name: &str, ty: &str) -> Result<String, String> {
    let v = match vars.get(collection, name, SINGLE_MODE).or_else(|| first_mode(vars, collection, name)) {
        Some(Value::Number(v)) => v,
        Some(Value::Colour(_)) => return Err(format!("{TOKENS}: {collection} `{name}` is a colour, {file} wants a number")),
        None => return Err(format!("{TOKENS} has no {collection} variable `{name}`, which {file} uses")),
    };
    match ty {
        "f32" => Ok(format!("{:?}", v as f32)),
        "u8" if v.fract() == 0.0 && (0.0..=255.0).contains(&v) => Ok(format!("{}", v as u8)),
        _ => Err(format!("{TOKENS}: {collection} `{name}` = {v} does not fit {file}'s {ty}")),
    }
}

/// The line `line` becomes with the design file's values.
fn rewrite_line(vars: &Variables, file: &str, collection: &str, mode: Option<&str>, line: &Line) -> Result<String, String> {
    match line {
        Line::Colour { indent, name, .. } => {
            let c = lookup_colour(vars, file, collection, name, mode.unwrap_or(SINGLE_MODE))?;
            Ok(format!("{indent}{name}: {},", format_colour(c)))
        }
        Line::Metric { indent, name, ty, .. } => {
            Ok(format!("{indent}pub const {name}: {ty} = {};", lookup_metric(vars, file, collection, name, ty)?))
        }
        Line::Role { indent, name, source, .. } => {
            let light = lookup_colour(vars, file, collection, name, "Light")?;
            let dark = lookup_colour(vars, file, collection, name, "Dark")?;
            if light[3] != 255 || dark[3] != 255 {
                return Err(format!("{TOKENS}: icon role `{name}` must be opaque"));
            }
            let rgb = |c: [u8; 4]| [c[0], c[1], c[2]];
            Ok(format!("{indent}(\"{name}\", \"{source}\", {}, {}),", format_bytes(rgb(light)), format_bytes(rgb(dark))))
        }
    }
}

fn line_name(line: &Line) -> &str {
    match line {
        Line::Colour { name, .. } | Line::Metric { name, .. } | Line::Role { name, .. } => name,
    }
}

/// The code's regions rewritten with the design file's values, file by file; or why not.
fn generate(root: &Path, vars: &Variables) -> Result<Vec<(&'static str, String, String)>, Vec<String>> {
    let mut problems = Vec::new();
    let mut used: BTreeSet<(String, String)> = BTreeSet::new();
    let mut files = Vec::new();
    for file in FILES {
        let text = std::fs::read_to_string(root.join(file)).map_err(|e| vec![format!("{file}: {e}")])?;
        let mut local = Vec::new();
        let mut found = Vec::new();
        let new = walk(
            file,
            &text,
            |collection, mode, line| {
                used.insert((collection.to_string(), line_name(line).to_string()));
                match rewrite_line(vars, file, collection, mode, line) {
                    Ok(new) => Some(new),
                    Err(e) => {
                        found.push(e);
                        None
                    }
                }
            },
            &mut local,
        );
        problems.extend(local);
        problems.extend(found);
        files.push((*file, text, new));
    }
    for (collection, name) in vars.0.keys() {
        if CODE_COLLECTIONS.contains(&collection.as_str()) && !used.contains(&(collection.clone(), name.clone())) {
            eprintln!("  note: {TOKENS}: {collection} `{name}` is not used by the code yet (add its field or constant, then rerun)");
        }
    }
    if problems.is_empty() { Ok(files) } else { Err(problems) }
}

/// The collections the code takes values from.
pub const CODE_COLLECTIONS: &[&str] = &["Colour", "Icon", "Metrics"];

fn first_mode(vars: &Variables, collection: &str, name: &str) -> Option<Value> {
    vars.0.get(&(collection.to_string(), name.to_string())).and_then(|m| m.values().next()).copied()
}

/// Write the design file's values into the code (`check`: only say whether the code already
/// holds them). Refuses, writing nothing, when the code uses a variable the design file lacks.
pub fn design_tokens(root: &Path, check: bool) -> Vec<String> {
    let text = match std::fs::read_to_string(root.join(TOKENS)) {
        Ok(t) => t,
        Err(e) => return vec![format!("{TOKENS}: {e} (cargo xtask design-pull exports it from the design file)")],
    };
    let vars = match read_variables(&text) {
        Ok(v) => v,
        Err(e) => return vec![e],
    };
    let files = match generate(root, &vars) {
        Ok(f) => f,
        Err(problems) => {
            let mut p = problems;
            p.push("nothing written: give the design file these variables, or remove them from the code".into());
            return p;
        }
    };
    let mut problems = Vec::new();
    for (file, old, new) in files {
        if old == new {
            continue;
        }
        if check {
            problems.push(format!("{file} does not hold {TOKENS}'s values: run `cargo xtask design-tokens`"));
        } else if let Err(e) = std::fs::write(root.join(file), new) {
            problems.push(format!("{file}: {e}"));
        } else {
            eprintln!("  wrote {file}");
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "pub mod metrics {\n    // design-tokens: Metrics\n    /// Doc.\n    pub const SPACE_2: f32 = 2.0;\n    pub const RADIUS_SM: u8 = 4;\n    // design-tokens: end\n}\nfn x() {\n    // design-tokens: Colour/Dark\n    panel: Color32::from_rgb(39, 43, 52),\n    backdrop: Color32::from_black_alpha(120),\n    // design-tokens: end\n}\nconst ROLES: &[(&str, &str, [u8; 3], [u8; 3])] = &[\n    // design-tokens: Icon\n    (\"ink\", \"#34465b\", [0x34, 0x46, 0x5b], [0x1f, 0x20, 0x21]),\n    // design-tokens: end\n];\n";

    fn vars(entries: &[(&str, &str, &str, Value)]) -> Variables {
        let mut m: BTreeMap<(String, String), BTreeMap<String, Value>> = BTreeMap::new();
        for (c, n, mode, v) in entries {
            m.entry((c.to_string(), n.to_string())).or_default().insert(mode.to_string(), *v);
        }
        Variables(m)
    }

    fn rewrite(v: &Variables) -> (String, Vec<String>) {
        let mut problems = Vec::new();
        let out = walk(
            "x.rs",
            SAMPLE,
            |collection, mode, line| match line {
                Line::Colour { indent, name, .. } => match v.get(collection, name, mode.unwrap_or(SINGLE_MODE)) {
                    Some(Value::Colour(c)) => Some(format!("{indent}{name}: {},", format_colour(c))),
                    _ => None,
                },
                _ => None,
            },
            &mut problems,
        );
        (out, problems)
    }

    #[test]
    fn every_form_the_theme_uses_parses() {
        assert_eq!(parse_colour("Color32::from_rgb(0x3a, 0x3f, 71)"), Some([0x3a, 0x3f, 71, 255]));
        assert_eq!(parse_colour("Color32::from_black_alpha(120)"), Some([0, 0, 0, 120]));
        assert_eq!(parse_colour("Color32::from_gray(235)"), Some([235, 235, 235, 255]));
        assert_eq!(parse_colour("Color32::WHITE"), Some([255; 4]));
        assert_eq!(parse_colour("Color32::from_rgba_unmultiplied(255, 255, 255, 28)"), Some([255, 255, 255, 28]));
        assert_eq!(parse_colour("Color32::from_rgb(256, 0, 0)"), None);
    }

    #[test]
    fn regions_rewrite_only_their_values() {
        let v = vars(&[("Colour", "panel", "Dark", Value::Colour([1, 2, 3, 255])), ("Colour", "backdrop", "Dark", Value::Colour([0, 0, 0, 120]))]);
        let (out, problems) = rewrite(&v);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(out.contains("    panel: Color32::from_rgb(0x01, 0x02, 0x03),\n"), "{out}");
        assert!(out.contains("    backdrop: Color32::from_rgba_unmultiplied(0x00, 0x00, 0x00, 0x78),\n"), "{out}");
        assert!(out.contains("    /// Doc.\n    pub const SPACE_2: f32 = 2.0;"), "the metrics stay: {out}");
    }

    #[test]
    fn a_region_left_open_is_a_problem() {
        let mut problems = Vec::new();
        walk("x.rs", "// design-tokens: Colour/Dark\npanel: Color32::WHITE,\n", |_, _, _| None, &mut problems);
        assert_eq!(problems.len(), 1, "{problems:?}");
        let mut problems = Vec::new();
        walk("x.rs", "// design-tokens: Colour/Dark\nwhat is this\n// design-tokens: end\n", |_, _, _| None, &mut problems);
        assert_eq!(problems.len(), 1, "{problems:?}");
    }

    #[test]
    fn tokens_json_values_and_aliases_resolve() {
        let text = r##"{"meta":{"variableCollections":{"C:1":{"name":"Colour","defaultModeId":"C:1/m1","modes":[{"modeId":"C:1/m1","name":"Light"},{"modeId":"C:1/m2","name":"Dark"}]},
            "C:2":{"name":"Metrics","defaultModeId":"C:2/m1","modes":[{"modeId":"C:2/m1","name":"Value"}]}},
            "variables":{"V:1":{"name":"panel","variableCollectionId":"C:1","resolvedType":"COLOR","valuesByMode":{"C:1/m1":{"r":1.0,"g":0.5,"b":0.0,"a":1.0},"C:1/m2":{"type":"VARIABLE_ALIAS","id":"V:2"}}},
            "V:2":{"name":"field","variableCollectionId":"C:1","resolvedType":"COLOR","valuesByMode":{"C:1/m1":{"r":0,"g":0,"b":0,"a":1},"C:1/m2":{"r":0.0,"g":0.0,"b":1.0,"a":0.5}}},
            "V:3":{"name":"SPACE_2","variableCollectionId":"C:2","resolvedType":"FLOAT","valuesByMode":{"C:2/m1":2.5}},
            "V:4":{"name":"loop","variableCollectionId":"C:2","resolvedType":"FLOAT","valuesByMode":{"C:2/m1":{"type":"VARIABLE_ALIAS","id":"V:4"}}}}}}"##;
        let v = read_variables(text).unwrap();
        assert_eq!(v.get("Colour", "panel", "Light"), Some(Value::Colour([255, 128, 0, 255])));
        assert_eq!(v.get("Colour", "panel", "Dark"), Some(Value::Colour([0, 0, 255, 128])), "the alias shows its target");
        assert_eq!(v.get("Metrics", "SPACE_2", "Value"), Some(Value::Number(2.5)));
        assert_eq!(v.get("Metrics", "loop", "Value"), None, "a loop resolves to nothing rather than hanging");
        assert!(read_variables("{").is_err() && read_variables("{}").is_err());
    }
}
