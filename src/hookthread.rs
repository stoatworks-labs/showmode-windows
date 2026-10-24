//! A thread of its own for a low-level hook (the Windows key guard and the cursor fence).
//! Windows calls a low-level hook on the thread that set it, through that thread's message
//! loop, and holds the input until it answers, so the hook thread does nothing else: on the app's
//! own thread, a display correction (which can take a second) would stall every key and every
//! mouse movement with it.
//!
//! The hook is set again every thirty seconds, the new one before the old one goes, and always on
//! this thread: in the spike, a hook re-set from any other thread stayed dead for good. Windows
//! is documented to drop a hook that keeps timing out without saying so (the spike's 1.5-second
//! stall didn't), and a hook set on the normal desktop never runs on the secure one (UAC,
//! Ctrl+Alt+Del, the lock screen).

use std::sync::mpsc;
use std::thread::JoinHandle;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, PeekMessageW, PostThreadMessageW, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx, HOOKPROC, MSG,
    PM_NOREMOVE, WINDOWS_HOOK_ID, WM_QUIT, WM_TIMER, WM_USER,
};

const REHOOK_MS: u32 = 30_000;

pub struct HookThread {
    id: u32,
    join: Option<JoinHandle<()>>,
}

impl HookThread {
    /// Sets the hook on a new thread. `poll`, if given, also runs there every so many
    /// milliseconds. None if Windows wouldn't set the hook.
    pub fn start(name: &str, kind: WINDOWS_HOOK_ID, proc: HOOKPROC, poll: Option<(u32, fn())>) -> Option<HookThread> {
        let (tx, rx) = mpsc::channel();
        let join = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || unsafe {
                let module: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
                let set = || SetWindowsHookExW(kind, proc, module, 0).ok();
                let Some(mut hook) = set() else {
                    let _ = tx.send(None);
                    return;
                };
                // A message queue before the thread's id goes out, so a quit posted at once lands.
                let mut msg = MSG::default();
                let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
                let _ = tx.send(Some(GetCurrentThreadId()));
                let rehook = SetTimer(None, 0, REHOOK_MS, None);
                let poll = poll.map(|(ms, f)| (SetTimer(None, 0, ms, None), f));
                // No windows here, so nothing to dispatch: thread timers arrive as WM_TIMER.
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message != WM_TIMER {
                        continue;
                    }
                    if msg.wParam.0 == rehook {
                        if let Some(new) = set() {
                            let _ = UnhookWindowsHookEx(hook);
                            hook = new;
                        }
                    } else if let Some((id, f)) = poll {
                        if msg.wParam.0 == id {
                            f();
                        }
                    }
                }
                let _ = UnhookWindowsHookEx(hook);
            })
            .ok()?;
        match rx.recv() {
            Ok(Some(id)) => Some(HookThread { id, join: Some(join) }),
            _ => {
                let _ = join.join();
                None
            }
        }
    }
}

impl Drop for HookThread {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}
