//! End-to-end tests of scan, decode, and synced playback with real video
//! files and no window.
//!
//! The tests make small videos with the `ffmpeg` command. When the command
//! is not installed, the tests print a note and pass.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use video_player::clock::Clock;
use video_player::library::{scan_folder, Library, MediaProbe};
use video_player::media::ffmpeg::{Decoder, FfmpegProbe};
use video_player::media::DecodeOptions;
use video_player::playback::{CameraPlayer, Waker};
use video_player::time::{Timestamp, MICROS_PER_SEC};

const FRAME: i64 = 40_000; // 25 frames each second
const MS: i64 = 1_000;

/// Camera `a`: 10:00:00 (4 s), 10:00:04 (4 s), then a gap, 10:00:12 (4 s).
/// Camera `b`: 10:00:00 (16 s). Key frames are 1 second apart.
fn fixtures() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("playback-fixtures");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        for (name, seconds) in [
            ("a_20261019T100000.mkv", 4),
            ("a_20261019T100004.mkv", 4),
            ("a_20261019T100012.mkv", 4),
            ("b_20261019T100000.mkv", 16),
        ] {
            let status = Command::new("ffmpeg")
                .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi"])
                .args(["-i", "testsrc2=size=320x240:rate=25", "-t", &seconds.to_string()])
                .args(["-an", "-pix_fmt", "yuv420p", "-c:v", "libx264", "-preset", "ultrafast", "-g", "25", "-bf", "0"])
                .arg(dir.join(name))
                .status();
            if !status.is_ok_and(|s| s.success()) {
                eprintln!("note: the ffmpeg command is not available, playback tests are skipped");
                return None;
            }
        }
        std::fs::write(dir.join("notes.txt"), "not a video").ok()?;
        std::fs::write(dir.join("a_20261019T100020.mkv"), "not a video").ok()?;
        Some(dir)
    })
    .as_deref()
}

fn ts(text: &str) -> Timestamp {
    Timestamp::parse_compact(text).unwrap()
}

fn library(dir: &Path) -> Library {
    scan_folder(dir, &FfmpegProbe).unwrap().library.unwrap()
}

/// Software decode, and hardware decode where the platform has it.
fn decode_modes() -> Vec<(&'static str, DecodeOptions)> {
    #[allow(unused_mut)]
    let mut modes = vec![("software", DecodeOptions { hw: None })];
    #[cfg(target_os = "macos")]
    modes.push((
        "hardware",
        DecodeOptions {
            hw: Some(video_player::media::HwDecode {
                device_type: ffmpeg_sys_next::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
                zero_copy_nv12: true,
                zero_copy_p010: true,
            }),
        },
    ));
    modes
}

struct Rig {
    clock: Arc<Clock>,
    players: Vec<CameraPlayer>,
}

impl Rig {
    fn new(library: &Library, opts: DecodeOptions) -> Self {
        let clock = Arc::new(Clock::new(library.start, library.end));
        let waker: Waker = Arc::new(|_| {});
        let players = library
            .cameras
            .iter()
            .map(|camera| CameraPlayer::start(camera.clone(), clock.clone(), opts, waker.clone()))
            .collect();
        Self { clock, players }
    }

    fn wake(&self) {
        self.players.iter().for_each(CameraPlayer::wake);
    }

    /// One "screen refresh": returns the clock time and the frame time of each camera.
    fn refresh(&mut self) -> (Timestamp, Vec<Option<Timestamp>>) {
        let clock = self.clock.snapshot();
        let frames = self
            .players
            .iter_mut()
            .map(|player| {
                player.update(&clock);
                player.frame_time()
            })
            .collect();
        (clock.time, frames)
    }

