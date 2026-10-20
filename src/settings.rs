//! Which guards engage with show mode, and which apps are allowed to play. Kept in
//! %LOCALAPPDATA%\ShowMode\settings.json; a change takes effect at the next start.

use crate::paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Guard {
    Sleep,
    ShowPlan,
    ScreenSaver,
    Notifications,
    Snap,
    CrashDialogs,
    Wallpaper,
    MuteOtherApps,
}

impl Guard {
    pub const ALL: [Guard; 8] = [
        Guard::Sleep,
        Guard::ShowPlan,
        Guard::ScreenSaver,
        Guard::Notifications,
        Guard::Snap,
        Guard::CrashDialogs,
        Guard::Wallpaper,
        Guard::MuteOtherApps,
    ];

    /// The key in settings.json, never shown.
    pub fn id(self) -> &'static str {
        match self {
            Guard::Sleep => "sleep",
            Guard::ShowPlan => "showPlan",
            Guard::ScreenSaver => "screenSaver",
            Guard::Notifications => "notifications",
            Guard::Snap => "snap",
            Guard::CrashDialogs => "crashDialogs",
            Guard::Wallpaper => "wallpaper",
            Guard::MuteOtherApps => "muteOtherApps",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Guard::Sleep => "Prevent display & system sleep",
            Guard::ShowPlan => "Show power plan: lid does nothing, no timeouts",
            Guard::ScreenSaver => "Turn off the screen saver",
            Guard::Notifications => "Do Not Disturb (notifications)",
            Guard::Snap => "Turn off Snap (edges, layouts, drag to maximise)",
            Guard::CrashDialogs => "Hide app crash dialogs (reports still saved)",
            Guard::Wallpaper => "Black out desktop wallpaper",
            Guard::MuteOtherApps => "Mute apps not allowed to play",
        }
    }

    /// Every guard defaults on except muting, which needs an allow-list before it is useful.
    pub fn default_on(self) -> bool {
        !matches!(self, Guard::MuteOtherApps)
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    #[serde(default)]
    guards: BTreeMap<String, bool>,
    /// Executable names (vmix64.exe), matched without regard to case.
    #[serde(default)]
    allowed_audio_apps: Vec<String>,
}

static STORED: Mutex<Option<Stored>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Stored) -> T) -> T {
    let mut guard = STORED.lock().unwrap();
    if guard.is_none() {
        let path = paths::data_dir().join("settings.json");
        *guard = Some(
            std::fs::read(path)
                .ok()
                .and_then(|d| serde_json::from_slice(&d).ok())
                .unwrap_or_default(),
        );
    }
    f(guard.as_mut().unwrap())
}

fn save(s: &Stored) {
    let path = paths::data_dir().join("settings.json");
    if let Ok(data) = serde_json::to_vec_pretty(s) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

pub fn is_on(g: Guard) -> bool {
    with(|s| s.guards.get(g.id()).copied().unwrap_or_else(|| g.default_on()))
}

pub fn set_on(g: Guard, on: bool) {
    with(|s| {
        s.guards.insert(g.id().to_string(), on);
        save(s);
    })
}

pub fn allowed_audio_apps() -> Vec<String> {
    with(|s| s.allowed_audio_apps.clone())
}

pub fn is_allowed_audio_app(exe: &str) -> bool {
    with(|s| s.allowed_audio_apps.iter().any(|a| a.eq_ignore_ascii_case(exe)))
}

pub fn toggle_allowed_audio_app(exe: &str) {
    with(|s| {
        if let Some(i) = s.allowed_audio_apps.iter().position(|a| a.eq_ignore_ascii_case(exe)) {
            s.allowed_audio_apps.remove(i);
        } else {
            s.allowed_audio_apps.push(exe.to_string());
            s.allowed_audio_apps.sort_by_key(|a| a.to_ascii_lowercase());
        }
        save(s);
    })
}
