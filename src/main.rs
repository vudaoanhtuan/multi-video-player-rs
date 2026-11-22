// A release build for Windows is a window program: no console window opens
// with it. The test build keeps the console, because its scripts print results.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions), not(feature = "selftest")),
    windows_subsystem = "windows"
)]

use std::path::PathBuf;
use std::sync::Arc;

use egui_wgpu::{wgpu, WgpuSetup};

fn main() -> eframe::Result {
    multi_video_player::platform::init_process();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    // The first argument can be the folder to open.
    let folder = std::env::args_os().nth(1).map(PathBuf::from).filter(|p| p.is_dir());

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(multi_video_player::ui::window_title(None))
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([640.0, 360.0]);
    // The icon in the Dock or the task bar. Without it, eframe shows its own icon.
    if let Some(icon) = multi_video_player::ui::app_icon::icon_data() {
        viewport = viewport.with_icon(icon);
    }
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    if let WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        // The `WGPU_BACKEND` environment variable replaces the choice of the platform.
        if let (Some(backends), None) = (multi_video_player::platform::gpu_backends(), wgpu::Backends::from_env()) {
            setup.instance_descriptor.backends = backends;
        }
        // Video tiles are a light load. Prefer the low power GPU.
        setup.power_preference = wgpu::PowerPreference::LowPower;
        // 16 bit textures let the GPU read 10 bit hardware frames with no copy.
        let base = setup.device_descriptor.clone();
        setup.device_descriptor = Arc::new(move |adapter| {
            let mut descriptor = base(adapter);
            descriptor.required_features |= adapter.features() & wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
            descriptor
        });
    }

    eframe::run_native(
        multi_video_player::APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(multi_video_player::ui::App::new(cc, folder)?))),
    )
}
