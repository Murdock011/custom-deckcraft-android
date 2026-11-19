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
/// Height of a tile: an icon over a label of up to two lines. Above the 48 pt touch minimum.
const TILE_H: f32 = 76.0;
/// Diameter of a colour swatch. Above the 48 pt touch minimum.
const SWATCH: f32 = 48.0;
/// Height of the one-line hint above tiles that need a selection.
const HINT_H: f32 = 32.0;
/// The sheet's header (title and Close), a section heading, a list row, and the tab bar.
const HEADER_H: f32 = 56.0;
const HEADING_H: f32 = 32.0;
const ROW_H: f32 = 52.0;
pub const TAB_BAR_H: f32 = 64.0;
/// The margin at a sheet's sides.
const GUTTER: f32 = 16.0;
/// The most a sheet takes of the window height (and of a window under 500 pt tall). The slide
/// keeps the rest.
const MAX_FRACTION: f32 = 0.50;
const MAX_FRACTION_SHORT: f32 = 0.60;

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

    /// The command the swatch row runs, with its heading, for sheets that have one.
    fn swatch_command(self) -> Option<(&'static str, &'static str)> {
        match self {
            Sheet::Text => Some(("format.color", "Font Color")),
            Sheet::Shape => Some(("shape.fill", "Fill")),
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

/// A section of a sheet: a heading and its tiles.
pub struct Section {
    pub heading: &'static str,
    pub tiles: Vec<Tile>,
}

fn section(heading: &'static str, tiles: Vec<Tile>) -> Section {
    Section { heading, tiles }
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

    /// The label the tile shows: the engine command's own, for a tile that runs a command as it
    /// is; the tile's, for one that runs it with fixed values (a layout, a shape, a pane).
    pub fn shown_label(&self) -> &'static str {
        let plain = self.params.as_object().is_some_and(|o| o.is_empty());
        if plain {
            if let Some(c) = deckcraft_engine::find_command(self.id) {
                return c.label;
            }
            if let Some(c) = crate::UI_COMMANDS.iter().find(|c| c.0 == self.id) {
                return c.1;
            }
        }
        self.label
    }
}

/// Tapping a tab opens its sheet; tapping the open tab closes it.
pub fn toggle(app: &mut SlideApp, sheet: Sheet) {
    app.phone_tools.clear();
    app.phone_sheet = if app.phone_sheet == Some(sheet) { None } else { Some(sheet) };
}

/// The tiles of `sheet` for the document as it is now, in one list.
#[cfg(test)]
pub fn tiles(app: &mut SlideApp, sheet: Sheet) -> Vec<Tile> {
    sections(app, sheet).into_iter().flat_map(|s| s.tiles).collect()
}

/// The sections of `sheet` for the document as it is now. Formatting tiles reflect the selection.
/// Both sheet styles (tiles and list) draw these, so the two show the same commands.
pub fn sections(app: &mut SlideApp, sheet: Sheet) -> Vec<Section> {
    match sheet {
        Sheet::Slides => {
            let (cur, count) = app.session.active().map(|d| (d.selection.slide, d.doc.slides.len())).unwrap_or((0, 0));
            let earlier = cur.saturating_sub(1);
            let later = (cur + 1).min(count.saturating_sub(1));
            vec![
                section(
                    "Slide",
                    vec![
                        tile(Icon::NewSlide, "New", "slide.new").keep(),
                        tile(Icon::DuplicateSlide, "Duplicate", "slide.duplicate").keep(),
                        tile(Icon::DeleteSlide, "Delete", "slide.delete").keep(),
                        tile(Icon::HideSlide, "Hide", "slide.hide").keep(),
                        tile(Icon::ChevronLeft, "Move Left", "slide.move").with(json!({"from": cur, "to": earlier})).keep(),
                        tile(Icon::ChevronRight, "Move Right", "slide.move").with(json!({"from": cur, "to": later})).keep(),
                        tile(Icon::ResetSlide, "Reset", "slide.reset").keep(),
                        tile(Icon::Section, "Section", "section.add"),
                    ],
                ),
                section(
                    "Layout",
                    vec![
                        tile(Icon::Layout, "Title Slide", "slide.layout").with(json!({"layout": "title"})),
                        tile(Icon::Layout, "Title and Content", "slide.layout").with(json!({"layout": "titleAndContent"})),
                        tile(Icon::Layout, "Section Header", "slide.layout").with(json!({"layout": "sectionHeader"})),
                        tile(Icon::Layout, "Two Content", "slide.layout").with(json!({"layout": "twoContent"})),
                        tile(Icon::Layout, "Comparison", "slide.layout").with(json!({"layout": "comparison"})),
                        tile(Icon::Layout, "Title Only", "slide.layout").with(json!({"layout": "titleOnly"})),
                        tile(Icon::Layout, "Blank", "slide.layout").with(json!({"layout": "blank"})),
                    ],
                ),
            ]
        }
        Sheet::Insert => vec![
            section(
                "Text",
                vec![
                    tile(Icon::TextBox, "Text Box", "insert.textBox"),
                    tile(Icon::Symbol, "Symbol…", "app.dialog").with(json!({"id": "symbol"})),
                    tile(Icon::Equation, "Equation…", "app.dialog").with(json!({"id": "equation"})),
                    tile(Icon::DateTime, "Date & Time", "insert.dateTime"),
                    tile(Icon::SlideNumber, "Slide Number", "insert.slideNumber"),
                    tile(Icon::HeaderFooter, "Header & Footer…", "app.dialog").with(json!({"id": "headerFooter"})),
                ],
            ),
            section(
                "Pictures and media",
                vec![
                    tile(Icon::Picture, "Picture…", "app.insertPictureDialog"),
                    tile(Icon::Video, "Video…", "app.insertVideoDialog"),
                    tile(Icon::Audio, "Audio…", "app.insertAudioDialog"),
                ],
            ),
            section(
                "Shapes",
                vec![
                    shape_tile(Icon::Shapes, "Rectangle", "rect"),
                    shape_tile(Icon::Shapes, "Oval", "ellipse"),
                    shape_tile(Icon::Shapes, "Rounded Rectangle", "roundRect"),
                    shape_tile(Icon::Shapes, "Triangle", "triangle"),
                    shape_tile(Icon::Star, "Star", "star5"),
                    tile(Icon::Shapes, "Shapes…", SHAPES),
                ],
            ),
            section(
                "Tables and charts",
                vec![
                    tile(Icon::Table, "Table", "insert.table").with(json!({"rows": 3, "cols": 3})),
                    tile(Icon::Chart, "Column Chart", "insert.chart").with(json!({"type": "column"})),
                    tile(Icon::Chart, "Line Chart", "insert.chart").with(json!({"type": "line"})),
                    tile(Icon::Chart, "Pie Chart", "insert.chart").with(json!({"type": "pie"})),
                    tile(Icon::SmartArt, "Diagram…", "app.dialog").with(json!({"id": "smartart"})),
                ],
            ),
            section(
                "Other",
                vec![
                    tile(Icon::Link, "Link…", "app.dialog").with(json!({"id": "hyperlink"})),
                    tile(Icon::Comment, "Comment…", "app.dialog").with(json!({"id": "comment"})),
                ],
            ),
        ],
        Sheet::Text => {
            let st = ribbon::fmt_state(app);
            let flag = |k: &str| st.get(k).and_then(Value::as_bool).unwrap_or(false);
            let align = st.get("align").and_then(Value::as_str).unwrap_or("").to_string();
            vec![
                section(
                    "Font",
                    vec![
                        tile(Icon::Bold, "Bold", "format.bold").keep().on(flag("bold")),
                        tile(Icon::Italic, "Italic", "format.italic").keep().on(flag("italic")),
                        tile(Icon::Underline, "Underline", "format.underline").keep().on(flag("underline")),
                        tile(Icon::Strikethrough, "Strikethrough", "format.strikethrough").keep().on(flag("strike")),
                        tile(Icon::Superscript, "Superscript", "format.superscript").keep().on(flag("superscript")),
                        tile(Icon::Subscript, "Subscript", "format.subscript").keep().on(flag("subscript")),
                        tile(Icon::FontGrow, "Larger", "format.grow").keep(),
                        tile(Icon::FontShrink, "Smaller", "format.shrink").keep(),
                        tile(Icon::Highlight, "Highlight…", SUB).with(json!({"sub": "highlight"})).keep(),
                        tile(Icon::ClearFormatting, "Clear", "format.clear").keep(),
                        tile(Icon::FontColor, "Font…", "format.fontDialog"),
                    ],
                ),
                section(
                    "Paragraph",
                    vec![
                        tile(Icon::AlignLeft, "Left", "format.alignLeft").keep().on(align == "l"),
                        tile(Icon::AlignCenter, "Center", "format.alignCenter").keep().on(align == "ctr"),
                        tile(Icon::AlignRight, "Right", "format.alignRight").keep().on(align == "r"),
                        tile(Icon::Justify, "Justify", "format.justify").keep().on(align == "just"),
                        tile(Icon::Bullets, "Bullets", "format.bullets").keep(),
                        tile(Icon::Numbering, "Numbering", "format.numbering").keep(),
                        tile(Icon::DecreaseIndent, "Outdent", "format.outdent").keep(),
                        tile(Icon::IncreaseIndent, "Indent", "format.indent").keep(),
                        tile(Icon::LineSpacing, "Paragraph…", "format.paragraphDialog"),
                    ],
                ),
            ]
        }
        Sheet::Shape => vec![
            section(
                "Arrange",
                vec![
                    tile(Icon::BringToFront, "To Front", "arrange.bringToFront").keep(),
                    tile(Icon::SendToBack, "To Back", "arrange.sendToBack").keep(),
                    tile(Icon::BringForward, "Forward", "arrange.bringForward").keep(),
                    tile(Icon::SendBackward, "Backward", "arrange.sendBackward").keep(),
                    tile(Icon::Group, "Group", "arrange.group").keep(),
                    tile(Icon::Ungroup, "Ungroup", "arrange.ungroup").keep(),
                ],
            ),
            section(
                "Rotate",
                vec![
                    tile(Icon::RotateLeft, "Rotate Left", "arrange.rotateLeft").keep(),
                    tile(Icon::RotateRight, "Rotate Right", "arrange.rotateRight").keep(),
                    tile(Icon::FlipH, "Flip Horizontal", "arrange.flipHorizontal").keep(),
                    tile(Icon::FlipV, "Flip Vertical", "arrange.flipVertical").keep(),
                ],
            ),
            section(
                "Shape",
                vec![
                    tile(Icon::ShapeOutline, "Outline…", SUB).with(json!({"sub": "outline"})).keep(),
                    tile(Icon::AlignObjects, "Align…", SUB).with(json!({"sub": "align"})).keep(),
                    tile(Icon::AltText, "Alt Text…", "app.dialog").with(json!({"id": "altText"})),
                    tile(Icon::Close, "Delete", "edit.delete").keep(),
                ],
            ),
        ],
        Sheet::Edit => vec![
            section(
                "Clipboard",
                vec![
                    tile(Icon::Cut, "Cut", "edit.cut"),
                    tile(Icon::Copy, "Copy", "edit.copy"),
                    tile(Icon::Paste, "Paste", "edit.paste"),
                    tile(Icon::Paste, "Paste Text", "edit.pasteText"),
                    tile(Icon::FormatPainter, "Painter", "format.painter"),
                ],
            ),
            section(
                "Editing",
                vec![
                    tile(Icon::Object, "Select All", "edit.selectAll"),
                    tile(Icon::DuplicateSlide, "Duplicate", "edit.duplicate"),
                    tile(Icon::Close, "Delete", "edit.delete"),
                    tile(Icon::Search, "Find", "app.find"),
                    tile(Icon::Spelling, "Replace", "app.replace"),
                ],
            ),
            section("History", vec![tile(Icon::Undo, "Undo", "edit.undo").keep(), tile(Icon::Redo, "Redo", "edit.redo").keep()]),
        ],
        Sheet::More => vec![
            section("Show", vec![tile(Icon::PlayFromStart, "Play", "show.fromStart"), tile(Icon::PlayFromCurrent, "Play Here", "show.fromCurrent")]),
            section(
                "View",
                vec![
                    tile(Icon::ViewNormal, "Normal", "view.normal"),
                    tile(Icon::ViewSorter, "Sorter", "view.sorter"),
                    tile(Icon::ViewOutline, "Outline", "view.outline"),
                    tile(Icon::Notes, "Notes", "view.notesPage"),
                    tile(Icon::ZoomIn, "Zoom In", "view.zoomIn").keep(),
                    tile(Icon::ZoomOut, "Zoom Out", "view.zoomOut").keep(),
                    tile(Icon::FitToWindow, "Fit", "view.fit").keep(),
                    tile(Icon::Gridlines, "Gridlines", "view.gridlines"),
                ],
            ),
            section(
                "Panes",
                vec![
                    tile(Icon::Themes, "Themes", "view.pane").with(json!({"pane": "designer"})),
                    tile(Icon::FormatBackground, "Background", "view.pane").with(json!({"pane": "background"})),
                    tile(Icon::AnimationPane, "Animations", "view.pane").with(json!({"pane": "animation"})),
                    tile(Icon::SelectionPane, "Selection", "view.pane").with(json!({"pane": "selection"})),
                    tile(Icon::ShowComments, "Comments", "view.pane").with(json!({"pane": "comments"})),
                    tile(Icon::Spelling, "Spelling…", "app.dialog").with(json!({"id": "spelling"})),
                ],
            ),
        ],
    }
}

/// The pseudo command of the "All tools" row: it opens the tools instead of running a command.
const ALL_TOOLS: &str = "phone.tools";
/// The pseudo command of the "Search" row: All tools, on Search.
const SEARCH: &str = "phone.search";
/// The pseudo command of the "Shapes…" tile: the engine's shape gallery as a picker.
const SHAPES: &str = "phone.shapes";

/// The pseudo command of a tile that opens a sub-page (`{"sub": name}`): colours, outline, align.
const SUB: &str = "phone.sub";
/// The pseudo commands of the font row: All tools' font and size pickers.
const FONT_NAME: &str = "phone.fontName";
const FONT_SIZE: &str = "phone.fontSize";

/// Whether a pseudo command of the phone sheets (not the engine's).
fn is_pseudo(id: &str) -> bool {
    matches!(id, ALL_TOOLS | SEARCH | SHAPES | SUB | FONT_NAME | FONT_SIZE)
}

/// The colour swatches of a colour page: the same 16 as WordCraft's and GridCraft's.
const COLOURS: [&str; 16] = [
    "#000000", "#FFFFFF", "#C00000", "#FF0000", "#FFC000", "#FFFF00", "#92D050", "#00B050", "#00B0F0", "#0070C0", "#002060", "#7030A0", "#1F4E79",
    "#2E75B6", "#A9D18E", "#F4B183",
];
/// Outline weights, in points.
const WEIGHTS: [f32; 6] = [0.75, 1.5, 2.25, 3.0, 4.5, 6.0];

/// A sub-page that replaces a sheet's body (plan 1.6): a colour grid, the outline, or Align.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sub {
    FontColor,
    Highlight,
    Fill,
    Outline,
    Align,
}

