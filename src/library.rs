//! Folder scan and camera timelines.
//!
//! See `specs/folder-scan-and-timeline.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::time::{Micros, Timestamp, MICROS_PER_SEC};

/// Length used for the last chunk of a camera when the file has no stored length.
pub const DEFAULT_CHUNK_LENGTH: Micros = 30 * 60 * MICROS_PER_SEC;

/// One video file.
#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub camera_id: Arc<str>,
    pub path: PathBuf,
    pub start: Timestamp,
    /// Exclusive end.
    pub end: Timestamp,
}

/// All chunks of one camera, sorted by start time.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraTimeline {
    pub camera_id: Arc<str>,
    pub chunks: Vec<Chunk>,
}

/// Result of [`CameraTimeline::find`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// Index of the chunk where `start <= time < end`.
    Chunk(usize),
    /// No chunk at this time. `next` is the index of the next chunk, if there is one.
    Gap { next: Option<usize> },
}

impl CameraTimeline {
    pub fn find(&self, time: Timestamp) -> Lookup {
        // Index of the first chunk that starts after `time`.
        let after = self.chunks.partition_point(|c| c.start <= time);
        if after > 0 && time < self.chunks[after - 1].end {
            return Lookup::Chunk(after - 1);
        }
        Lookup::Gap { next: (after < self.chunks.len()).then_some(after) }
    }
}

/// All cameras of one folder plus the total time range.
#[derive(Clone, Debug, PartialEq)]
pub struct Library {
    /// Sorted by camera id.
    pub cameras: Vec<Arc<CameraTimeline>>,
    /// Earliest chunk start across all cameras.
    pub start: Timestamp,
    /// Latest chunk end across all cameras.
    pub end: Timestamp,
}

/// Result of a folder scan.
#[derive(Debug)]
pub struct ScanResult {
    /// `None` when the folder has no usable video file.
    pub library: Option<Library>,
    /// Number of files that were skipped.
    pub skipped: usize,
}

/// Reads the stored video length of a file without decoding it.
pub trait MediaProbe: Sync {
    /// `Ok(Some(length))`: the file has a video track and a stored length.
    /// `Ok(None)`: the file has a video track but no stored length.
    /// `Err(_)`: the file cannot be opened or has no video track.
    fn video_length(&self, path: &Path) -> Result<Option<Micros>, String>;
}

/// Splits `<camera-id>_<start-time>.mkv` into camera id and start time.
/// The camera id is all text before the last underscore.
pub fn parse_file_name(name: &str) -> Option<(&str, Timestamp)> {
    let dot = name.rfind('.')?;
    if !name[dot + 1..].eq_ignore_ascii_case("mkv") {
        return None;
    }
    let stem = &name[..dot];
    let underscore = stem.rfind('_')?;
    let camera_id = &stem[..underscore];
    if camera_id.is_empty() {
        return None;
    }
    Some((camera_id, Timestamp::parse_compact(&stem[underscore + 1..])?))
}

/// A file that passed the file name check, with its probed length.
#[derive(Clone, Debug)]
pub struct ProbedFile {
    pub camera_id: String,
    pub path: PathBuf,
    pub start: Timestamp,
    pub length: Option<Micros>,
}

/// Scans the files directly inside `dir`. Folders inside `dir` are not read.
pub fn scan_folder(dir: &Path, probe: &dyn MediaProbe) -> std::io::Result<ScanResult> {
    let mut names: Vec<(String, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        // Hidden system files (for example `.DS_Store`) are not user files.
        // Ignore them and do not count them as skipped.
        if name.starts_with('.') {
            continue;
        }
        names.push((name, path));
    }
    // "First by file name order" needs a stable order.
    names.sort();

    let mut skipped = 0;
    let mut candidates: Vec<(String, PathBuf, Timestamp)> = Vec::new();
    for (name, path) in names {
        match parse_file_name(&name) {
            Some((camera, start)) => candidates.push((camera.to_owned(), path, start)),
            None => skipped += 1,
        }
    }

    // Probe in parallel. A folder can hold hundreds of files on a slow disk.
    type ProbeResult = Result<Option<Micros>, String>;
    let results: Vec<Mutex<Option<ProbeResult>>> = candidates.iter().map(|_| Mutex::new(None)).collect();
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
    std::thread::scope(|scope| {
        for _ in 0..workers.min(candidates.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((_, path, _)) = candidates.get(i) else { break };
                *results[i].lock().unwrap() = Some(probe.video_length(path));
            });
        }
    });

    let mut files = Vec::with_capacity(candidates.len());
    for ((camera_id, path, start), result) in candidates.into_iter().zip(results) {
        match result.into_inner().unwrap() {
            Some(Ok(length)) => files.push(ProbedFile { camera_id, path, start, length }),
            Some(Err(reason)) => {
                log::warn!("skipped {}: {reason}", path.display());
                skipped += 1;
            }
            None => skipped += 1,
        }
    }

    let (library, duplicates) = build_library(files);
    Ok(ScanResult { library, skipped: skipped + duplicates })
}

