//! The app window: camera grid and control bar. The UI is egui, which is the
//! same on all platforms.

pub mod app_icon;
mod bbox;
mod clipboard;
mod controls;
mod goto;
mod grid;
mod help;
mod picker;
#[cfg(feature = "selftest")]
mod selftest;
mod session;
mod settings;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, Key, Modifiers, RichText};

use crate::library::{scan_folder, ScanResult};
use crate::media::ffmpeg::FfmpegProbe;
use crate::media::{DecodeOptions, HwDecode};
use crate::platform::{self, AppMenu, MenuCommand};
use crate::playback::Waker;
use crate::render::VideoRenderer;
use crate::time::parse_pasted_time_of_day;
use crate::time::{Micros, MICROS_PER_SEC};
use session::Session;

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

/// The dot between the parts of the window title.
const TITLE_SEPARATOR: &str = " \u{b7} ";

/// The digit of a digit key. `None` for all other keys.
fn key_digit(key: Key) -> Option<u32> {
    const DIGIT_KEYS: [Key; 10] =
        [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
    DIGIT_KEYS.iter().position(|digit_key| *digit_key == key).map(|digit| digit as u32)
}

/// The speed of a speed hotkey, from the digit of its key.
///
/// | Keys | Speed | Speed with Shift |
/// |---|---|---|
/// | 1 to 9 | 1x to 9x | 11x to 19x |
/// | 0 | 10x | 20x |
fn key_speed(digit: u32, shift: bool) -> f64 {
    let speed = if digit == 0 { 10 } else { digit };
    f64::from(if shift { speed + 10 } else { speed })
}

/// Takes the presses of the speed hotkeys out of the input. Returns their speeds.
fn take_key_speeds(ctx: &egui::Context) -> Vec<f64> {
    let mut speeds = Vec::new();
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            let egui::Event::Key { key, physical_key, pressed: true, modifiers, .. } = event else { return true };
            // A speed hotkey has no modifier, or Shift only.
            if modifiers.ctrl || modifiers.alt || modifiers.command || modifiers.mac_cmd {
                return true;
            }
            // With Shift, a digit key gives the character that it makes with
            // Shift, for example "!" for the key 1. The physical key is
            // always the digit key.
            match physical_key.and_then(key_digit).or_else(|| key_digit(*key)) {
                Some(digit) => {
                    speeds.push(key_speed(digit, modifiers.shift));
                    false
                }
                None => true,
            }
        });
    });
    speeds
}

/// The jump of a jump hotkey: negative for back, positive for forward.
/// `None` for all other keys.
///
/// | Keys (back / forward) | Jump | Jump with Shift |
/// |---|---|---|
/// | Left / Right arrow | 2 s | 1 min |
/// | `,` / `.` | 5 s | 2 min |
/// | `;` / `'` | 10 s | 5 min |
/// | `[` / `]` | 30 s | 10 min |
fn key_jump(key: Key, shift: bool) -> Option<Micros> {
    const MIN: i64 = 60;
    // Seconds with no Shift, seconds with Shift, and the direction.
    let (plain, shifted, direction) = match key {
        Key::ArrowLeft => (2, MIN, -1),
        Key::ArrowRight => (2, MIN, 1),
        Key::Comma => (5, 2 * MIN, -1),
        Key::Period => (5, 2 * MIN, 1),
        // With Shift, some systems give the character that the key makes
        // with Shift (":", "{", "}") and not the key itself.
        Key::Semicolon | Key::Colon => (10, 5 * MIN, -1),
        Key::Quote => (10, 5 * MIN, 1),
        Key::OpenBracket | Key::OpenCurlyBracket => (30, 10 * MIN, -1),
        Key::CloseBracket | Key::CloseCurlyBracket => (30, 10 * MIN, 1),
        _ => return None,
    };
    let seconds = if shift { shifted } else { plain };
    Some(direction * seconds * MICROS_PER_SEC)
}

/// Takes the presses of the jump hotkeys out of the input. Returns their jumps.
fn take_key_jumps(ctx: &egui::Context) -> Vec<Micros> {
    let mut jumps = Vec::new();
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = event else { return true };
            // A jump hotkey has no modifier, or Shift only.
            if modifiers.ctrl || modifiers.alt || modifiers.command || modifiers.mac_cmd {
                return true;
            }
            match key_jump(*key, modifiers.shift) {
                Some(jump) => {
                    jumps.push(jump);
                    false
                }
                None => true,
            }
        });
    });
    jumps
}

