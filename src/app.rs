//! The notification-area app: a hidden window that owns the tray icon, the menu, the global
//! hotkeys and the timers, and answers other copies of ShowMode.exe (a showmode: URL opens one,
//! which hands its command over with WM_COPYDATA and exits).

use crate::controller::Controller;
use crate::integration::{self, Command};
use crate::settings::{self, Guard};
use crate::util::{list, wide};
use crate::{audio, icons, journal, note, paths, preflight, showlog};
use std::cell::RefCell;
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_F10, VK_F11};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const CLASS: PCWSTR = w!("StoatworksShowMode");
/// WM_COPYDATA tag for a command handed over by another copy ('SM').
pub const COPYDATA_COMMAND: usize = 0x534D;

const WM_TRAY: u32 = WM_APP + 1;
/// A command handed over by another copy, run from the message loop (see WM_COPYDATA).
const WM_RUN_COMMAND: u32 = WM_APP + 2;
/// NIN_SELECT | NINF_KEY: the icon chosen with the keyboard.
const NIN_KEYSELECT: u32 = 0x401;
const TRAY_ID: u32 = 1;
const HOTKEY_TOGGLE: i32 = 1;
/// Pauses and resumes the cursor fence.
const HOTKEY_FENCE: i32 = 2;
const TIMER_WATCH: usize = 1;
/// A second after the last WM_DISPLAYCHANGE: Windows sends several while a screen comes or goes.
const TIMER_DISPLAY: usize = 2;
/// How long the note stays up when a restart is waiting.
const URGENT_NOTE_MS: u32 = 20_000;

// Menu command ids.
const ID_TOGGLE: usize = 1;
const ID_FINISH: usize = 2;
const ID_LOGIN: usize = 3;
const ID_LOG: usize = 4;
const ID_ABOUT: usize = 5;
const ID_QUIT: usize = 6;
const ID_UPDATE: usize = 7;
const ID_FENCE_PAUSE: usize = 8;
const ID_GUARD: usize = 100;
const ID_SCREEN: usize = 150;
const ID_APP: usize = 200;

struct App {
    hwnd: HWND,
    show: Controller,
    icon: Option<HICON>,
    taskbar_created: u32,
    hotkey_ok: bool,
    fence_hotkey_ok: bool,
    /// The names behind the Allowed to Play items, by position, from the last menu built.
    menu_apps: Vec<String>,
    /// The screens behind the Keep Cursor Off items: device path, and whether it's main.
    menu_screens: Vec<(String, bool)>,
    /// The last WM_DISPLAYCHANGE, so the five-second check never acts mid-change.
    display_changed: Option<Instant>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs `f` on the app. Never call it while a modal loop (the menu, a message box) is up with
/// a borrow held: their messages come back through the window procedure.
fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> Option<T> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

pub fn run(first_command: Option<Command>) {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance, lpszClassName: CLASS, ..Default::default() };
        RegisterClassW(&wc);
        note::register(instance);
        // A real top-level window (never shown), not a message-only one: only top-level windows
        // hear the broadcasts this needs (TaskbarCreated, WM_DISPLAYCHANGE, WM_SETTINGCHANGE).
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS,
            w!("Show Mode"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            instance,
            None,
        ) else {
            showlog::note("launch: could not create the app window");
            return;
        };

        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        let hotkey_ok = RegisterHotKey(hwnd, HOTKEY_TOGGLE, MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT, VK_F11.0 as u32).is_ok();
        if !hotkey_ok {
            showlog::note("launch: Ctrl+Alt+Shift+F11 is taken by another app");
        }
        let fence_hotkey_ok =
            RegisterHotKey(hwnd, HOTKEY_FENCE, MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT, VK_F10.0 as u32).is_ok();
        if !fence_hotkey_ok {
            showlog::note("launch: Ctrl+Alt+Shift+F10 is taken by another app");
        }
        APP.with(|a| {
            *a.borrow_mut() =
                Some(App {
                    hwnd,
                    show: Controller::default(),
                    icon: None,
                    taskbar_created,
                    hotkey_ok,
                    fence_hotkey_ok,
                    menu_apps: Vec::new(),
                    menu_screens: Vec::new(),
                    display_changed: None,
                })
        });
        integration::register_url_scheme();
        update_tray(true);
        SetTimer(hwnd, TIMER_WATCH, 5000, None);

