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
