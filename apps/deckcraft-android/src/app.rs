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
use crate::ime;
use crate::keyboard::{Around, Keyboard};

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
    /// Turns the keyboard's messages into egui IME events and keeps what it needs between frames.
    keyboard: Keyboard,
    /// The presentation and text position the keyboard types into last frame; a change starts it over.
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
            keyboard: Keyboard::default(),
            ime_target: String::new(),
            inbox,
            seen: Vec::new(),
            locs: HashMap::new(),
            opening: Vec::new(),
        }
    }

    /// Turns keyboard messages into egui events for the next frame. It stops at the first message that
    /// makes events: what the next one means depends on the text those events change, and the context
    /// Kotlin reads is published after the frame that applied them.
    fn drain_ime(&mut self) {
        while let Some(m) = ime::pop() {
            let desc = format!("{m:?}");
            let events = self.keyboard.handle(m);
            log::debug!("DBG ime msg {desc} -> {} events", events.len());
            if !events.is_empty() {
                self.pending.extend(events);
                break;
            }
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
        // The keyboard starts over when the edit moves to another presentation or text position.
        let ime_out = ctx_output_ime(ui.ctx());
        let target = format!("{}:{}", self.app.session.active().map(|d| d.uid).unwrap_or_default(), ime_out.as_ref().map_or(0, |o| o.owner.value()));
        if target != self.ime_target {
            self.ime_target = target;
            self.keyboard.retarget();
        }
        self.keyboard.set_around(ime_out.as_ref().and_then(around_of));
        // egui waits for the platform to acknowledge a reset of the input method, and Android sends no
        // acknowledgement itself: Kotlin restarts the keyboard and answers (`Msg::ResetDone`).
        let reset = self.app.pending_ime_reset(ui.ctx().viewport_id());
        if reset.is_some() {
            log::debug!("DBG egui waits on reset {:?}", reset.map(|r| r.as_u64()));
        }
        if self.keyboard.on_reset_request(reset)
            && let Some(id) = reset
        {
            ime::request_reset(id.as_u64());
        }
        ime::publish_context(ime::encode(ime::taken(), &self.keyboard.context()));
        // egui sets `output.ime` while a text position is focused: that is when the keyboard shows.
        ime::WANTS_KEYBOARD.store(ime_out.is_some(), Ordering::Relaxed);
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

/// egui's IME output for this frame, if a text position has focus.
fn ctx_output_ime(ctx: &egui::Context) -> Option<egui::output::IMEOutput> {
    ctx.output(|o| o.ime.clone())
}

/// The text around a collapsed caret, with the composition left out. `None` when egui offers no
/// text (a shape that is selected but not being edited) or the selection is a range.
fn around_of(o: &egui::output::IMEOutput) -> Option<Around> {
    match &o.surrounding_text {
        egui::output::IMESurroundingText::Available(p) if p.cursor == p.anchor => Some(Around { text: p.text.clone(), caret: p.cursor }),
        _ => None,
    }
}
