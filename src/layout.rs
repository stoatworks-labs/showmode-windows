//! The display lock's arithmetic, kept apart from the Windows calls in display.rs so it can be
//! tested anywhere.
//!
//! Windows' display configuration (CCD) is a set of paths, each joining a source (a desktop,
//! one of \\.\DISPLAYn) to a target (a monitor). The source at (0,0) is the main display.
//! Monitors are mirrored (Duplicate) when they show desktops at the same place: on one adapter
//! they share a source, and across two (a hybrid-graphics laptop's panel and its HDMI port) each
//! has its own source, both at the same origin.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    /// The monitor, by its device path: stable while it stays on the same connector.
    pub key: String,
    /// The desktop it shows: adapter and source id.
    pub source: (u64, u32),
    /// Where that desktop sits on the virtual screen, and its size.
    pub origin: (i32, i32),
    pub size: (u32, u32),
}

/// For each monitor in a mirror, the monitors it mirrors.
pub type Mirrors = BTreeMap<String, BTreeSet<String>>;

/// What it takes to put the layout right.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    /// Outputs (by index) to move onto a desktop of their own, one new desktop per group.
    pub splits: Vec<Vec<usize>>,
    /// Subtracted from every desktop's origin, so the main display lands on (0,0).
    pub shift: (i32, i32),
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.splits.is_empty() && self.shift == (0, 0)
    }
}

/// The outputs grouped by where their desktop sits, so each group is one mirror (or one screen),
/// in the order Windows listed them.
fn groups(outputs: &[Output]) -> Vec<Vec<usize>> {
    let mut out: Vec<((i32, i32), Vec<usize>)> = Vec::new();
    for (i, o) in outputs.iter().enumerate() {
        match out.iter_mut().find(|(at, _)| *at == o.origin) {
            Some((_, g)) => g.push(i),
            None => out.push((o.origin, vec![i])),
        }
    }
    out.into_iter().map(|(_, g)| g).collect()
}

pub fn mirrors(outputs: &[Output]) -> Mirrors {
    let mut m = Mirrors::new();
    for g in groups(outputs).into_iter().filter(|g| g.len() > 1) {
        for &i in &g {
            let others = g.iter().filter(|&&j| j != i).map(|&j| outputs[j].key.clone()).collect();
            m.insert(outputs[i].key.clone(), others);
        }
    }
    m
}

/// The main display: the monitor showing the desktop at (0,0). When that desktop is mirrored,
/// the one `prefer` picks (the built-in screen, which outlasts a projector), else the first.
pub fn main_key(outputs: &[Output], prefer: impl Fn(usize) -> bool) -> Option<String> {
    let at_origin: Vec<usize> = (0..outputs.len()).filter(|&i| outputs[i].origin == (0, 0)).collect();
    at_origin.iter().copied().find(|&i| prefer(i)).or(at_origin.first().copied()).map(|i| outputs[i].key.clone())
}

fn mirrored(baseline: &Mirrors, a: &str, b: &str) -> bool {
    baseline.get(a).is_some_and(|s| s.contains(b))
}

/// Mirrors that weren't there at the start are split back out, and the main display goes
/// back to (0,0). A mirror that was there at the start is left alone, and the main display is
/// never the one split off.
pub fn plan(outputs: &[Output], baseline: &Mirrors, main: &str) -> Plan {
    let mut splits = Vec::new();
    for g in groups(outputs).into_iter().filter(|g| g.len() > 1) {
        let key = |i: usize| outputs[i].key.as_str();
        // The output that keeps this desktop: the main display, else one that was mirrored
        // with another here at the start, else the first.
        let anchor = g
            .iter()
            .copied()
            .find(|&i| key(i) == main)
            .or_else(|| g.iter().copied().find(|&i| g.iter().any(|&j| j != i && mirrored(baseline, key(i), key(j)))))
            .unwrap_or(g[0]);
        let mut rest: Vec<usize> = g.iter().copied().filter(|&i| i != anchor && !mirrored(baseline, key(anchor), key(i))).collect();
        // What's split off keeps any mirror it had at the start among itself.
        while !rest.is_empty() {
            let first = rest.remove(0);
            let mut group = vec![first];
            rest.retain(|&j| {
                let with = mirrored(baseline, key(first), key(j));
                if with {
                    group.push(j);
                }
                !with
            });
            splits.push(group);
        }
    }
    let shift = outputs.iter().find(|o| o.key == main).map(|o| o.origin).unwrap_or((0, 0));
    Plan { splits, shift }
}

