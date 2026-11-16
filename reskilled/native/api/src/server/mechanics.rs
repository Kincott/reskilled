//! Perks that do something rather than change a number: a last stand against a lethal blow, a potion effect for a
//! kill, crops growing around the player, and ripe crops planted again from their own seeds.

use std::collections::BTreeMap;
use std::sync::Mutex;

use horizon_client::mod_api::{
    EquipmentSlot, Flow, Host, ItemStack, NativeEffect, NativeInventoryKind, RandomSource,
};
use skills_core::defs::Matcher;
use skills_core::locks::{TargetKind, matches};

use super::data::HostFacts;
use super::effects::{effects_of, player_of_entity};
use super::enforce::inventory_index;
use super::state::online_players;
use super::{focus, homes, lock, queue};
use crate::text::{LAST_STAND, say};
use skills_core::effects::ActiveEffects;

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
/// How many enchantments each player's worn armor carries, as of their last tick.
static ARMOR_RUNES: Mutex<BTreeMap<u128, u32>> = Mutex::new(BTreeMap::new());
/// Each player's shortened effects and the time each had left at their last tick, to tell a new dose.
static SEEN_EFFECTS: Mutex<BTreeMap<u128, BTreeMap<String, i32>>> = Mutex::new(BTreeMap::new());
/// The tick each player's enchanted gear is next mended.
static REPAIR_DUE: Mutex<BTreeMap<u128, u64>> = Mutex::new(BTreeMap::new());
/// A placed block a player with salvage is breaking: player, dimension, position, block.
type Salvage = (u128, String, [i32; 3], String);

static SALVAGE: Mutex<Vec<Salvage>> = Mutex::new(Vec::new());
/// The tick each player got into bed.
static IN_BED: Mutex<BTreeMap<u128, u64>> = Mutex::new(BTreeMap::new());
/// Players who woke from a full sleep, rested on their next tick.
static RESTED: Mutex<Vec<u128>> = Mutex::new(Vec::new());
/// The tick each player's home effects are next looked at.
static HOME_DUE: Mutex<BTreeMap<u128, u64>> = Mutex::new(BTreeMap::new());
/// The ticks a night's sleep takes before the morning comes.
const FULL_SLEEP_TICKS: u64 = 100;
/// How often the effects near home are looked at, in ticks.
const HOME_CHECK_TICKS: u64 = 20;
/// A near-home effect lasts this long and is topped up when below `HOME_EFFECT_TOP_UP`, so its countdown passes
/// the points where effects like regeneration act.
const HOME_EFFECT_TICKS: i32 = 100;
const HOME_EFFECT_TOP_UP: i32 = 25;
const MONSTER: &str = "monster";
const MAX_HEALTH: &str = "minecraft:max_health";
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

/// The player's own tick, at `feet` in `dimension` with `health` and hotbar slot `selected`: their health and
/// armor runes are noted, kill effects given, harmful effects shortened, enchanted gear mended, crops around them
/// grown and their replants planted.
pub fn run_for(
    host: &mut dyn Host,
    player: u128,
    dimension: &str,
    feet: [f64; 3],
    health: f32,
    selected: usize,
) {
    lock(&HEALTH).insert(player, health);
    for effect in lock(&KILL_EFFECTS).remove(&player).unwrap_or_default() {
        host.add_effect(player, &effect);
    }
    count_armor_runes(host, player);
    shorten_effects(host, player);
    rest(host, player);
    home_effects(host, player, dimension, feet);
    mend_runes(host, player, selected);
    grow_crops(host, player, dimension, feet);
    plant_again(host, player);
}

/// A player's weapon hit (`causing` is their entity): an enchanted weapon deals more for each enchantment.
pub fn on_weapon_hit(causing: i32, weapon: Option<&ItemStack>, amount: &mut f32) {
    let (Some(player), Some(weapon)) = (player_of_entity(causing), weapon) else {
        return;
    };
    let Some((per, most)) = effects_of(player).and_then(|e| e.enchanted_weapon) else {
        return;
    };
    let runes = weapon.enchantments().len() as f64;
    if runes > 0.0 {
        *amount = (f64::from(*amount) * (1.0 + (runes * per).min(most))) as f32;
    }
}

