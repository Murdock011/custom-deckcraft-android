//! The phone layout: a narrow window (a phone held upright) gets a top app bar, a row of slide
//! chips and a bottom toolbar in place of the title bar, ribbon, thumbnail pane and status bar.
//! The slide keeps its canvas. Every control runs a command id, so the phone chrome adds no
//! behaviour of its own: it is the ribbon's commands behind bigger touch targets.

use egui::{Align2, Color32, Context, CornerRadius, Rect, Sense, Stroke, Ui, WidgetInfo, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::phone_sheet;
use crate::theme::{self, Tokens};

/// Windows narrower than this (points) get the phone layout. A phone held upright is about 390 pt.
pub const MAX_WIDTH: f32 = 600.0;
/// Windows shorter than this get it too: a phone held sideways is wider than [`MAX_WIDTH`] but only about
/// 360 to 430 pt tall, and the desktop ribbon would leave no room for the slide.
pub const MAX_HEIGHT: f32 = 500.0;

/// Whether a window of `size` points is a phone's: narrow (held upright) or short (held sideways).
pub fn is_phone_size(size: egui::Vec2) -> bool {
    size.x < MAX_WIDTH || size.y < MAX_HEIGHT
}
/// The Android minimum touch target, in points: every button and chip is at least this tall.
pub const TOUCH_H: f32 = 48.0;
/// Height of the top app bar, and in a window under 500 pt tall (a phone held sideways).
const APP_BAR_H: f32 = 64.0;
const APP_BAR_H_SHORT: f32 = 48.0;
/// Height of a slide chip (its thumbnail). The width follows the slide's shape.
const CHIP_H: f32 = 52.0;

/// Whether the phone layout is in use: the window is narrow or short. Show mode has its own full-window view.
pub fn active(ctx: &Context) -> bool {
    is_phone_size(ctx.content_rect().size())
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

/// The top app bar: File (the Backstage), the title (with ` •` for unsaved changes), Undo, Redo,
/// Save and Play from Start. Undo and Redo are disabled when they have nothing to do.
pub fn top_bar(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (title, dirty) = app.session.active().map(|d| (d.title(), d.is_dirty())).unwrap_or_default();
    let height = if ui.ctx().content_rect().height() < MAX_HEIGHT { APP_BAR_H_SHORT } else { APP_BAR_H };
    egui::Panel::top("phone_top").exact_size(height).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if icon_button(ui, Icon::File, "File", true, app.backstage_open()).clicked() {
                let _ = app.run("app.home", json!({}));
            }
            let right = 4.0 * TOUCH_H;
            let w = (ui.available_width() - right).max(0.0);
            let (rect, _) = ui.allocate_exact_size(vec2(w, TOUCH_H), Sense::hover());
            let text = if dirty { format!("{title} •") } else { title };
            ui.painter().with_clip_rect(rect).text(pos2(rect.min.x + 4.0, rect.center().y), Align2::LEFT_CENTER, text, theme::bold(18.0), t.text);
            let has_doc = app.session.active().is_some();
            let undo = has_doc && deckcraft_engine::cmd::can_undo(&app.session).is_ok();
            let redo = has_doc && deckcraft_engine::cmd::can_redo(&app.session).is_ok();
            if icon_button(ui, Icon::Undo, "Undo", undo, false).clicked() {
                let _ = app.run("edit.undo", json!({}));
            }
            if icon_button(ui, Icon::Redo, "Redo", redo, false).clicked() {
                let _ = app.run("edit.redo", json!({}));
            }
            if icon_button(ui, Icon::Save, "Save", has_doc, false).clicked() {
                let _ = app.run("file.save", json!({}));
            }
            if icon_button(ui, Icon::PlayFromStart, "Play from start", has_doc, false).clicked() {
                let _ = app.run("show.fromStart", json!({}));
            }
        });
    });
}

