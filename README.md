# Multi Video Player

A native desktop app that opens a folder of camera recordings and plays all cameras at the same time of day.
The behavior is defined in [specs/](specs/README.md).

## Requirements

macOS:

- The Xcode command line tools (`xcode-select --install`). They give the C compiler, `make`, and `git`.
- Rust 1.95 or later.
- An Intel Mac also needs `nasm` to build FFmpeg.

Windows 10 or later (64 bit):

- Visual Studio or the Visual Studio Build Tools, with the "Desktop development with C++" workload.
- Rust 1.95 or later, with the `x86_64-pc-windows-msvc` toolchain.
- Git for Windows.
- MSYS2 in `C:\msys64` (`winget install MSYS2.MSYS2`). FFmpeg has a Unix build system, which runs in the shell of MSYS2. The build script installs the MSYS2 packages `make`, `nasm`, and `diffutils` if they are missing. If MSYS2 is in a different folder, set `MSYS2_ROOT` to that folder.
- LLVM (`winget install LLVM.LLVM`). The Rust bindings of FFmpeg need `libclang` at build time. If LLVM is in a different folder, set `LIBCLANG_PATH` to the folder of `libclang.dll`.

The project builds its own FFmpeg. It does not use an FFmpeg of the system, for example from Homebrew.
Other systems use the fallback platform layer (see "Platform layer").

## Build and run

```sh
./scripts/build-ffmpeg.sh                # one time: builds FFmpeg in ffmpeg-build/ (about 1 minute)
cargo run --release                      # opens with no folder
cargo run --release -- /path/to/folder   # opens a folder
cargo test                               # unit tests and end-to-end playback tests
```

On Windows, use PowerShell. Only the first command is different:

```powershell
.\scripts\build-ffmpeg.ps1                # one time: builds FFmpeg in ffmpeg-build\ (about 15 minutes)
cargo run --release -- C:\path\to\folder
cargo test
```

If PowerShell refuses to run a script of the project, start it with `powershell -ExecutionPolicy Bypass -File scripts\build-ffmpeg.ps1`.

If `cargo` stops with `failed to run custom build command for ffmpeg-sys-next` and `'libavutil/avutil.h' file not found`, the FFmpeg of the project is not built. Run the FFmpeg build script.
If `cargo` stops on Windows with `Unable to find libclang`, install LLVM or set `LIBCLANG_PATH` (see "Requirements").

## FFmpeg

The FFmpeg build script (`scripts/build-ffmpeg.sh` on macOS, `scripts\build-ffmpeg.ps1` on Windows) downloads the FFmpeg source (version 7.1.5, pinned to one commit) and builds two things in `ffmpeg-build/`:

| Result | Content | Use |
|---|---|---|
| `ffmpeg-build/prefix/` | Static libraries and headers | `cargo run`, `cargo build`, `cargo test`, and the app bundle link to them |
| `ffmpeg-build/tools/bin/ffmpeg` (`ffmpeg.exe` on Windows) | A small `ffmpeg` command with a test picture source, the H.264 encoder of the system, and the MKV writer | The end-to-end tests make their test videos with it. It is not part of the app |

- The two scripts are independent. The Windows script does not use the shell script. FFmpeg has a Unix build system (a configure script and `make`), so the Windows script runs these two commands in the shell of MSYS2, with the Microsoft C compiler. The libraries then fit the Rust `msvc` toolchain.
- `.cargo/config.toml` points cargo to `ffmpeg-build/prefix/` (`FFMPEG_DIR`). All cargo commands link FFmpeg statically, so the executable uses only libraries that are part of the system.
- The libraries hold the MKV reader and these video decoders: H.264, HEVC, MJPEG, MPEG-4, MPEG-2, VP8, VP9, AV1. Hardware decode is VideoToolbox on macOS and Direct3D 11 on Windows. To add a format, change the decoder list in the two scripts (`DECODERS`, `$decoders`) and run the script again.
- The build for Windows has no zlib, because Windows has no zlib library. The app cannot read an MKV file with zlib-compressed tracks on Windows. Camera recordings do not usually use this compression.
- The script does nothing when its results are up to date. `ffmpeg-build/` is not in git, and `cargo clean` does not delete it.
- To use an FFmpeg that you built in a different folder, set `FFMPEG_DIR` in the shell. It must be a static build with the same parts, and with no external libraries.
- This FFmpeg build has the LGPL license. If you distribute the app, obey the LGPL rules for static linking.

