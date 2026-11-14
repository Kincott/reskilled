//! Skills that grow by doing their work, under the server's rule: hooks note the work as it happens (blocks broken
//! with the skill's tools, blows, projectile hits, hits taken, distance, enchanting, taming and breeding), and the
//! player's own tick turns it into progress, at the rule's pace, climbing levels as it fills.

use std::collections::BTreeMap;
use std::sync::Mutex;

use horizon_client::mod_api::Host;
use skills_core::defs::{Matcher, TargetKind};
use skills_core::locks::matches;
use skills_core::progress::{earn, level};

use super::data::{HostFacts, current};
use super::effects::player_of_entity;
use super::queue::{self, Job};
use super::state::{set_state, state_of};
use super::{lock, rules, sharing};
use crate::text::{RAISED, number, say, skill_name};

/// The skills work grows, by their shipped ids; a pack without one of them grows nothing there.
const MINING: &str = "reskilled:mining";
const GATHERING: &str = "reskilled:gathering";
const MELEE: &str = "reskilled:melee";
const RANGED: &str = "reskilled:ranged";
const DEFENSE: &str = "reskilled:defense";
const AGILITY: &str = "reskilled:agility";
const ARCANE: &str = "reskilled:arcane";
const TAMING: &str = "reskilled:taming";
/// What work is worth, in actions: a block, a blow or a hit is one; enchanting is five, taming ten, breeding three;
/// each block travelled a twentieth.
const ENCHANTING: f64 = 5.0;
const TAMING_ONE: f64 = 10.0;
const BREEDING: f64 = 3.0;
const PER_BLOCK: f64 = 0.05;
const HUNDREDTHS: f64 = 100.0;
const PERCENT: u64 = 100;

/// Work each player did since their last tick, per skill, in hundredths of an action.
static PENDING: Mutex<BTreeMap<u128, BTreeMap<&'static str, u64>>> = Mutex::new(BTreeMap::new());

fn note(player: u128, skill: &'static str, actions: f64) {
    let hundredths = (actions * HUNDREDTHS).round().max(0.0) as u64;
    *lock(&PENDING)
        .entry(player)
        .or_default()
        .entry(skill)
        .or_default() += hundredths;
}

fn in_tag(host: &dyn Host, item: &str, tag: &str) -> bool {
    matches(
        &Matcher::Tag(tag.into()),
        TargetKind::Item,
        item,
        &HostFacts(host),
    )
}

/// A block broken with what the main hand holds: a pickaxe grows Mining; an axe, shovel, hoe or shears Gathering.
pub fn on_break(host: &dyn Host, player: u128, main_hand: Option<&str>) {
    let Some(item) = main_hand else { return };
    if in_tag(host, item, "#minecraft:pickaxes") {
        note(player, MINING, 1.0);
    } else if item == "minecraft:shears"
        || ["#minecraft:axes", "#minecraft:shovels", "#minecraft:hoes"]
            .iter()
            .any(|tag| in_tag(host, item, tag))
    {
        note(player, GATHERING, 1.0);
    }
}

/// Things a blow lands on that are not creatures: hitting them grows nothing.
const NOT_CREATURES: [&str; 8] = [
    "minecraft:armor_stand",
    "minecraft:item_frame",
    "minecraft:glow_item_frame",
    "minecraft:painting",
    "minecraft:end_crystal",
    "minecraft:leash_knot",
    "minecraft:interaction",
    "minecraft:item",
];

/// A blow on an entity of `kind`: Melee grows when it is a creature, not a boat, a minecart or a decoration.
pub fn on_attack(player: u128, kind: &str) {
    let vehicle = ["boat", "_raft", "minecart"]
        .iter()
        .any(|end| kind.ends_with(end));
    if vehicle || NOT_CREATURES.contains(&kind) {
        return;
    }
    note(player, MELEE, 1.0);
}

pub fn on_projectile_hit(player: u128) {
    note(player, RANGED, 1.0);
}

pub fn on_hit_taken(player: u128) {
    note(player, DEFENSE, 1.0);
}

pub fn on_travel(player: u128, blocks: f64) {
    note(player, AGILITY, blocks * PER_BLOCK);
}

pub fn on_enchant(player: u128) {
    note(player, ARCANE, ENCHANTING);
}

pub fn on_tame(player: u128) {
    note(player, TAMING, TAMING_ONE);
}

pub fn on_breed(player: u128) {
    note(player, TAMING, BREEDING);
}

