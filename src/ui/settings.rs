//! User settings, and the settings dialog that changes them.
//! See `specs/controls-and-layout.md`.

//!
//! The app saves the settings as JSON text, for example:
//!
//! ```json
//! {
//!   "video_ratio": "Wide16x9",
//!   "show_camera_name": false,
//!   "decoder": "Hardware",
//!   "key_only_min_speed": 4
//! }
//! ```
//!
//! The name of a field in the file is the name of the field in [`Settings`].
//! A setting that is an enum is saved as the name of its value in the code.
//! Thus a new name for a field or for an enum value changes the file format.

use std::io;
use std::path::{Path, PathBuf};

use egui::{Context, Key, RichText};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::playback::DEFAULT_KEY_ONLY_MIN_SPEED;

/// The shape in which the videos are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Settings {
    pub video_ratio: VideoRatio,
    /// Show the camera id as a label in each tile.
    pub show_camera_name: bool,
    pub decoder: Decoder,
    /// From this speed, a camera shows key frames only, if its video has key
    /// frames often enough. [`KEY_ONLY_MIN_SPEED_MIN`] or more.
    pub key_only_min_speed: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            video_ratio: VideoRatio::default(),
            show_camera_name: false,
            decoder: Decoder::default(),
            key_only_min_speed: DEFAULT_KEY_ONLY_MIN_SPEED as u32,
        }
    }
}

impl Settings {
    /// The JSON text of the settings file.
    pub fn to_json(self) -> String {
        serde_json::to_string_pretty(&self).expect("settings have a JSON form")
    }

    /// Reads the JSON text of a settings file. A setting that the text does
    /// not have, or that has a value that is not valid, gets its default.
    pub fn from_json(text: &str) -> Self {
        let mut settings = Self::default();
        let Ok(serde_json::Value::Object(saved)) = serde_json::from_str(text) else {
            return settings;
        };
        read_field(&saved, "video_ratio", &mut settings.video_ratio);
        read_field(&saved, "show_camera_name", &mut settings.show_camera_name);
        read_field(&saved, "decoder", &mut settings.decoder);
        read_field(&saved, "key_only_min_speed", &mut settings.key_only_min_speed);
        settings.key_only_min_speed = settings.key_only_min_speed.max(KEY_ONLY_MIN_SPEED_MIN);
        settings
    }

    /// Reads the settings file. With no file, or with a file that the app
    /// cannot read, all settings have their defaults.
    pub fn load(file: &Path) -> Self {
        match std::fs::read_to_string(file) {
            Ok(text) => Self::from_json(&text),
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    log::warn!("cannot read the settings file {}: {error}", file.display());
                }
                Self::default()
            }
        }
    }

    /// Writes the settings file. Makes its folder if necessary.
    pub fn save(&self, file: &Path) -> io::Result<()> {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(file, self.to_json() + "\n")
    }
}

/// Puts the saved value of one setting in `setting`, if the value is valid.
fn read_field<T: DeserializeOwned>(saved: &serde_json::Map<String, serde_json::Value>, name: &str, setting: &mut T) {
    if let Some(value) = saved.get(name).and_then(|value| T::deserialize(value).ok()) {
        *setting = value;
    }
}

/// The name of the settings file in the settings folder of the app.
const FILE_NAME: &str = "settings.json";

/// The file in which the app saves the settings. `None`: the app does not
/// read or save settings.
///
/// `scripted` is true when a test script controls the app. The app then uses
/// a settings file only if the `MULTI_VIDEO_PLAYER_SETTINGS` environment
/// variable gives its path, so a test does not change the settings of the user.
pub fn file_path(scripted: bool) -> Option<PathBuf> {
    if let Some(file) = std::env::var_os("MULTI_VIDEO_PLAYER_SETTINGS").filter(|file| !file.is_empty()) {
        return Some(PathBuf::from(file));
    }
    if scripted {
        return None;
    }
    Some(crate::platform::config_dir()?.join(crate::APP_NAME).join(FILE_NAME))
}

