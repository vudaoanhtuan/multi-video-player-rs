# Multi-camera video player: specs

A desktop app that opens a folder of camera recordings and plays all cameras at the same time.
All cameras show the same time of day, taken from the file names.

## Specs

Build in this order.

1. [folder-scan-and-timeline.md](folder-scan-and-timeline.md): read the folder, group files by camera, build the timeline.
2. [synced-playback.md](synced-playback.md): one shared clock, black screen in gaps, smooth change between files.
3. [controls-and-layout.md](controls-and-layout.md): camera grid, control bar (timer, progress bar, speed), camera picker dialog.
4. [bbox-capture.md](bbox-capture.md): copy a rectangle area of one camera frame to the clipboard.

## Words used in all specs

| Word | Meaning |
|---|---|
| Chunk | One video file. Usually about 30 minutes long. |
| Camera timeline | All chunks of one camera, in time order, with gaps between them. |
| Gap | A time range where a camera has no chunk. |
| Clock | The one shared playback time. It is a time of day, for example 21:30:05. |
| Tile | The area of the window that shows one camera. |

## Rules for all specs

- The project folder is empty. Build a desktop app in Rust (assumed from the project name `video-player-rust`).
- The builder chooses the window and video decode libraries. The app must run on macOS.
- The app never plays audio. Do not decode audio tracks.
- The app only reads video files. It never changes, moves, or deletes them.
- File name times are local times. Do not convert time zones.
- Keep the full date and time inside the app. Show only the time (`HH:mm:ss`) to the user.