        let recovered = with_app(|a| a.show.recover_if_needed()).unwrap_or(false);
        if recovered {
            let left = with_app(|a| a.show.warnings.clone()).unwrap_or_default();
            if left.is_empty() {
                note::show("Show Mode put back the settings from a show that didn't finish.");
            } else {
                note::show(&left.join("\n"));
            }
        }
        if let Some(c) = first_command {
            command(c);
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn command(c: Command) {
    let engaged = with_app(|a| a.show.engaged).unwrap_or(false);
    match c {
        Command::On if !engaged => toggle(),
        Command::Off if engaged => toggle(),
        Command::Toggle => toggle(),
        _ => {}
    }
}

fn toggle() {
    let Some((engaged, warnings, hwnd)) = with_app(|a| {
        a.show.toggle();
        (a.show.engaged, a.show.warnings.clone(), a.hwnd)
    }) else {
        return;
    };
    unsafe {
        // While a show runs, an operator's accidental Start > Restart stops at Windows' list of
        // apps holding it up, with this as the reason. It can't stop a forced update restart.
        if engaged {
            let _ = ShutdownBlockReasonCreate(hwnd, w!("A show is running. End Show Mode first to put the PC's settings back."));
        } else {
            let _ = ShutdownBlockReasonDestroy(hwnd);
        }
    }
    update_tray(false);
    if engaged {
        // What couldn't be applied, then what Show Mode can only point out. A waiting restart
        // leads, and keeps the note up longer.
        let checks = preflight::items();
        for c in &checks {
            showlog::note(format!("check: {}", c.text));
        }
        let urgent: Vec<String> = checks.iter().filter(|c| c.urgent).map(|c| format!("⚠ {}", c.text)).collect();
        let lines: Vec<String> = urgent
            .iter()
            .cloned()
            .chain(warnings.iter().cloned())
            .chain(checks.iter().filter(|c| !c.urgent).map(|c| c.text.clone()))
            .collect();
        if !lines.is_empty() {
            let text = format!("Show Mode is on, but:\n{}", lines.join("\n"));
            if urgent.is_empty() {
                note::show(&text);
            } else {
                note::show_for(&text, URGENT_NOTE_MS);
            }
        }
    } else if !warnings.is_empty() {
        note::show(&format!("Show Mode is off.\n{}", warnings.join("\n")));
    }
}

/// Puts the display layout right if the display lock is on, and says so if it stands down.
fn display_check(why: &str) {
    if let Some(problem) = with_app(|a| a.show.enforce_display(why)).flatten() {
        note::show(&problem);
    }
}

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: TRAY_ID, ..Default::default() }
}

/// Adds the icon (first time, or after Explorer restarts) or refreshes it for the state.
fn update_tray(add: bool) {
    let Some((hwnd, on, paused, old)) =
        with_app(|a| (a.hwnd, a.show.engaged, a.show.fence.as_ref().is_some_and(|f| f.paused()), a.icon.take()))
    else {
        return;
    };
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96);
        let px = GetSystemMetricsForDpi(SM_CXSMICON, dpi);
        let icon = icons::tray(on, px);
        let mut nid = tray_data(hwnd);
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = icon.unwrap_or_default();
        let tip = wide(match (on, paused) {
            (true, true) => "Show Mode: on, cursor fence paused",
            (true, false) => "Show Mode: on",
            _ => "Show Mode: off",
        });
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        if add {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
            nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
        } else {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
        if let Some(old) = old {
            let _ = DestroyIcon(old);
        }
        with_app(|a| a.icon = icon);
    }
}