impl Sub {
    fn named(name: &str) -> Option<Sub> {
        match name {
            "fontColor" => Some(Sub::FontColor),
            "highlight" => Some(Sub::Highlight),
            "fill" => Some(Sub::Fill),
            "outline" => Some(Sub::Outline),
            "align" => Some(Sub::Align),
            _ => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Sub::FontColor => "Font Color",
            Sub::Highlight => "Highlight",
            Sub::Fill => "Fill",
            Sub::Outline => "Outline",
            Sub::Align => "Align",
        }
    }
}

fn sub_id() -> egui::Id {
    egui::Id::unique("phone_sheet_sub")
}

fn sub_page(ctx: &egui::Context) -> Option<Sub> {
    ctx.data(|d| d.get_temp::<Option<Sub>>(sub_id())).flatten()
}

fn set_sub_page(ctx: &egui::Context, s: Option<Sub>) {
    ctx.data_mut(|d| d.insert_temp(sub_id(), s));
}

/// Whether a sub-page is open (Back closes it before the sheet).
pub fn sub_open(ctx: &egui::Context) -> bool {
    sub_page(ctx).is_some()
}

/// Back from a sub-page to its sheet.
pub fn close_sub(ctx: &egui::Context) {
    set_sub_page(ctx, None);
}

