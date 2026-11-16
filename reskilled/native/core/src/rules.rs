//! The server's rules that operators change in game, on the screen's settings page: how raises are paid, whether
//! perks can be switched off, enchantment locks, shared skills and how skills are earned. Saved with
//! the world and sent to every player.

use serde::{Deserialize, Serialize};

/// What a raise costs: XP levels (worth more the higher the player is) or XP points (the same at any level).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostUnit {
    Levels,
    Points,
}

/// Who shares skill levels: nobody, the members of a party (`/skills party`), or everyone on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sharing {
    Off,
    Party,
    Server,
}

/// How skills grow: bought with XP, earned by doing the skill's work, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Progression {
    Buy,
    Earn,
    Both,
}

/// The rules. A world without saved rules plays by the defaults; a rule a saved world names that this version no
/// longer has (the refund rules) is passed over, so the rest are kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub cost_unit: CostUnit,
    /// With `cost_unit` points: XP points for each level of a raise's cost.
    pub points_per_level: u32,
    /// Players may switch their perks' effects off and on.
    pub perk_switches: bool,
    /// Enchanted gear needs the levels the enchantment locks name.
    pub enchantment_locks: bool,
    pub sharing: Sharing,
    pub progression: Progression,
    /// How fast skills are earned by doing, in percent of the normal pace.
    pub earn_rate: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            cost_unit: CostUnit::Levels,
            points_per_level: 20,
            perk_switches: true,
            enchantment_locks: true,
            sharing: Sharing::Off,
            progression: Progression::Buy,
            earn_rate: 100,
        }
    }
}

/// The ranges rules from outside (an operator's client, a saved file) are kept in, wider than the screen's choices.
const POINTS_PER_LEVEL: (u32, u32) = (1, 1000);
const EARN_RATE: (u32, u32) = (1, 1000);

impl Rules {
    /// These rules with every number inside its range: at least a point a level, a pace from 1 to 1000 percent.
    pub fn sanitized(mut self) -> Self {
        self.points_per_level = self
            .points_per_level
            .clamp(POINTS_PER_LEVEL.0, POINTS_PER_LEVEL.1);
        self.earn_rate = self.earn_rate.clamp(EARN_RATE.0, EARN_RATE.1);
        self
    }

    /// Whether skills can be bought with XP under these rules.
    pub fn buying(&self) -> bool {
        matches!(self.progression, Progression::Buy | Progression::Both)
    }

    /// Whether skills grow by doing their work under these rules.
    pub fn earning(&self) -> bool {
        matches!(self.progression, Progression::Earn | Progression::Both)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_from_outside_are_kept_in_safe_ranges() {
        // Given rules a modified client could send: free points and an absurd pace
        let wild = Rules {
            points_per_level: 0,
            earn_rate: u32::MAX,
            ..Rules::default()
        };
        let slow = Rules {
            earn_rate: 0,
            ..Rules::default()
        };

        // When they are made safe
        let (wild, slow) = (wild.sanitized(), slow.sanitized());

        // Then each value is brought inside its range, and the defaults pass unchanged
        assert_eq!((wild.points_per_level, wild.earn_rate), (1, 1000));
        assert_eq!(slow.earn_rate, 1);
        assert_eq!(Rules::default().sanitized(), Rules::default());
    }

    #[test]
    fn rules_round_trip_and_missing_fields_take_their_defaults() {
        // Given changed rules, and a saved file that names only one rule
        let rules = Rules {
            cost_unit: CostUnit::Points,
            sharing: Sharing::Party,
            progression: Progression::Both,
            ..Rules::default()
        };
        let partial = r#"{"perk_switches": false}"#;

        // When they are written and read back, and the partial file is read
        let back: Rules = serde_json::from_str(&serde_json::to_string(&rules).unwrap()).unwrap();
        let read: Rules = serde_json::from_str(partial).unwrap();

        // Then the rules come back equal, and the partial file keeps its one rule with the rest at their defaults
        assert_eq!(back, rules);
        assert_eq!(
            read,
            Rules {
                perk_switches: false,
                ..Rules::default()
            }
        );
        assert!(back.buying() && back.earning());
        assert!(Rules::default().buying() && !Rules::default().earning());
    }

    #[test]
    fn rules_saved_with_the_refund_rules_keep_the_rest() {
        // Given rules saved by a version that had perk refunds
        let saved =
            r#"{"cost_unit": "points", "respec": "free", "respec_cost": 7, "sharing": "party"}"#;

        // When they are read
        let read: Rules = serde_json::from_str(saved).unwrap();

        // Then the refund rules are passed over and the others are kept
        assert_eq!(
            read,
            Rules {
                cost_unit: CostUnit::Points,
                sharing: Sharing::Party,
                ..Rules::default()
            }
        );
    }
}
