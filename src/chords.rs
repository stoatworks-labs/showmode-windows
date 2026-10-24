//! Which keys the Windows key guard swallows, kept apart from the hook in keys.rs so it can be
//! tested anywhere.
//!
//! The Windows keys themselves, every key pressed while one is held (so Win+D, Win+Tab, Win+P,
//! Win+Plus and the Copilot key, which arrives as Win+Shift+F23, reach neither the shell nor the
//! show app), Ctrl+Esc (Start again) and Print Screen (the Snipping Tool, which dims every screen).
//! A key's release is swallowed only if its press was, so a key already down when the guard
//! starts is never left stuck down.

pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_SNAPSHOT: u32 = 0x2C;
pub const VK_LWIN: u32 = 0x5B;
pub const VK_RWIN: u32 = 0x5C;

/// The modifiers held when a key arrives, as the system saw them before it.
#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

pub struct Chords {
    /// The Windows keys held, by the guard's own count: the system never sees them.
    win: [bool; 2],
    /// Keys whose press was swallowed, so their release goes too.
    swallowed: [bool; 256],
}

impl Default for Chords {
    fn default() -> Chords {
        Chords { win: [false; 2], swallowed: [false; 256] }
    }
}

impl Chords {
    /// Whether to swallow this key event.
    pub fn swallow(&mut self, vk: u32, up: bool, mods: Mods) -> bool {
        let vk = (vk & 0xff) as usize;
        let win = match vk as u32 {
            VK_LWIN => Some(0),
            VK_RWIN => Some(1),
            _ => None,
        };
        if up {
            if let Some(w) = win {
                self.win[w] = false;
            }
            return std::mem::take(&mut self.swallowed[vk]);
        }
        if let Some(w) = win {
            self.win[w] = true;
        }
        let ctrl_esc = vk as u32 == VK_ESCAPE && mods.ctrl && !mods.shift && !mods.alt;
        let take = win.is_some() || self.win.iter().any(|&w| w) || ctrl_esc || vk as u32 == VK_SNAPSHOT;
        if take {
            self.swallowed[vk] = true;
        }
        take
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: u32 = 0x44;
    const VK_CONTROL_L: u32 = 0xA2;

    fn press(c: &mut Chords, vk: u32, mods: Mods) -> (bool, bool) {
        (c.swallow(vk, false, mods), c.swallow(vk, true, mods))
    }

    #[test]
    fn win_and_everything_under_it() {
        let mut c = Chords::default();
        let none = Mods::default();
        assert!(c.swallow(VK_LWIN, false, none));
        assert_eq!(press(&mut c, D, none), (true, true), "Win+D goes, D included");
        assert!(c.swallow(VK_LWIN, true, none));
        assert_eq!(press(&mut c, D, none), (false, false), "D on its own is typed");
        // The Copilot key: Win+Shift+F23.
        assert!(c.swallow(VK_LWIN, false, none));
        assert!(c.swallow(0xA0, false, none));
        assert_eq!(press(&mut c, 0x86, Mods { shift: true, ..none }), (true, true));
        assert!(c.swallow(0xA0, true, none));
        assert!(c.swallow(VK_LWIN, true, none));
    }

    #[test]
    fn a_key_held_before_win_comes_up_normally() {
        let mut c = Chords::default();
        let none = Mods::default();
        assert!(!c.swallow(VK_CONTROL_L, false, none));
        assert!(c.swallow(VK_RWIN, false, Mods { ctrl: true, ..none }));
        assert!(!c.swallow(VK_CONTROL_L, true, none), "its press got through, so its release does");
        assert!(c.swallow(VK_RWIN, true, none));
    }

    #[test]
    fn a_win_already_down_is_never_stuck() {
        let mut c = Chords::default();
        assert!(!c.swallow(VK_LWIN, true, Mods::default()), "a release whose press was never seen passes");
        assert!(!c.swallow(D, false, Mods::default()), "and no Windows key is taken as held");
    }

    #[test]
    fn ctrl_esc_and_print_screen() {
        let mut c = Chords::default();
        let ctrl = Mods { ctrl: true, ..Mods::default() };
        assert_eq!(press(&mut c, VK_ESCAPE, ctrl), (true, true));
        assert_eq!(press(&mut c, VK_ESCAPE, Mods { shift: true, ..ctrl }), (false, false), "Ctrl+Shift+Esc is Task Manager");
        assert_eq!(press(&mut c, VK_ESCAPE, Mods::default()), (false, false));
        assert_eq!(press(&mut c, VK_SNAPSHOT, Mods::default()), (true, true));
        assert_eq!(press(&mut c, VK_SNAPSHOT, Mods { alt: true, ..Mods::default() }), (true, true));
    }
}
