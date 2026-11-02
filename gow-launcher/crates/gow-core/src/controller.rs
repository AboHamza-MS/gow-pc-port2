//! The controller picker: the list of joysticks the game saw the last time it ran, and the checks on the device the player chose.
//!
//! The game writes the list (`GOW_PAD_LIST_FILE`, see `gow_pad_map.h` in the runtime) because only it knows what GLFW reports: the
//! index, name and GUID of every joystick, whether an SDL mapping exists for it (without one every button reads as released), and
//! how many raw buttons, hats and axes it has. The launcher shows the last list in the Controls tab and passes the chosen GUID back
//! as `GOW_PAD_DEVICE`.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One joystick as GLFW listed it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PadDevice {
    /// GLFW's joystick id (the game only reads ids 0 to 3).
    pub index: u32,
    pub name: String,
    /// GLFW's 32 hex digit identifier: the same for two pads of the same model.
    pub guid: String,
    /// GLFW has an SDL mapping for it. Without one the game sees every button released.
    pub mapped: bool,
    pub buttons: u32,
    pub hats: u32,
    pub axes: u32,
}

/// What the game wrote, plus when.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PadDeviceList {
    pub schema: u32,
    /// The GUID the game was asked for (empty: automatic).
    pub preferred: String,
    /// What the game did with it: `automatic`, `found`, `absent` or `outOfRange`.
    pub choice: String,
    /// The joystick the game read when the list was written (-1: none).
    pub active: i32,
    pub devices: Vec<PadDevice>,
    /// Unix seconds of the last change of the file, filled by [`read`] (0 when unknown).
    pub seen_at: u64,
}

/// A GLFW joystick GUID: 32 hexadecimal digits.
pub fn is_guid(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The list in `text`; `None` for anything that is not a list of this schema.
pub fn parse(text: &str) -> Option<PadDeviceList> {
    let list: PadDeviceList = serde_json::from_str(text).ok()?;
    (list.schema == 1).then_some(list)
}

/// The list the game left in `file`, with the time it was written. A missing or unreadable file is `None`.
pub fn read(file: &Path) -> Option<PadDeviceList> {
    // Lossy: the game writes UTF-8, but a device name from the system may not be valid UTF-8.
    let text = String::from_utf8_lossy(&std::fs::read(file).ok()?).into_owned();
    let mut list = parse(&text)?;
    list.seen_at = std::fs::metadata(file)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_secs());
    Some(list)
}

// ------------------------------------------------------------------------------------------------------ manual mapping

/// The SDL names of what a mapping can say a physical input is (the `guide` button included, which the game does not read).
pub const SDL_TARGETS: [&str; 21] = [
    "a",
    "b",
    "x",
    "y",
    "back",
    "guide",
    "start",
    "leftstick",
    "rightstick",
    "leftshoulder",
    "rightshoulder",
    "dpup",
    "dpdown",
    "dpleft",
    "dpright",
    "leftx",
    "lefty",
    "rightx",
    "righty",
    "lefttrigger",
    "righttrigger",
];

/// Which half of an axis counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    Whole,
    Positive,
    Negative,
}

/// One raw input of a joystick, in the numbering GLFW reports (the one an SDL mapping is written in).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawInput {
    Button(u32),
    /// A hat and one of its direction bits: 1 up, 2 right, 4 down, 8 left.
    Hat {
        index: u32,
        mask: u8,
    },
    Axis {
        index: u32,
        half: Half,
        invert: bool,
    },
}

/// Largest raw indices a mapping may name: far above any real pad, and a typo guard.
const MAX_BUTTON: u32 = 255;
const MAX_HAT: u32 = 31;
const MAX_AXIS: u32 = 63;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappingError {
    /// The text has no mapping at all.
    Empty,
    NotAssignment(String),
    UnknownTarget(String),
    DuplicateTarget(String),
    BadSource {
        target: String,
        source: String,
    },
    /// Nothing presses start: the game could not be started with this pad.
    NoStart,
}

impl std::fmt::Display for MappingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty"),
            Self::NotAssignment(entry) => write!(f, "'{entry}' is not target:source"),
            Self::UnknownTarget(name) => write!(f, "unknown target '{name}'"),
            Self::DuplicateTarget(name) => write!(f, "target '{name}' appears twice"),
            Self::BadSource { target, source } => write!(f, "'{source}' is not a valid input for '{target}'"),
            Self::NoStart => write!(f, "nothing is mapped to start"),
        }
    }
}

