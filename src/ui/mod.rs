//! The app window: camera grid and control bar. The UI is egui, which is the
//! same on all platforms.

mod bbox;
mod clipboard;
mod controls;
mod goto;
mod grid;
mod picker;
#[cfg(feature = "selftest")]
mod selftest;
mod session;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, Key, Modifiers, RichText};

use crate::library::{scan_folder, ScanResult};
use crate::media::ffmpeg::FfmpegProbe;
use crate::media::DecodeOptions;
use crate::platform::{self, AppMenu, MenuCommand};
use crate::playback::Waker;
use crate::render::VideoRenderer;
use crate::time::parse_pasted_time_of_day;
use session::{Session, ARROW_JUMP};

/// Refresh the timer at least this often while playing.
const TIMER_REFRESH: Duration = Duration::from_millis(250);
/// Shortest time between two screen refreshes caused by video frames.
/// Cameras record at 30 frames each second or less, so more refreshes than
/// about 70 each second only use power.
const MIN_REFRESH_INTERVAL: Duration = Duration::from_millis(14);
/// How long the "files skipped" message stays on screen.
const NOTICE_TIME: Duration = Duration::from_secs(8);
/// How long the "copied" message stays on screen.
const COPIED_NOTICE_TIME: Duration = Duration::from_secs(2);
/// How long other short messages stay on screen.
const MESSAGE_NOTICE_TIME: Duration = Duration::from_secs(4);

enum State {
    NoFolder,
    Loading { result: Receiver<Result<ScanResult, String>> },
    /// The scan found no usable video file.
    NoVideos,
    Failed(String),
    Ready(Box<Session>),
}

struct Notice {
    text: String,
    until: Instant,
}

