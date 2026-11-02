//! What the launcher remembers about the last game session (`logs/last-session.json`) and the notes the player writes
//! about a problem (`logs/markers.jsonl`). Both are plain files in the launcher's logs folder, so a report can be made
//! after a crash with no collector process running.

use std::fs;
use std::io;

use serde::{Deserialize, Serialize};

use crate::paths::AppPaths;
use crate::process::GameFinished;

pub const LAST_SESSION_FILE: &str = "last-session.json";
pub const MARKERS_FILE: &str = "markers.jsonl";
/// Markers kept; older ones are dropped when a new one is added.
pub const MAX_MARKERS: usize = 50;
/// Characters a marker keeps.
pub const MAX_MARKER_CHARS: usize = 200;

/// The last game session, written when it starts (`running`) and again when it ends.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LastSession {
    /// Unix seconds.
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub failed: bool,
    pub seconds: u64,
    /// The failure summary lines of the log (`process::summarize`), raw: the report redacts them.
    pub summary: Vec<String>,
    /// The i18n key of a recognized failure (`failureOpenGl`...).
    pub failure_kind: Option<String>,
}

fn session_file(paths: &AppPaths) -> std::path::PathBuf {
    paths.logs_dir().join(LAST_SESSION_FILE)
}

fn write_session(paths: &AppPaths, session: &LastSession) {
    let _ = fs::create_dir_all(paths.logs_dir());
    if let Ok(text) = serde_json::to_string_pretty(session) {
        let _ = fs::write(session_file(paths), text);
    }
}

/// The game just started.
pub fn session_started(paths: &AppPaths, now: u64) {
    write_session(paths, &LastSession { started_at: now, running: true, ..LastSession::default() });
}

/// The game did not start after all: puts back what [`session_started`] replaced.
pub fn session_not_started(paths: &AppPaths, previous: Option<LastSession>) {
    match previous {
        Some(session) => write_session(paths, &session),
        None => {
            let _ = fs::remove_file(session_file(paths));
        }
    }
}

/// The game ended: what the launcher knows about how.
pub fn session_ended(paths: &AppPaths, started_at: u64, finished: &GameFinished, now: u64) {
    write_session(
        paths,
        &LastSession {
            started_at,
            ended_at: Some(now),
            running: false,
            exit_code: finished.code,
            failed: finished.failed,
            seconds: finished.seconds,
            summary: finished.summary.lines().map(str::to_string).collect(),
            failure_kind: finished.failure_kind.clone(),
        },
    );
}

pub fn read_session(paths: &AppPaths) -> Option<LastSession> {
    serde_json::from_slice(&fs::read(session_file(paths)).ok()?).ok()
}

/// A note about a problem, with when it was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    /// Unix seconds.
    pub at: u64,
    /// Seconds since the game started, when the game was running.
    pub since_game_start: Option<u64>,
    pub text: String,
}

/// The player's text without control characters (line breaks and tabs become spaces), trimmed and cut to
/// [`MAX_MARKER_CHARS`] characters.
pub fn clean_marker_text(text: &str) -> String {
    let spaced: String = text
        .chars()
        .filter_map(|c| match c {
            '\n' | '\r' | '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect();
    spaced.trim().chars().take(MAX_MARKER_CHARS).collect::<String>().trim_end().to_string()
}

pub fn read_markers(paths: &AppPaths) -> Vec<Marker> {
    let text = fs::read_to_string(paths.logs_dir().join(MARKERS_FILE)).unwrap_or_default();
    text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// Appends a marker and keeps the last [`MAX_MARKERS`]. `None` when the text is empty once cleaned. While the game runs
/// the marker records how far into the session it was written.
pub fn add_marker(paths: &AppPaths, text: &str, now: u64, game_running: bool) -> io::Result<Option<Marker>> {
    let text = clean_marker_text(text);
    if text.is_empty() {
        return Ok(None);
    }
    let since_game_start = game_running
        .then(|| read_session(paths))
        .flatten()
        .filter(|session| session.running)
        .map(|session| now.saturating_sub(session.started_at));
    let marker = Marker { at: now, since_game_start, text };
    let mut markers = read_markers(paths);
    markers.push(marker.clone());
    let skip = markers.len().saturating_sub(MAX_MARKERS);
    let lines: Vec<String> = markers[skip..].iter().filter_map(|marker| serde_json::to_string(marker).ok()).collect();
    fs::create_dir_all(paths.logs_dir())?;
    fs::write(paths.logs_dir().join(MARKERS_FILE), lines.join("\n") + "\n")?;
    Ok(Some(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, AppPaths) {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(directory.path().join("data"), directory.path().join("cache"));
        (directory, paths)
    }

    #[test]
    fn markers_keep_the_last_fifty() {
        let (_directory, paths) = paths();
        for index in 0..60 {
            add_marker(&paths, &format!("note {index}"), 1000 + index, false).unwrap();
        }
        let markers = read_markers(&paths);
        assert_eq!(markers.len(), MAX_MARKERS);
        assert_eq!(markers.first().unwrap().text, "note 10");
        assert_eq!(markers.last().unwrap().text, "note 59");
    }

    #[test]
    fn marker_text_is_truncated_and_stripped_of_control_characters() {
        assert_eq!(clean_marker_text("  fell\nthrough\tthe\u{0}floor\u{7f} \u{1b}[31m "), "fell through thefloor [31m");
        let long = "é".repeat(500);
        assert_eq!(clean_marker_text(&long).chars().count(), MAX_MARKER_CHARS);
        assert_eq!(clean_marker_text(" \n\u{0} "), "");
        let (_directory, paths) = paths();
        assert_eq!(add_marker(&paths, " \n ", 5, false).unwrap(), None);
        assert!(read_markers(&paths).is_empty());
    }

    #[test]
    fn a_marker_written_during_the_game_says_how_far_in_it_was() {
        let (_directory, paths) = paths();
        session_started(&paths, 1000);
        let marker = add_marker(&paths, "boss glitch", 1090, true).unwrap().unwrap();
        assert_eq!(marker.since_game_start, Some(90));
        // The game is not running (a stale file from a launcher that was killed): no offset.
        assert_eq!(add_marker(&paths, "later", 5000, false).unwrap().unwrap().since_game_start, None);
    }

    #[test]
    fn a_game_that_never_started_leaves_the_previous_session() {
        let (_directory, paths) = paths();
        session_started(&paths, 50);
        let before = read_session(&paths);
        session_started(&paths, 60);
        session_not_started(&paths, before.clone());
        assert_eq!(read_session(&paths), before);
        session_not_started(&paths, None);
        assert_eq!(read_session(&paths), None);
    }

    #[test]
    fn the_session_is_written_at_start_and_at_the_end() {
        let (_directory, paths) = paths();
        assert_eq!(read_session(&paths), None);
        session_started(&paths, 100);
        assert_eq!(read_session(&paths).map(|s| (s.running, s.started_at, s.ended_at)), Some((true, 100, None)));
        let finished = GameFinished {
            code: Some(3),
            failed: true,
            seconds: 42,
            summary: "a\nb".into(),
            failure_kind: Some("failureOpenGl".into()),
        };
        session_ended(&paths, 100, &finished, 142);
        let session = read_session(&paths).unwrap();
        assert_eq!(
            session,
            LastSession {
                started_at: 100,
                ended_at: Some(142),
                running: false,
                exit_code: Some(3),
                failed: true,
                seconds: 42,
                summary: vec!["a".into(), "b".into()],
                failure_kind: Some("failureOpenGl".into()),
            }
        );
    }
}