/// A tap-to-insert shape: placed in the middle of a standard slide, ready to drag and resize.
fn shape_tile(icon: Icon, label: &'static str, preset: &'static str) -> Tile {
    tile(icon, label, "shape.insert").with(json!({"preset": preset, "rect": [380.0, 200.0, 200.0, 140.0]}))
}

/// The share of a window `window_h` tall a sheet may take.
fn max_fraction(window_h: f32) -> f32 {
    if window_h < 500.0 { MAX_FRACTION_SHORT } else { MAX_FRACTION }
}

/// Height the sheet takes in a window `window_h` tall: its header, then `rows` rows of tiles and
/// `headings` section headings, a swatch row and a hint when it has them. Longer sheets scroll.
pub fn sheet_height(window_h: f32, rows: usize, headings: usize, swatches: bool, hint: bool) -> f32 {
    let want = HEADER_H
        + rows as f32 * TILE_H
        + headings as f32 * HEADING_H
        + if swatches { HEADING_H + SWATCH + 12.0 } else { 0.0 }
        + if hint { HINT_H } else { 0.0 }
        + 12.0;
    want.min((window_h * max_fraction(window_h)).max(HEADER_H + TILE_H + 12.0))
}

/// Whether the List style is on (the floating quick bar and list sheets, no tab bar).
pub fn list_style(app: &SlideApp) -> bool {
    app.ui.phone_list_style
}

/// The bottom tab bar (Tiles style): one tab per sheet, each an icon over a label. Tapping a tab
/// opens or closes its sheet.
pub fn tab_bar(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let open = if app.phone_tools.is_empty() { app.phone_sheet } else { Some(Sheet::More) };
    egui::Panel::bottom("phone_tabs").exact_size(TAB_BAR_H).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        ui.painter().hline(ui.max_rect().x_range(), ui.max_rect().min.y, Stroke::new(1.0, t.border));
        let each = ui.available_width() / Sheet::ALL.len() as f32;
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        ui.horizontal(|ui| {
            for s in Sheet::ALL {
                let (rect, resp) = ui.allocate_exact_size(vec2(each, TAB_BAR_H), Sense::click());
                let on = open == Some(s);
                resp.widget_info(|| WidgetInfo::selected(egui::Role::Button, true, on, s.label()));
                let pill = Rect::from_center_size(pos2(rect.center().x, rect.min.y + 22.0), vec2(56.0_f32.min(each - 4.0), 32.0));
                if resp.is_pointer_button_down_on() {
                    ui.painter().rect_filled(pill, CornerRadius::same(16), t.pressed);
                } else if on {
                    ui.painter().rect_filled(pill, CornerRadius::same(16), t.selected);
                }
                let ink = if on { t.accent } else { t.text };
                icons::paint(ui.painter(), Rect::from_center_size(pill.center(), vec2(24.0, 24.0)), s.icon(), ink, false);
                let font = if on { theme::bold(12.0) } else { theme::font(12.0) };
                ui.painter().text(pos2(rect.center().x, rect.min.y + 50.0), Align2::CENTER_CENTER, s.label(), font, ink);
                if resp.clicked() {
                    toggle(app, s);
                }
            }
        });
    });
}

