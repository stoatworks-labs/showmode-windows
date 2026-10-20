//! Muting every app that isn't allowed to play, so Outlook's new-mail chime or a browser tab
//! never reaches the PA. The output's own volume and mute are never touched.
//!
//! Each app's audio session on every output is muted with the documented ISimpleAudioVolume,
//! including sound already playing, and new sessions are muted as they are created (before
//! their first sound, in the spike's recording). The catch is the reverse of the macOS tap:
//! Windows keeps a per-app mute after the muting process dies, and even across the app's next
//! launch. So:
//!
//! - every app is journalled (by executable path) before it is muted;
//! - a watchdog process (ShowMode.exe --watchdog) holds a handle to Show Mode and unmutes
//!   everything journalled the moment Show Mode dies, crash included;
//! - an app muted mid-show that has since quit can't be unmuted until it plays again, so it
//!   goes on a pending list, and Show Mode unmutes it when it next opens a session.
//!
//! Apps that play through ASIO or exclusive-mode WASAPI bypass sessions entirely: they can't
//! be muted, and they're never muted by accident either.

use crate::{journal, paths, settings, showlog, util};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use windows::core::{implement, Interface, Result as WinResult, PCWSTR};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Muting every app not on the allow-list.
    Show,
    /// Unmuting apps on the pending list as they reappear.
    Pending,
}

static MODE: Mutex<Option<Mode>> = Mutex::new(None);
static WORKER: Mutex<Option<Sender<Cmd>>> = Mutex::new(None);

enum Cmd {
    /// An output was added or changed state: attach to any new ones.
    Devices,
    /// The allow-list changed: apply it to every session.
    Sweep,
    /// Detach everything and end the thread.
    Stop(Sender<()>),
}

fn journalled() -> BTreeSet<String> {
    journal::get("audioMuted")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect()
}

fn journal_add(path: &str) {
    let mut set = journalled();
    if set.insert(path.to_string()) {
        journal::set("audioMuted", json!(set));
    }
}

// MARK: the pending list

fn pending_path() -> std::path::PathBuf {
    paths::data_dir().join("pending-unmute.json")
}

fn pending() -> BTreeSet<String> {
    let Some(v) = std::fs::read(pending_path()).ok().and_then(|d| serde_json::from_slice::<Value>(&d).ok()) else {
        return BTreeSet::new();
    };
    // Forgotten after 30 days: an app not opened in that time can be unmuted by hand.
    let since = v["since"].as_u64().unwrap_or(0);
    if now_secs().saturating_sub(since) > 30 * 86_400 {
        let _ = std::fs::remove_file(pending_path());
        return BTreeSet::new();
    }
    v["paths"].as_array().cloned().unwrap_or_default().into_iter().filter_map(|p| p.as_str().map(String::from)).collect()
}