## App bundle

### macOS

```sh
./scripts/bundle-macos.sh
open "dist/Multi Video Player.app"
```

The script makes `dist/Multi Video Player.app`. The app is one executable of about 18 MB with FFmpeg inside. You can move it to `/Applications` and open it with a double-click.

- The script runs `scripts/build-ffmpeg.sh` first, then makes a normal release build and packs it.
- The app runs on the CPU type of the Mac that built it (Apple Silicon or Intel).
- The signature is a local signature. To give the app to other persons, sign it with a Developer ID and notarize it. If you do not, macOS blocks the app on their Macs.

### Windows

```powershell
.\scripts\bundle-windows.ps1
& "dist\Multi Video Player.exe"
```

The script makes `dist\Multi Video Player.exe`. The app is one executable of about 22 MB with FFmpeg and the C runtime library inside. It needs no installer and no other file. You can copy it to a different computer with Windows 10 or later (64 bit) and open it with a double-click.

- The script runs `scripts\build-ffmpeg.ps1` first, then makes a normal release build and copies it.
- The script stops if the executable needs a library that is not part of Windows.
- A release build is a window program, so no console window opens with it. If you start it from a terminal, the log output goes to that terminal.
- The executable has no signature. Windows SmartScreen can show a warning on other computers. To prevent the warning, sign the executable with a code signing certificate.

## Controls

| Input | Result |
|---|---|
| Space | Play or pause |
| Left / Right arrow | Jump 10 seconds back / forward |
| C, or the camera button | Open the camera picker |
| Cmd+O (Ctrl+O on Windows) | Open a folder |
| Menu button (three dots) | "Open Folder…", "Close Folder", "Settings" (video ratio: "16:9" stretches each video to 16:9, "None" keeps the shape of the video), and "Help" (opens the user manual with all hotkeys in the web browser) |
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

- Hardware decode through VideoToolbox (macOS) or Direct3D 11 (Windows). If it fails for a stream, that stream uses software decode with 4 threads.
- Zero copy on macOS: the decoder output (`CVPixelBuffer`, backed by an IOSurface) becomes a Metal texture with no pixel copy. A shader converts YUV to RGB.
- Zero copy on Windows: the decode thread copies each picture on the GPU from the decoder (Direct3D 11) to a shared texture (NV12 or P010). The renderer (Direct3D 12) opens the same texture, and the shader reads its two planes directly. The pixels do not go through normal memory.
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

Measured on a Windows 10 desktop computer (Intel Core i5-10400, NVIDIA GeForce GTX 1070), with the same conditions:

| Case | CPU (one core = 100%) | Memory |
|---|---|---|
| 4 cameras (three 1080p, one 720p), zero copy | 35% | 241 MB |
| 16 cameras 1080p, zero copy | 54% | 440 MB |
| 4 cameras, hardware decode with frame copy | 60% | 314 MB |
| 16 cameras 1080p, hardware decode with frame copy | 183% | 906 MB |
| 4 cameras, software decode | 90% | 361 MB |

## Platform layer

All code that depends on the operating system is in `src/platform/`. The public functions are in `src/platform/mod.rs`:

