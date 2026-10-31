//! Captures one or two pad buttons in real time to open the in-game cheat menu. XInput only (Windows); it is loaded
//! on demand so the launcher does not depend on a specific `xinput.dll` and the capture fails cleanly without it.

use std::time::{Duration, Instant};

use serde::Serialize;

/// XInput `wButtons` bits.
pub mod bits {
    pub const DPAD_UP: u16 = 0x0001;
    pub const DPAD_DOWN: u16 = 0x0002;
    pub const DPAD_LEFT: u16 = 0x0004;
    pub const DPAD_RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const LEFT_THUMB: u16 = 0x0040;
    pub const RIGHT_THUMB: u16 = 0x0080;
    pub const LEFT_SHOULDER: u16 = 0x0100;
    pub const RIGHT_SHOULDER: u16 = 0x0200;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;
}

/// raylib button code the runtime expects, or 0 for a button it has no use for.
pub fn raylib_button(bit: u16) -> u8 {
    match bit {
        bits::DPAD_UP => 1,
        bits::DPAD_RIGHT => 2,
        bits::DPAD_DOWN => 3,
        bits::DPAD_LEFT => 4,
        bits::Y => 5,
        bits::B => 6,
        bits::A => 7,
        bits::X => 8,
        bits::LEFT_SHOULDER => 9,
        bits::RIGHT_SHOULDER => 11,
        bits::BACK => 13,
        bits::START => 15,
        bits::LEFT_THUMB => 16,
        bits::RIGHT_THUMB => 17,
        _ => 0,
    }
}

pub fn button_label(bit: u16) -> &'static str {
    match bit {
        bits::DPAD_UP => "D-pad ↑",
        bits::DPAD_RIGHT => "D-pad →",
        bits::DPAD_DOWN => "D-pad ↓",
        bits::DPAD_LEFT => "D-pad ←",
        bits::Y => "Y / △",
        bits::B => "B / ○",
        bits::A => "A / ✕",
        bits::X => "X / □",
        bits::LEFT_SHOULDER => "LB / L1",
        bits::RIGHT_SHOULDER => "RB / R1",
        bits::BACK => "Back / Select",
        bits::START => "Start",
        bits::LEFT_THUMB => "LS / L3",
        bits::RIGHT_THUMB => "RS / R3",
        _ => "",
    }
}