/// SDL's spelling of a raw input: `b3`, `h0.4`, `a2`, `+a2`, `-a2~`.
pub fn format_raw(raw: &RawInput) -> String {
    match raw {
        RawInput::Button(index) => format!("b{index}"),
        RawInput::Hat { index, mask } => format!("h{index}.{mask}"),
        RawInput::Axis { index, half, invert } => {
            let prefix = match half {
                Half::Whole => "",
                Half::Positive => "+",
                Half::Negative => "-",
            };
            format!("{prefix}a{index}{}", if *invert { "~" } else { "" })
        }
    }
}

fn parse_index(text: &str, max: u32) -> Option<u32> {
    // Digits only: no sign, no leading `+`, no empty text.
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok().filter(|value| *value <= max)
}

/// The inverse of [`format_raw`]; `None` for anything else.
pub fn parse_raw(text: &str) -> Option<RawInput> {
    let (half, rest) = match text.as_bytes().first()? {
        b'+' => (Half::Positive, &text[1..]),
        b'-' => (Half::Negative, &text[1..]),
        _ => (Half::Whole, text),
    };
    match rest.as_bytes().first()? {
        b'a' => {
            let (digits, invert) = match rest[1..].strip_suffix('~') {
                Some(digits) => (digits, true),
                None => (&rest[1..], false),
            };
            Some(RawInput::Axis { index: parse_index(digits, MAX_AXIS)?, half, invert })
        }
        b'b' if half == Half::Whole => Some(RawInput::Button(parse_index(&rest[1..], MAX_BUTTON)?)),
        b'h' if half == Half::Whole => {
            let (index, mask) = rest[1..].split_once('.')?;
            let mask = parse_index(mask, 8)?;
            matches!(mask, 1 | 2 | 4 | 8)
                .then_some(RawInput::Hat { index: parse_index(index, MAX_HAT)?, mask: mask as u8 })
        }
        _ => None,
    }
}

/// The `(target, input)` pairs of the fields of an SDL mapping (`a:b0,b:b1,...`, the part after the GUID and the name). Each target
/// at most once, and `start` is required.
pub fn parse_fields(fields: &str) -> Result<Vec<(&'static str, RawInput)>, MappingError> {
    if fields.trim().is_empty() {
        return Err(MappingError::Empty);
    }
    let mut pairs: Vec<(&'static str, RawInput)> = Vec::new();
    // One trailing comma is how SDL lines end.
    for entry in fields.strip_suffix(',').unwrap_or(fields).split(',') {
        let Some((target, source)) = entry.split_once(':') else {
            return Err(MappingError::NotAssignment(entry.to_string()));
        };
        let target = SDL_TARGETS
            .iter()
            .find(|name| **name == target)
            .ok_or_else(|| MappingError::UnknownTarget(target.to_string()))?;
        if pairs.iter().any(|(listed, _)| listed == target) {
            return Err(MappingError::DuplicateTarget(target.to_string()));
        }
        let raw = parse_raw(source)
            .ok_or_else(|| MappingError::BadSource { target: target.to_string(), source: source.to_string() })?;
        pairs.push((target, raw));
    }
    if !pairs.iter().any(|(target, _)| *target == "start") {
        return Err(MappingError::NoStart);
    }
    Ok(pairs)
}

/// The fields for the player's choices, in a fixed order whatever order they were made in; the same checks as [`parse_fields`].
pub fn build_fields(choices: &[(&str, RawInput)]) -> Result<String, MappingError> {
    if let Some((unknown, _)) = choices.iter().find(|(chosen, _)| !SDL_TARGETS.contains(chosen)) {
        return Err(MappingError::UnknownTarget(unknown.to_string()));
    }
    if let Some((duplicate, _)) =
        choices.iter().find(|(chosen, _)| choices.iter().filter(|(other, _)| other == chosen).count() > 1)
    {
        return Err(MappingError::DuplicateTarget(duplicate.to_string()));
    }
    let mut text = Vec::new();
    for target in SDL_TARGETS {
        if let Some((_, raw)) = choices.iter().find(|(chosen, _)| *chosen == target) {
            text.push(format!("{target}:{}", format_raw(raw)));
        }
    }
    let fields = text.join(",");
    parse_fields(&fields)?;
    Ok(fields)
}

pub fn fields_are_valid(fields: &str) -> bool {
    parse_fields(fields).is_ok()
}

/// What the game gets in `GOW_PAD_SDL_MAPPING`: `<guid>,<name>,<fields>`. The game adds the platform GLFW wants. `None` unless the
/// GUID and the fields are valid. The name only labels the mapping: commas and line breaks in it are replaced.
pub fn mapping_line(guid: &str, name: &str, fields: &str) -> Option<String> {
    if !is_guid(guid) || !fields_are_valid(fields) {
        return None;
    }
    let clean: String =
        name.chars().map(|c| if c == ',' || c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string();
    let clean = if clean.is_empty() { "Controller".to_string() } else { clean };
    Some(format!("{guid},{clean},{}", fields.strip_suffix(',').unwrap_or(fields)))
}

/// Whether every input of the fields exists on a joystick with these counts (the counts the game listed for it).
pub fn fits_device(fields: &str, device: &PadDevice) -> bool {
    parse_fields(fields).is_ok_and(|pairs| {
        pairs.iter().all(|(_, raw)| match raw {
            RawInput::Button(index) => *index < device.buttons,
            RawInput::Hat { index, .. } => *index < device.hats,
            RawInput::Axis { index, .. } => *index < device.axes,
        })
    })
}

// ------------------------------------------------------------------------------------------------------ live raw input

/// What one joystick does right now, from the game (`live.json`): the raw inputs held or moved away from rest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LiveDevice {
    pub index: u32,
    pub guid: String,
    /// Raw buttons held.
    pub buttons: Vec<u32>,
    /// `[hat, direction bits]` for each hat off rest.
    pub hats: Vec<(u32, u32)>,
    /// `[axis, distance from rest in hundredths]` for each axis that moved.
    pub axes: Vec<(u32, i32)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LiveState {
    pub schema: u32,
    /// Grows with every write: a changing beat means the game is alive, a still pad or not.
    pub beat: u64,
    pub devices: Vec<LiveDevice>,
    /// Milliseconds since the file was written, filled by [`read_live`].
    pub age_ms: u64,
}

pub fn parse_live(text: &str) -> Option<LiveState> {
    let state: LiveState = serde_json::from_str(text).ok()?;
    (state.schema == 1).then_some(state)
}

pub fn read_live(file: &Path) -> Option<LiveState> {
    let mut state = parse_live(&String::from_utf8_lossy(&std::fs::read(file).ok()?))?;
    state.age_ms = std::fs::metadata(file)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.elapsed().ok())
        .map_or(u64::MAX, |elapsed| elapsed.as_millis() as u64);
    Some(state)
}

/// Asks the game for the live feed: it writes `live.json` while this file is fresh (touched in the last ten seconds).
pub fn touch_probe(file: &Path) -> std::io::Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(file, b"1")
}