/// What a tap in a sheet asks for: a command with its values and whether the sheet stays open.
type Ran = Option<(&'static str, Value, bool)>;

/// The open sheet, as a panel above the tab bar (Tiles) or at the bottom (List). Draws nothing when
/// no sheet is open.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    // The all-tools screens, when open, take the sheet's place.
    if !app.phone_tools.is_empty() {
        crate::phone_tools::show(app, ui);
        return;
    }
    let Some(sheet) = app.phone_sheet else { return };
    remember(ui.ctx(), sheet);
    let t = Tokens::get(ui.ctx());
    let list = list_style(app);
    let sections = sections(app, sheet);
    let swatch_cmd = sheet.swatch_command();
    let window_h = ui.ctx().content_rect().height();
    let width = ui.ctx().content_rect().width() - 2.0 * GUTTER;
    let cols = ((width / TILE_MIN_W).floor() as usize).max(1);
    let rows: usize = sections.iter().map(|s| s.tiles.len().div_ceil(cols)).sum::<usize>() + usize::from(sheet == Sheet::More) * 2;
    let hint = sheet.needs_selection().filter(|(id, _)| !ribbon::enabled(app, id)).map(|(_, h)| h);
    let height = sheet_height(window_h, rows, sections.len(), swatch_cmd.is_some(), hint.is_some());
    let mut ran: Ran = None;
    let mut close = false;
    egui::Panel::bottom("phone_sheet")
        .exact_size(height)
        .frame(egui::Frame::NONE.fill(t.card).inner_margin(egui::Margin::symmetric(GUTTER as i8, 0)))
        .show(ui, |ui| {
            let full = ui.max_rect();
            ui.painter().hline(full.x_range().expand(GUTTER), full.min.y, Stroke::new(1.0, t.border));
            // Header: the title at the left (List: the tab menu), Close at the right.
            ui.horizontal(|ui| {
                ui.set_min_height(HEADER_H);
                ui.spacing_mut().item_spacing.x = 0.0;
                let w = (ui.available_width() - TOUCH_H).max(0.0);
                if list {
                    let (r, resp) = ui.allocate_exact_size(vec2(w, HEADER_H), Sense::click());
                    let chip = Rect::from_min_size(
                        pos2(r.left(), r.center().y - 20.0),
                        vec2(ui.fonts_mut(|f| f.layout_no_wrap(sheet.label().into(), theme::bold(17.0), t.text).size().x) + 48.0, 40.0),
                    );
                    ui.painter().rect_filled(chip, CornerRadius::same(20), t.hover);
                    ui.painter().text(pos2(chip.left() + 14.0, chip.center().y), Align2::LEFT_CENTER, sheet.label(), theme::bold(17.0), t.text);
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(pos2(chip.right() - 18.0, chip.center().y), vec2(18.0, 18.0)),
                        Icon::ChevronDown,
                        t.text,
                        false,
                    );
                    resp.widget_info(|| WidgetInfo::labeled(egui::Role::ComboBox, true, format!("Tab: {}", sheet.label())));
                    egui::Popup::menu(&resp).id(egui::Id::unique("phone_tab_menu")).width(236.0).show(|ui| {
                        if let Some(r) = tab_menu(app, ui, sheet) {
                            ran = Some(r);
                        }
                    });
                } else if let Some(sub) = sub_page(ui.ctx()) {
                    // A sub-page: Back to the sheet, then its title.
                    if header_button(ui, Icon::ChevronLeft, "Back", &t) {
                        set_sub_page(ui.ctx(), None);
                    }
                    let (r, _) = ui.allocate_exact_size(vec2((w - TOUCH_H).max(0.0), HEADER_H), Sense::hover());
                    ui.painter().text(pos2(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER, sub.title(), theme::bold(20.0), t.text);
                } else {
                    let (r, _) = ui.allocate_exact_size(vec2(w, HEADER_H), Sense::hover());
                    ui.painter().text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, sheet.label(), theme::bold(20.0), t.text);
                }
                if header_button(ui, Icon::Close, "Close", &t) {
                    close = true;
                }
            });
            if let Some(sub) = sub_page(ui.ctx()) {
                egui::ScrollArea::vertical().id_salt(("phone_sheet_sub", sub.title())).auto_shrink([false, false]).show(ui, |ui| {
                    if let Some(r) = sub_body(app, ui, sub, &t) {
                        ran = Some(r);
                    }
                });
                return;
            }
            egui::ScrollArea::vertical().id_salt(("phone_sheet_scroll", sheet.label())).auto_shrink([false, false]).show(ui, |ui| {
                if sheet == Sheet::Text
                    && let Some(r) = font_row(app, ui, &t)
                {
                    ran = Some(r);
                }
                if let Some(h) = hint {
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HINT_H), Sense::hover());
                    ui.painter().with_clip_rect(rect).text(rect.left_center(), Align2::LEFT_CENTER, h, theme::font(13.0), t.text_dim);
                }
                if sheet == Sheet::More && list_row(ui, Icon::Search, "Search", false, &t) {
                    ran = Some((SEARCH, json!({}), false));
                }
                if let Some((id, label)) = swatch_cmd {
                    heading(ui, label, &t);
                    let en = ribbon::enabled(app, id);
                    let more = if id == "shape.fill" { "fill" } else { "fontColor" };
                    if let Some(r) = swatch_row(ui, id, label, en, id == "shape.fill", more, &t) {
                        ran = Some(r);
                    }
                }
                for sec in &sections {
                    heading(ui, sec.heading, &t);
                    let r = if list { list_section(app, ui, &sec.tiles, &t) } else { tile_grid(app, ui, &sec.tiles, cols, &t) };
                    if r.is_some() {
                        ran = r;
                    }
                }
                if sheet == Sheet::More {
                    ui.add_space(8.0);
                    if list_row(ui, Icon::List, "All tools", true, &t) {
                        ran = Some((ALL_TOOLS, json!({}), false));
                    }
                }
                ui.add_space(GUTTER);
            });
        });
    if close {
        app.phone_sheet = None;
        set_sub_page(ui.ctx(), None);
    }
    if let Some((id, params, keep)) = ran {
        tap(app, ui.ctx(), id, params, keep);
    }
}

/// Runs what a tile, row or menu entry asked for.
fn tap(app: &mut SlideApp, ctx: &egui::Context, id: &'static str, params: Value, keep: bool) {
    match id {
        ALL_TOOLS => crate::phone_tools::open(app),
        SEARCH => crate::phone_tools::open_search(app),
        SHAPES => crate::phone_tools::open_command(app, "shape.insert"),
        FONT_NAME => crate::phone_tools::open_command(app, "format.font"),
        FONT_SIZE => crate::phone_tools::open_command(app, "format.size"),
        SUB => set_sub_page(ctx, params.get("sub").and_then(Value::as_str).and_then(Sub::named)),
        TAB => {
            if let Some(s) = params.get("tab").and_then(Value::as_u64).and_then(|i| Sheet::ALL.get(i as usize)) {
                app.phone_sheet = Some(*s);
            }
        }
        _ => {
            let _ = app.run(id, params);
            if !keep {
                app.phone_sheet = None;
            }
        }
    }
}

/// The pseudo command of a tab-menu entry: switch the sheet to tab `{"tab": index}`.
const TAB: &str = "phone.tab";

/// The List style's tab menu: every tab, then Search and All tools, pinned after a divider.
fn tab_menu(app: &SlideApp, ui: &mut Ui, open: Sheet) -> Ran {
    let t = Tokens::get(ui.ctx());
    let _ = app;
    let mut ran: Ran = None;
    for (i, s) in Sheet::ALL.iter().enumerate() {
        let on = *s == open;
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH_H), Sense::click());
        resp.widget_info(|| WidgetInfo::selected(egui::Role::MenuItem, true, on, s.label()));
        if on {
            ui.painter().rect_filled(r, CornerRadius::same(24), t.selected);
        } else if resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(r, CornerRadius::same(24), t.pressed);
        }
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(r.left() + 24.0, r.center().y), vec2(20.0, 20.0)),
            s.icon(),
            if on { t.accent } else { t.text },
            false,
        );
        ui.painter().text(pos2(r.left() + 48.0, r.center().y), Align2::LEFT_CENTER, s.label(), theme::font(15.0), if on { t.accent } else { t.text });
        if resp.clicked() {
            ran = Some((TAB, json!({"tab": i}), true));
            ui.close();
        }
    }
    ui.separator();
    for (icon, label, id) in [(Icon::Search, "Search", SEARCH), (Icon::List, "All tools", ALL_TOOLS)] {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH_H), Sense::click());
        resp.widget_info(|| WidgetInfo::labeled(egui::Role::MenuItem, true, label));
        if resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(r, CornerRadius::same(24), t.pressed);
        }
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 24.0, r.center().y), vec2(20.0, 20.0)), icon, t.text, false);
        ui.painter().text(pos2(r.left() + 48.0, r.center().y), Align2::LEFT_CENTER, label, theme::font(15.0), t.text);
        if resp.clicked() {
            ran = Some((id, json!({}), false));
            ui.close();
        }
    }
    ran
}

/// A 48 pt icon button for a sheet's header. True when tapped.
fn header_button(ui: &mut Ui, icon: Icon, label: &str, t: &Tokens) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(TOUCH_H, TOUCH_H), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, label));
    if resp.is_pointer_button_down_on() {
        ui.painter().circle_filled(r.center(), 22.0, t.pressed);
    }
    icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(24.0, 24.0)), icon, t.text, false);
    resp.clicked()
}

/// A section heading, small and in the accent colour.
fn heading(ui: &mut Ui, text: &str, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADING_H), Sense::hover());
    ui.painter().text(pos2(r.left(), r.bottom() - 8.0), Align2::LEFT_BOTTOM, text, theme::bold(12.0), t.accent);
}

/// A full-width row: an icon, the label, and a chevron when it opens more. True when tapped.
fn list_row(ui: &mut Ui, icon: Icon, label: &str, chevron: bool, t: &Tokens) -> bool {
    row(ui, icon, label, chevron, true, false, t).clicked()
}