/// Where a desktop split out of a mirror goes: to the right of everything, after the shift.
pub fn right_edge(outputs: &[Output], shift: (i32, i32)) -> i32 {
    outputs.iter().map(|o| o.origin.0 - shift.0 + o.size.0 as i32).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(key: &str, source: u32, origin: (i32, i32)) -> Output {
        Output { key: key.into(), source: (7, source), origin, size: (1920, 1080) }
    }

    #[test]
    fn nothing_to_do() {
        let o = vec![out("laptop", 0, (0, 0)), out("projector", 1, (1920, 0))];
        assert!(plan(&o, &mirrors(&o), "laptop").is_empty());
    }

    #[test]
    fn a_new_mirror_is_split_and_the_main_display_keeps_its_desktop() {
        let start = vec![out("laptop", 0, (0, 0))];
        let baseline = mirrors(&start);
        // A projector arrives and Windows duplicates the laptop onto it, listing it first.
        let now = vec![out("projector", 0, (0, 0)), out("laptop", 0, (0, 0))];
        let p = plan(&now, &baseline, "laptop");
        assert_eq!(p.splits, vec![vec![0]]);
        assert_eq!(p.shift, (0, 0));
        assert_eq!(right_edge(&now, p.shift), 1920);
    }

    #[test]
    fn a_mirror_across_two_adapters_is_a_mirror_too() {
        // Each screen has its own source, both at the origin.
        let o = vec![out("laptop", 0, (0, 0)), Output { source: (9, 0), ..out("projector", 0, (0, 0)) }];
        assert_eq!(mirrors(&o).len(), 2);
        assert_eq!(plan(&o, &Mirrors::new(), "laptop").splits, vec![vec![1]]);
    }

    #[test]
    fn a_mirror_from_the_start_is_left_alone() {
        let start = vec![out("laptop", 0, (0, 0)), out("confidence", 0, (0, 0)), out("projector", 1, (1920, 0))];
        let baseline = mirrors(&start);
        assert!(plan(&start, &baseline, "laptop").is_empty());
        // The confidence monitor drops out and comes back mirrored again: still fine.
        let again = vec![out("confidence", 0, (0, 0)), out("laptop", 0, (0, 0)), out("projector", 1, (1920, 0))];
        assert!(plan(&again, &baseline, "laptop").is_empty());
        // But the projector joining that mirror is split off.
        let joined = vec![out("laptop", 0, (0, 0)), out("confidence", 0, (0, 0)), out("projector", 0, (0, 0))];
        assert_eq!(plan(&joined, &baseline, "laptop").splits, vec![vec![2]]);
    }

    #[test]
    fn whatever_is_split_keeps_its_own_old_mirror() {
        // Two stage monitors mirrored to each other at the start, then everything duplicated.
        let start = vec![out("laptop", 0, (0, 0)), out("stageL", 1, (1920, 0)), out("stageR", 1, (1920, 0))];
        let baseline = mirrors(&start);
        let all = vec![out("laptop", 0, (0, 0)), out("stageL", 0, (0, 0)), out("stageR", 0, (0, 0))];
        assert_eq!(plan(&all, &baseline, "laptop").splits, vec![vec![1, 2]]);
    }

    #[test]
    fn the_main_display_goes_back_to_the_origin() {
        // Something made the projector main: the laptop now sits at (-1920, 0).
        let o = vec![out("laptop", 0, (-1920, 0)), out("projector", 1, (0, 0))];
        let p = plan(&o, &Mirrors::new(), "laptop");
        assert!(p.splits.is_empty());
        assert_eq!(p.shift, (-1920, 0));
        assert_eq!(right_edge(&o, p.shift), 3840);
    }

    #[test]
    fn a_missing_main_display_moves_nothing() {
        let o = vec![out("projector", 1, (0, 0))];
        assert!(plan(&o, &Mirrors::new(), "laptop").is_empty());
    }

    #[test]
    fn main_prefers_the_built_in_screen_in_a_mirror() {
        let o = vec![out("projector", 0, (0, 0)), out("laptop", 0, (0, 0)), out("side", 1, (1920, 0))];
        assert_eq!(main_key(&o, |i| i == 1).as_deref(), Some("laptop"));
        assert_eq!(main_key(&o, |_| false).as_deref(), Some("projector"));
    }
}