/// The bottom, from the screen edge up: the tab bar, the open sheet (if any), then a scrolling
/// row of slide chips (tap to go to a slide). The sheet sits above the tabs and below the chips,
/// so slide navigation stays in reach while it is open.
pub fn bottom_bar(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((doc, current)) = app.session.active().map(|d| (d.doc.clone(), d.selection.slide)) else { return };
    // Tiles: the tab bar at the bottom, the sheet above it. List: no tab bar; the sheet at the
    // bottom, or the floating quick bar above the slide chips while no sheet is open.
    let list = phone_sheet::list_style(app);
    if !list {
        phone_sheet::tab_bar(app, ui);
    }
    phone_sheet::show(app, ui);
    let ppp = ui.ctx().pixels_per_point();
    let aspect = (doc.slide_size.width / doc.slide_size.height.max(1.0)).clamp(0.5, 3.0) as f32;
    let chip_w = (CHIP_H * aspect).round();
    // Only chips in view render a thumbnail, a few a frame: a long deck settles over a few frames.
    let mut budget = 3u32;
    let mut pending = false;
    egui::Panel::bottom("phone_chips").exact_size(CHIP_H + 12.0).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        egui::ScrollArea::horizontal().id_salt("phone_chips").auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // Room for the current slide's 3 pt frame at the screen edge.
                ui.add_space(6.0);
                for i in 0..doc.slides.len() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(chip_w, CHIP_H), Sense::click());
                    let selected = i == current;
                    let label = format!("Slide {}", i + 1);
                    resp.widget_info(|| WidgetInfo::selected(egui::Role::Button, true, selected, &label));
                    let visible = ui.is_rect_visible(rect.expand2(vec2(chip_w, 0.0)));
                    let tex = if visible {
                        app.textures.thumb(ui.ctx(), &doc, i, (chip_w * ppp).round() as u32, app.ui.grayscale, &mut budget)
                    } else {
                        None
                    };
                    pending |= visible && tex.is_none();
                    let hidden = doc.slides.get(i).is_some_and(|sl| sl.hidden);
                    slide_chip(ui, rect, i, selected, hidden, tex, &t);
                    if resp.clicked() {
                        let _ = app.run("slide.go", json!({"index": i}));
                    }
                }
            });
        });
    });
    if list {
        phone_sheet::quick_bar(app, ui);
    }
    if pending {
        ui.ctx().request_repaint();
    }
}

/// One slide chip: the slide's thumbnail with its number in a corner. The current slide has an
/// accent frame; a hidden slide is faded and struck through. It is [`CHIP_H`] tall (above the 48 pt
/// touch minimum) and as wide as the slide's shape.
fn slide_chip(ui: &mut Ui, rect: Rect, index: usize, current: bool, hidden: bool, tex: Option<egui::TextureHandle>, t: &Tokens) {
    let p = ui.painter();
    match tex {
        Some(tex) => {
            p.image(
                tex.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                if hidden { Color32::from_white_alpha(120) } else { Color32::WHITE },
            );
        }
        None => {
            p.rect_filled(rect, CornerRadius::same(4), Color32::WHITE);
        }
    }
    if hidden {
        p.line_segment([rect.left_bottom(), rect.right_top()], Stroke::new(1.5, t.text_dim));
    }
    let (frame, width) = if current { (t.accent, 3.0) } else { (t.border, 1.0) };
    p.rect_stroke(rect, CornerRadius::same(4), Stroke::new(width, frame), egui::StrokeKind::Outside);
    // The number sits on a small tile so it reads over any slide.
    let badge = Rect::from_min_size(rect.min + vec2(3.0, 3.0), vec2(20.0, 16.0));
    p.rect_filled(badge, CornerRadius::same(4), if current { t.accent } else { t.chrome.gamma_multiply(0.85) });
    p.text(badge.center(), Align2::CENTER_CENTER, (index + 1).to_string(), theme::font(11.0), if current { t.accent_text } else { t.text });
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

    #[test]
    fn a_phone_held_sideways_gets_the_phone_layout() {
        assert!(is_phone_size(vec2(844.0, 390.0)), "a phone held sideways is wide but short");
        assert!(!is_phone_size(vec2(1280.0, 720.0)), "a desktop window");
        assert!(!is_phone_size(vec2(800.0, 1280.0)), "a tablet held upright");
        assert!(!is_phone_size(vec2(1280.0, MAX_HEIGHT)), "the breakpoint is exclusive");
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
