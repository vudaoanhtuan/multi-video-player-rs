//! Help: opens the user manual in the web browser of the system.
//!
//! The manual is the HTML file `assets/help/manual.html`. It is part of the
//! executable, so the app has no file to lose. The app writes a copy to the
//! temporary folder of the system and asks the system to open it. The copy
//! shows the version of the app.

use std::path::PathBuf;

const MANUAL_HTML: &str = include_str!("../../assets/help/manual.html");
/// The manual has this text where the version number goes.
const VERSION_PLACEHOLDER: &str = "{{version}}";
use crate::APP_VERSION;

/// The manual with the version number of this app.
fn manual_html() -> String {
    // The comment at the top of the file also names the placeholder. Replace
    // only the place in the page content, which is after the `body` tag.
    match MANUAL_HTML.split_once("<body>") {
        Some((head, body)) => format!("{head}<body>{}", body.replace(VERSION_PLACEHOLDER, APP_VERSION)),
        None => MANUAL_HTML.replace(VERSION_PLACEHOLDER, APP_VERSION),
    }
}

/// Writes the manual to a file and returns the path.
pub fn write_manual() -> Result<PathBuf, String> {
    let folder = std::env::temp_dir().join("multi-video-player");
    let path = folder.join("manual.html");
    std::fs::create_dir_all(&folder)
        .and_then(|()| std::fs::write(&path, manual_html()))
        .map_err(|error| format!("Cannot write the manual: {error}"))?;
    Ok(path)
}

/// Opens the manual in the web browser.
pub fn open_manual() -> Result<(), String> {
    let path = write_manual()?;
    crate::platform::open_in_browser(&path).map_err(|error| format!("Cannot open the manual: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_lists_every_hotkey() {
        // Keep this list equal to the keys in `App::handle_keys` and in the dialogs.
        for key in ["Space", "Left", "Right", "G", "Shift", "T", "C", "F", "Enter", "Esc", "Cmd", "O"] {
            assert!(MANUAL_HTML.contains(&format!("<kbd>{key}</kbd>")), "hotkey {key} is not in the manual");
        }
    }

    #[test]
    fn manual_has_all_sections() {
        for section in ["Hotkeys", "Open and close a folder", "Go to a time", "Bbox capture", "Settings"] {
            assert!(MANUAL_HTML.contains(&format!("<h2>{section}</h2>")), "no section {section}");
        }
    }

    #[test]
    fn manual_shows_the_app_version() {
        let html = manual_html();
        let (_, body) = html.split_once("<body>").unwrap();
        assert!(body.contains(&format!("<p class=\"version\">Version {APP_VERSION}</p>")), "no version line");
        assert!(!body.contains(VERSION_PLACEHOLDER), "the placeholder is left in the page");
        // The version is the package version, for example 0.1.0.
        assert!(APP_VERSION.split('.').count() == 3 && APP_VERSION.chars().all(|c| c.is_ascii_digit() || c == '.'));
        // The source file has exactly one placeholder in the page content.
        assert_eq!(MANUAL_HTML.split_once("<body>").unwrap().1.matches(VERSION_PLACEHOLDER).count(), 1);
    }

    #[test]
    fn manual_file_holds_the_manual_with_the_version() {
        let path = write_manual().unwrap();
        assert_eq!(path.file_name().unwrap(), "manual.html");
        assert_eq!(std::fs::read_to_string(path).unwrap(), manual_html());
    }
}
