//! App crash dialogs. When an app crashes, Windows Error Reporting can put a "stopped working"
//! window over everything. DontShowUI (per user, documented with the WER settings) keeps that
//! window away; the crash report is still collected and queued as usual.

use crate::{journal, reg, showlog};
use serde_json::{json, Value};

const KEY: &str = r"Software\Microsoft\Windows\Windows Error Reporting";
const NAME: &str = "DontShowUI";

pub fn engage() -> Option<String> {
    let before = reg::get_dword(KEY, NAME);
    if before == Some(1) {
        return None;
    }
    // Journalled as the number it was, or null for "not set".
    journal::record("crashDialogs", json!({ "previous": before }));
    if !reg::set_dword(KEY, NAME, 1) {
        return Some("Crash dialogs may still appear: the setting could not be written".into());
    }
    showlog::note(format!("crash dialogs: DontShowUI=1 (was {before:?})"));
    None
}

pub fn restore() -> bool {
    let Some(entry) = journal::get("crashDialogs") else { return true };
    let previous = match &entry["previous"] {
        Value::Number(n) => n.as_u64().map(|v| v as u32),
        _ => None,
    };
    // Someone else's change mid-show stands.
    if reg::get_dword(KEY, NAME) == Some(1) && !reg::restore_dword(KEY, NAME, previous) {
        return false;
    }
    showlog::note(format!("crash dialogs: DontShowUI put back to {previous:?}"));
    journal::remove("crashDialogs");
    true
}
