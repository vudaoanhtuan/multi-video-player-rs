//! System clipboard access through egui, which is the same on all platforms.

use egui::{Context, Event, ViewportCommand};

/// Frames to wait for the clipboard text after a request. The text comes
/// one or two frames later. An empty clipboard gives no answer.
const WAIT_FRAMES: u8 = 4;

#[derive(Default)]
pub struct Clipboard {
    /// Frames left to wait for the text of a read request.
    waiting: u8,
    /// Replaces the system clipboard in scripted tests, so a test does not
    /// change the clipboard of the person at the computer.
    #[cfg(feature = "selftest")]
    pub fake: Option<String>,
    /// The answer of the test clipboard to a read request.
    #[cfg(feature = "selftest")]
    fake_answer: Option<String>,
}

impl Clipboard {
    pub fn copy(&mut self, ctx: &Context, text: &str) {
        #[cfg(feature = "selftest")]
        if let Some(fake) = self.fake.as_mut() {
            *fake = text.to_owned();
            return;
        }
        ctx.copy_text(text.to_owned());
    }

    /// Asks for the clipboard text. egui has no direct read: the system
    /// sends the text as a paste event in a later frame. Get it with
    /// [`Clipboard::take_requested`].
    pub fn request(&mut self, ctx: &Context) {
        #[cfg(feature = "selftest")]
        if let Some(fake) = self.fake.clone() {
            // Answer in the next frame, like the system clipboard.
            self.fake_answer = Some(fake);
            ctx.request_repaint();
            return;
        }
        ctx.send_viewport_cmd(ViewportCommand::RequestPaste);
        self.waiting = WAIT_FRAMES;
    }

    /// Call one time in each frame, before the widgets. Returns the text of
    /// an earlier [`Clipboard::request`] when it arrives, and removes its
    /// paste event so that no widget uses it.
    pub fn take_requested(&mut self, ctx: &Context) -> Option<String> {
        #[cfg(feature = "selftest")]
        if let Some(answer) = self.fake_answer.take() {
            return Some(answer);
        }
        if self.waiting == 0 {
            return None;
        }
        let mut text = None;
        ctx.input_mut(|input| {
            input.events.retain(|event| match event {
                Event::Paste(pasted) if text.is_none() => {
                    text = Some(pasted.clone());
                    false
                }
                _ => true,
            });
        });
        if text.is_some() {
            self.waiting = 0;
        } else {
            self.waiting -= 1;
            if self.waiting > 0 {
                ctx.request_repaint();
            }
        }
        text
    }
}
