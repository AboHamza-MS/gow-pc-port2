//! Launcher settings, stored as JSON. Field names and defaults are those of the former QML `Settings` block
//! (`launcher/qml/Main.qml` in the removed Qt launcher; see git history), and `migrate` replays its `settingsVersion` steps.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub const SETTINGS_VERSION: u32 = 11;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub language: String,
    pub gpu: String,
    pub display_mode: String,
    pub width_px: i32,
    pub height_px: i32,
    pub aspect_mode: String,
    /// Widescreen switch: PCSX2's "Widescreen 16:9" patch (Hor+ camera at `aspect_mode`, `PC_WIDESCREEN`). Off keeps the
    /// game's original 4:3 picture.
    pub widescreen: bool,
    pub stretch: bool,
    pub filter: String,
    pub sharpness: f64,
    pub fxaa: bool,
    pub progressive480p: bool,
    /// "auto" or "1".."8" (`PS2X_GS_SCALE`).
    pub internal_resolution: String,
    pub fps_limit: i32,
    pub vsync: bool,
    pub show_fps: bool,
    pub elf: String,
    pub iso: String,
    pub executable: String,
    pub skip_videos: bool,
    pub fast_boot: bool,
    /// Draw the PCSX2-format texture replacements of `<installation>/textures/replacements` (`GOW_TEXTURE_REPLACE`).
    pub custom_textures: bool,
    /// Load the replacements in the background (the game's own texture is drawn until each one is ready); off waits for
    /// every file (`GOW_TEXTURE_REPLACE_SYNC`).
    pub async_textures: bool,
    pub settings_version: u32,
    pub dmc5_controls: bool,
    /// A,B,X,Y -> cross,circle,square,triangle; empty = preset.
    pub pad_buttons: String,
    /// `source=target;...` for every gamepad button (`PC_PAD_MAP`, see `padmap.rs`); empty = the preset and the face buttons above.
    pub pad_map: String,
    pub cancel_animations: bool,
    pub esc_quits: bool,
    pub minimize_on_play: bool,
    pub seconds_played: u64,
    pub last_session: u64,
    /// Key map in JSON (empty = default scheme).
    pub keys: String,
    /// Cheats active at start-up, comma-separated.
    pub cheats: String,
    /// raylib buttons ("6" or "6+9") that open the cheat menu.
    pub cheats_shortcut: String,
    pub retro_achievements_enabled: bool,
    pub retro_achievements_user: String,
    pub retro_achievements_hardcore: bool,
    pub retro_achievements_notifications: bool,
    pub optimization_destination: String,
    pub optimization_enabled: bool,
    pub optimize_during_game: bool,
    pub optimization_compiled: u32,
    /// Download and install a newer launcher at start-up when nothing is running. Off: the launcher only shows a notice.
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: String::new(),
            gpu: "auto".into(),
            display_mode: "fullscreen".into(),
            width_px: 1280,
            height_px: 800,
            aspect_mode: String::new(),
            widescreen: true,
            stretch: false,
            filter: "bilinear".into(),
            sharpness: 0.8,
            fxaa: true,
            progressive480p: true,
            internal_resolution: "2".into(),
            fps_limit: 60,
            vsync: false,
            show_fps: false,
            elf: String::new(),
            iso: String::new(),
            executable: String::new(),
            skip_videos: true,
            fast_boot: false,
            custom_textures: false,
            async_textures: true,
            settings_version: 0,
            dmc5_controls: false,
            pad_buttons: String::new(),
            pad_map: String::new(),
            cancel_animations: true,
            esc_quits: false,
            minimize_on_play: true,
            seconds_played: 0,
            last_session: 0,
            keys: String::new(),
            cheats: String::new(),
            cheats_shortcut: String::new(),
            retro_achievements_enabled: false,
            retro_achievements_user: String::new(),
            retro_achievements_hardcore: false,
            retro_achievements_notifications: true,
            optimization_destination: String::new(),
            optimization_enabled: true,
            // Off: a rebuild running with the game open starves it (the game stops entering its frame function and the
            // picture freezes). Rebuilds run with the game closed, and one started before the game opens is stopped.
            optimize_during_game: false,
            optimization_compiled: 0,
            auto_update: false,
        }
    }
}

/// Main display in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub width_px: u32,
    pub height_px: u32,
    pub hz: u32,
}

impl Default for DisplayInfo {
    fn default() -> Self {
        Self { width_px: 1920, height_px: 1080, hz: 60 }
    }
}

impl DisplayInfo {
    /// Closest aspect the port offers for this display.
    pub fn default_aspect_ratio(&self) -> &'static str {
        let ratio = self.width_px as f64 / self.height_px.max(1) as f64;
        if ratio >= 2.2 {
            "21:9"
        } else if ratio >= 1.7 {
            "16:9"
        } else if ratio >= 1.5 {
            "16:10"
        } else {
            "4:3"
        }
    }
}

