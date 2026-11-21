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

## Camera grid

- One tile per shown camera, in camera id order.
- Use a grid that is close to square: `columns = round up(square root of shown camera count)`.
- All tiles have the same size. The grid fills the space above the control bar and changes with the window size.
- The shape of the video inside the tile follows the "video ratio" setting (see "Settings"):
  - "16:9": stretch each video to 16:9. A video that is not 16:9 gets wider or narrower. Nothing is cut.
  - "None": keep the shape of the video.
- Fill the unused tile area with black.
- Show the camera id as a small label in a corner of each tile.

## Control bar

The control bar is one row at the bottom of the window. From left to right:

| Item | Use |
|---|---|
| Play/pause button | Starts and stops the clock. |
| Timer | Shows the clock time. A click on it copies the time and pauses playback. |
| Progress bar | Shows the clock position in the total time range. Takes all spare width. |
| Speed menu | Sets the playback speed. |
| Bbox button | A button with a scissors icon. Starts the bbox capture. See [bbox-capture.md](bbox-capture.md). |
| Cameras button | A button with a camera icon and the count of shown cameras. Opens the camera picker dialog. |
| Menu button | A button with three vertical dots, at the right end. Opens a menu with "Open Folder…", "Close Folder", and "Settings". |

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
- Shift + G reads the clipboard. If the clipboard text is a valid time, jump to it with no dialog. If not, nothing happens.
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
- Left and right arrow keys jump 10 seconds back and forward.

## Playback speed

- A menu with these speeds: 1x, 2x, 4x, 8x, 16x.
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
- The cameras button shows a camera icon and the count, for example "3/5". With no folder open, it shows the icon only.
- The bbox button and the cameras button have no text label. Each button shows its name and its key when the pointer is over it.
- The icons are SVG files from the Material Icons set: `play_arrow` and `pause` for the play/pause button, `content_cut` for the bbox button, `photo_camera` for the cameras button, and `more_vert` for the menu button. Keep the files in `assets/icons/`.
- An icon has the color of the button text in each button state (normal, pointer over, pressed, disabled).
- If no camera is selected, the grid is empty and the clock still works.

## Menu button

- A click on the menu button opens a small menu next to the button. A click outside the menu closes it.
- "Open Folder…" shows the folder dialog. It does the same as the other ways to open a folder. See [folder-scan-and-timeline.md](folder-scan-and-timeline.md).
- "Close Folder" stops playback, closes all video files, and drops the library. The window then shows the "open a folder" message, and the control bar is disabled.
- "Close Folder" is disabled when no folder is open.
- The menu button is always enabled, also when the control bar is disabled. The user needs it to open a folder.

## Settings

- The menu of the menu button has a "Settings" entry that opens a second menu.
- "Video ratio" has two options. Exactly one is selected: "None" and "16:9".
- The default is "16:9" at each start of the app.
- A change applies immediately to all tiles, in play or in pause.
- The setting does not change the decode, the clock, or the frame that a tile shows. The stretch is done when the frame is drawn.
- The setting stays when the user closes a folder or opens a different folder.

## Audio

- There is no audio and no volume control.

## Edge cases

- One camera only: the tile fills the grid.
- Many cameras (for example 16): tiles get small but stay readable. Do not add scroll bars to the grid.
- No folder open: show an "open a folder" message with an open button in the grid area, and disable the control bar. The menu button stays enabled.
- While a text box or a dialog has the keyboard focus, the space, C, G, T, F, and arrow keys do not control playback.

## Done when

- The window shows only the camera grid and the control bar. There is no camera list in the window.
- A folder with 5 cameras opens with 5 tiles in a 3 column grid.
- The camera picker dialog shows 5 checkboxes, all on, after the folder opens.
- Two checkboxes set to off plus OK leaves 3 tiles, and the tiles fill the grid space.
- Checkbox changes plus Cancel leaves the grid unchanged.
- A click at the middle of the progress bar sets the clock to the middle of the total time range.
- The G key, the text `21:45:10`, and Enter sets the clock to 21:45:10 and all tiles update.
- A paste of `214510` in the "go to time" dialog closes the dialog and sets the clock to 21:45:10.
- A paste of text that is not a time leaves the dialog open and the text box unchanged.
- Shift + G with `21:45:10` in the clipboard sets the clock to 21:45:10. With other text in the clipboard, the clock does not change.
- A paste of `21:45` in the "go to time" dialog does nothing. The typed text `21:45` and Enter sets the clock to 21:45:00.
- A click on the timer while playing pauses playback and puts the clock time in the clipboard. The T key does the same.
- The speed menu shows 1x, 2x, 4x, 8x, 16x.
- With the default setting, a 4:3 video fills a 16:9 area in its tile. With "None", the same video is shown as 4:3.
- "Close Folder" in the menu while playing shows the "open a folder" message and disables the control bar. The menu button still works.
- "Open Folder…" in the menu, with no folder open, opens a folder. All cameras are shown and the state is paused.
- A speed of 4x moves the shown time 4 seconds for each real second.
- The time labels at the two ends of the bar equal the earliest start and the latest end in the folder.
