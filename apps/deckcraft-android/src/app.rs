//! The desktop/web `SlideApp` on the native window, opened on a blank presentation.
//!
//! Keyboard input comes from the Kotlin `InputConnection` (`ImeView`, MainActivity.kt) through the
//! queue in `ime.rs`. Each frame the queued messages become egui text and key events, which the
//! slide canvas and the dialogs' text fields already handle. Files go through the system picker
//! (`files.rs`); unsaved work is kept by the engine's own recovery store.

use std::sync::atomic::Ordering;

use deckcraft_engine::Session;
use deckcraft_ui_egui::{Inbox, InboxEvent, InboxFile, Services, SlideApp};
use egui::{Event, Key};

use crate::clip;
use crate::files;
use crate::ime::{self, Msg, ime_lines};

/// Android KeyEvent codes the keyboard sends as key presses.
const KEYCODE_ESCAPE: i32 = 111;
const KEYCODE_DEL: i32 = 67;
const KEYCODE_FORWARD_DEL: i32 = 112;
const KEYCODE_ENTER: i32 = 66;
const KEYCODE_TAB: i32 = 61;

/// Most keys one `Delete` message may erase, so a hostile count can't flood the queue.
const MAX_ERASE: usize = 256;

pub struct Shell {
    app: SlideApp,
    /// Events from the keyboard, handed to egui with the next frame's input.
    pending: Vec<Event>,
    /// The keyboard's composing text, which is replaced as the user types (see `replace_composing`).
    composing: String,
    /// The active presentation, slide and selection last frame; a change calls `ime::retarget`.
    ime_target: String,
    inbox: Inbox,
}

impl Shell {
    pub fn new(ctx: &egui::Context) -> Shell {
        deckcraft_ui_egui::install_ime_pass_hooks(ctx);
        let inbox = Inbox::default();
        let services = Services {
            open_async: Some(Box::new(|purpose: &str, id: u64| files::request_open(purpose, id))),
            inbox: Some(inbox.clone()),
            download: Some(Box::new(|name: &str, bytes: &[u8]| files::save_as(name, bytes))),
            ..Default::default()
        };
        let mut app = SlideApp::new(Session::new(), services);
        // AutoRecover, as on the desktop: reopen what a previous run left unsaved.
        if let Some(dir) = files::DIR.get().map(|d| d.join("recovery")) {
            app.session.recovery_dir = Some(dir);
            if let Ok(v) = app.session.execute("file.recovery.open", &serde_json::json!({}))
                && let Some(n) = v.get("opened").and_then(|o| o.as_array()).map(Vec::len).filter(|n| *n > 0)
            {
                app.set_status(format!("Recovered {n} unsaved presentation{}", if n == 1 { "" } else { "s" }));
            }
        }
        Shell { app, pending: Vec::new(), composing: String::new(), ime_target: String::new(), inbox }
    }

    /// Turns the keyboard's messages into egui events for the next frame.
    fn drain_ime(&mut self) {
        while let Some(m) = ime::pop() {
            match m {
                Msg::Compose(t) => self.replace_composing(&t),
                Msg::Commit(t) => {
                    // Gboard sends Enter as a newline in the commit text; it becomes an Enter press.
                    for (i, line) in ime_lines(&t).into_iter().enumerate() {
                        if i > 0 {
                            self.composing.clear();
                            self.press(Key::Enter);
                        }
                        self.replace_composing(line);
                        self.composing.clear();
                    }
                }
                Msg::Finish => self.composing.clear(),
                Msg::Delete { before, after } => {
                    let before = before.min(MAX_ERASE);
                    for _ in 0..before {
                        self.press(Key::Backspace);
                    }
                    for _ in 0..after.min(MAX_ERASE) {
                        self.press(Key::Delete);
                    }
                    let keep = self.composing.chars().count().saturating_sub(before);
                    self.composing = self.composing.chars().take(keep).collect();
                }
                Msg::Key(code) => {
                    let key = match code {
                        KEYCODE_ENTER => Key::Enter,
                        KEYCODE_TAB => Key::Tab,
                        KEYCODE_DEL => Key::Backspace,
                        KEYCODE_FORWARD_DEL => Key::Delete,
                        KEYCODE_ESCAPE => Key::Escape,
                        _ => continue,
                    };
                    self.composing.clear();
                    self.press(key);
                }
            }
        }
    }

