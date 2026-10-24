//! The display lock: keeps the main display the one that was main when the show started, and
//! splits any display that starts mirroring mid-show back out to extended. Windows often puts a
//! laptop meeting an unfamiliar projector into Duplicate, and a projector that drops out and
//! comes back brings back whatever layout Windows remembers for it. Mirrors that were there at
//! the start (a confidence monitor showing the output, say) are taken as deliberate and left
//! alone.
//!
//! Everything goes through the documented CCD API (QueryDisplayConfig and SetDisplayConfig),
//! and every correction is a temporary layout: never SDC_SAVE_TO_DATABASE, so the layouts
//! Windows remembers for each set of screens are never touched, and the next time these
//! screens meet, Windows brings back its own.
//!
//! A split is made by hand where it can be: the new screen gets a desktop of its own and every
//! other screen stays where it is. Duplicate across two adapters can't always be undone that way
//! (on winlab, Windows put every screen on the virtual adapter's one desktop, leaving it none
//! to spare), so then Windows' own extended layout for these screens is applied instead, again
//! without saving, as long as no mirror was there at the start for it to undo.
//!
//! WM_DISPLAYCHANGE on the app's window, and the five-second check, bring it here. A layout
//! something else keeps reverting is let go rather than looped on: after four corrections
//! inside twenty seconds, or two in a row that didn't work, the lock stands down for the rest
//! of the show and says so. At the end the display that was main before the show is made main
//! again. Displays split out of a mirror stay extended: the mirror was Windows' guess, not the
//! operator's choice.

use crate::layout::{self, Mirrors, Output, Plan};
use crate::{journal, showlog, util};
use serde_json::json;
use std::time::{Duration, Instant};
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, LUID, POINTL};

const FIGHT_LIMIT: usize = 4;
const FIGHT_WINDOW: Duration = Duration::from_secs(20);
/// Corrections in a row that failed or didn't hold. Retrying one that can't work would go on
/// all show: the five-second check spaces the retries so the fight limit never trips.
const FAILURE_LIMIT: u32 = 2;

#[derive(Clone)]
struct Config {
    paths: Vec<DISPLAYCONFIG_PATH_INFO>,
    modes: Vec<DISPLAYCONFIG_MODE_INFO>,
}

fn query(flags: QUERY_DISPLAY_CONFIG_FLAGS) -> Option<Config> {
    // The layout can change between asking for the sizes and asking for the layout.
    for _ in 0..4 {
        let (mut np, mut nm) = (0u32, 0u32);
        if unsafe { GetDisplayConfigBufferSizes(flags, &mut np, &mut nm) } != ERROR_SUCCESS {
            return None;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        let mut topology = DISPLAYCONFIG_TOPOLOGY_ID::default();
        let topology = (flags == QDC_DATABASE_CURRENT).then_some(&mut topology as *mut _);
        let r = unsafe { QueryDisplayConfig(flags, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), topology) };
        if r == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if r != ERROR_SUCCESS {
            return None;
        }
        paths.truncate(np as usize);
        modes.truncate(nm as usize);
        return Some(Config { paths, modes });
    }
    None
}

fn luid(l: LUID) -> u64 {
    ((l.HighPart as u32 as u64) << 32) | l.LowPart as u64
}

/// The monitor at the end of a path.
struct Target {
    key: String,
    name: String,
    internal: bool,
}

fn target(p: &DISPLAYCONFIG_PATH_INFO) -> Target {
    let mut n = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
    n.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
    n.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
    n.header.adapterId = p.targetInfo.adapterId;
    n.header.id = p.targetInfo.id;
    let ok = unsafe { DisplayConfigGetDeviceInfo(&mut n.header) } == 0;
    let text = |s: &[u16]| String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]);
    let tech = p.targetInfo.outputTechnology;
    let internal = [
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL,
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED,
    ]
    .contains(&tech);
    let path = if ok { text(&n.monitorDevicePath) } else { String::new() };
    let friendly = if ok { text(&n.monitorFriendlyDeviceName) } else { String::new() };
    Target {
        key: if path.is_empty() { format!("{:x}:{}", luid(p.targetInfo.adapterId), p.targetInfo.id) } else { path },
        name: if !friendly.is_empty() {
            friendly
        } else if internal {
            "the built-in screen".into()
        } else {
            format!("display {}", p.targetInfo.id)
        },
        internal,
    }
}

