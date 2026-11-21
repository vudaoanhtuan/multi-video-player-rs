//! The camera grid. See `specs/controls-and-layout.md`.

use egui::{Align2, Color32, FontId, Rect, Ui};

use super::session::Session;
use crate::clock::Snapshot;
use crate::render::VideoRenderer;
use crate::time::Timestamp;

/// Color of the thin lines between tiles.
const GRID_LINE: Color32 = Color32::from_gray(38);

/// Columns and rows of a grid that is close to square.
pub fn grid_shape(count: usize) -> (usize, usize) {
    if count == 0 {
        return (0, 0);
    }
    let mut columns = (count as f64).sqrt().ceil() as usize;
    // Guard against a floating point result that is one too small.
    while columns * columns < count {
        columns += 1;
    }
    (columns, count.div_ceil(columns))
}

/// The largest rectangle with the given shape (width / height) inside `area`, centered.
pub fn fit_rect(aspect: f32, area: Rect) -> Rect {
    if aspect <= 0.0 || !aspect.is_finite() || area.width() <= 0.0 || area.height() <= 0.0 {
        return area;
    }
    let size = if area.width() / area.height() > aspect {
        egui::vec2(area.height() * aspect, area.height())
    } else {
        egui::vec2(area.width(), area.width() / aspect)
    };
    Rect::from_center_size(area.center(), size)
}

/// Draws all tiles. Returns the earliest time at which a tile must change.
pub fn show(ui: &mut Ui, session: &mut Session, clock: &Snapshot, renderer: &VideoRenderer) -> Option<Timestamp> {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, egui::Sense::hover());
    let painter = ui.painter_at(area);
    let (columns, rows) = grid_shape(session.tiles.len());
    if columns == 0 {
        painter.rect_filled(area, 0.0, Color32::BLACK);
        return None;
    }
    // Cells with no camera stay black, like the unused area inside a tile.
    painter.rect_filled(area, 0.0, Color32::BLACK);

    let cell = egui::vec2(area.width() / columns as f32, area.height() / rows as f32);
    let mut next_due: Option<Timestamp> = None;

    for (i, tile) in session.tiles.iter_mut().enumerate() {
        let (column, row) = (i % columns, i / columns);
        let rect = Rect::from_min_size(area.min + egui::vec2(column as f32 * cell.x, row as f32 * cell.y), cell);
        let painter = ui.painter_at(rect);

        if let Some(due) = tile.player.update(clock) {
            next_due = Some(next_due.map_or(due, |d| d.min(due)));
        }
        if let Some(draw) = renderer.prepare(&mut tile.surface, tile.player.frame()) {
            painter.add(renderer.paint_callback(fit_rect(draw.display_aspect, rect), &draw));
        }

        // Camera id label in the top left corner.
        let id = &session.library.cameras[tile.camera].camera_id;
        let galley = painter.layout_no_wrap(id.to_string(), FontId::proportional(12.0), Color32::WHITE);
        let label = Align2::LEFT_TOP.anchor_size(rect.min + egui::vec2(6.0, 6.0), galley.size() + egui::vec2(10.0, 4.0));
        painter.rect_filled(label, 3.0, Color32::from_black_alpha(160));
        painter.galley(label.min + egui::vec2(5.0, 2.0), galley, Color32::WHITE);
    }

    // Thin lines between the tiles.
    let line = egui::Stroke::new(1.0, GRID_LINE);
    for column in 1..columns {
        painter.vline(area.left() + column as f32 * cell.x, area.y_range(), line);
    }
    for row in 1..rows {
        painter.hline(area.x_range(), area.top() + row as f32 * cell.y, line);
    }
    next_due
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_is_close_to_square() {
        let shapes: Vec<_> = [0, 1, 2, 3, 4, 5, 6, 9, 10, 16, 17].iter().map(|n| grid_shape(*n)).collect();
        assert_eq!(
            shapes,
            [(0, 0), (1, 1), (2, 1), (2, 2), (2, 2), (3, 2), (3, 2), (3, 3), (4, 3), (4, 4), (5, 4)]
        );
    }

    #[test]
    fn video_keeps_its_shape() {
        let area = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 400.0));
        let wide = fit_rect(16.0 / 9.0, area);
        assert_eq!(wide.width(), 400.0);
        assert!((wide.height() - 225.0).abs() < 0.01);
        assert_eq!(wide.center(), area.center());
        let tall = fit_rect(0.5, area);
        assert_eq!((tall.width(), tall.height()), (200.0, 400.0));
    }
}
