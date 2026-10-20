//! Pre-show checks: what Show Mode can't safely switch off, only spot and say, each with the
//! one thing to do about it. Read-only: nothing here changes anything. They're listed under
//! Pre-show Checks in the menu and in the note at the start of a show, and the ones that can
//! change mid-show (`live`) stay at the top of the menu while it runs.
//!
//! The one that matters most is a pending restart. Windows Update restarts a PC on its own
//! deadline whatever any app says (WM_QUERYENDSESSION can't refuse a forced restart), so the
//! only defence is to restart before doors. It leads the note and the menu whenever it's there.

use crate::{cloudstore, reg};
use std::time::Instant;
use windows::core::PWSTR;
use windows::System::Power::{EnergySaverStatus, PowerManager};
use windows::Win32::Networking::NetworkListManager::{
    INetworkListManager, NetworkListManager, NLM_ENUM_NETWORK_CONNECTED, NLM_NETWORK_CATEGORY_PUBLIC,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::RemoteDesktop::{
    WTSClientProtocolType, WTSFreeMemory, WTSQuerySessionInformationW, WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION,
};
use windows::Win32::System::UpdateAgent::{ISystemInformation, SystemInformation};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_REMOTESESSION};

pub struct Check {
    pub text: String,
    /// Can change mid-show, so it stays at the top of the menu while one runs.
    pub live: bool,
    /// A pending restart: first in the note, and at the top of the menu whether or not a show
    /// is running.
    pub urgent: bool,
}

fn check(text: impl Into<String>, live: bool) -> Check {
    Check { text: text.into(), live, urgent: false }
}

// MARK: a pending restart

const UPDATE_RESTART_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired";
const SERVICING_RESTART_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending";

/// The Windows Update Agent's own answer.
fn update_agent_restart() -> Option<bool> {
    unsafe {
        let s: ISystemInformation = CoCreateInstance(&SystemInformation, None, CLSCTX_ALL).ok()?;
        s.RebootRequired().ok().map(|v| v.0 != 0)
    }
}

struct Restart {
    agent: Option<bool>,
    update_key: bool,
    servicing_key: bool,
}

fn restart() -> Restart {
    Restart {
        agent: update_agent_restart(),
        update_key: reg::machine_key_exists(UPDATE_RESTART_KEY),
        servicing_key: reg::machine_key_exists(SERVICING_RESTART_KEY),
    }
}

impl Restart {
    fn pending(&self) -> bool {
        self.agent == Some(true) || self.update_key || self.servicing_key
    }
}

// MARK: power

fn power_status() -> Option<SYSTEM_POWER_STATUS> {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s) }.ok()?;
    Some(s)
}

/// Running on battery, with the charge if Windows knows it.
fn battery() -> Option<String> {
    let s = power_status()?;
    // ACLineStatus 0 is offline; BatteryFlag 128 is "no system battery".
    if s.ACLineStatus != 0 || s.BatteryFlag == 128 {
        return None;
    }
    let pct = if s.BatteryLifePercent <= 100 { format!(" ({}%)", s.BatteryLifePercent) } else { String::new() };
    Some(format!("Running on battery{pct}: plug in for the show"))
}

/// Energy saver (battery saver before 24H2): Windows' documented status, or the power status
/// flag that mirrors it.
fn energy_saver() -> Option<bool> {
    let winrt = PowerManager::EnergySaverStatus().ok().map(|s| s == EnergySaverStatus::On);
    let flag = power_status().map(|s| s.SystemStatusFlag == 1);
    match (winrt, flag) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (None, None) => None,
        _ => Some(false),
    }
}

// MARK: Night light (read only: see cloudstore.rs)

