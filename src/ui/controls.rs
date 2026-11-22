//! The control bar: play/pause, timer, progress bar, speed menu, cameras button.
//! See `specs/controls-and-layout.md`.

use std::sync::OnceLock;

use egui::{Align, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind, Ui};

use super::session::Session;
use super::settings::{Settings, VideoRatio};
use crate::clock::{Snapshot, SPEEDS};
use crate::time::Timestamp;

const BAR_HEIGHT: f32 = 44.0;
const BUTTON_HEIGHT: f32 = 28.0;
const TIMER_WIDTH: f32 = 84.0;
const MARKER_RADIUS: f32 = 6.5;
const ACCENT: Color32 = Color32::from_rgb(76, 148, 255);
const NO_TIME: &str = "--:--:--";
/// Icons are drawn in a square of this size.
const ICON_SIZE: f32 = 20.0;

/// An SVG icon from `assets/icons`.
struct Icon {
    /// Name of the picture in the egui image cache.
    uri: &'static str,
    svg: &'static str,
    /// The SVG with a white fill, made at the first use.
    white: OnceLock<egui::load::Bytes>,
}

impl Icon {
    const fn new(uri: &'static str, svg: &'static str) -> Self {
        Self { uri, svg, white: OnceLock::new() }
    }

    /// The icon as a white picture. A tint then gives it the color of the
    /// button text. The source files are black, and a tint cannot change black.
    fn image(&self) -> egui::Image<'static> {
        let white = self.white.get_or_init(|| self.svg.replacen("<svg ", "<svg fill=\"#ffffff\" ", 1).into_bytes().into());
        egui::Image::from_bytes(self.uri, white.clone())
    }
}

static PLAY_ICON: Icon = Icon::new("bytes://icons/play_arrow.svg", include_str!("../../assets/icons/play_arrow.svg"));
static PAUSE_ICON: Icon = Icon::new("bytes://icons/pause.svg", include_str!("../../assets/icons/pause.svg"));
static MORE_ICON: Icon = Icon::new("bytes://icons/more_vert.svg", include_str!("../../assets/icons/more_vert.svg"));
static SCISSORS_ICON: Icon = Icon::new("bytes://icons/content_cut.svg", include_str!("../../assets/icons/content_cut.svg"));
static CAMERA_ICON: Icon = Icon::new("bytes://icons/photo_camera.svg", include_str!("../../assets/icons/photo_camera.svg"));

pub fn speed_label(speed: f64) -> String {
    format!("{speed}x")
}

/// A command from the menu of the control bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarAction {
    OpenFolder,
    CloseFolder,
    SetVideoRatio(VideoRatio),
    ShowHelp,
}

/// Shows the control bar. With no session, all items are shown disabled,
/// except the menu button: the user needs it to open a folder.
/// `can_close` is true when there is something to close.
pub fn show(
    ui: &mut Ui,
    mut session: Option<&mut Session>,
    clock: Option<&Snapshot>,
    can_close: bool,
    settings: &Settings,
) -> Option<BarAction> {
    ui.set_height(BAR_HEIGHT);
    let enabled = session.is_some();
    let mut action = None;
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;

        ui.add_enabled_ui(enabled, |ui| {
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
        });

        // The items on the right are laid out first, from the right end. The
        // progress bar then takes all the width that is left.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            action = more_menu(ui, can_close, settings);

            ui.add_enabled_ui(enabled, |ui| {
                // Cameras: a camera icon and the count of shown cameras.
                let count = session.as_deref().map(|s| format!("{}/{}", s.shown_count(), s.library.cameras.len()));
                let cameras = icon_button(ui, &CAMERA_ICON, count.as_deref(), false);
                if cameras.on_hover_text("Choose the cameras to show (C)").clicked() {
                    if let Some(session) = session.as_deref_mut() {
                        session.picker = Some(session.shown.clone());
                    }
                }
                // Bbox capture: a scissors icon. The button looks pressed during the camera choice.
                let picking = session.as_deref().is_some_and(|s| s.bbox.is_picking());
                let bbox = icon_button(ui, &SCISSORS_ICON, None, picking);
                if bbox.on_hover_text("Bbox capture: copy an area of a camera frame (F)").clicked() {
                    if let Some(session) = session.as_deref_mut() {
                        session.toggle_bbox();
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
    action
}

/// The menu button at the right end of the control bar: three vertical dots.
fn more_menu(ui: &mut Ui, can_close: bool, settings: &Settings) -> Option<BarAction> {
    let button = icon_button(ui, &MORE_ICON, None, false);
    let mut action = None;
    egui::Popup::menu(&button).show(|ui| {
        ui.set_min_width(150.0);
        if ui.button("Open Folder…").clicked() {
            action = Some(BarAction::OpenFolder);
            ui.close();
        }
        if ui.add_enabled(can_close, egui::Button::new("Close Folder")).clicked() {
            action = Some(BarAction::CloseFolder);
            ui.close();
        }
        ui.separator();
        ui.menu_button("Settings", |ui| {
            ui.set_min_width(130.0);
            ui.label(RichText::new("Video ratio").weak());
            for ratio in VideoRatio::ALL {
                if ui.radio(settings.video_ratio == ratio, ratio.label()).clicked() {
                    action = Some(BarAction::SetVideoRatio(ratio));
                    ui.close();
                }
            }
        });
        if ui.button("Help").clicked() {
            action = Some(BarAction::ShowHelp);
            ui.close();
        }
    });
    action
}

fn text_width(ui: &Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    ui.painter().layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x
}

/// The play/pause button. It shows the action of a click: a play icon
/// while paused, and a pause icon while playing.
fn play_button(ui: &mut Ui, playing: bool) -> egui::Response {
    let icon = if playing { &PAUSE_ICON } else { &PLAY_ICON };
    icon_button(ui, icon, None, false).on_hover_text(if playing { "Pause (Space)" } else { "Play (Space)" })
}

/// A button with an SVG icon. Optional text comes after the icon.
/// `selected` shows the button as pressed.
fn icon_button(ui: &mut Ui, icon: &Icon, text: Option<&str>, selected: bool) -> egui::Response {
    const PADDING: f32 = 8.0;
    const GAP: f32 = 5.0;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = text.map(|text| ui.painter().layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER));
    let text_width = galley.as_ref().map_or(0.0, |galley| GAP + galley.size().x);
    let size = egui::vec2(PADDING + ICON_SIZE + text_width + PADDING, BUTTON_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let visuals = ui.style().interact(&response);
    let (fill, stroke) = if selected {
        (ui.visuals().selection.bg_fill, ui.visuals().selection.stroke)
    } else {
        (visuals.weak_bg_fill, visuals.bg_stroke)
    };
    let color = if selected { ui.visuals().selection.stroke.color } else { visuals.text_color() };
    ui.painter().rect(rect, visuals.corner_radius, fill, stroke, StrokeKind::Inside);
    let icon_center = egui::pos2(rect.left() + PADDING + ICON_SIZE / 2.0, rect.center().y);
    icon.image().tint(color).paint_at(ui, Rect::from_center_size(icon_center, egui::Vec2::splat(ICON_SIZE)));
    if let Some(galley) = galley {
        let pos = egui::pos2(rect.left() + PADDING + ICON_SIZE + GAP, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, color);
    }
    response
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
        .on_hover_text("Playback speed (1 to 8 for 1x to 8x)");
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
