//! What active focus areas give: more of the XP kills and ores drop, and points for their tasks (breaking blocks,
//! smelting, travelling, taming, breeding). Points add up per player, fractions included, and whole ones are given
//! in the player's own tick, where Horizon reaches them.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

use horizon_client::mod_api::Host;
use skills_core::defs::{Reward, TargetKind};
use skills_core::locks::matches;

use super::data::HostFacts;
use super::effects::{player_of_entity, rewards_of};
use super::{lock, queue};

const PERCENT: f64 = 100.0;
/// A mob's death counts for the player who hurt it last within this many ticks.
const KILL_TICKS: u64 = 100;
/// A block's XP counts for the player who broke it within this many ticks.
const BREAK_TICKS: u64 = 1;
/// A step longer than this many blocks in a tick is a teleport, not travel.
const MOST_STEP: f64 = 10.0;
/// Placed blocks remembered, the oldest forgotten first.
const PLACED_KEPT: usize = 65_536;

/// A block's place: its dimension and position.
type Spot = (String, [i32; 3]);

/// Points each player has earned and not been given yet.
static OWED: Mutex<BTreeMap<u128, f64>> = Mutex::new(BTreeMap::new());
/// Who broke the block at each spot, and when, for the XP it drops.
static BROKEN: Mutex<BTreeMap<[i32; 3], (u128, u64)>> = Mutex::new(BTreeMap::new());
/// The player who last hurt each mob, and when.
static HURT: Mutex<BTreeMap<u32, (u128, u64)>> = Mutex::new(BTreeMap::new());
/// Spots where a player placed a block, by dimension, oldest first.
static PLACED: Mutex<(BTreeSet<Spot>, VecDeque<Spot>)> =
    Mutex::new((BTreeSet::new(), VecDeque::new()));
/// Where each travelling player was on the last tick.
static WHERE: Mutex<BTreeMap<u128, (String, [f64; 3])>> = Mutex::new(BTreeMap::new());

fn owe(player: u128, points: f64) {
    if points > 0.0 {
        *lock(&OWED).entry(player).or_default() += points;
    }
}

/// The points the player has earned and not been given yet.
#[cfg(test)]
pub fn owed(player: u128) -> f64 {
    lock(&OWED).get(&player).copied().unwrap_or(0.0)
}

/// The sum of what the player's focus gives through `pick`.
fn summed(player: u128, pick: impl Fn(&Reward) -> Option<f64>) -> f64 {
    rewards_of(player).iter().filter_map(pick).sum()
}

/// A player placed a block: breaking it later gives nothing.
pub fn on_place(dimension: &str, pos: [i32; 3]) {
    let spot = (dimension.to_string(), pos);
    let mut placed = lock(&PLACED);
    if placed.0.insert(spot.clone()) {
        placed.1.push_back(spot);
    }
    while placed.1.len() > PLACED_KEPT {
        if let Some(old) = placed.1.pop_front() {
            placed.0.remove(&old);
        }
    }
}

/// A player breaks a block: points for each reward it matches, and the block's XP noted as theirs. `rewarded` is
/// false for a creative or spectator player, who earns nothing. Whether a player had placed the block, so other
/// rewards for work (earning) can leave it out.
pub fn on_break(
    host: &mut dyn Host,
    player: u128,
    rewarded: bool,
    dimension: &str,
    pos: [i32; 3],
) -> bool {
    let spot = (dimension.to_string(), pos);
    let placed = {
        let mut placed = lock(&PLACED);
        let was = placed.0.remove(&spot);
        if was {
            placed.1.retain(|s| *s != spot);
        }
        was
    };
    if rewarded {
        pay_break(host, player, dimension, pos, placed);
    }
    placed
}

/// [`on_break`]'s rewards for a player who earns them.
fn pay_break(host: &mut dyn Host, player: u128, dimension: &str, pos: [i32; 3], placed: bool) {
    lock(&BROKEN).insert(pos, (player, queue::now()));
    let rewards = rewards_of(player);
    if !rewards.iter().any(|r| matches!(r, Reward::Break { .. })) {
        return;
    }
    let Some(level) = host.level(dimension) else {
        return;
    };
    let Some(block) = host.level_block(level, pos) else {
        return;
    };
    let mut points = 0.0;
    for reward in &rewards {
        let Reward::Break {
            blocks,
            points: each,
            state,
            ..
        } = reward
        else {
            continue;
        };
        let fits = if state.is_empty() {
            !placed
        } else {
            state.iter().all(|(key, value)| {
                host.level_block_property(level, pos, key).as_deref() == Some(value.as_str())
            })
        };
        if fits && matches(blocks, TargetKind::Block, &block, &HostFacts(&*host)) {
            points += each;
        }
    }
    owe(player, points);
}

