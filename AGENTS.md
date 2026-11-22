# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A native desktop app in Rust that opens a folder of camera recordings (`<camera-id>_<yyyyMMddTHHmmss>.mkv`) and plays all cameras at the same time of day. macOS and Windows are the supported platforms. OS-specific code stays in `src/platform/`.

`specs/` is the source of truth for behavior. Read the related spec before you change behavior.

## Commands

```sh
./scripts/build-ffmpeg.sh                     # one time: builds the FFmpeg of the project in ffmpeg-build/
cargo run --release -- /path/to/folder        # run
cargo test                                    # unit tests + end-to-end playback tests
cargo test --lib time::tests::parses_typed_time     # one unit test
cargo test --test playback paused_jump              # one end-to-end test (name filter)
cargo clippy --all-targets --features selftest      # lint, must be clean
./scripts/bundle-macos.sh                     # builds dist/Multi Video Player.app (one executable)
```

On Windows the cargo commands are the same. The scripts are PowerShell scripts:

```powershell
.\scripts\build-ffmpeg.ps1                    # one time: builds the FFmpeg of the project in ffmpeg-build\
.\scripts\bundle-windows.ps1                  # builds dist\Multi Video Player.exe (one executable)
```

- The project never uses an FFmpeg of the system (Homebrew, `pkg-config`, or `ffmpeg` in `PATH`). `scripts/build-ffmpeg.sh` builds static libraries in `ffmpeg-build/prefix/` and a small `ffmpeg` command in `ffmpeg-build/tools/bin/`. `.cargo/config.toml` sets `FFMPEG_DIR` to the libraries, and all cargo commands link them statically.
- Windows uses PowerShell scripts only. `scripts/build-ffmpeg.ps1` is the Windows form of `scripts/build-ffmpeg.sh`: it does not call the shell script, and a change for Windows must not change the shell scripts. It runs the configure script and `make` of FFmpeg in the shell of MSYS2 with the Microsoft compiler. Keep the FFmpeg version and the decoder list equal in the two scripts.
- If cargo fails in `ffmpeg-sys-next` with `'libavutil/avutil.h' file not found`, run the FFmpeg build script.
- Windows needs LLVM for `libclang` (the `ffmpeg-sys-next` build uses it). FFmpeg and the Rust program link the C runtime library statically (`-MT` in `scripts/build-ffmpeg.ps1`, `crt-static` in `.cargo/config.toml`). Keep the two equal.
- `tests/playback.rs` makes small videos with `ffmpeg-build/tools/bin/ffmpeg`. Do not call a system `ffmpeg` or `ffprobe` in tests or scripts.
- To add a decoder or another FFmpeg part, change the configure options in `scripts/build-ffmpeg.sh` and increase the number in its `WANT` line, so the script builds again. Do the same in `scripts/build-ffmpeg.ps1` (`$want`).
- cargo keeps a copy of the FFmpeg libraries in the build of `ffmpeg-sys-next` and does not see new libraries. The Windows script removes that copy after a build. In other cases, run `cargo clean -p ffmpeg-sys-next` (and with `--release`) after a new FFmpeg build.
- `cargo build` does not update `dist/Multi Video Player.app` or `dist\Multi Video Player.exe`. Run the bundle script after a change if the app bundle must have it. The bundle script uses the normal release build in `target/release`.
- A computer builds the code of its system only. After a change in `platform/mod.rs`, or in a type that the platform files use (for example `HwDecode`, `ImportedSurface`), update `macos.rs`, `windows.rs`, `fallback.rs`, and `tests/playback.rs` in the same change, and say which system was not built.

## Library versions

egui/eframe 0.37, wgpu 30, and ffmpeg-sys-next 9 are recent, and their APIs differ from older examples. Check the crate source in `~/.cargo/registry/src/` before you assume an API. Examples in this code: `eframe::App::ui(&mut self, ui, frame)` (not `update`), `egui::Panel::bottom(..).show(ui, ..)`, `egui::Id::unique(..)`, `egui::Popup::menu(&response)`, `egui::Modal`.

## Architecture

| Module | Role |
|---|---|
| `time`, `library`, `clock` | Pure logic: local timestamps, folder scan and camera timelines, the shared clock. No media or UI dependency. |
| `media` | Frame types, and the FFmpeg backend (`media/ffmpeg.rs`, raw FFI): length probe, decoder, hardware decode. |
| `playback` | One decode thread for each shown camera. |
| `render` | wgpu pipeline that draws NV12/P010 frames inside the egui render pass, and reads one frame back as a picture. |
| `platform` | Hardware decoder choice, zero-copy frame import, native menu, folder dialog, open a file in the web browser. `macos.rs`, `windows.rs`, and `fallback.rs` have the same functions. |
| `ui` | egui app. |

### Playback model (read `playback/mod.rs` and `playback/worker.rs` together)

- There is one `Clock`: a time of day that moves by real time x speed. It never waits for a camera. Each jump increments its `epoch`.
- Each `CameraPlayer` has a worker thread that decodes ahead of the clock into a small queue of `Slot`s. A slot is a frame or a "black" marker, with its time of day.
- On each refresh the UI thread calls `CameraPlayer::update`, which takes the latest slot that is not after the clock time. The UI thread never decodes and never waits.
- Frames of an old epoch are never shown. A tile is black after a jump until the exact frame is ready. This is a spec rule, not a fault.
- The worker opens the next chunk on a helper thread before the clock reaches it, skips to key frames when it is late, and decodes key frames only at 8x and 16x.
- A change to the clock must be followed by `wake()` on all players. The `Session` methods do this. Use them, not the `Clock` directly.

