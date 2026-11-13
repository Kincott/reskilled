//! Perk effects in game: attribute modifiers through vanilla's `/attribute`, the dig lock, damage, XP gain, anvil
//! cost, bonus drops and mount modifiers.
//!
//! Modifiers added with `/attribute` stay on the player and are saved with them, so what this mod applied is kept in
//! a player attachment too. After a login, respawn or data reload the next sync removes every modifier it knows of
//! before adding the wanted ones (vanilla refuses to add an id already there); between those, it applies the diff.
//! The damage hook has no `Host`, so each player's effects are cached here at every sync.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, Compound, Holder, Host, ItemStack,
    RandomSource, Tag,
};
use skills_core::defs::{Action, DamageKind, Direction};
use skills_core::effects::{
    ActiveEffects, AttributeMod, active_effects, attribute_commands, damage_multiplier, dig_lock,
};
use skills_core::locks::{TargetKind, check, matches};
use skills_core::progress::level;

use super::data::{HostFacts, current, requirements};
use super::lock;
use super::queue::Job;
use super::state::state_of;

/// The one tag the applied modifiers are kept in, as JSON.
const JSON_TAG: &str = "json";
/// A block's own loot table is `<namespace>:blocks/<block>`.
const BLOCK_TABLES: &str = "blocks/";

/// The modifiers this mod last applied to the player. Kept through death (the modifiers are not, so a respawn
/// resyncs in full) and not sent to clients.
pub static APPLIED: AttachmentType<Vec<AttributeMod>> = AttachmentType {
    namespace: "skills_and_perks",
    name: "applied",
    default: Vec::new,
    codec: Some(AttachmentCodec {
        encode: encode_applied,
        decode: decode_applied,
    }),
    copy_on_death: true,
    sync: false,
};

fn encode_applied(applied: &Vec<AttributeMod>) -> Result<Compound, AttachmentError> {
    let json = serde_json::to_string(applied).map_err(|_| AttachmentError::Encode)?;
    Ok([(JSON_TAG.into(), Tag::String(json.as_str().into()))].into())
}

fn decode_applied(saved: &Compound) -> Result<Vec<AttributeMod>, AttachmentError> {
    match saved.get(&JSON_TAG.into()) {
        Some(Tag::String(json)) => {
            serde_json::from_str(&json.to_string_lossy()).map_err(|_| AttachmentError::Decode)
        }
        _ => Err(AttachmentError::Decode),
    }
}

/// Each online player's active effects, as of their last sync.
static EFFECTS: Mutex<BTreeMap<u128, Arc<ActiveEffects>>> = Mutex::new(BTreeMap::new());
/// Entity id to player, learned whenever a hook names an online player.
static ENTITIES: Mutex<BTreeMap<u32, u128>> = Mutex::new(BTreeMap::new());
/// Each player's main-hand item, and whether they ignore locks, for the dig lock.
static MAIN_HAND: Mutex<BTreeMap<u128, (String, bool)>> = Mutex::new(BTreeMap::new());
/// Players whose next sync removes everything known before adding.
static FULL: Mutex<BTreeSet<u128>> = Mutex::new(BTreeSet::new());
/// The fraction of an XP point each player is owed.
static XP_CARRY: Mutex<BTreeMap<u128, f64>> = Mutex::new(BTreeMap::new());
/// The mount each player rides (its uuid as `/attribute` takes it) and the modifiers put on it.
static MOUNTED: Mutex<BTreeMap<u128, (String, Vec<AttributeMod>)>> = Mutex::new(BTreeMap::new());

