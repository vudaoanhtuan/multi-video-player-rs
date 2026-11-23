//! Development tool: runs a script of input events against the real window.
//! Only built with `--features selftest`.
//!
//! The script is in the `MULTI_VIDEO_PLAYER_SCRIPT` environment variable. Steps
//! are separated by `;`. Each step is `<seconds>:<action>[:<argument>]`.
//! The seconds count from the moment the folder scan is done.
//!
//! | Action | Argument | Result |
//! |---|---|---|
//! | `key` | egui key name, for example `Space` or `Shift+1` | Key press and release |
//! | `text` | text | Typed text |
//! | `paste` | text | A paste of this text (Cmd+V) |
//! | `clipboard` | text | Replaces the system clipboard with a test clipboard that holds this text |
//! | `clipshot` | file path | Writes the picture in the test clipboard as a PPM file |
//! | `click` | `x,y` in window points | Pointer click |
//! | `down`, `move`, `up` | `x,y` | Parts of a drag |
//! | `shot` | file path | Writes the window picture as a PPM file |
//! | `stats` | none | Prints the clock time and the frame time of each tile |
//! | `quit` | none | Closes the app |

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput};

#[derive(Clone, Debug)]
pub enum Action {
    Key(Modifiers, Key),
    Text(String),
    Paste(String),
    Clipboard(String),
    ClipShot(PathBuf),
    Down(Pos2),
    Move(Pos2),
    Up(Pos2),
    Shot(PathBuf),
    Stats,
    Quit,
}

impl Action {
    fn is_input(&self) -> bool {
        !matches!(self, Action::Shot(_) | Action::Stats | Action::Quit | Action::Clipboard(_) | Action::ClipShot(_))
    }
}

pub struct Script {
    steps: VecDeque<(f64, Action)>,
    start: Option<Instant>,
}

impl Script {
    pub fn from_env() -> Option<Self> {
        let text = std::env::var("MULTI_VIDEO_PLAYER_SCRIPT").ok()?;
        let mut steps = Vec::new();
        for step in text.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let mut parts = step.splitn(3, ':');
            let at: f64 = parts.next()?.trim().parse().expect("script: bad time");
            let action = parts.next().expect("script: no action").trim();
            let argument = parts.next().unwrap_or("").trim();
            let point = || {
                let (x, y) = argument.split_once(',').expect("script: point must be x,y");
                Pos2::new(x.trim().parse().expect("script: bad x"), y.trim().parse().expect("script: bad y"))
            };
            match action {
                "key" => {
                    let (modifiers, name) = match argument.strip_prefix("Shift+") {
                        Some(name) => (Modifiers::SHIFT, name),
                        None => (Modifiers::NONE, argument),
                    };
                    steps.push((at, Action::Key(modifiers, Key::from_name(name).expect("script: unknown key"))));
                }
                "text" => steps.push((at, Action::Text(argument.to_owned()))),
                "paste" => steps.push((at, Action::Paste(argument.to_owned()))),
                "clipboard" => steps.push((at, Action::Clipboard(argument.to_owned()))),
                "clipshot" => steps.push((at, Action::ClipShot(PathBuf::from(argument)))),
                "click" => {
                    steps.push((at, Action::Down(point())));
                    steps.push((at + 0.05, Action::Up(point())));
                }
                "down" => steps.push((at, Action::Down(point()))),
                "move" => steps.push((at, Action::Move(point()))),
                "up" => steps.push((at, Action::Up(point()))),
                "shot" => steps.push((at, Action::Shot(PathBuf::from(argument)))),
                "stats" => steps.push((at, Action::Stats)),
                "quit" => steps.push((at, Action::Quit)),
                other => panic!("script: unknown action {other}"),
            }
        }
        steps.sort_by(|a, b| a.0.total_cmp(&b.0));
        Some(Self { steps: steps.into(), start: None })
    }

    /// Starts the script time. Call when no folder scan is in progress.
    pub fn start(&mut self) {
        self.start.get_or_insert_with(Instant::now);
    }

    fn head_due(&self) -> Option<&Action> {
        let elapsed = self.start?.elapsed().as_secs_f64();
        self.steps.front().filter(|(at, _)| *at <= elapsed).map(|(_, action)| action)
    }

    /// Adds the input events that are due to the input of this frame.
    pub fn inject(&mut self, raw: &mut RawInput) {
        while self.head_due().is_some_and(Action::is_input) {
            let (_, action) = self.steps.pop_front().unwrap();
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            match action {
                Action::Key(modifiers, key) => {
                    for pressed in [true, false] {
                        raw.events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
                    }
                }
                Action::Text(text) => raw.events.push(Event::Text(text)),
                Action::Paste(text) => raw.events.push(Event::Paste(text)),
                Action::Down(pos) => raw.events.extend([Event::PointerMoved(pos), button(pos, true)]),
                Action::Move(pos) => raw.events.push(Event::PointerMoved(pos)),
                Action::Up(pos) => raw.events.extend([Event::PointerMoved(pos), button(pos, false)]),
                _ => unreachable!(),
            }
        }
    }

    /// Returns the next action that is not an input event, when it is due.
    pub fn take_command(&mut self) -> Option<Action> {
        if self.head_due().is_some_and(|action| !action.is_input()) {
            return self.steps.pop_front().map(|(_, action)| action);
        }
        None
    }

    /// Time until the next step.
    pub fn next_wake(&self) -> Option<Duration> {
        let (at, _) = self.steps.front()?;
        let elapsed = self.start?.elapsed().as_secs_f64();
        Some(Duration::from_secs_f64((at - elapsed).max(0.0)))
    }
}

/// Writes a window picture as a binary PPM file.
pub fn save_ppm(path: &std::path::Path, image: &egui::ColorImage) {
    let mut data = format!("P6\n{} {}\n255\n", image.size[0], image.size[1]).into_bytes();
    data.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
    if let Err(error) = std::fs::write(path, data) {
        eprintln!("script: cannot write {}: {error}", path.display());
    }
}
