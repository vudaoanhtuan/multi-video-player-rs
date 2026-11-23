# Controls and layout

## Goal

Show the cameras in a grid and give the user controls for play, time, speed, and camera choice.

## Depends on

- [folder-scan-and-timeline.md](folder-scan-and-timeline.md) for the camera list and the total time range.
- [synced-playback.md](synced-playback.md) for the clock, jump, and speed.

## Window layout

The window shows only two parts: the camera grid and the control bar. There is no side panel.

```
+----------------------------------------------------------+
|                                                          |
|                   camera grid (tiles)                    |
|                                                          |
+----------------------------------------------------------+
| play/pause | 21:30:05 | progress bar | speed | bbox | cameras | menu |
+----------------------------------------------------------+
```

- The app opens its window maximized: the window fills the screen that it opens on. This is not the full screen mode. The title bar, and the menu bar or the task bar of the system, stay visible.
- When the user restores the window, its size is 1280 x 800 points. The smallest window size is 640 x 360 points.

## Window title

- The app name is "Multi Video Player".
- The parts of the window title have a dot character (`·`) between them, with one space on each side.
- With no folder open, the window title is the app name and the app version, for example "Multi Video Player · 0.1.0".
- With a folder open, the title also shows the full path of the folder: "Multi Video Player · 0.1.0 · /path/to/folder".
- The path shows from the moment the user selects the folder, also during the scan and when the folder has no videos.
- "Close Folder" removes the path from the title.
- The version comes from the package version of the app.

## Camera grid

- One tile per shown camera, in camera id order.
- Use a grid that is close to square: `columns = round up(square root of shown camera count)`.
- All tiles have the same size. The grid fills the space above the control bar and changes with the window size.
- The shape of the video inside the tile follows the "video ratio" setting (see "Settings"):
  - "16:9": stretch each video to 16:9. A video that is not 16:9 gets wider or narrower. Nothing is cut.
  - "None": keep the shape of the video.
- Fill the unused tile area with black.
- If the "Camera name" setting is "Show", show the camera id as a small label in the top left corner of each tile. The default is no label (see "Settings").

## Control bar

The control bar is one row at the bottom of the window. From left to right:

| Item | Use |
|---|---|
| Play/pause button | Starts and stops the clock. |
| Timer | Shows the clock time. A click on it copies the time and pauses playback. |
| Progress bar | Shows the clock position in the total time range. Takes all spare width. |
| Speed menu | Sets the playback speed. |
| Bbox button | A button with a photo camera icon. Starts the bbox capture. See [bbox-capture.md](bbox-capture.md). |
| Cameras button | A button with a video camera icon and the count of shown cameras. Opens the camera picker dialog. |
| Menu button | A button with three vertical dots, at the right end. Opens a menu with "Open Folder…", "Close Folder", "Settings…", and "Help". |

## Play and pause

- The button changes between play and pause. The space key does the same.
- The button shows the action of a click: a play icon while paused, and a pause icon while playing.
- After a folder opens, the state is paused.
- If the clock is at the end of the range, play starts again from the start of the range.

## Timer

- Show the clock time as `HH:mm:ss`. Update it at least 4 times each second while playing.
- The user cannot edit the timer.
- A click on the timer, or the T key, does two things:
  - If the clock is playing, it pauses the clock.
  - It copies the clock time to the clipboard as `HH:mm:ss`, and shows a short "copied" message.
- The copied time is the time at which the clock stopped, so it is the time on the screen.

## Go to a time

- A typed time is valid in these forms: `HH:mm:ss`, `HH:mm`, `HHmmss`.
- A pasted time and a clipboard time are valid only in the full forms: `HH:mm:ss` with two digits in each part, or `HHmmss`. For example, `21:45` and `9:05:00` are not valid.
- Space at the two ends is ignored.

- The G key opens the "go to time" dialog. The dialog appears on top of the grid.
- The dialog has a text box that contains the current clock time, and "OK" and "Cancel" buttons.
  - OK starts the jump and closes the dialog. The Enter key does the same.
  - Cancel closes the dialog with no jump. The Escape key and a click outside the dialog do the same.
  - Text that is not a time, then OK: mark the text box in red, keep the dialog open, and do not change the clock.
- A paste while the dialog is open does not go into the text box.
  - The pasted text is a valid time: close the dialog and jump.
  - The pasted text is not a valid time: do nothing. The text box does not change.
- The H key reads the clipboard. If the clipboard text is a valid time, jump to it with no dialog. If not, nothing happens.
- Playback continues while the dialog is open.
- A time before the range start jumps to the range start. A time after the range end jumps to the range end.
- If the range passes midnight, a time before the range start time of day means the next day.

