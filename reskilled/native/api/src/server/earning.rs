//! Skills that grow by doing their work, under the server's rule: hooks note the work as it happens, and each skill's
//! `earned_by` rules (in its data file) say what that work is worth to it; the player's own tick turns it into
//! progress, at the rule's pace, climbing levels as it fills. No skill id or item is named here: a pack's own skills
//! grow the same way, and the settings' `not_creatures` and `projectile_damage` lists say which blows and hits count.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

use horizon_client::mod_api::Host;
use skills_core::defs::{EarnRule, Matcher, TargetKind};
use skills_core::locks::{Facts, NoFacts, matches};
use skills_core::progress::{earn, level};
use skills_core::registry::Defs;

use super::data::{HostFacts, current};
use super::effects::player_of_entity;
use super::queue::{self, Job};
use super::state::{set_state, state_of};
use super::{lock, rules, sharing};
use crate::text::{RAISED, number, say, skill_name};

/// The spots blocks were placed at that already counted, oldest first; at most this many are kept.
const BUILT_KEPT: usize = 65_536;
const HUNDREDTHS: f64 = 100.0;
const PERCENT: u64 = 100;

/// Work each player did since their last tick, per skill, in hundredths of an action.
static PENDING: Mutex<BTreeMap<u128, BTreeMap<String, u64>>> = Mutex::new(BTreeMap::new());

fn note(player: u128, skill: &str, actions: f64) {
    let hundredths = (actions * HUNDREDTHS).round().max(0.0) as u64;
    if hundredths == 0 {
        return;
    }
    *lock(&PENDING)
        .entry(player)
        .or_default()
        .entry(skill.to_string())
        .or_default() += hundredths;
}

/// Notes the work for every skill one of whose rules `worth` prices, at that price times `amount`.
fn credit(player: u128, amount: f64, worth: impl Fn(&EarnRule) -> Option<f64>) {
    let Some(data) = current() else { return };
    for (id, skill) in &data.defs.skills {
        let actions: f64 = skill.earned_by.iter().filter_map(&worth).sum();
        if actions > 0.0 {
            note(player, id, actions * amount);
        }
    }
}

/// Whether one of the matchers picks the id. Entities and damage types are matched by id and pattern only.
fn any_picks(matchers: &[Matcher], id: &str) -> bool {
    matchers
        .iter()
        .any(|m| matches(m, TargetKind::Entity, id, &NoFacts))
}

/// A block broken with what the main hand holds.
pub fn on_break(host: &dyn Host, player: u128, main_hand: Option<&str>) {
    let Some(item) = main_hand else { return };
    let facts = HostFacts(host);
    credit(player, 1.0, |rule| match rule {
        EarnRule::Break { with, worth } if picks_item(with, item, &facts) => Some(*worth),
        _ => None,
    });
}

fn picks_item(with: &Matcher, item: &str, facts: &dyn Facts) -> bool {
    matches(with, TargetKind::Item, item, facts)
}

/// A blow on an entity of `kind`, unless the settings say it is not a creature.
pub fn on_attack(player: u128, kind: &str) {
    if current().is_some_and(|data| not_a_creature(&data.defs, kind)) {
        return;
    }
    credit(player, 1.0, |rule| match rule {
        EarnRule::Attack { worth } => Some(*worth),
        _ => None,
    });
}

fn not_a_creature(defs: &Defs, kind: &str) -> bool {
    any_picks(&defs.settings.not_creatures, kind)
}

pub fn on_projectile_hit(player: u128) {
    credit(player, 1.0, |rule| match rule {
        EarnRule::ProjectileHit { worth } => Some(*worth),
        _ => None,
    });
}

pub fn on_hit_taken(player: u128) {
    credit(player, 1.0, |rule| match rule {
        EarnRule::Hurt { worth } => Some(*worth),
        _ => None,
    });
}

pub fn on_travel(player: u128, blocks: f64) {
    credit(player, blocks, |rule| match rule {
        EarnRule::Travel { per_block } => Some(*per_block),
        _ => None,
    });
}

