//! The "go to time" dialog. See `specs/controls-and-layout.md`.

use egui::{Color32, Context, Event, Key, Stroke, StrokeKind};

use super::session::Session;
use crate::time::{parse_pasted_time_of_day, parse_time_of_day};

const INVALID: Color32 = Color32::from_rgb(235, 80, 80);

/// Shows the dialog on top of the grid when it is open. Playback continues.
pub fn show(ctx: &Context, session: &mut Session) {
    if session.goto.is_none() {
        return;
    }

    // Pasted text does not go into the text box. A time in a full form
    // (`HH:mm:ss` or `HHmmss`) closes the dialog and jumps. Other text is
    // ignored, and the text box does not change.
    let mut pasted = Vec::new();
    ctx.input_mut(|input| {
        input.events.retain(|event| match event {
            Event::Paste(text) => {
                pasted.push(text.clone());
                false
            }
            _ => true,
        });
    });
    if let Some(time_of_day) = pasted.iter().find_map(|text| parse_pasted_time_of_day(text)) {
        session.goto = None;
        session.jump_to_time_of_day(time_of_day);
        return;
    }

    let Some(dialog) = session.goto.as_mut() else { return };
    let (mut ok, mut cancel) = (false, false);
    let modal = egui::Modal::new(egui::Id::unique("go-to-time")).show(ctx, |ui| {
        ui.set_width(280.0);
        ui.heading("Go to time");
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Type or paste HH:mm:ss or HHmmss").weak());
        ui.add_space(6.0);

        let mut text_edit = egui::TextEdit::singleline(&mut dialog.text)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY);
        if dialog.invalid {
            text_edit = text_edit.text_color(INVALID);
        }
        let mut output = text_edit.show(ui);
        let response = output.response.clone();
        if dialog.invalid {
            ui.painter().rect_stroke(response.rect, 2.0, Stroke::new(1.5, INVALID), StrokeKind::Outside);
        }
        if !dialog.focused {
            // First frame: take the keyboard focus and select the text, so
            // the user can type a new time immediately.
            dialog.focused = true;
            response.request_focus();
            let all = egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(dialog.text.chars().count()),
            );
            output.state.cursor.set_char_range(Some(all));
            output.state.store(ui.ctx(), response.id);
        }
        if response.changed() {
            dialog.invalid = false;
        }

        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ok = ui.button("OK").clicked();
            cancel = ui.button("Cancel").clicked();
        });
        response
    });

    if ok || ctx.input(|i| i.key_pressed(Key::Enter)) {
        match parse_time_of_day(&dialog.text) {
            Some(time_of_day) => {
                session.goto = None;
                session.jump_to_time_of_day(time_of_day);
            }
            None => {
                // Not a time: keep the dialog open and mark the text box in red.
                dialog.invalid = true;
                modal.inner.request_focus();
            }
        }
    } else if cancel || modal.should_close() {
        // Cancel, Escape, or a click outside: no jump.
        session.goto = None;
    }
}
