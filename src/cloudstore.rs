//! Reading Night light's state out of the shell's CloudStore blobs. Reading only: these blobs
//! are undocumented, and a mis-written one fails silently by breaking Settings (nightlight-cli
//! was withdrawn in May 2026 after exactly that), so Show Mode never writes them.
//!
//! A blob is "CB" 01 00, a few fields (one a timestamp), then a nested "CB" 01 00 record whose
//! length byte sits just before it. Each is checked for that shape before anything is read from
//! it, and anything else reads as unknown rather than as a guess.
//!
//! - The state record starts 10 00 when Night light is on and goes straight to D0 0A when it's
//!   off; its length is 0x15 or 0x13 to match (the byte nightlight-cli read at offset 18).
//! - The settings record starts 02 01 when the schedule is on.

const MAGIC: [u8; 4] = [0x43, 0x42, 0x01, 0x00];

/// The nested record's fields, after its own header. None unless the blob has the shape above.
fn record(blob: &[u8]) -> Option<&[u8]> {
    if !blob.starts_with(&MAGIC) {
        return None;
    }
    let at = blob[MAGIC.len()..].windows(MAGIC.len()).position(|w| w == MAGIC)? + MAGIC.len();
    let len = *blob.get(at.checked_sub(1)?)? as usize;
    let nested = blob.get(at..at + len)?;
    (nested.len() > MAGIC.len()).then(|| &nested[MAGIC.len()..])
}

/// Whether Night light is on now.
pub fn night_light_on(state: &[u8]) -> Option<bool> {
    let r = record(state)?;
    if r.starts_with(&[0x10, 0x00]) {
        Some(true)
    } else if r.starts_with(&[0xd0, 0x0a]) {
        Some(false)
    } else {
        None
    }
}

/// Whether Night light switches itself on to a schedule.
pub fn night_light_scheduled(settings: &[u8]) -> Option<bool> {
    record(settings).map(|r| r.starts_with(&[0x02, 0x01]))
}

pub fn hex(blob: &[u8]) -> String {
    blob.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFF: [u8; 41] = [
        0x43, 0x42, 0x01, 0x00, 0x0a, 0x02, 0x01, 0x00, 0x2a, 0x06, 0xf0, 0xe4, 0xcb, 0xf5, 0x05, 0x2a, 0x2b, 0x0e, 0x13,
        0x43, 0x42, 0x01, 0x00, 0xd0, 0x0a, 0x02, 0xc6, 0x14, 0xa0, 0xc1, 0xd6, 0xef, 0xb8, 0xab, 0xd0, 0xeb, 0x01, 0x00,
        0x00, 0x00, 0x00,
    ];

    fn on() -> Vec<u8> {
        let mut v = OFF.to_vec();
        v[18] = 0x15;
        v.splice(23..23, [0x10, 0x00]);
        v
    }

    #[test]
    fn reads_the_state() {
        assert_eq!(night_light_on(&OFF), Some(false));
        assert_eq!(night_light_on(&on()), Some(true));
        assert_eq!(on()[18], 0x15, "the byte nightlight-cli read");
    }

    #[test]
    fn anything_else_is_unknown() {
        assert_eq!(night_light_on(&[]), None);
        assert_eq!(night_light_on(&OFF[..20]), None, "cut short");
        let mut odd = OFF.to_vec();
        odd[23] = 0x99;
        assert_eq!(night_light_on(&odd), None, "an unfamiliar first field");
        let mut long = OFF.to_vec();
        long[18] = 0x7f;
        assert_eq!(night_light_on(&long), None, "a length past the end");
    }

    #[test]
    fn reads_the_schedule() {
        let mut s = OFF.to_vec();
        assert_eq!(night_light_scheduled(&s), Some(false));
        s[18] = 0x15;
        s.splice(23..23, [0x02, 0x01]);
        assert_eq!(night_light_scheduled(&s), Some(true));
    }
}