fn remove_tray() {
    if let Some(hwnd) = with_app(|a| a.hwnd) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
        }
    }
}

fn menu_text(s: &str) -> Vec<u16> {
    wide(&s.replace('&', "&&"))
}

unsafe fn item(menu: HMENU, id: usize, text: &str, checked: bool, enabled: bool) {
    let mut flags = MF_STRING;
    if checked {
        flags |= MF_CHECKED;
    }
    if !enabled {
        flags |= MF_GRAYED;
    }
    let t = menu_text(text);
    let _ = AppendMenuW(menu, flags, id, PCWSTR(t.as_ptr()));
}

unsafe fn separator(menu: HMENU) {
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
}

unsafe fn submenu(menu: HMENU, sub: HMENU, text: &str) {
    let t = menu_text(text);
    let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(t.as_ptr()));
}

/// Builds the menu, shows it at (x, y) and returns the chosen command (0 for none).
fn show_menu(x: i32, y: i32) -> usize {
    let Some((hwnd, engaged, warnings, unfinished, hotkey_ok, fence_hotkey_ok, fence)) = with_app(|a| {
        (
            a.hwnd,
            a.show.engaged,
            a.show.warnings.clone(),
            a.show.unfinished(),
            a.hotkey_ok,
            a.fence_hotkey_ok,
            a.show.fence.as_ref().map(|f| f.paused()),
        )
    }) else {
        return 0;
    };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return 0 };
        let key = if hotkey_ok { "\tCtrl+Alt+Shift+F11" } else { "" };
        item(menu, ID_TOGGLE, &format!("{}{key}", if engaged { "End Show Mode" } else { "Start Show Mode" }), false, true);
        if unfinished {
            item(menu, ID_FINISH, "Finish Restoring", false, true);
        }
        // A waiting restart leads, whether or not a show is running, and opens Windows Update.
        let checks = preflight::items();
        for c in checks.iter().filter(|c| c.urgent) {
            item(menu, ID_UPDATE, &format!("⚠ {}", c.text), false, true);
        }
        for w in &warnings {
            item(menu, 0, &format!("⚠ {w}"), false, false);
        }
        // While a show runs, the other checks that can change mid-show.
        for c in checks.iter().filter(|c| engaged && c.live && !c.urgent) {
            item(menu, 0, &format!("⚠ {}", c.text), false, false);
        }
        let checks_menu = CreatePopupMenu().unwrap();
        for c in &checks {
            item(checks_menu, 0, &format!("⚠ {}", c.text), false, false);
        }
        if checks.is_empty() {
            item(checks_menu, 0, "Nothing to look at", false, false);
        }
        separator(checks_menu);
        for line in [
            "Before doors: open every show app once, so a Windows Firewall prompt",
            "comes now and not mid-show, and plug in every screen you'll use.",
        ] {
            item(checks_menu, 0, line, false, false);
        }
        let title = if checks.is_empty() { "Pre-show Checks".to_string() } else { format!("Pre-show Checks ({})", checks.len()) };
        submenu(menu, checks_menu, &title);
        separator(menu);

        let guards = CreatePopupMenu().unwrap();
        for (i, g) in Guard::ALL.iter().enumerate() {
            if g.starts_section() {
                separator(guards);
            }
            item(guards, ID_GUARD + i, g.title(), settings::is_on(*g), true);
        }
        submenu(menu, guards, "What Show Mode Disables");

        // Keep Cursor Off: the screens connected now. The last unticked one can't be ticked.
        let screens_menu = CreatePopupMenu().unwrap();
        let screens = crate::display::screens();
        let fenced: Vec<bool> = screens.iter().map(|s| settings::is_fenced_screen(&s.key)).collect();
        let unticked = fenced.iter().filter(|f| !**f).count();
        if screens.len() < 2 {
            item(screens_menu, 0, "Only one screen: nothing to keep the cursor off", false, false);
        }
        for (i, s) in screens.iter().enumerate().filter(|_| screens.len() >= 2) {
            let twin = screens.iter().filter(|o| o.name == s.name).count() > 1;
            let label = format!(
                "{}{}{}",
                s.name,
                if twin { format!(" ({}x{} at {},{})", s.size.0, s.size.1, s.origin.0, s.origin.1) } else { String::new() },
                if s.origin == (0, 0) { "  (main: taskbar)" } else { "" }
            );
            item(screens_menu, ID_SCREEN + i, &label, fenced[i], fenced[i] || unticked > 1);
        }
        if let Some(paused) = fence {
            separator(screens_menu);
            let key = if fence_hotkey_ok { "\tCtrl+Alt+Shift+F10" } else { "" };
            item(screens_menu, ID_FENCE_PAUSE, &format!("Pause Cursor Fence{key}"), paused, true);
        }
        submenu(menu, screens_menu, "Keep Cursor Off");
        with_app(|a| a.menu_screens = screens.iter().map(|s| (s.key.clone(), s.origin == (0, 0))).collect());

        // Allowed to Play: apps with sound now, plus every app already ticked.
        let apps_menu = CreatePopupMenu().unwrap();
        let mut names: Vec<(String, bool)> = audio::current_apps();
        for a in settings::allowed_audio_apps() {
            if !names.iter().any(|(n, _)| n.eq_ignore_ascii_case(&a)) {
                names.push((a, false));
            }
        }
        names.sort_by_key(|(n, _)| n.to_ascii_lowercase());
        if names.is_empty() {
            item(apps_menu, 0, "No app is playing sound", false, false);
        }
        for (i, (n, playing)) in names.iter().enumerate() {
            let label = if *playing { format!("{n}  (playing)") } else { n.clone() };
            item(apps_menu, ID_APP + i, &label, settings::is_allowed_audio_app(n), true);
        }
        submenu(menu, apps_menu, "Allowed to Play");
        with_app(|a| a.menu_apps = names.into_iter().map(|(n, _)| n).collect());

        separator(menu);
        item(menu, ID_LOGIN, "Launch at Login", integration::launches_at_login(), true);
        item(menu, ID_LOG, "Open Log", false, true);
        item(menu, ID_ABOUT, &format!("About Show Mode {}", env!("CARGO_PKG_VERSION")), false, true);
        item(menu, ID_QUIT, "Quit", false, true);

        // Without the foreground window, the menu wouldn't close when clicking elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, x, y, 0, hwnd, None);
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        chosen.0 as usize
    }
}

