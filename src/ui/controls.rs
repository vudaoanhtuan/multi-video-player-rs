//! The control bar: play/pause, timer, progress bar, speed menu, cameras button.
//! See `specs/controls-and-layout.md`.

use egui::{Align, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind, Ui};

use super::session::Session;
use crate::clock::{Snapshot, SPEEDS};
use crate::time::Timestamp;

const BAR_HEIGHT: f32 = 44.0;
const BUTTON_HEIGHT: f32 = 28.0;
const TIMER_WIDTH: f32 = 84.0;
const MARKER_RADIUS: f32 = 6.5;
const ACCENT: Color32 = Color32::from_rgb(76, 148, 255);
const NO_TIME: &str = "--:--:--";

pub fn speed_label(speed: f64) -> String {
    format!("{speed}x")
}

/// Shows the control bar. With no session, all items are shown disabled.
pub fn show(ui: &mut Ui, mut session: Option<&mut Session>, clock: Option<&Snapshot>) {
    ui.set_height(BAR_HEIGHT);
    ui.add_enabled_ui(session.is_some(), |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;

            if play_button(ui, clock.is_some_and(|c| c.playing)).clicked() {
                if let Some(session) = session.as_deref_mut() {
                    session.toggle_play();
                }
            }
            match (session.as_deref_mut(), clock) {
                (Some(session), Some(clock)) => timer(ui, session, clock),
                _ => {
                    ui.add_sized([TIMER_WIDTH, BUTTON_HEIGHT], egui::Button::new(RichText::new(NO_TIME).monospace()));
                }
            }

            // The items on the right are laid out first. The progress bar
            // then takes all the width that is left.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let cameras = match session.as_deref() {
                    Some(s) => format!("Cameras {}/{}", s.shown_count(), s.library.cameras.len()),
                    None => "Cameras".to_owned(),
                };
                if ui.add_sized([0.0, BUTTON_HEIGHT], egui::Button::new(cameras)).clicked() {
                    if let Some(session) = session.as_deref_mut() {
                        session.picker = Some(session.shown.clone());
                    }
                }
                speed_menu(ui, session.as_deref(), clock);

                let range = session.as_deref().map(|s| (s.library.start, s.library.end));
                let end_text = range.map_or(NO_TIME.to_owned(), |(_, end)| end.format_hms());
                let start_text = range.map_or(NO_TIME.to_owned(), |(start, _)| start.format_hms());
                ui.label(RichText::new(end_text).monospace().weak());
                let start_width = text_width(ui, &start_text) + ui.spacing().item_spacing.x;
                let bar_width = (ui.available_width() - start_width).max(40.0);
                progress_bar(ui, bar_width, session, clock);
                ui.label(RichText::new(start_text).monospace().weak());
            });
        });
    });
}

fn text_width(ui: &Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    ui.painter().layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x
}

/// A button with a play triangle or two pause bars. The icon is drawn with
/// shapes, so it does not depend on a font.
fn play_button(ui: &mut Ui, playing: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, BUTTON_HEIGHT), Sense::click());
    let visuals = ui.style().interact(&response);
    ui.painter().rect(rect, visuals.corner_radius, visuals.bg_fill, visuals.bg_stroke, StrokeKind::Inside);
    let color = visuals.fg_stroke.color;
    let c = rect.center();
    if playing {
        for x in [-4.5, 1.5] {
            let bar = Rect::from_min_size(c + egui::vec2(x, -6.0), egui::vec2(3.0, 12.0));
            ui.painter().rect_filled(bar, 0.5, color);
        }
    } else {
        let points = vec![c + egui::vec2(-4.0, -6.5), c + egui::vec2(-4.0, 6.5), c + egui::vec2(6.5, 0.0)];
        ui.painter().add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    }
    response.on_hover_text(if playing { "Pause (Space)" } else { "Play (Space)" })
}

