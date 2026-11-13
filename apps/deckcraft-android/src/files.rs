//! Documents through Android's Storage Access Framework (SAF): the picker opens them, the system
//! save dialog creates them, and plain Save writes back to the document's content URI.
//!
//! Rust never calls Kotlin. Rust queues a line (`take_request`); `MainActivity.kt` polls it, shows
//! the system dialog or copies the bytes from a cache file, then reports through `save_done`,
//! `opened` or `notice`.
//!
//! A picked document's *location* is its content URI, a slash, then the provider's display name,
//! as in `content://…/document/…/talk.pptx`. The name is the last segment, so the title and the
//! file-type rules work unchanged. Only `.pptx` and `.deckcraft` documents get a location: other formats are opened
//! as copies and Save asks where to put the presentation.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// The app's private files folder, set at start-up. Cache files for Kotlin to copy live here.
pub static DIR: OnceLock<PathBuf> = OnceLock::new();

/// The folder of the engine's AutoRecover entries, inside the app's private files.
pub fn recovery_dir() -> Option<PathBuf> {
    DIR.get().map(|d| d.join("recovery"))
}

/// Set when DeckCraft asks for a document; `take_request` hands it to Kotlin once.
static OPEN_REQUESTED: AtomicBool = AtomicBool::new(false);
/// What the picker is for (`open`, `picture`, `audio`, `video`), as the UI asked.
static OPEN_PURPOSE: Mutex<String> = Mutex::new(String::new());
/// Ids of the open requests the picker has not answered yet, oldest first. The UI matches the
/// file it gets back to its request by id (`InboxFile::id`).
static OPEN_IDS: Mutex<VecDeque<u64>> = Mutex::new(VecDeque::new());
/// Ids of requests a newer request replaced: the app reports them as cancelled.
static CANCELLED: Mutex<Vec<u64>> = Mutex::new(Vec::new());
static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);
/// Lines for Kotlin, oldest first: `save\tticket\tname\tpath` or `write\tticket\turi\tpath`.
static REQUESTS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
/// Saves Kotlin has not reported on yet.
static FLIGHTS: Mutex<Vec<Flight>> = Mutex::new(Vec::new());
/// Picked documents: (location, cache file path).
static OPENED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
/// Finished saves: (presentation uid, the revision that was saved, the document's location). The
/// location is `None` when the save was cancelled or failed, and empty for a file that is not the
/// presentation (an export). The app marks that revision clean and adopts the location (plain Save
/// writes there next time).
static SAVED: Mutex<Vec<(u64, u64, Option<String>)>> = Mutex::new(Vec::new());
/// The active presentation's uid and revision at the start of this frame; a save queued now saves that state.
static SAVE_UID: AtomicU64 = AtomicU64::new(0);
static SAVE_REV: AtomicU64 = AtomicU64::new(0);
/// The active presentation's location, when it has one a plain Save can write back to.
static CURRENT: Mutex<Option<String>> = Mutex::new(None);
/// Messages for the status line.
static NOTICES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A save in flight: its ticket, the name it saves under, the presentation revision it saves, and
/// whether a success makes the file the presentation's location (a `.xlsx`, Save As or write back).
struct Flight {
    ticket: u64,
    uid: u64,
    name: String,
    rev: u64,
    adopt: bool,
}

/// Called by the app each frame with the active presentation's uid and revision.
pub fn set_revision(uid: u64, rev: u64) {
    SAVE_UID.store(uid, Ordering::Relaxed);
    SAVE_REV.store(rev, Ordering::Relaxed);
}

/// Called by the app each frame with the active presentation's location, if it has one.
pub fn set_current(loc: Option<String>) {
    if let Ok(mut c) = CURRENT.lock() {
        *c = loc;
    }
}

/// The app asked to write `name`. A save of the active presentation under its own file name goes
/// back to its document; anything else (Save As, a first save, an export) asks with the system dialog.
pub fn save(name: &str, bytes: &[u8]) {
    let current = CURRENT.lock().ok().and_then(|c| c.clone());
    let rev = SAVE_REV.load(Ordering::Relaxed);
    if let Some(loc) = current
        && keeps_location(name)
        && parse_location(&loc).is_some_and(|(_, n)| n == name)
        && save_in_place(&loc, bytes, rev)
    {
        return;
    }
    save_as(name, bytes);
}

