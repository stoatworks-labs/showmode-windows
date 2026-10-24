//! Other apps that can spoil a show from the background.
//!
//! **Quit and reopen.** f.lux and LightBulb tint every screen with the time of day; OneDrive,
//! Dropbox, Google Drive and friends sync over the show network and work the disk. None can be
//! paused from outside, so they're asked to quit and opened again when the show ends. Asked, never
//! killed: f.lux killed outright would leave its tint on the screens. OneDrive is asked with its
//! own documented /shutdown; the others are sent WM_CLOSE, as `taskkill` without /f sends it. An app
//! that hasn't gone within five seconds is left running and named in a warning. Each one's path
//! is journalled before it's asked, so a crash mid-show still reopens it at the next launch.
//!
//! **Syncthing**, through its own local REST API: every folder that is syncing is paused for the
//! show and only those are resumed afterwards, as on macOS. Syncthing saves the paused state, so a
//! folder it couldn't resume stays journalled and is tried again.
//!
//! **Looking only.** The remote-access and backup tools the pre-show checks name.

use crate::{journal, showlog, util};
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, WAIT_TIMEOUT, WPARAM};
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE};

// MARK: processes

pub struct Process {
    pub pid: u32,
    pub exe: String,
}

/// Every process on the PC, by executable name.
pub fn processes() -> Vec<Process> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snap, &mut e).is_ok();
        while more {
            let end = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            out.push(Process { pid: e.th32ProcessID, exe: String::from_utf16_lossy(&e.szExeFile[..end]) });
            more = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn session_of(pid: u32) -> Option<u32> {
    let mut s = 0u32;
    unsafe { ProcessIdToSessionId(pid, &mut s) }.ok()?;
    Some(s)
}

/// This user's session's processes: the only ones Show Mode quits or reopens.
fn own_processes() -> Vec<Process> {
    let mine = session_of(unsafe { GetCurrentProcessId() });
    processes().into_iter().filter(|p| mine.is_some() && session_of(p.pid) == mine).collect()
}

/// The display names of the tools in `table` that are running, in any session (a remote-access
/// tool usually runs as a service).
fn running(table: &[(&str, &str)]) -> Vec<String> {
    let procs = processes();
    let mut out: Vec<String> = Vec::new();
    for (exe, name) in table {
        if procs.iter().any(|p| p.exe.eq_ignore_ascii_case(exe)) && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

// MARK: quit and reopen

struct Quittable {
    name: &'static str,
    exe: &'static str,
    /// Run the app's own executable with these to ask it to quit, instead of WM_CLOSE.
    quit_with: Option<&'static str>,
    /// What it's opened again with: the arguments its own startup entry uses.
    reopen_with: &'static str,
}

const COLOUR_APPS: &[Quittable] = &[
    Quittable { name: "f.lux", exe: "flux.exe", quit_with: None, reopen_with: "/noshow" },
    Quittable { name: "LightBulb", exe: "LightBulb.exe", quit_with: None, reopen_with: "" },
];

const SYNC_APPS: &[Quittable] = &[
    Quittable { name: "OneDrive", exe: "OneDrive.exe", quit_with: Some("/shutdown"), reopen_with: "/background" },
    Quittable { name: "Dropbox", exe: "Dropbox.exe", quit_with: None, reopen_with: "" },
    Quittable { name: "Google Drive", exe: "GoogleDriveFS.exe", quit_with: None, reopen_with: "" },
    Quittable { name: "Box Drive", exe: "Box.exe", quit_with: None, reopen_with: "" },
    Quittable { name: "MEGA", exe: "MEGAsync.exe", quit_with: None, reopen_with: "" },
    Quittable { name: "Resilio Sync", exe: "Resilio Sync.exe", quit_with: None, reopen_with: "" },
];

/// Sends WM_CLOSE to every top-level window the process owns, shown or not: a notification-area
/// app's window is usually hidden.
fn close_windows(pid: u32) {
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        let mut owner = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut owner));
        if owner == lp.0 as u32 {
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        true.into()
    }
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(pid as isize));
    }
}

