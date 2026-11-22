//! User settings, and the settings dialog that changes them.
//! See `specs/controls-and-layout.md`.

use std::ops::RangeInclusive;

use egui::{Context, Key, RichText};

use crate::playback::DEFAULT_KEY_ONLY_MIN_SPEED;

/// The shape in which the videos are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VideoRatio {
    /// Each video keeps its own shape.
    None,
    /// Each video is stretched to 16:9. A video that is not 16:9 gets wider or narrower.
    #[default]
    Wide16x9,
}

impl VideoRatio {
    pub const ALL: [Self; 2] = [Self::None, Self::Wide16x9];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Wide16x9 => "16:9",
        }
    }

    /// The shape (width / height) in which to show a video with the shape `video`.
    pub fn apply(self, video: f32) -> f32 {
        match self {
            Self::None => video,
            Self::Wide16x9 => 16.0 / 9.0,
        }
    }
}

/// The decoder that decodes the videos.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Decoder {
    /// The video decoder of the GPU.
    #[default]
    Hardware,
    /// FFmpeg decodes on the CPU.
    Software,
}

impl Decoder {
    pub const ALL: [Self; 2] = [Self::Hardware, Self::Software];

    pub fn label(self) -> &'static str {
        match self {
            Self::Hardware => "Hardware",
            Self::Software => "Software",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub video_ratio: VideoRatio,
    /// Show the camera id as a label in each tile.
    pub show_camera_name: bool,
    pub decoder: Decoder,
    /// From this speed, a camera shows key frames only, if its video has key
    /// frames often enough. A whole number in [`KEY_ONLY_MIN_SPEED_RANGE`].
    pub key_only_min_speed: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            video_ratio: VideoRatio::default(),
            show_camera_name: false,
            decoder: Decoder::default(),
            key_only_min_speed: DEFAULT_KEY_ONLY_MIN_SPEED,
        }
    }
}

/// The permitted values of the "Key frames only from" setting. At 1x a
/// camera always shows all frames, and 16x is the highest speed of the app.
pub const KEY_ONLY_MIN_SPEED_RANGE: RangeInclusive<u32> = 2..=16;

/// How the user closed the settings dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogResult {
    /// Use the values of the dialog.
    Apply,
    /// Keep the settings as they were.
    Cancel,
}

/// Shows the settings dialog on top of the grid. `settings` holds the values
/// of the dialog. They are not in use until the user applies them.
/// `hardware_available` is false when the computer has no hardware decoder
/// that the app can use. The "Hardware" option is then disabled.
/// Returns a result when the user closed the dialog.
pub fn show_dialog(ctx: &Context, settings: &mut Settings, hardware_available: bool) -> Option<DialogResult> {
    let (mut apply, mut cancel) = (false, false);
    let modal = egui::Modal::new(egui::Id::unique("settings")).show(ctx, |ui| {
        ui.set_width(320.0);
        ui.heading("Settings");
        ui.add_space(6.0);

        ui.label(RichText::new("Video ratio").strong());
        ui.horizontal(|ui| {
            for ratio in VideoRatio::ALL {
                ui.radio_value(&mut settings.video_ratio, ratio, ratio.label());
            }
        });
        ui.label(RichText::new("16:9 stretches each video to 16:9. None keeps the shape of the video.").weak());
        ui.add_space(10.0);

        ui.label(RichText::new("Camera name").strong());
        ui.horizontal(|ui| {
            ui.radio_value(&mut settings.show_camera_name, false, "Hide");
            ui.radio_value(&mut settings.show_camera_name, true, "Show");
        });
        ui.label(RichText::new("Show puts the camera name in the top left corner of each video.").weak());
        ui.add_space(10.0);

        ui.label(RichText::new("Decoder").strong());
        ui.horizontal(|ui| {
            for decoder in Decoder::ALL {
                let enabled = hardware_available || decoder != Decoder::Hardware;
                ui.add_enabled_ui(enabled, |ui| {
                    ui.radio_value(&mut settings.decoder, decoder, decoder.label())
                        .on_disabled_hover_text("This computer has no hardware decoder that the app can use");
                });
            }
        });
        ui.label(RichText::new("Hardware uses the video decoder of the GPU. Software uses the CPU.").weak());
        ui.add_space(10.0);

        ui.label(RichText::new("Key frames only from").strong());
        ui.horizontal(|ui| {
            // A number box: the user types a value or drags the box.
            let mut speed = settings.key_only_min_speed.round() as u32;
            let (min, max) = (*KEY_ONLY_MIN_SPEED_RANGE.start(), *KEY_ONLY_MIN_SPEED_RANGE.end());
            ui.add(egui::DragValue::new(&mut speed).range(KEY_ONLY_MIN_SPEED_RANGE).speed(0.05).suffix("x"));
            settings.key_only_min_speed = f64::from(speed.clamp(min, max));
            ui.label(RichText::new(format!("A number from {min} to {max}")).weak());
        });
        ui.label(
            RichText::new(
                "From this speed, a camera shows key frames only, if its video has key frames often enough. \
                 The picture then changes in steps.",
            )
            .weak(),
        );

        ui.add_space(4.0);
        ui.separator();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            apply = ui.button("Apply").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });

    if apply || ctx.input(|i| i.key_pressed(Key::Enter)) {
        Some(DialogResult::Apply)
    } else if cancel || modal.should_close() {
        // Cancel, Escape, or a click outside: no change.
        Some(DialogResult::Cancel)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ratio_is_16_9() {
        assert_eq!(Settings::default().video_ratio, VideoRatio::Wide16x9);
    }

    #[test]
    fn default_key_only_speed_is_8x_and_is_an_option() {
        let default = Settings::default().key_only_min_speed;
        assert_eq!(default, 8.0);
        assert!(KEY_ONLY_MIN_SPEED_RANGE.contains(&(default as u32)));
    }

    #[test]
    fn key_only_speed_range_is_above_1x_up_to_the_highest_speed() {
        let highest = crate::clock::SPEEDS.into_iter().fold(0.0, f64::max);
        assert_eq!(*KEY_ONLY_MIN_SPEED_RANGE.start(), 2);
        assert_eq!(f64::from(*KEY_ONLY_MIN_SPEED_RANGE.end()), highest);
    }

    #[test]
    fn camera_name_is_hidden_by_default() {
        assert!(!Settings::default().show_camera_name);
    }

    #[test]
    fn default_decoder_is_hardware() {
        assert_eq!(Settings::default().decoder, Decoder::Hardware);
    }

    #[test]
    fn ratio_changes_the_shape_only_when_it_is_set() {
        let four_by_three = 4.0 / 3.0;
        assert_eq!(VideoRatio::None.apply(four_by_three), four_by_three);
        assert_eq!(VideoRatio::Wide16x9.apply(four_by_three), 16.0 / 9.0);
        assert_eq!(VideoRatio::Wide16x9.apply(32.0 / 9.0), 16.0 / 9.0);
    }
}
