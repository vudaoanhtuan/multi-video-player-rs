//! User settings, and the settings dialog that changes them.
//! See `specs/controls-and-layout.md`.

use egui::{Context, RichText};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub video_ratio: VideoRatio,
    pub decoder: Decoder,
}

/// Shows the settings dialog on top of the grid. A change of an option is
/// written to `settings` immediately, and the dialog stays open.
/// `hardware_available` is false when the computer has no hardware decoder
/// that the app can use. The "Hardware" option is then disabled.
/// Returns `true` when the user closed the dialog.
pub fn show_dialog(ctx: &Context, settings: &mut Settings, hardware_available: bool) -> bool {
    let mut close = false;
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

        ui.add_space(4.0);
        ui.separator();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = ui.button("Close").clicked();
        });
    });
    // The Close button, Escape, or a click outside.
    close || modal.should_close()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ratio_is_16_9() {
        assert_eq!(Settings::default().video_ratio, VideoRatio::Wide16x9);
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
