//! The one shared playback clock.
//!
//! The clock is the only source of time. Cameras follow the clock. The clock
//! never waits for a camera. See `specs/synced-playback.md`.

use std::sync::Mutex;
use std::time::Instant;

use crate::time::Timestamp;

/// Playback speeds offered by the speed menu: 1x to 20x. The digit keys set
/// them, see `ui::key_speed`.
pub const SPEEDS: [f64; 20] = [
    1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0, 20.0,
];
pub const DEFAULT_SPEED: f64 = 1.0;

/// The clock state at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub time: Timestamp,
    pub playing: bool,
    pub speed: f64,
    /// Changes at each jump. A frame that was decoded for an older epoch must not be shown.
    pub epoch: u64,
}

struct State {
    base_time: Timestamp,
    base_instant: Instant,
    playing: bool,
    speed: f64,
    epoch: u64,
}

impl State {
    fn time_at(&self, now: Instant, end: Timestamp) -> Timestamp {
        if !self.playing {
            return self.base_time;
        }
        let elapsed = now.saturating_duration_since(self.base_instant).as_secs_f64();
        let advanced = self.base_time.offset((elapsed * self.speed * 1e6) as i64);
        advanced.min(end)
    }

    /// Makes the current time the new base, so that a speed or state change
    /// does not move the clock.
    fn rebase(&mut self, now: Instant, end: Timestamp) {
        self.base_time = self.time_at(now, end);
        self.base_instant = now;
    }
}

pub struct Clock {
    start: Timestamp,
    end: Timestamp,
    state: Mutex<State>,
}

impl Clock {
    /// A paused clock at the start of the range, at the default speed.
    pub fn new(start: Timestamp, end: Timestamp) -> Self {
        Self {
            start,
            end,
            state: Mutex::new(State {
                base_time: start,
                base_instant: Instant::now(),
                playing: false,
                speed: DEFAULT_SPEED,
                epoch: 0,
            }),
        }
    }

    pub fn start(&self) -> Timestamp {
        self.start
    }

    pub fn end(&self) -> Timestamp {
        self.end
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot_at(Instant::now())
    }

    pub fn snapshot_at(&self, now: Instant) -> Snapshot {
        let s = self.state.lock().unwrap();
        Snapshot { time: s.time_at(now, self.end), playing: s.playing, speed: s.speed, epoch: s.epoch }
    }

    /// Pauses the clock when it has reached the end of the range.
    /// Returns `true` when this call paused the clock.
    pub fn stop_at_end(&self) -> bool {
        let now = Instant::now();
        let mut s = self.state.lock().unwrap();
        if s.playing && s.time_at(now, self.end) >= self.end {
            s.base_time = self.end;
            s.playing = false;
            return true;
        }
        false
    }

    /// Starts the clock. At the end of the range, play starts again from the start.
    pub fn play(&self) {
        let now = Instant::now();
        let mut s = self.state.lock().unwrap();
        if s.playing {
            return;
        }
        if s.base_time >= self.end {
            s.base_time = self.start;
            s.epoch += 1;
        }
        s.base_instant = now;
        s.playing = true;
    }

    pub fn pause(&self) {
        let now = Instant::now();
        let mut s = self.state.lock().unwrap();
        s.rebase(now, self.end);
        s.playing = false;
    }

    pub fn toggle(&self) {
        if self.snapshot().playing {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn set_speed(&self, speed: f64) {
        let now = Instant::now();
        let mut s = self.state.lock().unwrap();
        s.rebase(now, self.end);
        s.speed = speed;
    }

    /// Sets the clock to a new time. The play or pause state stays the same.
    pub fn jump(&self, time: Timestamp) {
        let now = Instant::now();
        let mut s = self.state.lock().unwrap();
        s.base_time = time.clamp(self.start, self.end);
        s.base_instant = now;
        s.epoch += 1;
    }

    /// Jumps by a signed number of microseconds from the current time.
    pub fn jump_by(&self, delta: i64) {
        let time = self.snapshot().time.offset(delta);
        self.jump(time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn clock() -> Clock {
        Clock::new(Timestamp(0), Timestamp(100_000_000))
    }

    fn at(clock: &Clock, base: Instant, secs: f64) -> Timestamp {
        clock.snapshot_at(base + Duration::from_secs_f64(secs)).time
    }

    #[test]
    fn starts_paused_at_range_start() {
        let c = clock();
        let s = c.snapshot();
        assert_eq!((s.time, s.playing, s.speed), (Timestamp(0), false, 1.0));
    }

    #[test]
    fn moves_by_real_time_times_speed() {
        let c = clock();
        c.set_speed(4.0);
        c.play();
        let base = c.state.lock().unwrap().base_instant;
        assert_eq!(at(&c, base, 1.0), Timestamp(4_000_000));
        assert_eq!(at(&c, base, 2.5), Timestamp(10_000_000));
    }

    #[test]
    fn stops_at_range_end_and_restarts_from_start() {
        let c = clock();
        c.jump(Timestamp(99_999_999));
        c.play();
        let base = c.state.lock().unwrap().base_instant;
        assert_eq!(at(&c, base, 5.0), Timestamp(100_000_000));
        std::thread::sleep(Duration::from_millis(2));
        assert!(c.stop_at_end());
        let s = c.snapshot();
        assert_eq!((s.time, s.playing), (Timestamp(100_000_000), false));

        let epoch = s.epoch;
        c.play();
        let s = c.snapshot();
        assert!(s.playing && s.time.0 < 1_000_000);
        assert_eq!(s.epoch, epoch + 1);
    }

    #[test]
    fn jump_keeps_play_state_and_is_limited_to_range() {
        let c = clock();
        c.jump(Timestamp(50_000_000));
        assert_eq!(c.snapshot().time, Timestamp(50_000_000));
        assert!(!c.snapshot().playing);
        c.jump(Timestamp(-5));
        assert_eq!(c.snapshot().time, Timestamp(0));
        c.jump(Timestamp(500_000_000));
        assert_eq!(c.snapshot().time, Timestamp(100_000_000));
        c.jump(Timestamp(10_000_000));
        c.play();
        c.jump_by(10_000_000);
        let s = c.snapshot();
        assert!(s.playing && s.time.0 >= 20_000_000 && s.time.0 < 21_000_000);
    }

    #[test]
    fn speed_change_does_not_move_the_clock() {
        let c = clock();
        c.jump(Timestamp(30_000_000));
        c.set_speed(16.0);
        assert_eq!(c.snapshot().time, Timestamp(30_000_000));
        assert_eq!(c.snapshot().speed, 16.0);
    }
}
