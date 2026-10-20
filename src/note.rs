//! The short note Show Mode shows by the notification area for a few seconds: a guard that
//! couldn't apply, a restore that didn't finish. It isn't a Windows notification, since show
//! mode may just have switched those off, and it never takes the focus. It appears on the
//! primary display (the one with the taskbar), which the operator keeps, not the projector.

use crate::util::wide;
use std::cell::RefCell;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

const CLASS: PCWSTR = w!("StoatworksShowModeNote");
const TIMER_HIDE: usize = 1;
const SHOW_MS: u32 = 7000;

thread_local! {
    static NOTE: RefCell<(HWND, String)> = const { RefCell::new((HWND(std::ptr::null_mut()), String::new())) };
}

pub fn register(instance: HINSTANCE) {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!("Show Mode"),
            WS_POPUP | WS_BORDER,
            0,
            0,
            10,
            10,
            None,
            None,
            instance,
            None,
        )
        .unwrap_or_default();
        NOTE.with(|n| n.borrow_mut().0 = hwnd);
    }
}

fn font(dpi: u32) -> HFONT {
    unsafe {
        CreateFontW(
            -(10 * dpi as i32 / 72),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            (DEFAULT_PITCH.0 | FF_SWISS.0) as u32,
            w!("Segoe UI"),
        )
    }
}

/// Shows `text` for a few seconds, bottom right of the primary display's work area.
pub fn show(text: &str) {
    show_for(text, SHOW_MS);
}

/// Shows `text` for `ms` milliseconds (or until clicked).
pub fn show_for(text: &str, ms: u32) {
    let hwnd = NOTE.with(|n| {
        let mut n = n.borrow_mut();
        n.1 = text.to_string();
        n.0
    });
    if hwnd.0.is_null() {
        return;
    }
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96);
        let pad = 12 * dpi as i32 / 96;
        let width = 360 * dpi as i32 / 96;
        // Measure the text at that width.
        let hdc = GetDC(hwnd);
        let f = font(dpi);
        let old = SelectObject(hdc, f);
        let mut buf = wide(text);
        buf.pop();
        let mut r = RECT { left: 0, top: 0, right: width - 2 * pad, bottom: 0 };
        DrawTextW(hdc, &mut buf, &mut r, DT_WORDBREAK | DT_NOPREFIX | DT_CALCRECT);
        SelectObject(hdc, old);
        let _ = DeleteObject(f);
        ReleaseDC(hwnd, hdc);
        let height = r.bottom + 2 * pad;

        let mut work = RECT::default();
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut work as *mut RECT as _), Default::default());
        let margin = 16 * dpi as i32 / 96;
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            work.right - width - margin,
            work.bottom - height - margin,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = InvalidateRect(hwnd, None, true);
        SetTimer(hwnd, TIMER_HIDE, ms, None);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let bg = CreateSolidBrush(COLORREF(0x002d1b0f)); // Stoatworks navy, as BGR
            FillRect(hdc, &rc, bg);
            let _ = DeleteObject(bg);
            let dpi = GetDpiForWindow(hwnd).max(96);
            let pad = 12 * dpi as i32 / 96;
            let f = font(dpi);
            let old = SelectObject(hdc, f);
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(0x00f4f5f5));
            let mut buf = wide(&NOTE.with(|n| n.borrow().1.clone()));
            buf.pop();
            let mut r = RECT { left: pad, top: pad, right: rc.right - pad, bottom: rc.bottom - pad };
            DrawTextW(hdc, &mut buf, &mut r, DT_WORDBREAK | DT_NOPREFIX);
            SelectObject(hdc, old);
            let _ = DeleteObject(f);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_HIDE => {
            let _ = KillTimer(hwnd, TIMER_HIDE);
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        // A click dismisses it.
        WM_LBUTTONUP => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
