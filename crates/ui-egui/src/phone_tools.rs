//! All tools: every command of the engine, reachable on a phone.
//!
//! The six bottom sheets (`phone_sheet`) are the everyday front page. This is the rest: one screen per
//! ribbon tab, built from the engine's own command list (`command_specs`), so a command the engine gains
//! shows up here without a hand-kept table. A row runs its command; a row that needs a choice opens a
//! picker fed by the engine's galleries (themes, transitions, animations, shapes, fonts, slide sizes…); a
//! row that needs typed values opens a small form; Search finds any command by name or id.
//!
//! Back (and Esc) closes one level: form or picker, then the tab, then the list of tabs.
//!
//! Only plain words and icons from `icons.rs` are drawn: a glyph the bundled fonts lack panics when laid out.

use deckcraft_engine::cmd::{CommandSpec, Param, command_specs, needs_arguments, parse_params};
use egui::{Align2, CornerRadius, Rect, Sense, Stroke, Ui, WidgetInfo, pos2, vec2};
use serde_json::{Value, json};

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// The ribbon and contextual tabs, in the order they are listed. Commands whose menu is one of the
/// aliases in [`tab_of`] are folded into these.
pub const TABS: [&str; 20] = [
    "Home",
    "Insert",
    "Design",
    "Transitions",
    "Animations",
    "Slide Show",
    "Review",
    "View",
    "Shape Format",
    "Picture Format",
    "Table Design",
    "Layout",
    "SmartArt Design",
    "Chart Design",
    "Playback",
    "Slide Master",
    "Handout Master",
    "Notes Master",
    "File",
    "Other",
];

/// Tabs shown only while the selection calls for them (see `ribbon::contextual`).
const CONTEXTUAL: [&str; 9] =
    ["Shape Format", "Picture Format", "Table Design", "Layout", "SmartArt Design", "Chart Design", "Playback", "Handout Master", "Notes Master"];

/// Height of a list row. Over the 48 pt touch minimum.
const ROW_H: f32 = 52.0;
/// The most of the window the tools take.
const MAX_FRACTION: f32 = 0.56;
/// Where a tap-to-insert shape goes: the middle of a standard slide, ready to drag and resize.
const DEFAULT_RECT: [f64; 4] = [380.0, 200.0, 200.0, 140.0];

/// One screen of the tools. The stack in `SlideApp::phone_tools` is the way back.
#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    /// The list of tabs, and Search.
    Tabs,
    /// Search every command.
    Search(String),
    /// One ribbon tab's commands.
    Tab(&'static str),
    /// A choice for one command.
    Pick(Pick),
    /// Values to type for one command.
    Input(Input),
}

/// A choice: each option is the parameters to run the command with.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub id: &'static str,
    pub title: String,
    pub options: Vec<(Value, String)>,
}

/// Values to type: (name, what the doc says of it, the text so far).
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub id: &'static str,
    pub title: String,
    pub fields: Vec<(String, String, String)>,
    /// Parameters that are always sent (a default rectangle).
    pub fixed: Value,
}

/// What a command's row does when tapped.
enum Act {
    Run,
    Pick(Pick),
    Input(Input),
    /// Needs a file or other value the phone cannot type; the row says where to go instead.
    Elsewhere(&'static str),
}

/// The tab a command is listed under, or `Other`. Menu-only groups are folded into a tab.
pub fn tab_of(spec: &CommandSpec) -> &'static str {
    let Some(first) = spec.menu.first() else { return "Other" };
    let m = match *first {
        "Arrange" | "Format Shape" => "Shape Format",
        "Edit" => "Home",
        "Selection Pane" => "View",
        "Table Layout" => "Layout",
        other => other,
    };
    TABS.iter().copied().find(|t| *t == m && *t != "Other").unwrap_or("Other")
}

/// The words a tab is called.
pub fn tab_label(tab: &str) -> &str {
    match tab {
        "SmartArt Design" => "Diagram Design",
        "Layout" => "Table Layout",
        other => other,
    }
}

