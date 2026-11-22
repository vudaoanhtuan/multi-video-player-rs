//! One open folder: the library, the clock, and the shown cameras.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::bbox::{BboxDialog, BboxMode};
use super::settings::VideoRatio;
use crate::clock::{Clock, DEFAULT_SPEED};
use crate::library::Library;
use crate::media::DecodeOptions;
use crate::playback::{CameraPlayer, Waker};
use crate::render::{TileSurface, VideoRenderer};
use crate::time::{resolve_time_of_day, Micros, Timestamp};

/// Time between tile updates during a drag on the progress bar: about 10 each second.
const SCRUB_INTERVAL: Duration = Duration::from_millis(100);
/// One shown camera.
pub struct Tile {
    /// Index into `Library::cameras`.
    pub camera: usize,
    pub player: CameraPlayer,
    pub surface: TileSurface,
}

/// The "go to time" dialog.
pub struct GotoDialog {
    pub text: String,
    /// The text is not a time. The text box is shown in red.
    pub invalid: bool,
    /// The text box got the keyboard focus on its first frame.
    pub focused: bool,
}

/// State of a press or drag on the progress bar.
#[derive(Default)]
pub struct Scrub {
    /// Time under the pointer while the button is down.
    pub target: Option<Timestamp>,
    /// The last target that was sent to the clock.
    sent: Option<(Timestamp, Instant)>,
}

pub struct Session {
    pub library: Library,
    pub clock: Arc<Clock>,
    /// One flag for each camera of the library.
    pub shown: Vec<bool>,
    /// Shown cameras in camera id order.
    pub tiles: Vec<Tile>,
    /// The open "go to time" dialog.
    pub goto: Option<GotoDialog>,
    /// Text that the user asked to copy. The app moves it to the clipboard.
    pub copy_request: Option<String>,
    /// A picture that the user asked to copy. The app moves it to the clipboard.
    pub copy_picture_request: Option<egui::ColorImage>,
    /// A short message for the user. The app shows it for some seconds.
    pub message: Option<String>,
    /// State of the bbox capture.
    pub bbox: BboxMode,
    /// Checkbox values of the open camera picker dialog.
    pub picker: Option<Vec<bool>>,
    pub scrub: Scrub,
    opts: DecodeOptions,
    /// The "Key frames only from" setting. Each player gets it.
    key_only_min_speed: f64,
    waker: Waker,
}

impl Session {
    /// All cameras shown, clock at the start of the range, paused, speed 1x.
    pub fn new(
        library: Library,
        opts: DecodeOptions,
        key_only_min_speed: f64,
        waker: Waker,
        renderer: &VideoRenderer,
    ) -> Self {
        let clock = Arc::new(Clock::new(library.start, library.end));
        debug_assert_eq!(clock.snapshot().speed, DEFAULT_SPEED);
        let mut session = Self {
            shown: Vec::new(),
            library,
            clock,
            tiles: Vec::new(),
            goto: None,
            copy_request: None,
            copy_picture_request: None,
            message: None,
            bbox: BboxMode::Off,
            picker: None,
            scrub: Scrub::default(),
            opts,
            key_only_min_speed,
            waker,
        };
        session.set_shown(vec![true; session.library.cameras.len()], renderer);
        session
    }

    /// Applies a camera choice. A hidden camera stops its decode and closes
    /// its files. A camera that becomes shown joins at the current clock time.
    pub fn set_shown(&mut self, shown: Vec<bool>, renderer: &VideoRenderer) {
        let mut old: Vec<Option<Tile>> = std::mem::take(&mut self.tiles).into_iter().map(Some).collect();
        for (camera, _) in shown.iter().enumerate().filter(|(_, on)| **on) {
            let kept = old.iter_mut().find(|t| t.as_ref().is_some_and(|t| t.camera == camera));
            let tile = match kept.and_then(Option::take) {
                Some(tile) => tile,
                None => {
                    let player = CameraPlayer::start(
                        self.library.cameras[camera].clone(),
                        self.clock.clone(),
                        self.opts,
                        self.waker.clone(),
                    );
                    player.set_key_only_min_speed(self.key_only_min_speed);
                    Tile { camera, player, surface: renderer.new_tile() }
                }
            };
            self.tiles.push(tile);
        }
        self.shown = shown;
    }

    /// Changes the decoder. Each shown camera opens its files again with the
    /// new decoder and joins at the current clock time. A tile is black until
    /// its frame is ready.
    pub fn set_decode(&mut self, opts: DecodeOptions, renderer: &VideoRenderer) {
        self.opts = opts;
        // Dropping a tile stops its decode thread.
        self.tiles.clear();
        self.set_shown(self.shown.clone(), renderer);
    }

