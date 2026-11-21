# Icons

SVG icons for the control bar. The files are not changed from the source.

| File | Icon | Use |
|---|---|---|
| `play_arrow.svg` | Material Icons, Filled, `play_arrow` | Play/pause button while paused |
| `pause.svg` | Material Icons, Outlined, `pause` | Play/pause button while playing |
| `content_cut.svg` | Material Icons, Outlined, `content_cut` | Bbox capture button |
| `photo_camera.svg` | Material Icons, Outlined, `photo_camera` | Cameras button |

Source: <https://github.com/google/material-design-icons> (folder `src/<category>/<icon>/<style>/24px.svg`, where the style folder is `materialiconsoutlined` for Outlined and `materialicons` for Filled).
The play icon uses the Filled style, because the Outlined play icon is an empty triangle and the pause icon is solid.
License: Apache License 2.0.

To add an icon: copy the 24px SVG file here, then add one `Icon` constant in `src/ui/controls.rs`.
The app draws the icons in white and gives them the color of the button text, so use icons with one color only.