## Progress bar

- The left end is the start of the total time range. The right end is the end of the total time range.
- Show the start time and the end time as `HH:mm:ss` labels at the two ends.
- A marker shows the clock position.
- A click on the bar jumps to that time.
- A drag of the marker jumps continuously. Limit tile updates during the drag to about 10 each second.
- When the pointer is over the bar, show the time at that position.
- Jump keys jump back and forward from the clock time. Each row has one key for back and one key for forward:

| Keys (back / forward) | Jump | Jump with Shift |
|---|---|---|
| Left arrow / Right arrow | 2 seconds | 1 minute |
| `,` / `.` | 5 seconds | 2 minutes |
| `;` / `'` | 10 seconds | 5 minutes |
| `[` / `]` | 30 seconds | 10 minutes |

- A jump key works in play and in pause. The play state does not change.
- A jump to a time before the start of the range goes to the start. A jump to a time after the end of the range goes to the end.
- A key that the user holds down repeats the jump.
- The keys are the keys of a US keyboard.

## Playback speed

- A menu with all whole speeds from 1x to 20x: 1x, 2x, 3x, and so on to 20x.
- The open menu shows all speeds if the window is high enough. In a low window, the menu scrolls.
- The digit keys set the speed. The menu shows the new speed.

| Keys | Speed |
|---|---|
| 1 to 9 | 1x to 9x: the key 1 sets 1x, the key 2 sets 2x, and so on |
| 0 | 10x |
| Shift + 1 to 9 | 11x to 19x: Shift + 1 sets 11x, Shift + 2 sets 12x, and so on |
| Shift + 0 | 20x |

- The digit keys are the keys of a US keyboard.
- The default is 1x after each folder open.
- A speed change applies immediately, in play or in pause.

## Camera picker dialog

- The cameras button or the C key opens the dialog. The dialog appears on top of the grid.
- The dialog lists all cameras in the folder in camera id order, each with a checkbox.
- A checkbox is on when its camera is shown in the grid at the time the dialog opens.
- When a folder opens, all cameras are shown, so all checkboxes are on.
- The dialog has "select all" and "select none" buttons.
- The dialog has "OK" and "Cancel" buttons.
  - OK applies the choice to the grid and closes the dialog. The Enter key does the same.
  - Cancel closes the dialog with no change. The Escape key does the same.
- Playback continues while the dialog is open.
- If the list is longer than the dialog, the list scrolls.
- The cameras button shows a video camera icon and the count, for example "3/5". With no folder open, it shows the icon only.
- The control bar is low, so the camera grid gets the most space. The buttons, the timer, and the speed menu have the same height, with a small space above and below them.
- The bbox button and the cameras button have no text label. Each button shows its name and its key when the pointer is over it.
- The icons are SVG files from the Material Icons set: `play_arrow` and `pause` for the play/pause button, `photo_camera` for the bbox button, `videocam` for the cameras button, and `more_vert` for the menu button. Keep the files in `assets/icons/`.
- An icon has the color of the button text in each button state (normal, pointer over, pressed, disabled).
- If no camera is selected, the grid is empty and the clock still works.

## Menu button

- A click on the menu button opens a small menu next to the button. A click outside the menu closes it.
- "Open Folder…" shows the folder dialog. It does the same as the other ways to open a folder. See [folder-scan-and-timeline.md](folder-scan-and-timeline.md).
- "Close Folder" stops playback, closes all video files, and drops the library. The window then shows the "open a folder" message, and the control bar is disabled.
- "Close Folder" is disabled when no folder is open.
- The menu button is always enabled, also when the control bar is disabled. The user needs it to open a folder.

## Settings

- The menu of the menu button has a "Settings…" entry. It opens the settings dialog. The dialog appears on top of the grid.
- The settings dialog is available in each state, also with no folder open.
- The dialog shows four settings: "Video ratio", "Camera name", "Decoder", and "Key frames only from". "Video ratio", "Camera name", and "Decoder" each have a group of options. Exactly one option of a group is selected. "Key frames only from" has a number box.
- When the dialog opens, it shows the settings that are in use.
- A change in the dialog is not in use immediately. Playback and the grid do not change while the user changes values in the dialog.
- The dialog has "Cancel" and "Apply" buttons.
  - Apply puts all values of the dialog in use and closes the dialog. The Enter key does the same.
  - Cancel closes the dialog with no change. The Escape key and a click outside the dialog do the same.
- Playback continues while the dialog is open. The hotkeys do nothing while the dialog is open.
- The app saves the settings, and starts with the saved settings. See "Saved settings".
- The settings stay when the user closes a folder or opens a different folder.

### Saved settings