    /// Replaces the keyboard's composing text with `text`. Only the part after the common prefix is
    /// erased and typed again: a keystroke that extends the word sends just the new letter.
    fn replace_composing(&mut self, text: &str) {
        let common = self.composing.chars().zip(text.chars()).take_while(|(a, b)| a == b).count();
        for _ in common..self.composing.chars().count() {
            self.press(Key::Backspace);
        }
        let added: String = text.chars().skip(common).collect();
        if !added.is_empty() {
            self.pending.push(Event::Text(added));
        }
        self.composing = text.to_string();
    }

    fn press(&mut self, key: Key) {
        for pressed in [true, false] {
            self.pending.push(Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: Default::default() });
        }
    }

    /// Files picked since the last frame go to the UI's inbox under their request ids; notices go to
    /// the status line.
    fn drain_files(&mut self) {
        let cancelled = files::drain_cancelled();
        let mut files_in: Vec<InboxFile> = Vec::new();
        for (id, loc, bytes) in files::drain_opened() {
            let name = files::parse_location(&loc).map_or_else(|| loc.clone(), |(_, n)| n.to_string());
            match id {
                Some(id) => files_in.push(InboxFile { id, name, bytes }),
                // Another app handed the file over (Open with, Share): there is no request, so it
                // opens or inserts as a dropped file does.
                None => self.app.open_or_insert_bytes(&name, &bytes),
            }
        }
        {
            let mut inbox = self.inbox.lock().unwrap_or_else(|e| e.into_inner());
            inbox.extend(cancelled.into_iter().map(|id| InboxEvent::Cancelled { id }));
            inbox.extend(files_in.into_iter().map(InboxEvent::File));
        }
        for msg in files::drain_notices() {
            self.app.set_status(msg);
        }
    }
}

impl eframe::App for Shell {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_ime();
        self.drain_files();
        if !self.pending.is_empty() {
            ctx.request_repaint();
        }
        // The activity is pausing: write the recovery copies now, the process may be killed next.
        if ime::FLUSH.swap(false, Ordering::Relaxed)
            && let Err(e) = self.app.session.execute("file.recovery.save", &serde_json::json!({}))
        {
            log::warn!("recovery copies not written: {e}");
        }
        self.app.logic(ctx);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Touch arrives in window pixels; the surface sits inside the system-bar padding.
        let ppp = ctx.pixels_per_point();
        let shift = egui::vec2(ime::INSET_LEFT.load(Ordering::Relaxed) as f32, ime::INSET_TOP.load(Ordering::Relaxed) as f32) / ppp;
        for e in &mut raw.events {
            match e {
                Event::PointerMoved(p) | Event::PointerButton { pos: p, .. } | Event::Touch { pos: p, .. } => *p -= shift,
                _ => {}
            }
        }
        raw.events.append(&mut self.pending);
        raw.system_theme = Some(if ime::NIGHT.load(Ordering::Relaxed) { egui::Theme::Dark } else { egui::Theme::Light });
        self.app.raw_input_hook(ctx, raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ime::UI_STARTED.store(true, Ordering::Relaxed);
        // The GPU failed (a surface error before the first frame): close eframe. `android_main` then keeps the
        // activity up for the driver dialog, without eframe.
        if ime::GPU_FAILED.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let _ = ime::WAKE.set(Box::new(move || ctx.request_repaint()));
        self.app.ui(ui);
        // The keyboard starts over when the edit moves to another presentation.
        let target = self.app.session.active().map(|d| d.uid.to_string()).unwrap_or_default();
        if target != self.ime_target {
            self.ime_target = target;
            self.composing.clear();
            ime::retarget("", 0);
        }
        // egui sets `output.ime` while a text position is focused: that is when the keyboard shows.
        ime::WANTS_KEYBOARD.store(ui.ctx().output(|o| o.ime.is_some()), Ordering::Relaxed);
        ime::CAN_GO_BACK.store(self.app.can_go_back(), Ordering::Relaxed);
        // egui-winit has no Android clipboard; copies go to ClipboardManager through Kotlin.
        let copies: Vec<String> = ui
            .ctx()
            .output(|o| o.commands.iter().filter_map(|c| if let egui::OutputCommand::CopyText(t) = c { Some(t.clone()) } else { None }).collect());
        if let Some(t) = copies.into_iter().last() {
            clip::copied(t);
        }
    }
}
