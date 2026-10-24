//! Two keyboard guards.
//!
//! **The Windows key.** A low-level keyboard hook, on a thread of its own (hookthread.rs),
//! swallows what chords.rs lists: the Windows keys and everything pressed with one, Ctrl+Esc and
//! Print Screen. So Start, Task View, Win+D, virtual desktops, Win+P, the Magnifier, Game Bar
//! and the Copilot key all do nothing over the show. Nothing is written anywhere: the hook goes
//! with the process, so a crash leaves nothing to put back. Win+L and Ctrl+Alt+Del are handled
//! below any hook and still work. The spike swallowed every Windows shortcut it tried on winlab
//! (25H2), Win+G included. A medium process's hook isn't called for input to an elevated
//! window, so the keys work again while one has the focus.
//!
//! **Accessibility shortcuts.** Shift five times (Sticky Keys), right Shift held for eight seconds
//! (Filter Keys), Num Lock held for five (Toggle Keys) and left Alt + left Shift + Num Lock (Mouse
//! Keys) each put a box over the show asking whether to turn the feature on. Only the shortcut is
//! switched off, the way Settings does it, never the feature itself: someone who uses Sticky Keys
//! keeps them. High Contrast's shortcut is left alone: writing its settings, even unchanged,
//! re-applies the Windows theme, which took up to six seconds on winlab and put the theme's sound
//! scheme back over the system sounds guard. Its chord ends in Print Screen, which the Windows
//! key guard swallows.

use crate::chords::{Chords, Mods};
use crate::hookthread::HookThread;
use crate::{journal, showlog, util};
use serde_json::{json, Value};
use std::sync::Mutex;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Accessibility::{FILTERKEYS, MOUSEKEYS, STICKYKEYS, TOGGLEKEYS};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::*;

// MARK: the Windows key

static CHORDS: Mutex<Option<Chords>> = Mutex::new(None);

unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let up = k.flags.0 & LLKHF_UP.0 != 0;
        let held = |vk| GetAsyncKeyState(vk as i32) < 0;
        let mods = Mods { ctrl: held(VK_CONTROL.0), shift: held(VK_SHIFT.0), alt: held(VK_MENU.0) };
        let eat = CHORDS.lock().ok().and_then(|mut c| c.as_mut().map(|c| c.swallow(k.vkCode, up, mods))).unwrap_or(false);
        if eat {
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

/// The Windows key guard while a show runs. Dropping it lets the keys through again.
pub struct WindowsKey(#[allow(dead_code)] HookThread);

impl WindowsKey {
    pub fn start() -> Result<WindowsKey, String> {
        *CHORDS.lock().unwrap() = Some(Chords::default());
        match HookThread::start("windows-key", WH_KEYBOARD_LL, Some(hook), None) {
            Some(t) => {
                showlog::note("keys: Windows key, Ctrl+Esc and Print Screen blocked");
                Ok(WindowsKey(t))
            }
            None => Err("The Windows key still works: Windows wouldn't set the keyboard hook".into()),
        }
    }
}

impl Drop for WindowsKey {
    fn drop(&mut self) {
        showlog::note("keys: Windows key let through again");
    }
}

// MARK: accessibility shortcuts

/// The HOTKEYACTIVE flag is 4 in each of these structures.
const HOTKEY_ACTIVE: u32 = 4;

struct Shortcut {
    key: &'static str,
    label: &'static str,
    get: SYSTEM_PARAMETERS_INFO_ACTION,
    set: SYSTEM_PARAMETERS_INFO_ACTION,
    size: usize,
}

const SHORTCUTS: [Shortcut; 4] = [
    Shortcut {
        key: "stickyKeys",
        label: "Sticky Keys",
        get: SPI_GETSTICKYKEYS,
        set: SPI_SETSTICKYKEYS,
        size: std::mem::size_of::<STICKYKEYS>(),
    },
    Shortcut {
        key: "filterKeys",
        label: "Filter Keys",
        get: SPI_GETFILTERKEYS,
        set: SPI_SETFILTERKEYS,
        size: std::mem::size_of::<FILTERKEYS>(),
    },
    Shortcut {
        key: "toggleKeys",
        label: "Toggle Keys",
        get: SPI_GETTOGGLEKEYS,
        set: SPI_SETTOGGLEKEYS,
        size: std::mem::size_of::<TOGGLEKEYS>(),
    },
    Shortcut {
        key: "mouseKeys",
        label: "Mouse Keys",
        get: SPI_GETMOUSEKEYS,
        set: SPI_SETMOUSEKEYS,
        size: std::mem::size_of::<MOUSEKEYS>(),
    },
];

/// Each structure starts with its size and then its flags; the rest is handed back to Windows as
/// Windows gave it.
#[derive(Clone, Copy)]
struct Buffer([u64; 4]);

impl Buffer {
    fn flags(&self) -> u32 {
        (self.0[0] >> 32) as u32
    }

    fn with_flags(mut self, flags: u32) -> Buffer {
        self.0[0] = (self.0[0] & 0xffff_ffff) | ((flags as u64) << 32);
        self
    }
}

fn read(s: &Shortcut) -> Option<Buffer> {
    let mut b = Buffer([s.size as u64, 0, 0, 0]);
    unsafe { SystemParametersInfoW(s.get, s.size as u32, Some(b.0.as_mut_ptr() as _), Default::default()) }.ok()?;
    Some(b)
}

fn write(s: &Shortcut, b: Buffer) -> bool {
    let mut b = b;
    unsafe { SystemParametersInfoW(s.set, s.size as u32, Some(b.0.as_mut_ptr() as _), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) }.is_ok()
}

fn hotkey_on(s: &Shortcut) -> Option<bool> {
    read(s).map(|b| b.flags() & HOTKEY_ACTIVE != 0)
}

pub fn engage_accessibility() -> Option<String> {
    let mut failed = Vec::new();
    let mut off = Vec::new();
    for s in &SHORTCUTS {
        let Some(b) = read(s) else {
            failed.push(s.label.to_string());
            continue;
        };
        if b.flags() & HOTKEY_ACTIVE == 0 {
            continue; // already off: nothing to undo later
        }
        // One entry for all four, each shortcut added to it before it's switched off.
        let mut entry = journal::get("accessibilityKeys").unwrap_or_else(|| json!({}));
        entry[s.key] = Value::Bool(true);
        journal::set("accessibilityKeys", entry);
        if write(s, b.with_flags(b.flags() & !HOTKEY_ACTIVE)) && hotkey_on(s) == Some(false) {
            off.push(s.label.to_string());
        } else {
            failed.push(s.label.to_string());
        }
    }
    if !off.is_empty() {
        showlog::note(format!("accessibility shortcuts off: {}", util::list(&off)));
    }
    (!failed.is_empty()).then(|| format!("The {} shortcut is still on: Windows did not take the change", util::list(&failed)))
}

pub fn restore_accessibility() -> bool {
    let Some(entry) = journal::get("accessibilityKeys") else { return true };
    let mut left = serde_json::Map::new();
    for s in SHORTCUTS.iter().filter(|s| entry[s.key].as_bool() == Some(true)) {
        // Only where it's still off: a shortcut switched back on by hand mid-show stands.
        let ok = match read(s) {
            Some(b) if b.flags() & HOTKEY_ACTIVE == 0 => write(s, b.with_flags(b.flags() | HOTKEY_ACTIVE)) && hotkey_on(s) == Some(true),
            Some(_) => true,
            None => false,
        };
        if !ok {
            left.insert(s.key.into(), Value::Bool(true));
        }
    }
    if !left.is_empty() {
        showlog::note(format!("accessibility shortcuts: could not switch back on {:?}", left.keys().collect::<Vec<_>>()));
        journal::set("accessibilityKeys", Value::Object(left));
        return false;
    }
    showlog::note("accessibility shortcuts: back on");
    journal::remove("accessibilityKeys");
    true
}

/// For --status.
pub fn describe() -> String {
    let parts: Vec<String> = SHORTCUTS
        .iter()
        .map(|s| {
            format!(
                "{} {}",
                s.label,
                match read(s) {
                    Some(b) => format!("flags {:#x} (shortcut {})", b.flags(), if b.flags() & HOTKEY_ACTIVE != 0 { "on" } else { "off" }),
                    None => "unreadable".into(),
                }
            )
        })
        .collect();
    format!("accessibility shortcuts: {}", parts.join(", "))
}