fn row(ui: &mut Ui, icon: Icon, label: &str, chevron: bool, enabled: bool, on: bool, t: &Tokens) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::selected(egui::Role::Button, enabled, on, label));
    if enabled && resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(r, CornerRadius::same(12), t.pressed);
    } else if on {
        ui.painter().rect_filled(r, CornerRadius::same(12), t.selected);
    }
    let ink = if !enabled {
        t.text.gamma_multiply(0.45)
    } else if on {
        t.accent
    } else {
        t.text
    };
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), vec2(24.0, 24.0)), icon, ink, !enabled);
    ui.painter().with_clip_rect(r).text(pos2(r.left() + 56.0, r.center().y), Align2::LEFT_CENTER, label, theme::font(16.0), ink);
    if chevron {
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.right() - 20.0, r.center().y), vec2(20.0, 20.0)), Icon::ChevronRight, ink, !enabled);
    }
    resp
}

/// The swatch row: one tap colours the selection.
/// The swatch row: one tap colours the selection. Fill starts with No Fill; the row ends with
/// More…, which opens the colour page `more`.
fn swatch_row(ui: &mut Ui, id: &'static str, label: &str, enabled: bool, none_first: bool, more: &'static str, t: &Tokens) -> Ran {
    let mut ran: Ran = None;
    let swatches: &[&str] = if none_first { &SWATCHES[..6] } else { &SWATCHES[..7] };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let each = ui.available_width() / 8.0;
        let sense = if enabled { Sense::click() } else { Sense::hover() };
        if none_first {
            let (rect, resp) = ui.allocate_exact_size(vec2(each, SWATCH + 12.0), sense);
            resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, enabled, "No Fill"));
            ui.painter().circle_stroke(rect.center(), SWATCH * 0.5 - 6.0, Stroke::new(1.5, t.border));
            ui.painter().line_segment(
                [rect.center() + vec2(-12.0, 12.0), rect.center() + vec2(12.0, -12.0)],
                Stroke::new(1.5, Color32::from_rgb(0xC0, 0, 0)),
            );
            if resp.clicked() {
                ran = Some((id, json!({"none": true}), true));
            }
        }
        for &hex in swatches {
            let (rect, resp) = ui.allocate_exact_size(vec2(each, SWATCH + 12.0), if enabled { Sense::click() } else { Sense::hover() });
            resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, enabled, format!("{label} {hex}")));
            let c = parse_hex(hex);
            let r = (SWATCH * 0.5 - 6.0) + if resp.is_pointer_button_down_on() { 2.0 } else { 0.0 };
            ui.painter().circle(rect.center(), r, if enabled { c } else { c.gamma_multiply(0.4) }, Stroke::new(1.5, t.border));
            if resp.clicked() {
                ran = Some((id, json!({"color": hex}), true));
            }
        }
        let (rect, resp) = ui.allocate_exact_size(vec2(each, SWATCH + 12.0), sense);
        resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, enabled, format!("More {label} colours")));
        ui.painter().circle_stroke(rect.center(), SWATCH * 0.5 - 6.0, Stroke::new(1.5, t.border));
        icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(20.0, 20.0)), Icon::More, t.text, !enabled);
        if resp.clicked() {
            ran = Some((SUB, json!({"sub": more}), true));
        }
    });
    ran
}

/// The Text sheet's font row: the font name (it opens the font list), −, the size (the size list), +.
fn font_row(app: &mut SlideApp, ui: &mut Ui, t: &Tokens) -> Ran {
    let st = ribbon::fmt_state(app);
    let en = ribbon::enabled(app, "format.font");
    let font = st.get("font").and_then(Value::as_str).unwrap_or("Font").to_string();
    let size = st.get("size").and_then(Value::as_f64).map(|s| crate::widgets::fmt_num(s, "")).unwrap_or_else(|| "–".into());
    let mut ran: Ran = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let w = (ui.available_width() - 3.0 * TOUCH_H - 3.0 * 4.0 - 16.0).max(TOUCH_H);
        let btn = |ui: &mut Ui, text: &str, name: &str, width: f32| -> bool {
            let (r, resp) = ui.allocate_exact_size(vec2(width, TOUCH_H), if en { Sense::click() } else { Sense::hover() });
            resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, en, name));
            ui.painter().rect_filled(r, CornerRadius::same(12), if en && resp.is_pointer_button_down_on() { t.pressed } else { t.hover });
            let ink = if en { t.text } else { t.text.gamma_multiply(0.45) };
            ui.painter().with_clip_rect(r.shrink(6.0)).text(r.center(), Align2::CENTER_CENTER, text, theme::font(15.0), ink);
            resp.clicked() && en
        };
        if btn(ui, &font, "Font", w) {
            ran = Some((FONT_NAME, json!({}), false));
        }
        if btn(ui, "−", "Decrease Font Size", TOUCH_H) {
            ran = Some(("format.shrink", json!({}), true));
        }
        if btn(ui, &size, "Font Size", TOUCH_H) {
            ran = Some((FONT_SIZE, json!({}), false));
        }
        if btn(ui, "+", "Increase Font Size", TOUCH_H) {
            ran = Some(("format.grow", json!({}), true));
        }
    });
    ran
}

/// A grid of 48 pt colour swatches, `first` (No Fill, No Color…) before them. One tap runs `id`.
fn colour_grid(ui: &mut Ui, id: &'static str, first: Option<(&str, Value)>, colours: &[&str], t: &Tokens) -> Ran {
    let mut ran: Ran = None;
    if let Some((label, params)) = first
        && row(ui, Icon::Close, label, false, true, false, t).clicked()
    {
        ran = Some((id, params, true));
    }
    let cols = ((ui.available_width() / (SWATCH + 8.0)).floor() as usize).max(1);
    for chunk in colours.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for &hex in chunk {
                let (rect, resp) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
                resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, hex));
                ui.painter().circle(rect.center(), SWATCH * 0.5 - 4.0, parse_hex(hex), Stroke::new(1.5, t.border));
                if resp.clicked() {
                    ran = Some((id, json!({"color": hex}), true));
                }
            }
        });
    }
    ran
}

/// The body of a sub-page.
fn sub_body(app: &SlideApp, ui: &mut Ui, sub: Sub, t: &Tokens) -> Ran {
    match sub {
        Sub::FontColor => colour_grid(ui, "format.color", None, &COLOURS, t),
        Sub::Fill => colour_grid(ui, "shape.fill", Some(("No Fill", json!({"none": true}))), &COLOURS, t),
        Sub::Highlight => colour_grid(ui, "format.highlight", Some(("No Color", json!({}))), &COLOURS[2..12], t),
        Sub::Outline => {
            let mut ran = colour_grid(ui, "shape.line", Some(("No Outline", json!({"none": true}))), &COLOURS, t);
            heading(ui, "Weight", t);
            for w in WEIGHTS {
                if row(ui, Icon::ShapeOutline, &format!("{} pt", crate::widgets::fmt_num(f64::from(w), "")), false, true, false, t).clicked() {
                    ran = Some(("shape.line", json!({"width": w}), true));
                }
            }
            ran
        }
        Sub::Align => {
            let mut ran: Ran = None;
            let en = ribbon::enabled(app, "arrange.align");
            for (icon, label, id, params) in [
                (Icon::AlignObjLeft, "Align Left", "arrange.align", json!({"edge": "left"})),
                (Icon::AlignObjCenter, "Align Center", "arrange.align", json!({"edge": "center"})),
                (Icon::AlignObjRight, "Align Right", "arrange.align", json!({"edge": "right"})),
                (Icon::AlignObjTop, "Align Top", "arrange.align", json!({"edge": "top"})),
                (Icon::AlignObjMiddle, "Align Middle", "arrange.align", json!({"edge": "middle"})),
                (Icon::AlignObjBottom, "Align Bottom", "arrange.align", json!({"edge": "bottom"})),
                (Icon::DistributeH, "Distribute Horizontally", "arrange.distribute", json!({"dir": "horizontal"})),
                (Icon::DistributeV, "Distribute Vertically", "arrange.distribute", json!({"dir": "vertical"})),
            ] {
                if row(ui, icon, label, false, en, false, t).clicked() && en {
                    ran = Some((id, params, true));
                }
            }
            ran
        }
    }
}