### Frames and rendering

- A decoded frame is `FrameData::Hw` (a frame in the memory of the hardware decoder, imported as GPU textures with no copy through `platform::HwFrameImporter`) or `FrameData::Cpu` (NV12 in memory, uploaded).
- macOS: the hardware frame is a `CVPixelBuffer` from VideoToolbox, imported as Metal textures.
- Windows: FFmpeg decodes with Direct3D 11 on its own device, which `platform/windows.rs` makes on the GPU of the renderer (`media::ffmpeg::install_hw_device`). The renderer uses Direct3D 12 and cannot read the frames of the decoder. The decode thread calls `HwDecode::export`, which copies the picture on the GPU to a shared texture and waits for the copy. The `HwSurface` then holds that texture (`HwSurface::exported`) and not the frame of the decoder. If the two devices cannot share textures, the app uses a frame copy through memory.
- Do not move the Windows decode to the Direct3D 12 device of the renderer (`AV_HWDEVICE_TYPE_D3D12VA`). It was tested: a stream that the decoder cannot decode makes the device fail, and the renderer stops with it.
- `ImportedSurface::visible` gives the part of the textures that holds the picture. The shader does not read outside it.
- `VideoRenderer::prepare` + `paint_callback` draw a tile. The tile rectangle is the only place where the video shape is applied, so the "video ratio" setting costs nothing.
- `VideoRenderer::read_picture` renders one frame with the same shader and reads it back. Bbox capture uses it.

### UI structure

- `ui/mod.rs` holds `App`: the state (`NoFolder`, `Loading`, `NoVideos`, `Failed`, `Ready(Session)`), settings, clipboard, hotkeys (`handle_keys`), and the refresh scheduling.
- `ui/session.rs` holds one open folder: library, clock, tiles, and dialog state. Widgets do not act on the app directly. They set request fields on the session (`copy_request`, `copy_picture_request`, `message`) or return a `BarAction`, and `App` handles them.
- The window is drawn only when necessary. While playing, `App` asks for the next refresh at the due time of the next queued frame. Do not add a continuous repaint.
- eframe does not call `ui()` for a covered or minimized window. `App::logic` keeps a slow heartbeat for that case.
- The settings dialog (`ui/settings.rs`) belongs to `App`, not to the session, because it also works with no folder open. `App::show_settings` applies a change. A change of the "Decoder" setting calls `Session::set_decode`, which starts the players of all shown cameras again.
- Icons are Material SVG files in `assets/icons/` (see its README). The app icon is `assets/app-icon/app-icon.svg`: `ui/app_icon.rs` draws it for the Dock or the task bar, the macOS bundle script makes `AppIcon.icns` from it through `examples/app_iconset.rs`, and on Windows `build.rs` puts it in the executable. The user manual is `assets/help/manual.html`. The app does not show HTML: "Help" writes a copy of the manual to the temporary folder and opens it in the system web browser (`ui/help.rs`, `platform::open_in_browser`). The copy shows the app version: the text `{{version}}` in the manual is replaced with the version from `Cargo.toml`.

## Rules for a behavior change

Make these in the same change:

1. Update the spec in `specs/` and, if a control or hotkey changes, the controls table in `README.md`.
2. Update `assets/help/manual.html`. If a hotkey changes, also update the list in the test `ui::help::tests::manual_lists_every_hotkey`.
3. A new hotkey goes in `App::handle_keys`. Hotkeys must do nothing while a dialog is open or a text box has the focus.

Specs, README, the manual, and code comments use short and direct sentences (Simplified Technical English) and no dash as punctuation.

## Testing the real window (`selftest` feature)

`cargo build --release --features selftest` adds a script tool (`src/ui/selftest.rs`). The `MULTI_VIDEO_PLAYER_SCRIPT` environment variable holds steps such as `1.0:key:Space;1.5:click:96,776;2.0:stats;2.2:shot:out.ppm;2.5:quit`. `stats` prints the clock, the settings, the frame time and lag of each tile, the memory of its frame (`frame=hw` or `frame=cpu`), and the dialog states. `shot` writes the window picture.

- The test window is usually covered by other windows, and screen capture is not permitted. The tool forces painted frames, so `shot` and `stats` still work.
- Use the `clipboard:<text>` step first when a test copies or reads the clipboard. It replaces the system clipboard with a test clipboard, so the clipboard of the user is not changed. Never print the content of the real clipboard.
- Do not click "Open Folder…" in a script unless `MULTI_VIDEO_PLAYER_TEST_FOLDER` is set. Without it, the native folder dialog opens on the screen of the user.
- "Help" in a script does not open the web browser. It writes the manual file and prints its path (`help manual=<path>`).
- The first launch after a build can lose a pointer event. Do one short warm-up launch before a test with clicks.
- `MULTI_VIDEO_PLAYER_DECODE=software` starts the app with the "Decoder" setting at "Software". `copy` turns off zero copy. Use them to test the other decode paths.
- Windows: the window of a test opens on the screen of the user and takes the focus. A release build with no `selftest` feature has no console, so use the `selftest` build for scripts. In a shell pipeline, read the exit code of the app itself: a crash at exit is hidden when only the output of a later command is checked.
