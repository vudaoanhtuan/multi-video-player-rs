//! The decode thread of one camera.
//!
//! The worker makes a stream of slots in time order: frames, and "black"
//! markers where the camera has no video. It follows the clock. It skips
//! frames when it is late, and it never slows the clock.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{Shared, Slot, Waker};
use crate::clock::{Clock, Snapshot};
use crate::library::{CameraTimeline, Lookup};
use crate::media::ffmpeg::Decoder;
use crate::media::DecodeOptions;
use crate::time::{Micros, Timestamp, MICROS_PER_SEC};

/// Frames decoded ahead of the clock for each camera.
const QUEUE_CAPACITY: usize = 4;
/// Open the next chunk this much real time before the clock reaches it.
/// The spec asks for at least 5 seconds.
const PRELOAD_REAL_SECS: f64 = 6.0;
/// A gap shorter than this keeps the last frame on screen.
const SHORT_GAP: Micros = MICROS_PER_SEC / 2;
/// Real-time lag that starts frame skipping.
const MAX_LAG_SECS: f64 = 0.5;
/// Real time allowed to reach the clock after a jump before frame skipping starts.
const SEEK_GRACE: Duration = Duration::from_secs(2);
/// From this speed, show key frames only when they come often enough.
const KEY_ONLY_MIN_SPEED: f64 = 8.0;
/// "Often enough": real time between two key frames at the current speed.
const KEY_ONLY_MAX_REAL_INTERVAL_SECS: f64 = 0.5;
/// Frame interval used before two frames are decoded.
const DEFAULT_FRAME_INTERVAL: Micros = 40_000;

enum Position {
    /// The chunk for the clock time is not known. True at the start and after a jump.
    Lost,
    /// Decoding the chunk with this index.
    InChunk(usize),
    /// Between chunks. `next` is the next chunk on the timeline, if there is one.
    Gap { next: Option<usize> },
}

/// A chunk that a helper thread opens ahead of time.
struct Preload {
    index: usize,
    result: Receiver<Result<Decoder, String>>,
}

pub(super) struct Worker {
    camera: Arc<CameraTimeline>,
    clock: Arc<Clock>,
    opts: DecodeOptions,
    shared: Arc<Shared>,
    waker: Waker,

    epoch: u64,
    position: Position,
    /// The open decoder and the index of its chunk.
    decoder: Option<(usize, Decoder)>,
    preload: Option<Preload>,
    /// The newest decoded frame. It goes to the queue when the next frame
    /// shows that it is still needed.
    pending: Option<Slot>,
    /// Time of the last decoded frame of the current chunk.
    last_frame_time: Option<Timestamp>,
    /// Where the current chunk was entered.
    entry_time: Timestamp,
    frame_interval: Micros,
    /// The decoder has reached the clock since the last jump.
    synced: bool,
    unsynced_since: Instant,
    /// The decoder is late: skip to key frames until it reaches the clock.
    catching_up: bool,
}

impl Worker {
    pub(super) fn new(
        camera: Arc<CameraTimeline>,
        clock: Arc<Clock>,
        opts: DecodeOptions,
        shared: Arc<Shared>,
        waker: Waker,
    ) -> Self {
        Self {
            camera,
            clock,
            opts,
            shared,
            waker,
            epoch: u64::MAX,
            position: Position::Lost,
            decoder: None,
            preload: None,
            pending: None,
            last_frame_time: None,
            entry_time: Timestamp(0),
            frame_interval: DEFAULT_FRAME_INTERVAL,
            synced: false,
            unsynced_since: Instant::now(),
            catching_up: false,
        }
    }

    pub(super) fn run(mut self) {
        loop {
            // Read the signal count before the clock. An event after this
            // point changes the count, so a later wait returns immediately.
            let seen = {
                let state = self.shared.state.lock().unwrap();
                if state.stop {
                    return;
                }
                state.signal
            };
            let clock = self.clock.snapshot();
            if clock.epoch != self.epoch {
                self.start_epoch(clock.epoch);
            }
            match self.position {
                Position::Lost => self.find_position(&clock),
                Position::InChunk(index) => self.decode_step(index, &clock, seen),
                Position::Gap { next } => self.gap_step(next, &clock, seen),
            }
        }
    }

    /// A jump happened. Drop all work for the old time.
    fn start_epoch(&mut self, epoch: u64) {
        self.epoch = epoch;
        {
            let mut state = self.shared.state.lock().unwrap();
            state.epoch = epoch;
            state.queue.clear();
        }
        self.pending = None;
        self.last_frame_time = None;
        self.catching_up = false;
        self.position = Position::Lost;
    }

    /// Does the timeline lookup for the clock time and opens the chunk at
    /// the correct position.
    fn find_position(&mut self, clock: &Snapshot) {
        match self.camera.find(clock.time) {
            Lookup::Chunk(index) => {
                let start = self.camera.chunks[index].start;
                let opened = self.open_decoder(index).and_then(|decoder| decoder.seek(clock.time.0 - start.0));
                match opened {
                    Ok(()) => {
                        self.position = Position::InChunk(index);
                        self.entry_time = clock.time;
                        self.last_frame_time = None;
                        self.synced = false;
                        self.unsynced_since = Instant::now();
                    }
                    Err(reason) => {
                        // Black until the next chunk. The tile is already black after a jump.
                        log::warn!("{}: {reason}", self.camera.chunks[index].path.display());
                        self.decoder = None;
                        self.position = Position::Gap { next: self.next_index(index) };
                    }
                }
            }
            Lookup::Gap { next } => {
                self.decoder = None;
                self.position = Position::Gap { next };
            }
        }
    }

