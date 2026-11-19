//! Dialogs on a phone (the shared phone shell, plan part 1.9): every dialog is a bottom sheet the
//! full width of the screen, with its title at the left and Close at the right, its body scrolling,
//! and its action buttons pinned at the bottom, equal in width and 48 pt tall. Ported from
//! GridCraft's `dialog_sheet`.

use egui::{Align2, Color32, Context, CornerRadius, Layout, Order, Pos2, Rect, Sense, Ui, WidgetInfo, vec2};

use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// A touch target (Android's 48 pt minimum): dialog buttons, rows and text fields.
pub const TOUCH_H: f32 = 48.0;
/// A dialog sheet's title row, and its pinned action row: a 48 pt row with 12 pt above and below.
const TITLE_H: f32 = 56.0;
const FOOTER_H: f32 = TOUCH_H + 24.0;
/// The gutter at a dialog sheet's sides and under its content.
const GUTTER: f32 = 16.0;
/// A dialog sheet's top corner radius, and the share of the screen past which it fills the screen.
const RADIUS: u8 = 28;
const FULL: f32 = 0.85;
const _: () = assert!(TOUCH_H >= 48.0 && TITLE_H >= 48.0);

fn height_key() -> egui::Id {
    egui::Id::unique("phone_dialog_sheet_h")
}

fn footer_key() -> egui::Id {
    egui::Id::unique("phone_dialog_footer")
}

fn wanted_key() -> egui::Id {
    egui::Id::unique("phone_dialog_footer_wanted")
}

/// The open dialog sheet's pinned action row this frame, or None when no sheet has one. Asking for
/// it also marks that the sheet has an action row, so the next frame reserves the space for it.
pub fn footer(ctx: &Context) -> Option<Rect> {
    ctx.data_mut(|d| d.insert_temp(wanted_key(), true));
    ctx.data(|d| d.get_temp::<Rect>(footer_key()))
}

/// Touch sizing for a dialog's controls: 48 pt minimum height for buttons, checkboxes, radio
/// buttons, combo boxes and sliders, and 16 pt text.
pub fn touch_style(ui: &mut Ui) {
    let style = ui.style_mut();
    style.spacing.interact_size.y = TOUCH_H;
    style.spacing.button_padding = vec2(12.0, 12.0);
    for ts in [egui::TextStyle::Body, egui::TextStyle::Button] {
        style.text_styles.insert(ts, theme::font(16.0));
    }
}

/// A single-line text field, 48 pt tall on the phone (egui's TextEdit ignores `interact_size`, so
/// the height is its `min_size`). Unchanged on the desktop.
pub fn touch_edit<'t>(ui: &Ui, edit: egui::TextEdit<'t>) -> egui::TextEdit<'t> {
    if crate::phone::active(ui.ctx()) { edit.min_size(vec2(0.0, TOUCH_H)).align(Align2::LEFT_CENTER) } else { edit }
}

/// Frames a scroll area keeps the focused widget in view after the visible area or the focus changed.
const FOLLOW_FRAMES: u8 = 3;

/// Keeps the focused widget in view while the visible area changes (the keyboard coming up or
/// going down) or the focus moves. Call inside a scroll area, after its content.
fn follow_focus(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let h = ctx.content_rect().height();
    let focus = ctx.memory(|m| m.focused());
    let key = egui::Id::unique("phone_follow_focus");
    let (prev_focus, prev_h, prev_left) = ctx.data(|d| d.get_temp::<(Option<egui::Id>, f32, u8)>(key)).unwrap_or((None, h, 0));
    let left = if focus != prev_focus || (prev_h - h).abs() > 1.0 { FOLLOW_FRAMES } else { prev_left };
    ctx.data_mut(|d| d.insert_temp(key, (focus, h, left.saturating_sub(1))));
    if left == 0 {
        return;
    }
    if let Some(r) = focus.and_then(|id| ctx.read_response(id)) {
        ui.scroll_to_rect(r.rect, None);
    }
    ctx.request_repaint();
}

