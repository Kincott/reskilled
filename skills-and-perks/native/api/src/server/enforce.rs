//! Lock enforcement: actions a player's skills do not allow are cancelled with a "Needs Mining 16" line, and gear
//! they may not wear comes off.
//!
//! The hooks only find who acts (an [`Actor`]); the functions here decide, so tests reach them without a
//! `NativeServerPlayer`, which a test cannot build.

use std::collections::BTreeMap;
use std::sync::Mutex;

use horizon_client::mod_api::{
    Compound, EntityRef, EquipmentSlot, Flow, Host, ItemStack, NativeServerPlayer, Tag,
};
use skills_core::defs::{Action, Settings};
use skills_core::locks::{TargetKind, Unmet, check};
use skills_core::progress::level;
use skills_core::registry::Defs;
use skills_core::wire::ToClient;

use super::data::{current, requirements};
use super::lock;
use super::queue::{self, Job};
use super::state::state_of;
use super::text::{NEEDS, number, say, skill_name};
use crate::MSG_CHANNEL;

/// `NativeServerPlayer::game_mode` values that may ignore locks.
const CREATIVE: i32 = 1;
const SPECTATOR: i32 = 3;
const PLAYER_KIND: &str = "minecraft:player";
const ITEM_ENTITY: &str = "minecraft:item";
/// The ticks before a dropped item can be picked up, as when a player drops one.
pub const PICKUP_DELAY_TICKS: i16 = 40;
/// Slots worn rather than held: the `Equip` lock applies to them.
const WORN: [EquipmentSlot; 5] = [
    EquipmentSlot::Head,
    EquipmentSlot::Chest,
    EquipmentSlot::Legs,
    EquipmentSlot::Feet,
    EquipmentSlot::OffHand,
];

/// The player an enforcement hook is about.
#[derive(Debug, Clone, Copy)]
pub struct Actor {
    pub player: u128,
    /// Their entity id this session, for the equipment helpers.
    pub entity: u32,
    /// Creative or spectator with `creative_ignores_locks` on.
    pub exempt: bool,
}

impl Actor {
    pub fn of(player: &NativeServerPlayer, settings: &Settings) -> Self {
        Actor {
            player: player.uuid,
            entity: u32::try_from(player.entity_id).unwrap_or_default(),
            exempt: exempt(player.game_mode, settings),
        }
    }
}

/// Whether a player in this game mode ignores locks.
pub fn exempt(game_mode: i32, settings: &Settings) -> bool {
    settings.creative_ignores_locks && matches!(game_mode, CREATIVE | SPECTATOR)
}

fn settings() -> Settings {
    current()
        .map(|data| data.defs.settings.clone())
        .unwrap_or_default()
}

/// The player whose action the hook hears.
pub fn acting(host: &dyn Host) -> Option<Actor> {
    host.acting_player()
        .map(|player| Actor::of(&player, &settings()))
}

/// An online player, by uuid or by entity id.
fn listed(host: &dyn Host, matches: impl Fn(&NativeServerPlayer) -> bool) -> Option<Actor> {
    let settings = settings();
    host.server_players()
        .iter()
        .find(|p| matches(p))
        .map(|p| Actor::of(p, &settings))
}

/// The rider, when it is a player.
pub fn rider(host: &dyn Host, entity: u32, kind: &str) -> Option<Actor> {
    let entity = i32::try_from(entity).ok()?;
    (kind == PLAYER_KIND)
        .then(|| listed(host, |p| p.entity_id == entity))
        .flatten()
}

/// The first check the player fails cancels the action, with the "Needs" line; exempt players pass all.
fn guard(host: &mut dyn Host, actor: &Actor, checks: &[(TargetKind, &str, Action)]) -> Flow {
    if actor.exempt {
        return Flow::Continue;
    }
    match first_unmet(host, actor.player, checks) {
        Some((defs, unmet)) => {
            refuse(host, actor.player, &defs, &unmet);
            Flow::Cancel
        }
        None => Flow::Continue,
    }
}

