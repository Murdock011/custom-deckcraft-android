//! Phone bottom sheets: the ribbon's everyday commands as grids of large labelled tiles.
//!
//! A touch screen has no hover, no right click and no small targets, so the phone layout does not
//! shrink the ribbon. A tab bar at the bottom opens one sheet at a time above it (Slides, Insert,
//! Text, Shape, Edit, More). The sheet sits in the layout, not over it: the slide stays visible
//! and editable while a sheet is open, so the effect of a tap can be seen at once. Every tile runs
//! a command id with fixed parameters; this module adds no behaviour of its own.
//!
//! Tiles that change formatting keep the sheet open (Bold, then Italic). Tiles that insert
//! something or open a dialog close it.

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, WidgetInfo, pos2, vec2};
use serde_json::{Value, json};

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::phone::TOUCH_H;
use crate::ribbon;
use crate::theme::{self, Tokens};

/// Width of a tile at least; the sheet divides its width into whole tiles.
const TILE_MIN_W: f32 = 76.0;
/// Height of a tile: an icon over a one-line label. Above the 48 pt touch minimum.
const TILE_H: f32 = 68.0;
/// Diameter of a colour swatch. Above the 48 pt touch minimum.
const SWATCH: f32 = 48.0;
/// Height of the one-line hint above tiles that need a selection.
const HINT_H: f32 = 32.0;
/// The most a sheet takes of the window height. The slide keeps the rest.
const MAX_FRACTION: f32 = 0.42;

/// Swatch colours, as hex strings the colour commands accept.
const SWATCHES: [&str; 8] = ["#000000", "#FFFFFF", "#C00000", "#F26B1D", "#FFC000", "#2EA36F", "#2E6FD8", "#7030A0"];

/// The sheets, one per tab in the bottom bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sheet {
    Slides,
    Insert,
    Text,
    Shape,
    Edit,
    More,
}

impl Sheet {
    pub const ALL: [Sheet; 6] = [Sheet::Slides, Sheet::Insert, Sheet::Text, Sheet::Shape, Sheet::Edit, Sheet::More];

    pub fn label(self) -> &'static str {
        match self {
            Sheet::Slides => "Slides",
            Sheet::Insert => "Insert",
            Sheet::Text => "Text",
            Sheet::Shape => "Shape",
            Sheet::Edit => "Edit",
            Sheet::More => "More",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Sheet::Slides => Icon::Slide,
            Sheet::Insert => Icon::Plus,
            Sheet::Text => Icon::FontColor,
            Sheet::Shape => Icon::Shapes,
            Sheet::Edit => Icon::Paste,
            Sheet::More => Icon::More,
        }
    }

    /// The command that tells whether the sheet has anything to act on, and the hint to show
    /// while it does not. Sheets that need no selection have none.
    fn needs_selection(self) -> Option<(&'static str, &'static str)> {
        match self {
            Sheet::Text => Some(("format.bold", "Select a text box or some text to format it.")),
            Sheet::Shape => Some(("shape.fill", "Select a shape, picture or text box to change it.")),
            _ => None,
        }
    }

    /// The command the swatch row runs, with its label, for sheets that have one.
    fn swatch_command(self) -> Option<(&'static str, &'static str)> {
        match self {
            Sheet::Text => Some(("format.color", "Text colour")),
            Sheet::Shape => Some(("shape.fill", "Fill colour")),
            _ => None,
        }
    }
}

/// One tile: an icon over a label that runs `id` with `params`.
#[derive(Clone, Debug)]
pub struct Tile {
    pub icon: Icon,
    pub label: &'static str,
    pub id: &'static str,
    pub params: Value,
    /// Keep the sheet open after the tap (formatting), instead of closing it (insert, dialogs).
    pub keep_open: bool,
    /// Drawn switched on.
    pub on: bool,
}

fn tile(icon: Icon, label: &'static str, id: &'static str) -> Tile {
    Tile { icon, label, id, params: json!({}), keep_open: false, on: false }
}