/// Asks Kotlin to show the document picker for `purpose`, as request `id`. One picker is up at a
/// time, so a request still waiting is cancelled by the new one.
pub fn request_open(purpose: &str, id: u64) {
    if let Ok(mut ids) = OPEN_IDS.lock() {
        if let Ok(mut c) = CANCELLED.lock() {
            c.extend(ids.drain(..));
        }
        ids.push_back(id);
    }
    if let Ok(mut p) = OPEN_PURPOSE.lock() {
        *p = purpose.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    }
    OPEN_REQUESTED.store(true, Ordering::Relaxed);
    crate::ime::wake();
}

/// Kotlin: the picker was dismissed, or the picked file could not be read. The oldest open request
/// is over, and the UI hears it as cancelled.
pub fn open_cancelled() {
    let id = OPEN_IDS.lock().ok().and_then(|mut ids| ids.pop_front());
    if let Some(id) = id
        && let Ok(mut c) = CANCELLED.lock()
    {
        c.push(id);
    }
    crate::ime::wake();
}

/// Requests that a newer request replaced since the last call.
pub fn drain_cancelled() -> Vec<u64> {
    CANCELLED.lock().map(|mut c| c.drain(..).collect()).unwrap_or_default()
}

/// Kotlin: the next request, or an empty string when there is none.
pub fn take_request() -> String {
    if OPEN_REQUESTED.swap(false, Ordering::Relaxed) {
        let purpose = OPEN_PURPOSE.lock().map(|p| p.clone()).unwrap_or_default();
        return format!("open\t{}", if purpose.is_empty() { "open" } else { &purpose });
    }
    REQUESTS.lock().ok().and_then(|mut q| q.pop_front()).unwrap_or_default()
}

/// The location of the document at `uri` whose provider name is `name`. A slash or control
/// character in the name becomes `_`, so the last slash always starts the name.
pub fn location(uri: &str, name: &str) -> String {
    let name: String = name.chars().map(|c| if c == '/' || c.is_control() { '_' } else { c }).collect();
    let name = if name.trim().is_empty() || name == "." || name == ".." { "document".to_string() } else { name };
    format!("{uri}/{name}")
}

/// The (URI, name) of `loc`, or `None` when it is not a content location.
pub fn parse_location(loc: &str) -> Option<(&str, &str)> {
    let (uri, name) = loc.rsplit_once('/')?;
    (uri.starts_with("content://") && uri.len() > "content://".len() && !name.is_empty()).then_some((uri, name))
}

/// Whether a document named `name` may keep a location (plain Save writes it back in place).
pub fn keeps_location(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".pptx") || name.to_ascii_lowercase().ends_with(".deckcraft")
}

fn tidy(name: &str) -> String {
    name.replace(['\t', '\n'], " ")
}

/// Writes `bytes` to a new cache file for Kotlin to copy. Failures go to the status line.
fn stage(name: &str, bytes: &[u8]) -> Option<(u64, String)> {
    let Some(dir) = DIR.get() else {
        notice(format!("Couldn't save {name}: no storage folder"));
        return None;
    };
    let ticket = NEXT_TICKET.fetch_add(1, Ordering::Relaxed);
    let file = dir.join(format!("save-{ticket}.bin"));
    match std::fs::write(&file, bytes) {
        Ok(()) => Some((ticket, file.to_string_lossy().into_owned())),
        Err(e) => {
            notice(format!("Couldn't save {name}: {e}"));
            None
        }
    }
}

fn queue(line: String, flight: Flight) {
    if let Ok(mut f) = FLIGHTS.lock() {
        f.push(flight);
    }
    if let Ok(mut q) = REQUESTS.lock() {
        q.push_back(line);
    }
    crate::ime::wake();
}

/// Reports a save of revision `rev` that never reached Kotlin as failed, so the app drops any
/// "close after save" it was waiting for (`app::Shell::finish_save`).
fn not_staged(uid: u64, rev: u64) {
    if let Ok(mut s) = SAVED.lock() {
        s.push((uid, rev, None));
    }
    crate::ime::wake();
}

