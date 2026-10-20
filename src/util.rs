//! Small Win32 helpers shared by the guards.

use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// A NUL-terminated UTF-16 copy of `s`.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Calls `f` with `s` as a PCWSTR that lives for the call.
pub fn with_wide<T>(s: &str, f: impl FnOnce(PCWSTR) -> T) -> T {
    let w = wide(s);
    f(PCWSTR(w.as_ptr()))
}

/// Takes ownership of a COM-allocated string.
pub unsafe fn take_co_string(p: PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = p.to_string().ok();
    CoTaskMemFree(Some(p.0 as _));
    s
}

/// The full path of a process's executable, if this user may ask.
pub fn exe_path_of_pid(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let r = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

/// "C:\Program Files\vMix\vMix64.exe" -> "vMix64.exe".
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

pub fn guid_string(g: &GUID) -> String {
    format!("{g:?}").to_ascii_lowercase()
}

pub fn parse_guid(s: &str) -> Option<GUID> {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 32 {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok().map(GUID::from_u128)
}

/// "a", "a and b", "a, b and c".
pub fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {}", rest.join(", "), last),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guids_round_trip() {
        let g = GUID::from_u128(0x381b4222_f694_41f0_9685_ff5bb260df2e);
        let s = guid_string(&g);
        assert_eq!(s, "381b4222-f694-41f0-9685-ff5bb260df2e");
        assert_eq!(parse_guid(&s), Some(g));
        assert_eq!(parse_guid("{381B4222-F694-41F0-9685-FF5BB260DF2E}"), Some(g));
    }

    #[test]
    fn lists() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(list(&v(&["a"])), "a");
        assert_eq!(list(&v(&["a", "b"])), "a and b");
        assert_eq!(list(&v(&["a", "b", "c"])), "a, b and c");
        assert_eq!(file_name(r"C:\Program Files\vMix\vMix64.exe"), "vMix64.exe");
    }
}
