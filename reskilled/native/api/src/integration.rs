//! What other mods may call: a player's level in a skill, whether they own a perk, and whether they may use an
//! ability a lock names (an ability is something with no item, block or entity behind it, such as a dash). Another
//! native mod lists Reskilled as an optional dependency and calls these behind `#[cfg(feature = "reskilled")]`;
//! see `INTEGRATION.md`. Call them on the server.

use horizon_client::mod_api::{Flow, Host};
use skills_core::defs::{Action, TargetKind};
use skills_core::progress::level;

use crate::server::data::current;
use crate::server::enforce::{Actor, guard, listed};
use crate::server::state::state_of;

/// The player's level in the skill: 1 when never raised, 0 for a skill the data does not have or a player whose
/// skills cannot be read.
pub fn skill_level(host: &mut dyn Host, player: u128, skill: &str) -> u32 {
    let Some(data) = current() else { return 0 };
    if data.defs.skill(skill).is_none() {
        return 0;
    }
    state_of(host, player).map_or(0, |state| level(&state, &data.defs, skill))
}

/// Whether the player owns the perk (switched on or off).
pub fn has_perk(host: &mut dyn Host, player: u128, perk: &str) -> bool {
    state_of(host, player).is_ok_and(|state| {
        state
            .skills
            .values()
            .any(|skill| skill.perks.contains(perk))
    })
}

/// Whether the player may use the ability (`dashmod:dash`): true when no lock names it or they have the levels it
/// needs, or when locks are off for them (creative or spectator, by the settings). When they may not, they get the
/// usual "Needs Agility 10" warning, as for any other lock. Run the ability only when this is true.
pub fn may_use_ability(host: &mut dyn Host, player: u128, ability: &str) -> bool {
    let exempt = listed(host, |p| p.uuid == player).is_some_and(|actor| actor.exempt);
    may_use_ability_as(host, player, exempt, ability)
}

/// [`may_use_ability`] for a player known to be exempt from locks or not.
fn may_use_ability_as(host: &mut dyn Host, player: u128, exempt: bool, ability: &str) -> bool {
    let actor = Actor {
        player,
        entity: 0,
        exempt,
        selected: 0,
    };
    guard(host, &actor, &[(TargetKind::Ability, ability, Action::Use)]) == Flow::Continue
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use skills_core::progress::{PlayerState, SkillProgress};

    use super::*;
    use crate::server::data::tests::load;
    use crate::server::state::set_state;
    use crate::test_host::{TestHost, exclusive};

    const PLAYER: u128 = 0x1234;
    const AGILITY: &str = "reskilled:agility";

    /// Agility known, a dash locked at Agility 10 and a perk to own, and the player at Agility `level`.
    fn world(level: u32, perks: &[&str]) -> TestHost {
        let mut host = TestHost::default();
        let agility = json!({"format": 1, "id": AGILITY, "name": "reskilled.skill.agility", "icon": "minecraft:feather"});
        let perk = json!({"format": 1, "id": "reskilled:agility_light_step", "skill": AGILITY, "name": "n",
            "description": "d", "cost": 2, "min_level": 4, "effects": []});
        let locks = json!({"format": 1, "entries": [{"target": {"ability": {"id": "dashmod:dash"}},
            "actions": ["use"], "requires": {AGILITY: 10}}]});
        load(
            &mut host,
            &[
                ("skills", vec![(AGILITY, agility)]),
                ("perks", vec![("reskilled:agility_light_step", perk)]),
                ("locks", vec![("dashmod:abilities", locks)]),
            ],
        );
        let mut state = PlayerState::default();
        let progress = SkillProgress {
            level,
            perks: perks.iter().map(|p| (*p).to_string()).collect(),
            ..SkillProgress::default()
        };
        state.skills.insert(AGILITY.into(), progress);
        set_state(&mut host, PLAYER, state).unwrap();
        host
    }

    #[test]
    fn may_use_ability_checks_the_level_and_says_what_is_missing() {
        // Given the dash locked at Agility 10, and players at Agility 9 and 10
        let _serial = exclusive();
        let mut nine = world(9, &[]);

        // When each asks to dash
        let refused = may_use_ability(&mut nine, PLAYER, "dashmod:dash");
        let mut ten = world(10, &[]);
        let allowed = may_use_ability(&mut ten, PLAYER, "dashmod:dash");

        // Then only the second may, and the first reads the usual line
        assert_eq!((refused, allowed), (false, true));
        assert_eq!(nine.action_bars_of(PLAYER), vec!["Needs Agility 10"]);
        assert!(ten.action_bars.is_empty());
    }

    #[test]
    fn an_ability_nothing_locks_is_free() {
        // Given a player at Agility 1
        let _serial = exclusive();
        let mut host = world(1, &[]);

        // When they ask to use an ability no lock names
        let allowed = may_use_ability(&mut host, PLAYER, "othermod:glide");

        // Then they may
        assert!(allowed);
    }

    #[test]
    fn players_whose_locks_are_off_may_use_locked_abilities() {
        // Given a creative player at Agility 1, whose locks are off
        let _serial = exclusive();
        let mut host = world(1, &[]);

        // When they ask to dash
        let allowed = may_use_ability_as(&mut host, PLAYER, true, "dashmod:dash");

        // Then they may
        assert!(allowed);
    }

    #[test]
    fn skill_level_and_has_perk_read_the_saved_state() {
        // Given a player at Agility 12 owning Light Step
        let _serial = exclusive();
        let mut host = world(12, &["reskilled:agility_light_step"]);

        // When another mod reads their Agility, an unknown skill, their perk and a perk they lack
        let agility = skill_level(&mut host, PLAYER, AGILITY);
        let unknown = skill_level(&mut host, PLAYER, "othermod:cooking");
        let owned = has_perk(&mut host, PLAYER, "reskilled:agility_light_step");
        let lacking = has_perk(&mut host, PLAYER, "reskilled:agility_wind_runner");

        // Then the level is 12, an unknown skill reads 0, and only the owned perk is there
        assert_eq!((agility, unknown, owned, lacking), (12, 0, true, false));
    }
}