pub struct App {
    renderer: VideoRenderer,
    decode: DecodeOptions,
    waker: Waker,
    commands: Receiver<MenuCommand>,
    has_native_menu: bool,
    _menu: Option<AppMenu>,
    state: State,
    notice: Option<Notice>,
    clipboard: clipboard::Clipboard,
    /// Shift+G asked for the clipboard text. A valid time in it starts a jump.
    clipboard_jump: bool,
    last_refresh: Instant,
    #[cfg(feature = "selftest")]
    script: Option<selftest::Script>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, folder: Option<PathBuf>) -> Result<Self, String> {
        let ctx = cc.egui_ctx.clone();
        ctx.set_theme(egui::Theme::Dark);

        let render_state = cc.wgpu_render_state.as_ref().ok_or("the wgpu renderer is not available")?;
        let support = platform::video_support(&render_state.device);
        let mut decode = DecodeOptions { hw: support.hw_decode };
        let mut importer = support.importer;
        // For tests and problem reports: VIDEO_PLAYER_DECODE=software | copy
        match std::env::var("VIDEO_PLAYER_DECODE").as_deref() {
            Ok("software") => decode.hw = None,
            Ok("copy") => {
                importer = None;
                if let Some(hw) = decode.hw.as_mut() {
                    hw.zero_copy_nv12 = false;
                    hw.zero_copy_p010 = false;
                }
            }
            _ => {}
        }
        log::info!("decode: {decode:?}, zero copy: {}", importer.is_some());
        let renderer = VideoRenderer::new(render_state, importer);

        let (sender, commands) = mpsc::channel();
        let menu_ctx = ctx.clone();
        let menu = platform::install_app_menu(move |command| {
            let _ = sender.send(command);
            menu_ctx.request_repaint();
        });

        let waker_ctx = ctx.clone();
        let waker: Waker = Arc::new(move |delay| waker_ctx.request_repaint_after(delay));

        let mut app = Self {
            renderer,
            decode,
            waker,
            commands,
            has_native_menu: menu.is_some(),
            _menu: menu,
            state: State::NoFolder,
            notice: None,
            clipboard: clipboard::Clipboard::default(),
            clipboard_jump: false,
            last_refresh: Instant::now(),
            #[cfg(feature = "selftest")]
            script: selftest::Script::from_env(),
        };
        if let Some(folder) = folder {
            app.open_folder(folder, &ctx);
        }
        Ok(app)
    }

    /// Stops playback, drops the old library, and scans the folder in the background.
    fn open_folder(&mut self, folder: PathBuf, ctx: &egui::Context) {
        let (sender, result) = mpsc::channel();
        // Replacing the state drops the old session. That stops its decode threads.
        self.state = State::Loading { result };
        self.notice = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let scan = scan_folder(&folder, &FfmpegProbe).map_err(|e| format!("Cannot read the folder: {e}"));
            let _ = sender.send(scan);
            ctx.request_repaint();
        });
    }

    fn choose_folder(&mut self, ctx: &egui::Context) {
        if let Some(folder) = platform::pick_folder() {
            self.open_folder(folder, ctx);
        }
    }

    /// Takes the scan result when the background scan is done.
    fn poll_scan(&mut self) {
        let State::Loading { result } = &self.state else { return };
        let scan = match result.try_recv() {
            Ok(scan) => scan,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("The folder scan stopped.".to_owned()),
        };
        self.state = match scan {
            Err(message) => State::Failed(message),
            Ok(scan) => {
                if scan.skipped > 0 {
                    let files = if scan.skipped == 1 { "file" } else { "files" };
                    self.notice = Some(Notice {
                        text: format!("{} {files} skipped", scan.skipped),
                        until: Instant::now() + NOTICE_TIME,
                    });
                }
                match scan.library {
                    Some(library) => State::Ready(Box::new(Session::new(
                        library,
                        self.decode,
                        self.waker.clone(),
                        &self.renderer,
                    ))),
                    None => State::NoVideos,
                }
            }
        };
    }

    /// Keyboard shortcuts. They do nothing while a text box or a dialog has
    /// the keyboard focus.
    fn handle_keys(&mut self, ctx: &egui::Context) {
        let State::Ready(session) = &mut self.state else { return };

        // The answer to an earlier Shift+G: jump if the clipboard text is a
        // time in a full form. With other text, nothing happens.
        if let Some(text) = self.clipboard.take_requested(ctx) {
            if std::mem::take(&mut self.clipboard_jump) && session.goto.is_none() {
                let time_of_day = parse_pasted_time_of_day(&text);
                // The clipboard can hold private text. Do not log the text.
                log::debug!("Shift+G: clipboard text is a time: {}", time_of_day.is_some());
                if let Some(time_of_day) = time_of_day {
                    session.jump_to_time_of_day(time_of_day);
                }
            }
        }

        if session.goto.is_some() || session.picker.is_some() || session.bbox.is_dialog() || ctx.text_edit_focused() {
            return;
        }
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_key(modifiers, key));
        if pressed(Modifiers::NONE, Key::Space) {
            session.toggle_play();
        }
        if pressed(Modifiers::NONE, Key::ArrowLeft) {
            session.jump_by(-ARROW_JUMP);
        }
        if pressed(Modifiers::NONE, Key::ArrowRight) {
            session.jump_by(ARROW_JUMP);
        }
        if pressed(Modifiers::NONE, Key::C) {
            session.picker = Some(session.shown.clone());
        }
        if pressed(Modifiers::NONE, Key::T) {
            session.copy_time_and_pause();
        }
        if pressed(Modifiers::NONE, Key::F) {
            session.toggle_bbox();
        }
        if session.bbox.is_picking() && pressed(Modifiers::NONE, Key::Escape) {
            session.toggle_bbox();
        }
        // Shift+G first: a check for G with no modifier also accepts Shift+G.
        if pressed(Modifiers::SHIFT, Key::G) {
            self.clipboard.request(ctx);
            self.clipboard_jump = true;
        } else if pressed(Modifiers::NONE, Key::G) {
            session.open_goto();
            // The same key press also arrives as the text "g". Keep it out of the text box.
            ctx.input_mut(|i| i.events.retain(|event| !matches!(event, egui::Event::Text(_))));
        }
    }

    /// Moves copy requests of the session to the clipboard, and messages of
    /// the session to the screen.
    fn handle_copy_request(&mut self, ctx: &egui::Context) {
        let State::Ready(session) = &mut self.state else { return };
        if let Some(text) = session.copy_request.take() {
            self.clipboard.copy(ctx, &text);
            self.notice = Some(Notice { text: format!("Copied {text}"), until: Instant::now() + COPIED_NOTICE_TIME });
        }
        if let Some(picture) = session.copy_picture_request.take() {
            self.clipboard.copy_image(ctx, picture);
        }
        if let Some(text) = session.message.take() {
            self.notice = Some(Notice { text, until: Instant::now() + MESSAGE_NOTICE_TIME });
        }
    }

    /// A message in the middle of the grid area, with an optional open button.
    /// Returns `true` when the user clicked the button.
    fn message(ui: &mut egui::Ui, text: &str, detail: Option<&str>, open_button: bool, spinner: bool) -> bool {
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0.0, Color32::BLACK);
        let mut clicked = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((area.height() / 2.0 - 60.0).max(0.0));
                if spinner {
                    ui.spinner();
                    ui.add_space(8.0);
                }
                ui.label(RichText::new(text).size(18.0).strong());
                if let Some(detail) = detail {
                    ui.add_space(4.0);
                    ui.label(RichText::new(detail).weak());
                }
                if open_button {
                    ui.add_space(14.0);
                    clicked = ui.add(egui::Button::new("Open Folder…").min_size(egui::vec2(140.0, 30.0))).clicked();
                }
            });
        });
        clicked
    }

    fn show_notice(&mut self, ui: &egui::Ui, area: egui::Rect) {
        let Some(notice) = &self.notice else { return };
        let left = notice.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            self.notice = None;
            return;
        }
        let painter = ui.painter_at(area);
        let galley = painter.layout_no_wrap(notice.text.clone(), egui::FontId::proportional(13.0), Color32::WHITE);
        let size = galley.size() + egui::vec2(16.0, 8.0);
        let rect = egui::Rect::from_min_size(egui::pos2(area.right() - size.x - 10.0, area.top() + 10.0), size);
        painter.rect_filled(rect, 4.0, Color32::from_black_alpha(190));
        painter.galley(rect.min + egui::vec2(8.0, 4.0), galley, Color32::WHITE);
        ui.ctx().request_repaint_after(left);
    }
}