/// A broken block drops `experience` points: the player who broke it gets their ore bonus on top.
pub fn block_xp(pos: [i32; 3], experience: u32) {
    let Some((player, at)) = lock(&BROKEN).remove(&pos) else {
        return;
    };
    if queue::now() > at + BREAK_TICKS {
        return;
    }
    let percent = summed(player, |r| match r {
        Reward::BlockXp { bonus_percent } => Some(f64::from(*bonus_percent)),
        _ => None,
    });
    owe(player, f64::from(experience) * percent / PERCENT);
}

/// An entity hurt another: when the one causing it is a player, the victim's death counts for them.
pub fn hurt(victim: i32, causing: i32) {
    let (Some(player), Ok(victim)) = (player_of_entity(causing), u32::try_from(victim)) else {
        return;
    };
    if player_of_entity(i32::try_from(victim).unwrap_or(-1)) != Some(player) {
        lock(&HURT).insert(victim, (player, queue::now()));
    }
}

/// The player who hurt the mob last, if that was recent enough to count its death as theirs.
pub fn credited(entity: u32) -> Option<u128> {
    let (player, at) = *lock(&HURT).get(&entity)?;
    (queue::now() <= at + KILL_TICKS).then_some(player)
}

/// A mob killed by a player drops `xp` points: the player who hurt it last gets their kill bonus on top.
pub fn kill_xp(entity: u32, xp: i32) {
    let Some((player, at)) = lock(&HURT).remove(&entity) else {
        return;
    };
    if queue::now() > at + KILL_TICKS || xp <= 0 {
        return;
    }
    let percent = summed(player, |r| match r {
        Reward::Kill { bonus_percent } => Some(f64::from(*bonus_percent)),
        _ => None,
    });
    owe(player, f64::from(xp) * percent / PERCENT);
}

/// The player took `count` items out of a furnace.
pub fn on_smelt(_host: &mut dyn Host, player: u128, count: u32) {
    let each = summed(player, |r| match r {
        Reward::Smelt { points } => Some(*points),
        _ => None,
    });
    owe(player, each * f64::from(count));
}

/// The player tamed an animal.
pub fn on_tame(_host: &mut dyn Host, player: u128) {
    owe(
        player,
        summed(player, |r| match r {
            Reward::Tame { points } => Some(*points),
            _ => None,
        }),
    );
}

/// The player bred a pair of animals.
pub fn on_breed(_host: &mut dyn Host, player: u128) {
    owe(
        player,
        summed(player, |r| match r {
            Reward::Breed { points } => Some(*points),
            _ => None,
        }),
    );
}

/// Each player's place this tick (dimension, position) and whether it counts as travel: not in creative flight,
/// creative or spectator. The distance since the last tick earns travel points, unless the player changed
/// dimension or moved too far at once.
pub fn travel(players: &[(u128, &str, [f64; 3], bool)]) {
    let mut seen = lock(&WHERE);
    for &(player, dimension, at, counts) in players {
        let before = seen.insert(player, (dimension.to_string(), at));
        let Some((was_in, was_at)) = before else {
            continue;
        };
        if !counts || was_in != dimension {
            continue;
        }
        let step = (0..3)
            .map(|i| (at[i] - was_at[i]).powi(2))
            .sum::<f64>()
            .sqrt();
        if step > MOST_STEP {
            continue;
        }
        super::earning::on_travel(player, step);
        let each = summed(player, |r| match r {
            Reward::Travel { points } => Some(*points),
            _ => None,
        });
        owe(player, each * step);
    }
}

/// [`travel`] for the players on the server now, who are also remembered by entity id for kills.
pub fn watch(host: &dyn Host) {
    let players = host.server_players();
    let steps: Vec<(u128, &str, [f64; 3], bool)> = players
        .iter()
        .map(|p| {
            super::effects::remember_entity(p.uuid, u32::try_from(p.entity_id).unwrap_or_default());
            let survival = rewarded(p.game_mode);
            (
                p.uuid,
                p.dimension.as_str(),
                p.position,
                survival && !p.flying,
            )
        })
        .collect();
    travel(&steps);
}

