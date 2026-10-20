//! Black wallpaper on every screen, so an extended desktop never shows your wallpaper on the
//! projector, and the original put back afterwards.
//!
//! Everything goes through the documented IDesktopWallpaper. Journalled first: the background
//! colour, the picture position, each connected monitor's picture, and a running slideshow's
//! folders and options. Then the background colour goes black and every monitor gets "no
//! picture", which is the solid-colour background, so a screen plugged in mid-show is black too.
//! Windows Spotlight as the desktop background has no documented way back, so with Spotlight
//! on the wallpaper is left alone and the warning says why.

use crate::{journal, reg, showlog, util};
use serde_json::{json, Value};
use windows::core::PCWSTR;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    DesktopWallpaper, IDesktopWallpaper, SHCreateShellItemArrayFromIDLists, SHParseDisplayName,
    DESKTOP_SLIDESHOW_OPTIONS, DESKTOP_WALLPAPER_POSITION, DSS_SLIDESHOW, SIGDN_FILESYSPATH,
};

const BLACK: COLORREF = COLORREF(0);
const WALLPAPERS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Wallpapers";
/// BackgroundType 3 is Windows Spotlight (community-documented, Windows 11).
const SPOTLIGHT: u32 = 3;

fn api() -> Option<IDesktopWallpaper> {
    unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }.ok()
}

fn monitors(w: &IDesktopWallpaper) -> Vec<String> {
    unsafe {
        let n = w.GetMonitorDevicePathCount().unwrap_or(0);
        (0..n).filter_map(|i| w.GetMonitorDevicePathAt(i).ok().and_then(|p| util::take_co_string(p))).collect()
    }
}

fn picture(w: &IDesktopWallpaper, monitor: &str) -> Option<String> {
    util::with_wide(monitor, |m| unsafe { w.GetWallpaper(m) }).ok().and_then(|p| unsafe { util::take_co_string(p) })
}

fn set_picture(w: &IDesktopWallpaper, monitor: Option<&str>, path: &str) -> bool {
    let p = util::wide(path);
    match monitor {
        Some(m) => util::with_wide(m, |m| unsafe { w.SetWallpaper(m, PCWSTR(p.as_ptr())) }),
        None => unsafe { w.SetWallpaper(PCWSTR::null(), PCWSTR(p.as_ptr())) },
    }
    .is_ok()
}

/// The slideshow's folders and options, if one is running.
fn slideshow(w: &IDesktopWallpaper) -> Option<Value> {
    unsafe {
        if w.GetStatus().ok()?.0 & DSS_SLIDESHOW.0 == 0 {
            return None;
        }
        let items = w.GetSlideshow().ok()?;
        let mut paths = Vec::new();
        for i in 0..items.GetCount().unwrap_or(0) {
            if let Some(p) = items.GetItemAt(i).ok().and_then(|it| it.GetDisplayName(SIGDN_FILESYSPATH).ok()) {
                if let Some(s) = util::take_co_string(p) {
                    paths.push(s);
                }
            }
        }
        let (mut options, mut tick) = (DESKTOP_SLIDESHOW_OPTIONS(0), 0u32);
        w.GetSlideshowOptions(&mut options, &mut tick).ok()?;
        (!paths.is_empty()).then(|| json!({ "paths": paths, "options": options.0, "tick": tick }))
    }
}

fn restart_slideshow(w: &IDesktopWallpaper, show: &Value) -> bool {
    let paths: Vec<String> =
        show["paths"].as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect()).unwrap_or_default();
    let mut pidls: Vec<*mut ITEMIDLIST> = Vec::new();
    for p in &paths {
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        if util::with_wide(p, |w| unsafe { SHParseDisplayName(w, None, &mut pidl, 0, None) }).is_ok() && !pidl.is_null() {
            pidls.push(pidl);
        }
    }
    let ok = unsafe {
        let consts: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| *p as *const _).collect();
        let r = SHCreateShellItemArrayFromIDLists(&consts).and_then(|array| {
            w.SetSlideshow(&array)?;
            w.SetSlideshowOptions(
                DESKTOP_SLIDESHOW_OPTIONS(show["options"].as_i64().unwrap_or(0) as i32),
                show["tick"].as_u64().unwrap_or(1_800_000) as u32,
            )
        });
        for p in pidls {
            CoTaskMemFree(Some(p as _));
        }
        r.is_ok()
    };
    ok
}