/// The clock time. It cannot be edited. A click copies the time and pauses playback.
fn timer(ui: &mut Ui, session: &mut Session, clock: &Snapshot) {
    // During a drag on the progress bar, show the time under the pointer.
    let text = session.scrub.target.unwrap_or(clock.time).format_hms();
    let label = RichText::new(text).monospace().strong();
    let button = ui.add_sized([TIMER_WIDTH, BUTTON_HEIGHT], egui::Button::new(label));
    if button.on_hover_text("Click or press T to copy the time and pause. Press G to go to a time.").clicked() {
        session.copy_time_and_pause();
    }
}

fn speed_menu(ui: &mut Ui, session: Option<&Session>, clock: Option<&Snapshot>) {
    let current = clock.map_or(1.0, |c| c.speed);
    egui::ComboBox::from_id_salt("speed-menu")
        .width(64.0)
        .selected_text(speed_label(current))
        .show_ui(ui, |ui| {
            for speed in SPEEDS {
                if ui.selectable_label(speed == current, speed_label(speed)).clicked() {
                    if let Some(session) = session {
                        session.set_speed(speed);
                    }
                }
            }
        })
        .response
        .on_hover_text("Playback speed");
}

/// The progress bar. A click jumps, a drag jumps continuously, and the
/// pointer shows the time at its position.
fn progress_bar(ui: &mut Ui, width: f32, session: Option<&mut Session>, clock: Option<&Snapshot>) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, BUTTON_HEIGHT), Sense::click_and_drag());
    let track = Rect::from_center_size(rect.center(), egui::vec2(rect.width() - 2.0 * MARKER_RADIUS, 4.0));
    let painter = ui.painter();
    painter.rect_filled(track, 2.0, ui.visuals().widgets.inactive.bg_fill);
    let (Some(session), Some(clock)) = (session, clock) else { return };

    let (start, end) = (session.library.start, session.library.end);
    let span = (end.0 - start.0).max(1) as f64;
    let time_at = |x: f32| -> Timestamp {
        let fraction = ((x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
        Timestamp(start.0 + (fraction * span).round() as i64)
    };

    if response.is_pointer_button_down_on() {
        if let Some(pointer) = response.interact_pointer_pos() {
            if let Some(retry) = session.scrub_to(time_at(pointer.x)) {
                // The pointer can stop before the last position is sent.
                ui.ctx().request_repaint_after(retry);
            }
        }
    } else if session.scrub.target.is_some() {
        session.end_scrub();
    }

    let marker_time = session.scrub.target.unwrap_or(clock.time);
    let fraction = ((marker_time.0 - start.0) as f64 / span).clamp(0.0, 1.0) as f32;
    let marker = egui::pos2(track.left() + fraction * track.width(), track.center().y);
    painter.rect_filled(Rect::from_min_max(track.min, egui::pos2(marker.x, track.max.y)), 2.0, ACCENT);
    let active = response.hovered() || response.is_pointer_button_down_on();
    painter.circle_filled(marker, if active { MARKER_RADIUS } else { MARKER_RADIUS - 1.5 }, Color32::WHITE);

    if let Some(pointer) = response.hover_pos().or(response.interact_pointer_pos().filter(|_| active)) {
        // Time at the pointer position, in a small box above the bar.
        let text = time_at(pointer.x).format_hms();
        let layer = egui::LayerId::new(egui::Order::Tooltip, response.id.with("time"));
        let painter = ui.ctx().layer_painter(layer);
        let galley = painter.layout_no_wrap(text, FontId::monospace(12.0), Color32::WHITE);
        let size = galley.size() + egui::vec2(12.0, 6.0);
        let x = pointer.x.clamp(rect.left() + size.x / 2.0, (rect.right() - size.x / 2.0).max(rect.left() + size.x / 2.0));
        let tip = Rect::from_center_size(egui::pos2(x, rect.top() - size.y / 2.0 - 6.0), size);
        painter.rect(tip, 4.0, Color32::from_gray(20), Stroke::new(1.0, Color32::from_gray(70)), StrokeKind::Inside);
        painter.galley(tip.min + egui::vec2(6.0, 3.0), galley, Color32::WHITE);
    }
}