fn save_pending(set: &BTreeSet<String>) {
    if set.is_empty() {
        let _ = std::fs::remove_file(pending_path());
    } else {
        let _ = std::fs::write(pending_path(), serde_json::to_vec(&json!({ "paths": set, "since": now_secs() })).unwrap());
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// MARK: one session

/// The full path of the session's process, or None for the system sounds session, a session
/// spanning several processes, or Show Mode itself.
fn session_process(ctl: &IAudioSessionControl) -> Option<String> {
    unsafe {
        let ctl2: IAudioSessionControl2 = ctl.cast().ok()?;
        if ctl2.IsSystemSoundsSession() == windows::Win32::Foundation::S_OK {
            return None;
        }
        let pid = ctl2.GetProcessId().ok()?;
        if pid == 0 || pid == std::process::id() {
            return None;
        }
        util::exe_path_of_pid(pid)
    }
}

/// The same, lowercased: how apps are matched and journalled.
fn session_exe(ctl: &IAudioSessionControl) -> Option<String> {
    session_process(ctl).map(|p| p.to_ascii_lowercase())
}

/// Applies the current mode to one session. Returns the exe it unmuted, if any.
fn apply(ctl: &IAudioSessionControl) -> Option<String> {
    let mode = (*MODE.lock().unwrap())?;
    let exe = session_exe(ctl)?;
    let vol: ISimpleAudioVolume = ctl.cast().ok()?;
    let muted = unsafe { vol.GetMute() }.ok()?.as_bool();
    match mode {
        Mode::Show => {
            let allowed = settings::is_allowed_audio_app(util::file_name(&exe));
            if allowed {
                if muted && journalled().contains(&exe) {
                    unsafe { vol.SetMute(false, std::ptr::null()) }.ok()?;
                    showlog::note(format!("audio: unmuted {exe} (now allowed)"));
                    return Some(exe);
                }
            } else if !muted {
                // Journal first: if this is the last thing Show Mode does, the watchdog and the
                // next launch still know to unmute it.
                journal_add(&exe);
                if unsafe { vol.SetMute(true, std::ptr::null()) }.is_ok() {
                    showlog::note(format!("audio: muted {exe}"));
                }
            }
            None
        }
        Mode::Pending => {
            let mut list = pending();
            if muted && list.remove(&exe) {
                unsafe { vol.SetMute(false, std::ptr::null()) }.ok()?;
                save_pending(&list);
                showlog::note(format!("audio: unmuted {exe}, muted in an earlier show"));
                return Some(exe);
            }
            None
        }
    }
}

fn sessions(mgr: &IAudioSessionManager2) -> Vec<IAudioSessionControl> {
    unsafe {
        let Ok(en) = mgr.GetSessionEnumerator() else { return Vec::new() };
        // GetCount is also what starts session notifications on this manager.
        let n = en.GetCount().unwrap_or(0);
        (0..n).filter_map(|i| en.GetSession(i).ok()).collect()
    }
}

fn outputs(en: &IMMDeviceEnumerator) -> Vec<(String, IMMDevice)> {
    unsafe {
        let Ok(c) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return Vec::new() };
        (0..c.GetCount().unwrap_or(0))
            .filter_map(|i| c.Item(i).ok())
            .filter_map(|d| d.GetId().ok().and_then(|id| util::take_co_string(id)).map(|id| (id, d)))
            .collect()
    }
}

// MARK: callbacks

#[implement(IAudioSessionNotification)]
struct SessionWatch;

impl IAudioSessionNotification_Impl for SessionWatch_Impl {
    fn OnSessionCreated(&self, new: Option<&IAudioSessionControl>) -> WinResult<()> {
        if let Some(ctl) = new {
            apply(ctl);
        }
        Ok(())
    }
}

#[implement(IMMNotificationClient)]
struct DeviceWatch(Mutex<Sender<Cmd>>);

impl DeviceWatch_Impl {
    fn poke(&self) {
        let _ = self.0.lock().unwrap().send(Cmd::Devices);
    }
}

impl IMMNotificationClient_Impl for DeviceWatch_Impl {
    fn OnDeviceStateChanged(&self, _id: &PCWSTR, _state: DEVICE_STATE) -> WinResult<()> {
        self.poke();
        Ok(())
    }
    fn OnDeviceAdded(&self, _id: &PCWSTR) -> WinResult<()> {
        self.poke();
        Ok(())
    }
    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> WinResult<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, _flow: EDataFlow, _role: ERole, _id: &PCWSTR) -> WinResult<()> {
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> WinResult<()> {
        Ok(())
    }
}

// MARK: the worker thread

/// Owns every COM object, since they can't cross threads in Rust: the device enumerator, one
/// session manager and notification per output, and the device watch.
fn worker(rx: std::sync::mpsc::Receiver<Cmd>, tx: Sender<Cmd>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
            showlog::note("audio: no device enumerator");
            return;
        };
        let devices: IMMNotificationClient = DeviceWatch(Mutex::new(tx)).into();
        let _ = en.RegisterEndpointNotificationCallback(&devices);
        let mut attached: Vec<(String, IAudioSessionManager2, IAudioSessionNotification)> = Vec::new();

        let attach = |attached: &mut Vec<(String, IAudioSessionManager2, IAudioSessionNotification)>| {
            for (id, dev) in outputs(&en) {
                if attached.iter().any(|(a, _, _)| *a == id) {
                    continue;
                }
                let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
                let watch: IAudioSessionNotification = SessionWatch.into();
                // Register first, then enumerate: GetCount starts the notifications, and any
                // session created in between is caught by one or the other.
                if mgr.RegisterSessionNotification(&watch).is_err() {
                    continue;
                }
                for s in sessions(&mgr) {
                    apply(&s);
                }
                attached.push((id, mgr, watch));
            }
        };
        attach(&mut attached);

        while let Ok(cmd) = rx.recv() {
            match cmd {
                Cmd::Devices => attach(&mut attached),
                Cmd::Sweep => {
                    for (_, mgr, _) in &attached {
                        for s in sessions(mgr) {
                            apply(&s);
                        }
                    }
                }
                Cmd::Stop(done) => {
                    for (_, mgr, watch) in &attached {
                        let _ = mgr.UnregisterSessionNotification(watch);
                    }
                    let _ = en.UnregisterEndpointNotificationCallback(&devices);
                    let _ = done.send(());
                    return;
                }
            }
        }
    }
}