/// Commands that read or answer (queries), not act: they have no row.
fn is_action(spec: &CommandSpec) -> bool {
    spec.journal
}

/// Commands listed under `tab`, in the engine's order.
pub fn tab_specs(tab: &str) -> Vec<&'static CommandSpec> {
    command_specs().iter().filter(|s| is_action(s) && tab_of(s) == tab).collect()
}

/// The tabs to list now: the ribbon's own, and a contextual one while the selection calls for it.
pub fn available_tabs(app: &SlideApp) -> Vec<&'static str> {
    let contextual = crate::ribbon::contextual(app);
    TABS.iter().copied().filter(|t| (!CONTEXTUAL.contains(t) || contextual.contains(t)) && !tab_specs(t).is_empty()).collect()
}

// ---------------------------------------------------------------- what a row does

/// The required parameters of `spec` (those in its first alternative that are not optional).
fn required<'a>(params: &'a [Param<'a>]) -> Vec<&'a Param<'a>> {
    params.iter().filter(|p| p.object == 0 && !p.optional && !p.or).collect()
}

/// Options spelled out in a params doc: `a|b|c` when every one is a plain word and the list is not cut short (`…`).
fn spelled_out(about: &str) -> Option<Vec<String>> {
    if about.contains('…') || about.contains("...") {
        return None;
    }
    let first = about.split([' ', ',', '}', '(']).next().unwrap_or("");
    let parts: Vec<&str> = first.split('|').collect();
    let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    (parts.len() >= 2 && parts.iter().all(|p| plain(p))).then(|| parts.into_iter().map(str::to_string).collect())
}