pub fn on_enchant(player: u128) {
    credit(player, 1.0, |rule| match rule {
        EarnRule::Enchant { worth } => Some(*worth),
        _ => None,
    });
}

/// Spots where a placed block already counted, so placing and breaking at one spot grows nothing.
/// A dimension and a block position.
type Spot = (String, [i32; 3]);

static BUILT: Mutex<(BTreeSet<Spot>, VecDeque<Spot>)> =
    Mutex::new((BTreeSet::new(), VecDeque::new()));

/// A player placed a block at `pos`: it counts once for each spot.
pub fn on_place(player: u128, dimension: &str, pos: [i32; 3]) {
    let spot = (dimension.to_string(), pos);
    {
        let mut built = lock(&BUILT);
        if !built.0.insert(spot.clone()) {
            return;
        }
        built.1.push_back(spot);
        while built.1.len() > BUILT_KEPT {
            if let Some(old) = built.1.pop_front() {
                built.0.remove(&old);
            }
        }
    }
    credit(player, 1.0, |rule| match rule {
        EarnRule::Place { worth } => Some(*worth),
        _ => None,
    });
}

/// A hit: a projectile a player shot (a damage type the settings' `projectile_damage` picks) counts for them, and a
/// hit a player takes from something counts for them.
pub fn hurt(victim: i32, causing: i32, damage_type: &str) {
    let projectile =
        current().is_some_and(|data| any_picks(&data.defs.settings.projectile_damage, damage_type));
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
        if data.defs.skill(&skill).is_none() {
            continue;
        }
        let paced = hundredths.saturating_mul(u64::from(rules.earn_rate)) / PERCENT;
        let (next, gained) = earn(
            &data.defs,
            &state,
            &skill,
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

/// Forgets what this module keeps for the running world, as the server stops.
pub(crate) fn reset() {
    lock(&PENDING).clear();
    *lock(&BUILT) = (BTreeSet::new(), VecDeque::new());
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

    /// How each of the eight skills grows, as the shipped skill files say.
    fn earned_by(skill: &str) -> serde_json::Value {
        match skill {
            "mining" => json!([{"type": "break", "with": {"tag": "#minecraft:pickaxes"}}]),
            "gathering" => json!([{"type": "break", "with": {"any_of": [{"id": "minecraft:shears"},
                {"tag": "#minecraft:axes"}, {"tag": "#minecraft:shovels"}, {"tag": "#minecraft:hoes"}]}}]),
            "melee" => json!([{"type": "attack"}]),
            "ranged" => json!([{"type": "projectile_hit"}]),
            "defense" => json!([{"type": "hurt"}]),
            "agility" => json!([{"type": "travel", "per_block": 0.05}]),
            "arcane" => json!([{"type": "enchant", "worth": 5}]),
            "building" => json!([{"type": "place"}]),
            _ => json!([]),
        }
    }

    /// The settings lists as shipped, in part: what is not a creature, and which damage is a projectile's.
    fn shipped_lists() -> serde_json::Value {
        json!({"format": 1,
            "not_creatures": [{"any_of": [{"id": "minecraft:armor_stand"}, {"id": "minecraft:item_frame"},
                {"id": "minecraft:painting"}, {"id": "minecraft:end_crystal"}, {"pattern": "*:*boat"},
                {"pattern": "*:*minecart*"}]}],
            "projectile_damage": [{"any_of": [{"pattern": "*:*arrow*"}, {"pattern": "*:*trident*"}]}]})
    }

    /// The eight skills known, the player online at level 1 in each, skills growing as `progression` says at
    /// `rate` percent.
    fn world(progression: Progression, rate: u32) -> TestHost {
        world_with(progression, rate, &[], &[])
    }

    /// [`world`], with more skills and settings files beside the shipped ones.
    fn world_with(
        progression: Progression,
        rate: u32,
        more_skills: &[(&str, serde_json::Value)],
        more_settings: &[(&str, serde_json::Value)],
    ) -> TestHost {
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
            "building",
        ]
        .iter()
        .map(|s| {
            let id = format!("reskilled:{s}");
            (
                id.clone(),
                json!({"format": 1, "id": id, "name": "n", "icon": "minecraft:stone",
                    "earned_by": earned_by(s)}),
            )
        })
        .collect();
        let mut skills: Vec<(&str, serde_json::Value)> = skills
            .iter()
            .map(|(id, j)| (id.as_str(), j.clone()))
            .collect();
        skills.extend(more_skills.iter().cloned());
        let mut settings = vec![("reskilled:defaults", shipped_lists())];
        settings.extend(more_settings.iter().cloned());
        load(&mut host, &[("skills", skills), ("settings", settings)]);
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

        // When the player strikes ten blows, enchants twice, places ten blocks, walks 200 blocks, lands ten
        // projectile hits and takes ten hits, and their tick runs
        for _ in 0..10 {
            on_attack(PLAYER, "minecraft:zombie");
            on_projectile_hit(PLAYER);
            on_hit_taken(PLAYER);
        }
        on_enchant(PLAYER);
        on_enchant(PLAYER);
        for x in 0..10 {
            on_place(PLAYER, "minecraft:overworld", [x, 64, 0]);
        }
        on_travel(PLAYER, 200.0);
        run_for(&mut host, PLAYER);

        // Then each skill climbs a level
        for skill in [
            "melee", "ranged", "defense", "arcane", "building", "agility",
        ] {
            assert_eq!(at(&mut host, skill), 2, "{skill}");
        }
    }

    #[test]
    fn a_packs_own_skill_grows_by_the_work_its_file_names() {
        // Given another mod's Digging skill, grown two actions a block broken with its drill
        let _serial = exclusive();
        let digging = json!({"format": 1, "id": "othermod:digging", "name": "n", "icon": "minecraft:stone",
            "earned_by": [{"type": "break", "with": {"id": "othermod:drill"}, "worth": 2}]});
        let mut host = world_with(
            Progression::Earn,
            100,
            &[("othermod:digging", digging)],
            &[],
        );

        // When the player breaks five blocks with the drill, and their tick runs
        for _ in 0..5 {
            on_break(&host, PLAYER, Some("othermod:drill"));
        }
        run_for(&mut host, PLAYER);

        // Then Digging climbs a level, and Mining (pickaxes only) does not
        let digging = level(
            &state_of(&mut host, PLAYER).unwrap(),
            &current().unwrap().defs,
            "othermod:digging",
        );
        assert_eq!((digging, at(&mut host, "mining")), (2, 1));
    }

    #[test]
    fn other_mods_add_things_that_are_not_creatures_and_projectile_damage() {
        // Given another mod's settings: its statue is not a creature, and its dart's damage is a projectile's
        let _serial = exclusive();
        let theirs = json!({"format": 1, "not_creatures": [{"id": "othermod:statue"}],
            "projectile_damage": [{"id": "othermod:dart"}]});
        let mut host = world_with(Progression::Earn, 100, &[], &[("othermod:lists", theirs)]);
        crate::server::effects::remember_entity(PLAYER, 7);

        // When the player strikes the statue and a modded cart ten times each, and hits a zombie (entity 9) with ten
        // darts, and their tick runs
        for _ in 0..10 {
            on_attack(PLAYER, "othermod:statue");
            on_attack(PLAYER, "othermod:rail_minecart");
            hurt(9, 7, "othermod:dart");
        }
        run_for(&mut host, PLAYER);

        // Then Melee stays at 1, and Ranged climbs
        assert_eq!((at(&mut host, "melee"), at(&mut host, "ranged")), (1, 2));
    }

    #[test]
    fn placing_at_the_same_spot_again_grows_nothing() {
        // Given skills growing by doing, Building at 1 (ten actions to climb)
        let _serial = exclusive();
        let mut host = world(Progression::Earn, 100);

        // When the player places and breaks a block at one spot twenty times, and their tick runs
        for _ in 0..20 {
            on_place(PLAYER, "minecraft:overworld", [0, 64, 0]);
        }
        run_for(&mut host, PLAYER);

        // Then Building is still at 1
        assert_eq!(at(&mut host, "building"), 1);
    }
}