    /// Changes the lowest speed at which the cameras show key frames only.
    /// The decode continues. Frames that are already decoded are still shown.
    pub fn set_key_only_min_speed(&mut self, speed: f64) {
        self.key_only_min_speed = speed;
        for tile in &self.tiles {
            tile.player.set_key_only_min_speed(speed);
        }
    }

    pub fn shown_count(&self) -> usize {
        self.shown.iter().filter(|on| **on).count()
    }

    /// Tells all decode threads that the clock changed.
    fn wake_players(&self) {
        for tile in &self.tiles {
            tile.player.wake();
        }
    }

    /// Pauses the clock if it is playing.
    pub fn pause(&self) {
        if self.clock.snapshot().playing {
            self.clock.pause();
            self.wake_players();
        }
    }

    /// The bbox button or the F key: pauses playback and starts the camera
    /// choice of the bbox capture. A second call stops the camera choice.
    pub fn toggle_bbox(&mut self) {
        match self.bbox {
            BboxMode::Off => {
                self.pause();
                self.bbox = BboxMode::PickCamera;
            }
            BboxMode::PickCamera => self.bbox = BboxMode::Off,
            BboxMode::Dialog(_) => {}
        }
    }

    /// The user clicked a tile during the camera choice. Opens the dialog
    /// with the frame that this tile shows.
    ///
    /// The picture in the dialog has the shape that the grid uses, so the
    /// user draws on the same picture that the tile showed.
    pub fn open_bbox_dialog(&mut self, ctx: &egui::Context, renderer: &VideoRenderer, tile: usize, ratio: VideoRatio) {
        self.pause();
        let Some(tile) = self.tiles.get(tile) else { return };
        let camera_id = self.library.cameras[tile.camera].camera_id.clone();
        let Some(frame) = tile.player.frame() else {
            // Stay in the camera choice, so the user can click a different camera.
            self.message = Some(format!("{camera_id} has no video at this time"));
            return;
        };
        let aspect = ratio.apply(frame.format.display_aspect());
        match renderer.read_picture(frame, Some(aspect)) {
            Some(picture) => {
                let time = self.clock.snapshot().time;
                self.bbox = BboxMode::Dialog(Box::new(BboxDialog::new(ctx, camera_id, time, picture)));
            }
            None => self.message = Some(format!("Cannot read the frame of {camera_id}")),
        }
    }

    pub fn toggle_play(&self) {
        self.clock.toggle();
        self.wake_players();
    }

    pub fn jump(&self, time: Timestamp) {
        self.clock.jump(time);
        self.wake_players();
    }

    /// Jumps to a time of day from the user. See [`resolve_time_of_day`]
    /// for the range and midnight rules.
    pub fn jump_to_time_of_day(&self, time_of_day: Micros) {
        self.jump(resolve_time_of_day(time_of_day, self.library.start, self.library.end));
    }

    /// Pauses playback and asks for a copy of the clock time. The copied
    /// time is the time at which the clock stopped, so it is the time on screen.
    pub fn copy_time_and_pause(&mut self) {
        self.pause();
        self.copy_request = Some(self.clock.snapshot().time.format_hms());
    }

    /// Opens the "go to time" dialog with the current clock time in the text box.
    pub fn open_goto(&mut self) {
        let text = self.clock.snapshot().time.format_hms();
        self.goto = Some(GotoDialog { text, invalid: false, focused: false });
    }

    pub fn jump_by(&self, delta: i64) {
        self.clock.jump_by(delta);
        self.wake_players();
    }

    pub fn set_speed(&self, speed: f64) {
        self.clock.set_speed(speed);
        self.wake_players();
    }

    /// Pauses at the end of the range. Call on each screen refresh.
    pub fn stop_at_end(&self) {
        if self.clock.stop_at_end() {
            self.wake_players();
        }
    }

    /// The pointer is down on the progress bar at `target`. The clock
    /// follows at a limited rate, so the decoders are not flooded.
    /// Returns the delay until the next call is needed, when a jump is held back.
    pub fn scrub_to(&mut self, target: Timestamp) -> Option<Duration> {
        self.scrub.target = Some(target);
        let now = Instant::now();
        match self.scrub.sent {
            Some((sent, _)) if sent == target => None,
            Some((_, at)) if now.duration_since(at) < SCRUB_INTERVAL => Some(SCRUB_INTERVAL - now.duration_since(at)),
            _ => {
                self.scrub.sent = Some((target, now));
                self.jump(target);
                None
            }
        }
    }

    /// The pointer button was released. Makes sure the last position is used.
    pub fn end_scrub(&mut self) {
        if let Some(target) = self.scrub.target.take() {
            if self.scrub.sent.is_none_or(|(sent, _)| sent != target) {
                self.jump(target);
            }
        }
        self.scrub.sent = None;
    }
}