/// Words made readable: `byParagraph` becomes "By paragraph", `medium2-accent1` "Medium2 accent1".
fn readable(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c == '-' || c == '_' {
            out.push(' ');
        } else if c.is_uppercase() && i > 0 {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else if i == 0 {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn query(app: &mut SlideApp, id: &str) -> Value {
    app.session.execute(id, &json!({})).unwrap_or(Value::Null)
}

/// Names from a list in a query's answer: `key` of the answer, each item a string or an object with `name`.
fn names(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().or_else(|| x.get("name").and_then(Value::as_str)).map(str::to_string)).collect())
        .unwrap_or_default()
}

/// `{param: value}` for each of `values`, labelled by the value itself.
fn simple(param: &str, values: Vec<String>) -> Vec<(Value, String)> {
    values.into_iter().map(|v| (json!({ param: v }), v)).collect()
}

/// The options the engine offers for `param` of command `id`, when it offers a list.
pub fn choices(app: &mut SlideApp, id: &str, param: &str, about: &str) -> Option<Vec<(Value, String)>> {
    let list = match (id, param) {
        ("design.theme", "name") => {
            let v = query(app, "design.themes");
            simple("name", names(&v, "themes"))
        }
        ("design.colors", "name") => {
            let v = query(app, "design.themes");
            simple("name", names(&v, "colorSchemes"))
        }
        ("design.fonts", "name") => {
            let v = query(app, "design.themes");
            simple("name", names(&v, "fontSchemes"))
        }
        ("design.effects", "name") => {
            let v = query(app, "design.themes");
            simple("name", names(&v, "effectSchemes"))
        }
        ("design.variant", "index") => {
            let v = query(app, "design.variants");
            let n = names(&v, "variants");
            n.into_iter().enumerate().map(|(i, name)| (json!({"index": i}), name)).collect()
        }
        ("design.slideSize", "preset") => {
            deckcraft_model::defaults::SLIDE_SIZES.iter().map(|(label, w, h)| (json!({"w": w, "h": h}), label.to_string())).collect()
        }
        ("transition.set", "kind") => {
            let v = query(app, "transition.list");
            v.as_array()
                .map(|a| a.iter().filter_map(|x| Some((json!({"kind": x.get("id")?}), x.get("label")?.as_str()?.to_string()))).collect())
                .unwrap_or_default()
        }
        ("animation.set" | "animation.add", "effect") => {
            let v = query(app, "animation.list");
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| {
                            let class = x.get("class").and_then(Value::as_str).unwrap_or("");
                            Some((json!({"effect": x.get("id")?}), format!("{} ({})", x.get("label")?.as_str()?, readable(class))))
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        ("shape.insert", "preset") => {
            let v = query(app, "shape.presets");
            v.as_array()
                .map(|a| a.iter().filter_map(|x| Some((json!({"preset": x.get("name")?}), x.get("label")?.as_str()?.to_string()))).collect())
                .unwrap_or_default()
        }
        ("picture.style", "style") => {
            let v = query(app, "picture.styles");
            v.as_array()
                .map(|a| a.iter().filter_map(|x| Some((json!({"style": x.get("id")?}), x.get("label")?.as_str()?.to_string()))).collect())
                .unwrap_or_default()
        }
        ("insert.chart" | "chart.type", "type") => {
            deckcraft_model::chart::ChartType::MAIN.iter().map(|k| (json!({"type": format!("{k:?}")}), k.label().to_string())).collect()
        }
        ("format.font", "family") => simple("family", crate::ribbon::font_families(app)),
        ("format.size", "size") => deckcraft_engine::cmd::format::SIZES.iter().map(|s| (json!({"size": s}), format!("{s} pt"))).collect(),
        _ => spelled_out(about).map(|v| v.into_iter().map(|s| (json!({ param: s.clone() }), readable(&s))).collect())?,
    };
    (!list.is_empty()).then_some(list)
}

/// Parameters a command is always given because the phone has no way to ask for them.
fn fixed(id: &str, param: &str) -> Option<Value> {
    match (id, param) {
        ("shape.insert", "rect") => Some(json!(DEFAULT_RECT)),
        _ => None,
    }
}

/// What tapping `spec` does now.
fn plan(app: &mut SlideApp, spec: &'static CommandSpec) -> Act {
    // Theme colours and fonts open a dialog when run with no value; on a phone a list of the schemes is easier.
    if matches!(spec.id, "design.colors" | "design.fonts")
        && let Some(options) = choices(app, spec.id, "name", "")
    {
        return Act::Pick(Pick { id: spec.id, title: spec.label.to_string(), options });
    }
    if !needs_arguments(spec.params) {
        return Act::Run;
    }
    let Ok(params) = parse_params(spec.params) else { return Act::Elsewhere("Use the File menu or the command search") };
    let req = required(&params);
    if req.iter().any(|p| matches!(p.name, "path" | "data")) {
        return Act::Elsewhere("Needs a file: use the Insert sheet or the File menu");
    }
    let mut fixed_params = json!({});
    let open: Vec<&&Param> = req
        .iter()
        .filter(|p| {
            if let Some(v) = fixed(spec.id, p.name) {
                fixed_params[p.name] = v;
                false
            } else {
                true
            }
        })
        .collect();
    if let [one] = open.as_slice()
        && let Some(mut options) = choices(app, spec.id, one.name, one.about)
    {
        if let Some(extra) = fixed_params.as_object().filter(|o| !o.is_empty()) {
            for (o, _) in &mut options {
                if let Some(map) = o.as_object_mut() {
                    map.extend(extra.clone());
                }
            }
        }
        return Act::Pick(Pick { id: spec.id, title: spec.label.to_string(), options });
    }
    let fields = open.iter().map(|p| (p.name.to_string(), p.about.to_string(), default_text(spec.id, p.name))).collect();
    Act::Input(Input { id: spec.id, title: spec.label.to_string(), fields, fixed: fixed_params })
}

/// What a field starts with.
fn default_text(id: &str, param: &str) -> String {
    match (id, param) {
        ("insert.table", "rows" | "cols") => "3".into(),
        _ => String::new(),
    }
}

/// A typed value as JSON: a number, true/false, a list or object when it reads as one, else text.
pub fn parse_value(text: &str) -> Value {
    let t = text.trim();
    serde_json::from_str::<Value>(t).unwrap_or_else(|_| Value::String(t.to_string()))
}

/// Runs a command, telling the person if it failed. True when it ran.
fn run_command(app: &mut SlideApp, id: &str, params: Value) -> bool {
    match app.run(id, params) {
        Ok(_) => true,
        Err(e) => {
            let label = deckcraft_engine::cmd::find_command(id).map_or(id, |s| s.label);
            app.set_status(format!("{label}: {e}"));
            false
        }
    }
}

// ---------------------------------------------------------------- drawing

/// Open the tools on the list of tabs.
pub fn open(app: &mut SlideApp) {
    app.phone_sheet = None;
    app.phone_tools = vec![Tool::Tabs];
}

/// Closes the top screen. True when there was one to close.
pub fn back(app: &mut SlideApp) -> bool {
    app.phone_tools.pop().is_some()
}

/// A full-width list row: a label with an optional second line and a chevron. True when tapped.
fn list_row(ui: &mut Ui, label: &str, sub: Option<&str>, chevron: bool, enabled: bool, on: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let h = if sub.is_some() { ROW_H + 8.0 } else { ROW_H };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect.shrink2(vec2(4.0, 1.0)), CornerRadius::same(10), t.pressed);
    } else if on {
        ui.painter().rect_filled(rect.shrink2(vec2(4.0, 1.0)), CornerRadius::same(10), t.selected);
    }
    let ink = if enabled { t.text } else { t.text.gamma_multiply(0.5) };
    let p = ui.painter().with_clip_rect(rect.shrink2(vec2(0.0, 0.0)));
    let right = if chevron { 40.0 } else { 12.0 };
    let _ = right;
    let y = if sub.is_some() { rect.center().y - 10.0 } else { rect.center().y };
    p.text(pos2(rect.left() + 16.0, y), Align2::LEFT_CENTER, label, theme::font(15.0), ink);
    if let Some(s) = sub {
        p.text(pos2(rect.left() + 16.0, rect.center().y + 12.0), Align2::LEFT_CENTER, s, theme::font(12.0), t.text.gamma_multiply(0.6));
    }
    if chevron {
        icons::paint(&p, Rect::from_center_size(pos2(rect.right() - 24.0, rect.center().y), vec2(20.0, 20.0)), Icon::ChevronRight, ink, !enabled);
    }
    resp.clicked()
}

fn section(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    ui.painter().text(pos2(rect.left() + 16.0, rect.center().y + 4.0), Align2::LEFT_CENTER, text, theme::font(12.0), t.accent);
}

fn title_of(tool: &Tool) -> String {
    match tool {
        Tool::Tabs => "All tools".into(),
        Tool::Search(_) => "Search".into(),
        Tool::Tab(t) => tab_label(t).to_string(),
        Tool::Pick(p) => p.title.clone(),
        Tool::Input(i) => i.title.clone(),
    }
}

/// What a tap on `spec` does: runs it, or opens a picker or a form.
fn tap(app: &mut SlideApp, spec: &'static CommandSpec) {
    if let Err(reason) = (spec.enabled)(&app.session) {
        app.set_status(format!("{}: {reason}", spec.label));
        return;
    }
    match plan(app, spec) {
        Act::Run => {
            if run_command(app, spec.id, json!({})) {
                // A command that opens a dialog or changes the slide: show the slide.
                app.phone_tools.clear();
            }
        }
        Act::Pick(p) => app.phone_tools.push(Tool::Pick(p)),
        Act::Input(i) => app.phone_tools.push(Tool::Input(i)),
        Act::Elsewhere(msg) => app.set_status(msg),
    }
}

/// A command's row. True when tapped.
fn command_row(app: &SlideApp, ui: &mut Ui, spec: &'static CommandSpec) -> bool {
    let reason = (spec.enabled)(&app.session).err();
    let more = needs_arguments(spec.params);
    list_row(ui, spec.label, reason.as_deref(), more, reason.is_none(), false)
}

/// The tools panel, above the tab bar. Draws nothing when the tools are closed.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let Some(mut tool) = app.phone_tools.pop() else { return };
    let t = Tokens::get(ui.ctx());
    let window_h = ui.ctx().content_rect().height();
    let height = (window_h * MAX_FRACTION).max(240.0);
    let mut close = false;
    let mut back_one = false;
    let mut next: Option<Tool> = None;
    let mut ran: Option<(&'static str, Value)> = None;
    egui::Panel::bottom("phone_tools").exact_size(height).frame(egui::Frame::NONE.fill(t.card)).show(ui, |ui| {
        ui.painter().hline(ui.max_rect().x_range(), ui.max_rect().min.y, Stroke::new(1.0, t.border));
        // Header: back, title, close.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let btn = |ui: &mut Ui, icon: Icon, label: &str| {
                let (r, resp) = ui.allocate_exact_size(vec2(52.0, 52.0), Sense::click());
                resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, label));
                if resp.is_pointer_button_down_on() {
                    ui.painter().circle_filled(r.center(), 22.0, t.pressed);
                }
                icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(24.0, 24.0)), icon, t.text, false);
                resp.clicked()
            };
            if btn(ui, Icon::ChevronLeft, "Back") {
                back_one = true;
            }
            let w = (ui.available_width() - 52.0).max(0.0);
            let (r, _) = ui.allocate_exact_size(vec2(w, 52.0), Sense::hover());
            ui.painter().with_clip_rect(r).text(pos2(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER, title_of(&tool), theme::bold(17.0), t.text);
            if btn(ui, Icon::Close, "Close") {
                close = true;
            }
        });
        egui::ScrollArea::vertical().id_salt("phone_tools_scroll").auto_shrink([false, false]).show(ui, |ui| match &mut tool {
            Tool::Tabs => {
                if list_row(ui, "Search all commands", None, true, true, false) {
                    next = Some(Tool::Search(String::new()));
                }
                section(ui, "Ribbon tabs");
                for tab in available_tabs(app) {
                    let n = tab_specs(tab).len();
                    if list_row(ui, &format!("{}  ({n})", tab_label(tab)), None, true, true, false) {
                        next = Some(Tool::Tab(tab));
                    }
                }
            }
            Tool::Search(q) => {
                let r = ui.add(egui::TextEdit::singleline(q).hint_text("Search commands").desired_width(f32::INFINITY).font(theme::font(16.0)));
                let _ = r;
                let needle = q.trim().to_lowercase();
                let mut shown = 0;
                for spec in command_specs().iter().filter(|s| is_action(s)) {
                    if needle.is_empty() || !(spec.label.to_lowercase().contains(&needle) || spec.id.to_lowercase().contains(&needle)) {
                        continue;
                    }
                    shown += 1;
                    if shown > 80 {
                        break;
                    }
                    if command_row(app, ui, spec) {
                        ran = Some((spec.id, Value::Null));
                    }
                }
                if needle.is_empty() {
                    section(ui, "Type a name, such as fade, table or theme");
                }
            }
            Tool::Tab(tab) => {
                let mut group: Option<&str> = None;
                for spec in tab_specs(tab) {
                    let g = spec.menu.get(1).copied();
                    if g != group {
                        group = g;
                        if let Some(name) = g {
                            section(ui, name);
                        }
                    }
                    if command_row(app, ui, spec) {
                        ran = Some((spec.id, Value::Null));
                    }
                }
            }
            Tool::Pick(p) => {
                for (params, label) in &p.options {
                    if list_row(ui, label, None, false, true, false) {
                        ran = Some((p.id, params.clone()));
                    }
                }
            }
            Tool::Input(i) => {
                for (name, about, value) in &mut i.fields {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(readable(name)).font(theme::font(13.0)).color(t.text.gamma_multiply(0.7)));
                    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY).font(theme::font(16.0)).hint_text(about.as_str()));
                }
                ui.add_space(14.0);
                let b = ui.add(
                    egui::Button::new(egui::RichText::new("Run").color(t.accent_text).size(16.0))
                        .fill(t.accent)
                        .min_size(vec2(ui.available_width(), 52.0)),
                );
                if b.clicked() {
                    let mut params = i.fixed.clone();
                    for (name, _, value) in &i.fields {
                        if !value.trim().is_empty() {
                            params[name] = parse_value(value);
                        }
                    }
                    ran = Some((i.id, params));
                }
            }
        });
    });
    // What was tapped, applied after drawing.
    if let Some((id, params)) = ran {
        if params.is_null() {
            if let Some(spec) = deckcraft_engine::cmd::find_command(id) {
                app.phone_tools.push(tool.clone());
                tap(app, spec);
                // `tap` may have cleared the tools (it ran) or pushed a picker or form; keep the screen we were on.
            }
            return finish(app, close, back_one, next);
        }
        if run_command(app, id, params) {
            let label = deckcraft_engine::cmd::find_command(id).map_or(id, |s| s.label);
            app.set_status(format!("{label}: done"));
            // Back to the tab it came from; a value typed or picked is applied.
            app.phone_tools.clear();
            return;
        }
        app.phone_tools.push(tool);
        return finish(app, close, back_one, next);
    }
    app.phone_tools.push(tool);
    finish(app, close, back_one, next);
}