/// The two lowest set bits of `mask`: a shortcut is at most two buttons.
pub fn first_two(mask: u16) -> u16 {
    let mut result = 0u16;
    for bit in 0..16 {
        if result.count_ones() >= 2 {
            break;
        }
        if mask & (1 << bit) != 0 {
            result |= 1 << bit;
        }
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Captured {
    /// raylib codes joined by '+', e.g. "6+9".
    pub binding: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureResult {
    Captured(Captured),
    /// Buttons were held, but none of them has a raylib code.
    Unknown,
}

/// The press detector: the first press opens a 350 ms window in which a second button can join the first.
pub struct Capture {
    mask: u16,
    last: u16,
    started: Option<Instant>,
}

pub const WINDOW: Duration = Duration::from_millis(350);

impl Capture {
    pub fn new() -> Self {
        Self { mask: 0, last: 0, started: None }
    }

    /// Feeds the buttons currently held. Returns the result once the window has expired.
    pub fn poll(&mut self, buttons: u16, now: Instant) -> Option<CaptureResult> {
        if self.mask == 0 && self.last == 0 && buttons != 0 {
            self.mask = first_two(buttons);
            self.started = Some(now);
        } else if self.mask != 0 {
            self.mask = first_two(self.mask | buttons);
        }
        self.last = buttons;
        let started = self.started?;
        if self.mask == 0 || now.duration_since(started) < WINDOW {
            return None;
        }
        Some(Self::result(self.mask))
    }

    fn result(mask: u16) -> CaptureResult {
        let (mut codes, mut labels) = (Vec::new(), Vec::new());
        for bit in 0..16 {
            let single = 1u16 << bit;
            if mask & single == 0 {
                continue;
            }
            let code = raylib_button(single);
            if code > 0 {
                codes.push(code.to_string());
                labels.push(button_label(single));
            }
        }
        if codes.is_empty() {
            CaptureResult::Unknown
        } else {
            CaptureResult::Captured(Captured { binding: codes.join("+"), label: labels.join(" + ") })
        }
    }
}

impl Default for Capture {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
mod xinput {
    use std::sync::OnceLock;

    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
    use windows_sys::Win32::UI::Input::XboxController::XINPUT_STATE;

    type GetState = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;

    fn function() -> Option<GetState> {
        static FUNCTION: OnceLock<Option<usize>> = OnceLock::new();
        let address = *FUNCTION.get_or_init(|| {
            // SAFETY: loading a system DLL by name and resolving one export.
            unsafe {
                let mut module = LoadLibraryA(c"xinput1_4.dll".as_ptr() as *const u8);
                if module.is_null() {
                    module = LoadLibraryA(c"xinput9_1_0.dll".as_ptr() as *const u8);
                }
                if module.is_null() {
                    return None;
                }
                GetProcAddress(module, c"XInputGetState".as_ptr() as *const u8).map(|f| f as usize)
            }
        });
        // SAFETY: the address came from GetProcAddress for XInputGetState, whose signature is `GetState`.
        address.map(|a| unsafe { std::mem::transmute::<usize, GetState>(a) })
    }

    pub fn available() -> bool {
        function().is_some()
    }

    /// Buttons held on the first connected pad (0 if none).
    pub fn buttons() -> u16 {
        let Some(get_state) = function() else { return 0 };
        for index in 0..4 {
            // SAFETY: zeroed XINPUT_STATE is a valid out-parameter.
            let mut state: XINPUT_STATE = unsafe { std::mem::zeroed() };
            if unsafe { get_state(index, &mut state) } == 0 {
                return state.Gamepad.wButtons;
            }
        }
        0
    }
}

/// `true` where a pad can be read at all.
pub fn supported() -> bool {
    #[cfg(windows)]
    {
        xinput::available()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Buttons held right now on the first connected pad.
pub fn read_buttons() -> u16 {
    #[cfg(windows)]
    {
        xinput::buttons()
    }
    #[cfg(not(windows))]
    {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_most_two_buttons_are_kept() {
        assert_eq!(first_two(bits::A | bits::B | bits::X), bits::A | bits::B);
        assert_eq!(first_two(0), 0);
        assert_eq!(first_two(bits::Y), bits::Y);
    }

    #[test]
    fn codes_and_labels_follow_the_runtime() {
        assert_eq!(raylib_button(bits::A), 7);
        assert_eq!(raylib_button(bits::START), 15);
        assert_eq!(raylib_button(0x0400), 0); // the Guide bit: not mapped
        assert_eq!(button_label(bits::LEFT_SHOULDER), "LB / L1");
    }

    #[test]
    fn a_second_button_inside_the_window_joins_the_first() {
        let start = Instant::now();
        let mut capture = Capture::new();
        assert_eq!(capture.poll(0, start), None);
        assert_eq!(capture.poll(bits::LEFT_SHOULDER, start), None);
        assert_eq!(capture.poll(bits::LEFT_SHOULDER | bits::RIGHT_SHOULDER, start + Duration::from_millis(100)), None);
        assert_eq!(capture.poll(0, start + Duration::from_millis(200)), None);
        let done = capture.poll(0, start + Duration::from_millis(400));
        assert_eq!(
            done,
            Some(CaptureResult::Captured(Captured { binding: "9+11".into(), label: "LB / L1 + RB / R1".into() }))
        );
    }

    #[test]
    fn a_single_button_is_reported_after_the_window() {
        let start = Instant::now();
        let mut capture = Capture::new();
        capture.poll(bits::B, start);
        let done = capture.poll(0, start + Duration::from_millis(351));
        assert_eq!(done, Some(CaptureResult::Captured(Captured { binding: "6".into(), label: "B / ○".into() })));
    }

    #[test]
    fn unmapped_buttons_are_unknown() {
        let start = Instant::now();
        let mut capture = Capture::new();
        capture.poll(0x0400, start);
        assert_eq!(capture.poll(0x0400, start + Duration::from_millis(400)), Some(CaptureResult::Unknown));
    }

    #[test]
    fn buttons_already_held_when_starting_do_not_count_until_released() {
        // The mask only opens on the first poll with nothing held before (last == 0).
        let start = Instant::now();
        let mut capture = Capture::new();
        assert_eq!(capture.poll(bits::A, start), None); // first sample is a press
        assert_eq!(capture.poll(bits::A, start + Duration::from_millis(10)), None);
    }
}