/// Whether a tile runs now: its command is enabled (a pseudo command always is).
fn tile_enabled(app: &SlideApp, tl: &Tile) -> bool {
    is_pseudo(tl.id) || ribbon::enabled(app, tl.id)
}

/// A section as a grid of tiles (Tiles style).
fn tile_grid(app: &SlideApp, ui: &mut Ui, tiles: &[Tile], cols: usize, t: &Tokens) -> Ran {
    let mut ran: Ran = None;
    let tile_w = ui.available_width() / cols as f32;
    for chunk in tiles.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for tl in chunk {
                let en = tile_enabled(app, tl);
                if tile_button(ui, tl, tile_w, en, t).clicked() && en {
                    ran = Some((tl.id, tl.params.clone(), tl.keep_open));
                }
            }
        });
    }
    ran
}

/// Whether a tile is a one-tap format or toggle, drawn in the List style's icon row.
fn one_tap(tl: &Tile) -> bool {
    tl.keep_open && tl.params.as_object().is_some_and(|o| o.is_empty()) && tl.id.starts_with("format.")
}

/// A section in the List style: its one-tap formats as a row of 48 pt icon buttons (scrolling
/// sideways when they do not fit), then one row per other command.
fn list_section(app: &SlideApp, ui: &mut Ui, tiles: &[Tile], t: &Tokens) -> Ran {
    let mut ran: Ran = None;
    let icons_row: Vec<&Tile> = tiles.iter().filter(|tl| one_tap(tl)).collect();
    if !icons_row.is_empty() {
        egui::ScrollArea::horizontal().id_salt(("phone_list_icons", tiles.first().map_or("", |t| t.id))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for tl in icons_row {
                    let en = tile_enabled(app, tl);
                    let (r, resp) = ui.allocate_exact_size(vec2(TOUCH_H, TOUCH_H), if en { Sense::click() } else { Sense::hover() });
                    resp.widget_info(|| WidgetInfo::selected(egui::Role::Button, en, tl.on, tl.shown_label()));
                    if en && resp.is_pointer_button_down_on() {
                        ui.painter().rect_filled(r, CornerRadius::same(12), t.pressed);
                    } else if tl.on {
                        ui.painter().rect_filled(r, CornerRadius::same(12), t.selected);
                    }
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(r.center(), vec2(24.0, 24.0)),
                        tl.icon,
                        if tl.on { t.accent } else { t.text },
                        !en,
                    );
                    if resp.on_hover_text(tl.shown_label()).clicked() && en {
                        ran = Some((tl.id, tl.params.clone(), tl.keep_open));
                    }
                }
            });
        });
    }
    for tl in tiles.iter().filter(|tl| !one_tap(tl)) {
        let en = tile_enabled(app, tl);
        let chevron = tl.id == SHAPES;
        if row(ui, tl.icon, tl.shown_label(), chevron, en, tl.on, t).clicked() && en {
            ran = Some((tl.id, tl.params.clone(), tl.keep_open));
        }
    }
    ran
}

fn last_id() -> egui::Id {
    egui::Id::unique("phone_last_sheet")
}

/// Keeps the sheet shown last: the List style's More opens it again.
fn remember(ctx: &egui::Context, sheet: Sheet) {
    ctx.data_mut(|d| d.insert_temp(last_id(), sheet));
}

/// The sheet shown last, or Slides.
pub fn last_sheet(ctx: &egui::Context) -> Sheet {
    ctx.data(|d| d.get_temp::<Sheet>(last_id())).unwrap_or(Sheet::Slides)
}

/// The quick bar's commands (List style): New Slide, Text Box, Picture, then Bold and Italic.
const QUICK: [(Icon, &str, &str); 5] = [
    (Icon::NewSlide, "New Slide", "slide.new"),
    (Icon::TextBox, "Text Box", "insert.textBox"),
    (Icon::Picture, "Picture", "app.insertPictureDialog"),
    (Icon::Bold, "Bold", "format.bold"),
    (Icon::Italic, "Italic", "format.italic"),
];

/// The List style's floating quick bar, above the slide chips: a few common commands and More,
/// which opens the sheet shown last. It hides while a sheet or All tools is open.
pub fn quick_bar(app: &mut SlideApp, ui: &mut Ui) {
    if app.phone_sheet.is_some() || !app.phone_tools.is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let st = ribbon::fmt_state(app);
    let mut ran: Option<&'static str> = None;
    let mut more = false;
    egui::Panel::bottom("phone_quick_bar")
        .exact_size(TOUCH_H + 8.0 + 16.0)
        // The strip around the pill takes the canvas colour: with no fill it showed black on Android.
        .frame(egui::Frame::NONE.fill(t.canvas).inner_margin(egui::Margin { left: GUTTER as i8, right: GUTTER as i8, top: 0, bottom: 16 }))
        .show(ui, |ui| {
            let bar = ui.max_rect();
            ui.painter().add(
                egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(50) }.as_shape(bar, CornerRadius::same(28)),
            );
            ui.painter().rect_filled(bar, CornerRadius::same(28), t.card);
            let n = QUICK.len() as f32 + 1.0;
            let used = n * TOUCH_H + 9.0 * 2.0;
            let x0 = bar.center().x - used / 2.0;
            let mut x = x0;
            let button = |ui: &mut Ui, x: f32, icon: Icon, label: &str, enabled: bool, on: bool| -> bool {
                let r = Rect::from_min_size(pos2(x, bar.center().y - TOUCH_H / 2.0), vec2(TOUCH_H, TOUCH_H));
                let resp = ui.interact(r, ui.scope_id().with(("phone_quick", label)), if enabled { Sense::click() } else { Sense::hover() });
                resp.widget_info(|| WidgetInfo::selected(egui::Role::Button, enabled, on, label));
                if enabled && resp.is_pointer_button_down_on() {
                    ui.painter().rect_filled(r.shrink(2.0), CornerRadius::same(12), t.pressed);
                } else if on {
                    ui.painter().rect_filled(r.shrink(2.0), CornerRadius::same(12), t.selected);
                }
                icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(24.0, 24.0)), icon, if on { t.accent } else { t.text }, !enabled);
                resp.clicked() && enabled
            };
            for (i, (icon, label, id)) in QUICK.iter().enumerate() {
                if i == 3 {
                    ui.painter().vline(x + 4.5, (bar.center().y - 12.0)..=(bar.center().y + 12.0), Stroke::new(1.0, t.border));
                    x += 9.0;
                }
                let on = match *id {
                    "format.bold" => st.get("bold").and_then(Value::as_bool).unwrap_or(false),
                    "format.italic" => st.get("italic").and_then(Value::as_bool).unwrap_or(false),
                    _ => false,
                };
                if button(ui, x, *icon, label, ribbon::enabled(app, id), on) {
                    ran = Some(id);
                }
                x += TOUCH_H;
            }
            ui.painter().vline(x + 4.5, (bar.center().y - 12.0)..=(bar.center().y + 12.0), Stroke::new(1.0, t.border));
            x += 9.0;
            if button(ui, x, Icon::More, "More", true, false) {
                more = true;
            }
        });
    if let Some(id) = ran {
        let _ = app.run(id, json!({}));
    }
    if more {
        app.phone_sheet = Some(last_sheet(ui.ctx()));
    }
}