    fn next_index(&self, index: usize) -> Option<usize> {
        (index + 1 < self.camera.chunks.len()).then_some(index + 1)
    }

    /// Makes `self.decoder` the decoder of chunk `index`. Uses the open
    /// decoder or the preloaded decoder when it is for the same chunk.
    fn open_decoder(&mut self, index: usize) -> Result<&mut Decoder, String> {
        if self.decoder.as_ref().is_none_or(|(open, _)| *open != index) {
            self.decoder = None;
            let decoder = match self.preload.take_if(|p| p.index == index) {
                Some(preload) => preload.result.recv().map_err(|_| "open thread stopped".to_owned())??,
                None => Decoder::open(&self.camera.chunks[index].path, self.opts)?,
            };
            self.decoder = Some((index, decoder));
        }
        Ok(&mut self.decoder.as_mut().unwrap().1)
    }

    /// Media time before a chunk start at which the chunk must be opened.
    /// It uses real time, so it is longer at a high speed.
    fn preload_window(clock: &Snapshot) -> Micros {
        (PRELOAD_REAL_SECS * clock.speed * 1e6) as Micros
    }

    /// Opens chunk `index` on a helper thread and decodes its first frame.
    /// The decode of the current chunk continues during this time.
    fn start_preload(&mut self, index: usize) {
        if self.preload.as_ref().is_some_and(|p| p.index == index) {
            return;
        }
        let (sender, result) = mpsc::channel();
        let path = self.camera.chunks[index].path.clone();
        let opts = self.opts;
        let shared = self.shared.clone();
        let spawned = std::thread::Builder::new().name("open chunk".to_owned()).spawn(move || {
            let opened = Decoder::open(&path, opts).and_then(|mut decoder| {
                decoder.prime()?;
                Ok(decoder)
            });
            let _ = sender.send(opened);
            shared.signal();
        });
        if spawned.is_ok() {
            self.preload = Some(Preload { index, result });
        }
    }

    /// Decodes one frame of the current chunk, or waits when the queue is full.
    fn decode_step(&mut self, index: usize, clock: &Snapshot, seen: u64) {
        if let Some(next) = self.next_index(index) {
            if self.camera.chunks[next].start.0 - clock.time.0 <= Self::preload_window(clock) {
                self.start_preload(next);
            }
        }
        if self.queue_len() >= QUEUE_CAPACITY {
            self.wait(seen, None);
            return;
        }

        let (start, end) = (self.camera.chunks[index].start, self.camera.chunks[index].end);
        let Some((_, decoder)) = self.decoder.as_mut() else {
            self.position = Position::Lost;
            return;
        };
        let frequent_keys = decoder
            .key_interval()
            .is_some_and(|interval| interval as f64 / 1e6 / clock.speed <= KEY_ONLY_MAX_REAL_INTERVAL_SECS);
        let key_only = clock.playing && (self.catching_up || (clock.speed >= KEY_ONLY_MIN_SPEED && frequent_keys));

        match decoder.next_frame(key_only) {
            Ok(Some(decoded)) => {
                let time = start.offset(decoded.pos);
                if time >= end {
                    // The next chunk owns this time.
                    self.finish_chunk(index, false);
                    return;
                }
                if time < start {
                    return;
                }
                if let Some(last) = self.last_frame_time {
                    let delta = time.0 - last.0;
                    if delta > 0 && delta < MICROS_PER_SEC && !key_only {
                        self.frame_interval = delta;
                    }
                }
                self.last_frame_time = Some(time);

                // Use the clock time of this moment, not of the start of the decode.
                let now = self.clock.snapshot();
                if now.epoch != self.epoch {
                    return;
                }
                self.track_lag(time, &now);
                if let Some(previous) = self.pending.take() {
                    // The new frame is also not after the clock: the previous
                    // frame is not the correct frame for any time from now on.
                    if time > now.time {
                        self.push(previous, &now);
                    }
                }
                let slot = Slot { time, frame: Some(Arc::new(decoded.frame)) };
                if key_only {
                    // The next frame can be far away. Show this one now.
                    self.push(slot, &now);
                } else {
                    self.pending = Some(slot);
                }
            }
            Ok(None) => self.finish_chunk(index, false),
            Err(reason) => {
                log::warn!("{}: {reason}", self.camera.chunks[index].path.display());
                self.finish_chunk(index, true);
            }
        }
    }

