//! Show Mode for Windows: locks a PC down for a show, then puts every setting back how you had
//! it.
//!
//!   ShowMode.exe                    the notification-area app
//!   ShowMode.exe showmode://on      hand a command (on, off, toggle) to the running app
//!   ShowMode.exe --status [--out F] print what it can see, changing nothing
//!   ShowMode.exe --watchdog <pid>   internal: unmutes apps if that Show Mode dies

#![cfg_attr(windows, windows_subsystem = "windows")]

mod chords;
mod cloudstore;
mod fence;
mod journal;
mod layout;
mod paths;
mod settings;
mod showlog;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod apps;
#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod controller;
#[cfg(windows)]
mod crashdialogs;
#[cfg(windows)]
mod cursorfence;
#[cfg(windows)]
mod display;
#[cfg(windows)]
mod dnd;
#[cfg(windows)]
mod explorer;
#[cfg(windows)]
mod hookthread;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod integration;
#[cfg(windows)]
mod keys;
#[cfg(windows)]
mod note;
#[cfg(windows)]
mod power;
#[cfg(windows)]
mod preflight;
#[cfg(windows)]
mod reg;
#[cfg(windows)]
mod sounds;
#[cfg(windows)]
mod spi;
#[cfg(windows)]
mod util;
#[cfg(windows)]
mod wallpaper;
#[cfg(windows)]
mod watchdog;

#[cfg(windows)]
fn main() {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, LPARAM, WPARAM};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SendMessageW, WM_COPYDATA};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();

    if let Some(pid) = value("--watchdog").and_then(|p| p.parse().ok()) {
        watchdog::run(pid);
        return;
    }

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    if args.iter().any(|a| a == "--status") {
        let text = app::status();
        match value("--out") {
            Some(path) => {
                let _ = std::fs::write(path, format!("{text}\n"));
            }
            None => unsafe {
                // A GUI-subsystem exe has no console of its own; borrow the caller's.
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
                println!("{text}");
            },
        }
        return;
    }

    let first = args.iter().find_map(|a| integration::parse(a));

    // One copy runs. A second one hands its command (if any) to the first and leaves.
    let _instance = unsafe { CreateMutexW(None, true, w!("Local\\StoatworksShowMode")) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        if let (Some(arg), Ok(hwnd)) = (args.first(), unsafe { FindWindowW(app::CLASS, None) }) {
            let text: Vec<u16> = arg.encode_utf16().collect();
            let cds = COPYDATASTRUCT {
                dwData: app::COPYDATA_COMMAND,
                cbData: (text.len() * 2) as u32,
                lpData: text.as_ptr() as *mut _,
            };
            unsafe {
                SendMessageW(hwnd, WM_COPYDATA, WPARAM(0), LPARAM(&cds as *const _ as isize));
            }
        }
        return;
    }

    showlog::note(format!("launch: Show Mode {} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::ARCH));
    app::run(first);
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Show Mode for Windows only runs on Windows. Build it with: cargo xwin build --release --target x86_64-pc-windows-msvc");
}
