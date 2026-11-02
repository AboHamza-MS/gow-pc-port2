//! `PC_PAD_MAP`: which PS2 button each physical gamepad button presses. The game parses the same text (`gow_pad_map.h` in the
//! runtime) and ignores a value it finds invalid, so the launcher only passes the variable when this module agrees it is valid.
//! `src/lib/padmap.ts` is the interface's copy of the rules; both are tested against `src/lib/padmap-cases.json`.
//!
//! The text is `source=target;source=target;...`. Sources are the Xbox names of the physical buttons, targets the PS2 buttons or
//! `none`. A source appears once, several sources may press the same target, a source that is not listed presses nothing, and
//! `start` must be the target of some source (a map without it would make the game impossible to start).

pub const SOURCES: [&str; 16] =
    ["a", "b", "x", "y", "lb", "rb", "lt", "rt", "back", "start", "ls", "rs", "up", "down", "left", "right"];

pub const TARGETS: [&str; 17] = [
    "cross", "circle", "square", "triangle", "l1", "r1", "l2", "r2", "select", "start", "l3", "r3", "up", "down",
    "left", "right", "none",
];

/// Why a value is not a valid map, in the words the game logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PadMapError {
    Empty,
    EmptyEntry,
    NotAssignment(String),
    UnknownSource(String),
    UnknownTarget(String),
    DuplicateSource(String),
    NoStart,
}

impl std::fmt::Display for PadMapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty"),
            Self::EmptyEntry => write!(f, "empty entry"),
            Self::NotAssignment(entry) => write!(f, "'{entry}' is not source=target"),
            Self::UnknownSource(name) => write!(f, "unknown source '{name}'"),
            Self::UnknownTarget(name) => write!(f, "unknown target '{name}'"),
            Self::DuplicateSource(name) => write!(f, "source '{name}' appears twice"),
            Self::NoStart => write!(f, "no button presses start"),
        }
    }
}

/// Trims a space or a tab at either end, and nothing else: the game's parser (`gow_pad_map.h`) does exactly that, so a value
/// with a stray line break (or a no-break space) is as invalid here as it is there, instead of passing in the launcher and
/// being refused by the game.
fn blank_trim(text: &str) -> &str {
    text.trim_matches(|c| c == ' ' || c == '\t')
}

/// The `(source, target)` pairs of a valid map, in the order written.
pub fn parse(value: &str) -> Result<Vec<(&'static str, &'static str)>, PadMapError> {
    if value.is_empty() {
        return Err(PadMapError::Empty);
    }
    let mut entries: Vec<(&'static str, &'static str)> = Vec::new();
    let mut parts: Vec<&str> = value.split(';').collect();
    // One trailing separator is fine.
    if parts.len() > 1 && parts.last().is_some_and(|last| blank_trim(last).is_empty()) {
        parts.pop();
    }
    for part in parts {
        let entry = blank_trim(part);
        if entry.is_empty() {
            return Err(PadMapError::EmptyEntry);
        }
        let Some((source, target)) = entry.split_once('=') else {
            return Err(PadMapError::NotAssignment(entry.to_string()));
        };
        let (source, target) = (blank_trim(source), blank_trim(target));
        let source = SOURCES
            .iter()
            .find(|name| **name == source)
            .ok_or_else(|| PadMapError::UnknownSource(source.to_string()))?;
        let target = TARGETS
            .iter()
            .find(|name| **name == target)
            .ok_or_else(|| PadMapError::UnknownTarget(target.to_string()))?;
        if entries.iter().any(|(listed, _)| listed == source) {
            return Err(PadMapError::DuplicateSource(source.to_string()));
        }
        entries.push((source, target));
    }
    if !entries.iter().any(|(_, target)| *target == "start") {
        return Err(PadMapError::NoStart);
    }
    Ok(entries)
}

pub fn pad_map_is_valid(value: &str) -> bool {
    parse(value).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Case {
        value: String,
        valid: bool,
        note: String,
    }

    /// The table `src/lib/padmap.test.ts` runs too.
    #[test]
    fn the_shared_table_of_cases_agrees() {
        let cases: Vec<Case> = serde_json::from_str(include_str!("../../../src/lib/padmap-cases.json")).unwrap();
        assert!(cases.len() > 15);
        for case in cases {
            assert_eq!(pad_map_is_valid(&case.value), case.valid, "{:?} ({})", case.value, case.note);
        }
    }

    #[test]
    fn the_reasons_are_the_ones_the_game_logs() {
        assert_eq!(parse("a=cross;b=circle"), Err(PadMapError::NoStart));
        assert_eq!(parse("a=cross;a=circle;start=start").unwrap_err().to_string(), "source 'a' appears twice");
        assert_eq!(parse("a=jump").unwrap_err().to_string(), "unknown target 'jump'");
        assert_eq!(parse("guide=a").unwrap_err().to_string(), "unknown source 'guide'");
        assert_eq!(parse("a").unwrap_err().to_string(), "'a' is not source=target");
        assert_eq!(parse("a=cross;start=start").unwrap(), vec![("a", "cross"), ("start", "start")]);
    }

    #[test]
    fn the_names_are_the_ones_the_runtime_knows() {
        assert_eq!(SOURCES.len(), 16);
        assert_eq!(TARGETS.len(), 17);
        let full: Vec<String> = SOURCES.iter().map(|source| format!("{source}=start")).collect();
        assert!(pad_map_is_valid(&full.join(";")));
    }
}
