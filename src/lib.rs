//! Multi-camera synced video player. See `specs/README.md`.
//!
//! Module map:
//! - [`time`], [`library`], [`clock`]: pure logic with no platform or media dependency.
//! - [`media`]: frame types and the FFmpeg decode backend.
//! - [`playback`]: one decode thread for each shown camera, all following the clock.
//! - [`render`]: wgpu pipeline that draws video frames.
//! - [`platform`]: the parts that depend on the operating system.
//! - [`ui`]: the egui window.

/// The name of the app, as the user sees it.
pub const APP_NAME: &str = "Multi Video Player";
/// The version of the app, from `Cargo.toml`.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod clock;
pub mod library;
pub mod media;
pub mod platform;
pub mod playback;
pub mod render;
pub mod time;
pub mod ui;
