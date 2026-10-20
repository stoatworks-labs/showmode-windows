//! Do Not Disturb, through the shell's own QuietHoursSettings COM server: the interface
//! Settings and the notification-centre bell use. Windows documents no way for an app to set
//! it (FocusSessionManager is a Limited Access Feature, unlocked per package by Microsoft), so
//! this is undocumented: if the class or interface ever goes, CoCreateInstance fails and the
//! guard reports itself unavailable rather than breaking anything. Reading the state back goes
//! through the documented ToastNotificationManagerForUser.NotificationMode.
//!
//! Checked in the spike on 25H2 as a standard user: the clock's DND badge, the Settings toggle
//! and toast delivery all agreed, and putting the original profile back restored all three.

#![allow(non_snake_case)]

use crate::{journal, showlog, util};
use serde_json::json;
use std::ffi::c_void;
use std::time::Duration;
use windows::core::{GUID, HRESULT, PCWSTR, PWSTR};
use windows::UI::Notifications::ToastNotificationManager;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_LOCAL_SERVER};

#[windows_core::interface("6bff4732-81ec-4ffb-ae67-b6c1bc29631f")]
unsafe trait IQuietHoursSettings: windows_core::IUnknown {
    unsafe fn get_UserSelectedProfile(&self, id: *mut PWSTR) -> HRESULT;
    unsafe fn put_UserSelectedProfile(&self, id: PCWSTR) -> HRESULT;
    unsafe fn GetProfile(&self, id: PCWSTR, out: *mut *mut c_void) -> HRESULT;
    unsafe fn GetAllProfileData(&self, count: *mut u32, data: *mut c_void) -> HRESULT;
    unsafe fn GetDisplayNameForProfile(&self, id: PCWSTR, out: *mut PWSTR) -> HRESULT;
    unsafe fn get_QuietMomentsManager(&self, out: *mut *mut c_void) -> HRESULT;
    unsafe fn get_OffProfileId(&self, out: *mut PWSTR) -> HRESULT;
    unsafe fn get_ActiveQuietMomentProfile(&self, out: *mut PWSTR) -> HRESULT;
    unsafe fn put_ActiveQuietMomentProfile(&self, id: PCWSTR) -> HRESULT;
    unsafe fn get_ActiveProfile(&self, out: *mut PWSTR) -> HRESULT;
}

const CLSID_QUIET_HOURS_SETTINGS: GUID = GUID::from_u128(0xf53321fa_34f8_4b7f_b9a3_361877cb94cf);
const PRIORITY_ONLY: &str = "Microsoft.QuietHoursProfile.PriorityOnly";
const UNRESTRICTED: &str = "Microsoft.QuietHoursProfile.Unrestricted";

/// The waits between reads when a write doesn't read back at once: about a second in all. On
/// winlab (25H2, 2026-10-20) a read straight after a write that took could still see the old
/// profile, most often while Windows' Duplicate rule had its own Do Not Disturb up; the new one
/// showed within 50 ms.
const READ_BACK_WAITS_MS: [u64; 6] = [50, 100, 150, 200, 250, 250];

fn settings() -> Option<IQuietHoursSettings> {
    unsafe { CoCreateInstance(&CLSID_QUIET_HOURS_SETTINGS, None, CLSCTX_LOCAL_SERVER) }.ok()
}

fn selected(s: &IQuietHoursSettings) -> Option<String> {
    unsafe {
        let mut p = PWSTR::null();
        s.get_UserSelectedProfile(&mut p).ok().ok()?;
        util::take_co_string(p)
    }
}

fn select(s: &IQuietHoursSettings, profile: &str) -> windows::core::Result<()> {
    util::with_wide(profile, |p| unsafe { s.put_UserSelectedProfile(p) }).ok()
}

/// Reads the selected profile until it is `want`, for about a second. Only reads: what to do
/// about the answer is the caller's. Returns the last read and how long it waited for it.
fn read_back(s: &IQuietHoursSettings, want: &str) -> (Option<String>, u64) {
    let mut now = selected(s);
    let mut waited = 0;
    for ms in READ_BACK_WAITS_MS {
        if now.as_deref() == Some(want) {
            break;
        }
        std::thread::sleep(Duration::from_millis(ms));
        waited += ms;
        now = selected(s);
    }
    (now, waited)
}

/// " after 300 ms" when a read-back had to wait, for the log.
fn after(waited: u64) -> String {
    if waited == 0 {
        String::new()
    } else {
        format!(" after {waited} ms")
    }
}

/// The documented read: 0 unrestricted, 1 priority only, 2 alarms only.
fn notification_mode() -> Option<i32> {
    ToastNotificationManager::GetDefault().and_then(|m| m.NotificationMode()).ok().map(|m| m.0)
}

pub fn engage() -> Option<String> {
    let Some(s) = settings() else {
        return Some("Do Not Disturb unavailable: this Windows has no QuietHoursSettings".into());
    };
    let Some(original) = selected(&s) else {
        return Some("Do Not Disturb left alone: its current setting could not be read".into());
    };
    if original != UNRESTRICTED {
        // Already on, as priority only or the stricter alarms only: leave it be.
        return None;
    }
    journal::record("dnd", json!(original));
    let (now, waited) = match select(&s, PRIORITY_ONLY) {
        Ok(()) => read_back(&s, PRIORITY_ONLY),
        Err(_) => (None, 0),
    };
    if now.as_deref() != Some(PRIORITY_ONLY) {
        return Some("Do Not Disturb is still off: Windows did not take the change".into());
    }
    showlog::note(format!(
        "dnd: on (was {original}){}, NotificationMode now {:?}",
        after(waited),
        notification_mode()
    ));
    None
}

/// Puts back the profile the user had. If they changed it themselves mid-show, theirs stands.
/// Until the profile reads back as something other than show mode's, the entry stays in the
/// journal for another try: a profile that can't be read doesn't count as put back.
pub fn restore() -> bool {
    let Some(original) = journal::get("dnd").and_then(|v| v.as_str().map(String::from)) else {
        return true;
    };
    let Some(s) = settings() else { return false };
    match selected(&s) {
        None => {
            showlog::note("dnd: could not be put back: its setting could not be read");
            return false;
        }
        Some(now) if now == original => showlog::note(format!("dnd: already back to {original}")),
        Some(now) if now != PRIORITY_ONLY => showlog::note(format!("dnd: left at {now}, changed mid-show")),
        Some(_) => {
            if let Err(e) = select(&s, &original) {
                showlog::note(format!("dnd: could not be put back: Windows refused the change ({e})"));
                return false;
            }
            let (now, waited) = read_back(&s, &original);
            if now.as_deref() != Some(original.as_str()) {
                showlog::note(format!(
                    "dnd: could not be put back: it reads {}{}",
                    now.as_deref().unwrap_or("nothing"),
                    after(waited)
                ));
                return false;
            }
            showlog::note(format!("dnd: put back to {original}{}", after(waited)));
        }
    }
    journal::remove("dnd");
    true
}

pub fn describe() -> String {
    match settings() {
        None => "Do Not Disturb unavailable (no QuietHoursSettings)".into(),
        Some(s) => format!(
            "Do Not Disturb profile {}, NotificationMode {}",
            selected(&s).unwrap_or_else(|| "unreadable".into()),
            notification_mode().map(|m| m.to_string()).unwrap_or_else(|| "unreadable".into())
        ),
    }
}