/// The first requirement the player does not meet, with the definitions it was judged by.
fn first_unmet(
    host: &mut dyn Host,
    player: u128,
    checks: &[(TargetKind, &str, Action)],
) -> Option<(std::sync::Arc<Defs>, Unmet)> {
    let data = current()?;
    let state = match state_of(host, player) {
        Ok(state) => state,
        Err(error) => {
            log::warn!(
                "Skills & Perks lets {player:032x} through: their state is unreadable ({error:?})"
            );
            return None;
        }
    };
    let levels = |skill: &str| level(&state, &data.defs, skill);
    checks.iter().find_map(|(kind, id, action)| {
        let reqs = requirements(host, *kind, id);
        check(&reqs, *action, &levels)
            .err()
            .map(|unmet| (data.defs.clone(), unmet))
    })
}

/// Tick each player last saw a "Needs" line.
static LAST_REFUSAL: Mutex<BTreeMap<u128, u64>> = Mutex::new(BTreeMap::new());

/// Tells the player what they need, at most once per `lock_message_cooldown_ticks`: the action-bar line, and a
/// message their client answers with a sound.
fn refuse(host: &mut dyn Host, player: u128, defs: &Defs, unmet: &Unmet) {
    let now = queue::now();
    let cooldown = u64::from(defs.settings.lock_message_cooldown_ticks);
    {
        let mut last = lock(&LAST_REFUSAL);
        if last.get(&player).is_some_and(|said| now < said + cooldown) {
            return;
        }
        last.insert(player, now);
    }
    host.action_bar(
        player,
        say(
            &NEEDS,
            &[skill_name(defs, &unmet.skill), number(unmet.level)],
        ),
    );
    let refused = ToClient::Refused {
        skill: unmet.skill.clone(),
        level: unmet.level,
    };
    queue::send(
        player,
        MSG_CHANNEL,
        serde_json::to_vec(&refused).expect("a refusal always encodes"),
    );
}

/// Using an item in the air.
pub fn on_use_item(host: &mut dyn Host, actor: &Actor, item: &str) -> Flow {
    guard(host, actor, &[(TargetKind::Item, item, Action::Use)])
}

/// Using an item on a block: the item's use, and placing it when it is a block that a lock covers.
pub fn on_use_item_on(host: &mut dyn Host, actor: &Actor, item: &str) -> Flow {
    guard(
        host,
        actor,
        &[
            (TargetKind::Item, item, Action::Use),
            (TargetKind::Block, item, Action::Place),
        ],
    )
}

/// A blow with what the main hand holds.
pub fn on_attack(host: &mut dyn Host, actor: &Actor, main_hand: Option<&str>) -> Flow {
    main_hand.map_or(Flow::Continue, |item| {
        guard(host, actor, &[(TargetKind::Item, item, Action::Attack)])
    })
}

/// Breaking a block with what the main hand holds.
pub fn on_destroy(host: &mut dyn Host, actor: &Actor, main_hand: Option<&str>) -> Flow {
    main_hand.map_or(Flow::Continue, |item| {
        guard(host, actor, &[(TargetKind::Item, item, Action::Break)])
    })
}

/// Getting on a vehicle; getting off is never refused.
pub fn on_mount(host: &mut dyn Host, actor: &Actor, vehicle: &str, mounting: bool) -> Flow {
    if !mounting {
        return Flow::Continue;
    }
    guard(host, actor, &[(TargetKind::Entity, vehicle, Action::Mount)])
}

/// Right-clicking an entity: the entity's own lock, then the use of what is in hand (wolf or horse armor).
pub fn on_interact(host: &mut dyn Host, actor: &Actor, target: &str, held: Option<&str>) -> Flow {
    let mut checks = vec![(TargetKind::Entity, target, Action::Interact)];
    checks.extend(held.map(|item| (TargetKind::Item, item, Action::Use)));
    guard(host, actor, &checks)
}

/// One of the game's menus about to open; `block` is the block it belongs to, none for a villager or a mount.
pub fn on_container(host: &mut dyn Host, actor: &Actor, block: Option<&str>) -> Flow {
    block.map_or(Flow::Continue, |block| {
        guard(host, actor, &[(TargetKind::Block, block, Action::Open)])
    })
}

