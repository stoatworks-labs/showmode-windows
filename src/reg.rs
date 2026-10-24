//! HKEY_CURRENT_USER values: read, write and delete, the way the guards journal them. A value
//! that is not there reads as None, and restoring None deletes it again. HKEY_LOCAL_MACHINE is
//! only ever read (the pre-show checks).

use crate::util::wide;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteKeyValueW, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW, HKEY,
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE, RRF_NOEXPAND,
    RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
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

pub fn get_binary(key: &str, name: &str) -> Option<Vec<u8>> {
    let (k, n) = (wide(key), wide(name));
    let mut size = 0u32;
    unsafe {
        let r = RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_BINARY, None, None, Some(&mut size));
        if r != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            PCWSTR(n.as_ptr()),
            RRF_RT_REG_BINARY,
            None,
            Some(buf.as_mut_ptr() as _),
            Some(&mut size),
        );
        if r != ERROR_SUCCESS {
            return None;
        }
        buf.truncate(size as usize);
        Some(buf)
    }
}

/// Whether a key under HKEY_LOCAL_MACHINE is there. Read only.
pub fn machine_key_exists(key: &str) -> bool {
    let k = wide(key);
    let mut h = HKEY::default();
    let r = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, PCWSTR(k.as_ptr()), 0, KEY_READ, &mut h) };
    if r == ERROR_SUCCESS {
        unsafe {
            let _ = RegCloseKey(h);
        }
    }
    r == ERROR_SUCCESS
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

/// The names of a key's subkeys; none if the key isn't there.
pub fn subkeys(key: &str) -> Vec<String> {
    let k = wide(key);
    let mut h = HKEY::default();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), 0, KEY_READ, &mut h) } != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0.. {
        let mut buf = [0u16; 256]; // the longest key name the registry allows, and its NUL
        let mut n = buf.len() as u32;
        let r = unsafe { RegEnumKeyExW(h, i, PWSTR(buf.as_mut_ptr()), &mut n, None, PWSTR::null(), None, None) };
        if r != ERROR_SUCCESS {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..n as usize]));
    }
    unsafe {
        let _ = RegCloseKey(h);
    }
    out
}

/// A string value as stored, unexpanded, and whether it is REG_EXPAND_SZ, so it can be written
/// back exactly. An empty `name` reads the key's default value.
pub fn get_raw_string(key: &str, name: &str) -> Option<(String, bool)> {
    let (k, n) = (wide(key), wide(name));
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let mut kind = REG_VALUE_TYPE(0);
    let mut size = 0u32;
    unsafe {
        let r = RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), flags, Some(&mut kind), None, Some(&mut size));
        if r != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2).max(1)];
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            PCWSTR(n.as_ptr()),
            flags,
            Some(&mut kind),
            Some(buf.as_mut_ptr() as _),
            Some(&mut size),
        );
        if r != ERROR_SUCCESS {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some((String::from_utf16_lossy(&buf[..end]), kind == REG_EXPAND_SZ))
    }
}

/// Writes a string value as REG_SZ or REG_EXPAND_SZ. An empty `name` sets the default value.
pub fn set_raw_string(key: &str, name: &str, value: &str, expand: bool) -> bool {
    let (k, n, v) = (wide(key), wide(name), wide(value));
    let r = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            if name.is_empty() { PCWSTR::null() } else { PCWSTR(n.as_ptr()) },
            if expand { REG_EXPAND_SZ.0 } else { REG_SZ.0 },
            Some(v.as_ptr() as _),
            (v.len() * 2) as u32,
        )
    };
    r == ERROR_SUCCESS
}
