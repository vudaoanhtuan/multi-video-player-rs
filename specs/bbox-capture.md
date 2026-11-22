# Bbox capture

## Goal

Let the user copy a rectangle area of one camera frame to the clipboard as a picture.

## Depends on

- [synced-playback.md](synced-playback.md) for the clock and the frame that each tile shows.
- [controls-and-layout.md](controls-and-layout.md) for the control bar and the camera grid.

## Start

- The control bar has a button with a scissors icon. The F key does the same as the button.
- The button or the key does two things:
  - If the clock is playing, it pauses the clock.
  - It starts the camera choice.
- The button looks pressed during the camera choice.
- The button or the key during the camera choice stops the camera choice. The Escape key does the same.

## Camera choice

- Show a short instruction on the grid: click a camera, Escape cancels.
- The tile under the pointer gets a highlight border.
- A click on a tile selects that camera and opens the frame dialog.
- A click on a tile that shows black (no video at this time): show a message. The camera choice continues.
- The control bar and the other keys work during the camera choice. For example, the user can jump to a different time and then click a camera.
- If the clock is playing at the time of the click, pause it first.

## Frame dialog

- The dialog appears on top of the grid. It shows the camera id, the clock time, and the frame that the tile showed at the click.
- The frame has the shape that the grid uses. With the "16:9" video ratio setting, the dialog shows the stretched picture. With "None", it shows the shape of the video.
- The picture keeps this shape in the dialog. It is as large as the window permits. A small video is shown at not more than 2 times its size.
- The dialog has a "Close" button. The Escape key does the same. A click outside the dialog does not close it.
- The user draws a rectangle on the frame with a press, a drag, and a release.
  - During the drag, show the rectangle and its position and size in picture pixels.
  - At the release, copy the rectangle area to the clipboard as a picture. Show a "copied" text with the size.
  - The dialog stays open. The user can draw again. Each new rectangle replaces the clipboard content.
- The picture has the height of the video. The width follows from the shape: a 1440 x 1080 video gives a 1920 x 1080 picture with the "16:9" setting, and a 1440 x 1080 picture with "None".
- The copied area is cut from this picture, so it does not depend on the size of the dialog. With the "16:9" setting, the copied area is stretched in the same way as the picture on the screen.
- Positions and sizes are in pixels of this picture.
- The rectangle is limited to the frame. A drag that ends outside the frame gives a rectangle that stops at the frame edge.
- A rectangle smaller than 2 x 2 picture pixels is not a selection. Nothing is copied.
- While the dialog is open, the other keys (space, jump keys, digits, C, G, T, F) do nothing.

## Edge cases and errors

- The frame cannot be read: show a message and do not open the dialog.
- No camera is shown: the camera choice starts, but there is no tile to click. Escape stops it.

## Done when

- The scissors button while playing pauses playback and shows the instruction on the grid.
- The tile under the pointer has a highlight border during the camera choice.
- A click on a tile opens the dialog with the frame of that camera.
- A drawn rectangle of 400 x 200 picture pixels puts a picture of 400 x 200 pixels in the clipboard.
- With the "16:9" setting, the dialog for a 1440 x 1080 video shows a 1920 x 1080 picture.
- The picture shows the same content as the rectangle on the screen.
- Close and Escape close the dialog. Playback stays paused.