/// The share of a blow the player's enchanted armor lets through: 1 with no ward.
pub fn armor_ward(player: u128) -> f64 {
    let Some((per, most)) = effects_of(player).and_then(|e| e.enchanted_armor) else {
        return 1.0;
    };
    let runes = f64::from(lock(&ARMOR_RUNES).get(&player).copied().unwrap_or(0));
    1.0 - (runes * per).min(most)
}

const ARMOR: [EquipmentSlot; 4] = [
    EquipmentSlot::Head,
    EquipmentSlot::Chest,
    EquipmentSlot::Legs,
    EquipmentSlot::Feet,
];

fn count_armor_runes(host: &dyn Host, player: u128) {
    if effects_of(player).is_none_or(|e| e.enchanted_armor.is_none()) {
        return;
    }
    let runes: usize = ARMOR
        .iter()
        .filter_map(|slot| host.player_equipment(player, *slot))
        .map(|stack| stack.enchantments().len())
        .sum();
    lock(&ARMOR_RUNES).insert(player, u32::try_from(runes).unwrap_or(u32::MAX));
}

/// A listed effect that came on the player since their last tick (new, or a fresh dose longer than what was left)
/// is cut to its share of its time, once.
fn shorten_effects(host: &mut dyn Host, player: u128) {
    let Some(effects) = effects_of(player).filter(|e| !e.shorter_effects.is_empty()) else {
        lock(&SEEN_EFFECTS).remove(&player);
        return;
    };
    let now = host.effects(player).unwrap_or_default();
    let seen = lock(&SEEN_EFFECTS).remove(&player).unwrap_or_default();
    let mut next = BTreeMap::new();
    for effect in now {
        let Some((_, share)) = effects
            .shorter_effects
            .iter()
            .find(|(id, _)| *id == effect.id)
        else {
            continue;
        };
        if effect.duration <= 0 {
            continue;
        }
        let fresh = seen
            .get(&effect.id)
            .is_none_or(|left| effect.duration > *left);
        let mut duration = effect.duration;
        if fresh {
            duration = ((f64::from(effect.duration) * share).ceil() as i32).max(1);
            let shorter = NativeEffect {
                duration,
                ..effect.clone()
            };
            host.remove_effect(player, &effect.id);
            host.add_effect(player, &shorter);
        }
        next.insert(effect.id.clone(), duration);
    }
    lock(&SEEN_EFFECTS).insert(player, next);
}

pub fn on_bed_enter(player: u128) {
    lock(&IN_BED).insert(player, queue::now());
}

/// The player left their bed: after a full night's sleep they rest on their next tick.
pub fn on_bed_leave(player: u128) {
    let entered = lock(&IN_BED).remove(&player);
    if entered.is_some_and(|at| queue::now() >= at + FULL_SLEEP_TICKS) {
        lock(&RESTED).push(player);
    }
}

/// A rested player with restful sleep is healed and loses the effects it clears.
fn rest(host: &mut dyn Host, player: u128) {
    let rested = {
        let mut rested = lock(&RESTED);
        let before = rested.len();
        rested.retain(|p| *p != player);
        before != rested.len()
    };
    let Some(effects) = effects_of(player).filter(|e| rested && !e.restful_sleep.is_empty()) else {
        return;
    };
    if let Some((_, most)) = host.attribute(player, MAX_HEALTH) {
        host.set_player_status(player, Some(most as f32), None, None);
    }
    for effect in &effects.restful_sleep {
        host.remove_effect(player, effect);
    }
}

/// Near their home, the player keeps their near-home effects.
fn home_effects(host: &mut dyn Host, player: u128, dimension: &str, feet: [f64; 3]) {
    let Some(effects) = effects_of(player).filter(|e| !e.near_home.is_empty()) else {
        return;
    };
    if !due(&HOME_DUE, player, HOME_CHECK_TICKS) {
        return;
    }
    homes::ensure_loaded(host);
    let Some(home) = homes::home_of(player) else {
        return;
    };
    let now = host.effects(player).unwrap_or_default();
    for (radius, effect, amplifier) in &effects.near_home {
        if !homes::near(&home, dimension, feet, *radius) {
            continue;
        }
        let left = now
            .iter()
            .find(|e| e.id == *effect && e.amplifier >= *amplifier)
            .map_or(0, |e| e.duration);
        if left < HOME_EFFECT_TOP_UP {
            host.add_effect(
                player,
                &NativeEffect {
                    id: effect.clone(),
                    amplifier: *amplifier,
                    duration: HOME_EFFECT_TICKS,
                    ambient: true,
                    particles: false,
                },
            );
        }
    }
}

