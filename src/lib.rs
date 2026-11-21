//! Multi-camera synced video player. See `specs/README.md`.
//!
//! Module map:
//! - [`time`], [`library`], [`clock`]: pure logic with no platform or media dependency.
//! - [`media`]: frame types and the FFmpeg decode backend.
//! - [`playback`]: one decode thread for each shown camera, all following the clock.
//! - [`render`]: wgpu pipeline that draws video frames.
//! - [`platform`]: the parts that depend on the operating system.
//! - [`ui`]: the egui window.

pub mod clock;
pub mod library;
pub mod media;
pub mod platform;
pub mod playback;
pub mod render;
pub mod time;
pub mod ui;
