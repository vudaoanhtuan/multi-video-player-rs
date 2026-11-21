//! Bbox capture: pick a camera, then draw a rectangle on its frame. The
//! rectangle area is copied to the clipboard as a picture.
//! See `specs/bbox-capture.md`.

use std::sync::Arc;

use egui::{Color32, Context, CursorIcon, Key, Pos2, Rect, RichText, Sense, Stroke, StrokeKind};

use super::session::Session;
use crate::time::Timestamp;

const ACCENT: Color32 = Color32::from_rgb(76, 148, 255);
/// A drag smaller than this, in picture pixels, is not a selection.
const MIN_SELECTION: u32 = 2;
/// A small video is shown larger, but not more than this factor.
const MAX_ZOOM: f32 = 2.0;

/// A rectangle in pixels of the picture in the dialog. The origin is the top
/// left corner. With the 16:9 ratio setting, the picture is the stretched video.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    /// The rectangle of whole pixels that covers the area between two points
    /// of a picture. The points can be outside the picture.
    pub fn from_points(a: Pos2, b: Pos2, picture: [usize; 2]) -> Self {
        let (w, h) = (picture[0] as f32, picture[1] as f32);
        let x0 = a.x.min(b.x).clamp(0.0, w).floor() as u32;
        let y0 = a.y.min(b.y).clamp(0.0, h).floor() as u32;
        let x1 = a.x.max(b.x).clamp(0.0, w).ceil() as u32;
        let y1 = a.y.max(b.y).clamp(0.0, h).ceil() as u32;
        Self { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
    }

    fn is_selection(&self) -> bool {
        self.width >= MIN_SELECTION && self.height >= MIN_SELECTION
    }
}

/// The dialog that shows one frame.
pub struct BboxDialog {
    pub camera_id: Arc<str>,
    pub time: Timestamp,
    /// The frame with the height of the video and the shape that the grid uses.
    pub picture: egui::ColorImage,
    texture: egui::TextureHandle,
    /// Where the current drag started, in picture pixels.
    drag_start: Option<Pos2>,
    pub selection: Option<PixelRect>,
    /// The selection that was copied last.
    pub copied: Option<PixelRect>,
}

impl BboxDialog {
    pub fn new(ctx: &Context, camera_id: Arc<str>, time: Timestamp, picture: egui::ColorImage) -> Self {
        let texture = ctx.load_texture("bbox-frame", picture.clone(), egui::TextureOptions::LINEAR);
        Self { camera_id, time, picture, texture, drag_start: None, selection: None, copied: None }
    }
}

/// Where the bbox capture is.
#[derive(Default)]
pub enum BboxMode {
    #[default]
    Off,
    /// The user must click a camera tile.
    PickCamera,
    Dialog(Box<BboxDialog>),
}

impl BboxMode {
    pub fn is_picking(&self) -> bool {
        matches!(self, Self::PickCamera)
    }

    pub fn is_dialog(&self) -> bool {
        matches!(self, Self::Dialog(_))
    }
}

