//! Engages and restores every enabled guard. Each guard journals what it replaces before it
//! changes it, so restore() — including a restore on the next launch after a crash — puts back
//! exactly what was there, and only what show mode changed.

use crate::cursorfence::CursorFence;
use crate::keys::WindowsKey;
use crate::settings::{self, Guard};
use crate::watchdog::Watchdog;
use crate::{
    apps, audio, crashdialogs, display, dnd, explorer, journal, keys, power, preflight, showlog, sounds, spi, util, wallpaper,
};
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Controller {
    pub engaged: bool,
    keep_awake: Option<power::KeepAwake>,
    watchdog: Option<Watchdog>,
    display: Option<display::Lock>,
    windows_key: Option<WindowsKey>,
    pub fence: Option<CursorFence>,
    /// From the last engage: guards that couldn't apply. From a restore: what isn't back yet.
    pub warnings: Vec<String>,
}

/// Journal key -> what to call it when it couldn't be put back.
const RESTORE_LABELS: &[(&str, &str)] = &[
    ("showPlan", "the power plan"),
    ("screenSaver", "the screen saver"),
    ("snap", "Snap"),
    ("dnd", "Do Not Disturb"),
    ("systemSounds", "system sounds"),
    ("crashDialogs", "crash dialogs"),
    ("autoPlay", "AutoPlay"),
    ("showDesktopCorner", "the Show Desktop corner"),
    ("windowShake", "window shake"),
    ("accessibilityKeys", "accessibility shortcuts"),
    ("wallpaper", "the wallpaper"),
    ("displayLayout", "the main display"),
    ("audioMuted", "muted apps"),
    ("quitApps", "apps to reopen"),
    ("syncthingFolders", "Syncthing folders"),
];