    /// Decides if the decoder is too far behind the clock.
    fn track_lag(&mut self, frame_time: Timestamp, now: &Snapshot) {
        if frame_time.0 + self.frame_interval >= now.time.0 {
            self.synced = true;
            self.catching_up = false;
        } else if now.playing {
            let lag_secs = (now.time.0 - frame_time.0) as f64 / 1e6 / now.speed;
            let too_late = if self.synced {
                lag_secs > MAX_LAG_SECS
            } else {
                // Directly after a jump the decoder starts at a key frame
                // before the clock. Give it time to reach the clock.
                self.unsynced_since.elapsed() > SEEK_GRACE
            };
            if too_late {
                self.catching_up = true;
            }
        }
    }

    /// The decoder reached the end of the chunk, or it failed.
    fn finish_chunk(&mut self, index: usize, failed: bool) {
        let now = self.clock.snapshot();
        if let Some(last) = self.pending.take() {
            self.push(last, &now);
        }
        let (start, end) = (self.camera.chunks[index].start, self.camera.chunks[index].end);
        // The video ends one frame interval after its last frame. In
        // key-frame-only mode the last frame of the file was read but not
        // decoded, so use the read position. After a failure, the frames
        // after the last decoded frame cannot be shown.
        let last_read = self.decoder.as_ref().and_then(|(_, decoder)| decoder.read_pos()).filter(|_| !failed);
        let last_video = self.last_frame_time.max(last_read.map(|pos| start.offset(pos)));
        let video_end = match last_video {
            Some(last) => last.offset(self.frame_interval).min(end),
            None => self.entry_time.clamp(start, end),
        };
        let next = self.next_index(index);
        // A very short gap keeps the last frame on screen. This prevents a black flash.
        let short_gap = next.is_some_and(|n| self.camera.chunks[n].start.0 - video_end.0 < SHORT_GAP);
        if failed || last_video.is_none() || !short_gap {
            self.push(Slot { time: video_end, frame: None }, &now);
        }
        self.decoder = None;
        self.last_frame_time = None;
        self.position = Position::Gap { next };
    }

    /// Waits for the next chunk and takes it over when it is open.
    fn gap_step(&mut self, next: Option<usize>, clock: &Snapshot, seen: u64) {
        let Some(index) = next else {
            // After the last chunk of this camera.
            self.wait(seen, None);
            return;
        };
        let start = self.camera.chunks[index].start;
        let lead = start.0 - clock.time.0;
        let window = Self::preload_window(clock);
        if lead > window {
            let real = Duration::from_secs_f64((lead - window) as f64 / 1e6 / clock.speed);
            self.wait(seen, clock.playing.then_some(real));
            return;
        }
        if lead < -((clock.speed * 1e6) as Micros) {
            // The clock is already far inside the chunk. Do a normal lookup
            // and seek. Do not decode the chunk from its first frame.
            self.position = Position::Lost;
            return;
        }

        self.start_preload(index);
        let Some(preload) = self.preload.as_ref() else {
            self.fail_chunk(index, "cannot start the open thread");
            return;
        };
        match preload.result.try_recv() {
            Ok(Ok(decoder)) => {
                self.preload = None;
                self.decoder = Some((index, decoder));
                self.position = Position::InChunk(index);
                self.entry_time = start;
                self.last_frame_time = None;
                self.synced = true;
                self.catching_up = false;
            }
            Ok(Err(reason)) => self.fail_chunk(index, &reason),
            Err(TryRecvError::Disconnected) => self.fail_chunk(index, "open thread stopped"),
            // The helper thread sends a signal when it is done.
            Err(TryRecvError::Empty) => self.wait(seen, None),
        }
    }

    /// A chunk cannot be opened: black from its start until the next chunk.
    fn fail_chunk(&mut self, index: usize, reason: &str) {
        log::warn!("{}: {reason}", self.camera.chunks[index].path.display());
        self.preload = None;
        let now = self.clock.snapshot();
        self.push(Slot { time: self.camera.chunks[index].start, frame: None }, &now);
        self.position = Position::Gap { next: self.next_index(index) };
    }

    fn queue_len(&self) -> usize {
        self.shared.state.lock().unwrap().queue.len()
    }

    fn push(&self, slot: Slot, now: &Snapshot) {
        let time = slot.time;
        let was_empty = {
            let mut state = self.shared.state.lock().unwrap();
            if state.epoch != self.epoch {
                return;
            }
            let was_empty = state.queue.is_empty();
            state.queue.push_back(slot);
            was_empty
        };
        // The UI knows the due time of the slots that it saw in the queue.
        // It must be told about a slot that arrives in an empty queue.
        if was_empty {
            let lead = time.0 - now.time.0;
            if lead <= 0 {
                (self.waker)(Duration::ZERO);
            } else if now.playing {
                (self.waker)(Duration::from_secs_f64(lead as f64 / 1e6 / now.speed));
            }
        }
    }

    /// Sleeps until an event after `seen`, or until the timeout.
    fn wait(&self, seen: u64, timeout: Option<Duration>) {
        let state = self.shared.state.lock().unwrap();
        let idle = |state: &mut super::SharedState| state.signal == seen && !state.stop;
        match timeout {
            Some(timeout) => drop(self.shared.cond.wait_timeout_while(state, timeout, idle).unwrap()),
            None => drop(self.shared.cond.wait_while(state, idle).unwrap()),
        }
    }
}
