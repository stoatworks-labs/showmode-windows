//! Which guards engage with show mode, which apps are allowed to play and which screens the
//! cursor is kept off. Kept in %LOCALAPPDATA%\ShowMode\settings.json; a guard switched on or off
//! takes effect at the next start.

use crate::paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Guard {
    Sleep,
    ShowPlan,
    EnergySaver,
    ScreenSaver,
    Notifications,
    SystemSounds,
    CrashDialogs,
    AutoPlay,
    Snap,
    ShowDesktop,
    WindowsKey,
    AccessibilityKeys,
    CursorFence,
    Wallpaper,
    DisplayLayout,
    ColourApps,
    MuteOtherApps,
    Syncthing,
    SyncApps,
}

impl Guard {
    /// In menu order: each section starts at a guard `starts_section` names.
    pub const ALL: [Guard; 19] = [
        Guard::Sleep,
        Guard::ShowPlan,
        Guard::EnergySaver,
        Guard::ScreenSaver,
        Guard::Notifications,
        Guard::SystemSounds,
        Guard::CrashDialogs,
        Guard::AutoPlay,
        Guard::Snap,
        Guard::ShowDesktop,
        Guard::WindowsKey,
        Guard::AccessibilityKeys,
        Guard::CursorFence,
        Guard::Wallpaper,
        Guard::DisplayLayout,
        Guard::ColourApps,
        Guard::MuteOtherApps,
        Guard::Syncthing,
        Guard::SyncApps,
    ];

    /// The key in settings.json, never shown.
    pub fn id(self) -> &'static str {
        match self {
            Guard::Sleep => "sleep",
            Guard::ShowPlan => "showPlan",
            Guard::EnergySaver => "energySaver",
            Guard::ScreenSaver => "screenSaver",
            Guard::Notifications => "notifications",
            Guard::SystemSounds => "systemSounds",
            Guard::CrashDialogs => "crashDialogs",
            Guard::AutoPlay => "autoPlay",
            Guard::Snap => "snap",
            Guard::ShowDesktop => "showDesktop",
            Guard::WindowsKey => "windowsKey",
            Guard::AccessibilityKeys => "accessibilityKeys",
            Guard::CursorFence => "cursorFence",
            Guard::Wallpaper => "wallpaper",
            Guard::DisplayLayout => "displayLayout",
            Guard::ColourApps => "colourApps",
            Guard::MuteOtherApps => "muteOtherApps",
            Guard::Syncthing => "syncthing",
            Guard::SyncApps => "syncApps",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Guard::Sleep => "Prevent display & system sleep",
            Guard::ShowPlan => "Show power plan: lid does nothing, no timeouts",
            Guard::EnergySaver => "Turn off Energy saver (in the show power plan)",
            Guard::ScreenSaver => "Turn off the screen saver",
            Guard::Notifications => "Do Not Disturb (notifications)",
            Guard::SystemSounds => "Turn off system sounds (error dings, devices plugging in)",
            Guard::CrashDialogs => "Hide app crash dialogs (reports still saved)",
            Guard::AutoPlay => "Turn off AutoPlay (the box when a drive is plugged in)",
            Guard::Snap => "Turn off Snap (edges, layouts, drag to maximise)",
            Guard::ShowDesktop => "Turn off the taskbar's Show Desktop corner & window shake",
            Guard::WindowsKey => "Block the Windows key, its shortcuts & Print Screen",
            Guard::AccessibilityKeys => "Turn off accessibility shortcuts (Shift five times…)",
            Guard::CursorFence => "Fence cursor off show screens (choose them under Keep Cursor Off)",
            Guard::Wallpaper => "Black out desktop wallpaper",
            Guard::DisplayLayout => "Lock main display & keep new screens extended",
            Guard::ColourApps => "Quit f.lux & LightBulb, and reopen them after",
            Guard::MuteOtherApps => "Mute apps not allowed to play",
            Guard::Syncthing => "Pause Syncthing",
            Guard::SyncApps => "Quit sync apps (OneDrive, Dropbox, Google Drive, Box, MEGA, Resilio)",
        }
    }

    /// Where the menu puts a separator: power, interruptions, keys and mouse, screens, sound,
    /// background apps.
    pub fn starts_section(self) -> bool {
        matches!(self, Guard::Notifications | Guard::Snap | Guard::Wallpaper | Guard::MuteOtherApps | Guard::Syncthing)
    }

    /// Every guard defaults on except muting, which needs an allow-list before it is useful, and
    /// quitting sync apps, which would cut a show off from media kept only in the cloud.
    pub fn default_on(self) -> bool {
        !matches!(self, Guard::MuteOtherApps | Guard::SyncApps)
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
    /// Screens the cursor fence keeps the cursor off, by device path.
    #[serde(default)]
    fenced_screens: Vec<String>,
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

pub fn fenced_screens() -> Vec<String> {
    with(|s| s.fenced_screens.clone())
}

pub fn is_fenced_screen(key: &str) -> bool {
    with(|s| s.fenced_screens.iter().any(|k| k == key))
}

pub fn toggle_fenced_screen(key: &str) {
    with(|s| {
        if let Some(i) = s.fenced_screens.iter().position(|k| k == key) {
            s.fenced_screens.remove(i);
        } else {
            s.fenced_screens.push(key.to_string());
        }
        save(s);
    })
}