fn menu_command(id: usize) {
    match id {
        ID_TOGGLE => toggle(),
        ID_FINISH => {
            let left = with_app(|a| {
                a.show.restore();
                a.show.warnings.clone()
            })
            .unwrap_or_default();
            note::show(if left.is_empty() { "Everything is put back." } else { &left[0] });
        }
        ID_LOGIN => integration::set_launch_at_login(!integration::launches_at_login()),
        ID_LOG => unsafe {
            let p = wide(&paths::log_file().to_string_lossy());
            ShellExecuteW(None, w!("open"), PCWSTR(p.as_ptr()), None, None, SW_SHOWNORMAL);
        },
        ID_ABOUT => unsafe {
            let text = wide(&format!(
                "Show Mode {} for Windows\n\nStops your show PC embarrassing you, and puts everything back afterwards.\n\nStoatworks Labs\nhttps://github.com/stoatworks-labs/showmode-windows",
                env!("CARGO_PKG_VERSION")
            ));
            MessageBoxW(None, PCWSTR(text.as_ptr()), w!("About Show Mode"), MB_OK | MB_ICONINFORMATION);
        },
        ID_QUIT => quit(),
        ID_UPDATE => unsafe {
            ShellExecuteW(None, w!("open"), w!("ms-settings:windowsupdate"), None, None, SW_SHOWNORMAL);
        },
        id if (ID_GUARD..ID_GUARD + Guard::ALL.len()).contains(&id) => {
            let g = Guard::ALL[id - ID_GUARD];
            settings::set_on(g, !settings::is_on(g));
        }
        ID_FENCE_PAUSE => toggle_fence_pause(),
        id if (ID_SCREEN..ID_APP).contains(&id) => {
            let Some((key, main)) = with_app(|a| a.menu_screens.get(id - ID_SCREEN).cloned()).flatten() else { return };
            if main && !settings::is_fenced_screen(&key) {
                let text = wide(
                    "Keep the cursor off the main display?\n\nThe taskbar, the notification area and Show Mode's own icon are there, so you couldn't reach them with the mouse during the show.\n\nUsually the better fix is to make the operator's screen the main display in Settings > System > Display before the show.",
                );
                let answer = unsafe { MessageBoxW(None, PCWSTR(text.as_ptr()), w!("Show Mode"), MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2) };
                if answer != IDYES {
                    return;
                }
            }
            settings::toggle_fenced_screen(&key);
            showlog::note(format!("fence: {key} {}", if settings::is_fenced_screen(&key) { "ticked" } else { "unticked" }));
            with_app(|a| {
                if let Some(f) = &a.show.fence {
                    f.screens_changed();
                }
            });
        }
        id if id >= ID_APP => {
            let name = with_app(|a| a.menu_apps.get(id - ID_APP).cloned()).flatten();
            if let Some(n) = name {
                settings::toggle_allowed_audio_app(&n);
                audio::allow_list_changed();
            }
        }
        _ => {}
    }
}

