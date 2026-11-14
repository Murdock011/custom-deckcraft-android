# Native Android port (DeckCraft 0.18.1)

Not built or run on Android yet: the container this was written in has no Android NDK and no GPU.
Everything below is checked only by desktop tests, clippy and a desktop type-check of `app.rs`.

## What was done

- `apps/deckcraft-android`: cdylib crate `deckcraft_android`. `android_main` runs eframe with
  `android-game-activity` and wgpu and hosts `deckcraft_ui_egui::SlideApp`. Desktop-only pieces
  (control server, Apple events, printing, audio, preferences) are left out. On desktop the crate is
  an empty library.
- `packaging/android-native/`: copied from the GridCraft fork's Gradle project and renamed to
  `ai.storyteller.deckcraft.native`, label "Custom DeckCraft", library `deckcraft_android`.
  Intent filters and the picker list presentation types. The picker asks for pictures, audio or
  video when the UI asks for those.
- Launcher icon: DeckCraft's upstream placeholder `assets/app-icon/deckcraft-1024.png` at 2/3 size
  on its tile colour (`#F26B1D`). Rows in `ATTRIBUTION.md`.
- `crates/ui-egui/src/phone.rs`: the phone layout (app bar, slide chips, bottom toolbar), used
  below 600 pt wide. `SlideApp::can_go_back` tells the host when Back has something to close.
- `vendor/egui`, `vendor/egui-winit`: copied from the WordCraft fork.

## Adaptations

- Soft keyboard: committed text becomes `egui::Event::Text`; composing text is replaced as it
  changes; Delete and key codes become key presses. Kotlin does not send composing regions.
- Files: the picker's answer carries the request id the UI gave (`Services::open_async`). A
  dismissed picker cancels it. Files handed over by other apps open as dropped files.
- Save: the system dialog or a write back to the opened document. A finished save marks the
  presentation clean only when it is still at the saved revision.
- Recovery: the engine's AutoRecover folder is in the app's private files. On pause the host
  writes the last snapshot from the JNI thread, with no frame.

## Phone sheets (added after the first build)

- `phone_sheet.rs`: a bottom tab bar (Slides, Insert, Text, Shape, Edit, More) opens one sheet of
  tiles above it. The sheet sits in the layout, so the slide stays visible. At most 42% of the
  window height; longer sheets scroll. Formatting tiles keep it open; insert and dialog tiles
  close it. Tiles that need a selection are greyed, with a one-line hint.
- All touch targets are at least 48 pt (tiles 68 pt tall, swatches 48 pt); a compile-time check
  covers the constants.
- Back (Escape) closes an open sheet, and ends a slide show. `can_go_back` reports both.
- Slide show: a finger swipe left or right turns the page (`show/ui.rs`, `swipe_direction`).
- Not run on a device. Tests cover tile command ids, dialog ids, tab toggling, Back, and a draw at
  phone width. `DECKCRAFT_SHOT_DIR=… cargo test -p deckcraft-ui-egui --lib sheet_screenshots`
  writes a PNG of each sheet (needs a Vulkan driver such as lavapipe).

## Open

- Run it on an emulator and a phone, and fix what breaks.
- Switch the keyboard to `Event::Ime` (Deck's IME handling already exists); check that the reset
  phase is acknowledged when Android skips `set_ime_allowed`.
- Slide thumbnails (the chips show numbers only), pinch zoom on a device, show controls for pen
  and laser by touch.
- File ▸ Close does not end the activity.
