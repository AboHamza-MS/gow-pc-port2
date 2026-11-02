//! Names the failures the launcher can explain to the player, so the interface shows a short localized hint above the raw
//! text. Only signatures that are verified are recognized: our own scripts' and runtime's English messages, CMake's
//! (always English) and Windows exit codes. Windows error texts are localized, so none is matched.

/// A failure with a known cause and a known remedy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// An installation kit file is gone (an antivirus, usually) and the bundled copy could not restore it.
    KitFile,
    /// No CMake that can configure the build (3.21 or newer, with `cmcldeps.exe`).
    CmakeUnusable,
    /// `install.ps1`'s `Download` could not fetch a file.
    Download,
    /// The game could not create an OpenGL context.
    OpenGl,
    /// Windows could not load a DLL the game needs (`STATUS_DLL_NOT_FOUND`).
    MissingDll,
    /// A DLL or the executable has the wrong architecture or is damaged (`STATUS_INVALID_IMAGE_FORMAT`).
    BadImage,
    // TODO needs a real log sample: DiskSpace and AccessDenied. The Windows texts ("not enough space on the disk",
    // "Access is denied") are localized and `install.ps1` prints no message of its own for either, so nothing is guessed.
}

impl FailureKind {
    /// The interface string that explains it.
    pub fn key(self) -> &'static str {
        match self {
            Self::KitFile => "failureKitFile",
            Self::CmakeUnusable => "failureCmakeUnusable",
            Self::Download => "failureDownload",
            Self::OpenGl => "failureOpenGl",
            Self::MissingDll => "failureMissingDll",
            Self::BadImage => "failureBadImage",
        }
    }
}

/// `0xC0000135`: Windows could not find a DLL the executable imports.
const STATUS_DLL_NOT_FOUND: i32 = -1_073_741_515;
/// `0xC000007B`: the image (or one of its DLLs) is for another architecture or is damaged.
const STATUS_INVALID_IMAGE_FORMAT: i32 = -1_073_741_701;

/// Looks at `last_error` (the installer's last `[error]` line) first, then at the last lines it printed.
pub fn classify_install_failure(last_error: &str, recent_lines: &[String]) -> Option<FailureKind> {
    std::iter::once(last_error).chain(recent_lines.iter().map(String::as_str)).find_map(install_line)
}

fn install_line(line: &str) -> Option<FailureKind> {
    let lower = line.to_lowercase();
    if lower.contains("cmake 3.21 or newer with cmcldeps.exe next to it is required")
        // CMake's own message when it is too old to take the build folder as it is called.
        || (lower.contains("the source directory") && lower.contains("does not exist"))
    {
        return Some(FailureKind::CmakeUnusable);
    }
    if lower.contains("could not download") {
        return Some(FailureKind::Download);
    }
    None
}

/// `true` if `text` has `OpenGL <digit>.<digit> core context unavailable`: the runtime's message when the driver gives no
/// core profile context. The version changes with the minimum the renderer asks for, so it is not part of the match.
fn mentions_missing_opengl_context(text: &str) -> bool {
    let bytes = text.as_bytes();
    let (head, tail) = ("OpenGL ", " core context unavailable");
    text.match_indices(head).any(|(at, _)| {
        let rest = &bytes[at + head.len()..];
        rest.len() >= 3 + tail.len()
            && rest[0].is_ascii_digit()
            && rest[1] == b'.'
            && rest[2].is_ascii_digit()
            && rest[3..].starts_with(tail.as_bytes())
    })
}

/// `log_text` is the game's whole log; `exit_code` how it ended.
pub fn classify_game_failure(log_text: &str, exit_code: Option<i32>) -> Option<FailureKind> {
    if mentions_missing_opengl_context(log_text) {
        return Some(FailureKind::OpenGl);
    }
    match exit_code {
        Some(STATUS_DLL_NOT_FOUND) => Some(FailureKind::MissingDll),
        Some(STATUS_INVALID_IMAGE_FORMAT) => Some(FailureKind::BadImage),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn install_failure_classification() {
        // The last error line decides first.
        let cmake =
            "A CMake 3.21 or newer with cmcldeps.exe next to it is required, but cmake (3.15.2) does not meet that";
        assert_eq!(classify_install_failure(cmake, &[]), Some(FailureKind::CmakeUnusable));
        assert_eq!(
            classify_install_failure("", &lines(&["CMake Error: The source directory \"C:/g/build\" does not exist."])),
            Some(FailureKind::CmakeUnusable)
        );
        let download = "Could not download https://example.org/x.zip (timed out). Check the Internet connection.";
        assert_eq!(classify_install_failure(download, &[]), Some(FailureKind::Download));
        // Without an `[error]` line, the last lines are searched.
        assert_eq!(classify_install_failure("", &lines(&["==> Downloading", download])), Some(FailureKind::Download));
        // Case does not matter.
        assert_eq!(classify_install_failure("COULD NOT DOWNLOAD x", &[]), Some(FailureKind::Download));
        // What is not recognized gets no hint.
        assert_eq!(classify_install_failure("ninja ended with code 1", &lines(&["[3/9] cl x.cpp"])), None);
        assert_eq!(classify_install_failure("", &[]), None);
    }

    #[test]
    fn game_failure_classification() {
        let log =
            |version: &str| format!("[gs] start\n[gs-hw] GL unavailable: OpenGL {version} core context unavailable\n");
        assert_eq!(classify_game_failure(&log("4.6"), Some(1)), Some(FailureKind::OpenGl));
        // The runtime fix lowers the minimum: the hint still applies.
        assert_eq!(classify_game_failure(&log("4.3"), Some(1)), Some(FailureKind::OpenGl));
        assert_eq!(classify_game_failure("OpenGL 4.6 core context unavailable", None), Some(FailureKind::OpenGl));
        assert_eq!(classify_game_failure("", Some(-1_073_741_515)), Some(FailureKind::MissingDll));
        assert_eq!(classify_game_failure("", Some(-1_073_741_701)), Some(FailureKind::BadImage));
        // An ordinary crash, an unrelated OpenGL line and a clean exit get no hint.
        assert_eq!(classify_game_failure("[gow] fatal: bad thing", Some(-1_073_741_819)), None);
        assert_eq!(classify_game_failure("OpenGL 4.6 core context created", Some(1)), None);
        assert_eq!(classify_game_failure("OpenGL x.6 core context unavailable", Some(1)), None);
        assert_eq!(classify_game_failure("", Some(0)), None);
        assert_eq!(classify_game_failure("", None), None);
    }

    #[test]
    fn every_kind_has_its_own_key() {
        let kinds = [
            FailureKind::KitFile,
            FailureKind::CmakeUnusable,
            FailureKind::Download,
            FailureKind::OpenGl,
            FailureKind::MissingDll,
            FailureKind::BadImage,
        ];
        let mut keys: Vec<_> = kinds.iter().map(|kind| kind.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), kinds.len());
        assert!(keys.iter().all(|key| key.starts_with("failure")));
    }
}