- Apply saves all settings in the settings file, if a value changed. Cancel does not change the file.
- At each start, the app reads the settings file and uses the saved settings.
- If there is no settings file, each setting has its default. This is the condition at the first start of the app.
- The settings file is `settings.json` in the folder `Multi Video Player`, in the folder that the system gives for the settings of apps:
  - macOS: `~/Library/Application Support/Multi Video Player/settings.json`
  - Windows: `%APPDATA%\Multi Video Player\settings.json`
- The app makes the folder and the file at the first save.
- The file is JSON text: one object with one field for each setting.

| Field | Setting | Value |
|---|---|---|
| `video_ratio` | Video ratio | `"None"` or `"Wide16x9"` |
| `show_camera_name` | Camera name | `false` for "Hide", `true` for "Show" |
| `decoder` | Decoder | `"Hardware"` or `"Software"` |
| `key_only_min_speed` | Key frames only from | A whole number, 1 or more |

- A setting that is an enum in the code is saved as the name of the enum value in the code, not as the label that the dialog shows. For example, the option "16:9" is saved as `"Wide16x9"`.
- Example of a file with the defaults:

```json
{
  "video_ratio": "Wide16x9",
  "show_camera_name": false,
  "decoder": "Hardware",
  "key_only_min_speed": 4
}
```

- A setting that the file does not have gets its default. A setting with a value that is not valid gets its default, and the other settings keep their saved values. A `key_only_min_speed` of 0 becomes 1.
- If the file is not a JSON object, or the app cannot read it, each setting has its default. The app does not show a message, and it does not change the file until the next Apply.
- The app ignores fields that it does not know.
- If the app cannot write the file, it shows a short message. The new settings are in use until the app closes.
- If the saved decoder is "Hardware" and the computer has no hardware decoder that the app can use, the app starts with "Software".
- The `MULTI_VIDEO_PLAYER_SETTINGS` environment variable replaces the path of the settings file. It is for tests.

### Video ratio

- "Video ratio" has two options: "None" and "16:9".
- The default is "16:9".
- After Apply, a change applies immediately to all tiles, in play or in pause.
- The setting does not change the decode, the clock, or the frame that a tile shows. The stretch is done when the frame is drawn.

### Camera name

- "Camera name" has two options: "Hide" and "Show".
- The default is "Hide".
- "Show": each tile shows the camera id as a small label in its top left corner, on top of the video. A black tile also shows the label.
- "Hide": the tiles show no label.
- After Apply, a change applies immediately to all tiles, in play or in pause.
- The setting does not change the decode, the clock, or the frame that a tile shows.
- The setting does not change the camera picker dialog or the bbox capture. The bbox capture dialog always shows the camera id, and the copied picture never has the label.

### Decoder

- "Decoder" has two options: "Hardware" and "Software".
  - "Hardware": the video decoder of the GPU decodes the videos. If it cannot decode a video, the CPU decodes that video.
  - "Software": the CPU decodes all videos.
- The default is "Hardware".
- If the computer has no hardware decoder that the app can use, "Software" is selected and "Hardware" is disabled.
- After Apply, a change applies immediately to all shown cameras, in play or in pause. Each shown camera opens its files again with the new decoder and joins at the clock time. A tile is black until its frame is ready.
- A change does not change the clock, the play state, the speed, or the camera choice.
- A camera that the user shows later, and a folder that the user opens later, use the selected decoder.

### Key frames only from

- "Key frames only from" is a number box. Its value is a whole number that is more than 0. There is no highest value. The box shows the value with an "x", for example "4x".
- The default is 4.
- The dialog shows one short sentence below the box: "From this speed, the app shows key frames only."
- To change the value, the user clicks the box and types a number. A drag on the box to the left or to the right also changes the value.
- The Enter key in the box does the same as Apply, with the typed number.
- A typed number below 1 becomes 1. Text that is not a number does not change the value.
- With a value above the highest speed, a camera shows all frames at each speed, unless it is late.
- At the speed of the value and at each higher speed, a camera shows key frames only, if its video has key frames often enough. See "Speed" in [synced-playback.md](synced-playback.md).
- Below that speed, a camera shows all frames, unless it is late.
- After Apply, a change applies immediately to all shown cameras, in play or in pause. The decode does not start again, and the tiles do not go black.
- A change does not change the clock, the play state, the speed, or the camera choice.
- A camera that the user shows later, and a folder that the user opens later, use the value.

## Help