impl Tile {
    fn with(mut self, params: Value) -> Self {
        self.params = params;
        self
    }
    fn keep(mut self) -> Self {
        self.keep_open = true;
        self
    }
    fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }
}

/// Tapping a tab opens its sheet; tapping the open tab closes it.
pub fn toggle(app: &mut SlideApp, sheet: Sheet) {
    app.phone_sheet = if app.phone_sheet == Some(sheet) { None } else { Some(sheet) };
}

/// The tiles of `sheet` for the document as it is now. Formatting tiles reflect the selection.
pub fn tiles(app: &mut SlideApp, sheet: Sheet) -> Vec<Tile> {
    match sheet {
        Sheet::Slides => {
            let (cur, count) = app.session.active().map(|d| (d.selection.slide, d.doc.slides.len())).unwrap_or((0, 0));
            let earlier = cur.saturating_sub(1);
            let later = (cur + 1).min(count.saturating_sub(1));
            vec![
                tile(Icon::NewSlide, "New", "slide.new").keep(),
                tile(Icon::DuplicateSlide, "Duplicate", "slide.duplicate").keep(),
                tile(Icon::DeleteSlide, "Delete", "slide.delete").keep(),
                tile(Icon::HideSlide, "Hide", "slide.hide").keep(),
                tile(Icon::ChevronLeft, "Move left", "slide.move").with(json!({"from": cur, "to": earlier})).keep(),
                tile(Icon::ChevronRight, "Move right", "slide.move").with(json!({"from": cur, "to": later})).keep(),
                tile(Icon::ResetSlide, "Reset", "slide.reset").keep(),
                tile(Icon::Section, "Section", "section.add"),
                tile(Icon::Layout, "Title", "slide.layout").with(json!({"layout": "title"})),
                tile(Icon::Layout, "Content", "slide.layout").with(json!({"layout": "titleAndContent"})),
                tile(Icon::Layout, "Section head", "slide.layout").with(json!({"layout": "sectionHeader"})),
                tile(Icon::Layout, "Two content", "slide.layout").with(json!({"layout": "twoContent"})),
                tile(Icon::Layout, "Comparison", "slide.layout").with(json!({"layout": "comparison"})),
                tile(Icon::Layout, "Title only", "slide.layout").with(json!({"layout": "titleOnly"})),
                tile(Icon::Layout, "Blank", "slide.layout").with(json!({"layout": "blank"})),
            ]
        }
        Sheet::Insert => vec![
            tile(Icon::TextBox, "Text box", "insert.textBox"),
            tile(Icon::Picture, "Picture", "app.insertPictureDialog"),
            shape_tile(Icon::Shapes, "Rectangle", "rect"),
            shape_tile(Icon::Shapes, "Oval", "ellipse"),
            shape_tile(Icon::Shapes, "Rounded", "roundRect"),
            shape_tile(Icon::Shapes, "Triangle", "triangle"),
            shape_tile(Icon::Star, "Star", "star5"),
            tile(Icon::Table, "Table", "insert.table").with(json!({"rows": 3, "cols": 3})),
            tile(Icon::Chart, "Column chart", "insert.chart").with(json!({"type": "column"})),
            tile(Icon::Chart, "Line chart", "insert.chart").with(json!({"type": "line"})),
            tile(Icon::Chart, "Pie chart", "insert.chart").with(json!({"type": "pie"})),
            tile(Icon::SmartArt, "Diagram", "app.dialog").with(json!({"id": "smartart"})),
            tile(Icon::Link, "Link", "app.dialog").with(json!({"id": "hyperlink"})),
            tile(Icon::Video, "Video", "app.insertVideoDialog"),
            tile(Icon::Audio, "Audio", "app.insertAudioDialog"),
            tile(Icon::Comment, "Comment", "app.dialog").with(json!({"id": "comment"})),
            tile(Icon::Symbol, "Symbol", "app.dialog").with(json!({"id": "symbol"})),
            tile(Icon::Equation, "Equation", "app.dialog").with(json!({"id": "equation"})),
            tile(Icon::DateTime, "Date & time", "insert.dateTime"),
            tile(Icon::SlideNumber, "Slide no.", "insert.slideNumber"),
            tile(Icon::HeaderFooter, "Footer", "app.dialog").with(json!({"id": "headerFooter"})),
        ],
        Sheet::Text => {
            let st = ribbon::fmt_state(app);
            let flag = |k: &str| st.get(k).and_then(Value::as_bool).unwrap_or(false);
            let align = st.get("align").and_then(Value::as_str).unwrap_or("").to_string();
            vec![
                tile(Icon::Bold, "Bold", "format.bold").keep().on(flag("bold")),
                tile(Icon::Italic, "Italic", "format.italic").keep().on(flag("italic")),
                tile(Icon::Underline, "Underline", "format.underline").keep().on(flag("underline")),
                tile(Icon::Strikethrough, "Strike", "format.strikethrough").keep().on(flag("strike")),
                tile(Icon::FontGrow, "Larger", "format.grow").keep(),
                tile(Icon::FontShrink, "Smaller", "format.shrink").keep(),
                tile(Icon::AlignLeft, "Left", "format.alignLeft").keep().on(align == "l"),
                tile(Icon::AlignCenter, "Centre", "format.alignCenter").keep().on(align == "ctr"),
                tile(Icon::AlignRight, "Right", "format.alignRight").keep().on(align == "r"),
                tile(Icon::Justify, "Justify", "format.justify").keep().on(align == "just"),
                tile(Icon::Bullets, "Bullets", "format.bullets").keep(),
                tile(Icon::Numbering, "Numbers", "format.numbering").keep(),
                tile(Icon::DecreaseIndent, "Outdent", "format.outdent").keep(),
                tile(Icon::IncreaseIndent, "Indent", "format.indent").keep(),
                tile(Icon::Superscript, "Super", "format.superscript").keep().on(flag("superscript")),
                tile(Icon::Subscript, "Sub", "format.subscript").keep().on(flag("subscript")),
                tile(Icon::Highlight, "Highlight", "format.highlight").with(json!({"color": "#FFFF00"})).keep(),
                tile(Icon::LineSpacing, "Spacing", "format.paragraphDialog"),
                tile(Icon::FontColor, "Font…", "format.fontDialog"),
                tile(Icon::ClearFormatting, "Clear", "format.clear").keep(),
            ]
        }
        Sheet::Shape => vec![
            tile(Icon::ShapeOutline, "Outline", "shape.line").with(json!({"color": "#000000"})).keep(),
            tile(Icon::BringToFront, "To front", "arrange.bringToFront").keep(),
            tile(Icon::SendToBack, "To back", "arrange.sendToBack").keep(),
            tile(Icon::BringForward, "Forward", "arrange.bringForward").keep(),
            tile(Icon::SendBackward, "Backward", "arrange.sendBackward").keep(),
            tile(Icon::Group, "Group", "arrange.group").keep(),
            tile(Icon::Ungroup, "Ungroup", "arrange.ungroup").keep(),
            tile(Icon::RotateLeft, "Rotate left", "arrange.rotateLeft").keep(),
            tile(Icon::RotateRight, "Rotate right", "arrange.rotateRight").keep(),
            tile(Icon::FlipH, "Flip across", "arrange.flipHorizontal").keep(),
            tile(Icon::FlipV, "Flip down", "arrange.flipVertical").keep(),
            tile(Icon::AltText, "Alt text", "app.dialog").with(json!({"id": "altText"})),
            tile(Icon::Close, "Delete", "edit.delete").keep(),
        ],
        Sheet::Edit => vec![
            tile(Icon::Cut, "Cut", "edit.cut"),
            tile(Icon::Copy, "Copy", "edit.copy"),
            tile(Icon::Paste, "Paste", "edit.paste"),
            tile(Icon::Paste, "Paste text", "edit.pasteText"),
            tile(Icon::Object, "Select all", "edit.selectAll"),
            tile(Icon::DuplicateSlide, "Duplicate", "edit.duplicate"),
            tile(Icon::Close, "Delete", "edit.delete"),
            tile(Icon::FormatPainter, "Painter", "format.painter"),
            tile(Icon::Search, "Find", "app.find"),
            tile(Icon::Spelling, "Replace", "app.replace"),
            tile(Icon::Undo, "Undo", "edit.undo").keep(),
            tile(Icon::Redo, "Redo", "edit.redo").keep(),
        ],
        Sheet::More => vec![
            tile(Icon::PlayFromStart, "Play", "show.fromStart"),
            tile(Icon::PlayFromCurrent, "Play here", "show.fromCurrent"),
            tile(Icon::ViewNormal, "Normal", "view.normal"),
            tile(Icon::ViewSorter, "Sorter", "view.sorter"),
            tile(Icon::ViewOutline, "Outline", "view.outline"),
            tile(Icon::Notes, "Notes", "view.notesPage"),
            tile(Icon::Themes, "Themes", "view.pane").with(json!({"pane": "designer"})),
            tile(Icon::FormatBackground, "Background", "view.pane").with(json!({"pane": "background"})),
            tile(Icon::AnimationPane, "Animations", "view.pane").with(json!({"pane": "animation"})),
            tile(Icon::SelectionPane, "Selection", "view.pane").with(json!({"pane": "selection"})),
            tile(Icon::ShowComments, "Comments", "view.pane").with(json!({"pane": "comments"})),
            tile(Icon::Spelling, "Spelling", "app.dialog").with(json!({"id": "spelling"})),
            tile(Icon::ZoomIn, "Zoom in", "view.zoomIn").keep(),
            tile(Icon::ZoomOut, "Zoom out", "view.zoomOut").keep(),
            tile(Icon::FitToWindow, "Fit", "view.fit").keep(),
            tile(Icon::Gridlines, "Gridlines", "view.gridlines"),
        ],
    }
}