/// Vanilla 26.3's damage type tags `is_projectile`, `is_fall`, `is_fire` and `is_explosion`, and the melee types,
/// read from the prepared game data (`damage-tags.json`, `damage-types.json`) on 2026-11-13.
const DAMAGE_KINDS: [(DamageKind, &[&str]); 5] = [
    (
        DamageKind::Melee,
        &[
            "player_attack",
            "mob_attack",
            "mob_attack_no_aggro",
            "mace_smash",
            "spear",
            "sting",
        ],
    ),
    (
        DamageKind::Projectile,
        &[
            "arrow",
            "trident",
            "mob_projectile",
            "unattributed_fireball",
            "fireball",
            "wither_skull",
            "thrown",
            "wind_charge",
        ],
    ),
    (DamageKind::Fall, &["fall", "ender_pearl", "stalagmite"]),
    (
        DamageKind::Fire,
        &[
            "in_fire",
            "campfire",
            "on_fire",
            "lava",
            "hot_floor",
            "sulfur_cube_hot",
            "unattributed_fireball",
            "fireball",
        ],
    ),
    (
        DamageKind::Explosion,
        &[
            "fireworks",
            "explosion",
            "player_explosion",
            "bad_respawn_point",
        ],
    ),
];

/// The kinds a vanilla damage type counts as; a mod's types count as none, so only `any` rules apply to them.
pub fn damage_kinds(damage_type: &str) -> Vec<DamageKind> {
    let Some(path) = damage_type.strip_prefix("minecraft:") else {
        return Vec::new();
    };
    DAMAGE_KINDS
        .iter()
        .filter(|(_, types)| types.contains(&path))
        .map(|(kind, _)| *kind)
        .collect()
}

pub fn remember_entity(player: u128, entity: u32) {
    lock(&ENTITIES).insert(entity, player);
}

/// The next sync of this player removes every modifier it knows of first.
pub fn mark_full(player: u128) {
    lock(&FULL).insert(player);
}

pub fn note_main_hand(player: u128, item: Option<&str>, exempt: bool) {
    let mut main_hand = lock(&MAIN_HAND);
    match item {
        Some(item) => main_hand.insert(player, (item.to_string(), exempt)),
        None => main_hand.remove(&player),
    };
}

/// Drops what is kept about a player who left.
pub fn forget(player: u128) {
    lock(&EFFECTS).remove(&player);
    lock(&ENTITIES).retain(|_, p| *p != player);
    lock(&MAIN_HAND).remove(&player);
    lock(&XP_CARRY).remove(&player);
    lock(&MOUNTED).remove(&player);
}

fn effects_of(player: u128) -> Option<Arc<ActiveEffects>> {
    lock(&EFFECTS).get(&player).cloned()
}

fn effects_of_entity(entity: i32) -> Option<Arc<ActiveEffects>> {
    let player = *lock(&ENTITIES).get(&u32::try_from(entity).ok()?)?;
    effects_of(player)
}

