//! The notification-area icons, drawn by scripts/make-icons.swift and embedded here: masks in
//! outline when show mode is off, filled when it's on, in ink for a light taskbar and white for
//! a dark one.

use crate::reg;
use windows::Win32::UI::WindowsAndMessaging::{CreateIconFromResourceEx, HICON, LR_DEFAULTCOLOR};

macro_rules! sizes {
    ($state:literal, $theme:literal) => {
        [
            (16, include_bytes!(concat!("../Resources/tray/", $state, "-", $theme, "-16.png")) as &[u8]),
            (20, include_bytes!(concat!("../Resources/tray/", $state, "-", $theme, "-20.png")) as &[u8]),
            (24, include_bytes!(concat!("../Resources/tray/", $state, "-", $theme, "-24.png")) as &[u8]),
            (32, include_bytes!(concat!("../Resources/tray/", $state, "-", $theme, "-32.png")) as &[u8]),
            (40, include_bytes!(concat!("../Resources/tray/", $state, "-", $theme, "-40.png")) as &[u8]),
            (48, include_bytes!(concat!("../Resources/tray/", $state, "-", $theme, "-48.png")) as &[u8]),
        ]
    };
}

type Set = [(i32, &'static [u8]); 6];
const OFF_LIGHT: Set = sizes!("off", "light");
const OFF_DARK: Set = sizes!("off", "dark");
const ON_LIGHT: Set = sizes!("on", "light");
const ON_DARK: Set = sizes!("on", "dark");

/// Whether the taskbar is light (Windows' "SystemUsesLightTheme"; dark when unset).
fn light_taskbar() -> bool {
    reg::get_dword(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "SystemUsesLightTheme") == Some(1)
}

/// The icon for a state at the size the taskbar wants (px): the smallest drawn size that is at
/// least that big, so Windows only ever scales down.
pub fn tray(on: bool, px: i32) -> Option<HICON> {
    let set = match (on, light_taskbar()) {
        (false, true) => &OFF_LIGHT,
        (false, false) => &OFF_DARK,
        (true, true) => &ON_LIGHT,
        (true, false) => &ON_DARK,
    };
    let want = px.max(16);
    let (_, png) = set.iter().find(|(s, _)| *s >= want).unwrap_or(&set[set.len() - 1]);
    // 0x00030000 is the icon format version every Windows since 3.0 expects; PNG data is fine.
    unsafe { CreateIconFromResourceEx(png, true, 0x0003_0000, want, want, LR_DEFAULTCOLOR) }.ok()
}