/// Save As: the system dialog creates a document named `name` holding `bytes`.
pub fn save_as(name: &str, bytes: &[u8]) {
    let (uid, rev) = (SAVE_UID.load(Ordering::Relaxed), SAVE_REV.load(Ordering::Relaxed));
    let Some((ticket, path)) = stage(name, bytes) else { return not_staged(uid, rev) };
    let line = format!("save\t{ticket}\t{}\t{path}", tidy(name));
    queue(line, Flight { ticket, uid, name: name.to_string(), rev, adopt: keeps_location(name) });
}

/// Plain Save of a picked document: writes `bytes` (the presentation at revision `rev`) over `loc`. Returns
/// `false` when `loc` is not a content location, so the caller can fall back to the engine's own Save.
pub fn save_in_place(loc: &str, bytes: &[u8], rev: u64) -> bool {
    let Some((uri, name)) = parse_location(loc) else { return false };
    let uid = SAVE_UID.load(Ordering::Relaxed);
    let Some((ticket, path)) = stage(name, bytes) else {
        not_staged(uid, rev);
        return true;
    };
    let line = format!("write\t{ticket}\t{uri}\t{path}");
    queue(line, Flight { ticket, uid, name: name.to_string(), rev, adopt: true });
    true
}

/// Kotlin: save `ticket` ended. Status 1 saved (`msg` is the document URI, `display` its name),
/// 0 cancelled, anything else failed with `msg`.
pub fn save_done(ticket: u64, status: i32, msg: String, display: String) {
    let flight = FLIGHTS.lock().ok().and_then(|mut f| f.iter().position(|x| x.ticket == ticket).map(|i| f.swap_remove(i)));
    let Some(flight) = flight else { return };
    // The revision's outcome for the app: the document's location when saved (empty when it has none),
    // None when cancelled or failed (`app::Shell::finish_save`).
    let outcome = match status {
        1 => {
            notice(format!("Saved {display}"));
            let loc = if flight.adopt && !msg.is_empty() { location(&msg, &display) } else { String::new() };
            Some(loc)
        }
        0 => None,
        _ => {
            notice(format!("Couldn't save {}: {msg}", flight.name));
            None
        }
    };
    if let Ok(mut s) = SAVED.lock() {
        s.push((flight.uid, flight.rev, outcome));
    }
    crate::ime::wake();
}

/// Kotlin: the user picked the document at `uri`; its bytes are in the cache file at `path`.
pub fn opened(name: String, path: String, uri: String) {
    if let Ok(mut v) = OPENED.lock() {
        v.push((location(&uri, &name), path));
    }
    crate::ime::wake();
}

/// Kotlin or Rust: say `msg` on the status line.
pub fn notice(msg: String) {
    if let Ok(mut n) = NOTICES.lock() {
        n.push(msg);
    }
    crate::ime::wake();
}

pub fn drain_notices() -> Vec<String> {
    NOTICES.lock().map(|mut n| n.drain(..).collect()).unwrap_or_default()
}

/// Saves that ended since the last call: (presentation uid, revision saved, location).
pub fn drain_saved() -> Vec<(u64, u64, Option<String>)> {
    SAVED.lock().map(|mut s| s.drain(..).collect()).unwrap_or_default()
}

