//! The audio watchdog: a second copy of ShowMode.exe, started while apps are muted, that
//! holds a handle to Show Mode and unmutes what the journal says was muted the moment Show
//! Mode dies. Windows keeps a per-app mute after the process that set it has gone, so without
//! this a crash mid-show would leave other apps silent until the next launch. A process handle,
//! not a pid, is what it waits on: a handle can't come to mean another process.
//!
//! It does nothing else. The rest of a crashed show waits for the next launch, as on macOS.

use crate::{audio, journal, showlog};
use std::process::{Child, Command};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE};

pub struct Watchdog(Child);

impl Watchdog {
    pub fn spawn() -> Option<Watchdog> {
        let exe = std::env::current_exe().ok()?;
        let child = Command::new(exe).args(["--watchdog", &std::process::id().to_string()]).spawn().ok()?;
        showlog::note(format!("audio: watchdog {} started", child.id()));
        Some(Watchdog(child))
    }
}

impl Drop for Watchdog {
    /// Ending the show stops the watchdog; it has nothing left to guard.
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `ShowMode.exe --watchdog <pid>`: waits for that process to end, then unmutes.
pub fn run(pid: u32) {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else {
            showlog::note(format!("watchdog: cannot open process {pid}"));
            return;
        };
        let r = WaitForSingleObject(h, INFINITE);
        let _ = CloseHandle(h);
        if r != WAIT_OBJECT_0 {
            return;
        }
    }
    journal::load();
    if journal::contains("audioMuted") {
        showlog::note(format!("watchdog: Show Mode ({pid}) ended mid-show; unmuting apps"));
        audio::restore();
    }
}