/// An equipment slot took another item: the main hand resyncs attributes (the dig lock follows it); a worn item the
/// player may not wear comes off on the tick.
pub fn on_equip(host: &mut dyn Host, actor: &Actor, slot: &str, item: Option<&str>) {
    let Some(slot) = slot_named(slot) else { return };
    if slot == EquipmentSlot::MainHand {
        queue::queue(Job::SyncAttributes(actor.player));
        return;
    }
    let Some(item) = item else { return };
    if WORN.contains(&slot)
        && guard(host, actor, &[(TargetKind::Item, item, Action::Equip)]) == Flow::Cancel
    {
        queue::queue(Job::Unequip {
            player: actor.player,
            entity: actor.entity,
            slot,
        });
    }
}

/// The slot `on_equip` names (`chest`, `offhand`...).
fn slot_named(name: &str) -> Option<EquipmentSlot> {
    Some(match name {
        "mainhand" => EquipmentSlot::MainHand,
        "offhand" => EquipmentSlot::OffHand,
        "feet" => EquipmentSlot::Feet,
        "legs" => EquipmentSlot::Legs,
        "chest" => EquipmentSlot::Chest,
        "head" => EquipmentSlot::Head,
        _ => return None,
    })
}

/// The tick's part of enforcement: an equipment check after login or reload, and taking a worn item off.
pub fn run_job(host: &mut dyn Host, job: Job) {
    match job {
        Job::CheckEquipment(player) => match listed(host, |p| p.uuid == player) {
            Some(actor) => check_equipment(host, &actor),
            None => log::debug!(
                "Skills & Perks skipped the equipment check of {player:032x}, who is not online"
            ),
        },
        Job::Unequip {
            player,
            entity,
            slot,
        } => unequip(host, player, entity, slot),
        other => log::warn!("Skills & Perks enforcement got a job it does not run: {other:?}"),
    }
}

/// Takes off every worn item the player may not wear, telling them once.
pub fn check_equipment(host: &mut dyn Host, actor: &Actor) {
    if actor.exempt {
        return;
    }
    for slot in WORN {
        unequip(host, actor.player, actor.entity, slot);
    }
}

/// Takes the item in the slot off when the player still may not wear it: into their inventory, else dropped at
/// their feet, else (the world refuses the drop) left on, so it is never lost.
fn unequip(host: &mut dyn Host, player: u128, entity: u32, slot: EquipmentSlot) {
    let Some(entity) = host.entity(entity) else {
        return;
    };
    let Some(living) = host.living(entity) else {
        return;
    };
    let Some(worn) = host.living_equipment(living, slot) else {
        return;
    };
    let Some((defs, unmet)) = first_unmet(
        host,
        player,
        &[(TargetKind::Item, &worn.item, Action::Equip)],
    ) else {
        return;
    };
    refuse(host, player, &defs, &unmet);
    if !host.living_set_equipment(living, slot, None) {
        log::warn!(
            "Skills & Perks could not take {} off {player:032x}",
            worn.item
        );
        return;
    }
    let Some(rest) = host.give_stack(player, worn) else {
        return;
    };
    if let Err(rest) = drop_at(host, entity, rest) {
        log::warn!(
            "Skills & Perks could not drop {} at {player:032x}, so it stays on",
            rest.item
        );
        host.living_set_equipment(living, slot, Some(rest));
    }
}

/// Drops the stack at the entity's feet, as it is (enchantments and all); gives it back when it cannot.
fn drop_at(host: &mut dyn Host, entity: EntityRef, stack: ItemStack) -> Result<(), ItemStack> {
    let (Some(level), Some(pos)) = (host.entity_level(entity), host.entity_position(entity)) else {
        return Err(stack);
    };
    let dimension = level.dimension();
    // An item entity needs an item to exist; `set_item_entity` then puts the real stack in.
    let placeholder: Compound = [
        ("id".into(), Tag::String(stack.item.as_ref().into())),
        ("count".into(), Tag::Int(1)),
    ]
    .into();
    let nbt: Compound = [
        ("Item".into(), Tag::Compound(placeholder)),
        ("PickupDelay".into(), Tag::Short(PICKUP_DELAY_TICKS)),
    ]
    .into();
    match host.summon(dimension, ITEM_ENTITY, pos, nbt) {
        Some(id) if host.set_item_entity(dimension, id, stack.clone()) => Ok(()),
        _ => Err(stack),
    }
}