const CLOUD_STORE: &str = r"Software\Microsoft\Windows\CurrentVersion\CloudStore\Store";
/// (state, settings): where Windows 11 keeps them, then where Windows 10 did.
const NIGHT_LIGHT_KEYS: &[(&str, &str)] = &[
    (
        r"DefaultAccount\Current\default$windows.data.bluelightreduction.bluelightreductionstate\windows.data.bluelightreduction.bluelightreductionstate",
        r"DefaultAccount\Current\default$windows.data.bluelightreduction.settings\windows.data.bluelightreduction.settings",
    ),
    (
        r"Cache\DefaultAccount\$$windows.data.bluelightreduction.bluelightreductionstate\Current",
        r"Cache\DefaultAccount\$$windows.data.bluelightreduction.settings\Current",
    ),
];

/// The two blobs, from the first place that has the state.
fn night_light_blobs() -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    NIGHT_LIGHT_KEYS.iter().find_map(|(state, settings)| {
        let s = reg::get_binary(&format!(r"{CLOUD_STORE}\{state}"), "Data")?;
        Some((s, reg::get_binary(&format!(r"{CLOUD_STORE}\{settings}"), "Data")))
    })
}

// MARK: the desktop

const ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

/// "Show my taskbar on all displays": on unless the value is 0. With no value, winlab (25H2)
/// showed a taskbar on all three of its displays.
fn taskbar_everywhere() -> bool {
    reg::get_dword(ADVANCED, "MMTaskbarEnabled") != Some(0)
}

/// "Minimise windows when a monitor is disconnected": on unless the value is 1.
fn minimises_on_disconnect() -> bool {
    reg::get_dword(r"Control Panel\Desktop", "MonitorRemovalRecalcBehavior") != Some(1)
}

// MARK: the network and the session

/// Connected networks whose profile is Public.
fn public_networks() -> Option<Vec<String>> {
    unsafe {
        let nlm: INetworkListManager = CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).ok()?;
        let nets = nlm.GetNetworks(NLM_ENUM_NETWORK_CONNECTED).ok()?;
        let mut out = Vec::new();
        loop {
            let mut one = [None];
            let mut fetched = 0u32;
            if nets.Next(&mut one, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(net) = one[0].take() else { break };
            if net.GetCategory().ok() == Some(NLM_NETWORK_CATEGORY_PUBLIC) {
                out.push(net.GetName().map(|n| n.to_string()).unwrap_or_default());
            }
        }
        Some(out)
    }
}

/// The session's client protocol: 0 the console, 2 Remote Desktop.
fn session_protocol() -> Option<u16> {
    unsafe {
        let mut buf = PWSTR::null();
        let mut bytes = 0u32;
        WTSQuerySessionInformationW(WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTSClientProtocolType, &mut buf, &mut bytes)
            .ok()?;
        let v = if bytes >= 2 && !buf.is_null() { *(buf.0 as *const u16) } else { 0 };
        WTSFreeMemory(buf.0 as _);
        Some(v)
    }
}

fn remote_session() -> bool {
    let metric = unsafe { GetSystemMetrics(SM_REMOTESESSION) } != 0;
    metric || session_protocol() == Some(2)
}

// MARK: the list

pub fn items() -> Vec<Check> {
    let mut out = Vec::new();
    if restart().pending() {
        out.push(Check {
            text: "Windows has a restart waiting: restart before doors, since an update restart can't be refused".into(),
            live: true,
            urgent: true,
        });
    }
    if remote_session() {
        out.push(check("This is a Remote Desktop session: the PC's own screens show the lock screen until it ends", true));
    }
    if let Some(b) = battery() {
        out.push(check(b, true));
    }
    if energy_saver() == Some(true) {
        out.push(check("Energy saver is on, slowing the PC: turn it off in Settings > System > Power", true));
    }
    for name in public_networks().unwrap_or_default() {
        let name = if name.is_empty() { "A network".to_string() } else { format!("\u{201c}{name}\u{201d}") };
        out.push(check(
            format!("{name} is a Public network, so NDI and Dante may find nothing: make it Private in Settings"),
            true,
        ));
    }
    if let Some((state, settings)) = night_light_blobs() {
        if cloudstore::night_light_on(&state) == Some(true) {
            out.push(check("Night light is on, tinting the projector too: turn it off in Settings > System > Display", true));
        } else if settings.as_deref().and_then(cloudstore::night_light_scheduled) == Some(true) {
            out.push(check("Night light is scheduled and can tint the projector mid-show: turn the schedule off in Settings", false));
        }
    }
    if taskbar_everywhere() {
        out.push(check("The taskbar shows on every display, the projector too: change it in Settings > Personalisation > Taskbar", false));
    }
    if minimises_on_disconnect() {
        out.push(check("A projector dropping out minimises its windows: untick that in Settings > System > Display > Multiple displays", false));
    }
    out
}

