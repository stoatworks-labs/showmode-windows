//! A plain activity log at %LOCALAPPDATA%\ShowMode\ShowMode.log, so an operator can see what
//! show mode did to the machine and when. Open Log in the menu opens it in Notepad.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn note(message: impl AsRef<str>) {
    let line = format!("{} {}\n", stamp(SystemTime::now()), message.as_ref());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(crate::paths::log_file()) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// UTC, ISO 8601 with milliseconds: 2026-10-20T09:15:02.123Z.
pub fn stamp(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, day) = civil(days);
    format!(
        "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_millis()
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's civil_from_days).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn stamps() {
        assert_eq!(stamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        // 2026-10-20T09:15:02.123Z
        let t = UNIX_EPOCH + Duration::from_millis(1_792_487_702_123);
        assert_eq!(stamp(t), "2026-10-20T09:15:02.123Z");
        // A leap day.
        let t = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(stamp(t), "2000-02-29T00:00:00.000Z");
    }
}