/// The lowest value of the "Key frames only from" setting. The value must be
/// more than 0. There is no highest value: with a value above the highest
/// speed, a camera shows key frames only when it is late.
pub const KEY_ONLY_MIN_SPEED_MIN: u32 = 1;

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
            let speed = &mut settings.key_only_min_speed;
            ui.add(egui::DragValue::new(speed).range(KEY_ONLY_MIN_SPEED_MIN..=u32::MAX).speed(0.05).suffix("x"));
            *speed = (*speed).max(KEY_ONLY_MIN_SPEED_MIN);
        });
        ui.label(RichText::new("From this speed, the app shows key frames only.").weak());

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
    fn default_key_only_speed_is_4x() {
        assert_eq!(Settings::default().key_only_min_speed, 4);
    }

    #[test]
    fn lowest_key_only_speed_is_1x() {
        // The value must be more than 0.
        assert_eq!(KEY_ONLY_MIN_SPEED_MIN, 1);
        assert!(Settings::default().key_only_min_speed >= KEY_ONLY_MIN_SPEED_MIN);
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

    /// Settings in which no setting has its default.
    fn changed() -> Settings {
        Settings { video_ratio: VideoRatio::None, show_camera_name: true, decoder: Decoder::Software, key_only_min_speed: 12 }
    }

    #[test]
    fn json_has_the_enum_names() {
        let json: serde_json::Value = serde_json::from_str(&Settings::default().to_json()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "video_ratio": "Wide16x9",
                "show_camera_name": false,
                "decoder": "Hardware",
                "key_only_min_speed": 4,
            })
        );
        assert!(changed().to_json().contains(r#""video_ratio": "None""#));
        assert!(changed().to_json().contains(r#""decoder": "Software""#));
    }

    #[test]
    fn json_gives_the_same_settings_again() {
        assert_eq!(Settings::from_json(&changed().to_json()), changed());
        assert_eq!(Settings::from_json(&Settings::default().to_json()), Settings::default());
    }

    #[test]
    fn text_that_is_not_a_json_object_gives_the_defaults() {
        for text in ["", "not json", "[1, 2]", "\"Software\"", "{\"decoder\": \"Software\""] {
            assert_eq!(Settings::from_json(text), Settings::default(), "{text:?}");
        }
    }

    #[test]
    fn missing_setting_gets_its_default() {
        let settings = Settings::from_json(r#"{"decoder": "Software"}"#);
        assert_eq!(settings, Settings { decoder: Decoder::Software, ..Settings::default() });
    }

    #[test]
    fn invalid_value_gets_its_default_and_the_others_stay() {
        // The label "16:9" is not the enum name. Enum names are case sensitive.
        let text = r#"{"video_ratio": "16:9", "show_camera_name": "yes", "decoder": "software", "key_only_min_speed": 12}"#;
        assert_eq!(Settings::from_json(text), Settings { key_only_min_speed: 12, ..Settings::default() });
        let text = r#"{"video_ratio": "None", "key_only_min_speed": -3, "other": 1}"#;
        assert_eq!(Settings::from_json(text), Settings { video_ratio: VideoRatio::None, ..Settings::default() });
        let text = r#"{"show_camera_name": true, "key_only_min_speed": 2.5}"#;
        assert_eq!(Settings::from_json(text), Settings { show_camera_name: true, ..Settings::default() });
    }

    #[test]
    fn saved_key_only_speed_below_the_lowest_becomes_the_lowest() {
        assert_eq!(Settings::from_json(r#"{"key_only_min_speed": 0}"#).key_only_min_speed, KEY_ONLY_MIN_SPEED_MIN);
    }

    #[test]
    fn settings_file_is_in_the_folder_of_the_app_and_a_test_script_does_not_use_it() {
        // The variable replaces the path. This test is for the path with no variable.
        if std::env::var_os("MULTI_VIDEO_PLAYER_SETTINGS").is_some() {
            return;
        }
        assert_eq!(file_path(true), None);
        let file = file_path(false).expect("the system gives a folder for settings");
        assert!(file.is_absolute());
        assert!(file.ends_with(Path::new("Multi Video Player").join("settings.json")), "{}", file.display());
    }

    #[test]
    fn save_makes_the_folder_and_load_reads_the_file() {
        let folder = std::env::temp_dir().join(format!("multi-video-player-settings-test-{}", std::process::id()));
        let file = folder.join("sub").join(FILE_NAME);
        assert_eq!(Settings::load(&file), Settings::default());
        changed().save(&file).unwrap();
        assert_eq!(Settings::load(&file), changed());
        // A second save replaces the file.
        Settings::default().save(&file).unwrap();
        assert_eq!(Settings::load(&file), Settings::default());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
