//! System sounds: the error ding, a device plugging in or out, the Recycle Bin, Windows' own
//! alarms. Switched to "No Sounds" the way the Sound control panel does it: every event's
//! .Current sound emptied and the scheme set to .None. Windows looks an event's sound up each
//! time it plays one, so it takes effect at once.
//!
//! The journal keeps each event's own sound (as stored, unexpanded) before any is emptied, and
//! restore puts back only the ones still empty, so a sound someone set mid-show stands. Engaging
//! over a restore that never finished adds to that entry and never overwrites it.
//!
//! It doesn't touch the output's volume or mute, or any app's sound: a show playing through the
//! same output plays as normal.

use crate::{journal, reg, showlog};
use serde_json::{json, Map};

const SCHEMES: &str = r"AppEvents\Schemes";
const APPS: &str = r"AppEvents\Schemes\Apps";
const NO_SOUNDS: &str = ".None";

/// Every event's .Current key, as "<app>\<event>" under Apps.
fn events() -> Vec<String> {
    let mut out = Vec::new();
    for app in reg::subkeys(APPS) {
        for event in reg::subkeys(&format!(r"{APPS}\{app}")) {
            out.push(format!(r"{app}\{event}"));
        }
    }
    out
}

fn current_key(event: &str) -> String {
    format!(r"{APPS}\{event}\.Current")
}

/// The events that make a sound now, with what they play.
fn sounding() -> Vec<(String, String, bool)> {
    events()
        .into_iter()
        .filter_map(|e| {
            let (sound, expand) = reg::get_raw_string(&current_key(&e), "")?;
            (!sound.is_empty()).then_some((e, sound, expand))
        })
        .collect()
}

pub fn engage() -> Option<String> {
    let found = sounding();
    let scheme = reg::get_string(SCHEMES, "");
    let mut entry = journal::get("systemSounds").unwrap_or_else(|| json!({ "scheme": scheme, "events": {} }));
    let mut added = 0;
    if let Some(events) = entry["events"].as_object_mut() {
        for (e, sound, expand) in &found {
            if !events.contains_key(e) {
                events.insert(e.clone(), json!({ "sound": sound, "expand": expand }));
                added += 1;
            }
        }
    }
    if found.is_empty() && !journal::contains("systemSounds") {
        return None; // no sounds already
    }
    journal::set("systemSounds", entry);
    let failed = found.iter().filter(|(e, _, _)| !reg::set_raw_string(&current_key(e), "", "", false)).count();
    reg::set_string(SCHEMES, "", NO_SOUNDS);
    showlog::note(format!("system sounds: {} silenced ({added} newly journalled), scheme was {scheme:?}", found.len() - failed));
    (failed > 0).then(|| format!("{failed} system sounds could not be switched off"))
}

pub fn restore() -> bool {
    let Some(entry) = journal::get("systemSounds") else { return true };
    let mut left = Map::new();
    let mut back = 0;
    for (e, v) in entry["events"].as_object().cloned().unwrap_or_default() {
        let sound = v["sound"].as_str().unwrap_or_default();
        let expand = v["expand"].as_bool().unwrap_or(false);
        let key = current_key(&e);
        // Only where it's still silent: a sound chosen mid-show stands.
        let silent = reg::get_raw_string(&key, "").is_none_or(|(s, _)| s.is_empty());
        if !silent {
            continue;
        }
        if reg::set_raw_string(&key, "", sound, expand) && reg::get_raw_string(&key, "").is_some_and(|(s, _)| s == sound) {
            back += 1;
        } else {
            left.insert(e, v);
        }
    }
    if !left.is_empty() {
        showlog::note(format!("system sounds: {back} put back, {} not", left.len()));
        let scheme = entry["scheme"].clone();
        journal::set("systemSounds", json!({ "scheme": scheme, "events": left }));
        return false;
    }
    // The scheme's name goes back too, unless someone chose another mid-show.
    if reg::get_string(SCHEMES, "").as_deref() == Some(NO_SOUNDS) {
        match entry["scheme"].as_str() {
            Some(s) => {
                reg::set_string(SCHEMES, "", s);
            }
            None => {
                reg::delete(SCHEMES, "");
            }
        }
    }
    showlog::note(format!("system sounds: {back} put back, scheme {:?}", reg::get_string(SCHEMES, "")));
    journal::remove("systemSounds");
    true
}

/// For --status.
pub fn describe() -> String {
    let n = sounding().len();
    format!("system sounds: scheme {:?}, {n} of {} events with a sound", reg::get_string(SCHEMES, ""), events().len())
}
