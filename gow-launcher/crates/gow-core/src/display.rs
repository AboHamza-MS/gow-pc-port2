//! Refresh rate of the primary display. Its size comes from the window toolkit (it knows the monitor's scale).

/// Current refresh rate in Hz of the primary display; 60 when it cannot be read.
pub fn refresh_rate() -> u32 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS};

        // SAFETY: zeroed DEVMODEW with dmSize set is the documented way to call EnumDisplaySettingsW.
        unsafe {
            let mut mode: DEVMODEW = std::mem::zeroed();
            mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            if EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &mut mode) != 0
                && mode.dmDisplayFrequency > 1
            {
                return mode.dmDisplayFrequency;
            }
        }
    }
    60
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_refresh_rate_is_plausible() {
        let hz = super::refresh_rate();
        assert!((24..=1000).contains(&hz), "{hz}");
    }
}
