//! "Find the broken mod": a guided binary search over an instance's mods.
//!
//! Each round turns off part of the remaining suspects (plus anything that
//! depends on them, so the game can still start). The player launches and
//! says whether the problem is still there; the suspects halve each time.
//! Calcite turns everything back on at the end.

use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use crate::health::ModInfo;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bisect {
    /// Files that may still be the culprit.
    pub suspects: Vec<String>,
    /// Files turned off for the current round.
    pub off: Vec<String>,
    /// Every file this search has turned off at some point (to restore).
    pub touched: Vec<String>,
    pub round: u32,
    /// What each file needs: one entry per dependency, listing the files
    /// that provide it. Any one of them being on is enough.
    needs: Vec<(String, Vec<Vec<String>>)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Step {
    /// Turn these off (and the rest of the suspects on), then launch.
    Test {
        turn_off: Vec<String>,
        turn_on: Vec<String>,
    },
    /// The search narrowed it down to one file.
    Found(String),
    /// Narrowed down to files that need each other, so they can't be
    /// tested apart.
    FoundGroup(Vec<String>),
    /// The problem happens even with every suspect turned off.
    NotAMod,
}

impl Bisect {
    /// Start a search over the enabled mods (libraries included as
    /// suspects; anything can be broken).
    pub fn start(mods: &[ModInfo]) -> Option<(Bisect, Step)> {
        let enabled: Vec<&ModInfo> = mods.iter().filter(|m| m.enabled).collect();
        if enabled.len() < 2 {
            return None;
        }
        let norm = |s: &str| s.to_ascii_lowercase().replace('-', "_");
        let needs = enabled
            .iter()
            .map(|m| {
                let deps: Vec<Vec<String>> = m
                    .depends
                    .iter()
                    // A mod that bundles what it needs doesn't need anyone else for it.
                    .filter(|d| !m.provided.iter().any(|p| norm(p) == norm(&d.id)))
                    .map(|d| {
                        enabled
                            .iter()
                            .filter(|other| other.file != m.file)
                            .filter(|other| other.provided.iter().any(|p| norm(p) == norm(&d.id)))
                            .map(|other| other.file.clone())
                            .collect::<Vec<String>>()
                    })
                    .filter(|providers| !providers.is_empty())
                    .collect();
                (m.file.clone(), deps)
            })
            .collect();
        let mut bisect = Bisect {
            suspects: enabled.iter().map(|m| m.file.clone()).collect(),
            off: Vec::new(),
            touched: Vec::new(),
            round: 0,
            needs,
        };
        bisect.suspects.sort();
        let step = bisect.plan();
        Some((bisect, step))
    }

    /// Rough number of launches left.
    pub fn rounds_left(&self) -> u32 {
        (self.suspects.len().max(1) as f64).log2().ceil() as u32
    }

    /// `files` plus everything that can't load without them: anything
    /// with a dependency whose every provider is turned off.
    fn with_dependents(&self, files: &[String]) -> BTreeSet<String> {
        let mut set: BTreeSet<String> = files.iter().cloned().collect();
        loop {
            let before = set.len();
            for (file, deps) in &self.needs {
                if !set.contains(file)
                    && deps
                        .iter()
                        .any(|providers| providers.iter().all(|p| set.contains(p)))
                {
                    set.insert(file.clone());
                }
            }
            if set.len() == before {
                return set;
            }
        }
    }

    /// `files` plus the mods that can't load without them, for keeping a
    /// culprit off at the end.
    pub fn and_dependents(&self, files: &[String]) -> Vec<String> {
        self.with_dependents(files).into_iter().collect()
    }

    /// Pick the next group to turn off.
    fn plan(&mut self) -> Step {
        if self.suspects.len() == 1 {
            return Step::Found(self.suspects[0].clone());
        }
        let target = self.suspects.len() / 2;
        // Leaves first: a mod nothing else needs can always be turned off
        // on its own, so the groups stay as small as possible.
        let dependents = |file: &String| {
            self.needs
                .iter()
                .filter(|(other, deps)| {
                    self.suspects.contains(other)
                        && deps.iter().any(|providers| providers.contains(file))
                })
                .count()
        };
        let mut order: Vec<&String> = self.suspects.iter().collect();
        order.sort_by_key(|f| (dependents(f), (*f).clone()));
        let suspects_in =
            |group: &BTreeSet<String>| self.suspects.iter().filter(|s| group.contains(*s)).count();
        let mut chosen: Vec<String> = Vec::new();
        let mut group = BTreeSet::new();
        for file in order {
            if suspects_in(&group) >= target {
                break;
            }
            if group.contains(file) {
                continue;
            }
            let mut trial = chosen.clone();
            trial.push(file.clone());
            let trial_group = self.with_dependents(&trial);
            // Never turn off every suspect: that tells us nothing.
            if suspects_in(&trial_group) == self.suspects.len() {
                continue;
            }
            chosen = trial;
            group = trial_group;
        }
        if chosen.is_empty() {
            // Every way to split them breaks a dependency: they need each other.
            return Step::FoundGroup(self.suspects.clone());
        }
        let suspects: HashSet<&String> = self.suspects.iter().collect();
        let off: Vec<String> = group.into_iter().collect();
        let turn_on: Vec<String> = self
            .off
            .iter()
            .filter(|f| !off.contains(f) && suspects.contains(f))
            .cloned()
            .collect();
        for file in &off {
            if !self.touched.contains(file) {
                self.touched.push(file.clone());
            }
        }
        self.off = off.clone();
        self.round += 1;
        Step::Test {
            turn_off: off,
            turn_on,
        }
    }

    /// Record the result of the current round.
    pub fn answer(&mut self, still_broken: bool) -> Step {
        let off: HashSet<&String> = self.off.iter().collect();
        if still_broken {
            // The culprit is among what stayed on.
            let remaining: Vec<String> = self
                .suspects
                .iter()
                .filter(|s| !off.contains(s))
                .cloned()
                .collect();
            if remaining.is_empty() {
                return Step::NotAMod;
            }
            self.suspects = remaining;
        } else {
            // Turning that group off fixed it: the culprit is in the group.
            self.suspects.retain(|s| off.contains(s));
        }
        self.plan()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::{Dependency, ModInfo};

    fn m(file: &str, id: &str, needs: &[&str]) -> ModInfo {
        ModInfo {
            file: file.into(),
            id: id.into(),
            name: id.into(),
            version: "1".into(),
            loaders: vec![crate::health::LoaderKind::Fabric],
            depends: needs
                .iter()
                .map(|n| Dependency {
                    id: n.to_string(),
                    range: "*".into(),
                })
                .collect(),
            breaks: vec![],
            provided: vec![id.into()],
            enabled: true,
        }
    }

    fn bundles(file: &str, id: &str, also: &[&str], needs: &[&str]) -> ModInfo {
        let mut info = m(file, id, needs);
        info.provided.extend(also.iter().map(|s| s.to_string()));
        info
    }

    /// Simulate a player: the game is broken while `culprit` is on.
    fn run(mods: &[ModInfo], culprit: &str) -> (Step, u32) {
        let (mut b, mut step) = Bisect::start(mods).unwrap();
        let mut enabled: HashSet<String> = mods.iter().map(|m| m.file.clone()).collect();
        loop {
            match step {
                Step::Test { turn_off, turn_on } => {
                    for f in &turn_off {
                        enabled.remove(f);
                    }
                    for f in turn_on {
                        enabled.insert(f);
                    }
                    let broken = enabled.contains(culprit);
                    step = b.answer(broken);
                }
                done => return (done, b.round),
            }
        }
    }

    #[test]
    fn finds_the_culprit_in_log_n_rounds() {
        let mods: Vec<ModInfo> = (0..64)
            .map(|i| m(&format!("mod{i:02}.jar"), &format!("mod{i:02}"), &[]))
            .collect();
        for culprit in ["mod00.jar", "mod37.jar", "mod63.jar"] {
            let (step, rounds) = run(&mods, culprit);
            assert_eq!(step, Step::Found(culprit.into()));
            assert!(rounds <= 6, "{rounds} rounds");
        }
    }

    #[test]
    fn dependents_go_off_with_their_dependency() {
        // b and c need a; turning a off must turn b and c off too.
        let mods = vec![
            m("a.jar", "a", &[]),
            m("b.jar", "b", &["a"]),
            m("c.jar", "c", &["a"]),
            m("d.jar", "d", &[]),
            m("e.jar", "e", &[]),
            m("f.jar", "f", &[]),
        ];
        // In every round of every search, if a.jar is off then b and c are off.
        for culprit in ["a.jar", "b.jar", "c.jar", "d.jar", "e.jar", "f.jar"] {
            let (mut b, mut step) = Bisect::start(&mods).unwrap();
            let mut enabled: HashSet<String> = mods.iter().map(|m| m.file.clone()).collect();
            while let Step::Test { turn_off, turn_on } = step {
                for f in &turn_off {
                    enabled.remove(f);
                }
                for f in turn_on {
                    enabled.insert(f);
                }
                if !enabled.contains("a.jar") {
                    assert!(!enabled.contains("b.jar") && !enabled.contains("c.jar"));
                }
                step = b.answer(enabled.contains(culprit));
            }
            assert_eq!(step, Step::Found(culprit.into()), "culprit {culprit}");
        }
        for culprit in ["b.jar", "e.jar"] {
            let (step, _) = run(&mods, culprit);
            assert_eq!(step, Step::Found(culprit.into()), "culprit {culprit}");
        }
    }

    #[test]
    fn a_module_two_mods_bundle_ties_neither() {
        // Sodium and Mod Menu both bundle "screen-api" and need it; neither
        // needs the other for it. Iris needs Sodium.
        let mods = vec![
            bundles("sodium.jar", "sodium", &["screen-api"], &["screen-api"]),
            bundles("modmenu.jar", "modmenu", &["screen-api"], &["screen-api"]),
            m("iris.jar", "iris", &["sodium"]),
            m("lithium.jar", "lithium", &[]),
            m("other.jar", "other", &["screen-api"]),
        ];
        for culprit in [
            "sodium.jar",
            "modmenu.jar",
            "iris.jar",
            "lithium.jar",
            "other.jar",
        ] {
            let (step, _) = run(&mods, culprit);
            assert_eq!(step, Step::Found(culprit.into()), "culprit {culprit}");
        }
    }

    #[test]
    fn mods_that_need_each_other_are_reported_together() {
        let mods = vec![
            m("a.jar", "a", &["b"]),
            m("b.jar", "b", &["a"]),
            m("c.jar", "c", &[]),
            m("d.jar", "d", &[]),
        ];
        let (step, _) = run(&mods, "a.jar");
        assert_eq!(step, Step::FoundGroup(vec!["a.jar".into(), "b.jar".into()]));
        let (step, _) = run(&mods, "d.jar");
        assert_eq!(step, Step::Found("d.jar".into()));
    }

    #[test]
    fn says_so_when_no_mod_is_to_blame() {
        let mods: Vec<ModInfo> = (0..4)
            .map(|i| m(&format!("m{i}.jar"), &format!("m{i}"), &[]))
            .collect();
        let (mut b, _) = Bisect::start(&mods).unwrap();
        // Broken whatever is turned off.
        let mut step = b.answer(true);
        while let Step::Test { .. } = step {
            step = b.answer(true);
        }
        // Ends at a single remaining suspect or NotAMod; never panics.
        assert!(matches!(
            step,
            Step::Found(_) | Step::FoundGroup(_) | Step::NotAMod
        ));
    }
}
