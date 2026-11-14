//! Turns the soft keyboard's messages (`ime::Msg`) into egui IME events.
//!
//! The editor already understands real input-method events (`Event::Ime`): a `Preedit` is the word
//! being composed and replaces the one before it, a `Commit` makes text final, and
//! `DeleteSurrounding` erases around the caret. This module sends those, so the editor, not the
//! host, owns the composition. It holds only what the host must know to translate correctly:
//! the composition it has sent, the span a suggestion will replace, the text around the caret that
//! egui last reported, and the IME reset egui is waiting to have acknowledged.
//!
//! No Android types are used, so the host tests run it.

use egui::{Event, ImeEvent, ImeResetId, Key, Modifiers};

use crate::ime::{self, Context, Msg, Span, ime_lines};

/// The most characters one delete message may erase. The count comes from outside.
pub const MAX_ERASE: usize = 64;

pub const KEYCODE_TAB: i32 = 61;
pub const KEYCODE_ENTER: i32 = 66;
pub const KEYCODE_DEL: i32 = 67;
pub const KEYCODE_FORWARD_DEL: i32 = 112;
pub const KEYCODE_ESCAPE: i32 = 111;

/// The text around the caret that egui reported last frame, with the composition left out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Around {
    pub text: String,
    /// The caret, in bytes of `text`.
    pub caret: usize,
}

#[derive(Default)]
pub struct Keyboard {
    /// The composition sent as a `Preedit` and not yet committed.
    preedit: String,
    /// Words the keyboard marked for replacement (`setComposingRegion`), still in the text.
    region: Option<Span>,
    /// What egui reported around the caret last frame; `None` when nothing can be edited.
    around: Option<Around>,
    /// The reset egui waits on.
    reset: Option<ImeResetId>,
}

fn press(key: Key) -> [Event; 2] {
    [
        Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: Modifiers::NONE },
        Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers: Modifiers::NONE },
    ]
}

fn key_for(code: i32) -> Option<Key> {
    match code {
        KEYCODE_ENTER => Some(Key::Enter),
        KEYCODE_TAB => Some(Key::Tab),
        KEYCODE_DEL => Some(Key::Backspace),
        KEYCODE_FORWARD_DEL => Some(Key::Delete),
        KEYCODE_ESCAPE => Some(Key::Escape),
        _ => None,
    }
}

impl Keyboard {
    /// The text around the caret as of this frame, from egui's IME output. `None` when no text
    /// position is being edited. A held span that no longer matches the text is dropped.
    pub fn set_around(&mut self, around: Option<Around>) {
        if around.is_none() {
            self.preedit.clear();
        }
        self.around = around;
        let stale = match (&self.region, &self.around) {
            (Some(span), Some(a)) => !ime::holds(span, &a.text, a.caret),
            _ => true,
        };
        if stale {
            self.region = None;
        }
    }

    /// The edit target changed (another shape, a table cell, the notes): nothing carries over.
    pub fn retarget(&mut self) {
        self.preedit.clear();
        self.region = None;
    }

    /// egui asked for a reset of the platform input method. The composition is gone on its side,
    /// so it is gone here too. Returns true for an id not seen before: the host must restart the
    /// keyboard and answer with `Msg::ResetDone`.
    pub fn on_reset_request(&mut self, id: Option<ImeResetId>) -> bool {
        match id {
            Some(id) if self.reset != Some(id) => {
                self.reset = Some(id);
                self.retarget();
                true
            }
            Some(_) => false,
            None => {
                self.reset = None;
                false
            }
        }
    }

    /// The context Kotlin reads: the text around the caret with the composition in it.
    pub fn context(&self) -> Context {
        let Some(a) = &self.around else { return Context { text: String::new(), caret: 0, composing: None } };
        if self.preedit.is_empty() {
            let span = self.region.as_ref();
            return ime::context(&a.text, a.caret, span).unwrap_or(Context { text: String::new(), caret: 0, composing: None });
        }
        let at = if a.text.is_char_boundary(a.caret) { a.caret } else { 0 };
        let mut text = String::with_capacity(a.text.len() + self.preedit.len());
        text.push_str(&a.text[..at]);
        text.push_str(&self.preedit);
        text.push_str(&a.text[at..]);
        let start = ime::utf16_at_byte(&text, at);
        let end = start + ime::utf16_len(&self.preedit);
        Context { caret: end, composing: Some((start, end)), text }
    }