/// The active paths as outputs, index for index. None if any path has no desktop to read.
fn outputs(cfg: &Config) -> Option<(Vec<Output>, Vec<Target>)> {
    let mut outs = Vec::new();
    let mut targets = Vec::new();
    for p in &cfg.paths {
        let m = cfg.modes.get(unsafe { p.sourceInfo.Anonymous.modeInfoIdx } as usize)?;
        if m.infoType != DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE {
            return None;
        }
        let sm = unsafe { m.Anonymous.sourceMode };
        let t = target(p);
        outs.push(Output {
            key: t.key.clone(),
            source: (luid(p.sourceInfo.adapterId), p.sourceInfo.id),
            origin: (sm.position.x, sm.position.y),
            size: (sm.width, sm.height),
        });
        targets.push(t);
    }
    Some((outs, targets))
}

fn current() -> Option<(Config, Vec<Output>, Vec<Target>)> {
    let cfg = query(QDC_ONLY_ACTIVE_PATHS)?;
    let (outs, targets) = outputs(&cfg)?;
    Some((cfg, outs, targets))
}

/// A connected screen, for the cursor fence and its menu.
pub struct ScreenInfo {
    /// The device path, as the display lock keys monitors.
    pub key: String,
    pub name: String,
    pub origin: (i32, i32),
    pub size: (u32, u32),
}

pub fn screens() -> Vec<ScreenInfo> {
    let Some((_, outs, targets)) = current() else { return Vec::new() };
    outs.into_iter()
        .zip(targets)
        .map(|(o, t)| ScreenInfo { key: o.key, name: t.name, origin: o.origin, size: o.size })
        .collect()
}

/// Applies a plan to the layout `cfg` describes, as a temporary layout. Returns SetDisplayConfig's
/// result (0 for success), or None if a split had no free desktop to go to.
fn apply(cfg: &Config, outs: &[Output], plan: &Plan) -> Option<i32> {
    let mut paths = cfg.paths.clone();
    let mut modes = cfg.modes.clone();
    for m in modes.iter_mut().filter(|m| m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE) {
        unsafe {
            m.Anonymous.sourceMode.position.x -= plan.shift.0;
            m.Anonymous.sourceMode.position.y -= plan.shift.1;
        }
    }
    if !plan.splits.is_empty() {
        // Every path the adapters could make, to find a desktop no screen is showing.
        let all = query(QDC_ALL_PATHS)?;
        let mut used: Vec<(u64, u32)> = outs.iter().map(|o| o.source).collect();
        let mut right = layout::right_edge(outs, plan.shift);
        let source_mode = |i: usize| unsafe { cfg.paths[i].sourceInfo.Anonymous.modeInfoIdx } as usize;
        for group in &plan.splits {
            // Each group goes to the right of everything. A screen with a desktop of its own (a
            // mirror across two adapters) just moves it; one sharing its desktop with a screen
            // that stays (a mirror on one adapter) gets a free desktop on the same adapter.
            let at = POINTL { x: right, y: 0 };
            right += group.iter().map(|&i| outs[i].size.0 as i32).max().unwrap_or(0);
            let shared = |i: usize| outs.iter().enumerate().any(|(j, o)| !group.contains(&j) && o.source == outs[i].source);
            let mut fresh: Vec<(u64, u32, u32)> = Vec::new(); // adapter, source id, mode index
            for &i in group {
                if !shared(i) {
                    modes[source_mode(i)].Anonymous.sourceMode.position = at;
                    continue;
                }
                let adapter = outs[i].source.0;
                let (id, index) = match fresh.iter().find(|f| f.0 == adapter) {
                    Some(&(_, id, index)) => (id, index),
                    None => {
                        let members: Vec<usize> = group.iter().copied().filter(|&j| shared(j) && outs[j].source.0 == adapter).collect();
                        let drives = |s: u32, j: usize| {
                            all.paths.iter().any(|p| {
                                luid(p.sourceInfo.adapterId) == adapter
                                    && p.sourceInfo.id == s
                                    && p.targetInfo.adapterId == cfg.paths[j].targetInfo.adapterId
                                    && p.targetInfo.id == cfg.paths[j].targetInfo.id
                            })
                        };
                        let free = all
                            .paths
                            .iter()
                            .filter(|p| luid(p.sourceInfo.adapterId) == adapter)
                            .map(|p| p.sourceInfo.id)
                            .find(|&s| !used.contains(&(adapter, s)) && members.iter().all(|&j| drives(s, j)))?;
                        used.push((adapter, free));
                        // The new desktop starts as a copy of the mirrored one.
                        let mut m = modes[source_mode(i)];
                        m.id = free;
                        m.Anonymous.sourceMode.position = at;
                        modes.push(m);
                        let index = (modes.len() - 1) as u32;
                        fresh.push((adapter, free, index));
                        (free, index)
                    }
                };
                paths[i].sourceInfo.id = id;
                paths[i].sourceInfo.Anonymous.modeInfoIdx = index;
            }
        }
    }
    // No SDC_SAVE_TO_DATABASE: a temporary layout, so Windows' remembered ones stay the user's.
    Some(unsafe { SetDisplayConfig(Some(&paths), Some(&modes), SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES) })
}