fn spawn(path: &str, args: &str) -> std::io::Result<std::process::Child> {
    const DETACHED_PROCESS: u32 = 0x8;
    let mut c = std::process::Command::new(path);
    if !args.is_empty() {
        c.args(args.split(' '));
    }
    if let Some(dir) = Path::new(path).parent() {
        c.current_dir(dir);
    }
    c.creation_flags(DETACHED_PROCESS).spawn()
}

fn journalled() -> Vec<Value> {
    journal::get("quitApps").and_then(|v| v.as_array().cloned()).unwrap_or_default()
}

/// Asks every running app in `table` to quit. Returns a warning naming any that wouldn't.
fn quit(table: &[Quittable]) -> Option<String> {
    let procs = own_processes();
    let mut asked: Vec<(&Quittable, u32, String)> = Vec::new();
    for app in table {
        for p in procs.iter().filter(|p| p.exe.eq_ignore_ascii_case(app.exe)) {
            let Some(path) = util::exe_path_of_pid(p.pid) else { continue };
            // Journalled before it's asked: a crash in between still reopens it.
            let mut list = journalled();
            if !list.iter().any(|v| v["path"].as_str() == Some(&path)) {
                list.push(json!({ "name": app.name, "path": path, "args": app.reopen_with }));
                journal::set("quitApps", Value::Array(list));
            }
            asked.push((app, p.pid, path));
        }
    }
    if asked.is_empty() {
        return None;
    }
    let mut quit_run: Vec<&str> = Vec::new();
    for (app, pid, path) in &asked {
        match app.quit_with {
            // Once per app: the running copy does the quitting.
            Some(args) if !quit_run.contains(&app.name) => {
                quit_run.push(app.name);
                if let Err(e) = spawn(path, args) {
                    showlog::note(format!("apps: couldn't ask {} to quit: {e}", app.name));
                }
            }
            Some(_) => {}
            None => close_windows(*pid),
        }
    }
    // Five seconds for the lot, together.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stayed: Vec<&str> = Vec::new();
    for (app, pid, _) in &asked {
        let gone = unsafe {
            match OpenProcess(PROCESS_SYNCHRONIZE, false, *pid) {
                Ok(h) => {
                    let left = deadline.saturating_duration_since(Instant::now()).as_millis() as u32;
                    let r = WaitForSingleObject(h, left);
                    let _ = CloseHandle(h);
                    r != WAIT_TIMEOUT
                }
                Err(_) => true, // already gone
            }
        };
        if !gone && !stayed.contains(&app.name) {
            stayed.push(app.name);
        }
    }
    // An app still running needs nothing reopened.
    let still: Vec<String> = asked
        .iter()
        .filter(|(app, _, _)| stayed.contains(&app.name))
        .map(|(_, _, path)| path.clone())
        .collect();
    let list: Vec<Value> = journalled().into_iter().filter(|v| !still.iter().any(|p| v["path"].as_str() == Some(p))).collect();
    if list.is_empty() {
        journal::remove("quitApps");
    } else {
        journal::set("quitApps", Value::Array(list));
    }
    let quit: Vec<String> = asked
        .iter()
        .filter(|(app, _, _)| !stayed.contains(&app.name))
        .map(|(app, _, _)| app.name.to_string())
        .fold(Vec::new(), |mut v, n| {
            if !v.contains(&n) {
                v.push(n)
            }
            v
        });
    if !quit.is_empty() {
        showlog::note(format!("apps: quit {}", util::list(&quit)));
    }
    if stayed.is_empty() {
        return None;
    }
    let names: Vec<String> = stayed.iter().map(|s| s.to_string()).collect();
    showlog::note(format!("apps: {} didn't quit within five seconds", util::list(&names)));
    Some(format!("{} wouldn't quit: pause it from its own icon", util::list(&names)))
}