| Function | macOS (`macos.rs`) | Windows (`windows.rs`) | Fallback (`fallback.rs`) |
|---|---|---|---|
| `video_support` | VideoToolbox decode, IOSurface to Metal texture import | Direct3D 11 decode, and textures that the decoder device shares with the Direct3D 12 device of the renderer. If sharing is not possible: Direct3D 11 decode with frame copy | Software decode |
| `install_app_menu` | Native menu bar with "Open Folder…" | No menu. The menu button, the open button, and Ctrl+O open a folder | No menu |
| `open_in_browser` | `open` command | Windows shell (`ShellExecuteW`) | `xdg-open` command |
| `pick_folder` | Native folder dialog | Native folder dialog | Native folder dialog |

Notes for Windows:

- The renderer always uses Direct3D 12 (`gpu_backends`). The `WGPU_BACKEND` environment variable can select a different backend, for example `vulkan`. The app then uses Direct3D 11 decode with frame copy.
- The decoder has its own Direct3D 11 device, on the GPU of the renderer. The renderer cannot read the textures of the decoder, and the decoder has only a small fixed number of them. So the decode thread copies each picture on the GPU to a texture from a pool of shared textures (`HwDecode::export`), and waits until the GPU has completed the copy. The texture goes back to the pool when the frame is dropped.
- FFmpeg can also decode with Direct3D 12 on the device of the renderer, which needs no copy on the GPU. The app does not use this. In a test with FFmpeg 7.1, an HEVC stream that this decoder could not decode made the device fail, and the app stopped. With two devices, a decoder problem cannot stop the renderer.
- A shared texture has the size of the picture, made even. The texture of the decoder can be larger, for example 1920 x 1088 for a picture of 1920 x 1080. `ImportedSurface::visible` tells the renderer which part of a texture holds the picture, and the shader does not read outside it.

To add Linux: add the `x11` and `wayland` features of `eframe`, and a platform file if hardware decode is necessary. The fallback file was not built or tested.

## User manual

The "Help" entry of the menu button opens `assets/help/manual.html` in the web browser of the system.
The file is part of the executable. At each use, the app writes a copy to the temporary folder of the system and opens that copy.
The copy shows the app version below the title. The version comes from `version` in `Cargo.toml`: the app replaces the text `{{version}}` in the manual.
A unit test checks that the manual lists each hotkey. When you add a hotkey, add it to the manual and to that test.

## Icons

The control bar icons are SVG files in `assets/icons/`, from the Material Icons set (Apache License 2.0). See `assets/icons/README.md` for how to add an icon.

The app icon is one SVG file: `assets/app-icon/app-icon.svg`. To change the icon, change this file only.
- The app draws it at start for the Dock or the task bar (`src/ui/app_icon.rs`).
- `scripts/bundle-macos.sh` draws it at each size that macOS uses and puts the result in the app bundle as `AppIcon.icns`. Finder shows this file.
- On Windows, `build.rs` draws it at each size that Windows uses and puts the result in the executable. The Explorer shows it.

## Development tools

- `MULTI_VIDEO_PLAYER_DECODE=software` or `MULTI_VIDEO_PLAYER_DECODE=copy` turns off hardware decode or zero copy. Use it to compare the decode paths.
- `RUST_LOG=info` prints the decode setup. `RUST_LOG=debug` also shows FFmpeg warnings.
- `cargo build --release --features selftest` adds a script tool that sends keys and clicks to the real window, saves window pictures, and prints the frame time of each tile. See `src/ui/selftest.rs`.

## Limits

- HDR video (BT.2020 PQ or HLG) is shown with no tone mapping.
- Windows: after a jump, FFmpeg makes the hardware decoder of each H.264 stream again, and all decoders work at the same time to reach the clock. The GPU has one video decoder for all streams. With 4 cameras the tiles are black for about 0.1 seconds after a jump during playback. With 16 cameras of 1080p on the test computer, they are black for 1 to 2 seconds.
- Windows: zero copy of 10 bit video (P010) was not tested with a real 10 bit recording.