/// Shows a dialog as a bottom sheet: the full width of the screen, a title row with Close, the
/// body scrolling in the space left, and its action row pinned at the bottom (see [`footer`]). The
/// sheet is as tall as its content was last frame, and the whole screen once that is over 85 % of
/// it. A tap on the dimmed screen above it closes it. `body` returns true to close. Returns
/// (`close` from the body, closed by Close or a tap outside).
pub fn dialog_sheet(ctx: &Context, id: egui::Id, title: &str, body: impl FnOnce(&mut Ui) -> bool) -> (bool, bool) {
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    let max_h = screen.height() * FULL;
    let wanted = ctx.data_mut(|d| {
        let w = d.get_temp::<bool>(wanted_key()).unwrap_or(true);
        d.insert_temp(wanted_key(), false);
        w
    });
    let total = ctx.data(|d| d.get_temp::<f32>(height_key())).unwrap_or(max_h);
    let full = total >= max_h;
    let h = if full { screen.height() } else { total };
    let rect = Rect::from_min_max(Pos2::new(screen.left(), screen.bottom() - h), screen.right_bottom());
    let footer_h = if wanted { FOOTER_H } else { 0.0 };
    let footer_rect = wanted.then(|| Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - FOOTER_H), rect.right_bottom()));
    let body_rect = Rect::from_min_max(Pos2::new(rect.left(), rect.top() + TITLE_H), Pos2::new(rect.right(), rect.bottom() - footer_h));
    ctx.data_mut(|d| match footer_rect {
        Some(r) => {
            d.insert_temp(footer_key(), r);
        }
        None => d.remove::<Rect>(footer_key()),
    });
    // The dimmed screen above the sheet: a tap there closes it.
    let above = Rect::from_min_max(screen.min, Pos2::new(screen.right(), rect.top()));
    let mut dismissed = false;
    egui::Area::new(id.with("scrim")).order(Order::Foreground).fixed_pos(above.min).constrain(false).movable(false).show(ctx, |ui| {
        let r = ui.allocate_rect(above, Sense::click());
        ui.painter().rect_filled(above, 0.0, Color32::from_black_alpha(110));
        dismissed = r.clicked();
    });
    let mut close = false;
    egui::Area::new(id).order(Order::Foreground).fixed_pos(rect.min).constrain(false).movable(false).show(ctx, |ui| {
        // Claim the sheet's rect, so a tap on it does not reach the dimmed screen under it.
        ui.allocate_rect(rect, Sense::click());
        let radius = if full { CornerRadius::ZERO } else { CornerRadius { nw: RADIUS, ne: RADIUS, sw: 0, se: 0 } };
        ui.painter().rect_filled(rect, radius, t.card);
        let handle = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 10.0), vec2(32.0, 4.0));
        ui.painter().rect_filled(handle, CornerRadius::same(2), t.border);
        let title_rect = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE_H));
        let close_rect =
            Rect::from_center_size(Pos2::new(title_rect.right() - GUTTER - TOUCH_H / 2.0, title_rect.center().y), vec2(TOUCH_H, TOUCH_H));
        let x = ui.interact(close_rect, id.with("close"), Sense::click());
        x.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, "Close"));
        if x.is_pointer_button_down_on() {
            ui.painter().circle_filled(close_rect.center(), 22.0, t.pressed);
        }
        icons::paint(ui.painter(), Rect::from_center_size(close_rect.center(), vec2(24.0, 24.0)), Icon::Close, t.text, false);
        dismissed |= x.clicked();
        let title_w = (title_rect.width() - 2.0 * GUTTER - TOUCH_H).max(0.0);
        let title_text = egui::Label::new(egui::RichText::new(title).font(theme::bold(18.0)).color(t.text)).truncate();
        // The title at the left, cut short with an ellipsis.
        let title_box = Rect::from_min_size(Pos2::new(title_rect.left() + GUTTER, title_rect.top()), vec2(title_w, title_rect.height()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(title_box).layout(Layout::left_to_right(egui::Align::Center)), |ui| ui.add(title_text));
        let mut body_ui =
            ui.new_child(egui::UiBuilder::new().max_rect(body_rect.shrink2(vec2(GUTTER, 0.0))).layout(Layout::top_down(egui::Align::Min)));
        let scroll = egui::ScrollArea::vertical().id_salt(id.with("body")).auto_shrink([false, false]).max_height(body_rect.height()).show(
            &mut body_ui,
            |ui| {
                touch_style(ui);
                let close = body(ui);
                follow_focus(ui);
                close
            },
        );
        close |= scroll.inner;
        let total_now = TITLE_H + scroll.content_size.y + GUTTER + footer_h;
        ctx.data_mut(|d| d.insert_temp(height_key(), total_now));
        if let Some(f) = footer_rect {
            ui.painter().line_segment([f.left_top(), f.right_top()], egui::Stroke::new(1.0, t.border));
        }
    });
    (close, dismissed)
}