fn is_black(w: &IDesktopWallpaper) -> bool {
    unsafe { w.GetBackgroundColor() }.ok() == Some(BLACK)
        && monitors(w).iter().all(|m| picture(w, m).is_some_and(|p| p.is_empty()))
}

pub fn engage() -> Option<String> {
    let Some(w) = api() else {
        return Some("Wallpaper left alone: Windows' wallpaper interface is unavailable".into());
    };
    if !journal::contains("wallpaper") {
        if reg::get_dword(WALLPAPERS_KEY, "BackgroundType") == Some(SPOTLIGHT) {
            return Some("Wallpaper left alone: Windows Spotlight can't be put back exactly afterwards".into());
        }
        if is_black(&w) {
            return None;
        }
        let pictures: serde_json::Map<String, Value> =
            monitors(&w).into_iter().map(|m| (m.clone(), json!(picture(&w, &m).unwrap_or_default()))).collect();
        let entry = json!({
            "background": unsafe { w.GetBackgroundColor() }.map(|c| c.0).unwrap_or(0),
            "position": unsafe { w.GetPosition() }.map(|p| p.0).unwrap_or(4),
            "pictures": pictures,
            "slideshow": slideshow(&w),
        });
        journal::record("wallpaper", entry);
    }
    if !apply_black() {
        return Some("Wallpaper may not be black on every screen: Windows refused the change".into());
    }
    showlog::note("wallpaper: blacked out");
    None
}

/// Black background, no picture on any monitor. Also run every few seconds mid-show, since
/// some remote-access tools put their idea of the wallpaper back when a session ends.
pub fn apply_black() -> bool {
    let Some(w) = api() else { return false };
    if is_black(&w) {
        return true;
    }
    let ok = unsafe { w.SetBackgroundColor(BLACK) }.is_ok() && set_picture(&w, None, "");
    if ok && journal::contains("wallpaper") {
        showlog::note("wallpaper: black put back");
    }
    ok
}

pub fn restore() -> bool {
    let Some(entry) = journal::get("wallpaper") else { return true };
    let Some(w) = api() else { return false };
    unsafe {
        let _ = w.SetBackgroundColor(COLORREF(entry["background"].as_u64().unwrap_or(0) as u32));
    }
    unsafe {
        let _ = w.SetPosition(DESKTOP_WALLPAPER_POSITION(entry["position"].as_i64().unwrap_or(4) as i32));
    }
    let mut ok = true;
    if let Some(show) = entry.get("slideshow").filter(|s| !s.is_null()) {
        ok = restart_slideshow(&w, show);
    } else {
        let pictures = entry["pictures"].as_object().cloned().unwrap_or_default();
        let distinct: std::collections::BTreeSet<&str> = pictures.values().filter_map(|v| v.as_str()).collect();
        if distinct.len() == 1 {
            // One picture everywhere: set it for every monitor, including any plugged in since.
            ok = set_picture(&w, None, distinct.first().unwrap());
        } else {
            let connected = monitors(&w);
            for (m, p) in &pictures {
                if connected.contains(m) {
                    ok &= set_picture(&w, Some(m), p.as_str().unwrap_or(""));
                }
            }
        }
    }
    if !ok {
        showlog::note("wallpaper: could not put everything back");
        return false;
    }
    showlog::note("wallpaper: put back");
    journal::remove("wallpaper");
    true
}

pub fn describe() -> String {
    let Some(w) = api() else { return "wallpaper interface unavailable".into() };
    let ms = monitors(&w);
    let pics: Vec<String> = ms.iter().map(|m| picture(&w, m).unwrap_or_else(|| "?".into())).collect();
    format!(
        "wallpaper: {} monitor(s), background {:06x}, pictures {:?}{}",
        ms.len(),
        unsafe { w.GetBackgroundColor() }.map(|c| c.0).unwrap_or(0),
        pics,
        if unsafe { w.GetStatus() }.is_ok_and(|s| s.0 & DSS_SLIDESHOW.0 != 0) { ", slideshow running" } else { "" }
    )
}