/// For --status: what each check read, and how long the lot took.
pub fn describe() -> String {
    let t = Instant::now();
    let r = restart();
    let yn = |b: bool| if b { "yes" } else { "no" };
    let opt = |b: Option<bool>| b.map(yn).unwrap_or("unreadable");
    let mut lines = vec![format!(
        "restart pending: {} (Windows Update Agent {}, RebootRequired key {}, RebootPending key {})",
        yn(r.pending()),
        opt(r.agent),
        yn(r.update_key),
        yn(r.servicing_key)
    )];
    let p = power_status();
    lines.push(format!(
        "power: {}, energy saver {} (EnergySaverStatus {:?}, SystemStatusFlag {:?})",
        match p {
            Some(s) if s.ACLineStatus == 0 && s.BatteryFlag != 128 => format!("on battery ({}%)", s.BatteryLifePercent),
            Some(s) => format!("mains (ACLineStatus {}, BatteryFlag {})", s.ACLineStatus, s.BatteryFlag),
            None => "unreadable".into(),
        },
        opt(energy_saver()),
        PowerManager::EnergySaverStatus().ok().map(|s| s.0),
        p.map(|s| s.SystemStatusFlag)
    ));
    lines.push(match night_light_blobs() {
        None => "night light: no state stored".into(),
        Some((state, settings)) => format!(
            "night light: on {}, scheduled {}\n  state blob: {}\n  settings blob: {}",
            opt(cloudstore::night_light_on(&state)),
            opt(settings.as_deref().and_then(cloudstore::night_light_scheduled)),
            cloudstore::hex(&state),
            settings.as_deref().map(cloudstore::hex).unwrap_or_else(|| "none".into())
        ),
    });
    lines.push(format!(
        "taskbar on all displays: {} (MMTaskbarEnabled {:?}); minimise windows on disconnect: {} (MonitorRemovalRecalcBehavior {:?})",
        yn(taskbar_everywhere()),
        reg::get_dword(ADVANCED, "MMTaskbarEnabled"),
        yn(minimises_on_disconnect()),
        reg::get_dword(r"Control Panel\Desktop", "MonitorRemovalRecalcBehavior")
    ));
    lines.push(format!(
        "public networks: {}",
        match public_networks() {
            None => "unreadable".into(),
            Some(n) if n.is_empty() => "none".into(),
            Some(n) => format!("{n:?}"),
        }
    ));
    lines.push(format!(
        "remote session: {} (SM_REMOTESESSION {}, protocol {:?})",
        yn(remote_session()),
        unsafe { GetSystemMetrics(SM_REMOTESESSION) },
        session_protocol()
    ));
    let read = t.elapsed();
    let t = Instant::now();
    let items = items();
    lines.push(format!("checks ({} ms to read, {} ms to list):", read.as_millis(), t.elapsed().as_millis()));
    if items.is_empty() {
        lines.push("  none".into());
    }
    for c in items {
        lines.push(format!("  {}{}{}", if c.urgent { "URGENT " } else { "" }, if c.live { "(live) " } else { "" }, c.text));
    }
    lines.join("\n")
}
