//! The camera picker dialog. See `specs/controls-and-layout.md`.

use egui::{Context, Key};

use super::session::Session;
use crate::render::VideoRenderer;

/// Shows the dialog on top of the grid when it is open. Playback continues.
pub fn show(ctx: &Context, session: &mut Session, renderer: &VideoRenderer) {
    let Some(draft) = session.picker.as_mut() else { return };
    let cameras = &session.library.cameras;

    let (mut ok, mut cancel) = (false, false);
    let modal = egui::Modal::new(egui::Id::unique("camera-picker")).show(ctx, |ui| {
        ui.set_width(320.0);
        ui.heading("Cameras");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Select all").clicked() {
                draft.fill(true);
            }
            if ui.button("Select none").clicked() {
                draft.fill(false);
            }
        });
        ui.separator();
        // A long list scrolls inside the dialog.
        let max_height = (ctx.content_rect().height() - 220.0).clamp(80.0, 420.0);
        egui::ScrollArea::vertical().max_height(max_height).auto_shrink([false, true]).show(ui, |ui| {
            for (camera, on) in cameras.iter().zip(draft.iter_mut()) {
                ui.checkbox(on, &*camera.camera_id);
            }
        });
        ui.separator();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ok = ui.button("OK").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });

    if ok || ctx.input(|i| i.key_pressed(Key::Enter)) {
        // OK applies the choice to the grid.
        if let Some(shown) = session.picker.take() {
            session.set_shown(shown, renderer);
        }
    } else if cancel || modal.should_close() {
        // Cancel, Escape, or a click outside: no change.
        session.picker = None;
    }
}