impl Controller {
    pub fn engage(&mut self) {
        if self.engaged {
            return;
        }
        self.warnings.clear();
        journal::load();
        journal::set("engaged", json!(showlog::stamp(std::time::SystemTime::now())));
        showlog::note(format!("show mode on (v{})", env!("CARGO_PKG_VERSION")));
        let mut warnings = Vec::new();
        let mut warn = |w: Option<String>| {
            if let Some(w) = w {
                showlog::note(format!("warning: {w}"));
                warnings.push(w);
            }
        };

        if settings::is_on(Guard::Sleep) {
            self.keep_awake = power::KeepAwake::take();
            if self.keep_awake.is_none() {
                warn(Some("The PC may still sleep: Windows refused the keep-awake request".into()));
            }
        }
        let (plan, saver) = (settings::is_on(Guard::ShowPlan), settings::is_on(Guard::EnergySaver));
        if plan || saver {
            warn(power::engage_show_plan(plan, saver));
            // Read back, allowing Windows a second to act on the new plan.
            let still_on = || preflight::energy_saver() == Some(true);
            if saver && still_on() {
                let deadline = Instant::now() + Duration::from_secs(1);
                while still_on() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(100));
                }
                if still_on() {
                    warn(Some("Energy saver is still on: turn it off in Quick Settings".into()));
                }
            }
        }
        if settings::is_on(Guard::ScreenSaver) {
            warn(spi::engage_screen_saver());
        }
        if settings::is_on(Guard::Snap) {
            warn(spi::engage_snap());
        }
        if settings::is_on(Guard::Notifications) {
            warn(dnd::engage());
        }
        if settings::is_on(Guard::SystemSounds) {
            warn(sounds::engage());
        }
        if settings::is_on(Guard::CrashDialogs) {
            warn(crashdialogs::engage());
        }
        if settings::is_on(Guard::AutoPlay) {
            warn(explorer::engage_autoplay());
        }
        if settings::is_on(Guard::ShowDesktop) {
            warn(explorer::engage_show_desktop());
        }
        if settings::is_on(Guard::AccessibilityKeys) {
            warn(keys::engage_accessibility());
        }
        if settings::is_on(Guard::WindowsKey) {
            match WindowsKey::start() {
                Ok(k) => self.windows_key = Some(k),
                Err(w) => warn(Some(w)),
            }
        }
        if settings::is_on(Guard::Wallpaper) {
            warn(wallpaper::engage());
        }
        if settings::is_on(Guard::DisplayLayout) {
            match display::Lock::start() {
                Ok(lock) => self.display = Some(lock),
                Err(w) => warn(Some(w)),
            }
        }
        if settings::is_on(Guard::CursorFence) {
            match CursorFence::start() {
                Ok(f) => self.fence = Some(f),
                Err(w) => warn(Some(w)),
            }
        }
        if settings::is_on(Guard::MuteOtherApps) {
            // The watchdog first, so there is never a muted app with nothing to unmute it.
            self.watchdog = Watchdog::spawn();
            if self.watchdog.is_none() {
                warn(Some("Other apps not muted: the watchdog that unmutes them after a crash would not start".into()));
            } else {
                if settings::allowed_audio_apps().is_empty() {
                    warn(Some("No app is allowed to play: tick the show app under Allowed to Play".into()));
                }
                audio::engage();
            }
        }
        // Last: these wait up to five seconds each for apps to quit.
        if settings::is_on(Guard::ColourApps) {
            warn(apps::quit_colour_apps());
        }
        if settings::is_on(Guard::Syncthing) {
            warn(apps::pause_syncthing());
        }
        if settings::is_on(Guard::SyncApps) {
            warn(apps::quit_sync_apps());
        }

        self.warnings = warnings;
        self.engaged = true;
    }

    /// Puts everything back, journal or not. What can't be put back stays in the journal for
    /// Finish Restoring and the next launch.
    pub fn restore(&mut self) {
        self.keep_awake = None;
        self.display = None;
        self.windows_key = None;
        self.fence = None;
        journal::load();
        if !journal::exists() {
            self.watchdog = None;
            self.engaged = false;
            self.warnings.clear();
            audio::resume_pending();
            return;
        }
        // Audio first: it's the one a slow restore could leave the audience waiting on.
        audio::restore();
        self.watchdog = None;
        power::restore_show_plan();
        spi::restore_screen_saver();
        spi::restore_snap();
        dnd::restore();
        sounds::restore();
        crashdialogs::restore();
        explorer::restore_autoplay();
        explorer::restore_show_desktop();
        keys::restore_accessibility();
        display::restore();
        wallpaper::restore();
        apps::resume_syncthing();
        // Last, so f.lux comes back to the screens as they were.
        apps::reopen();

        // Each restore removes its own entry once it's done; what's left needs another try.
        let left: Vec<String> = journal::keys().into_iter().filter(|k| k != "engaged").collect();
        journal::retain(&left);
        self.engaged = false;
        self.warnings.clear();
        if !left.is_empty() {
            let names: Vec<String> = left
                .iter()
                .map(|k| RESTORE_LABELS.iter().find(|(key, _)| key == k).map(|(_, l)| l.to_string()).unwrap_or(k.clone()))
                .collect();
            let w = format!("Not put back yet: {}. Choose Finish Restoring.", util::list(&names));
            showlog::note(format!("restore incomplete: {left:?}"));
            self.warnings.push(w);
        } else {
            showlog::note("show mode off: everything put back");
        }
    }

    pub fn toggle(&mut self) {
        if self.engaged {
            self.restore()
        } else {
            self.engage()
        }
    }

    /// A journal on disk at launch means the last session never restored. Returns whether it
    /// found one.
    pub fn recover_if_needed(&mut self) -> bool {
        if !journal::exists() {
            audio::resume_pending();
            return false;
        }
        showlog::note("launch: found the journal of an unfinished show; restoring");
        self.restore();
        true
    }

    /// Whether a restore left something behind (Finish Restoring is offered).
    pub fn unfinished(&self) -> bool {
        !self.engaged && journal::exists()
    }

    /// Every five seconds mid-show: puts the black wallpaper back if something changed it.
    pub fn tick(&mut self) {
        if self.engaged && journal::contains("wallpaper") {
            wallpaper::apply_black();
        }
    }

    /// After a display change (and every five seconds): puts the display layout right, then
    /// works the cursor fence out again for the layout it left. Returns the problem if the
    /// display lock has just stood down, which stays listed in the menu.
    pub fn enforce_display(&mut self, why: &str) -> Option<String> {
        if !self.engaged {
            return None;
        }
        let problem = self.display.as_mut().and_then(|d| d.enforce(why));
        if let Some(f) = &self.fence {
            f.screens_changed();
        }
        let problem = problem?;
        self.warnings.push(problem.clone());
        Some(problem)
    }
}
