//! Text from the soft keyboard. `MainActivity.kt`'s ImeView pushes what the keyboard sent (through
//! the JNI exports in `android.rs`); `app.rs` turns it into egui events on the next frame.
//!
//! The editing context Kotlin reads back is the text around the caret, published every frame from egui's
//! IME output (`publish_context`). Kotlin takes it only once Rust has read every message it sent (the
//! count in its first field), and keeps its own copy in between. `keyboard.rs` turns the messages into
//! egui IME events. Offsets from the keyboard are UTF-16 units; the helpers below convert them to bytes
//! and refuse offsets that are past the end or split a surrogate pair (they come from outside, so they
//! are hostile). See `notes/native-port.md`.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};

/// One message from the keyboard. `kind` in the JNI export: 0 commit, 1 composing text,
/// 2 finish composing, 3 delete around the caret, 4 key press, 5 composing region, 6 reset done.
#[derive(Debug, PartialEq)]
pub enum Msg {
    Commit(String),
    Compose(String),
    Finish,
    Delete {
        before: usize,
        after: usize,
    },
    Key(i32),
    /// `setComposingRegion`: UTF-16 offsets into the context text (`start..end`).
    Region {
        start: usize,
        end: usize,
    },
    /// Kotlin restarted the input method for reset request `n` (`take_reset`): the acknowledgement
    /// egui waits for before it accepts keyboard input again.
    ResetDone(u64),
}

static QUEUE: Mutex<VecDeque<Msg>> = Mutex::new(VecDeque::new());
/// Messages Rust has taken so far. Kotlin only trusts the context when this matches its own count.
static TAKEN: AtomicU64 = AtomicU64::new(0);

/// The context the input method reads (see `encode`), written every frame.
static CONTEXT: Mutex<String> = Mutex::new(String::new());
/// The id of the IME reset egui is waiting on (0 for none). Kotlin takes it (`take_reset`), restarts the
/// input method, and answers with `Msg::ResetDone`.
static RESET: AtomicU64 = AtomicU64::new(0);
/// Whether egui wants the soft keyboard (a text position has focus). Kotlin polls it.
pub static WANTS_KEYBOARD: AtomicBool = AtomicBool::new(false);
/// Set by Enter or Tab from the keyboard, so it stays up for the next line; cleared by a touch, a scroll, or Escape.
pub static KEEP_TYPING: AtomicBool = AtomicBool::new(false);
/// Whether File › Close (or the quit prompt) ended the app: Kotlin finishes the activity (`SlideApp::quit_requested`).
pub static QUIT: AtomicBool = AtomicBool::new(false);
/// Whether Android Back has a sheet, dialog or Backstage to close (`SlideApp::can_go_back`).
pub static CAN_GO_BACK: AtomicBool = AtomicBool::new(false);
/// The GPU could not be set up (no adapter, or the window surface failed before the first frame). Kotlin
/// shows a dialog for it (`nativeGpuFailed`).
pub static GPU_FAILED: AtomicBool = AtomicBool::new(false);
/// Whether eframe has run the first `ui` call, i.e. the window surface was set up (see `android::gpu_setup`).
pub static UI_STARTED: AtomicBool = AtomicBool::new(false);
/// How many displays Android reports (set by Kotlin; 1 until it says). Presenter View needs a second one.
static DISPLAYS: AtomicUsize = AtomicUsize::new(1);

/// Records the number of connected displays (Kotlin polls it, so a cast or desk mode connecting later counts).
pub fn set_displays(n: usize) {
    DISPLAYS.store(n.max(1), Ordering::Relaxed);
}

/// The display count the engine asks for (`Services::displays`).
pub fn display_count() -> Option<usize> {
    Some(DISPLAYS.load(Ordering::Relaxed))
}

/// Whether a slide show is running (`SlideApp::show`). Kotlin turns the screen to landscape while it is.
pub static PRESENTING: AtomicBool = AtomicBool::new(false);
/// The system's night mode, from Kotlin.
pub static NIGHT: AtomicBool = AtomicBool::new(false);
/// Left and top padding of the surface in pixels (set by Kotlin); touch positions arrive in window
/// pixels, so the host subtracts it.
pub static INSET_LEFT: AtomicI32 = AtomicI32::new(0);
pub static INSET_TOP: AtomicI32 = AtomicI32::new(0);

/// Wakes the event loop, which sleeps until something arrives. Set on the first frame.
pub static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn wake() {
    if let Some(f) = WAKE.get() {
        f();
    }
}

/// Queues a message from Kotlin (JNI thread) and wakes the event loop.
pub fn push(m: Msg) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push_back(m);
    }
    wake();
}