- The menu of the menu button has a "Help" entry. It opens the user manual in the web browser that the user set as the default.
- The manual explains each function of the app and has a table of all hotkeys.
- The manual shows the version of the app below its title, for example "Version 0.1.0". The version comes from the package version of the app. The manual file has the text `{{version}}` at that place, and the app replaces it when it writes the copy.
- The manual is one HTML file: `assets/help/manual.html`. It is part of the executable. To change the manual, change this file only.
- The app does not show HTML itself. It writes a copy of the manual to the temporary folder of the system and asks the system to open that file.
- Help is available in each state, also with no folder open.
- Help does not change playback.
- If the file cannot be written or opened, show a short message.
- A key name in the manual uses the `kbd` tag.
- When a hotkey or a function changes, change the manual in the same change.

## Audio

- There is no audio and no volume control.

## Edge cases

- One camera only: the tile fills the grid.
- Many cameras (for example 16): tiles get small but stay readable. Do not add scroll bars to the grid.
- No folder open: show an "open a folder" message with an open button in the grid area, and disable the control bar. The menu button stays enabled.
- While a text box or a dialog has the keyboard focus, the space, C, G, T, F, digit, and jump keys do not control playback.

## Done when

- The window shows only the camera grid and the control bar. There is no camera list in the window.
- The app opens with a maximized window.
- A folder with 5 cameras opens with 5 tiles in a 3 column grid.
- The camera picker dialog shows 5 checkboxes, all on, after the folder opens.
- Two checkboxes set to off plus OK leaves 3 tiles, and the tiles fill the grid space.
- Checkbox changes plus Cancel leaves the grid unchanged.
- A click at the middle of the progress bar sets the clock to the middle of the total time range.
- The G key, the text `21:45:10`, and Enter sets the clock to 21:45:10 and all tiles update.
- A paste of `214510` in the "go to time" dialog closes the dialog and sets the clock to 21:45:10.
- A paste of text that is not a time leaves the dialog open and the text box unchanged.
- The H key with `21:45:10` in the clipboard sets the clock to 21:45:10. With other text in the clipboard, the clock does not change.
- A paste of `21:45` in the "go to time" dialog does nothing. The typed text `21:45` and Enter sets the clock to 21:45:00.
- A click on the timer while playing pauses playback and puts the clock time in the clipboard. The T key does the same.
- The speed menu shows the 20 speeds from 1x to 20x.
- The 3 key sets the speed to 3x, in play and in pause. The speed menu then shows 3x.
- The 0 key sets 10x. Shift + 3 sets 13x. Shift + 0 sets 20x.
- A digit key typed in the "go to time" dialog does not change the speed.
- With the default setting, a 4:3 video fills a 16:9 area in its tile. With "None", the same video is shown as 4:3.
- "Settings…" in the menu opens the settings dialog, with and without an open folder. "16:9" and "Hardware" are selected after the first start of the app, when there is no settings file.
- A click on "Software" in the settings dialog while playing does not change the decode. A click on Apply then closes the dialog. The clock continues, and all tiles show frames again after a moment.
- With no settings file, the tiles show no camera id label after the app starts. A click on "Show" for "Camera name" and then on Apply shows the camera id in the top left corner of each tile.
- A click on "None" and then on Cancel leaves the video ratio at "16:9". The dialog shows "16:9" when it opens again.
- A click on the "Key frames only from" box, the text `6`, and Enter closes the dialog and sets the value to 6. The text `40` and Apply sets the value to 40. The text `0` and Apply sets the value to 1.
- With the default of 4 and a speed of 4x, a video with one key frame each second changes its picture 4 times each real second. With "Key frames only from" at 8, the same video at 4x shows the frames between the key frames.
- A click on "None", on "Show", and on Apply writes the settings file with `"video_ratio": "None"` and `"show_camera_name": true`. After the app closes and starts again, the tiles show the camera id, the videos keep their shape, and the dialog shows "None" and "Show".
- With `"decoder": "Software"` in the settings file, the dialog shows "Software" after the app starts.
- With `"video_ratio": "16:9"` and `"key_only_min_speed": 8` in the settings file, the app starts with the video ratio "16:9" (the default, because the value is not an enum name) and with "Key frames only from" at 8.
- A click on "None" and then on Cancel does not make or change the settings file.
- The Escape key closes the settings dialog with no change. The space key does not change the play state while the dialog is open.
- "Help" in the menu opens the manual in the web browser, with and without an open folder. The manual lists each hotkey of the app.
- The window title shows the app name, the version, and the path of the open folder. After "Close Folder", it shows the name and the version only.
- "Close Folder" in the menu while playing shows the "open a folder" message and disables the control bar. The menu button still works.
- "Open Folder…" in the menu, with no folder open, opens a folder. All cameras are shown and the state is paused.
- A speed of 4x moves the shown time 4 seconds for each real second.
- The time labels at the two ends of the bar equal the earliest start and the latest end in the folder.
