# Video Player

A native desktop app that opens a folder of camera recordings and plays all cameras at the same time of day.
The behavior is defined in [specs/](specs/README.md).

## Requirements

- Rust 1.95 or later.
- FFmpeg shared libraries and `pkg-config`. On macOS: `brew install ffmpeg pkg-config`. Tested with FFmpeg 7.1.
- macOS. Other systems use the fallback platform layer (see "Platform layer").

## Build and run

```sh
cargo run --release                      # opens with no folder
cargo run --release -- /path/to/folder   # opens a folder
cargo test                               # unit tests and end-to-end playback tests
```

The end-to-end tests make small videos with the `ffmpeg` command. They are skipped if the command is not installed.

## App bundle

```sh
./scripts/bundle-macos.sh
open "dist/Video Player.app"
```

The script makes `dist/Video Player.app`. The app is one executable of about 16 MB with FFmpeg inside. It uses only libraries that are part of macOS, so it runs with no Homebrew installation. You can move it to `/Applications` and open it with a double-click.

- The first run downloads the FFmpeg source (pinned to one commit) and builds a small static FFmpeg in `target/ffmpeg-static/`. This takes about 1 minute. Later runs use the result again.
- The static FFmpeg holds the MKV reader and these video decoders: H.264, HEVC, MJPEG, MPEG-4, MPEG-2, VP8, VP9, AV1. Change `DECODERS` in `scripts/build-ffmpeg.sh` to add a format.
- The app runs on the CPU type of the Mac that built it (Apple Silicon or Intel).
- The signature is a local signature. To give the app to other persons, sign it with a Developer ID and notarize it. If you do not, macOS blocks the app on their Macs.
- This FFmpeg build has the LGPL license. If you distribute the app, obey the LGPL rules for static linking.

## Controls

| Input | Result |
|---|---|
| Space | Play or pause |
| Left / Right arrow | Jump 10 seconds back / forward |
| C, or the camera button | Open the camera picker |
| Cmd+O | Open a folder |
| Menu button (three dots) | "Open Folder…", "Close Folder", and "Settings" (video ratio: "16:9" stretches each video to 16:9, "None" keeps the shape of the video) |
| G | Open the "go to time" dialog. Type or paste `HH:mm:ss` or `HHmmss` |
| Shift+G | Jump to the time in the clipboard, if the clipboard text is `HH:mm:ss` or `HHmmss` |
| T, or a click on the timer | Pause and copy the clock time |
| F, or the scissors button | Bbox capture: pause, click a camera, then draw a rectangle on its frame. The area is copied as a picture |
| Click or drag on the progress bar | Jump |

## Design

| Module | Content | Depends on the platform |
|---|---|---|
| `time`, `library`, `clock` | File name times, folder scan, camera timelines, the shared clock | No |
| `media` | Frame types and the FFmpeg decode backend | No (FFmpeg is portable) |
| `playback` | One decode thread for each shown camera | No |
| `render` | wgpu pipeline that draws NV12 and P010 frames | No (wgpu is portable) |
| `ui` | egui window: grid, control bar, dialogs (camera picker, go to time, bbox capture) | No (egui is portable) |
| `platform` | Hardware decoder choice, zero-copy frame import, app menu, folder dialog | Yes |

### Playback model

- There is one clock. It is a time of day that moves by `real time x speed`. It never waits for a camera.
- Each shown camera has a decode thread. The thread decodes a few frames ahead of the clock and puts them in a queue with their time of day. A "black" marker in the queue starts a gap.
- On each screen refresh, the UI thread takes the latest queue entry that is not after the clock time. The UI thread never decodes and never waits.
- A jump changes the clock epoch. Frames of an old epoch are not shown, so a tile is black until the exact frame for the new time is ready.
- The next chunk is opened on a helper thread 6 seconds of real time before the clock reaches it. Its first frame is decoded at that time.
- A decoder that is late skips to key frames. At 8x and 16x the decoder shows key frames only, if key frames come at least 2 times each real second.

### Performance

- Hardware decode through VideoToolbox. If it fails for a stream, that stream uses software decode with 4 threads.
- Zero copy: the decoder output (`CVPixelBuffer`, backed by an IOSurface) becomes a Metal texture with no pixel copy. A shader converts YUV to RGB.
- The window is drawn only when necessary: when a new video frame is due (not more than about 70 times each second), 4 times each second for the timer, and on input. A paused app uses no CPU.
- Hidden cameras have no decoder and no open file.

Measured on an Apple Silicon laptop, release build, 1x speed, window refreshing 70 times each second:

| Case | CPU (one core = 100%) | Memory |
|---|---|---|
| 5 cameras (four 1080p, one 720p), zero copy | 17% | 144 MB |
| 16 cameras 1080p, zero copy | 27% | 211 MB |
| 5 cameras, hardware decode with frame copy | 34% | 333 MB |
| 5 cameras, software decode | 67% | 428 MB |
| Paused | 0% | |

## Platform layer

All code that depends on the operating system is in `src/platform/`. The public functions are in `src/platform/mod.rs`:

| Function | macOS (`macos.rs`) | Fallback (`fallback.rs`) |
|---|---|---|
| `video_support` | VideoToolbox decode, IOSurface to Metal texture import | D3D11VA decode on Windows with frame copy. Software decode on other systems. |
| `install_app_menu` | Native menu bar with "Open Folder…" | No menu. The open button and Ctrl+O open a folder. |
| `pick_folder` | Native folder dialog | Native folder dialog |

To add Windows support with full performance:

1. Add `src/platform/windows.rs` with the same three functions and select it in `mod.rs`.
2. Implement `HwFrameImporter` for the D3D11 textures of the decoder. `macos.rs` shows the pattern: make a native texture, then wrap it with `create_texture_from_hal`.
3. Make the menu with `muda` (`init_for_hwnd`).
4. Add the `x11` and `wayland` features of `eframe` only if Linux is also a target.

Without these steps, Windows uses the fallback file. The fallback file was not built or tested on Windows.

## Icons

The control bar icons are SVG files in `assets/icons/`, from the Material Icons set (Apache License 2.0). See `assets/icons/README.md` for how to add an icon.

## Development tools

- `VIDEO_PLAYER_DECODE=software` or `VIDEO_PLAYER_DECODE=copy` turns off hardware decode or zero copy. Use it to compare the decode paths.
- `RUST_LOG=info` prints the decode setup. `RUST_LOG=debug` also shows FFmpeg warnings.
- `cargo build --release --features selftest` adds a script tool that sends keys and clicks to the real window, saves window pictures, and prints the frame time of each tile. See `src/ui/selftest.rs`.

## Limits

- `cargo run` links to the FFmpeg libraries of the system. Only the app bundle is self-contained.
- The app bundle has no custom icon.
- HDR video (BT.2020 PQ or HLG) is shown with no tone mapping.
