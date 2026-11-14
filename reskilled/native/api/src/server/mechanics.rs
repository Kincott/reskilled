//! Perks that do something rather than change a number: a last stand against a lethal blow, a potion effect for a
//! kill, crops growing around the player, and ripe crops planted again from their own seeds.

use std::collections::BTreeMap;
use std::sync::Mutex;

use horizon_client::mod_api::{Flow, Host, ItemStack, NativeEffect, RandomSource};
use skills_core::defs::Matcher;
use skills_core::locks::{TargetKind, matches};

use super::data::HostFacts;
use super::effects::{effects_of, player_of_entity};
use super::{focus, lock, queue};
use crate::text::{LAST_STAND, say};

const TICKS_PER_SECOND: u64 = 20;
/// How long a noted replant waits for its drops and the player's tick, in ticks.
const REPLANT_TICKS: u64 = 40;
const CROPS: &str = "#minecraft:crops";
const AGE: &str = "age";
/// `Level.setBlock`'s usual flags: tell neighbours and clients.
const UPDATE_ALL: i32 = 3;

/// Each player's health at their last tick, which a lethal blow is judged against.
static HEALTH: Mutex<BTreeMap<u128, f32>> = Mutex::new(BTreeMap::new());
/// The tick each player's last stand was last used.
static LAST_STAND_USED: Mutex<BTreeMap<u128, u64>> = Mutex::new(BTreeMap::new());
/// Potion effects earned by kills, given on the player's own tick.
static KILL_EFFECTS: Mutex<BTreeMap<u128, Vec<NativeEffect>>> = Mutex::new(BTreeMap::new());
/// The tick each player's crops next grow.
static GROWTH_DUE: Mutex<BTreeMap<u128, u64>> = Mutex::new(BTreeMap::new());
/// Ripe crops broken by players with replanting, waiting to be planted again.
static REPLANTS: Mutex<Vec<Replant>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, PartialEq)]
struct Replant {
    player: u128,
    dimension: String,
    pos: [i32; 3],
    block: String,
    seed: String,
    seed_taken: bool,
    at: u64,
}

/// The player's own tick, at `feet` in `dimension` with `health`: their health is noted, kill effects given, crops
/// around them grown and their replants planted.
pub fn run_for(host: &mut dyn Host, player: u128, dimension: &str, feet: [f64; 3], health: f32) {
    lock(&HEALTH).insert(player, health);
    for effect in lock(&KILL_EFFECTS).remove(&player).unwrap_or_default() {
        host.add_effect(player, &effect);
    }
    grow_crops(host, player, dimension, feet);
    plant_again(host, player);
}

/// A mob or player is about to die. A player whose last stand is ready, and who had at least its health before
/// the blow, stays alive at half a heart (Horizon leaves a cancelled death at 1 health).
pub fn on_death(host: &mut dyn Host, entity: u32, kind: &str) -> Flow {
    if kind != "minecraft:player" {
        return Flow::Continue;
    }
    let Some(player) = i32::try_from(entity).ok().and_then(player_of_entity) else {
        return Flow::Continue;
    };
    let Some((above, rest)) = effects_of(player).and_then(|e| e.last_stand) else {
        return Flow::Continue;
    };
    let healthy = lock(&HEALTH)
        .get(&player)
        .is_some_and(|health| *health >= above);
    let now = queue::now();
    let ready = lock(&LAST_STAND_USED)
        .get(&player)
        .is_none_or(|used| now >= used + u64::from(rest) * TICKS_PER_SECOND);
    if !healthy || !ready {
        return Flow::Continue;
    }
    lock(&LAST_STAND_USED).insert(player, now);
    host.action_bar(player, say(&LAST_STAND, &[]));
    Flow::Cancel
}