/// Up to three buttons side by side in the dialog sheet's pinned footer, equal widths, each 48 pt
/// tall, in `labels` order. `primary` is the accent button; Enter presses it when `enter` is set.
/// Returns the index clicked (outer None: no sheet footer is open, so the caller lays out inline).
pub fn footer_row(ui: &mut Ui, labels: &[&str], primary: usize, enter: bool) -> Option<Option<usize>> {
    let area = footer(ui.ctx())?;
    let mut fui = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(Layout::left_to_right(egui::Align::Center)));
    // At least the full footer height: with the keyboard up the stored area can be shorter.
    fui.set_clip_rect(Rect::from_min_size(area.min, vec2(area.width(), FOOTER_H.max(area.height()))));
    let r = area.shrink2(vec2(GUTTER, 12.0));
    let n = labels.len().max(1) as f32;
    let gap = 8.0;
    let w = ((r.width() - gap * (n - 1.0)) / n).max(0.0);
    let t = Tokens::get(fui.ctx());
    let mut clicked = None;
    for (i, label) in labels.iter().enumerate() {
        let rect = Rect::from_min_size(Pos2::new(r.left() + i as f32 * (w + gap), r.top()), vec2(w, TOUCH_H));
        let btn =
            if i == primary { egui::Button::new(egui::RichText::new(*label).color(t.accent_text)).fill(t.accent) } else { egui::Button::new(*label) };
        if fui.put(rect, btn.min_size(vec2(w, TOUCH_H))).clicked() {
            clicked = Some(i);
        }
    }
    if enter && clicked.is_none() && fui.input(|i| i.key_pressed(egui::Key::Enter)) {
        clicked = Some(primary);
    }
    Some(clicked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SlideApp;
    use egui_kittest::kittest::{NodeT, Queryable};
    use serde_json::json;

    /// Every dialog id the app knows (`dialogs::title`).
    const IDS: [&str; 42] = [
        "about",
        "accessibility",
        "action",
        "altText",
        "autocorrect",
        "bullets",
        "chartData",
        "columns",
        "comment",
        "customShow",
        "dateTime",
        "equation",
        "export",
        "find",
        "font",
        "gridOptions",
        "headerFooter",
        "hyperlink",
        "language",
        "outline",
        "paragraph",
        "pasteSpecial",
        "photoAlbum",
        "preferences",
        "quit",
        "renameLayout",
        "renameSection",
        "renameShape",
        "replaceFonts",
        "reuseSlides",
        "setupShow",
        "sizePosition",
        "slideSize",
        "smartart",
        "spelling",
        "symbol",
        "table",
        "tabs",
        "themeColors",
        "themeFonts",
        "trim",
        "zoom",
    ];

    /// On a 360 pt phone every dialog is a bottom sheet with Close, and no control lies past the
    /// screen's right edge.
    #[test]
    fn every_dialog_fits_a_phone() {
        let mut bad = Vec::new();
        for id in IDS {
            let mut app = SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
            deckcraft_engine::sample::open_sample(&mut app.session).expect("sample deck opens");
            let first = app.session.active().and_then(|d| d.doc.slides.first().and_then(|s| s.shapes.first().map(|sh| sh.id.0)));
            let _ = app.run("edit.select", json!({"ids": [first]}));
            let mut h = egui_kittest::Harness::builder().with_size(vec2(360.0, 800.0)).build_ui_state(
                |ui, app: &mut SlideApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
            // The test fonts lack some symbols (the Symbol and Bullets grids); this test is about layout.
            h.ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Tofu);
            h.run_steps(2);
            if h.state_mut().run("app.dialog", json!({"id": id})).is_err() || h.state().dialog.is_none() {
                continue;
            }
            h.run_steps(4);
            if h.state().dialog.is_none() {
                continue;
            }
            if h.query_all_by_role_and_label(egui::accesskit::Role::Button, "Close").next().is_none() {
                bad.push(format!("{id}: no Close"));
            }
            for node in h.query_all_by_role(egui::accesskit::Role::Button).chain(h.query_all_by_role(egui::accesskit::Role::TextInput)) {
                let r = node.rect();
                let name = node.accesskit_node().label().unwrap_or_default();
                // The slide chips scroll sideways under the sheet; they are not the dialog's.
                let chip = name.strip_prefix("Slide ").is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()));
                if !chip && r.right() > 361.0 && r.width() >= 1.0 {
                    bad.push(format!("{id}: {name} reaches past the screen: {r:?}"));
                }
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    /// Writes PNGs of a few dialog sheets into `$DECKCRAFT_SHOT_DIR`, for looking at the result.
    #[test]
    fn dialog_sheet_screenshots() {
        let Ok(dir) = std::env::var("DECKCRAFT_SHOT_DIR") else { return };
        for id in ["paragraph", "hyperlink", "preferences"] {
            let mut app = SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
            deckcraft_engine::sample::open_sample(&mut app.session).expect("sample deck opens");
            let mut h = egui_kittest::Harness::builder().with_size(vec2(360.0, 800.0)).with_pixels_per_point(2.0).build_ui_state(
                |ui, app: &mut SlideApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
            h.ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Tofu);
            h.run_steps(2);
            let _ = h.state_mut().run("app.dialog", json!({"id": id}));
            h.run_steps(5);
            if let Ok(img) = h.render() {
                let _ = img.save(format!("{dir}/dialog-{id}.png"));
            }
        }
    }
}
