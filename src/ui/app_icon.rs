//! The app icon. The source is one SVG file: `assets/app-icon/app-icon.svg`.
//!
//! The app draws it at start for the Dock or the task bar. The bundle script
//! uses [`png`] through `examples/app_iconset.rs` to make the icon file of
//! the app bundle. So the two icons always come from the same picture.

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

const ICON_SVG: &str = include_str!("../../assets/app-icon/app-icon.svg");
/// Size of the icon that the app sets at start.
const WINDOW_ICON_SIZE: u32 = 512;

/// Draws the icon as a square picture with `size` pixels on each side.
fn render(size: u32) -> Option<Pixmap> {
    let tree = Tree::from_str(ICON_SVG, &Options::default()).ok()?;
    let mut pixmap = Pixmap::new(size, size)?;
    let scale = Transform::from_scale(size as f32 / tree.size().width(), size as f32 / tree.size().height());
    resvg::render(&tree, scale, &mut pixmap.as_mut());
    Some(pixmap)
}

/// The icon for the window system.
pub fn icon_data() -> Option<egui::IconData> {
    let pixmap = render(WINDOW_ICON_SIZE)?;
    // The window system wants colors that are not multiplied by alpha.
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    Some(egui::IconData { rgba, width: pixmap.width(), height: pixmap.height() })
}

/// The icon as a PNG file with `size` pixels on each side.
pub fn png(size: u32) -> Option<Vec<u8>> {
    render(size)?.encode_png().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(icon: &egui::IconData, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * icon.width + x) * 4) as usize;
        icon.rgba[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn icon_has_the_expected_picture() {
        let icon = icon_data().expect("the icon SVG is valid");
        assert_eq!((icon.width, icon.height), (512, 512));
        assert_eq!(icon.rgba.len(), 512 * 512 * 4);
        // The corners are outside the rounded square: transparent.
        assert_eq!(pixel(&icon, 2, 2)[3], 0);
        // The center is inside the white play symbol.
        assert_eq!(pixel(&icon, 256, 256), [255, 255, 255, 255]);
        // Near the top edge of the square: the blue background.
        let [r, g, b, a] = pixel(&icon, 256, 70);
        assert!(a == 255 && b > 200 && b > r && b > g, "not blue: {r} {g} {b} {a}");
    }

    #[test]
    fn png_has_the_requested_size() {
        let png = png(64).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // Width and height are in the IHDR chunk, big endian.
        assert_eq!(&png[16..24], &[0, 0, 0, 64, 0, 0, 0, 64]);
    }
}