/// One tile: the icon, then the label on up to two lines, broken only at spaces (a word too wide
/// for a line is drawn smaller, down to 9 pt). An "on" tile is tinted; a disabled one greyed.
fn tile_button(ui: &mut Ui, tl: &Tile, w: f32, enabled: bool, t: &Tokens) -> egui::Response {
    let label = tl.shown_label();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, TILE_H), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::selected(egui::Role::Button, enabled, tl.on, label));
    if enabled && resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect.shrink(3.0), CornerRadius::same(12), t.pressed);
    } else if tl.on {
        ui.painter().rect_filled(rect.shrink(3.0), CornerRadius::same(12), t.selected);
    }
    let ink = if tl.on { t.accent } else { t.text };
    let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.min.y + 22.0), vec2(24.0, 24.0));
    match tl.params.get("layout").and_then(Value::as_str).filter(|_| tl.id == "slide.layout") {
        Some(layout) => paint_layout(ui.painter(), icon_rect, layout, if enabled { ink } else { ink.gamma_multiply(0.45) }, t),
        None => icons::paint(ui.painter(), icon_rect, tl.icon, ink, !enabled),
    }
    let label_ink = if enabled { ink } else { ink.gamma_multiply(0.45) };
    let max_w = (w - 4.0).max(1.0);
    let widest = label.split(' ').map(|word| ui.painter().layout_no_wrap(word.to_owned(), theme::font(12.0), label_ink).size().x).fold(0.0, f32::max);
    let size = if widest <= max_w { 12.0 } else { (12.0 * max_w / widest).floor().clamp(9.0, 11.0) };
    let mut job = egui::text::LayoutJob::simple(label.to_owned(), theme::font(size), label_ink, max_w);
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');
    job.halign = egui::Align::Center;
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(pos2(rect.center().x, rect.min.y + 40.0), galley, label_ink);
    resp
}

