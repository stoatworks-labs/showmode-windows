//! Everything show mode changed, with the value it replaced, written to disk *before* each
//! change is made. If the app crashes or is killed mid-show, the next launch finds the journal
//! and puts the machine back. The audio watchdog reads the same file when the app dies.

use crate::paths;
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;

static ENTRIES: Mutex<Option<Map<String, Value>>> = Mutex::new(None);

fn path() -> PathBuf {
    paths::data_dir().join("journal.json")
}

pub fn exists() -> bool {
    path().exists()
}

/// Reads the journal from disk, replacing whatever is held in memory.
pub fn load() {
    let entries = std::fs::read(path())
        .ok()
        .and_then(|d| serde_json::from_slice::<Map<String, Value>>(&d).ok())
        .unwrap_or_default();
    *ENTRIES.lock().unwrap() = Some(entries);
}

fn with<T>(f: impl FnOnce(&mut Map<String, Value>) -> T) -> T {
    let mut guard = ENTRIES.lock().unwrap();
    if guard.is_none() {
        drop(guard);
        load();
        guard = ENTRIES.lock().unwrap();
    }
    f(guard.as_mut().unwrap())
}

pub fn get(key: &str) -> Option<Value> {
    with(|e| e.get(key).cloned())
}

pub fn contains(key: &str) -> bool {
    with(|e| e.contains_key(key))
}

/// Sets an entry and saves straight away.
pub fn set(key: &str, value: Value) {
    with(|e| {
        e.insert(key.to_string(), value);
        save(e);
    });
}

/// Records a previous value once: a later record of the same key keeps the original, so
/// engaging twice (or over a restore that never finished) never journals show mode's own value
/// as "previous". Returns whether this call recorded it.
pub fn record(key: &str, previous: Value) -> bool {
    with(|e| {
        if e.contains_key(key) {
            return false;
        }
        e.insert(key.to_string(), previous);
        save(e);
        true
    })
}

pub fn remove(key: &str) {
    with(|e| {
        if e.remove(key).is_some() {
            save(e);
        }
    });
}

pub fn keys() -> Vec<String> {
    with(|e| e.keys().cloned().collect())
}

/// After a restore: keeps only the entries whose undo needs another try, so Finish Restoring
/// and the next launch retry them. With none, removes the journal.
pub fn retain(keep: &[String]) {
    with(|e| {
        e.retain(|k, _| keep.contains(k));
        save(e);
    });
}

fn save(entries: &Map<String, Value>) {
    let p = path();
    if entries.is_empty() {
        let _ = std::fs::remove_file(&p);
        return;
    }
    let Ok(data) = serde_json::to_vec_pretty(entries) else { return };
    // Written beside and renamed over, so a crash mid-write never leaves half a journal.
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, data).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn records_once_and_retains() {
        let dir = std::env::temp_dir().join(format!("showmode-journal-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("SHOWMODE_DATA_DIR", &dir);
        load();
        assert!(!exists());
        assert!(record("screenSaver", json!(true)));
        assert!(!record("screenSaver", json!(false)), "a second record keeps the first");
        assert_eq!(get("screenSaver"), Some(json!(true)));
        set("dnd", json!("Unrestricted"));
        assert!(exists());
        load();
        assert_eq!(get("dnd"), Some(json!("Unrestricted")), "survives a reload");
        retain(&["dnd".to_string()]);
        assert!(!contains("screenSaver"));
        retain(&[]);
        assert!(!exists(), "an empty journal is removed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