fn names(targets: &[Target], indexes: impl IntoIterator<Item = usize>) -> String {
    util::list(&indexes.into_iter().map(|i| targets[i].name.clone()).collect::<Vec<_>>())
}

/// The lock while a show runs.
pub struct Lock {
    main: String,
    main_name: String,
    baseline: Mirrors,
    recent: Vec<Instant>,
    failures: u32,
    stood_down: bool,
}

impl Lock {
    /// Journals the main display and takes the mirrors there now as deliberate.
    pub fn start() -> Result<Lock, String> {
        let Some((_, outs, targets)) = current() else {
            return Err("Display lock off: Windows wouldn't describe the displays".into());
        };
        let Some(main) = layout::main_key(&outs, |i| targets[i].internal) else {
            return Err("Display lock off: no display is main".into());
        };
        let main_name = targets[outs.iter().position(|o| o.key == main).unwrap_or(0)].name.clone();
        // Recorded once: over a restore that never finished, the main display from before that
        // show is the one to put back.
        journal::record("displayLayout", json!({ "main": main, "name": main_name }));
        let baseline = layout::mirrors(&outs);
        let mirrored: Vec<usize> = (0..outs.len()).filter(|&i| baseline.contains_key(&outs[i].key)).collect();
        showlog::note(format!(
            "display: lock on, keeping {main_name} main ({main}){}",
            if mirrored.is_empty() { String::new() } else { format!(", leaving {} mirrored", names(&targets, mirrored)) }
        ));
        Ok(Lock { main, main_name, baseline, recent: Vec::new(), failures: 0, stood_down: false })
    }

    /// Puts the layout right if something changed it. Returns the problem when it has just
    /// stood down.
    pub fn enforce(&mut self, why: &str) -> Option<String> {
        if self.stood_down {
            return None;
        }
        let (cfg, outs, targets) = current()?;
        let plan = layout::plan(&outs, &self.baseline, &self.main);
        if plan.is_empty() {
            return None;
        }
        // Each correction raises another display change, which finds nothing left to do. A
        // layout that keeps coming back means something else is fighting, and fighting back
        // would leave the screens blinking through the show.
        let now = Instant::now();
        self.recent.retain(|t| now.duration_since(*t) < FIGHT_WINDOW);
        self.recent.push(now);
        if self.recent.len() > FIGHT_LIMIT {
            return Some(self.stand_down("The displays keep changing back: Show Mode has stopped correcting them for this show"));
        }
        let mut what = Vec::new();
        if !plan.splits.is_empty() {
            what.push(format!("split {} out of a mirror", names(&targets, plan.splits.iter().flatten().copied())));
        }
        if plan.shift != (0, 0) {
            what.push(format!("made {} main again", self.main_name));
        }
        let what = util::list(&what);
        match self.correct(&cfg, &outs, &plan) {
            Ok(how) => {
                self.failures = 0;
                showlog::note(format!("display: {what}{how} ({why})"));
            }
            Err(e) => {
                self.failures += 1;
                showlog::note(format!("display: couldn't {what} ({why}): {e}"));
                if self.failures >= FAILURE_LIMIT {
                    return Some(self.stand_down("Show Mode couldn't put the displays back, so it has stopped trying for this show"));
                }
            }
        }
        None
    }