#[cfg(feature = "selftest")]
impl App {
    /// Runs the script steps that are due. See `selftest.rs`.
    fn run_script(&mut self, ctx: &egui::Context) {
        let Some(script) = self.script.as_mut() else { return };
        if !matches!(self.state, State::Loading { .. }) {
            script.start();
        }
        while let Some(action) = script.take_command() {
            match action {
                selftest::Action::Shot(path) => {
                    ctx.request_screenshot(move |image| selftest::save_ppm(&path, &image));
                }
                selftest::Action::Stats => {
                    if let State::Ready(session) = &self.state {
                        let clock = session.clock.snapshot();
                        let ms = |t: crate::time::Timestamp| format!("{}.{:03}", t.format_hms(), t.time_of_day() / 1000 % 1000);
                        let mut line = format!("stats clock={} playing={} speed={}", ms(clock.time), clock.playing, clock.speed);
                        for tile in &session.tiles {
                            let id = &session.library.cameras[tile.camera].camera_id;
                            match tile.player.frame_time() {
                                Some(t) => line += &format!(" | {id}={} lag={}ms", ms(t), (clock.time.0 - t.0) / 1000),
                                None => line += &format!(" | {id}=black"),
                            }
                        }
                        line += &format!(" | goto={}", if session.goto.is_some() { "open" } else { "closed" });
                        line += &match &session.bbox {
                            bbox::BboxMode::Off => " | bbox=off".to_owned(),
                            bbox::BboxMode::PickCamera => " | bbox=pick".to_owned(),
                            bbox::BboxMode::Dialog(d) => format!(
                                " | bbox=dialog:{}:{}x{}:selection={:?}:copied={:?}",
                                d.camera_id, d.picture.size[0], d.picture.size[1], d.selection, d.copied
                            )
                            .replace(' ', ""),
                        };
                        if let Some(notice) = &self.notice {
                            line += &format!(" | notice={}", notice.text.replace(' ', "_"));
                        }
                        if let Some(fake) = &self.clipboard.fake {
                            line += &format!(" | clipboard={fake}");
                        }
                        println!("{line}");
                    }
                }
                selftest::Action::Clipboard(text) => self.clipboard.fake = Some(text),
                selftest::Action::ClipShot(path) => match &self.clipboard.fake_image {
                    Some(image) => selftest::save_ppm(&path, image),
                    None => eprintln!("script: the test clipboard holds no picture"),
                },
                selftest::Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                _ => {}
            }
        }
        if let Some(delay) = script.next_wake() {
            ctx.request_repaint_after(delay);
        }
    }
}

