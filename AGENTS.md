# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A native desktop app in Rust that opens a folder of camera recordings (`<camera-id>_<yyyyMMddTHHmmss>.mkv`) and plays all cameras at the same time of day. macOS is the supported platform. Windows is a later target, so OS-specific code stays in `src/platform/`.

`specs/` is the source of truth for behavior. Read the related spec before you change behavior.

## Commands

```sh
cargo run --release -- /path/to/folder        # run (links to the Homebrew FFmpeg through pkg-config)
cargo test                                    # unit tests + end-to-end playback tests
cargo test --lib time::tests::parses_typed_time     # one unit test
cargo test --test playback paused_jump              # one end-to-end test (name filter)
cargo clippy --all-targets --features selftest      # lint, must be clean
./scripts/bundle-macos.sh                     # builds dist/Video Player.app (static FFmpeg, one executable)
```

- `tests/playback.rs` makes small videos with the `ffmpeg` command. The tests pass with a note if the command is not installed.
- `cargo build` does not update `dist/Video Player.app`. Run `scripts/bundle-macos.sh` after a change if the app bundle must have it. The bundle build uses its own target folder (`target/static`) and the `static-ffmpeg` feature with `FFMPEG_DIR`, so it does not disturb the normal build.

## Library versions

egui/eframe 0.37, wgpu 30, and ffmpeg-sys-next 9 are recent, and their APIs differ from older examples. Check the crate source in `~/.cargo/registry/src/` before you assume an API. Examples in this code: `eframe::App::ui(&mut self, ui, frame)` (not `update`), `egui::Panel::bottom(..).show(ui, ..)`, `egui::Id::unique(..)`, `egui::Popup::menu(&response)`, `egui::Modal`.

## Architecture

| Module | Role |
|---|---|
| `time`, `library`, `clock` | Pure logic: local timestamps, folder scan and camera timelines, the shared clock. No media or UI dependency. |
| `media` | Frame types, and the FFmpeg backend (`media/ffmpeg.rs`, raw FFI): length probe, decoder, hardware decode. |
| `playback` | One decode thread for each shown camera. |
| `render` | wgpu pipeline that draws NV12/P010 frames inside the egui render pass, and reads one frame back as a picture. |
| `platform` | Hardware decoder choice, zero-copy frame import, native menu, folder dialog, open a file in the web browser. `macos.rs` and `fallback.rs` have the same functions. |
| `ui` | egui app. |

### Playback model (read `playback/mod.rs` and `playback/worker.rs` together)

- There is one `Clock`: a time of day that moves by real time x speed. It never waits for a camera. Each jump increments its `epoch`.
- Each `CameraPlayer` has a worker thread that decodes ahead of the clock into a small queue of `Slot`s. A slot is a frame or a "black" marker, with its time of day.
- On each refresh the UI thread calls `CameraPlayer::update`, which takes the latest slot that is not after the clock time. The UI thread never decodes and never waits.
- Frames of an old epoch are never shown. A tile is black after a jump until the exact frame is ready. This is a spec rule, not a fault.
- The worker opens the next chunk on a helper thread before the clock reaches it, skips to key frames when it is late, and decodes key frames only at 8x and 16x.
- A change to the clock must be followed by `wake()` on all players. The `Session` methods do this. Use them, not the `Clock` directly.

### Frames and rendering

- A decoded frame is `FrameData::Hw` (a `CVPixelBuffer` from VideoToolbox, imported as Metal textures with no copy through `platform::HwFrameImporter`) or `FrameData::Cpu` (NV12 in memory, uploaded).
- `VideoRenderer::prepare` + `paint_callback` draw a tile. The tile rectangle is the only place where the video shape is applied, so the "video ratio" setting costs nothing.
- `VideoRenderer::read_picture` renders one frame with the same shader and reads it back. Bbox capture uses it.

### UI structure

- `ui/mod.rs` holds `App`: the state (`NoFolder`, `Loading`, `NoVideos`, `Failed`, `Ready(Session)`), settings, clipboard, hotkeys (`handle_keys`), and the refresh scheduling.
- `ui/session.rs` holds one open folder: library, clock, tiles, and dialog state. Widgets do not act on the app directly. They set request fields on the session (`copy_request`, `copy_picture_request`, `message`) or return a `BarAction`, and `App` handles them.
- The window is drawn only when necessary. While playing, `App` asks for the next refresh at the due time of the next queued frame. Do not add a continuous repaint.
- eframe does not call `ui()` for a covered or minimized window. `App::logic` keeps a slow heartbeat for that case.
- Icons are Material SVG files in `assets/icons/` (see its README). The app icon is `assets/app-icon/app-icon.svg`: `ui/app_icon.rs` draws it for the Dock, and the bundle script makes `AppIcon.icns` from it through `examples/app_iconset.rs`. The user manual is `assets/help/manual.html`. The app does not show HTML: "Help" writes a copy of the manual to the temporary folder and opens it in the system web browser (`ui/help.rs`, `platform::open_in_browser`). The copy shows the app version: the text `{{version}}` in the manual is replaced with the version from `Cargo.toml`.

## Rules for a behavior change

Make these in the same change:

1. Update the spec in `specs/` and, if a control or hotkey changes, the controls table in `README.md`.
2. Update `assets/help/manual.html`. If a hotkey changes, also update the list in the test `ui::help::tests::manual_lists_every_hotkey`.
3. A new hotkey goes in `App::handle_keys`. Hotkeys must do nothing while a dialog is open or a text box has the focus.

Specs, README, the manual, and code comments use short and direct sentences (Simplified Technical English) and no dash as punctuation.

## Testing the real window (`selftest` feature)

`cargo build --release --features selftest` adds a script tool (`src/ui/selftest.rs`). The `VIDEO_PLAYER_SCRIPT` environment variable holds steps such as `1.0:key:Space;1.5:click:96,776;2.0:stats;2.2:shot:out.ppm;2.5:quit`. `stats` prints the clock, the frame time and lag of each tile, and the dialog states. `shot` writes the window picture.

- The test window is usually covered by other windows, and screen capture is not permitted. The tool forces painted frames, so `shot` and `stats` still work.
- Use the `clipboard:<text>` step first when a test copies or reads the clipboard. It replaces the system clipboard with a test clipboard, so the clipboard of the user is not changed. Never print the content of the real clipboard.
- Do not click "Open Folder…" in a script unless `VIDEO_PLAYER_TEST_FOLDER` is set. Without it, the native folder dialog opens on the screen of the user.
- "Help" in a script does not open the web browser. It writes the manual file and prints its path (`help manual=<path>`).
- The first launch after a build can lose a pointer event. Do one short warm-up launch before a test with clicks.
- `VIDEO_PLAYER_DECODE=software` or `copy` turns off hardware decode or zero copy, to test the other decode paths.
