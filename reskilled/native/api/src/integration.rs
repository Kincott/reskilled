//! What other mods may call: a player's level in a skill, whether they own a perk, whether they may use an ability a
//! lock names (an ability is something with no item, block or entity behind it, such as a dash), and whether they
//! may wear an item in a slot of the other mod's own (an accessory slot). Another
//! native mod lists Reskilled as an optional dependency and calls these behind `#[cfg(feature = "reskilled")]`;
//! see `INTEGRATION.md`. Call them on the server.

use horizon_client::mod_api::{Flow, Host, ItemStack};
use skills_core::defs::{Action, TargetKind};
use skills_core::progress::level;

use crate::server::data::current;
use crate::server::enforce::{Actor, guard, listed, may_wear_stack};
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

/// Whether the player may put the stack into a slot of your mod's own (a ring or amulet slot): true when no lock
/// keeps them from wearing its item and none of its enchantments does, or when locks are off for them. When they may
/// not, they get the usual warning ("Needs Agility 10", or "Protection III needs Agility 6"). Accept the item only
/// when this is true.
pub fn may_equip(host: &mut dyn Host, player: u128, stack: &ItemStack) -> bool {
    let exempt = listed(host, |p| p.uuid == player).is_some_and(|actor| actor.exempt);
    may_equip_as(host, player, exempt, stack, true)
}

/// [`may_equip`] without the warning: for checking what a player already wears in your slots, after their levels
/// may have dropped (a reset, a death penalty, new data on `/reload`). Take off what this says no to, and tell the
/// player as your mod does.
pub fn may_wear(host: &mut dyn Host, player: u128, stack: &ItemStack) -> bool {
    let exempt = listed(host, |p| p.uuid == player).is_some_and(|actor| actor.exempt);
    may_equip_as(host, player, exempt, stack, false)
}

/// [`may_equip`] and [`may_wear`] for a player known to be exempt from locks or not.
fn may_equip_as(
    host: &mut dyn Host,
    player: u128,
    exempt: bool,
    stack: &ItemStack,
    tell: bool,
) -> bool {
    let actor = Actor {
        player,
        entity: 0,
        exempt,
        selected: 0,
    };
    may_wear_stack(host, &actor, stack, tell)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use skills_core::progress::{PlayerState, SkillProgress};

    use horizon_client::mod_api::ItemStack;

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
            "actions": ["use"], "requires": {AGILITY: 10}},
            {"target": {"item": {"id": "ringmod:swift_ring"}}, "actions": ["equip"], "requires": {AGILITY: 10}},
            {"target": {"enchantment": {"id": "minecraft:protection"}}, "enchantment_level": 3,
             "actions": ["equip"], "requires": {AGILITY: 6}}]});
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

    fn ring(item: &str, enchantment: Option<(&str, u32)>) -> ItemStack {
        let mut stack = ItemStack {
            item: item.into(),
            count: 1,
            damage: 0,
            extra: None,
        };
        if let Some((id, level)) = enchantment {
            stack.data_mut().enchantments.push((id.into(), level));
        }
        stack
    }

    #[test]
    fn may_equip_checks_the_items_lock_and_says_what_is_missing() {
        // Given the swift ring locked for wearing at Agility 10, and players at Agility 9 and 10
        let _serial = exclusive();
        let mut nine = world(9, &[]);

        // When each puts it into an accessory slot
        let refused = may_equip(&mut nine, PLAYER, &ring("ringmod:swift_ring", None));
        let mut ten = world(10, &[]);
        let allowed = may_equip(&mut ten, PLAYER, &ring("ringmod:swift_ring", None));

        // Then only the second may, and the first reads the usual line
        assert_eq!((refused, allowed), (false, true));
        assert_eq!(nine.action_bars_of(PLAYER), vec!["Needs Agility 10"]);
        assert!(ten.action_bars.is_empty());
    }

    #[test]
    fn may_equip_counts_the_items_enchantments() {
        // Given a ring no lock names, with Protection III (locked for wearing at Agility 6), and a player at 5
        let _serial = exclusive();
        let mut host = world(5, &[]);

        // When they put it on, and a plain ring of the same kind
        let enchanted = may_equip(
            &mut host,
            PLAYER,
            &ring("ringmod:plain_ring", Some(("minecraft:protection", 3))),
        );
        let plain = may_equip(&mut host, PLAYER, &ring("ringmod:plain_ring", None));

        // Then the enchanted one is refused, naming the enchantment, and the plain one is free
        assert_eq!((enchanted, plain), (false, true));
        assert_eq!(
            host.action_bars_of(PLAYER),
            vec!["Protection III needs Agility 6"]
        );
    }

    #[test]
    fn may_wear_answers_without_a_message() {
        // Given a player at Agility 9 wearing the swift ring (Agility 10) in an accessory slot
        let _serial = exclusive();
        let mut host = world(9, &[]);

        // When the accessory mod checks what they may keep wearing
        let kept = may_wear(&mut host, PLAYER, &ring("ringmod:swift_ring", None));

        // Then the ring must come off, and nothing is said (the accessory mod says what it does)
        assert!(!kept);
        assert!(host.action_bars.is_empty());
    }

    #[test]
    fn players_whose_locks_are_off_may_equip_locked_items() {
        // Given a creative player at Agility 1, whose locks are off
        let _serial = exclusive();
        let mut host = world(1, &[]);

        // When they put the swift ring on
        let allowed = may_equip_as(
            &mut host,
            PLAYER,
            true,
            &ring("ringmod:swift_ring", None),
            true,
        );

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
