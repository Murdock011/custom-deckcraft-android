//! The desktop/web `SlideApp` on the native window, opened on a blank presentation.
//!
//! Keyboard input comes from the Kotlin `InputConnection` (`ImeView`, MainActivity.kt) through the
//! queue in `ime.rs`. Each frame the queued messages become egui text and key events, which the
//! slide canvas and the dialogs' text fields already handle. Files go through the system picker
//! (`files.rs`); unsaved work is kept by the engine's own recovery store.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use deckcraft_engine::cmd::file::mark_saved;
use deckcraft_engine::{Session, recovery};
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

/// The unsaved presentations as the recovery copies should hold them, refreshed whenever one changes.
/// The pause flush (a JNI call, with no frame to wait for) writes this from its own thread.
static PENDING: Mutex<Vec<recovery::Snapshot>> = Mutex::new(Vec::new());
/// Serializes the writes: the engine's timed save and the pause flush use the same files.
static WRITE: Mutex<()> = Mutex::new(());

/// Writes the pending recovery copies, from any thread. Failures are logged; editing carries on.
pub fn write_pending() {
    let Some(dir) = files::recovery_dir() else { return };
    let _write = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let pending = PENDING.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if pending.is_empty() {
        return;
    }
    if let Err(e) = recovery::save_snapshots(&pending, &dir) {
        log::warn!("recovery copies not written: {e}");
    }
}

pub struct Shell {
    app: SlideApp,
    /// Events from the keyboard, handed to egui with the next frame's input.
    pending: Vec<Event>,
    /// The keyboard's composing text, which is replaced as the user types (see `replace_composing`).
    composing: String,
    /// The active presentation, slide and selection last frame; a change calls `ime::retarget`.
    ime_target: String,
    inbox: Inbox,
    /// The unsaved presentations as (uid, revision) when `PENDING` was last refreshed.
    seen: Vec<(u64, u64)>,
    /// Where each picked or saved presentation lives (its content location), by presentation uid.
    /// Plain Save writes back there.
    locs: HashMap<u64, String>,
    /// Locations of the presentations queued for opening this frame (see `adopt_opened`).
    opening: Vec<String>,
}

impl Shell {
    pub fn new(ctx: &egui::Context) -> Shell {
        deckcraft_ui_egui::install_ime_pass_hooks(ctx);
        let inbox = Inbox::default();
        let services = Services {
            open_async: Some(Box::new(|purpose: &str, id: u64| files::request_open(purpose, id))),
            inbox: Some(inbox.clone()),
            download: Some(Box::new(|name: &str, bytes: &[u8]| files::save(name, bytes))),
            ..Default::default()
        };
        let mut app = SlideApp::new(Session::new(), services);
        // AutoRecover, as on the desktop: reopen what a previous run left unsaved.
        if let Some(dir) = files::recovery_dir() {
            app.session.recovery_dir = Some(dir);
            if let Ok(v) = app.session.execute("file.recovery.open", &serde_json::json!({}))
                && let Some(n) = v.get("opened").and_then(|o| o.as_array()).map(Vec::len).filter(|n| *n > 0)
            {
                app.set_status(format!("Recovered {n} unsaved presentation{}", if n == 1 { "" } else { "s" }));
            }
        }
        Shell {
            app,
            pending: Vec::new(),
            composing: String::new(),
            ime_target: String::new(),
            inbox,
            seen: Vec::new(),
            locs: HashMap::new(),
            opening: Vec::new(),
        }
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

    /// The active presentation lives at `loc` now: plain Save writes back to it.
    fn adopt(&mut self, loc: &str) {
        if let Some(uid) = self.app.session.active().map(|d| d.uid) {
            self.locs.insert(uid, loc.to_string());
        }
    }

    /// A save of revision `rev` of presentation `uid` ended. `outcome` is `None` when it was cancelled or
    /// failed, and empty for a file that is not the presentation (an export). If the presentation is
    /// still at that revision it is clean now, as after a desktop save, and lives at the location;
    /// edits made since the save was asked for leave it unsaved.
    fn finish_save(&mut self, uid: u64, rev: u64, outcome: Option<String>) {
        let Some(loc) = outcome.filter(|l| !l.is_empty()) else { return };
        if !self.app.session.active().is_some_and(|d| d.uid == uid && d.revision == rev) {
            return;
        }
        let name = files::parse_location(&loc).map_or_else(|| loc.clone(), |(_, n)| n.to_string());
        match mark_saved(&mut self.app.session, Some(name)) {
            Ok(()) => {
                self.locs.insert(uid, loc);
            }
            Err(e) => log::warn!("saved presentation not marked clean: {e}"),
        }
    }

    /// Keeps `PENDING` current: a change to the unsaved presentations replaces it at once, so the pause
    /// flush writes the latest state even when no frame follows the edit.
    fn track_recovery(&mut self) {
        let state: Vec<(u64, u64)> = self.app.session.documents().iter().filter(|d| d.is_dirty()).map(|d| (d.uid, d.revision)).collect();
        if state != self.seen {
            *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = recovery::snapshots(&self.app.session);
            self.seen = state;
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
                Some(id) => {
                    if files::keeps_location(&name) {
                        self.opening.push(loc.clone());
                    }
                    files_in.push(InboxFile { id, name, bytes });
                }
                // Another app handed the file over (Open with, Share): there is no request, so it
                // opens or inserts as a dropped file does.
                None => {
                    let before = self.app.session.active().map(|d| d.uid);
                    self.app.open_or_insert_bytes(&name, &bytes);
                    if files::keeps_location(&name) && self.app.session.active().map(|d| d.uid) != before {
                        self.adopt(&loc);
                    }
                }
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
        // Saves queued during this frame's input save this revision, to this document.
        let active = self.app.session.active().map(|d| (d.uid, d.revision));
        files::set_revision(active.map_or(0, |a| a.0), active.map_or(0, |a| a.1));
        files::set_current(active.and_then(|a| self.locs.get(&a.0).cloned()));
        // Inbox files open inside `logic`: the presentation that became active lives where it was picked.
        let before = active.map(|a| a.0);
        self.app.logic(ctx);
        let opened = std::mem::take(&mut self.opening);
        if self.app.session.active().map(|d| d.uid) != before
            && let Some(loc) = opened.last().cloned()
        {
            self.adopt(&loc);
        }
        for (uid, rev, outcome) in files::drain_saved() {
            self.finish_save(uid, rev, outcome);
        }
        self.track_recovery();
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
        // egui-winit has no Android clipboard, so a hardware keyboard's Ctrl+V brings no text: hand over what
        // ClipboardManager holds, as the paste the shortcut asks for.
        let pasted = raw.events.iter().any(|e| matches!(e, Event::Key { key: Key::V, pressed: true, modifiers, .. } if modifiers.command));
        if pasted && let Some(t) = clip::text() {
            raw.events.push(Event::Paste(t));
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