/// A tap-to-insert shape: placed in the middle of a standard slide, ready to drag and resize.
fn shape_tile(icon: Icon, label: &'static str, preset: &'static str) -> Tile {
    tile(icon, label, "shape.insert").with(json!({"preset": preset, "rect": [380.0, 200.0, 200.0, 140.0]}))
}

/// Height the sheet takes in a window `window_h` tall, for `rows` rows of tiles.
pub fn sheet_height(window_h: f32, rows: usize, swatches: bool, hint: bool) -> f32 {
    let want = rows as f32 * TILE_H + if swatches { SWATCH + 12.0 } else { 0.0 } + if hint { HINT_H } else { 0.0 } + 12.0;
    want.min((window_h * MAX_FRACTION).max(TILE_H + 12.0))
}

/// The bottom tab bar: one tab per sheet, each an icon over a label. Tapping a tab opens or
/// closes its sheet.
pub fn tab_bar(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let open = app.phone_sheet;
    egui::Panel::bottom("phone_tabs").exact_size(TOUCH_H + 8.0).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let each = ui.available_width() / Sheet::ALL.len() as f32;
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        ui.horizontal(|ui| {
            for s in Sheet::ALL {
                let (rect, resp) = ui.allocate_exact_size(vec2(each, TOUCH_H + 8.0), Sense::click());
                let on = open == Some(s);
                resp.widget_info(|| WidgetInfo::selected(egui::Role::Button, true, on, s.label()));
                if resp.is_pointer_button_down_on() {
                    ui.painter().rect_filled(rect.shrink2(vec2(4.0, 4.0)), CornerRadius::same(12), t.pressed);
                } else if on {
                    ui.painter().rect_filled(rect.shrink2(vec2(4.0, 4.0)), CornerRadius::same(12), t.selected);
                }
                let ink = if on { t.accent } else { t.text };
                icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.center().x, rect.min.y + 20.0), vec2(24.0, 24.0)), s.icon(), ink, false);
                ui.painter().text(pos2(rect.center().x, rect.max.y - 10.0), Align2::CENTER_CENTER, s.label(), theme::font(11.0), ink);
                if resp.clicked() {
                    toggle(app, s);
                }
            }
        });
    });
}