    /// Erases the held span, if it still holds, before new text goes in its place.
    fn take_region(&mut self, out: &mut Vec<Event>) {
        let Some(span) = self.region.take() else { return };
        let holds = self.around.as_ref().is_some_and(|a| ime::holds(&span, &a.text, a.caret));
        if holds && self.preedit.is_empty() {
            let (before, after) = span.erase_counts();
            if before + after > 0 {
                out.push(Event::Ime(ImeEvent::DeleteSurrounding { before_chars: before.min(MAX_ERASE), after_chars: after.min(MAX_ERASE) }));
            }
        }
    }

    /// Commits a composition the keyboard has not finished, before something else acts.
    fn settle(&mut self, out: &mut Vec<Event>) {
        if !self.preedit.is_empty() {
            out.push(Event::Ime(ImeEvent::Commit(std::mem::take(&mut self.preedit))));
        }
    }

    /// The events for one keyboard message.
    pub fn handle(&mut self, m: Msg) -> Vec<Event> {
        let mut out = Vec::new();
        match m {
            Msg::Compose(t) => {
                if t.is_empty() && self.preedit.is_empty() {
                    return out;
                }
                self.take_region(&mut out);
                out.push(Event::Ime(ImeEvent::Preedit { text: t.clone(), active_range_chars: None }));
                self.preedit = t;
            }
            Msg::Commit(t) => {
                self.take_region(&mut out);
                for (i, line) in ime_lines(&t).into_iter().enumerate() {
                    if i > 0 {
                        self.settle(&mut out);
                        out.extend(press(Key::Enter));
                    }
                    if !line.is_empty() {
                        out.push(Event::Ime(ImeEvent::Commit(line.to_string())));
                    } else if i == 0 && !self.preedit.is_empty() {
                        // commitText("") over a composition deletes it.
                        out.push(Event::Ime(ImeEvent::Preedit { text: String::new(), active_range_chars: None }));
                    }
                    self.preedit.clear();
                }
            }
            Msg::Finish => self.settle(&mut out),
            Msg::Delete { before, after } => {
                self.settle(&mut out);
                self.region = None;
                let (before, after) = (before.min(MAX_ERASE), after.min(MAX_ERASE));
                if before + after > 0 {
                    out.push(Event::Ime(ImeEvent::DeleteSurrounding { before_chars: before, after_chars: after }));
                }
            }
            Msg::Region { start, end } => {
                if self.preedit.is_empty()
                    && let Some(a) = &self.around
                {
                    self.region = ime::region_span(&a.text, a.caret, start, end);
                }
            }
            Msg::Key(code) => {
                let Some(key) = key_for(code) else { return out };
                self.settle(&mut out);
                self.region = None;
                out.extend(press(key));
            }
            Msg::ResetDone(n) => {
                if let Some(id) = self.reset
                    && id.as_u64() == n
                {
                    self.reset = None;
                    out.push(Event::ImeResetCompleted { request_id: id });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn around(text: &str, caret: usize) -> Option<Around> {
        Some(Around { text: text.to_string(), caret })
    }
    fn ime_events(v: &[Event]) -> Vec<&ImeEvent> {
        v.iter().filter_map(|e| if let Event::Ime(i) = e { Some(i) } else { None }).collect()
    }
    fn preedit(t: &str) -> Event {
        Event::Ime(ImeEvent::Preedit { text: t.to_string(), active_range_chars: None })
    }
    fn commit(t: &str) -> Event {
        Event::Ime(ImeEvent::Commit(t.to_string()))
    }

    /// Gboard types h, he, hel as composing text, then finishes: the word stays, committed once.
    #[test]
    fn composing_then_finish_commits_the_word_and_never_cancels_it() {
        let mut kb = Keyboard::default();
        kb.set_around(around("", 0));
        for w in ["h", "he", "hel"] {
            assert_eq!(kb.handle(Msg::Compose(w.into())), vec![preedit(w)]);
        }
        assert_eq!(kb.handle(Msg::Finish), vec![commit("hel")], "finish commits what was composed");
        assert!(kb.handle(Msg::Finish).is_empty(), "and a second finish has nothing to commit");
    }

    #[test]
    fn deleting_the_last_composed_letter_cancels_the_composition() {
        let mut kb = Keyboard::default();
        kb.handle(Msg::Compose("h".into()));
        assert_eq!(kb.handle(Msg::Compose(String::new())), vec![preedit("")]);
        assert!(kb.handle(Msg::Compose(String::new())).is_empty(), "nothing left to cancel");
    }

    /// A suggestion over a word already in the text replaces it once, not doubles it.
    #[test]
    fn a_suggestion_over_an_existing_word_replaces_it_once() {
        let mut kb = Keyboard::default();
        kb.set_around(around("say helo", 8));
        assert!(kb.handle(Msg::Region { start: 4, end: 8 }).is_empty(), "a region only marks the words");
        let ev = kb.handle(Msg::Commit("hello ".into()));
        assert_eq!(ime_events(&ev), vec![&ImeEvent::DeleteSurrounding { before_chars: 4, after_chars: 0 }, &ImeEvent::Commit("hello ".into())]);
        // The region is used up: the next commit erases nothing.
        assert_eq!(kb.handle(Msg::Commit("x".into())), vec![commit("x")]);
    }

    #[test]
    fn a_region_whose_text_moved_on_is_dropped() {
        let mut kb = Keyboard::default();
        kb.set_around(around("say helo", 8));
        kb.handle(Msg::Region { start: 4, end: 8 });
        kb.set_around(around("say helo", 3)); // the caret moved
        assert_eq!(kb.handle(Msg::Commit("hello".into())), vec![commit("hello")]);
    }

    #[test]
    fn no_region_without_a_text_position() {
        let mut kb = Keyboard::default();
        kb.set_around(None);
        kb.handle(Msg::Region { start: 0, end: 3 });
        assert_eq!(kb.handle(Msg::Commit("a".into())), vec![commit("a")]);
    }

    #[test]
    fn enter_in_a_commit_is_an_enter_press_between_lines() {
        let mut kb = Keyboard::default();
        let ev = kb.handle(Msg::Commit("ab\ncd".into()));
        assert_eq!(ev.len(), 1 + 2 + 1);
        assert_eq!(ev[0], commit("ab"));
        assert!(matches!(ev[1], Event::Key { key: Key::Enter, pressed: true, .. }));
        assert!(matches!(ev[2], Event::Key { key: Key::Enter, pressed: false, .. }));
        assert_eq!(ev[3], commit("cd"));
    }

    /// A key press while composing commits the word first, so Enter does not drop it or start a reset.
    #[test]
    fn a_key_press_commits_the_composition_first() {
        let mut kb = Keyboard::default();
        kb.handle(Msg::Compose("hi".into()));
        let ev = kb.handle(Msg::Key(KEYCODE_ENTER));
        assert_eq!(ev[0], commit("hi"));
        assert!(matches!(ev[1], Event::Key { key: Key::Enter, pressed: true, .. }));
        assert!(kb.handle(Msg::Key(9999)).is_empty(), "an unknown key code does nothing");
    }

    #[test]
    fn delete_counts_are_clamped() {
        let mut kb = Keyboard::default();
        let ev = kb.handle(Msg::Delete { before: usize::MAX, after: 5 });
        assert_eq!(ev, vec![Event::Ime(ImeEvent::DeleteSurrounding { before_chars: MAX_ERASE, after_chars: 5 })]);
        assert!(kb.handle(Msg::Delete { before: 0, after: 0 }).is_empty());
    }

    /// The context Kotlin reads has the composition in it, marked, with the caret after it.
    #[test]
    fn the_context_marks_the_composition() {
        let mut kb = Keyboard::default();
        kb.set_around(around("ab cd", 2));
        kb.handle(Msg::Compose("é".into()));
        let c = kb.context();
        assert_eq!(c.text, "abé cd");
        assert_eq!((c.caret, c.composing), (3, Some((2, 3))));
        kb.set_around(None);
        assert_eq!(kb.context(), Context { text: String::new(), caret: 0, composing: None }, "nothing editable: an empty context");
    }

    /// egui's reset is acknowledged only with its own id: the keyboard does not go dead after a
    /// tap during composition, and a stale answer is ignored.
    #[test]
    fn a_reset_is_acknowledged_with_its_own_id_only() {
        let ctx = egui::Context::default();
        let id = ctx.request_ime_reset(egui::ViewportId::ROOT).expect("a reset id");
        let mut kb = Keyboard::default();
        kb.handle(Msg::Compose("he".into()));
        assert!(kb.on_reset_request(Some(id)), "a new request is reported once");
        assert!(!kb.on_reset_request(Some(id)), "and not again");
        assert_eq!(kb.context().composing, None, "the composition is gone with the reset");
        assert!(kb.handle(Msg::ResetDone(id.as_u64() + 1)).is_empty(), "another id is not this reset");
        assert_eq!(kb.handle(Msg::ResetDone(id.as_u64())), vec![Event::ImeResetCompleted { request_id: id }]);
        assert!(kb.handle(Msg::ResetDone(id.as_u64())).is_empty(), "answered once");
    }

    #[test]
    fn a_new_edit_target_forgets_the_composition_and_region() {
        let mut kb = Keyboard::default();
        kb.set_around(around("say helo", 8));
        kb.handle(Msg::Region { start: 4, end: 8 });
        kb.handle(Msg::Compose("x".into()));
        kb.retarget();
        assert!(kb.handle(Msg::Finish).is_empty());
    }
}