impl Settings {
    /// Reads the file; a missing or unreadable file gives the defaults (the caller migrates afterwards).
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    /// Atomic write: a crash in the middle never leaves a half-written file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(&temporary, path)
    }

    /// Replays the `settingsVersion` steps of the QML launcher and fills what is still empty.
    pub fn migrate(&mut self, prefers_portuguese: bool, display: &DisplayInfo) {
        if self.settings_version < 2 {
            // Fast boot came enabled and cut the opening; it is turned off once.
            self.fast_boot = false;
            self.settings_version = 2;
        }
        if self.settings_version < 3 {
            self.internal_resolution = "2".into();
            self.settings_version = 3;
        }
        if self.settings_version < 4 {
            self.settings_version = 4; // historical renderer migration
        }
        if self.settings_version < 5 {
            self.skip_videos = true;
            self.settings_version = 5;
        }
        if self.settings_version < 6 {
            // Waffle's runtime plays the videos natively, with sound.
            self.skip_videos = false;
            self.settings_version = 6;
        }
        if self.settings_version < 7 {
            // Old DMC5 preset that was enabled by default.
            self.dmc5_controls = false;
            self.pad_buttons.clear();
            self.settings_version = 7;
        }
        if self.settings_version < 9 {
            // The game keeps its logic at 60 Hz; 120 only speeds up the presentation.
            self.fps_limit = if self.fps_limit >= 120 { 120 } else { 60 };
            self.settings_version = 9;
        }
        if self.settings_version < 10 {
            // Rebuilding with the game open starves it (long freezes): it is off for everyone once. Whoever wants it back turns
            // the switch on again, now with a warning next to it.
            self.optimize_during_game = false;
            self.settings_version = 10;
        }
        if self.settings_version < 11 {
            // There is no unlimited presentation any more: the modes are 60, 120 and 144, and any other stored value (0 was
            // unlimited) snaps to the nearest one.
            self.fps_limit = match self.fps_limit {
                i32::MIN..=60 => 60,
                61..=120 => 120,
                _ => 144,
            };
            self.settings_version = 11;
        }
        if self.language != "pt-BR" && self.language != "en" {
            self.language = if prefers_portuguese { "pt-BR".into() } else { "en".into() };
        }
        if self.aspect_mode.is_empty() {
            self.aspect_mode = display.default_aspect_ratio().into();
        }
    }

    /// Internal GS scale actually requested: "auto" and anything above 6 fall back to 2x.
    pub fn selected_scale(&self) -> f64 {
        match self.internal_resolution.parse::<f64>() {
            Ok(n) if self.internal_resolution != "auto" && n <= 6.0 => n.max(1.0),
            _ => 2.0,
        }
    }

    /// Back to the defaults of the "Reset" button; paths, cheats' history and RetroAchievements are untouched.
    pub fn reset_game_options(&mut self, display: &DisplayInfo) {
        self.gpu = "auto".into();
        self.display_mode = "fullscreen".into();
        self.width_px = 1280;
        self.height_px = if (display.width_px as f64 / display.height_px.max(1) as f64) < 1.7 { 800 } else { 720 };
        self.aspect_mode = display.default_aspect_ratio().into();
        self.widescreen = true;
        self.stretch = false;
        self.filter = "bilinear".into();
        self.sharpness = 0.8;
        self.fxaa = true;
        self.progressive480p = true;
        self.internal_resolution = "2".into();
        self.fps_limit = 60;
        self.vsync = false;
        self.show_fps = false;
        self.skip_videos = true;
        self.fast_boot = false;
        self.dmc5_controls = false;
        self.pad_buttons.clear();
        self.pad_map.clear();
        self.cancel_animations = true;
        self.esc_quits = false;
        self.minimize_on_play = true;
        self.cheats.clear();
        self.cheats_shortcut.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_install_ends_at_the_current_version_with_videos_on() {
        let mut settings = Settings::default();
        settings.migrate(false, &DisplayInfo::default());
        assert_eq!(settings.settings_version, SETTINGS_VERSION);
        assert!(!settings.skip_videos); // v5 turns them off, v6 puts them back
        assert_eq!(settings.language, "en");
        assert_eq!(settings.aspect_mode, "16:9");
    }

    #[test]
    fn language_follows_the_system_only_when_unsupported() {
        let mut settings = Settings { language: "es".into(), ..Settings::default() };
        settings.migrate(true, &DisplayInfo::default());
        assert_eq!(settings.language, "pt-BR");
        let mut settings = Settings { language: "en".into(), ..Settings::default() };
        settings.migrate(true, &DisplayInfo::default());
        assert_eq!(settings.language, "en");
    }

    #[test]
    fn version_nine_keeps_a_chosen_120() {
        let mut settings = Settings { settings_version: 8, fps_limit: 144, ..Settings::default() };
        settings.migrate(false, &DisplayInfo::default());
        assert_eq!(settings.fps_limit, 120);
        let mut settings = Settings { settings_version: 8, fps_limit: 30, ..Settings::default() };
        settings.migrate(false, &DisplayInfo::default());
        assert_eq!(settings.fps_limit, 60);
    }

    #[test]
    fn a_current_file_is_left_alone() {
        let mut settings = Settings {
            settings_version: SETTINGS_VERSION,
            skip_videos: true,
            fast_boot: true,
            language: "pt-BR".into(),
            aspect_mode: "21:9".into(),
            ..Settings::default()
        };
        let before = settings.clone();
        settings.migrate(false, &DisplayInfo::default());
        assert_eq!(settings, before);
    }

    #[test]
    fn version_ten_turns_off_the_rebuild_during_the_game_once() {
        let mut old = Settings { settings_version: 9, optimize_during_game: true, ..Settings::default() };
        old.migrate(false, &DisplayInfo::default());
        assert!(!old.optimize_during_game);
        assert_eq!(old.settings_version, SETTINGS_VERSION);
        // Whoever turns it back on keeps it: the migration is not replayed.
        let mut chosen = Settings { settings_version: 10, optimize_during_game: true, ..Settings::default() };
        chosen.migrate(false, &DisplayInfo::default());
        assert!(chosen.optimize_during_game);
    }

    #[test]
    fn version_eleven_snaps_the_frame_limit_to_60_120_or_144() {
        for (stored, expected) in [(0, 60), (30, 60), (60, 60), (90, 120), (120, 120), (144, 144), (240, 144)] {
            let mut settings = Settings { settings_version: 10, fps_limit: stored, ..Settings::default() };
            settings.migrate(false, &DisplayInfo::default());
            assert_eq!(settings.fps_limit, expected, "stored {stored}");
        }
    }

    #[test]
    fn aspect_follows_the_display() {
        let wide = DisplayInfo { width_px: 3440, height_px: 1440, hz: 60 };
        let laptop = DisplayInfo { width_px: 1920, height_px: 1200, hz: 60 };
        let old = DisplayInfo { width_px: 1024, height_px: 768, hz: 60 };
        assert_eq!(wide.default_aspect_ratio(), "21:9");
        assert_eq!(laptop.default_aspect_ratio(), "16:10");
        assert_eq!(old.default_aspect_ratio(), "4:3");
    }

    #[test]
    fn scale_falls_back_to_two() {
        let scale = |text: &str| Settings { internal_resolution: text.into(), ..Settings::default() }.selected_scale();
        assert_eq!(scale("auto"), 2.0);
        assert_eq!(scale("8"), 2.0);
        assert_eq!(scale("6"), 6.0);
        assert_eq!(scale("3"), 3.0);
        assert_eq!(scale("0.5"), 1.0);
        assert_eq!(scale("x"), 2.0);
    }

    #[test]
    fn json_round_trip_uses_the_qml_names() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let settings = Settings { cheats_shortcut: "6+9".into(), ..Settings::default() };
        settings.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"cheatsShortcut\""));
        assert!(text.contains("\"progressive480p\""));
        assert_eq!(Settings::load(&path), settings);
    }

    #[test]
    fn an_old_file_has_no_button_map_and_the_reset_clears_one() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        std::fs::write(&path, r#"{"padButtons":"cross,circle,square,triangle","settingsVersion":11}"#).unwrap();
        let mut loaded = Settings::load(&path);
        assert_eq!(loaded.pad_map, "");
        loaded.pad_map = "a=cross;start=start".into();
        loaded.save(&path).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"padMap\""));
        assert_eq!(Settings::load(&path).pad_map, "a=cross;start=start");
        loaded.reset_game_options(&DisplayInfo::default());
        assert_eq!(loaded.pad_map, "");
    }

    #[test]
    fn auto_update_is_off_unless_the_file_says_otherwise() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        // A file written before the option existed.
        std::fs::write(&path, r#"{"gpu":"auto","settingsVersion":11}"#).unwrap();
        let mut loaded = Settings::load(&path);
        assert!(!loaded.auto_update);
        loaded.migrate(false, &DisplayInfo::default());
        assert!(!loaded.auto_update);
        std::fs::write(&path, r#"{"autoUpdate":true}"#).unwrap();
        assert!(Settings::load(&path).auto_update);
    }

    #[test]
    fn unknown_and_missing_fields_are_tolerated() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        std::fs::write(&path, r#"{"gpu":"ahorro","futureField":1}"#).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.gpu, "ahorro");
        assert_eq!(loaded.filter, "bilinear");
        assert_eq!(Settings::load(&directory.path().join("missing.json")), Settings::default());
    }
}