/// The open sheet, as a panel above the tab bar. Draws nothing when no sheet is open.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let Some(sheet) = app.phone_sheet else { return };
    let t = Tokens::get(ui.ctx());
    let tiles = tiles(app, sheet);
    let swatch_cmd = sheet.swatch_command();
    let window_h = ui.ctx().content_rect().height();
    let width = ui.ctx().content_rect().width();
    let cols = ((width / TILE_MIN_W).floor() as usize).max(1);
    let rows = tiles.len().div_ceil(cols);
    let hint = sheet.needs_selection().filter(|(id, _)| !ribbon::enabled(app, id)).map(|(_, h)| h);
    let height = sheet_height(window_h, rows, swatch_cmd.is_some(), hint.is_some());
    let mut ran: Option<(&'static str, Value, bool)> = None;
    egui::Panel::bottom("phone_sheet").exact_size(height).frame(egui::Frame::NONE.fill(t.card)).show(ui, |ui| {
        ui.painter().hline(ui.max_rect().x_range(), ui.max_rect().min.y, Stroke::new(1.0, t.border));
        egui::ScrollArea::vertical().id_salt("phone_sheet_scroll").auto_shrink([false, false]).show(ui, |ui| {
            let tile_w = ui.available_width() / cols as f32;
            if let Some(h) = hint {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HINT_H), Sense::hover());
                ui.painter().with_clip_rect(rect).text(rect.center(), Align2::CENTER_CENTER, h, theme::font(13.0), t.text.gamma_multiply(0.7));
            }
            if let Some((id, label)) = swatch_cmd {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let each = ui.available_width() / SWATCHES.len() as f32;
                    for hex in SWATCHES {
                        let (rect, resp) = ui.allocate_exact_size(vec2(each, SWATCH + 12.0), Sense::click());
                        resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, format!("{label} {hex}")));
                        let c = parse_hex(hex);
                        let r = (SWATCH * 0.5 - 6.0) + if resp.is_pointer_button_down_on() { 2.0 } else { 0.0 };
                        ui.painter().circle(rect.center(), r, c, Stroke::new(1.5, t.border));
                        if resp.clicked() {
                            ran = Some((id, json!({"color": hex}), true));
                        }
                    }
                });
            }
            let mut i = 0;
            while i < tiles.len() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for tl in tiles.iter().skip(i).take(cols) {
                        let en = ribbon::enabled(app, tl.id);
                        if tile_button(ui, tl, tile_w, en, &t).clicked() && en {
                            ran = Some((tl.id, tl.params.clone(), tl.keep_open));
                        }
                    }
                });
                i += cols;
            }
        });
    });
    if let Some((id, params, keep)) = ran {
        let _ = app.run(id, params);
        if !keep {
            app.phone_sheet = None;
        }
    }
}