/// Picked documents as (request id, location, bytes). The cache files are removed; unreadable ones
/// are reported. A file with no request waiting has no id.
pub fn drain_opened() -> Vec<(Option<u64>, String, Vec<u8>)> {
    let picked: Vec<(String, String)> = OPENED.lock().map(|mut v| v.drain(..).collect()).unwrap_or_default();
    picked
        .into_iter()
        .filter_map(|(loc, path)| {
            let bytes = std::fs::read(&path);
            let _ = std::fs::remove_file(&path);
            let id = OPEN_IDS.lock().ok().and_then(|mut ids| ids.pop_front());
            match bytes {
                Ok(b) => Some((id, loc, b)),
                Err(e) => {
                    notice(format!("Couldn't read {loc}: {e}"));
                    if let Some(id) = id
                        && let Ok(mut c) = CANCELLED.lock()
                    {
                        c.push(id);
                    }
                    None
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A newer open request cancels the one still waiting; a picked file answers the oldest one; a
    /// dismissed picker cancels it. (One test: the queues are process-wide.)
    #[test]
    fn open_requests_are_answered_in_order_and_replaced_ones_are_cancelled() {
        let _ = drain_cancelled();
        request_open("open", 1);
        request_open("picture", 2);
        assert_eq!(drain_cancelled(), vec![1], "the newer request replaces the waiting one");
        assert_eq!(take_request(), "open\tpicture");
        open_cancelled();
        assert_eq!(drain_cancelled(), vec![2], "a dismissed picker cancels the request");
        open_cancelled();
        assert!(drain_cancelled().is_empty(), "nothing is waiting, so nothing is cancelled");
        request_open("a/b c!", 3);
        assert_eq!(take_request(), "open\tabc", "the purpose is reduced to letters and digits");
        open_cancelled();
        let _ = drain_cancelled();
    }

    #[test]
    fn locations_keep_the_name_last_and_round_trip() {
        let uri = "content://auth/doc/primary%3ADownload%2Ftalk.pptx";
        let loc = location(uri, "talk (1).pptx");
        assert_eq!(parse_location(&loc), Some((uri, "talk (1).pptx")));
        assert_eq!(location("content://a/b", "x/y\tz.xlsx"), "content://a/b/x_y_z.xlsx");
        assert_eq!(location("content://a/b", ".."), "content://a/b/document");
        assert_eq!(parse_location("/data/user/0/app/files/talk.pptx"), None, "a file path is not a location");
        assert_eq!(parse_location("content://a"), None, "no name");
    }

    #[test]
    fn saves_are_routed_and_adopted_and_exports_are_not() {
        // One test owns the saved queue and the storage folder, so the parallel test runner cannot
        // drain it under us.
        // No storage folder is set yet, so staging fails: the app must hear that the save ended
        // (a None outcome clears "close after save"), or a later plain Save could close the app.
        set_revision(9, 4);
        let rev = SAVE_REV.load(Ordering::Relaxed);
        save_as("Deck1.pptx", b"x");
        assert!(save_in_place("content://d/u/Deck1.pptx", b"x", rev));
        assert_eq!(drain_saved(), vec![(9, 4, None), (9, 4, None)], "a save that cannot be staged reports as failed");
        let (ok, ex) = (7_000_001, 7_000_002);
        queue("save".into(), Flight { ticket: ok, uid: 9, name: "Deck1.pptx".into(), rev: 5, adopt: keeps_location("Deck1.pptx") });
        queue("save".into(), Flight { ticket: ex, uid: 9, name: "Book1.csv".into(), rev: 5, adopt: keeps_location("Book1.csv") });
        save_done(ok, 1, "content://d/u".into(), "Deck1.pptx".into());
        save_done(ex, 1, "content://d/p".into(), "Book1.csv".into());
        // The export is reported with an empty location: it did not adopt one, and it does not clean the presentation.
        assert_eq!(drain_saved(), vec![(9, 5, Some("content://d/u/Deck1.pptx".to_string())), (9, 5, Some(String::new()))]);
        assert!(drain_notices().iter().any(|n| n == "Saved Book1.csv"));

        // With a storage folder, `save` writes back to the active presentation's document under its
        // own name, and asks with the dialog for anything else.
        let dir = std::env::temp_dir().join(format!("deckcraft-files-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = DIR.set(dir.clone());
        while !take_request().is_empty() {}
        set_current(Some("content://d/u/Deck1.pptx".into()));
        save("Deck1.pptx", b"x");
        assert!(take_request().starts_with("write\t"), "its own name goes back to the document");
        save("Other.pptx", b"x");
        assert!(take_request().starts_with("save\t"), "another name asks where to put it");
        save("Deck1.pdf", b"x");
        assert!(take_request().starts_with("save\t"), "an export asks where to put it");
        set_current(None);
        save("Deck1.pptx", b"x");
        assert!(take_request().starts_with("save\t"), "a presentation with no document asks where to put it");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
