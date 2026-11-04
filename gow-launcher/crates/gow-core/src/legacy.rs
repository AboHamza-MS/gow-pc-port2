//! One-time import of the settings the Qt/QML launcher stored: `HKCU\Software\GOW_PC\Launcher\settings`
//! on Windows and `~/.config/GOW_PC/Launcher.conf` (`[settings]`) on Linux. Values are read as text and converted
//! per field, so the registry type Qt picked (string or DWORD) does not matter.

use std::collections::BTreeMap;
use std::path::Path;

use crate::settings::Settings;

pub type RawValues = BTreeMap<String, String>;

fn parse_bool(text: &str) -> Option<bool> {
    match text.trim().to_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// Builds settings from raw `name -> text` pairs. Unknown or unparsable values keep the default.
pub fn settings_from_raw(raw: &RawValues) -> Settings {
    let mut s = Settings::default();
    let text = |name: &str, target: &mut String| {
        if let Some(value) = raw.get(name) {
            *target = value.clone();
        }
    };
    let boolean = |name: &str, target: &mut bool| {
        if let Some(value) = raw.get(name).and_then(|v| parse_bool(v)) {
            *target = value;
        }
    };
    let integer = |name: &str, target: &mut i32| {
        if let Some(value) = raw.get(name).and_then(|v| v.trim().parse::<f64>().ok()) {
            *target = value as i32;
        }
    };
    let unsigned = |name: &str, target: &mut u64| {
        if let Some(value) = raw.get(name).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| *v >= 0.0) {
            *target = value as u64;
        }
    };
    let real = |name: &str, target: &mut f64| {
        if let Some(value) = raw.get(name).and_then(|v| v.trim().replace(',', ".").parse::<f64>().ok()) {
            *target = value;
        }
    };

    text("language", &mut s.language);
    text("gpu", &mut s.gpu);
    text("displayMode", &mut s.display_mode);
    integer("widthPx", &mut s.width_px);
    integer("heightPx", &mut s.height_px);
    text("aspectMode", &mut s.aspect_mode);
    boolean("stretch", &mut s.stretch);
    text("filter", &mut s.filter);
    real("sharpness", &mut s.sharpness);
    boolean("fxaa", &mut s.fxaa);
    boolean("progressive480p", &mut s.progressive480p);
    text("internalResolution", &mut s.internal_resolution);
    integer("fpsLimit", &mut s.fps_limit);
    boolean("vsync", &mut s.vsync);
    boolean("showFps", &mut s.show_fps);
    text("elf", &mut s.elf);
    text("iso", &mut s.iso);
    text("executable", &mut s.executable);
    boolean("skipVideos", &mut s.skip_videos);
    boolean("fastBoot", &mut s.fast_boot);
    if let Some(version) = raw.get("settingsVersion").and_then(|v| v.trim().parse::<u32>().ok()) {
        s.settings_version = version;
    }
    boolean("dmc5Controls", &mut s.dmc5_controls);
    text("padButtons", &mut s.pad_buttons);
    boolean("cancelAnimations", &mut s.cancel_animations);
    boolean("escQuits", &mut s.esc_quits);
    boolean("minimizeOnPlay", &mut s.minimize_on_play);
    unsigned("secondsPlayed", &mut s.seconds_played);
    unsigned("lastSession", &mut s.last_session);
    text("keys", &mut s.keys);
    text("cheats", &mut s.cheats);
    text("cheatsShortcut", &mut s.cheats_shortcut);
    boolean("retroAchievementsEnabled", &mut s.retro_achievements_enabled);
    text("retroAchievementsUser", &mut s.retro_achievements_user);
    boolean("retroAchievementsHardcore", &mut s.retro_achievements_hardcore);
    boolean("retroAchievementsNotifications", &mut s.retro_achievements_notifications);
    text("optimizationDestination", &mut s.optimization_destination);
    boolean("optimizationEnabled", &mut s.optimization_enabled);
    boolean("optimizeDuringGame", &mut s.optimize_during_game);
    let mut compiled = s.optimization_compiled as i32;
    integer("optimizationCompiled", &mut compiled);
    s.optimization_compiled = compiled.max(0) as u32;
    s
}

