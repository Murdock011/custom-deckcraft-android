# Changes to the DeckCraft engine in this fork

This fork adds a native Android host (`apps/deckcraft-android`, `packaging/android-native/`) on top
of the DeckCraft engine (see the [README](../README.md)). This page lists every change it makes
**below the user interface**, so anyone can see exactly where it departs from upstream. All of
these were written by AI coding agents.

Baseline: [storytold/deckcraft](https://github.com/storytold/deckcraft) `main` at commit
`69379cf6` (engine 0.18.1). The fork keeps the full upstream history.

"Engine" here means every crate except `crates/ui-egui` (the interface) and `apps/` (the programs
that host it). The Android host lives in `apps/deckcraft-android`.

## Behaviour changes

| Area | File | What changed | Why |
|---|---|---|---|
| AutoRecover | `crates/engine/src/recovery.rs` | Adds `Snapshot`, `snapshots` and `save_snapshots`: the unsaved presentations as shared data that can be written from another thread. `save` now uses them and writes the same files as before. | Android can kill a paused app before another frame runs, so the host writes the copies from the pause callback's thread. |

## No behaviour change

| File | What changed |
|---|---|
| `xtask/src/layers.rs` | `deckcraft-android` is listed as an exempt app, like the other hosts. |
| `Cargo.toml` | `egui` and `egui-winit` point at `vendor/` (the same revision with the Android keyboard patches). |

## Tests added

- `recovery::tests::a_snapshot_is_written_from_another_thread_as_it_was` (engine).
- Host tests in `apps/deckcraft-android` (file locations, open request ids, save routing).
- `phone::tests` in `crates/ui-egui` (the breakpoint, and the chrome running commands).

Last updated: 2026-11-14.
