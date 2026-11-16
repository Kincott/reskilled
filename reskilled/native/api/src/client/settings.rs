//! The options' choices: the player's display settings and the server's rules, each with its name, value in words,
//! what it does, and the next value a click picks.

use horizon_client::mod_api::Text;

use skills_core::rules::{CostUnit, Progression, Rules, Sharing};

use super::cache::{ClientSettings, Corner, WarningPlace};
use crate::text::{
    HELP_CHIMES, HELP_CLICKS, HELP_CORNER, HELP_COST, HELP_DIM, HELP_EARN_RATE, HELP_ENCHANT,
    HELP_HIDE_MET, HELP_OWN, HELP_PLACE, HELP_POINTS, HELP_PROGRESSION, HELP_REFUSAL, HELP_SHARING,
    HELP_SHOW, HELP_STRIP, HELP_SWITCHES, HELP_TIME, Message, OPT_CHIMES, OPT_CLICKS, OPT_CORNER,
    OPT_DIM, OPT_HIDE_MET, OPT_OWN, OPT_PLACE, OPT_REFUSAL, OPT_SHOW, OPT_STRIP, OPT_TIME,
    RULE_COST, RULE_EARN_RATE, RULE_ENCHANT, RULE_POINTS, RULE_PROGRESSION, RULE_SHARING,
    RULE_SWITCHES, SECTION_ICON, SECTION_SOUNDS, SECTION_STRIP, SECTION_TOOLTIPS, SECTION_WARNING,
    VAL_ABOVE, VAL_BOTH, VAL_BOTTOM_LEFT, VAL_BOTTOM_RIGHT, VAL_BUY, VAL_EARN, VAL_EVERYONE,
    VAL_LEVELS, VAL_OFF, VAL_ON, VAL_PARTY, VAL_POINTS, VAL_SECONDS, VAL_TOP, VAL_TOP_LEFT,
    VAL_TOP_RIGHT, number, say,
};

/// The warning's lengths a click goes through, in seconds.
const WARNING_SECONDS: [u32; 3] = [2, 3, 5];

/// One setting the page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    WarningPlace,
    WarningSeconds,
    LockIcon,
    LockCorner,
    DimLocked,
    HideMet,
    OwnLevel,
    SkillStrip,
    ClickSounds,
    ProgressSounds,
    RefusalSound,
}

/// The display settings, in the options' first tab's order.
pub const CHOICES: [Choice; 11] = [
    Choice::WarningPlace,
    Choice::WarningSeconds,
    Choice::LockIcon,
    Choice::LockCorner,
    Choice::DimLocked,
    Choice::HideMet,
    Choice::OwnLevel,
    Choice::SkillStrip,
    Choice::ClickSounds,
    Choice::ProgressSounds,
    Choice::RefusalSound,
];

/// The section of the options a setting sits in.
pub fn section(choice: Choice) -> &'static Message {
    match choice {
        Choice::WarningPlace | Choice::WarningSeconds => &SECTION_WARNING,
        Choice::LockIcon | Choice::LockCorner | Choice::DimLocked => &SECTION_ICON,
        Choice::HideMet | Choice::OwnLevel => &SECTION_TOOLTIPS,
        Choice::SkillStrip => &SECTION_STRIP,
        Choice::ClickSounds | Choice::ProgressSounds | Choice::RefusalSound => &SECTION_SOUNDS,
    }
}

fn on_off(on: bool) -> Text {
    say(if on { &VAL_ON } else { &VAL_OFF }, &[])
}