pub fn quit_colour_apps() -> Option<String> {
    quit(COLOUR_APPS)
}

pub fn quit_sync_apps() -> Option<String> {
    quit(SYNC_APPS)
}

/// Opens again every app the show quit that isn't running already. An app whose program has
/// gone needs nothing; one that wouldn't start stays journalled for another try.
pub fn reopen() -> bool {
    let list = journalled();
    if list.is_empty() {
        journal::remove("quitApps");
        return true;
    }
    let procs = own_processes();
    let mut left = Vec::new();
    for v in list {
        let (name, path, args) = (
            v["name"].as_str().unwrap_or_default(),
            v["path"].as_str().unwrap_or_default(),
            v["args"].as_str().unwrap_or_default(),
        );
        let exe = util::file_name(path);
        if procs.iter().any(|p| p.exe.eq_ignore_ascii_case(exe)) {
            continue;
        }
        if !Path::new(path).exists() {
            showlog::note(format!("apps: {name} isn't at {path} any more; nothing to reopen"));
            continue;
        }
        match spawn(path, args) {
            Ok(_) => showlog::note(format!("apps: reopened {name}")),
            Err(e) => {
                showlog::note(format!("apps: couldn't reopen {name}: {e}"));
                left.push(v);
            }
        }
    }
    if left.is_empty() {
        journal::remove("quitApps");
        true
    } else {
        journal::set("quitApps", Value::Array(left));
        false
    }
}

// MARK: Syncthing

struct Syncthing {
    https: bool,
    port: u16,
    key: String,
}

fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let a = s.find(open)? + open.len();
    let b = s[a..].find(close)? + a;
    Some(&s[a..b])
}

