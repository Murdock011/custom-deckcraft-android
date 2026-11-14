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

## Open

- Run it on an emulator and a phone, and fix what breaks.
- Switch the keyboard to `Event::Ime` (Deck's IME handling already exists); check that the reset
  phase is acknowledged when Android skips `set_ime_allowed`.
- Format and Insert sheets, slide thumbnails, Paste button, pinch zoom, slide show touch controls.
- File ▸ Close does not end the activity.