    fn stand_down(&mut self, problem: &str) -> String {
        self.stood_down = true;
        showlog::note(format!("display: {problem}"));
        problem.to_string()
    }

    /// One correction: by hand, else (for a split) Windows' own extended layout, and then the
    /// main display put back if that layout moved it. Says how, or why it didn't work.
    fn correct(&self, cfg: &Config, outs: &[Output], plan: &Plan) -> Result<&'static str, String> {
        let mut how = "";
        let r = apply(cfg, outs, plan);
        if r != Some(0) {
            let by_hand = match r {
                None => "no free desktop on that adapter".to_string(),
                Some(e) => format!("SetDisplayConfig error {e}"),
            };
            if plan.splits.is_empty() || !self.baseline.is_empty() {
                return Err(by_hand);
            }
            // Not saved: the next time these screens meet, Windows brings back its own layout.
            let e = unsafe { SetDisplayConfig(None, None, SDC_APPLY | SDC_TOPOLOGY_EXTEND) };
            if e != 0 {
                return Err(format!("{by_hand}, and Windows' extended layout failed too (error {e})"));
            }
            how = " with Windows' own extended layout";
        }
        // Read back: a call can report success without taking effect, and the extended layout
        // Windows brings back may have another main display.
        let (cfg, outs, _) = current().ok_or("the layout couldn't be read back")?;
        let again = layout::plan(&outs, &self.baseline, &self.main);
        if again.is_empty() {
            return Ok(how);
        }
        let held = again.splits.is_empty()
            && apply(&cfg, &outs, &again) == Some(0)
            && current().is_some_and(|(_, o, _)| layout::plan(&o, &self.baseline, &self.main).is_empty());
        if held {
            return Ok(if how.is_empty() { "" } else { " with Windows' own extended layout, then the main display moved back" });
        }
        Err("it didn't hold".into())
    }
}

/// Makes the display that was main before the show main again, if it's connected. One that
/// isn't connected needs nothing: the corrections were never saved, so when it comes back
/// Windows brings back its own layout for it.
pub fn restore() -> bool {
    let Some(entry) = journal::get("displayLayout") else { return true };
    let main = entry["main"].as_str().unwrap_or_default().to_string();
    let name = entry["name"].as_str().unwrap_or("the main display").to_string();
    let Some((cfg, outs, _)) = current() else {
        showlog::note("display: couldn't read the layout to put the main display back");
        return false;
    };
    let Some(origin) = outs.iter().find(|o| o.key == main).map(|o| o.origin) else {
        showlog::note(format!("display: {name} isn't connected; Windows brings its own layout back with it"));
        journal::remove("displayLayout");
        return true;
    };
    if origin != (0, 0) {
        let r = apply(&cfg, &outs, &Plan { splits: Vec::new(), shift: origin });
        // Read back: a call can report success without taking effect.
        let back = current().and_then(|(_, o, _)| o.iter().find(|o| o.key == main).map(|o| o.origin)) == Some((0, 0));
        if !back {
            showlog::note(format!("display: couldn't make {name} main again (SetDisplayConfig {r:?})"));
            return false;
        }
        showlog::note(format!("display: {name} is main again"));
    }
    journal::remove("displayLayout");
    true
}

fn summary(cfg: &Config) -> String {
    let Some((outs, targets)) = outputs(cfg) else { return "unreadable".into() };
    let mirrors = layout::mirrors(&outs);
    outs.iter()
        .zip(&targets)
        .map(|(o, t)| {
            format!(
                "{} {}x{} at ({},{}){}{} [{}]",
                t.name,
                o.size.0,
                o.size.1,
                o.origin.0,
                o.origin.1,
                if o.origin == (0, 0) { " main" } else { "" },
                if mirrors.contains_key(&o.key) { " mirrored" } else { "" },
                o.key
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// For --status: the layout now, and the one Windows remembers for these screens.
pub fn describe() -> String {
    let now = query(QDC_ONLY_ACTIVE_PATHS).map(|c| summary(&c)).unwrap_or_else(|| "unreadable".into());
    let saved = query(QDC_DATABASE_CURRENT).map(|c| summary(&c)).unwrap_or_else(|| "none".into());
    format!("displays: {now}\ndisplays as Windows remembers them: {saved}")
}
