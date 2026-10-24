//! The cursor fence: keeps the mouse off the screens the audience sees, which are ticked under
//! Keep Cursor Off and remembered by device path (the screen and the socket it's in).
//!
//! A low-level mouse hook, on a thread of its own (hookthread.rs), swallows any movement that
//! would land on a fenced screen and puts the cursor at the nearest point it may use instead,
//! so it stops dead at the edge. Unlike macOS this needs no permission. A poll every 10 ms on the
//! same thread backs it up, for whatever moves the cursor without a movement the hook sees: a
//! program setting the position, or input meant for an elevated window, which a medium process's
//! hook isn't called for. Nothing is written anywhere, so a crash leaves nothing to put back.
//!
//! It never traps the cursor (fence.rs): with every screen ticked, or the only unticked one
//! unplugged, it stands down until that changes. The secure desktop (UAC, Ctrl+Alt+Del, the lock
//! screen) frees the cursor whatever any hook does; the fence takes hold again on the way back.
//! Ctrl+Alt+Shift+F10 pauses it, to drag a window onto a show screen on purpose.

use crate::fence::{Fence, Rect, Screen};
use crate::hookthread::HookThread;
use crate::{display, settings, showlog};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

static FENCE: Mutex<Option<Fence>> = Mutex::new(None);
static PAUSED: AtomicBool = AtomicBool::new(false);

/// Where to put the cursor instead, if `p` is on a fenced screen.
fn correction(p: (i32, i32)) -> Option<(i32, i32)> {
    if PAUSED.load(Ordering::Relaxed) {
        return None;
    }
    let f = FENCE.lock().ok()?;
    let f = f.as_ref()?;
    f.blocks(p).then(|| f.nearest_allowed(p))
}

unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wp.0 as u32 == WM_MOUSEMOVE {
        let m = &*(lp.0 as *const MSLLHOOKSTRUCT);
        if let Some((x, y)) = correction((m.pt.x, m.pt.y)) {
            // SetCursorPos raises no hook event of its own (the spike), so this can't loop.
            let _ = SetCursorPos(x, y);
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

fn poll() {
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&mut p) }.is_err() {
        return; // the secure desktop is up
    }
    if let Some((x, y)) = correction((p.x, p.y)) {
        unsafe {
            let _ = SetCursorPos(x, y);
        }
    }
}

/// Works out the fence again from the screens connected now and the ones ticked. Returns
/// whether it changed, and whether it's fencing anything now.
fn recompute() -> (bool, bool) {
    let fenced = settings::fenced_screens();
    let screens: Vec<Screen> = display::screens()
        .into_iter()
        .map(|s| Screen { rect: Rect::at(s.origin, s.size), fenced: fenced.contains(&s.key) })
        .collect();
    let f = Fence::new(&screens);
    let on = f.is_some();
    let mut held = FENCE.lock().unwrap();
    let changed = *held != f;
    *held = f;
    (changed, on)
}

/// The fence while a show runs. Dropping it frees the cursor.
pub struct CursorFence {
    #[allow(dead_code)]
    thread: HookThread,
}

impl CursorFence {
    pub fn start() -> Result<CursorFence, String> {
        PAUSED.store(false, Ordering::Relaxed);
        let (_, on) = recompute();
        let Some(thread) = HookThread::start("cursor-fence", WH_MOUSE_LL, Some(hook), Some((10, poll))) else {
            *FENCE.lock().unwrap() = None;
            return Err("The cursor isn't fenced: Windows wouldn't set the mouse hook".into());
        };
        showlog::note(format!(
            "fence: on, {}",
            if on { "keeping the cursor off the ticked screens" } else { "standing by: no ticked screen with an unticked one to go to" }
        ));
        Ok(CursorFence { thread })
    }

    pub fn paused(&self) -> bool {
        PAUSED.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, paused: bool) {
        PAUSED.store(paused, Ordering::Relaxed);
        showlog::note(if paused { "fence: paused" } else { "fence: resumed" });
    }

    /// After a display change or a screen ticked: works the fence out again.
    pub fn screens_changed(&self) {
        let (changed, on) = recompute();
        if changed {
            showlog::note(format!("fence: screens changed, {}", if on { "fencing" } else { "standing by" }));
        }
    }
}

impl Drop for CursorFence {
    fn drop(&mut self) {
        *FENCE.lock().unwrap() = None;
        PAUSED.store(false, Ordering::Relaxed);
        showlog::note("fence: off");
    }
}

/// For --status.
pub fn describe() -> String {
    let fenced = settings::fenced_screens();
    let screens: Vec<String> = display::screens()
        .into_iter()
        .map(|s| format!("{}{}", s.name, if fenced.contains(&s.key) { " (fenced)" } else { "" }))
        .collect();
    format!("cursor fence screens: {} ({} remembered as fenced)", screens.join(", "), fenced.len())
}
