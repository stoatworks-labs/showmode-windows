//! The two guards that are a SystemParametersInfo switch: the screen saver, and Snap (windows
//! snapping to the screen edges, Snap layouts and drag-to-top-to-maximise, all one switch).
//! Each is written the way Settings writes it, saved and broadcast, after the old value is
//! journalled; restore writes the old value back only if show mode's value is still there.

use crate::{journal, showlog};
use serde_json::json;
use windows::Win32::Foundation::BOOL;
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETSCREENSAVEACTIVE, SPI_GETWINARRANGING,
    SPI_SETSCREENSAVEACTIVE, SPI_SETWINARRANGING, SYSTEM_PARAMETERS_INFO_ACTION,
};

struct Switch {
    key: &'static str,
    label: &'static str,
    get: SYSTEM_PARAMETERS_INFO_ACTION,
    set: SYSTEM_PARAMETERS_INFO_ACTION,
    /// SPI_SETSCREENSAVEACTIVE takes the value in uiParam, SPI_SETWINARRANGING in pvParam.
    in_pv: bool,
}

const SCREEN_SAVER: Switch = Switch {
    key: "screenSaver",
    label: "screen saver",
    get: SPI_GETSCREENSAVEACTIVE,
    set: SPI_SETSCREENSAVEACTIVE,
    in_pv: false,
};

const SNAP: Switch = Switch { key: "snap", label: "Snap", get: SPI_GETWINARRANGING, set: SPI_SETWINARRANGING, in_pv: true };

fn read(s: &Switch) -> Option<bool> {
    let mut v = BOOL(0);
    unsafe { SystemParametersInfoW(s.get, 0, Some(&mut v as *mut BOOL as _), Default::default()) }.ok()?;
    Some(v.as_bool())
}

fn write(s: &Switch, on: bool) -> bool {
    let flags = SPIF_UPDATEINIFILE | SPIF_SENDCHANGE;
    unsafe {
        if s.in_pv {
            SystemParametersInfoW(s.set, 0, Some(on as usize as _), flags)
        } else {
            SystemParametersInfoW(s.set, on as u32, None, flags)
        }
    }
    .is_ok()
}

/// Switches it off if it is on. Returns a warning if Windows didn't take the change.
fn engage(s: &Switch) -> Option<String> {
    let Some(now) = read(s) else {
        return Some(format!("The {} setting could not be read, so it was left alone", s.label));
    };
    if !now {
        return None; // already off: nothing to undo later
    }
    journal::record(s.key, json!(true));
    if !write(s, false) || read(s) != Some(false) {
        return Some(format!("The {} is still on: Windows did not take the change", s.label));
    }
    showlog::note(format!("{}: switched off", s.label));
    None
}

/// Puts it back on if show mode switched it off. A setting someone switched back on by hand
/// mid-show is left as it is.
fn restore(s: &Switch) -> bool {
    if journal::get(s.key).and_then(|v| v.as_bool()) != Some(true) {
        journal::remove(s.key);
        return true;
    }
    if read(s) == Some(false) && !write(s, true) {
        return false;
    }
    showlog::note(format!("{}: back on", s.label));
    journal::remove(s.key);
    true
}

pub fn engage_screen_saver() -> Option<String> {
    engage(&SCREEN_SAVER)
}

pub fn restore_screen_saver() -> bool {
    restore(&SCREEN_SAVER)
}

pub fn engage_snap() -> Option<String> {
    engage(&SNAP)
}

pub fn restore_snap() -> bool {
    restore(&SNAP)
}

pub fn describe() -> String {
    let s = |v: Option<bool>| match v {
        Some(true) => "on",
        Some(false) => "off",
        None => "unreadable",
    };
    format!("screen saver {}, Snap {}", s(read(&SCREEN_SAVER)), s(read(&SNAP)))
}
