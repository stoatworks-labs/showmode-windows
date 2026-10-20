//! HKEY_CURRENT_USER values: read, write and delete, the way the guards journal them. A value
//! that is not there reads as None, and restoring None deletes it again.

use crate::util::wide;
use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_DWORD,
    RRF_RT_REG_SZ,
};

pub fn get_dword(key: &str, name: &str) -> Option<u32> {
    let (k, n) = (wide(key), wide(name));
    let mut v = 0u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            PCWSTR(n.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as _),
            Some(&mut size),
        )
    };
    (r == ERROR_SUCCESS).then_some(v)
}

pub fn set_dword(key: &str, name: &str, value: u32) -> bool {
    let (k, n) = (wide(key), wide(name));
    let r = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            PCWSTR(n.as_ptr()),
            REG_DWORD.0,
            Some(&value as *const u32 as _),
            4,
        )
    };
    r == ERROR_SUCCESS
}

pub fn get_string(key: &str, name: &str) -> Option<String> {
    let (k, n) = (wide(key), wide(name));
    let mut size = 0u32;
    unsafe {
        let r = RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut size));
        if r != ERROR_SUCCESS || size == 0 {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            PCWSTR(n.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as _),
            Some(&mut size),
        );
        if r != ERROR_SUCCESS {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

/// An empty `name` sets the key's default value.
pub fn set_string(key: &str, name: &str, value: &str) -> bool {
    let (k, n, v) = (wide(key), wide(name), wide(value));
    let r = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            if name.is_empty() { PCWSTR::null() } else { PCWSTR(n.as_ptr()) },
            REG_SZ.0,
            Some(v.as_ptr() as _),
            (v.len() * 2) as u32,
        )
    };
    r == ERROR_SUCCESS
}

/// Deletes a value; one that was never there counts as deleted.
pub fn delete(key: &str, name: &str) -> bool {
    let (k, n) = (wide(key), wide(name));
    let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr())) };
    r == ERROR_SUCCESS || r.0 == 2 // ERROR_FILE_NOT_FOUND
}

/// Puts a DWORD back to what the journal recorded: a number, or absent.
pub fn restore_dword(key: &str, name: &str, previous: Option<u32>) -> bool {
    match previous {
        Some(v) => set_dword(key, name, v),
        None => delete(key, name),
    }
}