impl Choice {
    pub fn name(self) -> &'static Message {
        match self {
            Choice::WarningPlace => &OPT_PLACE,
            Choice::WarningSeconds => &OPT_TIME,
            Choice::LockIcon => &OPT_SHOW,
            Choice::LockCorner => &OPT_CORNER,
            Choice::DimLocked => &OPT_DIM,
            Choice::HideMet => &OPT_HIDE_MET,
            Choice::OwnLevel => &OPT_OWN,
            Choice::SkillStrip => &OPT_STRIP,
            Choice::ClickSounds => &OPT_CLICKS,
            Choice::ProgressSounds => &OPT_CHIMES,
            Choice::RefusalSound => &OPT_REFUSAL,
        }
    }

    /// What the setting does, for its tooltip.
    pub fn help(self) -> &'static Message {
        match self {
            Choice::WarningPlace => &HELP_PLACE,
            Choice::WarningSeconds => &HELP_TIME,
            Choice::LockIcon => &HELP_SHOW,
            Choice::LockCorner => &HELP_CORNER,
            Choice::DimLocked => &HELP_DIM,
            Choice::HideMet => &HELP_HIDE_MET,
            Choice::OwnLevel => &HELP_OWN,
            Choice::SkillStrip => &HELP_STRIP,
            Choice::ClickSounds => &HELP_CLICKS,
            Choice::ProgressSounds => &HELP_CHIMES,
            Choice::RefusalSound => &HELP_REFUSAL,
        }
    }

    /// The setting's value, in words.
    pub fn value(self, s: &ClientSettings) -> Text {
        match self {
            Choice::WarningPlace => say(
                match s.warning {
                    WarningPlace::AboveHotbar => &VAL_ABOVE,
                    WarningPlace::Top => &VAL_TOP,
                    WarningPlace::Off => &VAL_OFF,
                },
                &[],
            ),
            Choice::WarningSeconds => say(&VAL_SECONDS, &[number(s.warning_seconds)]),
            Choice::LockIcon => on_off(s.lock_icon),
            Choice::LockCorner => say(
                match s.lock_corner {
                    Corner::BottomLeft => &VAL_BOTTOM_LEFT,
                    Corner::TopLeft => &VAL_TOP_LEFT,
                    Corner::TopRight => &VAL_TOP_RIGHT,
                    Corner::BottomRight => &VAL_BOTTOM_RIGHT,
                },
                &[],
            ),
            Choice::DimLocked => on_off(s.dim_locked),
            Choice::HideMet => on_off(s.hide_met_requirements),
            Choice::OwnLevel => on_off(s.show_own_level),
            Choice::SkillStrip => on_off(s.skill_strip),
            Choice::ClickSounds => on_off(s.click_sounds),
            Choice::ProgressSounds => on_off(s.progress_sounds),
            Choice::RefusalSound => on_off(s.refusal_sound),
        }
    }

    /// Moves the setting to its next value, round to the first after the last.
    pub fn next(self, s: &mut ClientSettings) {
        match self {
            Choice::WarningPlace => {
                s.warning = match s.warning {
                    WarningPlace::AboveHotbar => WarningPlace::Top,
                    WarningPlace::Top => WarningPlace::Off,
                    WarningPlace::Off => WarningPlace::AboveHotbar,
                }
            }
            Choice::WarningSeconds => {
                let at = WARNING_SECONDS.iter().position(|n| *n == s.warning_seconds);
                s.warning_seconds =
                    WARNING_SECONDS[at.map_or(0, |i| (i + 1) % WARNING_SECONDS.len())];
            }
            Choice::LockIcon => s.lock_icon = !s.lock_icon,
            Choice::LockCorner => {
                s.lock_corner = match s.lock_corner {
                    Corner::BottomLeft => Corner::TopLeft,
                    Corner::TopLeft => Corner::TopRight,
                    Corner::TopRight => Corner::BottomRight,
                    Corner::BottomRight => Corner::BottomLeft,
                }
            }
            Choice::DimLocked => s.dim_locked = !s.dim_locked,
            Choice::HideMet => s.hide_met_requirements = !s.hide_met_requirements,
            Choice::OwnLevel => s.show_own_level = !s.show_own_level,
            Choice::SkillStrip => s.skill_strip = !s.skill_strip,
            Choice::ClickSounds => s.click_sounds = !s.click_sounds,
            Choice::ProgressSounds => s.progress_sounds = !s.progress_sounds,
            Choice::RefusalSound => s.refusal_sound = !s.refusal_sound,
        }
    }
}