/// Builds the camera timelines. `files` must be in file name order.
/// Returns the library and the number of files skipped as duplicates.
pub fn build_library(files: Vec<ProbedFile>) -> (Option<Library>, usize) {
    let mut skipped = 0;
    let mut by_camera: BTreeMap<String, Vec<ProbedFile>> = BTreeMap::new();
    for file in files {
        by_camera.entry(file.camera_id.clone()).or_default().push(file);
    }

    let mut cameras = Vec::with_capacity(by_camera.len());
    for (camera_id, mut files) in by_camera {
        // A stable sort keeps the file name order for equal start times.
        files.sort_by_key(|f| f.start);
        let before = files.len();
        files.dedup_by_key(|f| f.start);
        skipped += before - files.len();

        let camera_id: Arc<str> = camera_id.into();
        let mut chunks = Vec::with_capacity(files.len());
        for (i, file) in files.iter().enumerate() {
            let next_start = files.get(i + 1).map(|f| f.start);
            let mut end = match file.length.filter(|len| *len > 0) {
                Some(length) => file.start.offset(length),
                None => next_start.unwrap_or(file.start.offset(DEFAULT_CHUNK_LENGTH)),
            };
            if let Some(next_start) = next_start {
                end = end.min(next_start);
            }
            chunks.push(Chunk {
                camera_id: camera_id.clone(),
                path: file.path.clone(),
                start: file.start,
                end,
            });
        }
        cameras.push(Arc::new(CameraTimeline { camera_id, chunks }));
    }

    let all_chunks = || cameras.iter().flat_map(|c| c.chunks.iter());
    let (Some(start), Some(end)) = (all_chunks().map(|c| c.start).min(), all_chunks().map(|c| c.end).max())
    else {
        return (None, skipped);
    };
    (Some(Library { cameras, start, end }), skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Micros = 60 * MICROS_PER_SEC;

    fn ts(text: &str) -> Timestamp {
        Timestamp::parse_compact(text).unwrap()
    }

    fn file(camera: &str, start: &str, length: Option<Micros>) -> ProbedFile {
        ProbedFile {
            camera_id: camera.into(),
            path: PathBuf::from(format!("{camera}_{start}.mkv")),
            start: ts(start),
            length,
        }
    }

    #[test]
    fn file_name_format() {
        let (camera, start) = parse_file_name("floor-1-camera-2_20261019T213000.mkv").unwrap();
        assert_eq!(camera, "floor-1-camera-2");
        assert_eq!(start, ts("20261019T213000"));

        // The camera id is all text before the last underscore.
        let (camera, _) = parse_file_name("north_gate_cam_20261019T213000.mkv").unwrap();
        assert_eq!(camera, "north_gate_cam");

        for bad in [
            "notes.txt",
            "camera_20261019T213000.mp4",
            "camera-20261019T213000.mkv",
            "_20261019T213000.mkv",
            "camera_20261019.mkv",
            "camera_20261019T213000",
        ] {
            assert!(parse_file_name(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn three_cameras_and_total_range() {
        let files = vec![
            file("a", "20261019T213000", Some(30 * MIN)),
            file("a", "20261019T220000", Some(30 * MIN)),
            file("b", "20261019T211500", Some(30 * MIN)),
            file("c", "20261019T220000", Some(45 * MIN)),
            file("c", "20261019T213000", Some(30 * MIN)),
            file("c", "20261019T210000", Some(30 * MIN)),
        ];
        let (library, skipped) = build_library(files);
        let library = library.unwrap();
        assert_eq!(skipped, 0);
        let counts: Vec<_> = library.cameras.iter().map(|c| (&*c.camera_id, c.chunks.len())).collect();
        assert_eq!(counts, [("a", 2), ("b", 1), ("c", 3)]);
        assert_eq!(library.start, ts("20261019T210000"));
        assert_eq!(library.end, ts("20261019T224500"));
        // Chunks are sorted by start.
        let c = &library.cameras[2];
        assert!(c.chunks.windows(2).all(|w| w[0].start < w[1].start));
    }

    #[test]
    fn missing_length_uses_next_start_or_default() {
        let files = vec![file("a", "20261019T213000", None), file("a", "20261019T215500", None)];
        let library = build_library(files).0.unwrap();
        let chunks = &library.cameras[0].chunks;
        assert_eq!(chunks[0].end, ts("20261019T215500"));
        assert_eq!(chunks[1].end, ts("20261019T222500"));
    }

    #[test]
    fn overlap_is_cut_at_next_start() {
        let files = vec![
            file("a", "20261019T213000", Some(40 * MIN)),
            file("a", "20261019T220000", Some(30 * MIN)),
        ];
        let library = build_library(files).0.unwrap();
        assert_eq!(library.cameras[0].chunks[0].end, ts("20261019T220000"));
    }

    #[test]
    fn duplicate_start_keeps_first_file() {
        let mut first = file("a", "20261019T213000", Some(10 * MIN));
        first.path = PathBuf::from("a_20261019T213000.MKV");
        let second = file("a", "20261019T213000", Some(20 * MIN));
        let (library, skipped) = build_library(vec![first.clone(), second]);
        assert_eq!(skipped, 1);
        let chunks = &library.unwrap().cameras[0].chunks;
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].path, first.path);
    }

    #[test]
    fn chunk_can_pass_midnight() {
        let files = vec![file("a", "20261019T235000", Some(30 * MIN))];
        let library = build_library(files).0.unwrap();
        assert_eq!(library.end, ts("20261020T002000"));
        assert_eq!(library.cameras[0].find(ts("20261020T001000")), Lookup::Chunk(0));
    }

    #[test]
    fn empty_input_gives_no_library() {
        assert_eq!(build_library(Vec::new()), (None, 0));
    }

    #[test]
    fn find_chunk_and_gap() {
        let files = vec![
            file("a", "20261019T213000", Some(10 * MIN)),
            file("a", "20261019T220000", Some(10 * MIN)),
        ];
        let library = build_library(files).0.unwrap();
        let camera = &library.cameras[0];
        assert_eq!(camera.find(ts("20261019T212959")), Lookup::Gap { next: Some(0) });
        assert_eq!(camera.find(ts("20261019T213000")), Lookup::Chunk(0));
        assert_eq!(camera.find(ts("20261019T213959")), Lookup::Chunk(0));
        // The end is exclusive.
        assert_eq!(camera.find(ts("20261019T214000")), Lookup::Gap { next: Some(1) });
        assert_eq!(camera.find(ts("20261019T220500")), Lookup::Chunk(1));
        assert_eq!(camera.find(ts("20261019T221000")), Lookup::Gap { next: None });
    }

    struct FakeProbe;
    impl MediaProbe for FakeProbe {
        fn video_length(&self, path: &Path) -> Result<Option<Micros>, String> {
            if path.to_string_lossy().contains("broken") {
                Err("cannot open".into())
            } else {
                Ok(Some(5 * MIN))
            }
        }
    }

    #[test]
    fn scan_skips_and_counts_bad_files() {
        let dir = std::env::temp_dir().join(format!("vp-scan-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cam3_20261019T213000.mkv")).unwrap(); // a folder, not a file
        for name in [
            "cam1_20261019T213000.mkv",
            "cam1_20261019T214000.mkv",
            "cam2_20261019T213000.mkv",
            "broken_20261019T213000.mkv",
            "readme.txt",
            "cam1_bad-time.mkv",
            ".DS_Store",
        ] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let result = scan_folder(&dir, &FakeProbe).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        let library = result.library.unwrap();
        assert_eq!(result.skipped, 3); // broken file, readme.txt, bad time
        assert_eq!(library.cameras.len(), 2);
        assert_eq!(library.cameras[0].chunks.len(), 2);
        assert_eq!(library.start, ts("20261019T213000"));
        assert_eq!(library.end, ts("20261019T214500"));
    }
}