fn start_worker(mode: Mode) {
    *MODE.lock().unwrap() = Some(mode);
    let mut w = WORKER.lock().unwrap();
    if let Some(tx) = w.as_ref() {
        let _ = tx.send(Cmd::Sweep);
        return;
    }
    let (tx, rx) = channel();
    let tx2 = tx.clone();
    std::thread::spawn(move || worker(rx, tx2));
    *w = Some(tx);
}

fn stop_worker() {
    *MODE.lock().unwrap() = None;
    if let Some(tx) = WORKER.lock().unwrap().take() {
        let (done, wait) = channel();
        if tx.send(Cmd::Stop(done)).is_ok() {
            let _ = wait.recv_timeout(std::time::Duration::from_secs(3));
        }
    }
}

// MARK: the guard

/// Starts muting. The watchdog is the caller's to start (see watchdog::spawn).
pub fn engage() {
    start_worker(Mode::Show);
    showlog::note(format!("audio: muting apps not allowed to play (allowed: {:?})", settings::allowed_audio_apps()));
}

/// The allow-list changed mid-show.
pub fn allow_list_changed() {
    if *MODE.lock().unwrap() == Some(Mode::Show) {
        if let Some(tx) = WORKER.lock().unwrap().as_ref() {
            let _ = tx.send(Cmd::Sweep);
        }
    }
}

/// Stops muting and unmutes every journalled app that is still muted. Apps that have quit
/// go on the pending list. Safe to run from the watchdog after Show Mode has died.
pub fn restore() -> bool {
    stop_worker();
    let journalled = journalled();
    if journalled.is_empty() {
        journal::remove("audioMuted");
        resume_pending();
        return true;
    }
    let mut seen = BTreeSet::new();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) {
            for (_, dev) in outputs(&en) {
                let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
                for s in sessions(&mgr) {
                    let Some(exe) = session_exe(&s) else { continue };
                    if !journalled.contains(&exe) {
                        continue;
                    }
                    if let Ok(vol) = s.cast::<ISimpleAudioVolume>() {
                        if vol.GetMute().is_ok_and(|m| m.as_bool()) && vol.SetMute(false, std::ptr::null()).is_ok() {
                            showlog::note(format!("audio: unmuted {exe}"));
                        }
                        seen.insert(exe);
                    }
                }
            }
        }
    }
    let gone: BTreeSet<String> = journalled.difference(&seen).cloned().collect();
    if !gone.is_empty() {
        let mut list = pending();
        list.extend(gone.iter().cloned());
        save_pending(&list);
        showlog::note(format!("audio: not running, unmuted when they next play: {gone:?}"));
    }
    journal::remove("audioMuted");
    resume_pending();
    true
}

/// At launch and after a restore: while anything is pending, watch for it.
pub fn resume_pending() {
    if !pending().is_empty() && MODE.lock().unwrap().is_none() {
        start_worker(Mode::Pending);
    }
}

/// The apps with an audio session now, by executable name, and whether each is playing.
/// For the Allowed to Play menu; works on any thread with COM initialised.
pub fn current_apps() -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    unsafe {
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return out };
        for (_, dev) in outputs(&en) {
            let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            for s in sessions(&mgr) {
                let Some(path) = session_process(&s) else { continue };
                let playing = s.GetState().is_ok_and(|st| st == AudioSessionStateActive);
                let name = util::file_name(&path).to_string();
                match out.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                    Some(e) => e.1 |= playing,
                    None => out.push((name, playing)),
                }
            }
        }
    }
    out.sort_by_key(|(n, _)| n.to_ascii_lowercase());
    out
}