/// `/attribute` names an entity by its uuid, written with hyphens.
fn uuid_text(uuid: u128) -> String {
    let hex = format!("{uuid:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// The tick's part of perk effects.
pub fn run_job(host: &mut dyn Host, job: Job) {
    match job {
        Job::SyncAttributes(player) => sync_attributes(host, player),
        Job::Mount {
            player,
            vehicle,
            mounting,
        } => mount(host, player, vehicle, mounting),
        other => log::warn!("Skills & Perks effects got a job they do not run: {other:?}"),
    }
}

/// Brings the player's modifiers in line with their active perks and main hand, and caches their effects.
pub fn sync_attributes(host: &mut dyn Host, player: u128) {
    let Some(data) = current() else { return };
    let Ok(state) = state_of(host, player) else {
        return;
    };
    let effects = active_effects(&data.defs, &state);
    let mut wanted = effects.attributes.clone();
    let main_hand = lock(&MAIN_HAND).get(&player).cloned();
    if let Some((item, false)) = main_hand {
        let levels = |skill: &str| level(&state, &data.defs, skill);
        if check(
            &requirements(host, TargetKind::Item, &item),
            Action::Break,
            &levels,
        )
        .is_err()
        {
            wanted.push(dig_lock());
        }
    }
    lock(&EFFECTS).insert(player, Arc::new(effects));
    let holder = Holder::Player(player);
    let applied = APPLIED.get(host, holder).unwrap_or_default();
    let target = uuid_text(player);
    let lines = if lock(&FULL).remove(&player) {
        let mut known = applied.clone();
        known.extend(
            wanted
                .iter()
                .filter(|w| !applied.iter().any(|a| a.id == w.id))
                .cloned(),
        );
        let mut lines = attribute_commands(&target, &known, &[]);
        lines.extend(attribute_commands(&target, &[], &wanted));
        lines
    } else {
        attribute_commands(&target, &applied, &wanted)
    };
    for line in &lines {
        if !host.run_command(line) {
            log::debug!("Skills & Perks: `{line}` did nothing");
        }
    }
    if let Err(error) = APPLIED.set(host, holder, wanted) {
        log::warn!("Skills & Perks could not record the modifiers of {player:032x}: {error:?}");
    }
}

/// Puts the rider's mount modifiers on the vehicle they got on, and takes them off the one they left.
fn mount(host: &mut dyn Host, player: u128, vehicle: u32, mounting: bool) {
    if let Some((target, modifiers)) = lock(&MOUNTED).remove(&player) {
        for line in attribute_commands(&target, &modifiers, &[]) {
            host.run_command(&line);
        }
    }
    if !mounting {
        return;
    }
    let Some(effects) = effects_of(player).filter(|e| !e.mount_attributes.is_empty()) else {
        return;
    };
    let Some(uuid) = host
        .entity(vehicle)
        .and_then(|entity| host.entity_uuid(entity))
    else {
        return;
    };
    let target = uuid_text(uuid);
    for line in attribute_commands(&target, &[], &effects.mount_attributes) {
        host.run_command(&line);
    }
    lock(&MOUNTED).insert(player, (target, effects.mount_attributes.clone()));
}

/// Scales a hit by the attacker's dealt and the victim's taken multipliers for its kinds.
pub fn on_damage(victim: i32, damage_type: &str, causing: i32, amount: &mut f32) {
    let kinds = damage_kinds(damage_type);
    let dealt =
        effects_of_entity(causing).map_or(1.0, |e| damage_multiplier(&e, Direction::Dealt, &kinds));
    let taken =
        effects_of_entity(victim).map_or(1.0, |e| damage_multiplier(&e, Direction::Taken, &kinds));
    let multiplier = dealt * taken;
    if multiplier != 1.0 {
        *amount = (f64::from(*amount) * multiplier) as f32;
    }
}

/// Scales XP points gained, carrying the fraction to the next gain so small orbs add up.
pub fn on_xp(player: u128, amount: &mut i32) {
    let Some(effects) = effects_of(player).filter(|e| e.xp_gain != 1.0) else {
        return;
    };
    if *amount <= 0 {
        return;
    }
    let mut carry = lock(&XP_CARRY);
    let owed = f64::from(*amount) * effects.xp_gain + carry.get(&player).copied().unwrap_or(0.0);
    let whole = owed.floor();
    carry.insert(player, owed - whole);
    *amount = whole as i32;
}

/// Scales an anvil's level cost, rounding up and never below 1.
pub fn on_anvil(player: u128, cost: &mut i32) {
    let Some(effects) = effects_of(player).filter(|e| e.anvil_cost != 1.0) else {
        return;
    };
    if *cost > 0 {
        *cost = ((f64::from(*cost) * effects.anvil_cost).ceil() as i32).max(1);
    }
}

/// For each of the player's drop bonuses that matches the broken block, by its chance, one more of a random stack.
pub fn bonus_drops(
    host: &mut dyn Host,
    player: u128,
    dimension: &str,
    table: &str,
    drops: &mut Vec<ItemStack>,
) {
    let Some((namespace, path)) = table.split_once(':') else {
        return;
    };
    let Some(block) = path
        .strip_prefix(BLOCK_TABLES)
        .map(|block| format!("{namespace}:{block}"))
    else {
        return;
    };
    let Some(effects) = effects_of(player) else {
        return;
    };
    if drops.is_empty() || effects.drops.is_empty() {
        return;
    }
    let facts = HostFacts(&*host);
    let chances: Vec<f64> = effects
        .drops
        .iter()
        .filter(|(matcher, _)| matches(matcher, TargetKind::Block, &block, &facts))
        .map(|(_, chance)| *chance)
        .collect();
    let Some(random) = host.level_random(dimension) else {
        return;
    };
    for chance in chances {
        if random.next_double() < chance {
            let count = i32::try_from(drops.len()).unwrap_or(i32::MAX);
            let index = random.next_int_bound(count).unwrap_or(0);
            let extra = drops[usize::try_from(index).unwrap_or(0)].clone();
            drops.push(extra);
        }
    }
}

#[cfg(test)]
pub(crate) fn reset() {
    lock(&EFFECTS).clear();
    lock(&ENTITIES).clear();
    lock(&MAIN_HAND).clear();
    lock(&FULL).clear();
    lock(&XP_CARRY).clear();
    lock(&MOUNTED).clear();
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::{
        Holder, ItemStack, LegacyRandom, dispatch_living_incoming_damage,
        dispatch_player_logged_out, dispatch_player_xp_change, dispatch_prepare_anvil,
    };
    use serde_json::{Value, json};
    use skills_core::effects::DIG_LOCK;
    use skills_core::progress::{PlayerState, SkillProgress};

    use super::*;
    use crate::server::data::tests::load;
    use crate::server::queue::{Job, take_queued};
    use crate::server::state::set_state;
    use crate::test_host::{TestHost, exclusive, mods};

    const PLAYER: u128 = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;
    const TARGET: &str = "01234567-89ab-cdef-0123-456789abcdef";
    const ENTITY: u32 = 77;
    const MOB: i32 = 500;
    const HORSE: u32 = 600;
    const HORSE_UUID: u128 = 0x0000_0000_0000_0000_0000_0000_0000_0258;
    const HORSE_TARGET: &str = "00000000-0000-0000-0000-000000000258";

    fn perk(id: &str, min_level: u32, effects: Value) -> (String, Value) {
        let json = json!({"format": 1, "id": id, "skill": "skills_and_perks:mining", "name": "n", "description": "d",
            "cost": 1, "min_level": min_level, "effects": effects});
        (id.to_string(), json)
    }

    /// Mining, a diamond pickaxe lock (Mining 16), and one perk per effect type.
    fn world(host: &mut TestHost) {
        let mining = json!({"format": 1, "id": "skills_and_perks:mining", "name": "n", "icon": "minecraft:stone"});
        let lock = json!({"format": 1, "entries": [{"target": {"item": {"id": "minecraft:diamond_pickaxe"}},
            "actions": ["break"], "requires": {"skills_and_perks:mining": 16}}]});
        let perks = [
            perk(
                "skills_and_perks:grip",
                1,
                json!([{"type": "attribute", "attribute": "minecraft:block_break_speed",
                "amount": 0.05, "operation": "add_multiplied_base"}]),
            ),
            perk(
                "skills_and_perks:hard_hat",
                8,
                json!([{"type": "attribute", "attribute": "minecraft:armor",
                "amount": 2.0, "operation": "add_value"}]),
            ),
            perk(
                "skills_and_perks:fury",
                1,
                json!([
                {"type": "damage", "direction": "dealt", "kind": "melee", "multiplier": 1.5},
                {"type": "damage", "direction": "taken", "kind": "fall", "multiplier": 0.5},
                {"type": "damage", "direction": "taken", "kind": "fire", "multiplier": 0.8}]),
            ),
            perk(
                "skills_and_perks:lore",
                1,
                json!([{"type": "xp_gain", "multiplier": 1.1},
                {"type": "anvil_cost", "multiplier": 0.5}]),
            ),
            perk(
                "skills_and_perks:veins",
                1,
                json!([
                {"type": "drop_bonus", "target": {"tag": "#minecraft:coal_ores"}, "chance": 1.0},
                {"type": "drop_bonus", "target": {"tag": "#minecraft:iron_ores"}, "chance": 0.5}]),
            ),
            perk(
                "skills_and_perks:rider",
                1,
                json!([{"type": "mount_attribute", "attribute": "minecraft:movement_speed",
                "amount": 0.1, "operation": "add_multiplied_base"}]),
            ),
            perk(
                "skills_and_perks:free_smith",
                1,
                json!([{"type": "anvil_cost", "multiplier": 0.0}]),
            ),
        ];
        let perks: Vec<(&str, Value)> = perks
            .iter()
            .map(|(id, json)| (id.as_str(), json.clone()))
            .collect();
        load(
            host,
            &[
                ("skills", vec![("skills_and_perks:mining", mining)]),
                ("perks", perks),
                ("locks", vec![("skills_and_perks:tiers", lock)]),
            ],
        );
        take_queued();
        host.block_tags = vec![
            ("minecraft:coal_ore".into(), "minecraft:coal_ores".into()),
            ("minecraft:iron_ore".into(), "minecraft:iron_ores".into()),
        ];
    }

    /// The player at a Mining level, owning the given perks, already synced once since login.
    fn owning(host: &mut TestHost, level: u32, perks: &[&str]) {
        let mut state = PlayerState::default();
        let progress = SkillProgress {
            level,
            perks: perks
                .iter()
                .map(|p| format!("skills_and_perks:{p}"))
                .collect(),
            ..SkillProgress::default()
        };
        state
            .skills
            .insert("skills_and_perks:mining".into(), progress);
        set_state(host, PLAYER, state).unwrap();
        remember_entity(PLAYER, ENTITY);
        sync_attributes(host, PLAYER);
        host.commands.clear();
    }

    fn add(attribute: &str, id: &str, amount: &str, operation: &str) -> String {
        format!("attribute {TARGET} {attribute} modifier add {id} {amount} {operation}")
    }

    fn remove(attribute: &str, id: &str) -> String {
        format!("attribute {TARGET} {attribute} modifier remove {id}")
    }

    const GRIP: &str = "skills_and_perks:skills_and_perks/grip/0";
    const HARD_HAT: &str = "skills_and_perks:skills_and_perks/hard_hat/0";
    const SPEED: &str = "minecraft:block_break_speed";

    #[test]
    fn first_sync_after_login_replaces_every_modifier() {
        // Given a player owning Grip and Hard Hat at Mining 9, whose record from last session lists a perk the pack
        // has since removed
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        owning(&mut host, 9, &["grip", "hard_hat"]);
        let stale = AttributeMod {
            id: "skills_and_perks:oldpack/gone/0".into(),
            attribute: "minecraft:luck".into(),
            amount: 1.0,
            operation: skills_core::defs::Operation::AddValue,
        };
        APPLIED
            .set(
                &mut host as &mut dyn Host,
                Holder::Player(PLAYER),
                vec![stale],
            )
            .unwrap();

        // When they log in again and their attributes sync
        mark_full(PLAYER);
        sync_attributes(&mut host, PLAYER);

        // Then every known modifier is removed first (whether or not it is there), then each wanted one added
        assert_eq!(
            host.commands,
            vec![
                remove("minecraft:luck", "skills_and_perks:oldpack/gone/0"),
                remove(SPEED, GRIP),
                remove("minecraft:armor", HARD_HAT),
                add(SPEED, GRIP, "0.05", "add_multiplied_base"),
                add("minecraft:armor", HARD_HAT, "2", "add_value"),
            ]
        );
    }

    #[test]
    fn attribute_sync_runs_the_diff() {
        // Given a player synced with Grip only
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        owning(&mut host, 9, &["grip"]);

        // When they get Hard Hat and their attributes sync, then sync again with nothing changed
        owning_without_sync(&mut host, 9, &["grip", "hard_hat"]);
        sync_attributes(&mut host, PLAYER);
        let first = std::mem::take(&mut host.commands);
        sync_attributes(&mut host, PLAYER);

        // Then only the new modifier is added, and an unchanged sync runs nothing
        assert_eq!(
            first,
            vec![add("minecraft:armor", HARD_HAT, "2", "add_value")]
        );
        assert!(host.commands.is_empty());
    }

    fn owning_without_sync(host: &mut TestHost, level: u32, perks: &[&str]) {
        let mut state = PlayerState::default();
        let progress = SkillProgress {
            level,
            perks: perks
                .iter()
                .map(|p| format!("skills_and_perks:{p}"))
                .collect(),
            ..SkillProgress::default()
        };
        state
            .skills
            .insert("skills_and_perks:mining".into(), progress);
        set_state(host, PLAYER, state).unwrap();
    }

    #[test]
    fn inactive_perk_modifier_is_removed() {
        // Given a player at Mining 9 synced with Hard Hat (needs Mining 8)
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        owning(&mut host, 9, &["hard_hat"]);

        // When a death penalty leaves them at Mining 7 and their attributes sync
        owning_without_sync(&mut host, 7, &["hard_hat"]);
        sync_attributes(&mut host, PLAYER);

        // Then the modifier comes off
        assert_eq!(host.commands, vec![remove("minecraft:armor", HARD_HAT)]);
    }

    #[test]
    fn dig_lock_added_for_locked_main_hand_and_removed_after() {
        // Given a level-1 player
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        owning(&mut host, 1, &[]);

        // When they take a diamond pickaxe (Mining 16 to break) in hand, then a stick, syncing after each, and an
        // exempt (creative) player takes the pickaxe
        note_main_hand(PLAYER, Some("minecraft:diamond_pickaxe"), false);
        sync_attributes(&mut host, PLAYER);
        let with_pickaxe = std::mem::take(&mut host.commands);
        note_main_hand(PLAYER, Some("minecraft:stick"), false);
        sync_attributes(&mut host, PLAYER);
        let with_stick = std::mem::take(&mut host.commands);
        note_main_hand(PLAYER, Some("minecraft:diamond_pickaxe"), true);
        sync_attributes(&mut host, PLAYER);

        // Then the dig lock comes with the pickaxe and goes with it, and an exempt player never gets it
        assert_eq!(
            with_pickaxe,
            vec![add(SPEED, DIG_LOCK, "-1", "add_multiplied_total")]
        );
        assert_eq!(with_stick, vec![remove(SPEED, DIG_LOCK)]);
        assert!(host.commands.is_empty());
    }

    #[test]
    fn dealt_and_taken_multipliers_apply_by_kind() {
        // Given a synced player with Fury (melee dealt x1.5, fall taken x0.5, fire taken x0.8)
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        owning(&mut host, 1, &["fury"]);
        let player = ENTITY as i32;
        let hit = |victim: i32, damage_type: &str, causing: i32| {
            let (mut amount, mut canceled) = (10.0_f32, false);
            dispatch_living_incoming_damage(
                &mods(),
                victim,
                "minecraft:zombie",
                damage_type,
                causing,
                causing,
                &mut amount,
                &mut canceled,
            );
            amount
        };

        // When they hit a mob, fall, catch a fireball (fire and projectile), and a mob hits another mob
        let amounts = [
            hit(MOB, "minecraft:player_attack", player),
            hit(player, "minecraft:fall", -1),
            hit(player, "minecraft:fireball", MOB),
            hit(player, "minecraft:mob_attack", MOB),
            hit(MOB + 1, "minecraft:mob_attack", MOB),
        ];

        // Then each kind's rule applies, and hits that are not theirs are unchanged
        assert_eq!(amounts, [15.0, 5.0, 8.0, 10.0, 10.0]);
    }

    #[test]
    fn xp_gain_carries_fractions() {
        // Given a synced player with Lore (XP x1.1)
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        owning(&mut host, 1, &["lore"]);
        let mut gain = |player: u128, points: i32| {
            let (mut amount, mut cancel) = (points, false);
            dispatch_player_xp_change(&mut host, &mods(), player, "Ann", &mut amount, &mut cancel);
            amount
        };

        // When they pick up ten 1-point orbs, lose 3 points, and someone without the perk picks one up
        let orbs: i32 = (0..10).map(|_| gain(PLAYER, 1)).sum();
        let loss = gain(PLAYER, -3);
        let other = gain(0x99, 1);

        // Then the tenth orb carries the fractions to an eleventh point, a loss is untouched, and others get 1
        assert_eq!((orbs, loss, other), (11, -3, 1));
    }

    #[test]
    fn anvil_cost_never_below_1() {
        // Given a synced player with Lore (anvil x0.5)
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        owning(&mut host, 1, &["lore"]);
        let mut price = |cost: i32| {
            let (mut cost, mut result) = (cost, None);
            dispatch_prepare_anvil(
                &mut host,
                &mods(),
                PLAYER,
                "Ann",
                [0, 64, 0],
                &mut cost,
                &mut result,
            );
            (cost, result.is_none())
        };

        // When anvil work costing 7, 1 and 0 levels is prepared, then 5 levels for a player whose perk makes it free
        let costs = [price(7), price(1), price(0)];
        owning(&mut host, 1, &["free_smith"]);
        let (mut cost, mut result) = (5, None);
        dispatch_prepare_anvil(
            &mut host,
            &mods(),
            PLAYER,
            "Ann",
            [0, 64, 0],
            &mut cost,
            &mut result,
        );

        // Then the cost halves rounding up, never below 1 (even at x0), a free result stays free, and the result is
        // untouched
        assert_eq!(costs, [(4, true), (1, true), (0, true)]);
        assert_eq!(cost, 1);
    }

    #[test]
    fn login_resyncs_in_full() {
        // Given a player owning Grip whose modifiers may or may not still be on them
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        owning_without_sync(&mut host, 1, &["grip"]);
        crate::server::effects::APPLIED
            .set(
                &mut host as &mut dyn Host,
                Holder::Player(PLAYER),
                Vec::new(),
            )
            .unwrap();

        // When they log in and their attributes sync
        horizon_client::mod_api::dispatch_player_logged_in(&mut host, &mods(), PLAYER, "Ann");
        sync_attributes(&mut host, PLAYER);

        // Then Grip is removed before it is added, so an old copy cannot make the add fail
        assert_eq!(
            host.commands,
            vec![
                remove(SPEED, GRIP),
                add(SPEED, GRIP, "0.05", "add_multiplied_base")
            ]
        );
    }

    #[test]
    fn login_check_notes_the_main_hand() {
        // Given a level-1 player who logs in holding a diamond pickaxe (Mining 16 to break)
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        owning(&mut host, 1, &[]);
        let pickaxe = ItemStack {
            item: "minecraft:diamond_pickaxe".into(),
            count: 1,
            damage: 0,
            extra: None,
        };
        host.equipment
            .push((horizon_client::mod_api::EquipmentSlot::MainHand, pickaxe));

        // When the login's equipment check runs, then their attributes sync
        let actor = crate::server::enforce::Actor {
            player: PLAYER,
            entity: ENTITY,
            exempt: false,
        };
        crate::server::enforce::check_equipment(&mut host, &actor);
        sync_attributes(&mut host, PLAYER);

        // Then the dig lock is on before they ever switch items
        assert_eq!(
            host.commands,
            vec![add(SPEED, DIG_LOCK, "-1", "add_multiplied_total")]
        );
    }

    #[test]
    fn drop_bonus_duplicates_with_its_chance() {
        // Given a synced player with Veins (coal ores always, iron ores half the time) and a seeded random source
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        owning(&mut host, 1, &["veins"]);
        host.random = Some(LegacyRandom::new(42));
        let coal = ItemStack {
            item: "minecraft:coal".into(),
            count: 1,
            damage: 0,
            extra: None,
        };
        let mut breaks = |table: &str, player: u128| {
            let mut drops = vec![coal.clone()];
            bonus_drops(&mut host, player, "minecraft:overworld", table, &mut drops);
            drops.len()
        };

        // When they break coal ore, stone, coal ore without the perk (another player), and iron ore 400 times
        let coal_ore = breaks("minecraft:blocks/coal_ore", PLAYER);
        let stone = breaks("minecraft:blocks/stone", PLAYER);
        let other = breaks("minecraft:blocks/coal_ore", 0x99);
        let iron_bonus = (0..400)
            .filter(|_| breaks("minecraft:blocks/iron_ore", PLAYER) == 2)
            .count();

        // Then coal ore always drops one more stack, stone and others never do, and iron about half the time
        assert_eq!((coal_ore, stone, other), (2, 1, 1));
        assert!((150..250).contains(&iron_bonus), "{iron_bonus}");
    }

    #[test]
    fn mount_modifiers_follow_the_rider() {
        // Given a synced player with Rider (mount speed +10%) and a horse
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        owning(&mut host, 1, &["rider"]);
        host.entity_uuids.insert(HORSE, HORSE_UUID);
        let modifier = "skills_and_perks:skills_and_perks/rider/0";

        // When they get on the horse, then off it, on the ticks after the mount events
        run_job(
            &mut host,
            Job::Mount {
                player: PLAYER,
                vehicle: HORSE,
                mounting: true,
            },
        );
        let on = std::mem::take(&mut host.commands);
        run_job(
            &mut host,
            Job::Mount {
                player: PLAYER,
                vehicle: HORSE,
                mounting: false,
            },
        );

        // Then the horse gets the modifier while ridden and loses it after
        assert_eq!(
            on,
            vec![format!(
                "attribute {HORSE_TARGET} minecraft:movement_speed modifier add {modifier} 0.1 add_multiplied_base"
            )]
        );
        assert_eq!(
            host.commands,
            vec![format!(
                "attribute {HORSE_TARGET} minecraft:movement_speed modifier remove {modifier}"
            )]
        );
    }

    #[test]
    fn logout_forgets_the_player() {
        // Given a synced player with Fury
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        owning(&mut host, 1, &["fury"]);

        // When they log out and a hit names their old entity id
        dispatch_player_logged_out(&mut host, &mods(), PLAYER, "Ann");
        let (mut amount, mut canceled) = (10.0_f32, false);
        dispatch_living_incoming_damage(
            &mods(),
            MOB,
            "minecraft:zombie",
            "minecraft:player_attack",
            ENTITY as i32,
            ENTITY as i32,
            &mut amount,
            &mut canceled,
        );

        // Then their perks no longer apply
        assert_eq!(amount, 10.0);
    }

    #[test]
    fn damage_kinds_follow_vanilla_tags() {
        // Given damage types from each of vanilla's tags, one in two, and one in none
        // When they are classified
        let kinds = [
            "minecraft:spear",
            "minecraft:arrow",
            "minecraft:stalagmite",
            "minecraft:lava",
            "minecraft:player_explosion",
            "minecraft:fireball",
            "minecraft:magic",
        ]
        .map(damage_kinds);

        // Then each lands in its kinds
        use skills_core::defs::DamageKind::*;
        assert_eq!(
            kinds,
            [
                vec![Melee],
                vec![Projectile],
                vec![Fall],
                vec![Fire],
                vec![Explosion],
                vec![Projectile, Fire],
                vec![]
            ]
        );
    }
}