/// The text of the window title: the app name, the version, and the folder
/// that is open, with a dot between the parts.
pub fn window_title(folder: Option<&Path>) -> String {
    let mut title = format!("{}{TITLE_SEPARATOR}{}", crate::APP_NAME, crate::APP_VERSION);
    if let Some(folder) = folder {
        title += TITLE_SEPARATOR;
        title += &folder.display().to_string();
    }
    title
}

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
    /// The hardware decode of this computer. `None`: the app can decode on the CPU only.
    hw_decode: Option<HwDecode>,
    waker: Waker,
    commands: Receiver<MenuCommand>,
    has_native_menu: bool,
    _menu: Option<AppMenu>,
    state: State,
    /// The folder that the user opened. It stays until the user closes it.
    folder: Option<PathBuf>,
    /// The window title that the window has now.
    title: String,
    notice: Option<Notice>,
    settings: settings::Settings,
    /// The values of the open settings dialog. They are not in use until the
    /// user applies them. The dialog does not belong to a session: it also
    /// works with no folder open.
    settings_draft: Option<settings::Settings>,
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
        // The loader that draws the SVG icons of the control bar.
        egui_extras::install_image_loaders(&ctx);
        // egui gives a popup this height at most. The default is 400 points,
        // which is too low for the 20 rows of the speed menu.
        ctx.all_styles_mut(|style| style.spacing.default_area_size.y = 600.0);

        let render_state = cc.wgpu_render_state.as_ref().ok_or("the wgpu renderer is not available")?;
        let support = platform::video_support(&render_state.device);
        let mut hw_decode = support.hw_decode;
        let mut importer = support.importer;
        let mut settings = settings::Settings::default();
        // For tests and problem reports: MULTI_VIDEO_PLAYER_DECODE=software | copy
        match std::env::var("MULTI_VIDEO_PLAYER_DECODE").as_deref() {
            // The start value of the "Decoder" setting. The user can change it in the settings dialog.
            Ok("software") => settings.decoder = settings::Decoder::Software,
            Ok("copy") => {
                importer = None;
                if let Some(hw) = hw_decode.as_mut() {
                    hw.zero_copy_nv12 = false;
                    hw.zero_copy_p010 = false;
                }
            }
            _ => {}
        }
        if hw_decode.is_none() {
            settings.decoder = settings::Decoder::Software;
        }
        log::info!("hardware decode: {hw_decode:?}, zero copy: {}, decoder: {}", importer.is_some(), settings.decoder.label());
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
            hw_decode,
            waker,
            commands,
            has_native_menu: menu.is_some(),
            _menu: menu,
            state: State::NoFolder,
            folder: None,
            title: window_title(None),
            notice: None,
            settings,
            settings_draft: None,
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

    /// The decode options for the "Decoder" setting.
    fn decode_options(&self) -> DecodeOptions {
        match self.settings.decoder {
            settings::Decoder::Hardware => DecodeOptions { hw: self.hw_decode },
            settings::Decoder::Software => DecodeOptions { hw: None },
        }
    }

    /// Shows the settings dialog when it is open. "Apply" puts its values in use.
    fn show_settings(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.settings_draft.as_mut() else { return };
        let Some(result) = settings::show_dialog(ctx, draft, self.hw_decode.is_some()) else { return };
        let Some(new) = self.settings_draft.take() else { return };
        if result == settings::DialogResult::Apply {
            self.apply_settings(new, ctx);
        }
    }

    /// Puts new settings in use.
    fn apply_settings(&mut self, new: settings::Settings, ctx: &egui::Context) {
        let before = std::mem::replace(&mut self.settings, new);
        if self.settings == before {
            return;
        }
        if self.settings.decoder != before.decoder {
            log::info!("decoder: {}", self.settings.decoder.label());
            let opts = self.decode_options();
            if let State::Ready(session) = &mut self.state {
                session.set_decode(opts, &self.renderer);
            }
        }
        if self.settings.key_only_min_speed != before.key_only_min_speed {
            if let State::Ready(session) = &mut self.state {
                session.set_key_only_min_speed(self.settings.key_only_min_speed);
            }
        }
        // The grid of this refresh was drawn with the old settings.
        ctx.request_repaint();
    }

    /// Stops playback, drops the old library, and scans the folder in the background.
    fn open_folder(&mut self, folder: PathBuf, ctx: &egui::Context) {
        let (sender, result) = mpsc::channel();
        // Replacing the state drops the old session. That stops its decode threads.
        self.state = State::Loading { result };
        self.notice = None;
        // The window title shows the full path, also for a relative path from the command line.
        self.folder = Some(std::path::absolute(&folder).unwrap_or_else(|_| folder.clone()));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let scan = scan_folder(&folder, &FfmpegProbe).map_err(|e| format!("Cannot read the folder: {e}"));
            let _ = sender.send(scan);
            ctx.request_repaint();
        });
    }

    /// Stops playback, closes all files, and shows the "open a folder" message.
    fn close_folder(&mut self) {
        // Replacing the state drops the session. That stops its decode threads.
        self.state = State::NoFolder;
        self.folder = None;
        self.notice = None;
        self.clipboard_jump = false;
    }

    /// Opens the user manual in the web browser. Shows a message if that fails.
    fn show_help(&mut self) {
        // A scripted test must not open a browser on the computer of the user.
        #[cfg(feature = "selftest")]
        if self.script.is_some() {
            match help::write_manual() {
                Ok(path) => println!("help manual={}", path.display()),
                Err(error) => println!("help error={error}"),
            }
            return;
        }
        if let Err(text) = help::open_manual() {
            log::warn!("{text}");
            self.notice = Some(Notice { text, until: Instant::now() + MESSAGE_NOTICE_TIME });
        }
    }

    /// Changes the window title when the open folder changed. The command
    /// is sent only on a change, because each command causes a refresh.
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = window_title(self.folder.as_deref());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn choose_folder(&mut self, ctx: &egui::Context) {
        // Scripted tests cannot use the native dialog. They give the folder here.
        #[cfg(feature = "selftest")]
        if let Some(folder) = std::env::var_os("MULTI_VIDEO_PLAYER_TEST_FOLDER") {
            self.open_folder(PathBuf::from(folder), ctx);
            return;
        }
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
        let opts = self.decode_options();
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
                        opts,
                        self.settings.key_only_min_speed,
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

        let dialog = self.settings_draft.is_some() || session.goto.is_some() || session.picker.is_some() || session.bbox.is_dialog();
        if dialog || ctx.text_edit_focused() {
            return;
        }
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_key(modifiers, key));
        if pressed(Modifiers::NONE, Key::Space) {
            session.toggle_play();
        }
        for jump in take_key_jumps(ctx) {
            session.jump_by(jump);
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
        for speed in take_key_speeds(ctx) {
            session.set_speed(speed);
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
                    match &self.state {
                        State::Ready(_) => {}
                        State::NoFolder => {
                            let settings = if self.settings_draft.is_some() { "open" } else { "closed" };
                            println!("stats state=no-folder title={:?} | settings={settings}", self.title);
                        }
                        State::Loading { .. } => println!("stats state=loading"),
                        State::NoVideos => println!("stats state=no-videos"),
                        State::Failed(_) => println!("stats state=failed"),
                    }
                    if let State::Ready(session) = &self.state {
                        let clock = session.clock.snapshot();
                        let ms = |t: crate::time::Timestamp| format!("{}.{:03}", t.format_hms(), t.time_of_day() / 1000 % 1000);
                        let mut line = format!("stats clock={} playing={} speed={}", ms(clock.time), clock.playing, clock.speed);
                        line += &format!(" ratio={} decoder={}", self.settings.video_ratio.label(), self.settings.decoder.label());
                        line += &format!(" keyonly={}", self.settings.key_only_min_speed);
                        line += &format!(" names={}", if self.settings.show_camera_name { "shown" } else { "hidden" });
                        line += &format!(" title={:?}", self.title);
                        for tile in &session.tiles {
                            let id = &session.library.cameras[tile.camera].camera_id;
                            match tile.player.frame_time() {
                                Some(t) => {
                                    // "hw": the frame is in the memory of the hardware decoder.
                                    let hw = tile.player.frame().is_some_and(|f| matches!(f.data, crate::media::FrameData::Hw(_)));
                                    let memory = if hw { "hw" } else { "cpu" };
                                    line += &format!(" | {id}={} lag={}ms frame={memory}", ms(t), (clock.time.0 - t.0) / 1000);
                                }
                                None => line += &format!(" | {id}=black"),
                            }
                        }
                        line += &format!(" | goto={}", if session.goto.is_some() { "open" } else { "closed" });
                        line += &format!(" | settings={}", if self.settings_draft.is_some() { "open" } else { "closed" });
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

        let can_close = !matches!(self.state, State::NoFolder);
        let mut bar_action = None;
        egui::Panel::bottom("control-bar").resizable(false).show(ui, |ui| {
            let session = match &mut self.state {
                State::Ready(session) => Some(&mut **session),
                _ => None,
            };
            bar_action = controls::show(ui, session, clock.as_ref(), can_close);
        });
        self.handle_copy_request(&ctx);
        match bar_action {
            Some(controls::BarAction::OpenFolder) => want_open = true,
            Some(controls::BarAction::CloseFolder) => self.close_folder(),
            Some(controls::BarAction::OpenSettings) => self.settings_draft = Some(self.settings),
            Some(controls::BarAction::ShowHelp) => self.show_help(),
            None => {}
        }
        // The session can be gone now. Do not use the clock of a closed session.
        let clock = clock.filter(|_| matches!(self.state, State::Ready(_)));

        let mut next_due = None;
        let grid_area = egui::CentralPanel::no_frame()
            .show(ui, |ui| {
                let area = ui.available_rect_before_wrap();
                match &mut self.state {
                    State::Ready(session) => {
                        // Use the clock value of this refresh for all tiles, so all tiles show the same time.
                        let clock = clock.as_ref().expect("a session has a clock");
                        next_due = grid::show(ui, session, clock, &self.renderer, &self.settings);
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
        self.show_settings(&ctx);
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
        // After all actions of this frame, so that an open or a close shows immediately.
        self.update_title(&ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_hotkeys_have_the_jumps_of_the_spec() {
        const S: Micros = MICROS_PER_SEC;
        // Back key, forward key, jump, jump with Shift.
        let table = [
            (Key::ArrowLeft, Key::ArrowRight, 2 * S, 60 * S),
            (Key::Comma, Key::Period, 5 * S, 120 * S),
            (Key::Semicolon, Key::Quote, 10 * S, 300 * S),
            (Key::OpenBracket, Key::CloseBracket, 30 * S, 600 * S),
        ];
        for (back, forward, plain, shifted) in table {
            assert_eq!(key_jump(back, false), Some(-plain), "{back:?}");
            assert_eq!(key_jump(forward, false), Some(plain), "{forward:?}");
            assert_eq!(key_jump(back, true), Some(-shifted), "Shift+{back:?}");
            assert_eq!(key_jump(forward, true), Some(shifted), "Shift+{forward:?}");
        }
        // The characters that Shift makes from ";", "[", and "]".
        assert_eq!(key_jump(Key::Colon, true), Some(-300 * S));
        assert_eq!(key_jump(Key::OpenCurlyBracket, true), Some(-600 * S));
        assert_eq!(key_jump(Key::CloseCurlyBracket, true), Some(600 * S));
        for key in [Key::Space, Key::G, Key::Num1, Key::ArrowUp, Key::ArrowDown, Key::Slash] {
            assert_eq!(key_jump(key, false), None, "{key:?}");
        }
    }

    #[test]
    fn speed_hotkeys_have_the_speeds_of_the_spec() {
        let digit = |key| key_digit(key).unwrap();
        assert_eq!(key_speed(digit(Key::Num1), false), 1.0);
        assert_eq!(key_speed(digit(Key::Num9), false), 9.0);
        assert_eq!(key_speed(digit(Key::Num0), false), 10.0);
        assert_eq!(key_speed(digit(Key::Num1), true), 11.0);
        assert_eq!(key_speed(digit(Key::Num9), true), 19.0);
        assert_eq!(key_speed(digit(Key::Num0), true), 20.0);
        for key in [Key::Space, Key::G, Key::Exclamationmark, Key::Comma] {
            assert_eq!(key_digit(key), None, "{key:?}");
        }
        // Each speed of the speed menu has one hotkey, and each hotkey sets a speed of the menu.
        let mut speeds: Vec<f64> = (0..10).flat_map(|digit| [key_speed(digit, false), key_speed(digit, true)]).collect();
        speeds.sort_by(f64::total_cmp);
        assert_eq!(speeds, crate::clock::SPEEDS);
    }

    #[test]
    fn speed_hotkeys_are_taken_from_the_input() {
        let press = |key, physical_key, modifiers| egui::Event::Key { key, physical_key, pressed: true, repeat: false, modifiers };
        let events = vec![
            press(Key::Num3, Some(Key::Num3), Modifiers::NONE),
            // A real keyboard gives "!" for Shift+1, and no character key for Shift+2.
            press(Key::Exclamationmark, Some(Key::Num1), Modifiers::SHIFT),
            press(Key::Num2, Some(Key::Num2), Modifiers::SHIFT),
            // A test script gives no physical key.
            press(Key::Num0, None, Modifiers::SHIFT),
            // Not speed hotkeys: these stay in the input.
            press(Key::Num4, Some(Key::Num4), Modifiers::COMMAND),
            press(Key::G, Some(Key::G), Modifiers::NONE),
        ];
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput { events, ..Default::default() });
        assert_eq!(take_key_speeds(&ctx), [3.0, 11.0, 12.0, 20.0]);
        assert_eq!(ctx.input(|input| input.events.len()), 2);
        assert_eq!(take_key_speeds(&ctx), [0.0; 0]);
        // egui does not permit a drop of texture changes that were not applied.
        ctx.end_pass().textures_delta.clear();
    }

    #[test]
    fn window_title_has_name_version_and_folder() {
        let base = window_title(None);
        assert_eq!(base, format!("Multi Video Player · {}", env!("CARGO_PKG_VERSION")));
        let with_folder = window_title(Some(Path::new("/recordings/shop 1")));
        assert_eq!(with_folder, format!("{base} · /recordings/shop 1"));
    }
}