/// The oldest queued message, if any.
/// Splits keyboard commit text at newlines ("\n", "\r\n" or a lone "\r"); the pieces are the lines between Enter presses.
pub fn ime_lines(text: &str) -> Vec<&str> {
    text.split("\r\n").flat_map(|s| s.split(['\r', '\n'])).collect()
}

pub fn pop() -> Option<Msg> {
    let m = QUEUE.lock().ok()?.pop_front()?;
    TAKEN.fetch_add(1, Ordering::Relaxed);
    Some(m)
}

/// Messages Rust has taken so far (the first field of the context).
pub fn taken() -> u64 {
    TAKEN.load(Ordering::Relaxed)
}

/// Stores the encoded context for Kotlin to read (`context_text`).
pub fn publish_context(encoded: String) {
    if let Ok(mut c) = CONTEXT.lock() {
        *c = encoded;
    }
}

/// The last published context, for Kotlin (empty until the first frame).
pub fn context_text() -> String {
    CONTEXT.lock().map(|c| c.clone()).unwrap_or_default()
}

/// Asks Kotlin to restart the input method for reset request `id` (egui's `ImeResetId`).
pub fn request_reset(id: u64) {
    RESET.store(id, Ordering::Relaxed);
    wake();
}

/// The reset request Kotlin has not seen yet, once ("" when none).
pub fn take_reset() -> String {
    match RESET.swap(0, Ordering::Relaxed) {
        0 => String::new(),
        id => id.to_string(),
    }
}

/// UTF-16 length of `s`.
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The byte offset in `text` of the UTF-16 offset `units`, or `None` when it is past the end or
/// falls inside a surrogate pair.
pub fn byte_at_utf16(text: &str, units: usize) -> Option<usize> {
    let mut u = 0usize;
    for (b, c) in text.char_indices() {
        if u == units {
            return Some(b);
        }
        u = u.checked_add(c.len_utf16())?;
        if u > units {
            return None;
        }
    }
    (u == units).then_some(text.len())
}

/// The UTF-16 offset of byte `byte` in `text` (0 when `byte` is not a boundary).
pub fn utf16_at_byte(text: &str, byte: usize) -> usize {
    text.get(..byte).map_or(0, utf16_len)
}

/// A span of document text the input method is composing. The text stays in the document; the
/// span only marks it, so nothing is held outside the document. Offsets are bytes in the paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Where the span starts in the paragraph.
    pub start: usize,
    /// The caret, where the input method left it (inside `start..end`).
    pub caret: usize,
    /// The text of the span, as the document holds it.
    pub text: String,
}

impl Span {
    pub fn end(&self) -> usize {
        self.start.saturating_add(self.text.len())
    }

    /// Chars before and after the caret inside the span: what erasing the span takes.
    pub fn erase_counts(&self) -> (usize, usize) {
        let rel = self.caret.saturating_sub(self.start);
        let head = self.text.get(..rel).unwrap_or("");
        let tail = self.text.get(rel..).unwrap_or("");
        (head.chars().count(), tail.chars().count())
    }
}

/// Whether `span` still holds in `para` with the caret at `caret`: the text is still there, and the
/// caret has not moved since the span was last read.
pub fn holds(span: &Span, para: &str, caret: usize) -> bool {
    caret == span.caret
        && span.caret >= span.start
        && span.caret <= span.end()
        && para.is_char_boundary(caret)
        && para.get(span.start..span.end()) == Some(span.text.as_str())
}

/// The composing region `start..end` (UTF-16, in `para`) the input method set over text. The words
/// stay where they are, marked as the composition, with the caret inside them. `None` when the
/// offsets are invalid, the caret is outside them, or the region is empty.
pub fn region_span(para: &str, caret: usize, start: usize, end: usize) -> Option<Span> {
    let (s, e) = (start.min(end), start.max(end));
    let sb = byte_at_utf16(para, s)?;
    let eb = byte_at_utf16(para, e)?;
    if sb == eb || !para.is_char_boundary(caret) || caret < sb || caret > eb {
        return None;
    }
    Some(Span { start: sb, caret, text: para.get(sb..eb)?.to_string() })
}

/// What the input method reads: the paragraph, the caret and the composing span, in UTF-16 units.
/// `composing` is `None` without a span.
#[derive(Debug, PartialEq, Eq)]
pub struct Context {
    pub text: String,
    pub caret: usize,
    pub composing: Option<(usize, usize)>,
}