fn toggle_fence_pause() {
    let Some(paused) = with_app(|a| {
        let f = a.show.fence.as_ref()?;
        f.set_paused(!f.paused());
        Some(f.paused())
    })
    .flatten() else {
        return;
    };
    update_tray(false);
    note::show(if paused {
        "Cursor fence paused: Ctrl+Alt+Shift+F10 again to resume."
    } else {
        "Cursor fence back on."
    });
}

/// Quitting ends the show first, so nothing is left changed.
fn quit() {
    with_app(|a| {
        if a.show.engaged {
            a.show.restore();
        }
    });
    remove_tray();
    unsafe { PostQuitMessage(0) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            let event = (lp.0 & 0xffff) as u32;
            // Version 4 icons send NIN_SELECT for a left click (after the raw button messages,
            // which are ignored so the menu opens once) and WM_CONTEXTMENU for a right one.
            if event == WM_CONTEXTMENU || event == NIN_SELECT || event == NIN_KEYSELECT {
                let (x, y) = ((wp.0 & 0xffff) as i16 as i32, ((wp.0 >> 16) & 0xffff) as i16 as i32);
                let id = show_menu(x, y);
                if id != 0 {
                    menu_command(id);
                }
            }
            LRESULT(0)
        }
        WM_HOTKEY if wp.0 as i32 == HOTKEY_TOGGLE => {
            toggle();
            LRESULT(0)
        }
        WM_HOTKEY if wp.0 as i32 == HOTKEY_FENCE => {
            toggle_fence_pause();
            LRESULT(0)
        }
        WM_COPYDATA => {
            let cds = &*(lp.0 as *const COPYDATASTRUCT);
            if cds.dwData == COPYDATA_COMMAND && !cds.lpData.is_null() {
                let words = std::slice::from_raw_parts(cds.lpData as *const u16, cds.cbData as usize / 2);
                let text = String::from_utf16_lossy(words);
                showlog::note(format!("command: {}", text.trim_end_matches('\0')));
                // Not run here: WM_COPYDATA is a synchronous call from another process, and
                // COM refuses outgoing calls while one is in progress (RPC_E_CANTCALLOUT_
                // ININPUTSYNCCALL), which took Do Not Disturb and the wallpaper down on winlab.
                if let Some(c) = integration::parse(text.trim_end_matches('\0')) {
                    let code = match c {
                        Command::On => 1,
                        Command::Off => 2,
                        Command::Toggle => 3,
                    };
                    let _ = PostMessageW(hwnd, WM_RUN_COMMAND, WPARAM(code), LPARAM(0));
                }
            }
            LRESULT(1)
        }
        WM_RUN_COMMAND => {
            match wp.0 {
                1 => command(Command::On),
                2 => command(Command::Off),
                3 => command(Command::Toggle),
                _ => {}
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_WATCH => {
            with_app(|a| a.show.tick());
            // A backstop for a change that raised no WM_DISPLAYCHANGE, never in the middle of one.
            let quiet = with_app(|a| a.display_changed.is_none_or(|t| t.elapsed() > Duration::from_secs(3))).unwrap_or(false);
            if quiet {
                display_check("five-second check");
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_DISPLAY => {
            let _ = KillTimer(hwnd, TIMER_DISPLAY);
            // A screen plugged in mid-show gets the black wallpaper, and the layout is put right.
            with_app(|a| a.show.tick());
            display_check("after a display change");
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            // Sent, not posted, so nothing that calls COM runs here (see WM_COPYDATA): the work
            // waits for the timer, which also lets a burst of changes settle.
            with_app(|a| a.display_changed = Some(Instant::now()));
            SetTimer(hwnd, TIMER_DISPLAY, 1000, None);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_SETTINGCHANGE => {
            // The taskbar switching between light and dark.
            update_tray(false);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_QUERYENDSESSION => {
            // Holding a show: refuse an ordinary restart or sign-out, which puts this app on
            // Windows' list with its reason. A critical one can't be refused, so say yes.
            const ENDSESSION_CRITICAL: isize = 0x4000_0000;
            let engaged = with_app(|a| a.show.engaged).unwrap_or(false);
            LRESULT(if engaged && lp.0 & ENDSESSION_CRITICAL == 0 { 0 } else { 1 })
        }
        WM_ENDSESSION => {
            if wp.0 != 0 {
                showlog::note("session ending: restoring");
                with_app(|a| a.show.restore());
                remove_tray();
            }
            LRESULT(0)
        }
        WM_CLOSE => LRESULT(0),
        _ if msg == with_app(|a| a.taskbar_created).unwrap_or(u32::MAX) && msg != 0 => {
            update_tray(true);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// --status: what Show Mode can see, without changing anything.
pub fn status() -> String {
    let mut lines = vec![format!("Show Mode {} for Windows", env!("CARGO_PKG_VERSION"))];
    journal::load();
    lines.push(if journal::exists() {
        format!("journal: present ({})", list(&journal::keys()))
    } else {
        "journal: none".into()
    });
    lines.push(format!("power: {}", crate::power::describe()));
    lines.push(crate::spi::describe());
    lines.push(crate::dnd::describe());
    lines.push(crate::sounds::describe());
    lines.push(crate::explorer::describe());
    lines.push(crate::keys::describe());
    lines.push(crate::wallpaper::describe());
    lines.push(crate::display::describe());
    lines.push(crate::cursorfence::describe());
    lines.push(crate::apps::describe());
    let apps = audio::current_apps();
    lines.push(format!(
        "audio sessions: {}",
        if apps.is_empty() {
            "none".into()
        } else {
            apps.iter().map(|(n, p)| if *p { format!("{n} (playing)") } else { n.clone() }).collect::<Vec<_>>().join(", ")
        }
    ));
    lines.push(format!("allowed to play: {:?}", settings::allowed_audio_apps()));
    for g in Guard::ALL {
        lines.push(format!("guard {:<17} {}", g.id(), if settings::is_on(g) { "on" } else { "off" }));
    }
    lines.push(preflight::describe());
    lines.join("\n")
}