/// A hit: a projectile a player shot grows their Ranged, and a hit a player takes from something grows their
/// Defense.
pub fn hurt(victim: i32, causing: i32, damage_type: &str) {
    let projectile = ["arrow", "trident", "thrown", "fireball", "wind_charge"]
        .iter()
        .any(|kind| damage_type.contains(kind));
    if let Some(shooter) = player_of_entity(causing).filter(|_| projectile) {
        on_projectile_hit(shooter);
    }
    if let Some(hit) = player_of_entity(victim).filter(|_| causing != 0 && causing != victim) {
        on_hit_taken(hit);
    }
}

/// The player's own tick: their work turns into progress at the rule's pace, levels climbed are said on the action
/// bar and shared. Under the rule that skills are only bought, the work is dropped.
pub fn run_for(host: &mut dyn Host, player: u128) {
    let Some(work) = lock(&PENDING).remove(&player) else {
        return;
    };
    let rules = rules::current(host);
    if !rules.earning() {
        return;
    }
    let (Some(data), Ok(mut state)) = (current(), state_of(host, player)) else {
        return;
    };
    let mut climbed = Vec::new();
    for (skill, hundredths) in work {
        if data.defs.skill(skill).is_none() {
            continue;
        }
        let paced = hundredths.saturating_mul(u64::from(rules.earn_rate)) / PERCENT;
        let (next, gained) = earn(
            &data.defs,
            &state,
            skill,
            u32::try_from(paced).unwrap_or(u32::MAX),
        );
        state = next;
        if gained > 0 {
            climbed.push(skill);
        }
    }
    if let Err(error) = set_state(host, player, state.clone()) {
        log::warn!("Reskilled could not keep the progress {player:032x} earned: {error:?}");
        return;
    }
    for skill in &climbed {
        let line = say(
            &RAISED,
            &[
                skill_name(&data.defs, skill),
                number(level(&state, &data.defs, skill)),
            ],
        );
        host.action_bar(player, line);
    }
    if !climbed.is_empty() {
        sharing::record(host, player, &state);
        queue::queue(Job::SyncAttributes(player));
    }
}

/// A player left: their unturned work is dropped.
pub fn forget(player: u128) {
    lock(&PENDING).remove(&player);
}

