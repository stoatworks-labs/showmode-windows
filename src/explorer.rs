//! The guards that are one per-user number Explorer reads: the taskbar's Show Desktop corner (a
//! stray click there sweeps every window aside, the show included), window shake (shaking a
//! title bar minimises everything else) and AutoPlay (the box that comes up when a drive or a
//! phone is plugged in). Each is the value Settings writes for its switch. The value it replaces,
//! or that there was none, is journalled first; then Explorer is told, the way Settings tells it.
//! Restore puts the old value back only where show mode's value is still there.
//!
//! The corner went and came back within two seconds of the broadcast on winlab (25H2), read
//! through UI Automation.

use crate::{journal, reg, showlog};
use serde_json::{json, Value};
use windows::core::w;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};

const ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const AUTOPLAY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers";

struct Switch {
    key: &'static str,
    label: &'static str,
    reg_key: &'static str,
    name: &'static str,
    /// Show mode's value.
    show: u32,
}

const SHOW_DESKTOP_CORNER: Switch =
    Switch { key: "showDesktopCorner", label: "the Show Desktop corner", reg_key: ADVANCED, name: "TaskbarSd", show: 0 };
const WINDOW_SHAKE: Switch =
    Switch { key: "windowShake", label: "window shake", reg_key: ADVANCED, name: "DisallowShaking", show: 1 };
const AUTOPLAY_SWITCH: Switch =
    Switch { key: "autoPlay", label: "AutoPlay", reg_key: AUTOPLAY, name: "DisableAutoplay", show: 1 };

/// What Settings sends after changing a taskbar setting.
fn tell_explorer() {
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("TraySettings").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            2000,
            None,
        );
    }
}

/// Writes show mode's value if it isn't there already. Returns false if it couldn't.
fn engage(s: &Switch) -> bool {
    let before = reg::get_dword(s.reg_key, s.name);
    if before == Some(s.show) {
        return true; // already as a show wants it: nothing to undo later
    }
    // Journalled as the number it was, or null for "not set".
    journal::record(s.key, json!({ "previous": before }));
    if !reg::set_dword(s.reg_key, s.name, s.show) || reg::get_dword(s.reg_key, s.name) != Some(s.show) {
        showlog::note(format!("{}: {}={} could not be written", s.label, s.name, s.show));
        return false;
    }
    showlog::note(format!("{}: {}={} (was {before:?})", s.label, s.name, s.show));
    true
}

fn restore(s: &Switch) -> bool {
    let Some(entry) = journal::get(s.key) else { return true };
    let previous = match &entry["previous"] {
        Value::Number(n) => n.as_u64().map(|v| v as u32),
        _ => None,
    };
    // A value someone changed by hand mid-show stands.
    if reg::get_dword(s.reg_key, s.name) == Some(s.show) {
        if !reg::restore_dword(s.reg_key, s.name, previous) || reg::get_dword(s.reg_key, s.name) != previous {
            showlog::note(format!("{}: could not put {} back to {previous:?}", s.label, s.name));
            return false;
        }
        showlog::note(format!("{}: {} put back to {previous:?}", s.label, s.name));
    }
    journal::remove(s.key);
    true
}

pub fn engage_show_desktop() -> Option<String> {
    let ok = engage(&SHOW_DESKTOP_CORNER) & engage(&WINDOW_SHAKE);
    tell_explorer();
    (!ok).then(|| "The Show Desktop corner or window shake may still work: the setting could not be written".into())
}

pub fn restore_show_desktop() -> bool {
    let ok = restore(&SHOW_DESKTOP_CORNER) & restore(&WINDOW_SHAKE);
    tell_explorer();
    ok
}

pub fn engage_autoplay() -> Option<String> {
    (!engage(&AUTOPLAY_SWITCH)).then(|| "AutoPlay may still open when a drive is plugged in: the setting could not be written".into())
}

pub fn restore_autoplay() -> bool {
    restore(&AUTOPLAY_SWITCH)
}

/// For --status.
pub fn describe() -> String {
    let v = |s: &Switch| format!("{}={:?}", s.name, reg::get_dword(s.reg_key, s.name));
    format!("explorer: {} {} {}", v(&SHOW_DESKTOP_CORNER), v(&WINDOW_SHAKE), v(&AUTOPLAY_SWITCH))
}