/// Vanilla's game mode numbers in which a player earns focus rewards.
const SURVIVAL: i32 = 0;
const ADVENTURE: i32 = 2;

/// Whether a player in this game mode earns focus rewards: survival and adventure do.
pub fn rewarded(game_mode: i32) -> bool {
    matches!(game_mode, SURVIVAL | ADVENTURE)
}

/// The player's own tick (`player_tick`, where Horizon reaches them): gives them the whole points they are owed,
/// keeping the fraction; when giving fails they wait for the next tick.
pub fn pay(host: &mut dyn Host, player: u128) {
    let due = lock(&OWED).get(&player).copied().unwrap_or(0.0).floor();
    if due < 1.0 {
        return;
    }
    let points = due.min(f64::from(u32::MAX)) as u32;
    if host.give_experience(player, points)
        && let Some(owed) = lock(&OWED).get_mut(&player)
    {
        *owed -= f64::from(points);
    }
}

/// A player left: what they were owed below a point, and where they were, are forgotten.
pub fn forget(player: u128) {
    lock(&OWED).remove(&player);
    lock(&WHERE).remove(&player);
}

/// Forgets what this module keeps for the running world, as the server stops.
pub(crate) fn reset() {
    lock(&OWED).clear();
    lock(&BROKEN).clear();
    lock(&HURT).clear();
    *lock(&PLACED) = (BTreeSet::new(), VecDeque::new());
    lock(&WHERE).clear();
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::Host;
    use serde_json::json;

    use super::*;
    use crate::server::data::tests::load;
    use crate::server::effects::{remember_entity, sync_attributes};
    use crate::server::state::set_state;
    use crate::test_host::{OVERWORLD, TestHost, exclusive};
    use skills_core::progress::PlayerState;

    const PLAYER: u128 = 0x1234;
    const ENTITY: u32 = 42;
    const LOG: [i32; 3] = [1, 64, 1];

    /// Working and Fighting with the shipped kinds of reward, and a player focused on the `areas` named.
    fn focused(areas: &[&str]) -> TestHost {
        let mut host = TestHost::default();
        host.block_tags
            .push(("minecraft:oak_log".into(), "minecraft:logs".into()));
        let working = json!({"format": 1, "id": "reskilled:working", "name": "w", "skills": [], "rewards": [
            {"task": "block_xp", "bonus_percent": 50},
            {"task": "break", "blocks": {"tag": "#minecraft:logs"}, "points": 0.25},
            {"task": "break", "blocks": {"id": "minecraft:wheat"}, "points": 0.5, "state": {"age": "7"}},
            {"task": "smelt", "points": 0.1}]});
        let fighting = json!({"format": 1, "id": "reskilled:fighting", "name": "f", "skills": [], "rewards": [
            {"task": "kill", "bonus_percent": 50}]});
        let roaming = json!({"format": 1, "id": "reskilled:roaming", "name": "r", "skills": [], "rewards": [
            {"task": "travel", "points": 0.02}, {"task": "tame", "points": 5}, {"task": "breed", "points": 2}]});
        load(
            &mut host,
            &[(
                "focus",
                vec![
                    ("reskilled:working", working),
                    ("reskilled:fighting", fighting),
                    ("reskilled:roaming", roaming),
                ],
            )],
        );
        let state = PlayerState {
            focus: areas.iter().map(|a| format!("reskilled:{a}")).collect(),
            ..PlayerState::default()
        };
        set_state(&mut host, PLAYER, state).unwrap();
        sync_attributes(&mut host, PLAYER);
        remember_entity(PLAYER, ENTITY);
        host.commands.clear();
        host
    }

    fn log_at(host: &mut TestHost, pos: [i32; 3]) {
        host.blocks
            .insert(pos, ("minecraft:oak_log".into(), vec![]));
    }

    fn wheat_at(host: &mut TestHost, pos: [i32; 3], age: &str) {
        host.blocks.insert(
            pos,
            ("minecraft:wheat".into(), vec![("age".into(), age.into())]),
        );
    }

    /// The points given so far.
    fn paid(host: &TestHost) -> u32 {
        host.experience_given.get(&PLAYER).copied().unwrap_or(0)
    }

    /// The player's own tick, where they are paid.
    fn tick(host: &mut TestHost) {
        pay(host, PLAYER);
    }

    #[test]
    fn breaking_logs_adds_up_to_whole_points_paid_on_the_tick() {
        // Given a player focused on Working, whose logs give a quarter point each
        let _serial = exclusive();
        let mut host = focused(&["working"]);

        // When they break three logs and the tick runs, then a fourth and five more and the tick runs
        for x in 0..3 {
            log_at(&mut host, [x, 64, 0]);
            on_break(&mut host, PLAYER, true, OVERWORLD, [x, 64, 0]);
        }
        tick(&mut host);
        let after_three = paid(&host);
        for x in 3..9 {
            log_at(&mut host, [x, 64, 0]);
            on_break(&mut host, PLAYER, true, OVERWORLD, [x, 64, 0]);
        }
        tick(&mut host);

        // Then nothing is paid for three quarters, then two whole points for nine, the last quarter kept
        assert_eq!(after_three, 0);
        assert_eq!(paid(&host), 2);
        assert_eq!(owed(PLAYER), 0.25);
    }

    #[test]
    fn breaking_says_whether_the_block_was_placed_by_a_player() {
        // Given a player focused on Working and a log nobody placed
        let _serial = exclusive();
        let mut host = focused(&["working"]);
        log_at(&mut host, LOG);

        // When they place a log and break it, then break the natural one
        on_place(OVERWORLD, LOG);
        let placed = on_break(&mut host, PLAYER, true, OVERWORLD, LOG);
        let natural = on_break(&mut host, PLAYER, true, OVERWORLD, LOG);

        // Then the first break says the block was placed, so other rewards (earning) can skip it
        assert_eq!((placed, natural), (true, false));
    }

    #[test]
    fn a_block_a_player_placed_gives_nothing() {
        // Given a player focused on Working
        let _serial = exclusive();
        let mut host = focused(&["working"]);
        log_at(&mut host, LOG);

        // When they place a log and break it, four times, then break a log nobody placed four times at that spot
        for _ in 0..4 {
            on_place(OVERWORLD, LOG);
            on_break(&mut host, PLAYER, true, OVERWORLD, LOG);
        }
        tick(&mut host);
        let placed = paid(&host);
        for _ in 0..4 {
            on_break(&mut host, PLAYER, true, OVERWORLD, LOG);
        }
        tick(&mut host);

        // Then the placed logs give nothing and the others a point
        assert_eq!(placed, 0);
        assert_eq!(paid(&host), 1);
    }

    #[test]
    fn only_a_grown_crop_gives_even_when_planted_by_hand() {
        // Given a player focused on Working, a young wheat and a grown one they planted themselves
        let _serial = exclusive();
        let mut host = focused(&["working"]);
        wheat_at(&mut host, [0, 64, 0], "3");
        wheat_at(&mut host, [1, 64, 0], "7");
        on_place(OVERWORLD, [1, 64, 0]);

        // When they harvest both
        on_break(&mut host, PLAYER, true, OVERWORLD, [0, 64, 0]);
        on_break(&mut host, PLAYER, true, OVERWORLD, [1, 64, 0]);

        // Then only the grown one counts, planted or not
        assert_eq!(owed(PLAYER), 0.5);
    }

    #[test]
    fn nothing_is_given_without_the_focus_or_outside_survival() {
        // Given a player focused on Fighting only, and one focused on Working who plays in creative
        let _serial = exclusive();
        let mut host = focused(&["fighting"]);
        log_at(&mut host, LOG);

        // When the first breaks a log, and as the second (creative) breaks one too
        on_break(&mut host, PLAYER, true, OVERWORLD, LOG);
        let without = owed(PLAYER);
        let mut host = focused(&["working"]);
        log_at(&mut host, LOG);
        on_break(&mut host, PLAYER, false, OVERWORLD, LOG);

        // Then neither earns anything
        assert_eq!((without, owed(PLAYER)), (0.0, 0.0));
    }

    #[test]
    fn ores_drop_half_again_their_xp() {
        // Given a player focused on Working breaking an ore that drops 4 points, and a block nobody is breaking
        let _serial = exclusive();
        let mut host = focused(&["working"]);
        host.blocks
            .insert(LOG, ("minecraft:coal_ore".into(), vec![]));

        // When the ore is broken and drops its XP, and XP drops elsewhere
        on_break(&mut host, PLAYER, true, OVERWORLD, LOG);
        block_xp(LOG, 4);
        block_xp([9, 9, 9], 4);
        tick(&mut host);

        // Then the player is given half of the ore's 4 points more, and nothing for the other block
        assert_eq!(paid(&host), 2);
    }

    #[test]
    fn kills_drop_half_again_their_xp_to_the_player_who_hurt_the_mob() {
        // Given a player focused on Fighting who hurt mob 77, and a mob nobody hurt
        let _serial = exclusive();
        let mut host = focused(&["fighting"]);
        let entity = i32::try_from(ENTITY).unwrap();
        hurt(77, entity);
        hurt(entity, 99);

        // When mob 77 dies dropping 10 points, and mob 88 dropping 10, and the tick runs
        kill_xp(77, 10);
        kill_xp(88, 10);
        tick(&mut host);

        // Then the player gets 5 more, once
        assert_eq!(paid(&host), 5);
        kill_xp(77, 10);
        assert_eq!(owed(PLAYER), 0.0);
    }

    #[test]
    fn a_hit_long_before_the_death_does_not_count() {
        // Given a player focused on Fighting who hurt mob 77, then 101 ticks passed
        let _serial = exclusive();
        let mut host = focused(&["fighting"]);
        hurt(77, i32::try_from(ENTITY).unwrap());
        for _ in 0..=KILL_TICKS {
            crate::server::queue::run(&mut host, vec![]);
        }

        // When mob 77 dies dropping 10 points
        kill_xp(77, 10);

        // Then the kill is not theirs
        assert_eq!(owed(PLAYER), 0.0);
    }

    #[test]
    fn smelting_taming_and_breeding_give_points() {
        // Given a player focused on Working and Roaming
        let _serial = exclusive();
        let mut host = focused(&["working", "roaming"]);

        // When they take 20 items out of a furnace, tame an animal and breed a pair, and the tick runs
        on_smelt(&mut host, PLAYER, 20);
        on_tame(&mut host, PLAYER);
        on_breed(&mut host, PLAYER);
        tick(&mut host);

        // Then they get 2 + 5 + 2 points
        assert_eq!(paid(&host), 9);
    }

    #[test]
    fn travelling_gives_points_by_distance_but_not_by_teleport_or_flight() {
        // Given a player focused on Roaming, at 0, 64, 0
        let _serial = exclusive();
        let mut host = focused(&["roaming"]);
        let mut at = [0.0, 64.0, 0.0];
        travel(&[(PLAYER, OVERWORLD, at, true)]);

        // When they walk 50 blocks a step at a time, teleport 1000 blocks, fly 100 blocks, then go to the Nether
        for _ in 0..50 {
            at[0] += 1.0;
            travel(&[(PLAYER, OVERWORLD, at, true)]);
        }
        let walked = owed(PLAYER);
        at[0] += 1000.0;
        travel(&[(PLAYER, OVERWORLD, at, true)]);
        for _ in 0..100 {
            at[0] += 1.0;
            travel(&[(PLAYER, OVERWORLD, at, false)]);
        }
        travel(&[(PLAYER, "minecraft:the_nether", [0.0, 64.0, 0.0], true)]);
        tick(&mut host);

        // Then the walk earns its point, and nothing else does
        assert!((walked - 1.0).abs() < 1e-9, "{walked}");
        assert_eq!(paid(&host), 1);
    }

    #[test]
    fn points_wait_when_they_cannot_be_given() {
        // Given a player owed five points, and a tick where giving experience fails
        let _serial = exclusive();
        let mut host = focused(&["roaming"]);
        on_tame(&mut host, PLAYER);
        host.give_levels_fails = true;

        // When their tick runs, then runs again once giving works
        tick(&mut host);
        let kept = owed(PLAYER);
        host.give_levels_fails = false;
        tick(&mut host);

        // Then nothing is lost: the points are given the next time
        assert_eq!(kept, 5.0);
        assert_eq!(paid(&host), 5);
        let _ = host.window();
    }
}