pub fn clear_probe(file: &Path) {
    let _ = std::fs::remove_file(file);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the runtime writes for the dongle of issue #12 (`gow_pad_picker_test.cpp` checks the same text).
    const DONGLE: &str = r#"{"schema":1,"platform":"windows","preferred":"0300dddd5e040000a102000000010000","choice":"found","active":3,"devices":[
        {"index":0,"name":"2.4G Consumer Control","guid":"0300aaaa5e040000a102000000010000","mapped":true,"buttons":1,"hats":0,"axes":0},
        {"index":1,"name":"2.4G Keyboard","guid":"0300bbbb5e040000a102000000010000","mapped":true,"buttons":1,"hats":0,"axes":0},
        {"index":3,"name":"Generic X-Box pad","guid":"0300dddd5e040000a102000000010000","mapped":false,"buttons":15,"hats":1,"axes":6}]}"#;

    #[test]
    fn the_list_the_game_writes_is_read() {
        let list = parse(DONGLE).unwrap();
        assert_eq!(list.choice, "found");
        assert_eq!(list.active, 3);
        assert_eq!(list.preferred, "0300dddd5e040000a102000000010000");
        assert_eq!(list.devices.len(), 3);
        let pad = &list.devices[2];
        assert_eq!((pad.index, pad.name.as_str(), pad.mapped), (3, "Generic X-Box pad", false));
        assert_eq!((pad.buttons, pad.hats, pad.axes), (15, 1, 6));
        assert!(is_guid(&pad.guid));
    }

    #[test]
    fn an_empty_list_and_a_file_of_another_schema() {
        let empty =
            parse(r#"{"schema":1,"platform":"linux","preferred":"","choice":"automatic","active":-1,"devices":[]}"#)
                .unwrap();
        assert!(empty.devices.is_empty());
        assert_eq!(empty.active, -1);
        assert_eq!(parse(r#"{"schema":2,"devices":[]}"#), None);
        assert_eq!(parse("not json"), None);
        assert_eq!(parse(""), None);
        // A half-written file (the game renames a finished one over the old, but be safe) is not a list.
        assert_eq!(parse(&DONGLE[..DONGLE.len() / 2]), None);
    }

    #[test]
    fn a_name_with_escapes_survives() {
        let text = r#"{"schema":1,"devices":[{"index":0,"name":"Pad \"Two\" \\ with\ttab\nline","guid":"00"}]}"#;
        assert_eq!(parse(text).unwrap().devices[0].name, "Pad \"Two\" \\ with\ttab\nline");
    }

    #[test]
    fn the_file_is_read_with_its_time() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("pads").join("devices.json");
        assert_eq!(read(&file), None);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, DONGLE).unwrap();
        let list = read(&file).unwrap();
        assert_eq!(list.devices.len(), 3);
        assert!(list.seen_at > 1_700_000_000);
        std::fs::write(&file, b"{\"schema\":1,\"devices\":[{\"index\":0,\"name\":\"bad \xff byte\"}]}").unwrap();
        assert_eq!(read(&file).unwrap().devices[0].name, "bad \u{fffd} byte");
    }

    #[derive(serde::Deserialize)]
    struct FieldsCase {
        fields: String,
        valid: bool,
        note: String,
    }

    /// The table `src/lib/sdlmap.test.ts` runs too.
    #[test]
    fn the_shared_table_of_mapping_cases_agrees() {
        let cases: Vec<FieldsCase> = serde_json::from_str(include_str!("../../../src/lib/sdlmap-cases.json")).unwrap();
        assert!(cases.len() > 20);
        for case in cases {
            assert_eq!(fields_are_valid(&case.fields), case.valid, "{:?} ({})", case.fields, case.note);
        }
    }

    #[test]
    fn raw_inputs_are_written_and_read_in_sdls_spelling() {
        let all = [
            (RawInput::Button(0), "b0"),
            (RawInput::Button(255), "b255"),
            (RawInput::Hat { index: 0, mask: 4 }, "h0.4"),
            (RawInput::Hat { index: 2, mask: 1 }, "h2.1"),
            (RawInput::Axis { index: 2, half: Half::Whole, invert: false }, "a2"),
            (RawInput::Axis { index: 2, half: Half::Whole, invert: true }, "a2~"),
            (RawInput::Axis { index: 5, half: Half::Positive, invert: false }, "+a5"),
            (RawInput::Axis { index: 5, half: Half::Negative, invert: true }, "-a5~"),
        ];
        for (raw, text) in all {
            assert_eq!(format_raw(&raw), text);
            assert_eq!(parse_raw(text), Some(raw), "{text}");
        }
        for bad in
            ["", "b", "a", "h0", "h0.3", "x1", "b256", "a64", "h32.1", "b1~", "+b1", "-h0.1", "a1~~", "b 1", "b1.2"]
        {
            assert_eq!(parse_raw(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_players_choices_become_the_fields_in_a_fixed_order() {
        // The 2.4G pad of issue #12 as the game's list describes it: 15 buttons, one hat, six axes (the xpad layout).
        let choices = [
            ("start", RawInput::Button(7)),
            ("a", RawInput::Button(0)),
            ("leftx", RawInput::Axis { index: 0, half: Half::Whole, invert: false }),
            ("dpup", RawInput::Hat { index: 0, mask: 1 }),
            ("lefttrigger", RawInput::Axis { index: 2, half: Half::Positive, invert: false }),
        ];
        let fields = build_fields(&choices).unwrap();
        assert_eq!(fields, "a:b0,start:b7,dpup:h0.1,leftx:a0,lefttrigger:+a2");
        // The same choices in another order give the same text, and the text reads back to the same choices.
        let mut reversed = choices;
        reversed.reverse();
        assert_eq!(build_fields(&reversed).unwrap(), fields);
        let read: Vec<(String, String)> =
            parse_fields(&fields).unwrap().iter().map(|(target, raw)| (target.to_string(), format_raw(raw))).collect();
        assert_eq!(read[1], ("start".to_string(), "b7".to_string()));
        assert_eq!(read.len(), 5);
        // What would make the game unusable or the line malformed is refused.
        assert_eq!(build_fields(&[("a", RawInput::Button(0))]), Err(MappingError::NoStart));
        assert_eq!(build_fields(&[]), Err(MappingError::Empty));
        assert_eq!(
            build_fields(&[("start", RawInput::Button(7)), ("start", RawInput::Button(8))]),
            Err(MappingError::DuplicateTarget("start".into()))
        );
        assert_eq!(
            build_fields(&[("start", RawInput::Button(7)), ("jump", RawInput::Button(1))]),
            Err(MappingError::UnknownTarget("jump".into()))
        );
        // An input no real pad has is refused too, so a built text always reads back.
        assert_eq!(
            build_fields(&[("start", RawInput::Button(7)), ("a", RawInput::Button(999))]),
            Err(MappingError::BadSource { target: "a".into(), source: "b999".into() })
        );
    }

    #[test]
    fn the_line_for_the_game_has_the_guid_a_clean_name_and_the_fields() {
        let guid = "0300dddd5e040000a102000000010000";
        assert_eq!(
            mapping_line(guid, "Generic X-Box pad", "a:b0,start:b7").as_deref(),
            Some("0300dddd5e040000a102000000010000,Generic X-Box pad,a:b0,start:b7")
        );
        // Commas, line breaks and control characters of a name cannot break the line; an empty name gets a label.
        assert_eq!(
            mapping_line(guid, "Pad, \"two\"\nnew\tline", "start:b7").as_deref(),
            Some(format!("{guid},Pad  \"two\" new line,start:b7").as_str())
        );
        assert_eq!(
            mapping_line(guid, "  ", "start:b7").as_deref(),
            Some(format!("{guid},Controller,start:b7").as_str())
        );
        // The trailing comma of an SDL line is not doubled.
        assert_eq!(mapping_line(guid, "P", "start:b7,").as_deref(), Some(format!("{guid},P,start:b7").as_str()));
        // Nothing is built for a bad GUID or bad fields.
        assert_eq!(mapping_line("Generic X-Box pad", "P", "start:b7"), None);
        assert_eq!(mapping_line(guid, "P", "a:b0"), None);
        assert_eq!(mapping_line(guid, "P", ""), None);
    }

    #[test]
    fn a_mapping_fits_a_device_only_when_it_has_those_inputs() {
        let pad = PadDevice { buttons: 15, hats: 1, axes: 6, ..PadDevice::default() };
        assert!(fits_device("a:b14,start:b7,dpup:h0.1,leftx:a5", &pad));
        assert!(!fits_device("a:b15,start:b7", &pad));
        assert!(!fits_device("start:b7,dpup:h1.1", &pad));
        assert!(!fits_device("start:b7,leftx:a6", &pad));
        assert!(!fits_device("a:b0", &pad)); // invalid fields never fit
    }

    #[test]
    fn the_live_feed_of_the_game_is_read() {
        let text = r#"{"schema":1,"beat":7,"devices":[
            {"index":3,"guid":"0300dddd5e040000a102000000010000","buttons":[2],"hats":[[0,4]],"axes":[[1,-90],[2,200]]},
            {"index":0,"guid":"0300aaaa5e040000a102000000010000","buttons":[],"hats":[],"axes":[]}]}"#;
        let state = parse_live(text).unwrap();
        assert_eq!(state.beat, 7);
        assert_eq!(state.devices.len(), 2);
        let pad = &state.devices[0];
        assert_eq!(
            (pad.index, pad.buttons.clone(), pad.hats.clone(), pad.axes.clone()),
            (3, vec![2], vec![(0, 4)], vec![(1, -90), (2, 200)])
        );
        assert_eq!(parse_live(r#"{"schema":2,"devices":[]}"#), None);
        assert_eq!(parse_live("{"), None);
    }

    #[test]
    fn the_probe_file_and_the_live_file_work_through_the_disk() {
        let directory = tempfile::tempdir().unwrap();
        let probe = directory.path().join("pads").join("probe");
        let live = directory.path().join("pads").join("live.json");
        assert_eq!(read_live(&live), None);
        touch_probe(&probe).unwrap();
        assert!(probe.is_file());
        touch_probe(&probe).unwrap(); // touching again is fine
        std::fs::write(&live, r#"{"schema":1,"beat":1,"devices":[]}"#).unwrap();
        let state = read_live(&live).unwrap();
        assert!(state.age_ms < 5_000, "{}", state.age_ms);
        clear_probe(&probe);
        assert!(!probe.exists());
        clear_probe(&probe); // already gone: nothing to do
    }

    #[test]
    fn a_guid_is_32_hex_digits() {
        assert!(is_guid("0300dddd5e040000a102000000010000"));
        assert!(is_guid("0300DDDD5E040000A102000000010000"));
        assert!(!is_guid(""));
        assert!(!is_guid("0300dddd5e040000a1020000000100"));
        assert!(!is_guid("0300dddd5e040000a102000000010000 "));
        assert!(!is_guid("0300dddd5e040000a10200000001000g"));
        assert!(!is_guid("0300dddd5e040000a102000000010000,x"));
    }
}
