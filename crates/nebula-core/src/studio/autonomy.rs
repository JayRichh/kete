//! AUTONOMY: how far a studio repo's agents may go without Jay, A0 to A3.
//!
//! The level is machine-local (`studio` in `config.local.json`) and only a
//! person changes it. Kete enforces it where it can: A0 refuses new spawns
//! into the repo, and A1 and A2 deny commands through Claude permission
//! rules on every session the orchestrator spawns there and every role
//! session ([`Autonomy::denies`]), which `kete _guard` applies again on
//! every shell command those sessions run.

use serde::{Deserialize, Serialize};

/// A studio repo's autonomy level. Travels as `"A0"` to `"A3"`.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Autonomy {
    /// Hold: no idle notes, and orchestrator and sibling spawns into the
    /// repo are refused.
    A0,
    /// Plan (the default): idle notes; no pushes, pull requests, ships or
    /// tags.
    #[default]
    A1,
    /// Deliver: idle notes; pushes and pull requests, no tags or releases.
    A2,
    /// Release: idle notes, nothing denied.
    A3,
}

/// A1's denies: push, pull request, release, tag and ship commands, as
/// Bash and PowerShell rules. A0 returns these too: the guard applies them
/// to sessions already running when the level drops to Hold.
const A1_DENIES: &[&str] = &[
    "Bash(git push *)",
    "Bash(gh pr *)",
    "Bash(gh release *)",
    "Bash(git tag *)",
    "Bash(kete ship *)",
    "Bash(kete ado pr *)",
    "Bash(bro ship *)",
    "Bash(bro ado pr *)",
    "PowerShell(git push *)",
    "PowerShell(gh pr *)",
    "PowerShell(gh release *)",
    "PowerShell(git tag *)",
    "PowerShell(kete ship *)",
    "PowerShell(kete ado pr *)",
    "PowerShell(bro ship *)",
    "PowerShell(bro ado pr *)",
];

/// A2's denies: tags, releases and tag pushes.
const A2_DENIES: &[&str] = &[
    "Bash(git tag *)",
    "Bash(gh release *)",
    "Bash(git push --tags *)",
    "Bash(git push * --tags*)",
    "Bash(git push --follow-tags *)",
    "Bash(git push * --follow-tags*)",
    "Bash(git push *refs/tags/*)",
    "PowerShell(git tag *)",
    "PowerShell(gh release *)",
    "PowerShell(git push --tags *)",
    "PowerShell(git push * --tags*)",
    "PowerShell(git push --follow-tags *)",
    "PowerShell(git push * --follow-tags*)",
    "PowerShell(git push *refs/tags/*)",
];

impl Autonomy {
    /// Every level, lowest first.
    pub const ALL: [Autonomy; 4] = [Autonomy::A0, Autonomy::A1, Autonomy::A2, Autonomy::A3];

    /// `A0` to `A3`, any case, trimmed; None for anything else.
    pub fn parse(word: &str) -> Option<Autonomy> {
        match word.trim().to_ascii_uppercase().as_str() {
            "A0" => Some(Autonomy::A0),
            "A1" => Some(Autonomy::A1),
            "A2" => Some(Autonomy::A2),
            "A3" => Some(Autonomy::A3),
            _ => None,
        }
    }

    /// `"A0"` to `"A3"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Autonomy::A0 => "A0",
            Autonomy::A1 => "A1",
            Autonomy::A2 => "A2",
            Autonomy::A3 => "A3",
        }
    }

    /// The level's name: Hold, Plan, Deliver or Release.
    pub fn label(self) -> &'static str {
        match self {
            Autonomy::A0 => "Hold",
            Autonomy::A1 => "Plan",
            Autonomy::A2 => "Deliver",
            Autonomy::A3 => "Release",
        }
    }

    /// Whether the orchestrator and sessions may start new sessions in the
    /// repo: every level but A0.
    pub fn allows_spawn(self) -> bool {
        self != Autonomy::A0
    }

    /// The Claude permission rules the level denies, Bash and PowerShell
    /// forms. A0 returns A1's list; A3 denies nothing.
    pub fn denies(self) -> &'static [&'static str] {
        match self {
            Autonomy::A0 | Autonomy::A1 => A1_DENIES,
            Autonomy::A2 => A2_DENIES,
            Autonomy::A3 => &[],
        }
    }
}