/// Reads `[settings]` of a Qt ini file (`key=value`, values possibly quoted).
pub fn read_ini(text: &str) -> RawValues {
    let mut values = RawValues::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = section.eq_ignore_ascii_case("settings");
            continue;
        }
        if !inside || line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim();
            let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value);
            values.insert(key.trim().to_string(), value.replace("\\\\", "\\"));
        }
    }
    values
}

#[cfg(windows)]
pub fn read_registry() -> Option<RawValues> {
    use winreg::enums::RegType;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::{RegKey, RegValue};

    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\GOW_PC\Launcher\settings", KEY_READ)
        .ok()?;
    let mut values = RawValues::new();
    for entry in key.enum_values().filter_map(Result::ok) {
        let (name, value): (String, RegValue) = entry;
        let text = match value.vtype {
            RegType::REG_DWORD if value.bytes.len() >= 4 => {
                u32::from_le_bytes([value.bytes[0], value.bytes[1], value.bytes[2], value.bytes[3]]).to_string()
            }
            RegType::REG_SZ | RegType::REG_EXPAND_SZ => {
                let units: Vec<u16> = value.bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
            }
            _ => continue,
        };
        values.insert(name, text);
    }
    Some(values)
}

#[cfg(not(windows))]
pub fn read_registry() -> Option<RawValues> {
    None
}

/// Settings of the former launcher, if there are any on this machine.
pub fn import_previous() -> Option<Settings> {
    if let Some(raw) = read_registry().filter(|raw| !raw.is_empty()) {
        return Some(settings_from_raw(&raw));
    }
    let config = dirs::config_dir()?.join(crate::paths::ORGANIZATION).join("Launcher.conf");
    import_ini_file(&config)
}

pub fn import_ini_file(path: &Path) -> Option<Settings> {
    let raw = read_ini(&std::fs::read_to_string(path).ok()?);
    (!raw.is_empty()).then(|| settings_from_raw(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(pairs: &[(&str, &str)]) -> RawValues {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn values_convert_per_field() {
        let settings = settings_from_raw(&raw(&[
            ("gpu", "rendimiento"),
            ("widthPx", "1920"),
            ("fxaa", "false"),
            ("vsync", "1"),
            ("sharpness", "0,65"),
            ("settingsVersion", "6"),
            ("secondsPlayed", "3600"),
            ("keys", r#"{"cross":[90]}"#),
        ]));
        assert_eq!(settings.gpu, "rendimiento");
        assert_eq!(settings.width_px, 1920);
        assert!(!settings.fxaa);
        assert!(settings.vsync);
        assert_eq!(settings.sharpness, 0.65);
        assert_eq!(settings.settings_version, 6);
        assert_eq!(settings.seconds_played, 3600);
        assert_eq!(settings.keys, r#"{"cross":[90]}"#);
    }

    #[test]
    fn garbage_keeps_the_defaults() {
        let settings = settings_from_raw(&raw(&[("widthPx", "wide"), ("fxaa", "maybe"), ("secondsPlayed", "-5")]));
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn a_migrated_import_gets_the_current_rules() {
        let mut settings =
            settings_from_raw(&raw(&[("settingsVersion", "5"), ("skipVideos", "true"), ("fpsLimit", "144")]));
        settings.migrate(false, &crate::settings::DisplayInfo::default());
        assert!(!settings.skip_videos); // version 6
        assert_eq!(settings.fps_limit, 60); // version 12: an old FPS mode never turns on the experimental limit
    }

    #[test]
    fn ini_reads_only_the_settings_section() {
        let values = read_ini(
            "[General]\ngpu=wrong\n\n[settings]\ngpu=auto\nelf=\"/home/u/game/SCUS_973.99\"\n; comment\nfxaa=true\n",
        );
        assert_eq!(values["gpu"], "auto");
        assert_eq!(values["elf"], "/home/u/game/SCUS_973.99");
        assert_eq!(values.len(), 3);
    }
}