/// Applies the header's and the lists' navigation after drawing.
fn finish(app: &mut SlideApp, close: bool, back_one: bool, next: Option<Tool>) {
    if close {
        app.phone_tools.clear();
    } else if back_one {
        let _ = back(app);
    } else if let Some(n) = next {
        app.phone_tools.push(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_sample() -> SlideApp {
        let mut app = SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
        deckcraft_engine::sample::open_sample(&mut app.session).expect("sample deck opens");
        app.fonts_ready = true;
        app.styled = true;
        app
    }

    fn select_first_shape(app: &mut SlideApp) {
        let id = app.session.active().and_then(|d| d.doc.slides.first().and_then(|s| s.shapes.first().map(|sh| sh.id.0)));
        if let Some(id) = id {
            let _ = app.run("edit.select", json!({"ids": [id]}));
        }
    }

    /// Every action command is listed under exactly one tab, so no engine command is out of reach.
    #[test]
    fn every_action_command_is_in_one_tab() {
        let mut seen = std::collections::HashSet::new();
        for tab in TABS {
            for s in tab_specs(tab) {
                assert!(seen.insert(s.id), "`{}` is listed under two tabs", s.id);
                assert!(!s.label.is_empty(), "`{}` has no label", s.id);
            }
        }
        let actions = command_specs().iter().filter(|s| is_action(s)).count();
        assert_eq!(seen.len(), actions, "every action command has a tab");
        assert!(actions > 250, "the engine has more than 250 action commands, found {actions}");
    }

    #[test]
    fn labels_are_plain_enough_to_draw() {
        // A glyph the bundled fonts lack panics when laid out: tab, command and group names stay ASCII-ish.
        let ok = |s: &str| s.chars().all(|c| c.is_ascii() || c == '…' || c == '\u{b0}' || c == '’');
        for s in command_specs().iter().filter(|s| is_action(s)) {
            assert!(ok(s.label), "label of `{}` has an unusual character: {}", s.id, s.label);
            for m in s.menu {
                assert!(ok(m), "menu of `{}` has an unusual character: {m}", s.id);
            }
        }
    }

    #[test]
    fn spelled_out_options_need_every_word_and_no_ellipsis() {
        assert_eq!(spelled_out("speaker|browsed|kiosk"), Some(vec!["speaker".to_string(), "browsed".into(), "kiosk".into()]));
        assert_eq!(spelled_out("a|b|… (see x)"), None, "a list cut short is not offered");
        assert_eq!(spelled_out("[x,y,w,h] pt"), None);
        assert_eq!(spelled_out("pt"), None);
    }

    #[test]
    fn typed_values_become_numbers_lists_or_words() {
        assert_eq!(parse_value("12"), json!(12));
        assert_eq!(parse_value(" true "), json!(true));
        assert_eq!(parse_value("[1,2]"), json!([1, 2]));
        assert_eq!(parse_value("hello"), json!("hello"));
    }

    #[test]
    fn readable_words() {
        assert_eq!(readable("byParagraph"), "By paragraph");
        assert_eq!(readable("medium2-accent1"), "Medium2 accent1");
        assert_eq!(readable("fade"), "Fade");
    }

    /// The galleries the phone had no way to reach list real options.
    #[test]
    fn the_gallery_commands_offer_options() {
        let mut app = app_with_sample();
        for (id, param) in [
            ("design.theme", "name"),
            ("design.colors", "name"),
            ("design.fonts", "name"),
            ("design.effects", "name"),
            ("design.variant", "index"),
            ("design.slideSize", "preset"),
            ("transition.set", "kind"),
            ("animation.set", "effect"),
            ("animation.add", "effect"),
            ("shape.insert", "preset"),
            ("picture.style", "style"),
            ("insert.chart", "type"),
            ("format.font", "family"),
            ("format.size", "size"),
        ] {
            let list = choices(&mut app, id, param, "");
            assert!(list.as_ref().is_some_and(|l| !l.is_empty()), "`{id}` {param} offers no options");
        }
    }

    /// Choosing the first and last option of the main galleries runs the command.
    #[test]
    fn choosing_from_a_gallery_runs_the_command() {
        for id in [
            "design.theme",
            "design.colors",
            "design.fonts",
            "design.effects",
            "design.variant",
            "design.slideSize",
            "transition.set",
            "animation.set",
            "animation.add",
            "shape.insert",
            "format.size",
        ] {
            for pick_last in [false, true] {
                let mut app = app_with_sample();
                select_first_shape(&mut app);
                let spec = deckcraft_engine::cmd::find_command(id).expect("a known command");
                let Act::Pick(p) = plan(&mut app, spec) else { panic!("`{id}` does not open a picker") };
                let (params, label) = if pick_last { p.options.last() } else { p.options.first() }.cloned().expect("an option");
                app.run(id, params.clone()).unwrap_or_else(|e| panic!("`{id}` {label} ({params}) failed: {e}"));
            }
        }
    }

    /// Commands that need no value run from a tap; those that need a choice open a picker or a form.
    #[test]
    fn a_row_runs_opens_a_picker_or_asks_for_values() {
        let mut app = app_with_sample();
        let kind = |app: &mut SlideApp, id: &str| match plan(app, deckcraft_engine::cmd::find_command(id).expect("known")) {
            Act::Run => "run",
            Act::Pick(_) => "pick",
            Act::Input(_) => "input",
            Act::Elsewhere(_) => "elsewhere",
        };
        assert_eq!(kind(&mut app, "slide.duplicate"), "run");
        assert_eq!(kind(&mut app, "transition.set"), "pick");
        assert_eq!(kind(&mut app, "design.theme"), "pick");
        assert_eq!(kind(&mut app, "insert.table"), "input");
        assert_eq!(kind(&mut app, "file.open"), "elsewhere");
    }

    /// A form's typed values run the command: a 2 by 4 table.
    #[test]
    fn a_form_runs_its_command_with_the_typed_values() {
        let mut app = app_with_sample();
        let spec = deckcraft_engine::cmd::find_command("insert.table").expect("known");
        let Act::Input(mut form) = plan(&mut app, spec) else { panic!("insert.table opens a form") };
        assert_eq!(form.fields.iter().map(|f| f.2.as_str()).collect::<Vec<_>>(), ["3", "3"], "rows and columns start at 3");
        let before = app.session.active().and_then(|d| d.doc.slides.get(d.selection.slide).map(|s| s.shapes.len())).unwrap_or(0);
        form.fields[0].2 = "2".into();
        form.fields[1].2 = "4".into();
        let mut params = form.fixed.clone();
        for (name, _, value) in &form.fields {
            params[name] = parse_value(value);
        }
        app.run("insert.table", params).expect("the table is inserted");
        let after = app.session.active().and_then(|d| d.doc.slides.get(d.selection.slide).map(|s| s.shapes.len())).unwrap_or(0);
        assert!(after > before, "a table was added");
    }

    #[test]
    fn back_closes_one_screen_at_a_time() {
        let mut app = app_with_sample();
        open(&mut app);
        app.phone_tools.push(Tool::Tab("Design"));
        assert!(app.can_go_back());
        assert!(back(&mut app) && matches!(app.phone_tools.last(), Some(Tool::Tabs)));
        assert!(back(&mut app) && app.phone_tools.is_empty());
        assert!(!back(&mut app), "nothing left to close");
    }

    /// Every tab, a picker, a form and the search draw at a phone's size, held both ways.
    #[test]
    fn every_screen_draws_at_phone_size() {
        let ctx = egui::Context::default();
        let mut app = app_with_sample();
        select_first_shape(&mut app);
        for size in [vec2(360.0, 640.0), vec2(640.0, 360.0)] {
            let mut screens = vec![Tool::Tabs, Tool::Search(String::new()), Tool::Search("fade".into())];
            screens.extend(TABS.iter().map(|t| Tool::Tab(t)));
            for spec_id in ["design.theme", "transition.set", "insert.table"] {
                let spec = deckcraft_engine::cmd::find_command(spec_id).expect("known");
                match plan(&mut app, spec) {
                    Act::Pick(p) => screens.push(Tool::Pick(p)),
                    Act::Input(i) => screens.push(Tool::Input(i)),
                    _ => {}
                }
            }
            for s in screens {
                app.phone_tools = vec![s];
                let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)), ..Default::default() };
                ctx.run_ui(raw, |ui| app.ui(ui)).drop_without_applying_deltas();
            }
        }
    }

    /// Writes PNGs of some tools screens at phone size into `$DECKCRAFT_SHOT_DIR`, for looking at the result.
    /// Does nothing when the variable is unset or no GPU renderer is available.
    #[test]
    fn tools_screenshots() {
        let Ok(dir) = std::env::var("DECKCRAFT_SHOT_DIR") else { return };
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(390.0, 800.0)).with_pixels_per_point(2.0).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default()),
        );
        harness.run_steps(2);
        let _ = deckcraft_engine::sample::open_sample(&mut harness.state_mut().session);
        harness.run_steps(2);
        let mut screens: Vec<(&str, Vec<Tool>)> = vec![
            ("tabs", vec![Tool::Tabs]),
            ("tab-design", vec![Tool::Tabs, Tool::Tab("Design")]),
            ("tab-transitions", vec![Tool::Tabs, Tool::Tab("Transitions")]),
            ("search", vec![Tool::Tabs, Tool::Search("font".into())]),
        ];
        for (name, id) in [("pick-theme", "design.theme"), ("pick-transition", "transition.set"), ("form-table", "insert.table")] {
            let app = harness.state_mut();
            let spec = deckcraft_engine::cmd::find_command(id).expect("known");
            let tool = match plan(app, spec) {
                Act::Pick(p) => Tool::Pick(p),
                Act::Input(i) => Tool::Input(i),
                _ => continue,
            };
            screens.push((name, vec![Tool::Tabs, tool]));
        }
        for (name, stack) in screens {
            harness.state_mut().phone_tools = stack;
            harness.run_steps(3);
            if let Ok(img) = harness.render() {
                let _ = img.save(format!("{dir}/tools-{name}.png"));
            }
        }
    }
}