#[cfg(test)]
pub(crate) fn reset() {
    lock(&LAST_REFUSAL).clear();
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::{EquipmentSlot, Flow, ItemStack, Tag};
    use serde_json::{Value, json};
    use skills_core::defs::Settings;
    use skills_core::progress::{PlayerState, SkillProgress};
    use skills_core::wire::ToClient;

    use super::*;
    use crate::MSG_CHANNEL;
    use crate::server::data::tests::load;
    use crate::server::queue::{Job, run, take_queued};
    use crate::server::state::set_state;
    use crate::server::test_host::{PLAYER_POSITION, SUMMONED_ID, TestHost, exclusive};

    const PLAYER: u128 = 0xfeed;
    const ENTITY: u32 = 42;
    const SKILLS: [&str; 5] = ["mining", "melee", "defense", "taming", "arcane"];

    fn stack(item: &str) -> ItemStack {
        ItemStack {
            item: item.into(),
            count: 1,
            damage: 0,
            extra: None,
        }
    }

    fn entry(target: Value, actions: &[&str], skill: &str, level: u32) -> Value {
        json!({"target": target, "actions": actions, "requires": {format!("skills_and_perks:{skill}"): level}})
    }

    /// Five skills and one lock per action kind.
    fn with_locks(host: &mut TestHost) {
        let skills: Vec<(String, Value)> = SKILLS
            .iter()
            .map(|s| {
                let id = format!("skills_and_perks:{s}");
                let json = json!({"format": 1, "id": id, "name": format!("skills_and_perks.skill.{s}"), "icon": "minecraft:stone"});
                (id, json)
            })
            .collect();
        let item = |id: &str| json!({"item": {"id": id}});
        let locks = json!({"format": 1, "entries": [
            entry(item("minecraft:diamond_pickaxe"), &["break"], "mining", 16),
            entry(item("minecraft:diamond_sword"), &["attack"], "melee", 8),
            entry(item("minecraft:shield"), &["use"], "defense", 4),
            entry(item("minecraft:wolf_armor"), &["use"], "taming", 6),
            entry(item("minecraft:diamond_chestplate"), &["equip"], "defense", 16),
            entry(json!({"block": {"id": "minecraft:tnt"}}), &["place"], "mining", 10),
            entry(json!({"block": {"id": "minecraft:anvil"}}), &["open"], "arcane", 6),
            entry(json!({"entity": {"id": "minecraft:horse"}}), &["mount"], "taming", 4),
            entry(json!({"entity": {"id": "minecraft:villager"}}), &["interact"], "arcane", 2),
        ]});
        let skills: Vec<(&str, Value)> = skills
            .iter()
            .map(|(id, json)| (id.as_str(), json.clone()))
            .collect();
        load(
            host,
            &[
                ("skills", skills),
                ("locks", vec![("skills_and_perks:test", locks)]),
            ],
        );
        take_queued();
    }

    /// The player at level 1 in every skill, or at 30 when `skilled`.
    fn player(host: &mut TestHost, skilled: bool) -> Actor {
        let mut state = PlayerState::default();
        if skilled {
            for s in SKILLS {
                state.skills.insert(
                    format!("skills_and_perks:{s}"),
                    SkillProgress {
                        level: 30,
                        ..SkillProgress::default()
                    },
                );
            }
        }
        set_state(host, PLAYER, state).unwrap();
        Actor {
            player: PLAYER,
            entity: ENTITY,
            exempt: false,
        }
    }

    /// A world with the locks, and the player at level 1 (`skilled` false) or 30.
    fn setup(skilled: bool) -> (TestHost, Actor) {
        let mut host = TestHost::default();
        with_locks(&mut host);
        let actor = player(&mut host, skilled);
        (host, actor)
    }

    #[test]
    fn locked_use_is_cancelled() {
        // Given a level-1 player holding a shield (Defense 4 to use)
        let _serial = exclusive();
        let (mut host, actor) = setup(false);

        // When they use it
        let flow = on_use_item(&mut host, &actor, "minecraft:shield");

        // Then it is cancelled and they read what they need
        assert_eq!(flow, Flow::Cancel);
        assert_eq!(host.action_bars_of(PLAYER), vec!["Needs Defense 4"]);
    }

    #[test]
    fn unlocked_use_passes() {
        // Given a skilled player holding a shield, and a level-1 player holding a stick
        let _serial = exclusive();
        let (mut host, skilled) = setup(true);

        // When they use them
        let shield = on_use_item(&mut host, &skilled, "minecraft:shield");
        let unskilled = player(&mut host, false);
        let stick = on_use_item(&mut host, &unskilled, "minecraft:stick");

        // Then both go ahead with no message
        assert_eq!((shield, stick), (Flow::Continue, Flow::Continue));
        assert!(host.action_bars.is_empty());
    }

    #[test]
    fn locked_place_is_cancelled() {
        // Given a level-1 player holding TNT (Mining 10 to place)
        let _serial = exclusive();
        let (mut host, actor) = setup(false);

        // When they click a block with it
        let flow = on_use_item_on(&mut host, &actor, "minecraft:tnt");

        // Then the placing is cancelled
        assert_eq!(flow, Flow::Cancel);
        assert_eq!(host.action_bars_of(PLAYER), vec!["Needs Mining 10"]);
    }

    #[test]
    fn unlocked_place_passes() {
        // Given a skilled player holding TNT, and a shield used on a block
        let _serial = exclusive();
        let (mut host, actor) = setup(true);

        // When they click a block with each
        let tnt = on_use_item_on(&mut host, &actor, "minecraft:tnt");
        let shield = on_use_item_on(&mut host, &actor, "minecraft:shield");

        // Then both go ahead
        assert_eq!((tnt, shield), (Flow::Continue, Flow::Continue));
    }

    #[test]
    fn locked_use_on_a_block_is_cancelled() {
        // Given a level-1 player holding a shield
        let _serial = exclusive();
        let (mut host, actor) = setup(false);

        // When they use it on a block
        let flow = on_use_item_on(&mut host, &actor, "minecraft:shield");

        // Then the item's own use lock applies there too
        assert_eq!(flow, Flow::Cancel);
    }

    #[test]
    fn locked_attack_is_cancelled() {
        // Given a level-1 player holding a diamond sword (Melee 8 to attack)
        let _serial = exclusive();
        let (mut host, actor) = setup(false);

        // When they hit something
        let flow = on_attack(&mut host, &actor, Some("minecraft:diamond_sword"));

        // Then the blow is cancelled
        assert_eq!(flow, Flow::Cancel);
        assert_eq!(host.action_bars_of(PLAYER), vec!["Needs Melee 8"]);
    }

    #[test]
    fn unlocked_attack_passes() {
        // Given a skilled player with the sword, and a level-1 player with an empty hand
        let _serial = exclusive();
        let (mut host, skilled) = setup(true);

        // When they hit something
        let sword = on_attack(&mut host, &skilled, Some("minecraft:diamond_sword"));
        let unskilled = player(&mut host, false);
        let fist = on_attack(&mut host, &unskilled, None);

        // Then both blows land
        assert_eq!((sword, fist), (Flow::Continue, Flow::Continue));
    }

    #[test]
    fn locked_break_is_cancelled() {
        // Given a level-1 player holding a diamond pickaxe (Mining 16 to break)
        let _serial = exclusive();
        let (mut host, actor) = setup(false);

        // When they break a block
        let flow = on_destroy(&mut host, &actor, Some("minecraft:diamond_pickaxe"));

        // Then the break is cancelled
        assert_eq!(flow, Flow::Cancel);
        assert_eq!(host.action_bars_of(PLAYER), vec!["Needs Mining 16"]);
    }

    #[test]
    fn unlocked_break_passes() {
        // Given a skilled player with the pickaxe
        let _serial = exclusive();
        let (mut host, actor) = setup(true);

        // When they break a block
        let flow = on_destroy(&mut host, &actor, Some("minecraft:diamond_pickaxe"));

        // Then it breaks
        assert_eq!(flow, Flow::Continue);
    }

    #[test]
    fn locked_mount_is_cancelled() {
        // Given a level-1 player and a horse (Taming 4 to ride)
        let _serial = exclusive();
        let (mut host, actor) = setup(false);

        // When they get on it, then (riding anyway) get off it
        let mount = on_mount(&mut host, &actor, "minecraft:horse", true);
        let dismount = on_mount(&mut host, &actor, "minecraft:horse", false);

        // Then getting on is cancelled, and getting off never is
        assert_eq!((mount, dismount), (Flow::Cancel, Flow::Continue));
        assert_eq!(host.action_bars_of(PLAYER), vec!["Needs Taming 4"]);
    }

    #[test]
    fn unlocked_mount_passes() {
        // Given a skilled player and a horse, and a level-1 player and a boat
        let _serial = exclusive();
        let (mut host, skilled) = setup(true);

        // When they get on
        let horse = on_mount(&mut host, &skilled, "minecraft:horse", true);
        let unskilled = player(&mut host, false);
        let boat = on_mount(&mut host, &unskilled, "minecraft:oak_boat", true);

        // Then both ride
        assert_eq!((horse, boat), (Flow::Continue, Flow::Continue));
    }

    #[test]
    fn locked_interact_is_cancelled() {
        // Given a level-1 player, a villager (Arcane 2 to use), and wolf armor in hand (Taming 6 to use)
        let _serial = exclusive();
        let (mut host, actor) = setup(false);

        // When they right-click the villager empty-handed, and a wolf holding the armor
        let villager = on_interact(&mut host, &actor, "minecraft:villager", None);
        for _ in 0..20 {
            run(&mut host, vec![]);
        }
        host.action_bars.clear();
        let wolf = on_interact(
            &mut host,
            &actor,
            "minecraft:wolf",
            Some("minecraft:wolf_armor"),
        );

        // Then both are cancelled: the target's lock, and the held item's use lock
        assert_eq!((villager, wolf), (Flow::Cancel, Flow::Cancel));
        assert_eq!(host.action_bars_of(PLAYER), vec!["Needs Taming 6"]);
    }

    #[test]
    fn unlocked_interact_passes() {
        // Given a skilled player
        let _serial = exclusive();
        let (mut host, actor) = setup(true);

        // When they right-click the villager, and a wolf with the armor
        let villager = on_interact(&mut host, &actor, "minecraft:villager", None);
        let wolf = on_interact(
            &mut host,
            &actor,
            "minecraft:wolf",
            Some("minecraft:wolf_armor"),
        );

        // Then both go ahead
        assert_eq!((villager, wolf), (Flow::Continue, Flow::Continue));
    }

    #[test]
    fn locked_open_is_cancelled() {
        // Given a level-1 player at an anvil (Arcane 6 to open)
        let _serial = exclusive();
        let (mut host, actor) = setup(false);

        // When its menu is about to open
        let flow = on_container(&mut host, &actor, Some("minecraft:anvil"));

        // Then it stays shut
        assert_eq!(flow, Flow::Cancel);
        assert_eq!(host.action_bars_of(PLAYER), vec!["Needs Arcane 6"]);
    }

    #[test]
    fn unlocked_open_passes() {
        // Given a skilled player at an anvil, and a level-1 player at a chest and at a villager's menu (no block)
        let _serial = exclusive();
        let (mut host, skilled) = setup(true);

        // When the menus open
        let anvil = on_container(&mut host, &skilled, Some("minecraft:anvil"));
        let unskilled = player(&mut host, false);
        let chest = on_container(&mut host, &unskilled, Some("minecraft:chest"));
        let trade = on_container(&mut host, &unskilled, None);

        // Then all of them open
        assert_eq!(
            (anvil, chest, trade),
            (Flow::Continue, Flow::Continue, Flow::Continue)
        );
    }

    #[test]
    fn creative_bypasses_locks() {
        // Given a level-1 player who is exempt (creative or spectator, with the setting on)
        let _serial = exclusive();
        let (mut host, mut actor) = setup(false);
        actor.exempt = true;

        // When they do every locked thing
        let flows = [
            on_use_item(&mut host, &actor, "minecraft:shield"),
            on_attack(&mut host, &actor, Some("minecraft:diamond_sword")),
            on_destroy(&mut host, &actor, Some("minecraft:diamond_pickaxe")),
            on_mount(&mut host, &actor, "minecraft:horse", true),
            on_container(&mut host, &actor, Some("minecraft:anvil")),
        ];
        on_equip(
            &mut host,
            &actor,
            "chest",
            Some("minecraft:diamond_chestplate"),
        );

        // Then nothing is cancelled, nothing is queued and nothing is said
        assert!(flows.iter().all(|f| *f == Flow::Continue));
        assert!(take_queued().is_empty());
        assert!(host.action_bars.is_empty());
    }

    #[test]
    fn exempt_follows_game_mode_and_setting() {
        // Given the four game modes, with the setting on and off
        let on = Settings::default();
        let off = Settings {
            creative_ignores_locks: false,
            ..Settings::default()
        };

        // When each is asked whether it ignores locks
        let with_on = [0, 1, 2, 3].map(|mode| exempt(mode, &on));
        let with_off = [0, 1, 2, 3].map(|mode| exempt(mode, &off));

        // Then only creative (1) and spectator (3) do, and only while the setting is on
        assert_eq!(with_on, [false, true, false, true]);
        assert_eq!(with_off, [false; 4]);
    }

    #[test]
    fn message_is_rate_limited() {
        // Given a level-1 player
        let _serial = exclusive();
        let (mut host, actor) = setup(false);

        // When they swing a locked sword three times in one tick, then again 19 and 20 ticks later
        let flows = [0; 3].map(|_| on_attack(&mut host, &actor, Some("minecraft:diamond_sword")));
        run(&mut host, vec![]);
        let refused_first = host.sent_to(PLAYER, MSG_CHANNEL).len();
        for _ in 0..18 {
            run(&mut host, vec![]);
        }
        on_attack(&mut host, &actor, Some("minecraft:diamond_sword"));
        let after_19 = host.action_bars.len();
        run(&mut host, vec![]);
        on_attack(&mut host, &actor, Some("minecraft:diamond_sword"));
        run(&mut host, vec![]);

        // Then every swing is cancelled, but the line and the sound come once per 20 ticks
        assert!(flows.iter().all(|f| *f == Flow::Cancel));
        assert_eq!((refused_first, after_19), (1, 1));
        assert_eq!(host.action_bars.len(), 2);
        let sounds: Vec<ToClient> = host
            .sent_to(PLAYER, MSG_CHANNEL)
            .iter()
            .map(|m| serde_json::from_slice(m).unwrap())
            .collect();
        let refused = ToClient::Refused {
            skill: "skills_and_perks:melee".into(),
            level: 8,
        };
        assert_eq!(sounds, vec![refused.clone(), refused]);
    }

    #[test]
    fn locked_armor_is_unequipped() {
        // Given a level-1 player who puts on a diamond chestplate (Defense 16 to wear)
        let _serial = exclusive();
        let (mut host, actor) = setup(false);
        host.equipment
            .push((EquipmentSlot::Chest, stack("minecraft:diamond_chestplate")));

        // When the equip is heard, then the tick runs
        on_equip(
            &mut host,
            &actor,
            "chest",
            Some("minecraft:diamond_chestplate"),
        );
        let queued = take_queued();
        run(&mut host, queued.clone());

        // Then it comes off into their inventory, and they read why
        assert_eq!(
            queued,
            vec![Job::Unequip {
                player: PLAYER,
                entity: ENTITY,
                slot: EquipmentSlot::Chest
            }]
        );
        assert!(host.equipment.is_empty());
        assert_eq!(
            host.given,
            vec![(PLAYER, stack("minecraft:diamond_chestplate"))]
        );
        assert_eq!(host.action_bars_of(PLAYER), vec!["Needs Defense 16"]);
    }

    #[test]
    fn full_inventory_drops_the_unequipped_item() {
        // Given a level-1 player wearing an enchanted diamond chestplate, with a full inventory
        let _serial = exclusive();
        let (mut host, _) = setup(false);
        let mut chestplate = stack("minecraft:diamond_chestplate");
        chestplate.damage = 17;
        host.equipment
            .push((EquipmentSlot::Chest, chestplate.clone()));
        host.inventory_full = true;

        // When it is taken off
        run(
            &mut host,
            vec![Job::Unequip {
                player: PLAYER,
                entity: ENTITY,
                slot: EquipmentSlot::Chest,
            }],
        );

        // Then it is dropped at their feet as it was, never lost
        assert!(host.equipment.is_empty());
        assert_eq!(host.summoned.len(), 1);
        let (dimension, kind, pos, nbt) = &host.summoned[0];
        assert_eq!(
            (dimension.as_str(), kind.as_str(), *pos),
            ("minecraft:overworld", "minecraft:item", PLAYER_POSITION)
        );
        assert_eq!(nbt.get(&"PickupDelay".into()), Some(&Tag::Short(40)));
        assert_eq!(host.item_entities, vec![(SUMMONED_ID, chestplate)]);
    }

    #[test]
    fn unequipped_item_stays_on_when_it_cannot_be_dropped() {
        // Given a full inventory, and a world that refuses the item entity
        let _serial = exclusive();
        let (mut host, _) = setup(false);
        host.equipment
            .push((EquipmentSlot::Chest, stack("minecraft:diamond_chestplate")));
        host.inventory_full = true;
        host.summon_fails = true;

        // When it is taken off
        run(
            &mut host,
            vec![Job::Unequip {
                player: PLAYER,
                entity: ENTITY,
                slot: EquipmentSlot::Chest,
            }],
        );

        // Then it goes back on rather than vanish
        assert_eq!(
            host.equipment,
            vec![(EquipmentSlot::Chest, stack("minecraft:diamond_chestplate"))]
        );
    }

    #[test]
    fn unequip_leaves_a_slot_no_longer_locked() {
        // Given an unequip queued, then the player raising Defense to 30 before the tick
        let _serial = exclusive();
        let (mut host, _) = setup(false);
        host.equipment
            .push((EquipmentSlot::Chest, stack("minecraft:diamond_chestplate")));
        player(&mut host, true);

        // When the tick runs
        run(
            &mut host,
            vec![Job::Unequip {
                player: PLAYER,
                entity: ENTITY,
                slot: EquipmentSlot::Chest,
            }],
        );

        // Then the chestplate stays on
        assert_eq!(host.equipment.len(), 1);
        assert!(host.given.is_empty());
    }

    #[test]
    fn reload_rechecks_equipment() {
        // Given a level-1 player wearing a diamond chestplate while no lock applies to it
        let _serial = exclusive();
        let mut host = TestHost::default();
        let defense = json!({"format": 1, "id": "skills_and_perks:defense", "name": "n", "icon": "minecraft:stone"});
        load(
            &mut host,
            &[(
                "skills",
                vec![("skills_and_perks:defense", defense.clone())],
            )],
        );
        let actor = player(&mut host, false);
        host.equipment
            .push((EquipmentSlot::Chest, stack("minecraft:diamond_chestplate")));
        host.equipment.push((
            EquipmentSlot::MainHand,
            stack("minecraft:diamond_chestplate"),
        ));
        check_equipment(&mut host, &actor);
        let before = host.equipment.len();

        // When a data pack adds the lock and the data reloads, and the equipment check runs
        with_locks(&mut host);
        check_equipment(&mut host, &actor);

        // Then the worn chestplate comes off; the one merely held stays
        assert_eq!(before, 2);
        assert_eq!(
            host.equipment,
            vec![(
                EquipmentSlot::MainHand,
                stack("minecraft:diamond_chestplate")
            )]
        );
        assert_eq!(
            host.given,
            vec![(PLAYER, stack("minecraft:diamond_chestplate"))]
        );
    }

    #[test]
    fn main_hand_change_queues_attribute_sync() {
        // Given a level-1 player
        let _serial = exclusive();
        let (mut host, actor) = setup(false);

        // When their main hand takes a diamond pickaxe
        on_equip(
            &mut host,
            &actor,
            "mainhand",
            Some("minecraft:diamond_pickaxe"),
        );

        // Then their attributes are resynced (the dig lock follows the main hand), and nothing comes off
        assert_eq!(take_queued(), vec![Job::SyncAttributes(PLAYER)]);
        assert!(host.action_bars.is_empty());
    }
}