/// Shows the frame dialog when it is open.
pub fn show_dialog(ctx: &Context, session: &mut Session) {
    let BboxMode::Dialog(dialog) = &mut session.bbox else { return };
    let [width, height] = dialog.picture.size;

    // Fit the frame in the window, with room for the title and the buttons.
    let window = ctx.content_rect().size();
    let room = egui::vec2((window.x - 80.0).max(160.0), (window.y - 170.0).max(90.0));
    let scale = (room.x / width as f32).min(room.y / height as f32).min(MAX_ZOOM);
    let shown_size = egui::vec2(width as f32 * scale, height as f32 * scale);

    let mut close = false;
    let mut copy = None;
    egui::Modal::new(egui::Id::unique("bbox-capture")).show(ctx, |ui| {
        ui.set_width(shown_size.x);
        ui.horizontal(|ui| {
            ui.heading("Bbox capture");
            ui.label(RichText::new(format!("{}  {}", dialog.camera_id, dialog.time.format_hms())).monospace().weak());
        });
        ui.add_space(4.0);

        let (rect, response) = ui.allocate_exact_size(shown_size, Sense::drag());
        let painter = ui.painter_at(rect);
        let whole = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(dialog.texture.id(), rect, whole, Color32::WHITE);
        if response.hovered() || response.dragged() {
            ctx.set_cursor_icon(CursorIcon::Crosshair);
        }

        let to_pixels = |pos: Pos2| ((pos - rect.min) / scale).to_pos2();
        if response.drag_started() {
            // The drag starts where the button went down, not where egui saw the first movement.
            let origin = ctx.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
            dialog.drag_start = origin.map(to_pixels);
            dialog.selection = None;
            dialog.copied = None;
        }
        if let (Some(start), Some(pointer)) = (dialog.drag_start, response.interact_pointer_pos()) {
            dialog.selection = Some(PixelRect::from_points(start, to_pixels(pointer), dialog.picture.size));
        }
        if response.drag_stopped() {
            dialog.drag_start = None;
            match dialog.selection.filter(PixelRect::is_selection) {
                Some(area) => {
                    let (origin, size) = ([area.x as usize, area.y as usize], [area.width as usize, area.height as usize]);
                    copy = Some(dialog.picture.region_by_pixels(origin, size));
                    dialog.copied = Some(area);
                }
                None => dialog.selection = None,
            }
        }

        if let Some(area) = dialog.selection {
            let min = rect.min + egui::vec2(area.x as f32, area.y as f32) * scale;
            let shown = Rect::from_min_size(min, egui::vec2(area.width as f32, area.height as f32) * scale);
            // A dark line and a bright line: the rectangle is visible on all pictures.
            painter.rect_stroke(shown.expand(1.0), 0.0, Stroke::new(1.0, Color32::from_black_alpha(200)), StrokeKind::Outside);
            painter.rect_stroke(shown, 0.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let text = match (dialog.copied, dialog.selection) {
                (Some(a), _) => format!("Copied {} x {} px at ({}, {})", a.width, a.height, a.x, a.y),
                (None, Some(a)) => format!("{} x {} px at ({}, {})", a.width, a.height, a.x, a.y),
                (None, None) => "Draw a rectangle on the picture to copy that area.".to_owned(),
            };
            ui.label(text);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.button("Close").clicked();
            });
        });
    });

    if let Some(picture) = copy {
        session.copy_picture_request = Some(picture);
    }
    // A click outside the dialog does not close it: a drag can end there by accident.
    if close || ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
        session.bbox = BboxMode::Off;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_covers_whole_pixels_in_any_drag_direction() {
        let picture = [1920, 1080];
        let expected = PixelRect { x: 10, y: 20, width: 91, height: 31 };
        assert_eq!(PixelRect::from_points(Pos2::new(10.2, 20.7), Pos2::new(100.1, 50.5), picture), expected);
        assert_eq!(PixelRect::from_points(Pos2::new(100.1, 50.5), Pos2::new(10.2, 20.7), picture), expected);
        assert_eq!(PixelRect::from_points(Pos2::new(100.1, 20.7), Pos2::new(10.2, 50.5), picture), expected);
    }

    #[test]
    fn rectangle_is_limited_to_the_picture() {
        let area = PixelRect::from_points(Pos2::new(-50.0, 1000.0), Pos2::new(3000.0, 5000.0), [1920, 1080]);
        assert_eq!(area, PixelRect { x: 0, y: 1000, width: 1920, height: 80 });
        assert!(area.is_selection());
    }

    #[test]
    fn a_click_is_not_a_selection() {
        let area = PixelRect::from_points(Pos2::new(40.3, 40.3), Pos2::new(40.6, 40.9), [1920, 1080]);
        assert_eq!((area.width, area.height), (1, 1));
        assert!(!area.is_selection());
    }
}