impl std::fmt::Display for Autonomy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.as_str(), self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_plan_and_words_parse_in_any_case() {
        assert_eq!(Autonomy::default(), Autonomy::A1);
        assert_eq!(Autonomy::parse(" a2 "), Some(Autonomy::A2));
        assert_eq!(Autonomy::parse("A3"), Some(Autonomy::A3));
        assert_eq!(Autonomy::parse("A4"), None);
        assert_eq!(Autonomy::parse("deliver"), None);
        for level in Autonomy::ALL {
            assert_eq!(Autonomy::parse(level.as_str()), Some(level));
        }
        assert_eq!(
            Autonomy::ALL.map(Autonomy::label),
            ["Hold", "Plan", "Deliver", "Release"]
        );
        assert_eq!(Autonomy::A2.to_string(), "A2 (Deliver)");
    }

    #[test]
    fn only_hold_refuses_spawns() {
        assert!(!Autonomy::A0.allows_spawn());
        assert!(Autonomy::A1.allows_spawn());
        assert!(Autonomy::A2.allows_spawn());
        assert!(Autonomy::A3.allows_spawn());
    }

    #[test]
    fn each_level_denies_exactly_its_rules() {
        let plan = [
            "Bash(git push *)",
            "Bash(gh pr *)",
            "Bash(gh release *)",
            "Bash(git tag *)",
            "Bash(kete ship *)",
            "Bash(kete ado pr *)",
            "Bash(bro ship *)",
            "Bash(bro ado pr *)",
            "PowerShell(git push *)",
            "PowerShell(gh pr *)",
            "PowerShell(gh release *)",
            "PowerShell(git tag *)",
            "PowerShell(kete ship *)",
            "PowerShell(kete ado pr *)",
            "PowerShell(bro ship *)",
            "PowerShell(bro ado pr *)",
        ];
        assert_eq!(Autonomy::A1.denies(), plan);
        assert_eq!(Autonomy::A0.denies(), plan, "Hold keeps Plan's denies");
        assert_eq!(
            Autonomy::A2.denies(),
            [
                "Bash(git tag *)",
                "Bash(gh release *)",
                "Bash(git push --tags *)",
                "Bash(git push * --tags*)",
                "Bash(git push --follow-tags *)",
                "Bash(git push * --follow-tags*)",
                "Bash(git push *refs/tags/*)",
                "PowerShell(git tag *)",
                "PowerShell(gh release *)",
                "PowerShell(git push --tags *)",
                "PowerShell(git push * --tags*)",
                "PowerShell(git push --follow-tags *)",
                "PowerShell(git push * --follow-tags*)",
                "PowerShell(git push *refs/tags/*)",
            ]
        );
        assert!(Autonomy::A3.denies().is_empty());
    }

    #[test]
    fn every_bash_rule_has_its_powershell_twin() {
        for level in Autonomy::ALL {
            let rules = level.denies();
            let bash: Vec<&str> = rules
                .iter()
                .filter_map(|r| r.strip_prefix("Bash("))
                .collect();
            let ps: Vec<&str> = rules
                .iter()
                .filter_map(|r| r.strip_prefix("PowerShell("))
                .collect();
            assert_eq!(bash, ps, "{level}");
            assert_eq!(bash.len() + ps.len(), rules.len(), "{level}");
        }
    }

    #[test]
    fn levels_travel_as_their_names() {
        assert_eq!(serde_json::to_string(&Autonomy::A2).unwrap(), "\"A2\"");
        let back: Autonomy = serde_json::from_str("\"A0\"").unwrap();
        assert_eq!(back, Autonomy::A0);
        let wire = rmp_serde::to_vec(&Autonomy::A3).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Autonomy>(&wire).unwrap(),
            Autonomy::A3
        );
    }
}