/// The homes of the players online, each with `radius` from their effects.
fn online_homes(radius: impl Fn(&ActiveEffects) -> Option<u32>) -> Vec<(homes::Home, u32)> {
    online_players()
        .into_iter()
        .filter_map(|player| {
            let r = effects_of(player).and_then(|e| radius(&e))?;
            Some((homes::home_of(player)?, r))
        })
        .collect()
}

/// An explosion in `dimension` is about to break `blocks`: those near the home of an online player with a home
/// ward are spared.
pub fn on_explosion(dimension: &str, blocks: &mut Vec<[i32; 3]>) {
    let wards = online_homes(|e| e.home_ward);
    if wards.is_empty() {
        return;
    }
    blocks.retain(|block| {
        let centre = block.map(|v| f64::from(v) + 0.5);
        !wards
            .iter()
            .any(|(home, radius)| homes::near(home, dimension, centre, *radius))
    });
}

/// Whether a natural spawn of `category` may happen at `pos`: monsters are kept away from the home of an online
/// player with a safe haven.
pub fn may_spawn(dimension: &str, category: &str, pos: [i32; 3]) -> bool {
    if category != MONSTER {
        return true;
    }
    let centre = pos.map(|v| f64::from(v) + 0.5);
    !online_homes(|e| e.safe_haven)
        .iter()
        .any(|(home, radius)| homes::near(home, dimension, centre, *radius))
}

/// On the interval, each enchanted item worn, in the off hand or in the hand loses some damage.
fn mend_runes(host: &mut dyn Host, player: u128, selected: usize) {
    let Some((interval, points)) = effects_of(player).and_then(|e| e.rune_repair) else {
        return;
    };
    if !due(&REPAIR_DUE, player, u64::from(interval) * TICKS_PER_SECOND) {
        return;
    }
    let slots = [
        EquipmentSlot::Head,
        EquipmentSlot::Chest,
        EquipmentSlot::Legs,
        EquipmentSlot::Feet,
        EquipmentSlot::OffHand,
        EquipmentSlot::MainHand,
    ];
    for slot in slots {
        let index = match slot {
            EquipmentSlot::MainHand => Some(selected),
            worn => inventory_index(worn),
        };
        let (Some(index), Some(stack)) = (index, host.player_equipment(player, slot)) else {
            continue;
        };
        if stack.damage == 0 || stack.enchantments().is_empty() {
            continue;
        }
        let mended = ItemStack {
            damage: stack.damage.saturating_sub(points),
            ..stack.clone()
        };
        let _ = host.set_inventory_slot_typed(
            player,
            NativeInventoryKind::Player,
            index,
            Some(&stack),
            Some(mended),
        );
    }
}