/// A mob of `kind` killed by a player is about to drop its XP: the player who hurt it last earns their kill
/// effects for it, given on their next tick.
pub fn on_kill(host: &dyn Host, entity: u32, kind: &str) {
    let Some(player) = focus::credited(entity) else {
        return;
    };
    let Some(effects) = effects_of(player) else {
        return;
    };
    let facts = HostFacts(host);
    let earned: Vec<NativeEffect> = effects
        .on_kill
        .iter()
        .filter(|(.., mobs)| matches(mobs, TargetKind::Entity, kind, &facts))
        .map(|(effect, seconds, amplifier, _)| NativeEffect {
            id: effect.clone(),
            amplifier: *amplifier,
            duration: i32::try_from(u64::from(*seconds) * TICKS_PER_SECOND).unwrap_or(i32::MAX),
            ambient: false,
            particles: true,
        })
        .collect();
    if !earned.is_empty() {
        lock(&KILL_EFFECTS)
            .entry(player)
            .or_default()
            .extend(earned);
    }
}

/// A player is breaking the block at `pos`: a ripe crop they replant is noted, to take a seed from its drops.
pub fn on_break(host: &mut dyn Host, player: u128, dimension: &str, pos: [i32; 3]) {
    let Some(effects) = effects_of(player).filter(|e| !e.replant.is_empty()) else {
        return;
    };
    let Some(level) = host.level(dimension) else {
        return;
    };
    let Some(block) = host.level_block(level, pos) else {
        return;
    };
    let Some(crop) = effects.replant.iter().find(|crop| crop.block == block) else {
        return;
    };
    let age = host.level_block_property(level, pos, AGE);
    if age.and_then(|a| a.parse::<u32>().ok()) != Some(crop.ripe_age) {
        return;
    }
    let mut replants = lock(&REPLANTS);
    replants.retain(|r| r.dimension != dimension || r.pos != pos);
    replants.push(Replant {
        player,
        dimension: dimension.to_string(),
        pos,
        block: crop.block.clone(),
        seed: crop.seed.clone(),
        seed_taken: false,
        at: queue::now(),
    });
}

/// A block broken by `player` drops `drops`: a noted ripe crop gives up one of its seeds to be planted again.
pub fn on_drops(player: u128, dimension: &str, pos: [i32; 3], drops: &mut Vec<ItemStack>) {
    let mut replants = lock(&REPLANTS);
    let Some(replant) = replants
        .iter_mut()
        .find(|r| r.player == player && r.dimension == dimension && r.pos == pos && !r.seed_taken)
    else {
        return;
    };
    let Some(at) = drops
        .iter()
        .position(|stack| *stack.item == *replant.seed && stack.count > 0)
    else {
        return;
    };
    drops[at].count -= 1;
    if drops[at].count == 0 {
        drops.remove(at);
    }
    replant.seed_taken = true;
}

fn plant_again(host: &mut dyn Host, player: u128) {
    let now = queue::now();
    let ready: Vec<Replant> = {
        let mut replants = lock(&REPLANTS);
        let (mine, others): (Vec<_>, Vec<_>) = std::mem::take(&mut *replants)
            .into_iter()
            .partition(|r| r.player == player);
        let (ready, waiting): (Vec<_>, Vec<_>) = mine.into_iter().partition(|r| r.seed_taken);
        *replants = others;
        replants.extend(waiting.into_iter().filter(|r| now <= r.at + REPLANT_TICKS));
        ready
    };
    for replant in ready {
        let Some(level) = host.level(&replant.dimension) else {
            continue;
        };
        let empty = host
            .level_block(level, replant.pos)
            .is_none_or(|block| block == "minecraft:air");
        if let (true, Some(state)) = (empty, host.default_state(&replant.block)) {
            host.set_block(&replant.dimension, replant.pos, state, UPDATE_ALL);
        }
    }
}

