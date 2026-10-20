//! How Show Mode hooks into Windows for the user: the showmode: URL scheme, so Companion, a
//! show-control script or a terminal can drive it, and Launch at Login. Both are per user, under
//! HKEY_CURRENT_USER, so neither needs admin.

use crate::reg;

const SCHEME_KEY: &str = r"Software\Classes\showmode";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "Show Mode";

fn exe() -> String {
    std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Registers showmode:// for this user, pointing at this copy of the app. Run at every launch,
/// so moving the app keeps the scheme working.
pub fn register_url_scheme() {
    let exe = exe();
    if exe.is_empty() {
        return;
    }
    reg::set_string(SCHEME_KEY, "", "URL:Show Mode");
    reg::set_string(SCHEME_KEY, "URL Protocol", "");
    reg::set_string(&format!(r"{SCHEME_KEY}\DefaultIcon"), "", &format!("\"{exe}\",0"));
    reg::set_string(&format!(r"{SCHEME_KEY}\shell\open\command"), "", &format!("\"{exe}\" \"%1\""));
}

pub fn launches_at_login() -> bool {
    reg::get_string(RUN_KEY, RUN_NAME).is_some()
}

pub fn set_launch_at_login(on: bool) {
    if on {
        reg::set_string(RUN_KEY, RUN_NAME, &format!("\"{}\"", exe()));
    } else {
        reg::delete(RUN_KEY, RUN_NAME);
    }
}

/// What a showmode: URL or command-line word asks for.
#[derive(Debug, PartialEq)]
pub enum Command {
    On,
    Off,
    Toggle,
}

/// "showmode://on", "showmode:off/", "toggle" and so on.
pub fn parse(arg: &str) -> Option<Command> {
    let a = arg.trim().to_ascii_lowercase();
    let word = a.strip_prefix("showmode:").unwrap_or(&a).trim_matches('/');
    match word {
        "on" => Some(Command::On),
        "off" => Some(Command::Off),
        "toggle" => Some(Command::Toggle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_urls() {
        assert_eq!(parse("showmode://on"), Some(Command::On));
        assert_eq!(parse("SHOWMODE://OFF/"), Some(Command::Off));
        assert_eq!(parse("showmode:toggle"), Some(Command::Toggle));
        assert_eq!(parse("toggle"), Some(Command::Toggle));
        assert_eq!(parse("showmode://fence-on"), None);
    }
}