/// A small picture of a slide layout: the slide's outline, the title bar, and the content boxes.
fn paint_layout(p: &egui::Painter, icon: Rect, layout: &str, ink: Color32, t: &Tokens) {
    let slide = Rect::from_center_size(icon.center(), vec2(32.0, 20.0));
    p.rect_stroke(slide, CornerRadius::same(2), Stroke::new(1.2, ink), egui::StrokeKind::Inside);
    let fill = ink.gamma_multiply(0.35);
    let r = |x0: f32, y0: f32, x1: f32, y1: f32| {
        Rect::from_min_max(
            pos2(slide.left() + x0 * slide.width(), slide.top() + y0 * slide.height()),
            pos2(slide.left() + x1 * slide.width(), slide.top() + y1 * slide.height()),
        )
    };
    let bars: Vec<Rect> = match layout {
        "title" => vec![r(0.15, 0.30, 0.85, 0.50), r(0.30, 0.58, 0.70, 0.68)],
        "titleAndContent" => vec![r(0.12, 0.12, 0.88, 0.26), r(0.12, 0.36, 0.88, 0.86)],
        "sectionHeader" => vec![r(0.12, 0.40, 0.80, 0.58), r(0.12, 0.64, 0.60, 0.72)],
        "twoContent" => vec![r(0.12, 0.12, 0.88, 0.26), r(0.12, 0.36, 0.47, 0.86), r(0.53, 0.36, 0.88, 0.86)],
        "comparison" => vec![
            r(0.12, 0.10, 0.88, 0.22),
            r(0.12, 0.30, 0.47, 0.38),
            r(0.53, 0.30, 0.88, 0.38),
            r(0.12, 0.44, 0.47, 0.88),
            r(0.53, 0.44, 0.88, 0.88),
        ],
        "titleOnly" => vec![r(0.12, 0.12, 0.88, 0.26)],
        _ => vec![],
    };
    let _ = t;
    for b in bars {
        p.rect_filled(b, CornerRadius::same(1), fill);
    }
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
                assert!(is_pseudo(tl.id) || ribbon::known(tl.id), "{:?} tile `{}` runs unknown command `{}`", s, tl.label, tl.id);
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
        let h = sheet_height(800.0, 6, 3, false, false);
        assert!(h <= 800.0 * MAX_FRACTION + 0.01, "{h}");
        assert!(h >= TILE_H, "a sheet always shows a whole tile row");
        // A short sheet takes only what it needs.
        assert!(sheet_height(800.0, 1, 1, false, false) < h);
        // A tiny window still shows a row.
        assert!(sheet_height(100.0, 6, 3, true, true) >= TILE_H);
        // A short window (a phone held sideways) lets the sheet take more of it.
        assert!(sheet_height(400.0, 6, 3, false, false) <= 400.0 * MAX_FRACTION_SHORT + 0.01);
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
        let up = tiles(&mut app, Sheet::Slides).into_iter().find(|t| t.label == "Move Left").expect("a Move Left tile");
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
        // The List style: the quick bar, then the Text sheet with its tab menu chip.
        harness.state_mut().ui.phone_list_style = true;
        for (name, s) in [("list-closed", None), ("list-text", Some(Sheet::Text))] {
            harness.state_mut().phone_sheet = s;
            harness.run_steps(3);
            if let Ok(img) = harness.render() {
                let _ = img.save(format!("{dir}/sheet-{name}.png"));
            }
        }
        harness.state_mut().ui.phone_list_style = false;
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

#[cfg(test)]
mod style_tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    type H = egui_kittest::Harness<'static, SlideApp>;

    fn harness(list: bool) -> H {
        let mut app = SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
        deckcraft_engine::sample::open_sample(&mut app.session).expect("sample deck opens");
        app.ui.phone_list_style = list;
        let mut h = egui_kittest::Harness::builder().with_size(vec2(360.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        h
    }

    fn has(h: &H, label: &str) -> bool {
        h.query_all_by_label(label).next().is_some()
    }

    /// Both styles draw every sheet at 360 pt and list the same commands per sheet (one model).
    #[test]
    fn both_styles_draw_every_sheet() {
        for list in [false, true] {
            let mut h = harness(list);
            for s in Sheet::ALL {
                h.state_mut().phone_sheet = Some(s);
                h.run_steps(3);
                assert!(has(&h, "Close"), "{s:?} has a Close button (list {list})");
            }
        }
    }

    /// Tiles has a tab bar and no quick bar; List has the quick bar and no tab bar, and its More opens
    /// the sheet shown last.
    #[test]
    fn list_style_has_a_quick_bar_and_no_tab_bar() {
        let h = harness(false);
        assert!(has(&h, "Slides") && has(&h, "Insert"), "Tiles draws the tab bar");
        assert!(!has(&h, "New Slide"), "no quick bar in Tiles");
        let mut h = harness(true);
        assert!(!has(&h, "Insert"), "no tab bar in List");
        assert!(has(&h, "New Slide") && has(&h, "Bold"), "the quick bar shows");
        h.get_by_role_and_label(egui::accesskit::Role::Button, "More").click();
        h.run_steps(3);
        assert_eq!(h.state().phone_sheet, Some(Sheet::Slides), "More opens the first sheet the first time");
        h.state_mut().phone_sheet = Some(Sheet::Text);
        h.run_steps(2);
        h.state_mut().phone_sheet = None;
        h.run_steps(2);
        h.get_by_role_and_label(egui::accesskit::Role::Button, "More").click();
        h.run_steps(3);
        assert_eq!(h.state().phone_sheet, Some(Sheet::Text), "More opens the sheet shown last");
    }

    /// The More sheet starts with Search and ends with All tools; both open the tools.
    #[test]
    fn more_has_search_and_all_tools() {
        let mut h = harness(false);
        h.state_mut().phone_sheet = Some(Sheet::More);
        h.run_steps(3);
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Search").click();
        h.run_steps(3);
        assert!(matches!(h.state().phone_tools.last(), Some(crate::phone_tools::Tool::Search(_))), "Search opens the tools on Search");
    }

    /// Tile labels are the engine's, for tiles that run a command as it is.
    #[test]
    fn plain_tiles_show_the_engine_label() {
        let mut app = SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
        deckcraft_engine::sample::open_sample(&mut app.session).expect("sample deck opens");
        let text = tiles(&mut app, Sheet::Text);
        let center = text.iter().find(|t| t.id == "format.alignCenter").map(Tile::shown_label);
        assert_eq!(center, deckcraft_engine::find_command("format.alignCenter").map(|c| c.label));
        let layout = tiles(&mut app, Sheet::Slides).into_iter().find(|t| t.id == "slide.layout").map(|t| t.shown_label());
        assert_eq!(layout, Some("Title Slide"), "a tile with fixed values keeps its own label");
    }

    /// While text is edited the keyboard bar replaces the tab bar and sheet; Done ends the edit.
    #[test]
    fn keyboard_bar_while_editing_text() {
        let mut h = harness(false);
        h.state_mut().phone_sheet = Some(Sheet::Insert);
        let id = h.state().session.active().and_then(|d| d.doc.slides.first().and_then(|s| s.shapes.first().map(|sh| sh.id.0)));
        h.state_mut().run("text.edit", json!({"id": id})).expect("edit the title's text");
        h.run_steps(3);
        assert!(has(&h, "Done") && has(&h, "Bold") && has(&h, "Larger"), "the keyboard bar shows");
        assert!(!has(&h, "Slides"), "the tab bar hides while typing");
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Done").click();
        h.run_steps(3);
        assert!(!crate::phone::editing_text(h.state()), "Done ends the edit");
        assert!(has(&h, "Slides"), "the tab bar returns");
    }

    /// A long press (a secondary click here) on the slide opens the selection bar, not the context
    /// menu. Duplicate runs and closes it; More opens the rest; Back closes More, then the bar.
    #[test]
    fn long_press_opens_the_selection_bar() {
        let mut h = harness(false);
        let id = h.state().session.active().and_then(|d| d.doc.slides.first().and_then(|s| s.shapes.first().map(|sh| sh.id.0)));
        h.state_mut().run("edit.select", json!({"ids": [id]})).expect("select the title");
        h.run_steps(2);
        let at = pos2(180.0, 200.0);
        for pressed in [true, false] {
            h.input_mut().events.extend([
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE },
            ]);
            h.run_steps(1);
        }
        h.run_steps(2);
        assert!(h.state().phone_select.is_some(), "the selection bar opened");
        for name in ["Cut", "Copy", "Paste", "Duplicate", "More actions"] {
            assert!(has(&h, name), "{name} is on the bar");
        }
        h.get_by_role_and_label(egui::accesskit::Role::Button, "More actions").click();
        h.run_steps(2);
        assert!(has(&h, "Bring to Front") && has(&h, "Alt Text…"), "More lists the rest");
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        assert_eq!(h.state().phone_select.map(|b| b.more), Some(false), "Back closes More first");
        let count = |h: &H| h.state().session.active().and_then(|d| d.doc.slides.first().map(|s| s.shapes.len())).unwrap_or(0);
        // The press may have landed beside the title: select it again, as a press on it would.
        h.state_mut().run("edit.select", json!({"ids": [id]})).expect("select the title");
        h.run_steps(2);
        let before = count(&h);
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Duplicate").click();
        h.run_steps(2);
        assert_eq!(count(&h), before + 1, "Duplicate ran");
        assert!(h.state().phone_select.is_none(), "an action closes the bar");
    }

    /// Highlight, Outline and Align open sub-pages with a Back arrow; a swatch there runs its
    /// command; Back (Escape) returns to the sheet first. The Text sheet has the font row.
    #[test]
    fn sub_pages_and_the_font_row() {
        let mut h = harness(false);
        let id = h.state().session.active().and_then(|d| d.doc.slides.first().and_then(|s| s.shapes.first().map(|sh| sh.id.0)));
        h.state_mut().run("edit.select", json!({"ids": [id]})).expect("select the title");
        h.state_mut().phone_sheet = Some(Sheet::Text);
        h.run_steps(3);
        assert!(has(&h, "Font") && has(&h, "Font Size") && has(&h, "Increase Font Size"), "the font row shows");
        h.get_by_role_and_label(egui::accesskit::Role::Button, "More Font Color colours").click();
        h.run_steps(3);
        assert!(has(&h, "Back") && has(&h, "#92D050"), "the font colour page shows its swatches");
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        assert_eq!(h.state().phone_sheet, Some(Sheet::Text), "Back closes the page, not the sheet");
        assert!(!has(&h, "#92D050"));
        h.state_mut().phone_sheet = Some(Sheet::Shape);
        h.run_steps(3);
        assert!(has(&h, "No Fill"), "Fill starts with No Fill");
        // The Outline… tile sits below the fold of a 800 pt window: open its page as the tile does.
        set_sub_page(&h.ctx, Some(Sub::Outline));
        h.run_steps(3);
        assert!(has(&h, "No Outline") && has(&h, "0.75 pt"), "the outline page lists colours and weights");
        h.get_by_role_and_label(egui::accesskit::Role::Button, "0.75 pt").click();
        h.run_steps(2);
        assert!(h.state().phone_sheet.is_some(), "a format keeps the sheet open");
    }

    /// `view.theme` sets Light, Dark or System, and `ui.phoneSheetStyle` the sheet style.
    #[test]
    fn theme_and_style_commands() {
        let mut app = SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
        app.run("view.theme", json!({"mode": "dark"})).expect("dark");
        assert_eq!(app.ui.brightness, theme::Brightness::Dark);
        app.run("view.theme", json!({"mode": "system"})).expect("system");
        assert!(app.ui.follow_system);
        app.run("view.dark", json!({"on": false})).expect("view.dark still works");
        assert!(!app.ui.follow_system && app.ui.brightness == theme::Brightness::Light);
        assert!(app.run("view.theme", json!({"mode": "blue"})).is_err());
        app.run("ui.phoneSheetStyle", json!({"style": "list"})).expect("list");
        assert!(app.ui.phone_list_style);
        assert!(app.run("ui.phoneSheetStyle", json!({"style": "grid"})).is_err());
        assert!(!crate::state::UiState::default().phone_list_style, "Tiles is the default");
    }
}
