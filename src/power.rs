//! Power: a keep-awake request, and a temporary "show plan".
//!
//! The request holds the display and the system on, the way a video player does, and goes
//! away with the process, so a crash cannot leave a PC that never sleeps. Windows ends it
//! itself on a lid close, the power button, Start > Sleep and the lock screen, which is why the
//! show plan exists: a copy of the user's active power plan with the lid set to do nothing and
//! no display, sleep or hibernate timeouts. The user's own plan is never edited. The journal
//! holds the original plan's GUID and the copy's, and restoring re-activates the original and
//! deletes the copy. A standard user can do all of this (checked on 25H2 in the spike).

use crate::{journal, showlog, util};
use serde_json::json;
use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL, WIN32_ERROR};
use windows::Win32::System::Power::*;
use windows::Win32::System::Registry::HKEY;
use windows::Win32::System::Threading::{
    POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
};

const SUB_BUTTONS: GUID = GUID::from_u128(0x4f971e89_eebd_4455_a8de_9e59040e7347);
const LIDACTION: GUID = GUID::from_u128(0x5ca83367_6e45_459f_a27b_476b1d01c936);
const SUB_VIDEO: GUID = GUID::from_u128(0x7516b95f_f776_4464_8c53_06167f40cc99);
const VIDEOIDLE: GUID = GUID::from_u128(0x3c0bc021_c8a8_4e07_a973_6b14cbcb2b7e);
const SUB_SLEEP: GUID = GUID::from_u128(0x238c9fa8_0aad_41ed_83f4_97be242c8f20);
const STANDBYIDLE: GUID = GUID::from_u128(0x29f6c1db_86da_48c5_9fdb_f2b67b1f44da);
const HIBERNATEIDLE: GUID = GUID::from_u128(0x9d7815a6_7ee4_497e_8888_515a05f02364);
/// Balanced, the plan every Windows has, for when the original has gone by restore time.
const BALANCED: GUID = GUID::from_u128(0x381b4222_f694_41f0_9685_ff5bb260df2e);

/// Every value written on the copy is 0: do nothing, or never.
const SHOW_PLAN: &[(&str, GUID, GUID)] = &[
    ("lid close action", SUB_BUTTONS, LIDACTION),
    ("display off", SUB_VIDEO, VIDEOIDLE),
    ("sleep", SUB_SLEEP, STANDBYIDLE),
    ("hibernate", SUB_SLEEP, HIBERNATEIDLE),
];

const PLAN_NAME: &str = "Show Mode (during a show)";

/// The keep-awake request. Dropping it releases it.
pub struct KeepAwake(HANDLE);

impl KeepAwake {
    pub fn take() -> Option<KeepAwake> {
        let mut reason = util::wide("A show is running (Show Mode)");
        let ctx = REASON_CONTEXT {
            Version: 0, // POWER_REQUEST_CONTEXT_VERSION
            Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
            Reason: REASON_CONTEXT_0 { SimpleReasonString: PWSTR(reason.as_mut_ptr()) },
        };
        unsafe {
            let h = PowerCreateRequest(&ctx).ok()?;
            // Microsoft: a SystemRequired request "must be taken in addition to" DisplayRequired.
            let ok = PowerSetRequest(h, PowerRequestDisplayRequired).is_ok()
                && PowerSetRequest(h, PowerRequestSystemRequired).is_ok();
            if !ok {
                let _ = CloseHandle(h);
                return None;
            }
            showlog::note("power: keep-awake request taken");
            Some(KeepAwake(h))
        }
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        unsafe {
            let _ = PowerClearRequest(self.0, PowerRequestDisplayRequired);
            let _ = PowerClearRequest(self.0, PowerRequestSystemRequired);
            let _ = CloseHandle(self.0);
        }
        showlog::note("power: keep-awake request released");
    }
}

fn ok(e: WIN32_ERROR) -> bool {
    e.0 == 0
}

fn active_scheme() -> Option<GUID> {
    unsafe {
        let mut p: *mut GUID = std::ptr::null_mut();
        if !ok(PowerGetActiveScheme(HKEY::default(), &mut p)) || p.is_null() {
            return None;
        }
        let g = *p;
        let _ = LocalFree(HLOCAL(p as _));
        Some(g)
    }
}