    /// Refreshes until `done` is true for the frame times. Panics after 5 seconds.
    fn wait_until(&mut self, what: &str, done: impl Fn(&[Option<Timestamp>]) -> bool) -> Vec<Option<Timestamp>> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let (_, frames) = self.refresh();
            if done(&frames) {
                return frames;
            }
            assert!(Instant::now() < deadline, "timeout: {what}, frames: {frames:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[test]
fn probe_reads_the_stored_length() {
    let Some(dir) = fixtures() else { return };
    let length = FfmpegProbe.video_length(&dir.join("a_20261019T100000.mkv")).unwrap().unwrap();
    assert!((length - 4 * MICROS_PER_SEC).abs() < 100 * MS, "{length}");
    assert!(FfmpegProbe.video_length(&dir.join("notes.txt")).is_err());
}

#[test]
fn scan_builds_the_timelines() {
    let Some(dir) = fixtures() else { return };
    let scan = scan_folder(dir, &FfmpegProbe).unwrap();
    assert_eq!(scan.skipped, 2); // notes.txt and the broken mkv
    let library = scan.library.unwrap();
    let chunks: Vec<_> = library.cameras.iter().map(|c| (&*c.camera_id, c.chunks.len())).collect();
    assert_eq!(chunks, [("a", 3), ("b", 1)]);
    assert_eq!(library.start, ts("20261019T100000"));
    assert_eq!(library.end, ts("20261019T100016"));
}

#[test]
fn decoder_seek_starts_at_a_key_frame_before_the_target() {
    let Some(dir) = fixtures() else { return };
    for (mode, opts) in decode_modes() {
        let mut decoder = Decoder::open(&dir.join("b_20261019T100000.mkv"), opts).unwrap();
        decoder.seek(2_500 * MS).unwrap();
        let positions: Vec<i64> = (0..20).map(|_| decoder.next_frame(false).unwrap().unwrap().pos).collect();
        assert_eq!(positions[0], 2 * MICROS_PER_SEC, "{mode}");
        assert!(positions.windows(2).all(|w| w[1] - w[0] == FRAME), "{mode}: {positions:?}");

        // Key-frame-only mode gives one frame for each second of video.
        decoder.seek(0).unwrap();
        let keys: Vec<i64> = (0..5).map(|_| decoder.next_frame(true).unwrap().unwrap().pos).collect();
        assert_eq!(keys, [0, 1, 2, 3, 4].map(|s| s * MICROS_PER_SEC), "{mode}");

        // The end of the file.
        decoder.seek(15_900 * MS).unwrap();
        let mut last = None;
        while let Some(frame) = decoder.next_frame(false).unwrap() {
            last = Some(frame.pos);
        }
        assert_eq!(last, Some(16 * MICROS_PER_SEC - FRAME), "{mode}");
    }
}

#[test]
fn paused_jump_shows_the_exact_frame() {
    let Some(dir) = fixtures() else { return };
    let library = library(dir);
    for (mode, opts) in decode_modes() {
        let mut rig = Rig::new(&library, opts);
        // 10:00:06.510 is inside the second chunk of camera `a`.
        let target = ts("20261019T100006").offset(510 * MS);
        rig.clock.jump(target);
        rig.wake();
        let frames = rig.wait_until("frames after jump", |f| f.iter().all(Option::is_some));
        // The latest frame that is not after the clock: 6.480 s.
        let expected = ts("20261019T100006").offset(480 * MS);
        assert_eq!(frames, [Some(expected), Some(expected)], "{mode}");

        // A later refresh does not replace the frame while paused.
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(rig.refresh().1, [Some(expected), Some(expected)], "{mode}");
    }
}

#[test]
fn gap_is_black_and_other_cameras_continue() {
    let Some(dir) = fixtures() else { return };
    let library = library(dir);
    let mut rig = Rig::new(&library, DecodeOptions { hw: None });
    rig.clock.jump(ts("20261019T100009"));
    rig.wake();
    let frames = rig.wait_until("camera b after jump", |f| f[1].is_some());
    assert_eq!(frames[0], None);
    std::thread::sleep(Duration::from_millis(200));
    let (_, frames) = rig.refresh();
    assert_eq!(frames, [None, Some(ts("20261019T100009"))]);
}

#[test]
fn plays_in_sync_through_a_chunk_change_and_into_a_gap() {
    let Some(dir) = fixtures() else { return };
    let library = library(dir);
    for (mode, opts) in decode_modes() {
        let mut rig = Rig::new(&library, opts);
        rig.clock.jump(ts("20261019T100003"));
        rig.wake();
        rig.wait_until("first frames", |f| f.iter().all(Option::is_some));
        rig.clock.play();
        rig.wake();

        let chunk_change = ts("20261019T100004");
        let gap_start = ts("20261019T100008");
        let started = Instant::now();
        let mut seen_second_chunk = false;
        // Play from 10:00:03 to 10:00:08.5 at 2x.
        rig.clock.set_speed(2.0);
        loop {
            let (now, frames) = rig.refresh();
            if now >= gap_start.offset(500 * MS) {
                // Camera `a` is in its gap: black. Camera `b` plays.
                assert_eq!(frames[0], None, "{mode}");
                assert!(frames[1].is_some(), "{mode}");
                break;
            }
            // Leave a margin around the gap start, where black is correct.
            if now < gap_start.offset(-FRAME) {
                for (camera, frame) in frames.iter().enumerate() {
                    let frame = frame.unwrap_or_else(|| panic!("{mode}: camera {camera} is black at {now:?}"));
                    let lag = now.0 - frame.0;
                    assert!((0..150 * MS).contains(&lag), "{mode}: camera {camera} lag {lag} us at {now:?}");
                }
                seen_second_chunk |= frames[0].is_some_and(|f| f >= chunk_change);
            }
            std::thread::sleep(Duration::from_millis(4));
        }
        assert!(seen_second_chunk, "{mode}");

        // The clock moved 5.5 seconds of video in 2.75 seconds of real time.
        let real = started.elapsed().as_secs_f64();
        assert!((real - 2.75).abs() < 0.25, "{mode}: {real}");
    }
}

#[test]
fn high_speed_skips_frames_and_keeps_the_clock_rate() {
    let Some(dir) = fixtures() else { return };
    let library = library(dir);
    let mut rig = Rig::new(&library, DecodeOptions { hw: None });
    rig.wait_until("first frames", |f| f.iter().all(Option::is_some));
    rig.clock.set_speed(16.0);
    rig.clock.play();
    rig.wake();
    let started = Instant::now();
    let mut worst_lag = 0;
    while started.elapsed() < Duration::from_millis(700) {
        let (now, frames) = rig.refresh();
        if let Some(frame) = frames[1] {
            worst_lag = worst_lag.max(now.0 - frame.0);
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    let (now, _) = rig.refresh();
    let moved = (now.0 - library.start.0) as f64 / 1e6;
    let expected = started.elapsed().as_secs_f64() * 16.0;
    assert!((moved - expected).abs() < 0.5, "clock moved {moved} s, expected {expected} s");
    // Key frames are 1 second apart. The frame on screen is never much older than that.
    assert!(worst_lag < 2 * MICROS_PER_SEC, "worst lag {worst_lag} us");
}

#[test]
fn clock_stops_at_the_end_of_the_range() {
    let Some(dir) = fixtures() else { return };
    let library = library(dir);
    let mut rig = Rig::new(&library, DecodeOptions { hw: None });
    rig.clock.jump(library.end.offset(-100 * MS));
    rig.clock.play();
    rig.wake();
    std::thread::sleep(Duration::from_millis(250));
    assert!(rig.clock.stop_at_end());
    let (now, frames) = rig.refresh();
    assert_eq!(now, library.end);
    assert_eq!(frames, [None, None]);
    assert!(!rig.clock.snapshot().playing);
}