#[cfg(test)]
pub(crate) fn reset() {
    lock(&PENDING).clear();
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use skills_core::progress::{PlayerState, level};
    use skills_core::rules::{Progression, Rules};

    use super::*;
    use crate::server::data::current;
    use crate::server::data::tests::load;
    use crate::server::rules::force;
    use crate::server::state::{logged_in, set_state, state_of};
    use crate::test_host::{TestHost, exclusive};

    const PLAYER: u128 = 0x1234;

    /// The eight skills known, the player online at level 1 in each, skills growing as `progression` says at
    /// `rate` percent.
    fn world(progression: Progression, rate: u32) -> TestHost {
        let mut host = TestHost::default();
        host.item_tags
            .push(("minecraft:iron_pickaxe".into(), "minecraft:pickaxes".into()));
        host.item_tags
            .push(("minecraft:iron_axe".into(), "minecraft:axes".into()));
        let skills: Vec<(String, serde_json::Value)> = [
            "mining",
            "gathering",
            "melee",
            "ranged",
            "defense",
            "agility",
            "arcane",
            "taming",
        ]
        .iter()
        .map(|s| {
            let id = format!("reskilled:{s}");
            (
                id.clone(),
                json!({"format": 1, "id": id, "name": "n", "icon": "minecraft:stone"}),
            )
        })
        .collect();
        let skills: Vec<(&str, serde_json::Value)> = skills
            .iter()
            .map(|(id, j)| (id.as_str(), j.clone()))
            .collect();
        load(&mut host, &[("skills", skills)]);
        logged_in(PLAYER, "Pat");
        set_state(&mut host, PLAYER, PlayerState::default()).unwrap();
        force(Rules {
            progression,
            earn_rate: rate,
            ..Rules::default()
        });
        host
    }

    fn at(host: &mut TestHost, skill: &str) -> u32 {
        level(
            &state_of(host, PLAYER).unwrap(),
            &current().unwrap().defs,
            &format!("reskilled:{skill}"),
        )
    }

    #[test]
    fn breaking_with_a_pickaxe_grows_mining_and_says_so() {
        // Given skills growing by doing, Mining at 1 (ten actions to climb)
        let _serial = exclusive();
        let mut host = world(Progression::Earn, 100);

        // When the player breaks ten blocks with a pickaxe and one with an axe, and their tick runs
        for _ in 0..10 {
            on_break(&host, PLAYER, Some("minecraft:iron_pickaxe"));
        }
        on_break(&host, PLAYER, Some("minecraft:iron_axe"));
        run_for(&mut host, PLAYER);

        // Then Mining reaches 2 and they read it, and the axe's block counts toward Gathering, not Mining
        assert_eq!(at(&mut host, "mining"), 2);
        assert_eq!(at(&mut host, "gathering"), 1);
        assert_eq!(host.action_bars_of(PLAYER), vec!["Mining is now level 2."]);
        let gathering = state_of(&mut host, PLAYER).unwrap().skills["reskilled:gathering"].earned;
        assert_eq!(gathering, 100);
    }

    #[test]
    fn blows_on_things_that_are_not_creatures_grow_nothing() {
        // Given skills growing by doing, Melee at 1 (ten blows to climb)
        let _serial = exclusive();
        let mut host = world(Progression::Earn, 100);

        // When the player strikes boats, minecarts, armor stands, item frames and paintings ten times each
        for kind in [
            "minecraft:oak_boat",
            "minecraft:minecart",
            "minecraft:armor_stand",
            "minecraft:item_frame",
            "minecraft:painting",
            "minecraft:end_crystal",
        ] {
            for _ in 0..10 {
                on_attack(PLAYER, kind);
            }
        }
        run_for(&mut host, PLAYER);
        let after_things = at(&mut host, "melee");
        for _ in 0..10 {
            on_attack(PLAYER, "minecraft:zombie");
        }
        run_for(&mut host, PLAYER);

        // Then none of it counts, and ten blows on a zombie do
        assert_eq!((after_things, at(&mut host, "melee")), (1, 2));
    }

    #[test]
    fn the_pace_scales_what_work_is_worth() {
        // Given skills growing by doing at half pace
        let _serial = exclusive();
        let mut host = world(Progression::Both, 50);

        // When the player breaks ten blocks with a pickaxe, their tick runs, then ten more and it runs again
        for _ in 0..10 {
            on_break(&host, PLAYER, Some("minecraft:iron_pickaxe"));
        }
        run_for(&mut host, PLAYER);
        let after_ten = at(&mut host, "mining");
        for _ in 0..10 {
            on_break(&host, PLAYER, Some("minecraft:iron_pickaxe"));
        }
        run_for(&mut host, PLAYER);

        // Then ten are only half a level, and twenty make one
        assert_eq!((after_ten, at(&mut host, "mining")), (1, 2));
    }

    #[test]
    fn nothing_grows_when_skills_are_only_bought() {
        // Given skills that are only bought
        let _serial = exclusive();
        let mut host = world(Progression::Buy, 100);

        // When the player does plenty of work and their tick runs
        for _ in 0..50 {
            on_break(&host, PLAYER, Some("minecraft:iron_pickaxe"));
            on_attack(PLAYER, "minecraft:zombie");
        }
        run_for(&mut host, PLAYER);

        // Then nothing grows, and nothing waits to grow later
        assert_eq!((at(&mut host, "mining"), at(&mut host, "melee")), (1, 1));
        force(Rules {
            progression: Progression::Earn,
            ..Rules::default()
        });
        run_for(&mut host, PLAYER);
        assert_eq!(at(&mut host, "mining"), 1);
    }

    #[test]
    fn each_kind_of_work_grows_its_skill() {
        // Given skills growing by doing
        let _serial = exclusive();
        let mut host = world(Progression::Earn, 100);

        // When the player strikes ten blows, enchants twice, tames an animal, walks 200 blocks, lands ten
        // projectile hits and takes ten hits, and their tick runs
        for _ in 0..10 {
            on_attack(PLAYER, "minecraft:zombie");
            on_projectile_hit(PLAYER);
            on_hit_taken(PLAYER);
        }
        on_enchant(PLAYER);
        on_enchant(PLAYER);
        on_tame(PLAYER);
        on_travel(PLAYER, 200.0);
        run_for(&mut host, PLAYER);

        // Then each skill climbs a level
        for skill in ["melee", "ranged", "defense", "arcane", "taming", "agility"] {
            assert_eq!(at(&mut host, skill), 2, "{skill}");
        }
    }
}
