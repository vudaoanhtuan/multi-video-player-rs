//! Synced playback. See `specs/synced-playback.md`.
//!
//! Each shown camera has one [`CameraPlayer`]. Its worker thread decodes
//! ahead of the clock and puts time-stamped slots in a small queue. On each
//! screen refresh the UI thread calls [`CameraPlayer::update`], which takes
//! the latest slot that is not after the clock time. The UI thread never
//! decodes and never waits for a worker.

mod worker;

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::clock::{Clock, Snapshot};
use crate::library::CameraTimeline;
use crate::media::{DecodeOptions, VideoFrame};
use crate::time::Timestamp;

/// Asks the UI for a screen refresh after a delay. Workers call it when a
/// new slot is ready.
pub type Waker = Arc<dyn Fn(Duration) + Send + Sync>;

/// The default lowest speed at which a camera shows key frames only.
pub const DEFAULT_KEY_ONLY_MIN_SPEED: f64 = 4.0;

/// What a tile shows from `time` until the time of the next slot.
pub struct Slot {
    pub time: Timestamp,
    /// `None` is black.
    pub frame: Option<Arc<VideoFrame>>,
}

struct SharedState {
    /// Clock epoch of the slots in `queue`.
    epoch: u64,
    /// Slots in time order.
    queue: VecDeque<Slot>,
    /// Goes up at each event that the worker must look at.
    signal: u64,
    stop: bool,
    /// From this speed, the worker decodes key frames only when they come often enough.
    key_only_min_speed: f64,
}

struct Shared {
    state: Mutex<SharedState>,
    cond: Condvar,
}

impl Shared {
    fn signal(&self) {
        self.state.lock().unwrap().signal += 1;
        self.cond.notify_all();
    }
}

/// Plays one camera. Drop it to stop the decode and close the files.
pub struct CameraPlayer {
    shared: Arc<Shared>,
    epoch: u64,
    current: Option<Slot>,
}

impl CameraPlayer {
    /// Starts the worker thread. The camera joins at the current clock time.
    pub fn start(camera: Arc<CameraTimeline>, clock: Arc<Clock>, opts: DecodeOptions, waker: Waker) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(SharedState {
                epoch: u64::MAX,
                queue: VecDeque::new(),
                signal: 0,
                stop: false,
                key_only_min_speed: DEFAULT_KEY_ONLY_MIN_SPEED,
            }),
            cond: Condvar::new(),
        });
        let name = format!("decode {}", camera.camera_id);
        let worker = worker::Worker::new(camera, clock, opts, shared.clone(), waker);
        std::thread::Builder::new()
            .name(name)
            .spawn(move || worker.run())
            .expect("cannot start a decode thread");
        Self { shared, epoch: u64::MAX, current: None }
    }

    /// Moves to the slot for the clock time. Returns the time of the next
    /// slot in the queue, which is when the tile must change again.
    pub fn update(&mut self, clock: &Snapshot) -> Option<Timestamp> {
        if self.epoch != clock.epoch {
            // After a jump, show black until the frame for the new time is ready.
            self.epoch = clock.epoch;
            self.current = None;
        }
        let mut state = self.shared.state.lock().unwrap();
        if state.epoch != clock.epoch {
            // The worker has not seen the jump yet. Its queue holds old frames.
            return None;
        }
        let mut popped = false;
        while state.queue.front().is_some_and(|slot| slot.time <= clock.time) {
            // Slots that are already replaced by a later slot are skipped here.
            self.current = state.queue.pop_front();
            popped = true;
        }
        let next_due = state.queue.front().map(|slot| slot.time);
        if popped {
            state.signal += 1;
            drop(state);
            self.shared.cond.notify_all();
        }
        next_due
    }

    /// The frame to show now. `None` is black.
    pub fn frame(&self) -> Option<&Arc<VideoFrame>> {
        self.current.as_ref()?.frame.as_ref()
    }

    /// Time of the slot on screen. For tests and diagnostics.
    pub fn frame_time(&self) -> Option<Timestamp> {
        self.current.as_ref().filter(|slot| slot.frame.is_some()).map(|slot| slot.time)
    }

    /// Sets the lowest speed at which the worker decodes key frames only.
    /// The change applies to the next frame that the worker decodes.
    pub fn set_key_only_min_speed(&self, speed: f64) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.key_only_min_speed = speed;
            state.signal += 1;
        }
        self.shared.cond.notify_all();
    }

    /// Tells the worker that the clock changed (play, pause, speed, jump).
    pub fn wake(&self) {
        self.shared.signal();
    }
}

impl Drop for CameraPlayer {
    fn drop(&mut self) {
        // Do not join: the worker can be inside a slow file open. It stops
        // at its next check and then closes its files.
        self.shared.state.lock().unwrap().stop = true;
        self.shared.cond.notify_all();
    }
}