fn scheme_exists(g: &GUID) -> bool {
    let mut size = 0u32;
    // Asking for the friendly name's size fails for a scheme that isn't there.
    ok(unsafe { PowerReadFriendlyName(HKEY::default(), Some(g), None, None, None, &mut size) })
}

/// Makes and activates the show plan. Returns a warning if it could not.
pub fn engage_show_plan() -> Option<String> {
    if journal::contains("showPlan") {
        // A plan from a show that never finished restoring is still in place.
        return None;
    }
    let Some(original) = active_scheme() else {
        return Some("Show power plan skipped: the active plan could not be read".into());
    };
    // Journal the original before anything is created, then the copy as soon as it exists.
    journal::set("showPlan", json!({ "original": util::guid_string(&original) }));
    let copy = unsafe {
        let mut p: *mut GUID = std::ptr::null_mut();
        if !ok(PowerDuplicateScheme(HKEY::default(), &original, &mut p)) || p.is_null() {
            journal::remove("showPlan");
            return Some("Show power plan skipped: Windows would not copy the active plan".into());
        }
        let g = *p;
        let _ = LocalFree(HLOCAL(p as _));
        g
    };
    journal::set(
        "showPlan",
        json!({ "original": util::guid_string(&original), "copy": util::guid_string(&copy) }),
    );

    let name = util::wide(PLAN_NAME);
    let bytes = unsafe { std::slice::from_raw_parts(name.as_ptr() as *const u8, name.len() * 2) };
    unsafe {
        let _ = PowerWriteFriendlyName(HKEY::default(), &copy, None, None, bytes);
    }
    let mut failed = Vec::new();
    for (label, sub, setting) in SHOW_PLAN {
        let (ac, dc) = unsafe {
            (
                PowerWriteACValueIndex(HKEY::default(), &copy, Some(sub), Some(setting), 0),
                PowerWriteDCValueIndex(HKEY::default(), &copy, Some(sub), Some(setting), 0),
            )
        };
        if !ok(ac) || dc != 0 {
            failed.push(label.to_string());
        }
    }
    if !ok(unsafe { PowerSetActiveScheme(HKEY::default(), Some(&copy)) }) || active_scheme() != Some(copy) {
        restore_show_plan();
        return Some("Show power plan not applied: Windows would not switch to it".into());
    }
    showlog::note(format!(
        "power: show plan {} active, copied from {}",
        util::guid_string(&copy),
        util::guid_string(&original)
    ));
    if failed.is_empty() {
        None
    } else {
        showlog::note(format!("power: show plan could not set {failed:?}"));
        Some(format!("Show power plan is on, but its {} could not be changed", util::list(&failed)))
    }
}

/// Puts the original plan back and deletes the copy. Returns false only if the copy is still
/// the active plan, so the journal keeps it for another try.
pub fn restore_show_plan() -> bool {
    let Some(entry) = journal::get("showPlan") else { return true };
    let original = entry["original"].as_str().and_then(util::parse_guid);
    let copy = entry["copy"].as_str().and_then(util::parse_guid);
    let Some(copy) = copy else {
        // The copy was never made.
        journal::remove("showPlan");
        return true;
    };
    if active_scheme() == Some(copy) {
        let back = original.filter(scheme_exists).unwrap_or(BALANCED);
        if !ok(unsafe { PowerSetActiveScheme(HKEY::default(), Some(&back)) }) {
            showlog::note("power: could not re-activate the original plan");
            return false;
        }
    }
    // If the operator chose another plan mid-show, that choice stands; only the copy goes.
    if scheme_exists(&copy) {
        let _ = unsafe { PowerDeleteScheme(HKEY::default(), &copy) };
    }
    showlog::note(format!(
        "power: show plan removed, active plan {}",
        active_scheme().map(|g| util::guid_string(&g)).unwrap_or_default()
    ));
    journal::remove("showPlan");
    true
}

/// For --status.
pub fn describe() -> String {
    match active_scheme() {
        Some(g) => {
            let shown = journal::get("showPlan")
                .and_then(|e| e["copy"].as_str().and_then(util::parse_guid))
                .is_some_and(|c| c == g);
            format!("active plan {}{}", util::guid_string(&g), if shown { " (the show plan)" } else { "" })
        }
        None => "active plan unreadable".into(),
    }
}
