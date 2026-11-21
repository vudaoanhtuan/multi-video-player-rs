//! User settings. They are changed in the menu of the control bar.

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub video_ratio: VideoRatio,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ratio_is_16_9() {
        assert_eq!(Settings::default().video_ratio, VideoRatio::Wide16x9);
    }

    #[test]
    fn ratio_changes_the_shape_only_when_it_is_set() {
        let four_by_three = 4.0 / 3.0;
        assert_eq!(VideoRatio::None.apply(four_by_three), four_by_three);
        assert_eq!(VideoRatio::Wide16x9.apply(four_by_three), 16.0 / 9.0);
        assert_eq!(VideoRatio::Wide16x9.apply(32.0 / 9.0), 16.0 / 9.0);
    }
}