/// The GUI's port and API key from Syncthing's config; None if it isn't set up for this user
/// or listens on a socket. Always the loopback address: the GUI may listen on every interface.
fn syncthing() -> Option<Syncthing> {
    let dir = std::env::var_os("LOCALAPPDATA")?;
    let config = std::fs::read_to_string(Path::new(&dir).join("Syncthing").join("config.xml")).ok()?;
    let gui = between(&config, "<gui", "</gui>")?;
    let open_tag = &gui[..gui.find('>')?];
    let https = open_tag.contains(r#"tls="true""#);
    let address = between(gui, "<address>", "</address>")?;
    let port = address.rsplit(':').next()?.trim().parse().ok()?;
    let key = between(gui, "<apikey>", "</apikey>")?.trim().to_string();
    (!key.is_empty()).then_some(Syncthing { https, port, key })
}

/// One request with three-second timeouts, accepting the self-signed certificate Syncthing's
/// GUI uses (on the loopback address only). None if Syncthing didn't answer.
fn call(api: &Syncthing, method: &str, path: &str, body: Option<&str>) -> Option<(u32, Vec<u8>)> {
    unsafe {
        let session = WinHttpOpen(w!("Show Mode"), WINHTTP_ACCESS_TYPE_NO_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
        if session.is_null() {
            return None;
        }
        let _ = WinHttpSetTimeouts(session, 3000, 3000, 3000, 3000);
        let result = (|| {
            let connect = WinHttpConnect(session, w!("127.0.0.1"), api.port, 0);
            if connect.is_null() {
                return None;
            }
            let (m, p) = (util::wide(method), util::wide(path));
            let flags = if api.https { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
            let request =
                WinHttpOpenRequest(connect, PCWSTR(m.as_ptr()), PCWSTR(p.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null_mut(), flags);
            let out = (|| {
                if request.is_null() {
                    return None;
                }
                if api.https {
                    let ignore: u32 = SECURITY_FLAG_IGNORE_UNKNOWN_CA
                        | SECURITY_FLAG_IGNORE_CERT_CN_INVALID
                        | SECURITY_FLAG_IGNORE_CERT_DATE_INVALID
                        | SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE;
                    let _ = WinHttpSetOption(Some(request), WINHTTP_OPTION_SECURITY_FLAGS, Some(&ignore.to_ne_bytes()));
                }
                let headers = util::wide(&format!("X-API-Key: {}\r\nContent-Type: application/json\r\n", api.key));
                let body = body.unwrap_or("").as_bytes();
                let len = body.len() as u32;
                WinHttpSendRequest(
                    request,
                    Some(&headers[..headers.len() - 1]),
                    (len > 0).then_some(body.as_ptr() as *const _),
                    len,
                    len,
                    0,
                )
                .ok()?;
                WinHttpReceiveResponse(request, std::ptr::null_mut()).ok()?;
                let mut status = 0u32;
                let mut size = 4u32;
                WinHttpQueryHeaders(
                    request,
                    WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                    PCWSTR::null(),
                    Some(&mut status as *mut u32 as _),
                    &mut size,
                    std::ptr::null_mut(),
                )
                .ok()?;
                let mut data = Vec::new();
                loop {
                    let mut buf = [0u8; 8192];
                    let mut n = 0u32;
                    if WinHttpReadData(request, buf.as_mut_ptr() as _, buf.len() as u32, &mut n).is_err() || n == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..n as usize]);
                }
                Some((status, data))
            })();
            if !request.is_null() {
                let _ = WinHttpCloseHandle(request);
            }
            let _ = WinHttpCloseHandle(connect);
            out
        })();
        let _ = WinHttpCloseHandle(session);
        result
    }
}

/// Asked first, because Windows takes about two seconds to give up on a refused loopback
/// connection (winlab): with Syncthing set up but not running, every call would cost that.
fn syncthing_running() -> bool {
    processes().iter().any(|p| p.exe.eq_ignore_ascii_case("syncthing.exe"))
}

fn folder_path(id: &str) -> String {
    let mut s = String::from("/rest/config/folders/");
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

fn set_paused(api: &Syncthing, id: &str, paused: bool) -> bool {
    let body = json!({ "paused": paused }).to_string();
    call(api, "PATCH", &folder_path(id), Some(&body)).is_some_and(|(code, _)| code == 200)
}

/// Pauses every folder that's syncing. Returns a warning if Syncthing answered but wouldn't
/// pause one; nothing when it's paused or not running at all.
pub fn pause_syncthing() -> Option<String> {
    let api = syncthing()?;
    if !syncthing_running() {
        return None;
    }
    let (code, data) = call(&api, "GET", "/rest/config/folders", None)?;
    if code != 200 {
        showlog::note(format!("syncthing: listing folders answered {code}"));
        return Some("Syncthing wasn't paused: it refused Show Mode's request".into());
    }
    let folders: Vec<Value> = serde_json::from_slice(&data).ok()?;
    let syncing: Vec<String> = folders
        .iter()
        .filter(|f| f["paused"].as_bool() == Some(false))
        .filter_map(|f| f["id"].as_str().map(str::to_string))
        .collect();
    if syncing.is_empty() {
        return None;
    }
    // Journalled first, added to any leftover list.
    let mut list: Vec<Value> = journal::get("syncthingFolders").and_then(|v| v.as_array().cloned()).unwrap_or_default();
    for id in &syncing {
        if !list.iter().any(|v| v.as_str() == Some(id)) {
            list.push(json!(id));
        }
    }
    journal::set("syncthingFolders", Value::Array(list));
    let failed = syncing.iter().filter(|id| !set_paused(&api, id, true)).count();
    showlog::note(format!("syncthing: paused {} of {} folders", syncing.len() - failed, syncing.len()));
    (failed > 0).then(|| format!("Syncthing didn't pause {failed} of its folders"))
}

pub fn resume_syncthing() -> bool {
    let Some(ids) = journal::get("syncthingFolders").and_then(|v| v.as_array().cloned()) else { return true };
    // With no Syncthing config any more, there's nothing to resume.
    let Some(api) = syncthing() else {
        journal::remove("syncthingFolders");
        return true;
    };
    if !syncthing_running() {
        showlog::note(format!("syncthing: not running, so {} folders stay paused until it's back", ids.len()));
        return false;
    }
    let left: Vec<Value> = ids.into_iter().filter(|v| !v.as_str().is_some_and(|id| set_paused(&api, id, false))).collect();
    if left.is_empty() {
        showlog::note("syncthing: folders resumed");
        journal::remove("syncthingFolders");
        true
    } else {
        showlog::note(format!("syncthing: {} folders not resumed (not answering?)", left.len()));
        journal::set("syncthingFolders", Value::Array(left));
        false
    }
}

// MARK: looking only

const REMOTE_TOOLS: &[(&str, &str)] = &[
    ("TeamViewer.exe", "TeamViewer"),
    ("TeamViewer_Service.exe", "TeamViewer"),
    ("AnyDesk.exe", "AnyDesk"),
    ("rustdesk.exe", "RustDesk"),
    ("parsecd.exe", "Parsec"),
    ("remoting_host.exe", "Chrome Remote Desktop"),
    ("SRServer.exe", "Splashtop"),
    ("tvnserver.exe", "TightVNC"),
    ("winvnc.exe", "UltraVNC"),
    ("vncserver.exe", "RealVNC"),
];

const BACKUP_TOOLS: &[(&str, &str)] = &[
    ("bzserv.exe", "Backblaze"),
    ("CrashPlanService.exe", "CrashPlan"),
    ("CarboniteService.exe", "Carbonite"),
    ("Duplicati.Server.exe", "Duplicati"),
    ("Duplicati.GUI.TrayIcon.exe", "Duplicati"),
];

pub fn remote_tools() -> Vec<String> {
    running(REMOTE_TOOLS)
}

pub fn backup_tools() -> Vec<String> {
    running(BACKUP_TOOLS)
}

/// The sync apps running for this user (for the check when they're not being quit).
pub fn sync_apps() -> Vec<String> {
    let table: Vec<(&str, &str)> = SYNC_APPS.iter().map(|a| (a.exe, a.name)).collect();
    let mine: Vec<String> = own_processes().into_iter().map(|p| p.exe).collect();
    table.iter().filter(|(exe, _)| mine.iter().any(|m| m.eq_ignore_ascii_case(exe))).map(|(_, n)| n.to_string()).fold(
        Vec::new(),
        |mut v, n| {
            if !v.contains(&n) {
                v.push(n)
            }
            v
        },
    )
}

/// For --status.
pub fn describe() -> String {
    let mine: Vec<String> = own_processes().into_iter().map(|p| p.exe).collect();
    let up = |t: &[Quittable]| -> Vec<String> {
        t.iter().filter(|a| mine.iter().any(|m| m.eq_ignore_ascii_case(a.exe))).map(|a| a.name.to_string()).collect()
    };
    let st = match syncthing() {
        None => "not set up".to_string(),
        Some(api) if !syncthing_running() => format!("set up on port {}, not running", api.port),
        Some(api) => match call(&api, "GET", "/rest/config/folders", None) {
            Some((200, data)) => {
                let folders: Vec<Value> = serde_json::from_slice(&data).unwrap_or_default();
                let paused = folders.iter().filter(|f| f["paused"].as_bool() == Some(true)).count();
                format!("{} folders, {paused} paused ({}:{})", folders.len(), if api.https { "https" } else { "http" }, api.port)
            }
            Some((code, _)) => format!("answered {code}"),
            None => format!("not answering on port {}", api.port),
        },
    };
    format!(
        "colour apps running: {:?}; sync apps running: {:?}; syncthing: {st}; remote tools: {:?}; backup tools: {:?}; reopen after the show: {}",
        up(COLOUR_APPS),
        up(SYNC_APPS),
        remote_tools(),
        backup_tools(),
        journalled().len()
    )
}
