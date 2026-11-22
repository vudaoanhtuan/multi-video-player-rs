# Folder scan and timeline

## Goal

Read a folder of video files and turn it into one camera timeline per camera, plus one total time range for the progress bar.

## In scope

- Open a folder from a menu item in the app menu (on a system that has an app menu), the "Open Folder…" entry of the control bar menu, the open button on the empty window, or a command line argument.
- Read file names and video lengths.
- Build the camera timelines and the total time range.

## Out of scope

- Folders inside the chosen folder. Only read files directly in the chosen folder.
- Watching the folder for new files after it is open.

## File name format

`<camera-id>_<start-time>.mkv`

- `start-time` has the format `yyyyMMdd'T'HHmmss`. Example: `20261019T213000` is 19 October 2026, 21:30:00.
- `camera-id` is all text before the last underscore. It can contain dashes and underscores.
- Example: `floor-1-camera-2_20261019T213000.mkv` gives camera `floor-1-camera-2`, start 21:30:00.

## Behavior

1. List the files in the folder. Keep only files that match the file name format.
2. For each file, read the video length from the file. Do not decode the full file.
3. Make one chunk per file: `start` from the file name, `end = start + length`.
4. Group chunks by camera id. Sort each group by start time.
5. Make the total time range: the earliest chunk start and the latest chunk end across all cameras.
6. Run the scan in the background. Show a "loading" message until it is done.
7. When the scan is done, show all cameras, set the clock to the start of the total time range, and stay paused.

The total time range uses all cameras in the folder, not only the cameras shown.
It does not change when the user hides or shows a camera.

## Data

```
Chunk { camera id, file path, start, end }
Camera timeline { camera id, chunks sorted by start }
Library { camera timelines sorted by camera id, range start, range end }
```

Lookup needed by playback:

```
find(camera, time) ->
  the chunk where start <= time < end, or
  "gap", with the next chunk of that camera if there is one
```

## Edge cases and errors

- File name does not match the format: skip the file. Show the number of skipped files after the scan.
- File cannot be opened or has no video track: skip it and count it as skipped.
- File has no stored length: use the start of the next chunk of the same camera as the end. For the last chunk, use 30 minutes.
- Chunk end is later than the start of the next chunk of the same camera: set the chunk end to that next start.
- Two files with the same camera id and start time: keep the first by file name order, skip the other.
- A chunk passes midnight: this works because the app keeps the full date and time.
- No matching files in the folder: show a "no videos found" message. The app has no library and the controls stay disabled.
- The user opens a second folder: stop playback, drop the old library, and scan the new folder.
- The user closes the folder ("Close Folder" in the control bar menu): stop playback, close all files, and drop the library. The app has no library, as at the start with no folder.

## Done when

- A folder with files from 3 cameras gives 3 camera timelines with the correct chunk count for each camera.
- The total time range equals the earliest start and the latest end in the folder.
- Files with bad names are skipped and counted, and the app does not stop.
- The window stays responsive during the scan of a folder with 500 files.
