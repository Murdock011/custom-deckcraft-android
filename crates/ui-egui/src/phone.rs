//! The phone layout: a narrow window (a phone held upright) gets a top app bar, a row of slide
//! chips and a bottom toolbar in place of the title bar, ribbon, thumbnail pane and status bar.
//! The slide keeps its canvas. Every control runs a command id, so the phone chrome adds no
//! behaviour of its own: it is the ribbon's commands behind bigger touch targets.

use egui::{Align2, Context, CornerRadius, Rect, Sense, Ui, WidgetInfo, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// Windows narrower than this (points) get the phone layout. A phone held upright is about 390 pt.
pub const MAX_WIDTH: f32 = 600.0;
/// The Android minimum touch target, in points: every button and chip is at least this tall.
pub const TOUCH_H: f32 = 48.0;
/// Height of the top app bar.
const APP_BAR_H: f32 = 56.0;
/// Width of a slide chip.
const CHIP_W: f32 = 56.0;

/// Whether the phone layout is in use: the window is narrow. Show mode has its own full-window view.
pub fn active(ctx: &Context) -> bool {
    ctx.content_rect().width() < MAX_WIDTH
}

/// A square icon button of [`TOUCH_H`], running nothing itself: the caller acts on the response.
/// `on` draws the button as switched on (the accent colour).
fn icon_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, on: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(TOUCH_H, TOUCH_H), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if enabled && resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect.shrink(4.0), CornerRadius::same(24), t.pressed);
    } else if on {
        ui.painter().rect_filled(rect.shrink(4.0), CornerRadius::same(24), t.selected);
    }
    let ink = if on { t.accent } else { t.text };
    icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(24.0, 24.0)), icon, ink, !enabled);
    resp
}

/// The top app bar: File (the Backstage), the title, Undo, Redo and Play from Start.
pub fn top_bar(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (title, dirty) = app.session.active().map(|d| (d.title(), d.is_dirty())).unwrap_or_default();
    egui::Panel::top("phone_top").exact_size(APP_BAR_H).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if icon_button(ui, Icon::File, "File", true, app.backstage_open()).clicked() {
                let _ = app.run("app.home", json!({}));
            }
            let right = 3.0 * TOUCH_H;
            let w = (ui.available_width() - right).max(0.0);
            let (rect, _) = ui.allocate_exact_size(vec2(w, TOUCH_H), Sense::hover());
            let text = if dirty { format!("{title} •") } else { title };
            ui.painter().with_clip_rect(rect).text(pos2(rect.min.x + 4.0, rect.center().y), Align2::LEFT_CENTER, text, theme::font(16.0), t.text);
            let has_doc = app.session.active().is_some();
            if icon_button(ui, Icon::Undo, "Undo", has_doc, false).clicked() {
                let _ = app.run("edit.undo", json!({}));
            }
            if icon_button(ui, Icon::Redo, "Redo", has_doc, false).clicked() {
                let _ = app.run("edit.redo", json!({}));
            }
            if icon_button(ui, Icon::PlayFromStart, "Play from start", has_doc, false).clicked() {
                let _ = app.run("show.fromStart", json!({}));
            }
        });
    });
}

/// The bottom: a scrolling row of slide chips (tap to go to a slide), then the toolbar of the
/// commands a phone needs most often.
pub fn bottom_bar(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((count, current)) = app.session.active().map(|d| (d.doc.slides.len(), d.selection.slide)) else { return };
    egui::Panel::bottom("phone_bottom").exact_size(TOUCH_H * 2.0 + 8.0).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        egui::ScrollArea::horizontal().id_salt("phone_chips").auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for i in 0..count {
                    if slide_chip(ui, i, i == current).clicked() {
                        let _ = app.run("slide.go", json!({"index": i}));
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let each = ui.available_width() / 6.0;
            ui.spacing_mut().item_spacing.x = (each - TOUCH_H).max(0.0);
            let tools: [(Icon, &str, &str); 6] = [
                (Icon::NewSlide, "New slide", "slide.new"),
                (Icon::TextBox, "Text box", "insert.textBox"),
                (Icon::Bold, "Bold", "format.bold"),
                (Icon::Italic, "Italic", "format.italic"),
                (Icon::Underline, "Underline", "format.underline"),
                (Icon::DuplicateSlide, "Duplicate slide", "slide.duplicate"),
            ];
            for (icon, label, id) in tools {
                if icon_button(ui, icon, label, true, false).clicked() {
                    let _ = app.run(id, json!({}));
                }
            }
        });
    });
}

/// One slide chip: its number on a [`TOUCH_H`]-tall tile, filled when it is the current slide.
fn slide_chip(ui: &mut Ui, index: usize, current: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(CHIP_W, TOUCH_H), Sense::click());
    let label = format!("Slide {}", index + 1);
    resp.widget_info(|| WidgetInfo::selected(egui::Role::Button, true, current, &label));
    let (fill, ink) = if current { (t.accent, t.accent_text) } else { (t.card, t.text) };
    ui.painter().rect_filled(rect.shrink2(vec2(0.0, 4.0)), CornerRadius::same(8), fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, (index + 1).to_string(), theme::font(15.0), ink);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_windows_get_the_phone_layout() {
        let ctx = Context::default();
        let run = |w: f32| {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(w, 800.0))), ..Default::default() };
            let mut seen = false;
            ctx.run_ui(raw, |ui| seen = active(ui.ctx())).drop_without_applying_deltas();
            seen
        };
        assert!(run(390.0), "a phone held upright");
        assert!(!run(MAX_WIDTH), "the breakpoint is exclusive");
        assert!(!run(1280.0), "a desktop window");
    }

    /// A whole deck drawn at phone width: the chips and toolbar run commands, and the slide count
    /// and current slide follow them.
    #[test]
    fn the_phone_chrome_runs_commands() {
        let ctx = Context::default();
        let mut app = SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
        deckcraft_engine::sample::open_sample(&mut app.session).expect("sample deck opens");
        // Fonts and style are set up before the first full pass, as the other UI tests do.
        app.fonts_ready = true;
        app.styled = true;
        let frame = |app: &mut SlideApp| {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(390.0, 800.0))), ..Default::default() };
            ctx.run_ui(raw, |ui| app.ui(ui)).drop_without_applying_deltas();
        };
        frame(&mut app);
        let before = app.session.active().map(|d| d.doc.slides.len()).unwrap_or(0);
        app.run("slide.new", json!({})).expect("New slide runs");
        frame(&mut app);
        assert_eq!(app.session.active().map(|d| d.doc.slides.len()), Some(before + 1));
        app.run("slide.go", json!({"index": 0})).expect("a chip's command runs");
        frame(&mut app);
        assert_eq!(app.session.active().map(|d| d.selection.slide), Some(0));
    }
}
