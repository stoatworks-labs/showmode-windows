//! The cursor fence's geometry, kept apart from the hook in cursorfence.rs so it can be tested
//! anywhere. Coordinates are physical pixels on the virtual screen, which is what a per-monitor
//! aware process gets from the mouse hook, GetCursorPos and the display configuration alike.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    /// Exclusive, as Windows' own rectangles are.
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn at(origin: (i32, i32), size: (u32, u32)) -> Rect {
        Rect { left: origin.0, top: origin.1, right: origin.0 + size.0 as i32, bottom: origin.1 + size.1 as i32 }
    }

    fn contains(&self, (x, y): (i32, i32)) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    fn clamp(&self, (x, y): (i32, i32)) -> (i32, i32) {
        (x.clamp(self.left, self.right - 1), y.clamp(self.top, self.bottom - 1))
    }
}

/// A screen as the fence sees it: where its desktop is, and whether it's ticked.
pub struct Screen {
    pub rect: Rect,
    pub fenced: bool,
}

#[derive(Debug, PartialEq)]
pub struct Fence {
    allowed: Vec<Rect>,
    blocked: Vec<Rect>,
}

impl Fence {
    /// None when there's nothing to do (no screen ticked) or nowhere to go (every screen ticked,
    /// or the only one left unticked is unplugged): the fence never traps the cursor. A desktop
    /// is blocked only if every screen showing it is ticked, so a mirror of the operator's screen
    /// stays usable.
    pub fn new(screens: &[Screen]) -> Option<Fence> {
        let mut allowed: Vec<Rect> = Vec::new();
        let mut blocked: Vec<Rect> = Vec::new();
        for s in screens.iter().filter(|s| !s.fenced) {
            if !allowed.contains(&s.rect) {
                allowed.push(s.rect);
            }
        }
        for s in screens.iter().filter(|s| s.fenced) {
            if !allowed.contains(&s.rect) && !blocked.contains(&s.rect) {
                blocked.push(s.rect);
            }
        }
        (!allowed.is_empty() && !blocked.is_empty()).then_some(Fence { allowed, blocked })
    }

    pub fn blocks(&self, p: (i32, i32)) -> bool {
        self.blocked.iter().any(|r| r.contains(p)) && !self.allowed.iter().any(|r| r.contains(p))
    }

    /// The nearest point on a screen the cursor may use.
    pub fn nearest_allowed(&self, p: (i32, i32)) -> (i32, i32) {
        let d = |q: (i32, i32)| (q.0 - p.0) as i64 * (q.0 - p.0) as i64 + (q.1 - p.1) as i64 * (q.1 - p.1) as i64;
        self.allowed.iter().map(|r| r.clamp(p)).min_by_key(|&q| d(q)).unwrap_or(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(origin: (i32, i32), size: (u32, u32), fenced: bool) -> Screen {
        Screen { rect: Rect::at(origin, size), fenced }
    }

    #[test]
    fn stops_at_the_edge() {
        // The operator's 1920x1080 at the origin, the projector to its right.
        let f = Fence::new(&[s((0, 0), (1920, 1080), false), s((1920, 0), (1280, 800), true)]).unwrap();
        assert!(!f.blocks((1919, 500)));
        assert!(f.blocks((1920, 500)));
        assert_eq!(f.nearest_allowed((1930, 500)), (1919, 500));
        assert_eq!(f.nearest_allowed((2500, 1000)), (1919, 1000), "straight across, on the same row");
    }

    #[test]
    fn nearest_of_two_allowed() {
        // Projector in the middle, an operator screen either side.
        let f = Fence::new(&[
            s((-1920, 0), (1920, 1080), false),
            s((0, 0), (1920, 1080), true),
            s((1920, 0), (1920, 1080), false),
        ])
        .unwrap();
        assert_eq!(f.nearest_allowed((100, 300)), (-1, 300));
        assert_eq!(f.nearest_allowed((1800, 300)), (1920, 300));
    }

    #[test]
    fn never_traps() {
        assert_eq!(Fence::new(&[s((0, 0), (1920, 1080), true)]), None, "every screen ticked");
        assert_eq!(Fence::new(&[s((0, 0), (1920, 1080), false)]), None, "nothing ticked");
        assert_eq!(Fence::new(&[]), None);
    }

    #[test]
    fn a_mirror_with_an_unticked_screen_stays_usable() {
        let f = Fence::new(&[
            s((0, 0), (1920, 1080), false),
            s((0, 0), (1920, 1080), true),
            s((1920, 0), (1280, 800), false),
        ]);
        assert_eq!(f, None, "the ticked screen mirrors an unticked one, so nothing is blocked");
    }
}