/// One of the server's rules the options' second tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleChoice {
    Cost,
    PointsPerLevel,
    Progression,
    EarnRate,
    PerkSwitches,
    EnchantmentLocks,
    Sharing,
}

/// The rules, in the tab's order.
pub const RULES: [RuleChoice; 7] = [
    RuleChoice::Cost,
    RuleChoice::PointsPerLevel,
    RuleChoice::Progression,
    RuleChoice::EarnRate,
    RuleChoice::PerkSwitches,
    RuleChoice::EnchantmentLocks,
    RuleChoice::Sharing,
];

const POINTS_PER_LEVEL: [u32; 3] = [10, 20, 50];
const EARN_RATES: [u32; 3] = [50, 100, 200];

/// The value after `now` in `values`, round to the first.
fn after(values: &[u32], now: u32) -> u32 {
    let at = values.iter().position(|v| *v == now);
    values[at.map_or(0, |i| (i + 1) % values.len())]
}

impl RuleChoice {
    pub fn name(self) -> &'static Message {
        match self {
            RuleChoice::Cost => &RULE_COST,
            RuleChoice::PointsPerLevel => &RULE_POINTS,
            RuleChoice::Progression => &RULE_PROGRESSION,
            RuleChoice::EarnRate => &RULE_EARN_RATE,
            RuleChoice::PerkSwitches => &RULE_SWITCHES,
            RuleChoice::EnchantmentLocks => &RULE_ENCHANT,
            RuleChoice::Sharing => &RULE_SHARING,
        }
    }

    pub fn help(self) -> &'static Message {
        match self {
            RuleChoice::Cost => &HELP_COST,
            RuleChoice::PointsPerLevel => &HELP_POINTS,
            RuleChoice::Progression => &HELP_PROGRESSION,
            RuleChoice::EarnRate => &HELP_EARN_RATE,
            RuleChoice::PerkSwitches => &HELP_SWITCHES,
            RuleChoice::EnchantmentLocks => &HELP_ENCHANT,
            RuleChoice::Sharing => &HELP_SHARING,
        }
    }

    pub fn value(self, r: &Rules) -> Text {
        let percent = |p: u32| Text::literal(format!("{p}%"));
        match self {
            RuleChoice::Cost => say(
                match r.cost_unit {
                    CostUnit::Levels => &VAL_LEVELS,
                    CostUnit::Points => &VAL_POINTS,
                },
                &[],
            ),
            RuleChoice::PointsPerLevel => number(r.points_per_level),
            RuleChoice::Progression => say(
                match r.progression {
                    Progression::Buy => &VAL_BUY,
                    Progression::Earn => &VAL_EARN,
                    Progression::Both => &VAL_BOTH,
                },
                &[],
            ),
            RuleChoice::EarnRate => percent(r.earn_rate),
            RuleChoice::PerkSwitches => on_off(r.perk_switches),
            RuleChoice::EnchantmentLocks => on_off(r.enchantment_locks),
            RuleChoice::Sharing => say(
                match r.sharing {
                    Sharing::Off => &VAL_OFF,
                    Sharing::Party => &VAL_PARTY,
                    Sharing::Server => &VAL_EVERYONE,
                },
                &[],
            ),
        }
    }

    /// Moves the rule to its next value, round to the first after the last.
    pub fn next(self, r: &mut Rules) {
        match self {
            RuleChoice::Cost => {
                r.cost_unit = match r.cost_unit {
                    CostUnit::Levels => CostUnit::Points,
                    CostUnit::Points => CostUnit::Levels,
                }
            }
            RuleChoice::PointsPerLevel => {
                r.points_per_level = after(&POINTS_PER_LEVEL, r.points_per_level)
            }
            RuleChoice::Progression => {
                r.progression = match r.progression {
                    Progression::Buy => Progression::Earn,
                    Progression::Earn => Progression::Both,
                    Progression::Both => Progression::Buy,
                }
            }
            RuleChoice::EarnRate => r.earn_rate = after(&EARN_RATES, r.earn_rate),
            RuleChoice::PerkSwitches => r.perk_switches = !r.perk_switches,
            RuleChoice::EnchantmentLocks => r.enchantment_locks = !r.enchantment_locks,
            RuleChoice::Sharing => {
                r.sharing = match r.sharing {
                    Sharing::Off => Sharing::Party,
                    Sharing::Party => Sharing::Server,
                    Sharing::Server => Sharing::Off,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::cache::SETTINGS_FILE;
    use crate::test_host::{TestHost, exclusive};

    #[test]
    fn every_choice_comes_back_round_to_where_it_started() {
        // Given the default settings
        let start = ClientSettings::default();

        for choice in CHOICES {
            // When the choice is clicked until it repeats
            let mut s = start.clone();
            let mut seen = vec![choice.value(&s).plain()];
            loop {
                choice.next(&mut s);
                let value = choice.value(&s).plain();
                if s == start {
                    break;
                }
                assert!(
                    !seen.contains(&value),
                    "{choice:?} repeats {value} before coming back"
                );
                seen.push(value);
            }

            // Then it went through at least two values and changed nothing else
            assert!(seen.len() >= 2, "{choice:?}");
        }
    }

    #[test]
    fn every_rule_comes_back_round_to_where_it_started() {
        // Given the default rules
        let start = Rules::default();

        for rule in RULES {
            // When the rule is clicked until it repeats
            let mut r = start.clone();
            let mut seen = vec![rule.value(&r).plain()];
            loop {
                rule.next(&mut r);
                if r == start {
                    break;
                }
                let value = rule.value(&r).plain();
                assert!(!seen.contains(&value), "{rule:?} repeats {value}");
                seen.push(value);
            }

            // Then it went through at least two values
            assert!(seen.len() >= 2, "{rule:?}");
        }
    }

    #[test]
    fn the_warning_goes_above_the_hotbar_then_top_then_off_and_lasts_2_3_or_5_seconds() {
        // Given the default settings
        let mut s = ClientSettings::default();

        // When the place and the length are clicked through
        let mut places = vec![Choice::WarningPlace.value(&s).plain()];
        let mut lengths = vec![Choice::WarningSeconds.value(&s).plain()];
        for _ in 0..2 {
            Choice::WarningPlace.next(&mut s);
            places.push(Choice::WarningPlace.value(&s).plain());
            Choice::WarningSeconds.next(&mut s);
            lengths.push(Choice::WarningSeconds.value(&s).plain());
        }

        // Then they read as the player sees them
        assert_eq!(places, ["Above hotbar", "Top", "Off"]);
        assert_eq!(lengths, ["3 s", "5 s", "2 s"]);
    }

    #[test]
    fn settings_are_saved_and_read_back() {
        // Given a config folder and changed settings
        let _serial = exclusive();
        let dir = std::env::temp_dir().join(format!("reskilled_settings_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let host = TestHost {
            config: Some(dir.clone()),
            ..TestHost::default()
        };
        let mut s = ClientSettings::default();
        Choice::LockCorner.next(&mut s);
        Choice::RefusalSound.next(&mut s);

        // When they are saved and read back, and a file from the first version is read
        s.save(&host);
        let read = ClientSettings::load(&host);
        std::fs::write(
            dir.join(SETTINGS_FILE),
            r#"{"hide_met_requirements": true}"#,
        )
        .unwrap();
        let old = ClientSettings::load(&host);
        std::fs::remove_dir_all(&dir).unwrap();

        // Then the changes come back, and the old file keeps its one setting with the rest at their defaults
        assert_eq!(read, s);
        assert_eq!(
            old,
            ClientSettings {
                hide_met_requirements: true,
                ..ClientSettings::default()
            }
        );
    }
}