/// Whether a timer of `every` ticks for the player came due this tick; the first call only starts it.
fn due(timers: &Mutex<BTreeMap<u128, u64>>, player: u128, every: u64) -> bool {
    let now = queue::now();
    let mut timers = lock(timers);
    match timers.get(&player) {
        Some(at) if now >= *at => {
            timers.insert(player, now + every);
            true
        }
        Some(_) => false,
        None => {
            timers.insert(player, now + every);
            false
        }
    }
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

/// A player is breaking the block at `pos` (`placed` when a player had placed it): a block they placed is noted
/// for salvage, and a ripe crop they replant is noted, to take a seed from its drops.
pub fn on_break(host: &mut dyn Host, player: u128, dimension: &str, pos: [i32; 3], placed: bool) {
    let Some(effects) = effects_of(player) else {
        return;
    };
    let Some(level) = host.level(dimension) else {
        return;
    };
    let Some(block) = host.level_block(level, pos) else {
        return;
    };
    // A planted crop has an age; its harvest is not salvaged.
    if placed && effects.salvage && host.level_block_property(level, pos, AGE).is_none() {
        let mut salvage = lock(&SALVAGE);
        salvage.retain(|(_, d, p, _)| d != dimension || *p != pos);
        salvage.push((player, dimension.to_string(), pos, block.clone()));
    }
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

/// A block broken by `player` drops `drops`: a placed block noted for salvage drops itself instead (unless it
/// already does), and a noted
/// ripe crop gives up one of its seeds to be planted again.
pub fn on_drops(
    host: &dyn Host,
    player: u128,
    dimension: &str,
    pos: [i32; 3],
    drops: &mut Vec<ItemStack>,
) {
    let salvaged = {
        let mut salvage = lock(&SALVAGE);
        let at = salvage
            .iter()
            .position(|(p, d, at, _)| *p == player && d == dimension && *at == pos);
        at.map(|at| salvage.remove(at).3)
    };
    if let Some(block) = salvaged {
        // A block that already drops itself keeps that drop: it may carry what a bare item would lose (a shulker
        // box's contents, a banner's patterns, a name).
        if drops.iter().any(|stack| *stack.item == *block) {
            return;
        }
        if let Some(item) = host.item_from_snbt(&format!("{{id:\"{block}\",count:1}}")) {
            *drops = vec![item];
            return;
        }
    }
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
    if !due(&GROWTH_DUE, player, u64::from(interval) * TICKS_PER_SECOND) {
        return;
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
    lock(&ARMOR_RUNES).clear();
    lock(&SEEN_EFFECTS).clear();
    lock(&REPAIR_DUE).clear();
    lock(&SALVAGE).clear();
    lock(&IN_BED).clear();
    lock(&RESTED).clear();
    lock(&HOME_DUE).clear();
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
        run_for(host, PLAYER, OVERWORLD, FEET, health, 0);
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
            on_break(&mut host, PLAYER, OVERWORLD, pos, false);
            host.blocks.remove(&pos);
            let mut drops = vec![
                ItemStack::new("minecraft:wheat", 1),
                ItemStack::new("minecraft:wheat_seeds", 2),
            ];
            on_drops(&host, PLAYER, OVERWORLD, pos, &mut drops);
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

    fn enchanted(item: &str, enchantments: &[&str], damage: u32) -> ItemStack {
        let mut stack = ItemStack::new(item, 1);
        stack.damage = damage;
        for e in enchantments {
            stack.data_mut().enchantments.push(((*e).into(), 1));
        }
        stack
    }

    fn wear(host: &mut TestHost, index: usize, stack: ItemStack) {
        host.slots.resize(41, None);
        host.slots[index] = Some(stack);
    }

    #[test]
    fn enchanted_weapons_hit_harder_by_their_enchantments_up_to_the_most() {
        // Given 3% per enchantment up to 15%, and a sword with two enchantments and one with six
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(
            &mut host,
            json!([{"type": "enchanted_weapon", "per_enchantment": 0.03, "max": 0.15}]),
        );
        let two = enchanted("minecraft:iron_sword", &["a:a", "a:b"], 0);
        let six = enchanted(
            "minecraft:iron_sword",
            &["a:a", "a:b", "a:c", "a:d", "a:e", "a:f"],
            0,
        );

        // When each hits for 10, and a plain sword does
        let hit = |weapon: Option<&ItemStack>| {
            let mut amount: f32 = 10.0;
            on_weapon_hit(ENTITY as i32, weapon, &mut amount);
            amount
        };
        let plain = ItemStack::new("minecraft:iron_sword", 1);

        // Then the first deals 10.6, the second 11.5, and the plain one 10
        assert!((hit(Some(&two)) - 10.6).abs() < 1e-4);
        assert!((hit(Some(&six)) - 11.5).abs() < 1e-4);
        assert_eq!(hit(Some(&plain)), 10.0);
    }

    #[test]
    fn enchanted_armor_softens_every_blow_up_to_the_most() {
        // Given 1% per enchantment up to 8%, and three enchantments across the worn chestplate and boots
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(
            &mut host,
            json!([{"type": "enchanted_armor", "per_enchantment": 0.01, "max": 0.08}]),
        );
        wear(
            &mut host,
            38,
            enchanted("minecraft:iron_chestplate", &["a:a", "a:b"], 0),
        );
        wear(
            &mut host,
            36,
            enchanted("minecraft:iron_boots", &["a:c"], 0),
        );

        // When their tick counts the runes and a zombie hits them for 10
        tick(&mut host, 20.0);
        let mut amount = 10.0;
        crate::server::effects::on_damage(ENTITY as i32, "minecraft:mob_attack", 5, &mut amount);

        // Then they take 9.7
        assert!((amount - 9.7).abs() < 1e-4, "{amount}");
    }

    #[test]
    fn harmful_effects_are_shortened_once_when_they_come() {
        // Given poison lasting 60% as long, and poison for 200 ticks coming on the player
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(
            &mut host,
            json!([{"type": "shorter_effects", "effects": ["minecraft:poison"], "multiplier": 0.6}]),
        );
        let poison = |duration| NativeEffect {
            id: "minecraft:poison".into(),
            amplifier: 0,
            duration,
            ambient: false,
            particles: true,
        };
        host.active_effects.push(poison(200));

        // When their tick runs, the poison ticks down, their tick runs again, then a new potion gives 300 ticks
        tick(&mut host, 20.0);
        let first = host.active_effects[0].duration;
        host.active_effects[0].duration -= 1;
        tick(&mut host, 20.0);
        let kept = host.active_effects[0].duration;
        host.active_effects[0] = poison(300);
        tick(&mut host, 20.0);

        // Then it is cut to 120 once, left alone as it wears off, and the new dose cut to 180
        assert_eq!(
            (first, kept, host.active_effects[0].duration),
            (120, 119, 180)
        );
    }

    #[test]
    fn living_runes_mend_enchanted_gear_on_the_interval() {
        // Given 10 points of repair every second, a worn enchanted chestplate 50 damaged, a plain helmet 50
        // damaged, and an enchanted sword in the hand 5 damaged
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(
            &mut host,
            json!([{"type": "rune_repair", "interval_seconds": 1, "points": 10}]),
        );
        wear(
            &mut host,
            38,
            enchanted("minecraft:iron_chestplate", &["a:a"], 50),
        );
        wear(&mut host, 39, enchanted("minecraft:iron_helmet", &[], 50));
        wear(&mut host, 0, enchanted("minecraft:iron_sword", &["a:a"], 5));

        // When a little over a second of their ticks run
        for _ in 0..21 {
            tick(&mut host, 20.0);
        }

        // Then the chestplate is at 40, the helmet untouched, and the sword fully mended
        let damage = |i: usize| host.slots[i].as_ref().unwrap().damage;
        assert_eq!((damage(38), damage(39), damage(0)), (40, 50, 0));
    }

    fn at_home(host: &mut TestHost) {
        crate::server::homes::set(host, PLAYER, OVERWORLD, [0, 64, 0], "bed");
    }

    #[test]
    fn blocks_the_player_placed_drop_themselves() {
        // Given salvage, and a placed glass, a glass nobody placed, and a placed block with no item
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(&mut host, json!([{"type": "salvage"}]));
        host.unknown_items.push("minecraft:fire".into());
        let breaks = [
            ([1, 64, 0], "minecraft:glass", true),
            ([2, 64, 0], "minecraft:glass", false),
            ([3, 64, 0], "minecraft:fire", true),
        ];

        // When the player breaks each and they drop nothing
        let mut dropped = Vec::new();
        for (pos, block, placed) in breaks {
            host.blocks.insert(pos, (block.to_string(), vec![]));
            on_break(&mut host, PLAYER, OVERWORLD, pos, placed);
            let mut drops = Vec::new();
            on_drops(&host, PLAYER, OVERWORLD, pos, &mut drops);
            dropped.push(drops.iter().map(|s| s.item.to_string()).collect::<Vec<_>>());
        }

        // Then only the placed glass drops itself
        assert_eq!(
            dropped,
            vec![vec!["minecraft:glass".to_string()], vec![], vec![]]
        );
    }

    #[test]
    fn salvage_leaves_a_block_that_already_drops_itself_as_it_drops() {
        // Given salvage, and a placed shulker box whose own drop carries its contents
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(&mut host, json!([{"type": "salvage"}]));
        let pos = [1, 64, 0];
        host.blocks
            .insert(pos, ("minecraft:shulker_box".to_string(), vec![]));
        let mut full = ItemStack::new("minecraft:shulker_box", 1);
        full.damage = 7; // stands in for the contents component the bare item would lose

        // When the player breaks it
        on_break(&mut host, PLAYER, OVERWORLD, pos, true);
        let mut drops = vec![full.clone()];
        on_drops(&host, PLAYER, OVERWORLD, pos, &mut drops);

        // Then its own drop is kept as it was, not swapped for a bare shulker box
        assert_eq!(drops, vec![full]);
    }

    #[test]
    fn a_full_sleep_heals_and_clears_but_a_nap_does_not() {
        // Given restful sleep clearing poison, and a poisoned player
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(
            &mut host,
            json!([{"type": "restful_sleep", "clears": ["minecraft:poison"]}]),
        );
        host.active_effects.push(NativeEffect {
            id: "minecraft:poison".into(),
            amplifier: 0,
            duration: 400,
            ambient: false,
            particles: true,
        });

        // When they nap for 10 ticks, then sleep 100, their tick running after each
        on_bed_enter(PLAYER);
        for _ in 0..10 {
            tick(&mut host, 10.0);
        }
        on_bed_leave(PLAYER);
        tick(&mut host, 10.0);
        let after_nap = (host.health_set.clone(), host.active_effects.len());
        on_bed_enter(PLAYER);
        for _ in 0..100 {
            tick(&mut host, 10.0);
        }
        on_bed_leave(PLAYER);
        tick(&mut host, 10.0);

        // Then the nap did nothing, and the sleep healed them to their 24 health and took the poison off
        assert_eq!(after_nap, (vec![], 1));
        assert_eq!(host.health_set, vec![(PLAYER, 24.0)]);
        assert!(host.active_effects.is_empty());
    }

    #[test]
    fn near_home_the_player_gets_its_effect() {
        // Given regeneration within 16 blocks of home, and a bed at 0 64 0
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(
            &mut host,
            json!([{"type": "near_home", "radius": 16, "effect": "minecraft:regeneration", "amplifier": 0}]),
        );
        at_home(&mut host);

        // When the player spends two seconds by the bed, then two seconds 40 blocks away
        for _ in 0..40 {
            tick(&mut host, 20.0);
        }
        let near = host.effects_given.len();
        host.effects_given.clear();
        for _ in 0..40 {
            run(&mut host, Vec::new());
            run_for(&mut host, PLAYER, OVERWORLD, [40.5, 64.0, 0.5], 20.0, 0);
        }

        // Then they had regeneration at home and none away
        assert!(near > 0);
        assert!(host.effects_given.is_empty());
    }

    #[test]
    fn explosions_near_home_spare_the_blocks_while_the_owner_is_on() {
        // Given a home ward of 24 blocks, a bed at 0 64 0, and the owner online
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(&mut host, json!([{"type": "home_ward", "radius": 24}]));
        at_home(&mut host);
        crate::server::state::logged_in(PLAYER, "Ann");

        // When a creeper blows up blocks by the bed and far away, and one in the nether by the same spot
        let mut blocks = vec![[5, 64, 5], [100, 64, 0]];
        on_explosion(OVERWORLD, &mut blocks);
        let mut nether = vec![[5, 64, 5]];
        on_explosion("minecraft:the_nether", &mut nether);

        // Then only the far block and the nether one break
        assert_eq!(blocks, vec![[100, 64, 0]]);
        assert_eq!(nether, vec![[5, 64, 5]]);
    }

    #[test]
    fn monsters_do_not_spawn_near_a_safe_home() {
        // Given a safe haven of 32 blocks, a bed at 0 64 0, and the owner online
        let _serial = exclusive();
        let mut host = TestHost::default();
        owning(&mut host, json!([{"type": "safe_haven", "radius": 32}]));
        at_home(&mut host);
        crate::server::state::logged_in(PLAYER, "Ann");

        // When monsters and animals would spawn by the bed, and monsters far away
        let near = may_spawn(OVERWORLD, "monster", [10, 64, 10]);
        let animals = may_spawn(OVERWORLD, "creature", [10, 64, 10]);
        let far = may_spawn(OVERWORLD, "monster", [80, 64, 0]);

        // Then only the monsters by the bed are kept away
        assert_eq!((near, animals, far), (false, true, true));
    }
}