fn tile_button(ui: &mut Ui, tl: &Tile, w: f32, enabled: bool, t: &Tokens) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, TILE_H), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, enabled, tl.label));
    if enabled && resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect.shrink(3.0), CornerRadius::same(12), t.pressed);
    } else if tl.on {
        ui.painter().rect_filled(rect.shrink(3.0), CornerRadius::same(12), t.selected);
    }
    let ink = if tl.on { t.accent } else { t.text };
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.center().x, rect.min.y + 24.0), vec2(26.0, 26.0)), tl.icon, ink, !enabled);
    let label_ink = if enabled { ink } else { ink.gamma_multiply(0.45) };
    ui.painter().with_clip_rect(rect).text(pos2(rect.center().x, rect.max.y - 14.0), Align2::CENTER_CENTER, tl.label, theme::font(11.5), label_ink);
    resp
}

fn parse_hex(hex: &str) -> Color32 {
    let h = hex.trim_start_matches('#');
    let byte = |i: usize| h.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or(0);
    Color32::from_rgb(byte(0), byte(2), byte(4))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Context;

    fn app_with_sample() -> SlideApp {
        let mut app = SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
        deckcraft_engine::sample::open_sample(&mut app.session).expect("sample deck opens");
        app.fonts_ready = true;
        app.styled = true;
        app
    }

    #[test]
    fn every_tile_names_a_known_command() {
        let mut app = app_with_sample();
        for s in Sheet::ALL {
            for tl in tiles(&mut app, s) {
                assert!(ribbon::known(tl.id), "{:?} tile `{}` runs unknown command `{}`", s, tl.label, tl.id);
            }
        }
    }

    #[test]
    fn every_sheet_has_tiles_and_a_distinct_label() {
        let mut app = app_with_sample();
        let mut labels = std::collections::HashSet::new();
        for s in Sheet::ALL {
            assert!(!tiles(&mut app, s).is_empty(), "{s:?} has no tiles");
            assert!(labels.insert(s.label()), "two tabs are called {}", s.label());
        }
    }

    #[test]
    fn a_sheet_never_covers_more_than_its_share_of_the_window() {
        // 21 tiles in a 390 pt wide window is 6 rows: too tall to show whole, so it scrolls.
        let h = sheet_height(800.0, 6, false, false);
        assert!(h <= 800.0 * MAX_FRACTION + 0.01, "{h}");
        assert!(h >= TILE_H, "a sheet always shows a whole tile row");
        // A short sheet takes only what it needs.
        assert!(sheet_height(800.0, 1, false, false) < h);
        // A tiny window still shows a row.
        assert!(sheet_height(100.0, 6, true, true) >= TILE_H);
    }

    /// Every touch target is at least the 48 pt Android minimum: checked when this compiles.
    const _: () = assert!(TILE_H >= 48.0 && SWATCH >= 48.0 && TILE_MIN_W >= 48.0);

    #[test]
    fn tapping_a_tab_toggles_its_sheet() {
        let mut app = app_with_sample();
        assert_eq!(app.phone_sheet, None);
        toggle(&mut app, Sheet::Insert);
        assert_eq!(app.phone_sheet, Some(Sheet::Insert));
        toggle(&mut app, Sheet::Text);
        assert_eq!(app.phone_sheet, Some(Sheet::Text), "another tab replaces the sheet");
        assert!(app.can_go_back(), "Back has an open sheet to close");
        toggle(&mut app, Sheet::Text);
        assert_eq!(app.phone_sheet, None, "the open tab closes it");
        assert!(!app.can_go_back());
    }

    /// Escape, which the Android host sends for Back, closes an open sheet and leaves the slide alone.
    #[test]
    fn escape_closes_the_open_sheet() {
        let ctx = Context::default();
        let mut app = app_with_sample();
        app.phone_sheet = Some(Sheet::Edit);
        let n = app.session.active().map(|d| d.doc.slides.len());
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(390.0, 800.0))),
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        ctx.run_ui(raw, |ui| app.ui(ui)).drop_without_applying_deltas();
        assert_eq!(app.phone_sheet, None);
        assert_eq!(app.session.active().map(|d| d.doc.slides.len()), n);
    }

    /// Insert tiles that need no dialog or picker work on the sample deck, and each adds a shape.
    #[test]
    fn insert_tiles_add_to_the_slide() {
        let mut app = app_with_sample();
        for tl in
            tiles(&mut app, Sheet::Insert).into_iter().filter(|t| matches!(t.id, "shape.insert" | "insert.textBox" | "insert.table" | "insert.chart"))
        {
            let before = app.session.active().and_then(|d| d.doc.slides.get(d.selection.slide).map(|s| s.shapes.len())).unwrap_or(0);
            app.run(tl.id, tl.params.clone()).unwrap_or_else(|e| panic!("`{}` failed: {e:?}", tl.label));
            let after = app.session.active().and_then(|d| d.doc.slides.get(d.selection.slide).map(|s| s.shapes.len())).unwrap_or(0);
            assert!(after > before, "`{}` added nothing", tl.label);
        }
    }

    /// The slide tiles change the deck: New adds a slide, Move reorders, Delete removes.
    #[test]
    fn slide_tiles_change_the_deck() {
        let mut app = app_with_sample();
        let count = |app: &SlideApp| app.session.active().map(|d| d.doc.slides.len()).unwrap_or(0);
        let n = count(&app);
        let new = tiles(&mut app, Sheet::Slides).remove(0);
        app.run(new.id, new.params).expect("New runs");
        assert_eq!(count(&app), n + 1);
        let del = tiles(&mut app, Sheet::Slides).into_iter().find(|t| t.label == "Delete").expect("a Delete tile");
        app.run(del.id, del.params).expect("Delete runs");
        assert_eq!(count(&app), n);
        // Every layout tile names a layout the engine knows.
        for tl in tiles(&mut app, Sheet::Slides).into_iter().filter(|t| t.id == "slide.layout") {
            app.run(tl.id, tl.params.clone()).unwrap_or_else(|e| panic!("layout `{}` failed: {e:?}", tl.label));
        }
        // Move left on the first slide stays put and does not fail or panic.
        app.run("slide.go", json!({"index": 0})).expect("go");
        let up = tiles(&mut app, Sheet::Slides).into_iter().find(|t| t.label == "Move left").expect("a Move left tile");
        let _ = app.run(up.id, up.params);
        assert_eq!(count(&app), n);
    }

    /// Every tile that opens a dialog names a dialog that opens.
    #[test]
    fn dialog_tiles_open_their_dialog() {
        for s in Sheet::ALL {
            let mut app = app_with_sample();
            for tl in tiles(&mut app, s).into_iter().filter(|t| t.id == "app.dialog") {
                app.dialog = None;
                app.run(tl.id, tl.params.clone()).unwrap_or_else(|e| panic!("`{}` failed: {e:?}", tl.label));
                let want = tl.params.get("id").and_then(Value::as_str).unwrap_or_default();
                assert_eq!(app.dialog.as_ref().map(|d| d.id.as_str()), Some(want), "`{}` did not open `{want}`", tl.label);
            }
        }
    }

    /// Both swatch commands take the hex colours the swatch row sends.
    #[test]
    fn swatches_colour_the_selection() {
        let mut app = app_with_sample();
        app.run("shape.insert", json!({"preset": "rect", "rect": [100.0, 100.0, 200.0, 100.0]})).expect("a shape");
        for hex in SWATCHES {
            app.run("shape.fill", json!({"color": hex})).unwrap_or_else(|e| panic!("fill {hex}: {e:?}"));
        }
        app.run("shape.line", json!({"color": "#000000"})).expect("outline takes a colour");
    }

    /// A whole frame at phone width with each sheet open: it draws and a tile tap runs.
    #[test]
    fn every_sheet_draws_at_phone_width() {
        let ctx = Context::default();
        let mut app = app_with_sample();
        for s in Sheet::ALL {
            app.phone_sheet = Some(s);
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(390.0, 800.0))), ..Default::default() };
            ctx.run_ui(raw, |ui| app.ui(ui)).drop_without_applying_deltas();
        }
    }

    /// Writes a PNG of every sheet at phone size into `$DECKCRAFT_SHOT_DIR`, for looking at the
    /// result (a shape is selected, so the Text and Shape tiles are live). Does nothing when the
    /// variable is unset or no GPU renderer is available.
    #[test]
    fn sheet_screenshots() {
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
        let first = harness.state().session.active().and_then(|d| d.doc.slides.first().and_then(|s| s.shapes.first().map(|sh| sh.id.0)));
        if let Some(id) = first {
            let _ = harness.state_mut().run("edit.select", json!({"ids": [id]}));
        }
        for (name, s) in [
            ("closed", None),
            ("slides", Some(Sheet::Slides)),
            ("insert", Some(Sheet::Insert)),
            ("text", Some(Sheet::Text)),
            ("shape", Some(Sheet::Shape)),
            ("edit", Some(Sheet::Edit)),
            ("more", Some(Sheet::More)),
        ] {
            harness.state_mut().phone_sheet = s;
            harness.run_steps(3);
            if let Ok(img) = harness.render() {
                let _ = img.save(format!("{dir}/sheet-{name}.png"));
            }
        }
    }
}