/// The context around the caret at byte `caret` of `para`. `None` when `caret` is not a character
/// boundary of `para`.
pub fn context(para: &str, caret: usize, span: Option<&Span>) -> Option<Context> {
    if !para.is_char_boundary(caret) {
        return None;
    }
    let composing = span.map(|s| (utf16_at_byte(para, s.start), utf16_at_byte(para, s.end())));
    Some(Context { text: para.to_string(), caret: utf16_at_byte(para, caret), composing })
}

/// One context line for Kotlin: `ack`, caret, composing start and end (-1 for none), then the text.
/// Fields are split by U+0001, and the text comes last so it may contain anything else.
pub fn encode(ack: u64, c: &Context) -> String {
    let (s, e) = c.composing.map_or((-1, -1), |(s, e)| (s as i64, e as i64));
    format!("{ack}\u{1}{}\u{1}{s}\u{1}{e}\u{1}{}", c.caret, c.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_newlines_split_into_lines() {
        assert_eq!(ime_lines("Hello"), ["Hello"]);
        assert_eq!(ime_lines("ab\ncd"), ["ab", "cd"]);
        assert_eq!(ime_lines("ab\r\ncd\re"), ["ab", "cd", "e"]);
        assert_eq!(ime_lines("\n"), ["", ""]);
    }

    #[test]
    fn utf16_offsets_map_over_accents_and_emoji() {
        // "a" 1 unit, "é" 1 unit (2 bytes), "😀" 2 units (surrogate pair, 4 bytes), "b" 1 unit.
        let t = "aé😀b";
        assert_eq!(utf16_len(t), 5);
        assert_eq!(byte_at_utf16(t, 0), Some(0));
        assert_eq!(byte_at_utf16(t, 2), Some(3));
        assert_eq!(byte_at_utf16(t, 3), None, "inside the surrogate pair");
        assert_eq!(byte_at_utf16(t, 4), Some(7));
        assert_eq!(byte_at_utf16(t, 5), Some(8), "the end");
        assert_eq!(byte_at_utf16(t, 6), None, "past the end");
        assert_eq!(utf16_at_byte(t, 3), 2);
        assert_eq!(utf16_at_byte(t, 2), 0, "not a boundary");
    }

    #[test]
    fn a_region_marks_words_in_place_and_refuses_hostile_offsets() {
        let s = region_span("say helo", 8, 4, 8).expect("a region");
        assert_eq!((s.start, s.text.as_str()), (4, "helo"));
        assert_eq!(s.erase_counts(), (4, 0));
        assert!(region_span("say helo", 8, 8, 8).is_none(), "empty");
        assert!(region_span("say helo", 2, 4, 8).is_none(), "the caret is outside it");
        assert!(region_span("say helo", 8, 4, 99).is_none(), "past the end");
        assert!(region_span("a😀", 5, 1, 2).is_none(), "splits a surrogate pair");
        assert_eq!(region_span("héllo", 3, 1, 2).map(|s| s.text), Some("é".to_string()), "units 1..2 are the é, bytes 1..3");
        assert!(region_span("héllo", 2, 1, 2).is_none(), "a caret inside the é");
    }

    #[test]
    fn a_span_holds_only_while_its_text_and_caret_are_where_they_were() {
        let s = region_span("say helo", 8, 4, 8).expect("a region");
        assert!(holds(&s, "say helo", 8));
        assert!(!holds(&s, "say helo", 7), "the caret moved");
        assert!(!holds(&s, "say hall", 8), "the text changed");
        assert!(!holds(&s, "say", 8), "the text is gone");
    }

    #[test]
    fn the_encoded_context_keeps_the_text_last() {
        let c = Context { text: "a\u{1}b".into(), caret: 1, composing: Some((0, 1)) };
        assert_eq!(encode(7, &c), "7\u{1}1\u{1}0\u{1}1\u{1}a\u{1}b");
        let none = Context { text: "x".into(), caret: 0, composing: None };
        assert_eq!(encode(0, &none), "0\u{1}0\u{1}-1\u{1}-1\u{1}x");
    }

    /// A phone has one display unless Android says otherwise, so a slide show is the plain full-window view;
    /// with a second display connected the engine may open Presenter View.
    #[test]
    fn one_display_until_android_reports_more() {
        assert_eq!(display_count(), Some(1), "the default is one display");
        set_displays(2);
        assert_eq!(display_count(), Some(2));
        set_displays(0);
        assert_eq!(display_count(), Some(1), "never fewer than the one we are drawn on");
        set_displays(1);
    }

    #[test]
    fn a_reset_request_is_taken_once() {
        assert_eq!(take_reset(), "");
        request_reset(41);
        assert_eq!(take_reset(), "41");
        assert_eq!(take_reset(), "", "seen only once");
    }
}