fn grow_crops(host: &mut dyn Host, player: u128, dimension: &str, feet: [f64; 3]) {
    let Some((radius, interval, chance)) = effects_of(player).and_then(|e| e.crop_growth) else {
        return;
    };
    let now = queue::now();
    let every = u64::from(interval) * TICKS_PER_SECOND;
    {
        let mut due = lock(&GROWTH_DUE);
        match due.get(&player) {
            Some(at) if now >= *at => {
                due.insert(player, now + every);
            }
            Some(_) => return,
            None => {
                due.insert(player, now + every);
                return;
            }
        }
    }
    let Some(level) = host.level(dimension) else {
        return;
    };
    let crops = Matcher::Tag(CROPS.into());
    let centre = feet.map(|v| v.floor() as i32);
    let r = i32::try_from(radius).unwrap_or(0);
    let mut growing = Vec::new();
    for dx in -r..=r {
        for dz in -r..=r {
            for dy in -1..=1 {
                let pos = [centre[0] + dx, centre[1] + dy, centre[2] + dz];
                let Some(block) = host.level_block(level, pos) else {
                    continue;
                };
                if !matches(&crops, TargetKind::Block, &block, &HostFacts(&*host)) {
                    continue;
                }
                if let Some(age) = host
                    .level_block_property(level, pos, AGE)
                    .and_then(|a| a.parse::<u32>().ok())
                {
                    growing.push((pos, age));
                }
            }
        }
    }
    for (pos, age) in growing {
        let roll = host
            .level_random(dimension)
            .map(|random| random.next_double());
        if roll.is_some_and(|roll| roll < chance) {
            // A crop at its last stage has no next age: the change is refused and it stays ripe.
            host.set_block_property(pos, AGE, &(age + 1).to_string());
        }
    }
}