impl eframe::App for App {
    /// Runs before each `ui` call. It also runs while the window is covered
    /// or minimized, when eframe does not call `ui`.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let State::Ready(session) = &self.state {
            session.stop_at_end();
            if session.clock.snapshot().playing {
                // Keep a slow heartbeat while playing. Without it, a window
                // that was covered shows an old frame until the next input event.
                ctx.request_repaint_after(TIMER_REFRESH);
            }
        }

        // The script must also run when the window is covered or the display
        // is off. eframe does not paint a hidden window, so this hook asks for
        // a painted frame on each call, which also causes the next call. The
        // sleep holds this loop at the highest refresh rate that the app uses
        // for video.
        #[cfg(feature = "selftest")]
        if self.script.is_some() {
            std::thread::sleep(MIN_REFRESH_INTERVAL.saturating_sub(self.last_refresh.elapsed()));
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        }
    }

    #[cfg(feature = "selftest")]
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(script) = self.script.as_mut() {
            script.inject(raw_input);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let frame_start = Instant::now();

        let mut want_open = false;
        while let Ok(command) = self.commands.try_recv() {
            match command {
                MenuCommand::OpenFolder => want_open = true,
            }
        }
        // Platforms with no native menu use the keyboard shortcut.
        if !self.has_native_menu && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::O)) {
            want_open = true;
        }

        self.poll_scan();
        self.handle_keys(&ctx);

        let clock = match &self.state {
            State::Ready(session) => Some(session.clock.snapshot()),
            _ => None,
        };

        egui::Panel::bottom("control-bar").resizable(false).show(ui, |ui| {
            let session = match &mut self.state {
                State::Ready(session) => Some(&mut **session),
                _ => None,
            };
            controls::show(ui, session, clock.as_ref());
        });
        self.handle_copy_request(&ctx);

        let mut next_due = None;
        let grid_area = egui::CentralPanel::no_frame()
            .show(ui, |ui| {
                let area = ui.available_rect_before_wrap();
                match &mut self.state {
                    State::Ready(session) => {
                        // Use the clock value of this refresh for all tiles, so all tiles show the same time.
                        let clock = clock.as_ref().expect("a session has a clock");
                        next_due = grid::show(ui, session, clock, &self.renderer);
                    }
                    State::NoFolder => {
                        want_open |= Self::message(ui, "Open a folder of camera recordings", None, true, false);
                    }
                    State::Loading { .. } => {
                        Self::message(ui, "Loading…", None, false, true);
                    }
                    State::NoVideos => {
                        let detail = self.notice.as_ref().map(|n| n.text.as_str());
                        want_open |= Self::message(ui, "No videos found", detail, true, false);
                    }
                    State::Failed(reason) => {
                        want_open |= Self::message(ui, reason, None, true, false);
                    }
                }
                area
            })
            .inner;

        if matches!(self.state, State::Ready(_)) {
            self.show_notice(ui, grid_area);
        }
        if let State::Ready(session) = &mut self.state {
            picker::show(&ctx, session, &self.renderer);
            goto::show(&ctx, session);
            bbox::show_dialog(&ctx, session);
        }
        // Requests from the dialogs and from a click on a tile.
        self.handle_copy_request(&ctx);

        // Ask for the next refresh. While paused, nothing is drawn until an
        // input event or a new frame arrives.
        if let (State::Ready(_), Some(clock)) = (&self.state, clock) {
            if clock.playing {
                let mut delay = TIMER_REFRESH;
                if let Some(due) = next_due {
                    let media = (due.0 - clock.time.0).max(0) as f64 / 1e6;
                    delay = delay.min(Duration::from_secs_f64(media / clock.speed));
                }
                let earliest = (self.last_refresh + MIN_REFRESH_INTERVAL).saturating_duration_since(Instant::now());
                ctx.request_repaint_after(delay.max(earliest));
            }
        }
        self.last_refresh = frame_start;
        #[cfg(feature = "selftest")]
        self.run_script(&ctx);

        if want_open {
            self.choose_folder(&ctx);
        }
    }
}