/// Forgets what this module keeps for the running world, as the server stops.
pub(crate) fn reset() {
    lock(&HEALTH).clear();
    lock(&LAST_STAND_USED).clear();
    lock(&KILL_EFFECTS).clear();
    lock(&GROWTH_DUE).clear();
    lock(&REPLANTS).clear();
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::{BlockStateId, Flow, ItemStack};
    use serde_json::{Value, json};
    use skills_core::progress::{PlayerState, SkillProgress};

    use super::*;
    use crate::server::data::tests::load;
    use crate::server::effects::{remember_entity, sync_attributes};
    use crate::server::queue::run;
    use crate::server::state::set_state;
    use crate::test_host::{TestHost, exclusive};

    const PLAYER: u128 = 0x1234;
    const ENTITY: u32 = 77;
    const ZOMBIE: u32 = 90;
    const COW: u32 = 91;
    const OVERWORLD: &str = "minecraft:overworld";
    const FEET: [f64; 3] = [0.5, 64.0, 0.5];

    /// Defense known, and the player owning one perk with `effects`, synced.
    fn owning(host: &mut TestHost, effects: Value) {
        let defense =
            json!({"format": 1, "id": "reskilled:defense", "name": "n", "icon": "minecraft:stone"});
        let perk = json!({"format": 1, "id": "reskilled:p", "skill": "reskilled:defense", "name": "n",
            "description": "d", "cost": 1, "min_level": 1, "effects": effects});
        load(
            host,
            &[
                ("skills", vec![("reskilled:defense", defense)]),
                ("perks", vec![("reskilled:p", perk)]),
            ],
        );
        let mut state = PlayerState::default();
        let progress = SkillProgress {
            level: 1,
            perks: ["reskilled:p".to_string()].into(),
            ..SkillProgress::default()
        };
        state.skills.insert("reskilled:defense".into(), progress);
        set_state(host, PLAYER, state).unwrap();
        remember_entity(PLAYER, ENTITY);
        sync_attributes(host, PLAYER);
    }

    /// The player's own tick at `FEET` with `health`.
    fn tick(host: &mut TestHost, health: f32) {
        run(host, Vec::new());
        run_for(host, PLAYER, OVERWORLD, FEET, health);
    }

    #[test]
    fn a_last_stand_saves_a_healthy_player_once_then_rests() {
        // Given a last stand above 6 health resting 1 second, and a player at 10 health
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(
            &mut host,
            json!([{"type": "last_stand", "above": 6.0, "cooldown_seconds": 1}]),
        );
        tick(&mut host, 10.0);

        // When a blow kills them, another at once, and one after the rest while they were at 4 health
        let first = on_death(&mut host, ENTITY, "minecraft:player");
        let again = on_death(&mut host, ENTITY, "minecraft:player");
        for _ in 0..25 {
            tick(&mut host, 4.0);
        }
        let weak = on_death(&mut host, ENTITY, "minecraft:player");

        // Then only the first is survived, and they are told
        assert_eq!(
            (first, again, weak),
            (Flow::Cancel, Flow::Continue, Flow::Continue)
        );
        assert_eq!(host.action_bars_of(PLAYER).len(), 1);
    }

    #[test]
    fn killing_a_listed_mob_gives_the_effect_on_the_players_tick() {
        // Given Strength I for 5 seconds on killing zombies
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(
            &mut host,
            json!([{"type": "effect_on_kill", "effect": "minecraft:strength", "seconds": 5,
                "amplifier": 0, "mobs": {"id": "minecraft:zombie"}}]),
        );

        // When the player hurts and kills a zombie and a cow, and their tick runs
        for (mob, kind) in [(ZOMBIE, "minecraft:zombie"), (COW, "minecraft:cow")] {
            crate::server::focus::hurt(mob as i32, ENTITY as i32);
            on_kill(&host, mob, kind);
        }
        tick(&mut host, 20.0);

        // Then they get Strength I for 100 ticks once, for the zombie
        let given: Vec<(String, u8, i32)> = host
            .effects_given
            .iter()
            .map(|(_, e)| (e.id.clone(), e.amplifier, e.duration))
            .collect();
        assert_eq!(given, vec![("minecraft:strength".to_string(), 0, 100)]);
    }

    #[test]
    fn crops_around_the_player_grow_on_the_interval() {
        // Given crop growth every second within 2 blocks at certain chance, a wheat at age 3 beside the player and
        // one 5 blocks away
        let _serial = exclusive();
        let mut host = TestHost {
            random: Some(horizon_client::mod_api::LegacyRandom::new(1)),
            ..TestHost::default()
        };
        host.block_tags
            .push(("minecraft:wheat".into(), "minecraft:crops".into()));
        owning(
            &mut host,
            json!([{"type": "crop_growth", "radius": 2, "interval_seconds": 1, "chance": 1.0}]),
        );
        let wheat = || {
            (
                "minecraft:wheat".to_string(),
                vec![("age".to_string(), "3".to_string())],
            )
        };
        host.blocks.insert([1, 64, 0], wheat());
        host.blocks.insert([5, 64, 0], wheat());

        // When a little over a second of their ticks run
        for _ in 0..21 {
            tick(&mut host, 20.0);
        }

        // Then the near wheat grew a stage and the far one did not
        let age = |pos| host.blocks[&pos].1[0].1.clone();
        assert_eq!(
            (age([1, 64, 0]), age([5, 64, 0])),
            ("4".to_string(), "3".to_string())
        );
    }

    #[test]
    fn a_ripe_crop_is_planted_again_with_one_of_its_seeds() {
        // Given replanting wheat, ripe at 7, and a ripe and an unripe wheat
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(
            &mut host,
            json!([{"type": "replant", "crops": [{"block": "minecraft:wheat",
                "seed": "minecraft:wheat_seeds", "ripe_age": 7}]}]),
        );
        host.default_states.insert("minecraft:wheat".into(), 42);
        let wheat = |age: &str| {
            (
                "minecraft:wheat".to_string(),
                vec![("age".to_string(), age.to_string())],
            )
        };
        host.blocks.insert([1, 64, 0], wheat("7"));
        host.blocks.insert([2, 64, 0], wheat("3"));

        // When the player breaks both, each dropping wheat and two seeds, and their tick runs
        let mut kept = Vec::new();
        for pos in [[1, 64, 0], [2, 64, 0]] {
            on_break(&mut host, PLAYER, OVERWORLD, pos);
            host.blocks.remove(&pos);
            let mut drops = vec![
                ItemStack::new("minecraft:wheat", 1),
                ItemStack::new("minecraft:wheat_seeds", 2),
            ];
            on_drops(PLAYER, OVERWORLD, pos, &mut drops);
            kept.push(drops.iter().map(|s| s.count).sum::<u32>());
        }
        tick(&mut host, 20.0);

        // Then the ripe one gave up a seed and was planted again; the unripe one kept its drops and stays broken
        assert_eq!(kept, vec![2, 3]);
        assert_eq!(
            host.blocks_set,
            vec![(OVERWORLD.to_string(), [1, 64, 0], BlockStateId(42))]
        );
    }
}
