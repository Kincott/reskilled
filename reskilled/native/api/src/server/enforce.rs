//! Lock enforcement: actions a player's skills do not allow are cancelled with a "Needs Mining 16" line, and gear
//! they may not wear comes off.
//!
//! The hooks only find who acts (an [`Actor`]); the functions here decide, so tests reach them without a
//! `NativeServerPlayer`, which a test cannot build.

use std::collections::BTreeMap;
use std::sync::Mutex;

use horizon_client::mod_api::{
    Compound, EntityRef, EquipmentSlot, Flow, Host, ItemStack, NativeInventoryKind,
    NativeServerPlayer, Tag,
};
use skills_core::defs::{Action, Settings};
use skills_core::locks::{Facts, TargetKind, Unmet, check};
use skills_core::progress::level;
use skills_core::registry::Defs;
use skills_core::wire::ToClient;

use super::data::{HostFacts, current, requirements};
use super::effects;
use super::lock;
use super::queue::{self, Job};
use super::state::{runs_mod, state_of};
use crate::MSG_CHANNEL;
use crate::text::{NEEDS, number, say, skill_name};

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
    /// Their selected hotbar slot, 0 to 8.
    pub selected: usize,
}

impl Actor {
    /// The player as a hook sees them; their entity id is remembered for the damage hook, which only has ids.
    pub fn of(player: &NativeServerPlayer, settings: &Settings) -> Self {
        let actor = Actor {
            player: player.uuid,
            entity: u32::try_from(player.entity_id).unwrap_or_default(),
            exempt: exempt(player.game_mode, settings),
            selected: usize::from(player.selected_slot),
        };
        effects::remember_entity(actor.player, actor.entity);
        actor
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
pub fn listed(host: &dyn Host, matches: impl Fn(&NativeServerPlayer) -> bool) -> Option<Actor> {
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
        Some((defs, unmet, action)) => {
            refuse(host, actor.player, &defs, &unmet, action);
            Flow::Cancel
        }
        None => Flow::Continue,
    }
}

/// The first requirement the player does not meet, with the definitions it was judged by and the action it is for.
fn first_unmet(
    host: &mut dyn Host,
    player: u128,
    checks: &[(TargetKind, &str, Action)],
) -> Option<(std::sync::Arc<Defs>, Unmet, Action)> {
    let data = current()?;
    let state = match state_of(host, player) {
        Ok(state) => state,
        Err(error) => {
            log::warn!(
                "Reskilled lets {player:032x} through: their state is unreadable ({error:?})"
            );
            return None;
        }
    };
    let levels = |skill: &str| level(&state, &data.defs, skill);
    checks.iter().find_map(|(kind, id, action)| {
        let reqs = requirements(host, *kind, id);
        check(&reqs, *action, &levels)
            .err()
            .map(|unmet| (data.defs.clone(), unmet, *action))
    })
}

/// The action-bar line a client without the mod reads: vanilla's red, the skill and level in vanilla's gold.
const REFUSED_COLOUR: u32 = 0xFF_5555;
const NEEDED_COLOUR: u32 = 0xFF_AA00;

/// Tick each player last saw a "Needs" line.
static LAST_REFUSAL: Mutex<BTreeMap<u128, u64>> = Mutex::new(BTreeMap::new());

/// Tells the player what they need, at most once per `lock_message_cooldown_ticks`: the action-bar line, and a
/// message their client answers with a sound.
fn refuse(host: &mut dyn Host, player: u128, defs: &Defs, unmet: &Unmet, action: Action) {
    let now = queue::now();
    let cooldown = u64::from(defs.settings.lock_message_cooldown_ticks);
    {
        let mut last = lock(&LAST_REFUSAL);
        if last.get(&player).is_some_and(|said| now < said + cooldown) {
            return;
        }
        last.insert(player, now);
    }
    if !runs_mod(player) {
        let needed = [
            skill_name(defs, &unmet.skill).with_color(NEEDED_COLOUR),
            number(unmet.level).with_color(NEEDED_COLOUR),
        ];
        host.action_bar(player, say(&NEEDS, &needed).with_color(REFUSED_COLOUR));
        return;
    }
    let refused = ToClient::Refused {
        skill: unmet.skill.clone(),
        level: unmet.level,
        action,
    };
    queue::send(
        player,
        MSG_CHANNEL,
        serde_json::to_vec(&refused).expect("a refusal always encodes"),
    );
}

/// Using an item in the air. A right-click that puts on armor the player may not wear is let through and the piece
/// sent back to the hand from the tick, as a real move: refusing the use would leave the client showing the armor
/// it already put on, since the server does not correct a refused use.
pub fn on_use_item(host: &mut dyn Host, actor: &Actor, item: &str) -> Flow {
    let flow = guard(host, actor, &[(TargetKind::Item, item, Action::Use)]);
    if flow == Flow::Continue
        && !actor.exempt
        && first_unmet(
            host,
            actor.player,
            &[(TargetKind::Item, item, Action::Equip)],
        )
        .is_some()
    {
        lock(&HAND_EQUIPS).insert(actor.player, (queue::now(), actor.selected));
    }
    flow
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
        effects::note_main_hand(actor.player, item, actor.exempt);
        queue::queue(Job::SyncAttributes(actor.player));
        return;
    }
    let Some(item) = item else { return };
    if WORN.contains(&slot)
        && guard(host, actor, &[(TargetKind::Item, item, Action::Equip)]) == Flow::Cancel
    {
        let to = match (
            equipped_from_hand(actor.player),
            placed_from_cursor(actor.player),
        ) {
            (Some(hand), _) => Return::Hand(hand),
            (None, true) => Return::Cursor,
            (None, false) => Return::Inventory,
        };
        queue::queue(Job::Unequip {
            player: actor.player,
            entity: actor.entity,
            slot,
            to,
        });
    }
}

/// `inventory_click`'s window id for the player's own inventory, and its modes (vanilla's `ClickType` ids).
pub const PLAYER_WINDOW: i32 = 0;
pub const PICKUP: i32 = 0;
pub const QUICK_MOVE: i32 = 1;
pub const SWAP: i32 = 2;
/// The player window's equipment slots: 5 head, 6 chest, 7 legs, 8 feet, 45 off hand.
const WINDOW_EQUIPMENT: [i32; 5] = [5, 6, 7, 8, 45];
/// The player window's main inventory (9..=35, the same in the inventory) and hotbar (36..=44, inventory 0..=8).
const WINDOW_MAIN: std::ops::RangeInclusive<i32> = 9..=35;
const WINDOW_HOTBAR: std::ops::RangeInclusive<i32> = 36..=44;
const HOTBAR_SIZE: i32 = 9;
/// A click on an equipment slot counts for the equip that follows within this many ticks.
const CLICK_TICKS: u64 = 2;
/// Wearables vanilla puts on by shift-click that no armor tag names.
const ELYTRA: &str = "minecraft:elytra";
const SHIELD: &str = "minecraft:shield";

/// The tick each player last clicked one of their equipment slots, so the item they placed from the cursor can go
/// back to it.
static EQUIPMENT_CLICKS: Mutex<BTreeMap<u128, u64>> = Mutex::new(BTreeMap::new());
/// The tick each player last right-clicked armor they may not wear, and the hotbar slot it was in.
static HAND_EQUIPS: Mutex<BTreeMap<u128, (u64, usize)>> = Mutex::new(BTreeMap::new());

fn equipped_from_hand(player: u128) -> Option<usize> {
    let (tick, hand) = lock(&HAND_EQUIPS).remove(&player)?;
    (queue::now() <= tick + CLICK_TICKS).then_some(hand)
}

fn placed_from_cursor(player: u128) -> bool {
    lock(&EQUIPMENT_CLICKS)
        .remove(&player)
        .is_some_and(|tick| queue::now() <= tick + CLICK_TICKS)
}

/// A click in the player's own inventory, before it moves anything: a shift-click or number key that would put on
/// a wearable they may not wear is refused, so the item stays where it is. A click from the cursor straight into an
/// equipment slot cannot be judged here (the server does not say what the cursor holds); it is noted, and
/// `on_equip` sends the item back to the cursor.
pub fn on_inventory_click(
    host: &mut dyn Host,
    actor: &Actor,
    window: i32,
    slot: i32,
    button: i32,
    mode: i32,
) -> Flow {
    if window != PLAYER_WINDOW || actor.exempt {
        return Flow::Continue;
    }
    let item = match mode {
        PICKUP if WINDOW_EQUIPMENT.contains(&slot) => {
            lock(&EQUIPMENT_CLICKS).insert(actor.player, queue::now());
            return Flow::Continue;
        }
        QUICK_MOVE => match carried_at(host, actor.player, slot) {
            Some(item) if equips_into_empty_slot(host, actor.player, &item) => item,
            _ => return Flow::Continue,
        },
        SWAP if WINDOW_EQUIPMENT.contains(&slot) && (0..HOTBAR_SIZE).contains(&button) => {
            match host
                .player_stacks(actor.player)
                .and_then(|s| s.get(button as usize).cloned().flatten())
            {
                Some(stack) => stack.item.to_string(),
                None => return Flow::Continue,
            }
        }
        _ => return Flow::Continue,
    };
    guard(host, actor, &[(TargetKind::Item, &item, Action::Equip)])
}

/// The item in a window slot of the player's main inventory or hotbar.
fn carried_at(host: &dyn Host, player: u128, window_slot: i32) -> Option<String> {
    let index = if WINDOW_MAIN.contains(&window_slot) {
        window_slot
    } else if WINDOW_HOTBAR.contains(&window_slot) {
        window_slot - WINDOW_HOTBAR.start()
    } else {
        return None;
    };
    let stacks = host.player_stacks(player)?;
    Some(
        stacks
            .get(usize::try_from(index).ok()?)?
            .as_ref()?
            .item
            .to_string(),
    )
}

/// Whether a shift-click would put the item on: it is a wearable whose slot is empty.
fn equips_into_empty_slot(host: &dyn Host, player: u128, item: &str) -> bool {
    let facts = HostFacts(host);
    let slot = [
        ("minecraft:head_armor", EquipmentSlot::Head),
        ("minecraft:chest_armor", EquipmentSlot::Chest),
        ("minecraft:leg_armor", EquipmentSlot::Legs),
        ("minecraft:foot_armor", EquipmentSlot::Feet),
    ]
    .into_iter()
    .find(|(tag, _)| facts.item_in_tag(item, tag))
    .map(|(_, slot)| slot)
    .or_else(|| (item == ELYTRA).then_some(EquipmentSlot::Chest))
    .or_else(|| (item == SHIELD).then_some(EquipmentSlot::OffHand));
    slot.is_some_and(|slot| host.player_equipment(player, slot).is_none())
}

/// The swap-hands key: refused when the main hand holds something the player may not wear in the off hand.
pub fn on_swap_hands(host: &mut dyn Host, actor: &Actor) -> Flow {
    match host.held_stack(actor.player) {
        Some(held) => guard(
            host,
            actor,
            &[(TargetKind::Item, &held.item, Action::Equip)],
        ),
        None => Flow::Continue,
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
                "Reskilled skipped the equipment check of {player:032x}, who is not online"
            ),
        },
        Job::Unequip {
            player,
            entity,
            slot,
            to,
        } => unequip(host, player, entity, slot, to),
        other => log::warn!("Reskilled enforcement got a job it does not run: {other:?}"),
    }
}

/// Takes off every worn item the player may not wear, into their inventory, telling them once.
pub fn check_equipment(host: &mut dyn Host, actor: &Actor) {
    let main_hand = host.held_stack(actor.player);
    effects::note_main_hand(
        actor.player,
        main_hand.as_ref().map(|stack| &*stack.item),
        actor.exempt,
    );
    if actor.exempt {
        return;
    }
    for slot in WORN {
        unequip(host, actor.player, actor.entity, slot, Return::Inventory);
    }
}

/// Where an item taken off goes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Return {
    /// Back on the cursor it was just placed from, as if the slot had refused it; the inventory when the cursor is
    /// taken.
    Cursor,
    /// Back into the hotbar slot it was right-clicked from, swapping with what vanilla put there.
    Hand(usize),
    Inventory,
}

/// The slot's place in `NativeInventoryKind::Player`'s numbering: 36 feet, 37 legs, 38 chest, 39 head, 40 off hand.
pub(crate) fn inventory_index(slot: EquipmentSlot) -> Option<usize> {
    match slot {
        EquipmentSlot::Feet => Some(36),
        EquipmentSlot::Legs => Some(37),
        EquipmentSlot::Chest => Some(38),
        EquipmentSlot::Head => Some(39),
        EquipmentSlot::OffHand => Some(40),
        EquipmentSlot::MainHand | EquipmentSlot::Body | EquipmentSlot::Saddle => None,
    }
}

/// Takes the item in the slot off when the player still may not wear it: to `to` first, else into their
/// inventory, else dropped at their feet, else (the world refuses the drop) left on, so it is never lost. Only the
/// typed inventory helpers are used: `give_stack` cannot reach a single-player world's owner from the server tick.
fn unequip(host: &mut dyn Host, player: u128, entity: u32, slot: EquipmentSlot, to: Return) {
    let Some(index) = inventory_index(slot) else {
        return;
    };
    let Some(worn) = host.player_equipment(player, slot) else {
        return;
    };
    let Some((defs, unmet, action)) = first_unmet(
        host,
        player,
        &[(TargetKind::Item, &worn.item, Action::Equip)],
    ) else {
        return;
    };
    refuse(host, player, &defs, &unmet, action);
    let from = (NativeInventoryKind::Player, index);
    let returned = match to {
        Return::Cursor => relocate(host, player, from, (NativeInventoryKind::Cursor, 0), &worn),
        Return::Hand(hand) => match host
            .player_stacks(player)
            .and_then(|stacks| stacks.get(hand).cloned())
        {
            Some(None) => relocate(
                host,
                player,
                from,
                (NativeInventoryKind::Player, hand),
                &worn,
            ),
            Some(Some(there)) => swap(
                host,
                player,
                from,
                (NativeInventoryKind::Player, hand),
                &worn,
                &there,
            ),
            None => false,
        },
        Return::Inventory => false,
    };
    if returned {
        return;
    }
    let free = host
        .player_stacks(player)
        .and_then(|stacks| stacks.iter().position(Option::is_none));
    if let Some(free) = free
        && relocate(
            host,
            player,
            from,
            (NativeInventoryKind::Player, free),
            &worn,
        )
    {
        return;
    }
    if host
        .set_inventory_slot_typed(player, from.0, from.1, Some(&worn), None)
        .is_err()
    {
        log::warn!("Reskilled could not take {} off {player:032x}", worn.item);
        return;
    }
    let dropped = host
        .entity(entity)
        .map_or(Err(worn.clone()), |entity| drop_at(host, entity, worn));
    if let Err(rest) = dropped {
        log::warn!(
            "Reskilled could not drop {} at {player:032x}, so it stays on",
            rest.item
        );
        let _ = host.set_inventory_slot_typed(player, from.0, from.1, None, Some(rest));
    }
}

/// Swaps two of the player's slots holding `a` and `b`, undoing the first write when the other slot changed.
fn swap(
    host: &mut dyn Host,
    player: u128,
    at_a: (NativeInventoryKind, usize),
    at_b: (NativeInventoryKind, usize),
    a: &ItemStack,
    b: &ItemStack,
) -> bool {
    if host
        .set_inventory_slot_typed(player, at_b.0, at_b.1, Some(b), Some(a.clone()))
        .is_err()
    {
        return false;
    }
    if host
        .set_inventory_slot_typed(player, at_a.0, at_a.1, Some(a), Some(b.clone()))
        .is_ok()
    {
        return true;
    }
    let _ = host.set_inventory_slot_typed(player, at_b.0, at_b.1, Some(a), Some(b.clone()));
    false
}

/// Moves the stack from one of the player's slots to an empty one: the new slot is filled first, then the old one
/// emptied, and the fill undone when the old slot changed meanwhile.
fn relocate(
    host: &mut dyn Host,
    player: u128,
    from: (NativeInventoryKind, usize),
    to: (NativeInventoryKind, usize),
    stack: &ItemStack,
) -> bool {
    if host
        .set_inventory_slot_typed(player, to.0, to.1, None, Some(stack.clone()))
        .is_err()
    {
        return false;
    }
    if host
        .set_inventory_slot_typed(player, from.0, from.1, Some(stack), None)
        .is_ok()
    {
        return true;
    }
    let _ = host.set_inventory_slot_typed(player, to.0, to.1, Some(stack), None);
    false
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
    lock(&EQUIPMENT_CLICKS).clear();
    lock(&HAND_EQUIPS).clear();
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
    use crate::test_host::{PLAYER_POSITION, SUMMONED_ID, TestHost, exclusive};

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
        json!({"target": target, "actions": actions, "requires": {format!("reskilled:{skill}"): level}})
    }

    /// Five skills and one lock per action kind.
    fn with_locks(host: &mut TestHost) {
        let skills: Vec<(String, Value)> = SKILLS
            .iter()
            .map(|s| {
                let id = format!("reskilled:{s}");
                let json = json!({"format": 1, "id": id, "name": format!("reskilled.skill.{s}"), "icon": "minecraft:stone"});
                (id, json)
            })
            .collect();
        let item = |id: &str| json!({"item": {"id": id}});
        let locks = json!({"format": 1, "entries": [
            entry(item("minecraft:diamond_pickaxe"), &["break"], "mining", 16),
            entry(item("minecraft:diamond_sword"), &["attack"], "melee", 8),
            entry(item("minecraft:shield"), &["use", "equip"], "defense", 4),
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
                ("locks", vec![("reskilled:test", locks)]),
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
                    format!("reskilled:{s}"),
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
            selected: 0,
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
        let refused_first = host.action_bars.len();
        for _ in 0..18 {
            run(&mut host, vec![]);
        }
        on_attack(&mut host, &actor, Some("minecraft:diamond_sword"));
        let after_19 = host.action_bars.len();
        run(&mut host, vec![]);
        on_attack(&mut host, &actor, Some("minecraft:diamond_sword"));
        run(&mut host, vec![]);

        // Then every swing is cancelled, but the line comes once per 20 ticks, and a client without the mod is
        // sent nothing else
        assert!(flows.iter().all(|f| *f == Flow::Cancel));
        assert_eq!((refused_first, after_19), (1, 1));
        assert_eq!(host.action_bars.len(), 2);
        assert!(host.sent_to(PLAYER, MSG_CHANNEL).is_empty());
    }

    #[test]
    fn a_client_with_the_mod_draws_the_warning_itself() {
        // Given a level-1 player whose client said it runs the mod
        let _serial = exclusive();
        let (mut host, actor) = setup(false);
        crate::server::actions::handle(&mut host, PLAYER, skills_core::wire::ToServer::Hello);

        // When they swing a diamond sword (Melee 8 here) and the tick runs
        let flow = on_attack(&mut host, &actor, Some("minecraft:diamond_sword"));
        run(&mut host, vec![]);

        // Then it is cancelled, and the client is told what was refused and why instead of an action-bar line
        let told: Vec<ToClient> = host
            .sent_to(PLAYER, MSG_CHANNEL)
            .iter()
            .map(|m| serde_json::from_slice(m).unwrap())
            .collect();
        let refused = ToClient::Refused {
            skill: "reskilled:melee".into(),
            level: 8,
            action: Action::Attack,
        };
        assert_eq!(flow, Flow::Cancel);
        assert_eq!(told, vec![refused]);
        assert!(host.action_bars.is_empty());
    }

    #[test]
    fn the_mod_is_forgotten_when_the_player_leaves() {
        // Given a player whose client said it runs the mod, who then left and came back on a vanilla client
        let _serial = exclusive();
        let (mut host, actor) = setup(false);
        crate::server::actions::handle(&mut host, PLAYER, skills_core::wire::ToServer::Hello);
        crate::server::state::logged_out(PLAYER);

        // When a lock refuses them
        on_attack(&mut host, &actor, Some("minecraft:diamond_sword"));

        // Then they read the action-bar line again
        assert_eq!(host.action_bars_of(PLAYER), vec!["Needs Melee 8"]);
    }

    /// The locks' chestplate, which a level-1 player may not wear (Defense 16).
    fn chestplate() -> ItemStack {
        stack("minecraft:diamond_chestplate")
    }

    fn unequip_job(to: Return) -> Job {
        Job::Unequip {
            player: PLAYER,
            entity: ENTITY,
            slot: EquipmentSlot::Chest,
            to,
        }
    }

    #[test]
    fn right_click_equip_goes_back_to_the_hand() {
        // Given a level-1 player holding the chestplate in hotbar slot 3, wearing nothing
        let _serial = exclusive();
        let (mut host, mut actor) = setup(false);
        actor.selected = 3;
        host.selected = 3;
        host.slots = vec![None; 41];
        host.slots[3] = Some(chestplate());

        // When they right-click with it: the use goes through, vanilla puts it on, the equip is heard, the tick runs
        let flow = on_use_item(&mut host, &actor, "minecraft:diamond_chestplate");
        host.slots[3] = None;
        host.wear(EquipmentSlot::Chest, chestplate());
        on_equip(
            &mut host,
            &actor,
            "chest",
            Some("minecraft:diamond_chestplate"),
        );
        let queued = take_queued();
        run(&mut host, queued.clone());

        // Then it is back in the hand it came from (a real move, so the client is corrected), and they read why
        assert_eq!(flow, Flow::Continue);
        assert_eq!(queued, vec![unequip_job(Return::Hand(3))]);
        assert_eq!(host.worn(EquipmentSlot::Chest), None);
        assert_eq!(host.carried(), vec![(3, chestplate())]);
        assert_eq!(host.action_bars_of(PLAYER), vec!["Needs Defense 16"]);
    }

    #[test]
    fn right_click_swap_puts_both_back() {
        // Given a level-1 player wearing a leather chestplate who right-clicks the diamond one from hotbar slot 0,
        // which vanilla swaps
        let _serial = exclusive();
        let (mut host, actor) = setup(false);
        host.slots = vec![None; 41];
        on_use_item(&mut host, &actor, "minecraft:diamond_chestplate");
        host.slots[0] = Some(stack("minecraft:leather_chestplate"));
        host.wear(EquipmentSlot::Chest, chestplate());

        // When the equip is heard and the tick runs
        on_equip(
            &mut host,
            &actor,
            "chest",
            Some("minecraft:diamond_chestplate"),
        );
        run(&mut host, take_queued());

        // Then the leather one is worn again and the diamond one is back in the hand
        assert_eq!(
            host.worn(EquipmentSlot::Chest),
            Some(&stack("minecraft:leather_chestplate"))
        );
        assert_eq!(host.carried(), vec![(0, chestplate())]);
    }

    #[test]
    fn shift_click_into_an_empty_armor_slot_is_cancelled() {
        // Given a level-1 player with the chestplate in main slot 12 (window slot 12) and in hotbar slot 3 (window
        // slot 39), host facts that know it is chest armor, and nothing worn
        let _serial = exclusive();
        let (mut host, actor) = setup(false);
        host.item_tags = vec![(
            "minecraft:diamond_chestplate".into(),
            "minecraft:chest_armor".into(),
        )];
        host.slots = vec![None; 41];
        host.slots[12] = Some(chestplate());
        host.slots[3] = Some(chestplate());

        // When each is shift-clicked
        let main = on_inventory_click(&mut host, &actor, PLAYER_WINDOW, 12, 0, QUICK_MOVE);
        let hotbar = on_inventory_click(&mut host, &actor, PLAYER_WINDOW, 39, 0, QUICK_MOVE);

        // Then both clicks are refused, so the chestplate stays where it is
        assert_eq!((main, hotbar), (Flow::Cancel, Flow::Cancel));
    }

    #[test]
    fn shift_clicks_that_cannot_equip_pass() {
        // Given a level-1 player with the chestplate in slot 12 while already wearing one, a stick in slot 13, and
        // a skilled player's chestplate in slot 14
        let _serial = exclusive();
        let (mut host, actor) = setup(false);
        host.item_tags = vec![(
            "minecraft:diamond_chestplate".into(),
            "minecraft:chest_armor".into(),
        )];
        host.slots = vec![None; 41];
        host.slots[12] = Some(chestplate());
        host.slots[13] = Some(stack("minecraft:stick"));
        host.wear(EquipmentSlot::Chest, stack("minecraft:leather_chestplate"));

        // When they are shift-clicked, and a chest's window is clicked
        let worn_slot_taken =
            on_inventory_click(&mut host, &actor, PLAYER_WINDOW, 12, 0, QUICK_MOVE);
        let stick = on_inventory_click(&mut host, &actor, PLAYER_WINDOW, 13, 0, QUICK_MOVE);
        let chest = on_inventory_click(&mut host, &actor, 3, 12, 0, QUICK_MOVE);

        // Then they go ahead: none would put the chestplate on
        assert_eq!(
            (worn_slot_taken, stick, chest),
            (Flow::Continue, Flow::Continue, Flow::Continue)
        );
    }

    #[test]
    fn number_key_swap_into_an_armor_slot_is_cancelled() {
        // Given a level-1 player with the chestplate in hotbar slot 2, and a stick in hotbar slot 4
        let _serial = exclusive();
        let (mut host, actor) = setup(false);
        host.slots = vec![None; 41];
        host.slots[2] = Some(chestplate());
        host.slots[4] = Some(stack("minecraft:stick"));

        // When they hover the chest slot (window slot 6) and press 3, then 5
        let chestplate_key = on_inventory_click(&mut host, &actor, PLAYER_WINDOW, 6, 2, SWAP);
        let stick_key = on_inventory_click(&mut host, &actor, PLAYER_WINDOW, 6, 4, SWAP);

        // Then the chestplate's swap is refused, and the stick's is left to vanilla
        assert_eq!((chestplate_key, stick_key), (Flow::Cancel, Flow::Continue));
    }

    #[test]
    fn swap_hands_with_a_locked_shield_is_cancelled() {
        // Given a level-1 player holding a shield (Defense 4 to use and wear), then a stick
        let _serial = exclusive();
        let (mut host, actor) = setup(false);
        host.slots = vec![Some(stack("minecraft:shield"))];

        // When they press the swap-hands key with each
        let shield = on_swap_hands(&mut host, &actor);
        host.slots = vec![Some(stack("minecraft:stick"))];
        let stick = on_swap_hands(&mut host, &actor);

        // Then the shield stays in the main hand, and the stick swaps
        assert_eq!((shield, stick), (Flow::Cancel, Flow::Continue));
    }

    #[test]
    fn armor_placed_from_the_cursor_goes_back_to_the_cursor() {
        // Given a level-1 player who drops the chestplate from the cursor into the chest slot (window slot 6)
        let _serial = exclusive();
        let (mut host, actor) = setup(false);
        let click = on_inventory_click(&mut host, &actor, PLAYER_WINDOW, 6, 0, PICKUP);
        host.wear(EquipmentSlot::Chest, chestplate());

        // When the equip is heard, then the tick runs
        on_equip(
            &mut host,
            &actor,
            "chest",
            Some("minecraft:diamond_chestplate"),
        );
        let queued = take_queued();
        run(&mut host, queued.clone());

        // Then it is back on the cursor, as if the slot had refused it, and they read why
        assert_eq!(click, Flow::Continue);
        assert_eq!(queued, vec![unequip_job(Return::Cursor)]);
        assert_eq!(
            (host.worn(EquipmentSlot::Chest), host.cursor.clone()),
            (None, Some(chestplate()))
        );
        assert!(host.carried().is_empty());
        assert_eq!(host.action_bars_of(PLAYER), vec!["Needs Defense 16"]);
    }

    #[test]
    fn an_old_equipment_click_does_not_send_a_later_equip_to_the_cursor() {
        // Given a level-1 player who clicked the chest slot, then three ticks later is made to wear the chestplate
        // (a dispenser)
        let _serial = exclusive();
        let (mut host, actor) = setup(false);
        on_inventory_click(&mut host, &actor, PLAYER_WINDOW, 6, 0, PICKUP);
        for _ in 0..3 {
            run(&mut host, vec![]);
        }
        host.wear(EquipmentSlot::Chest, chestplate());

        // When the equip is heard
        on_equip(
            &mut host,
            &actor,
            "chest",
            Some("minecraft:diamond_chestplate"),
        );

        // Then it is not sent to the cursor but to the inventory
        assert_eq!(take_queued(), vec![unequip_job(Return::Inventory)]);
    }

    #[test]
    fn clicks_in_other_windows_are_left_alone() {
        // Given a level-1 player with nothing worn and the chestplate in inventory slot 12, at an open chest
        let _serial = exclusive();
        let (mut host, actor) = setup(false);
        host.item_tags = vec![(
            "minecraft:diamond_chestplate".into(),
            "minecraft:chest_armor".into(),
        )];
        host.slots = vec![None; 41];
        host.slots[12] = Some(chestplate());

        // When slot 12 of the chest's window (the chest's own slot, not the inventory's) is shift-clicked
        let flow = on_inventory_click(&mut host, &actor, 3, 12, 0, QUICK_MOVE);

        // Then this mod leaves it to vanilla: the chest's numbering is not the inventory's
        assert_eq!(flow, Flow::Continue);
    }

    #[test]
    fn locked_armor_goes_into_the_inventory() {
        // Given a level-1 player wearing the chestplate (put on by a dispenser, or before a reload), a stick in
        // hotbar slot 0
        let _serial = exclusive();
        let (mut host, _) = setup(false);
        host.slots = vec![Some(stack("minecraft:stick"))];
        host.wear(EquipmentSlot::Chest, chestplate());

        // When it is taken off on the tick
        run(&mut host, vec![unequip_job(Return::Inventory)]);

        // Then it is in the first free inventory slot, not on the ground
        assert_eq!(host.worn(EquipmentSlot::Chest), None);
        assert_eq!(
            host.carried(),
            vec![(0, stack("minecraft:stick")), (1, chestplate())]
        );
        assert!(host.summoned.is_empty());
    }

    #[test]
    fn cursor_return_falls_back_to_the_inventory() {
        // Given a player whose cursor already holds something when the chestplate must come off
        let _serial = exclusive();
        let (mut host, _) = setup(false);
        host.cursor = Some(stack("minecraft:stick"));
        host.wear(EquipmentSlot::Chest, chestplate());

        // When it is taken off back to the cursor
        run(&mut host, vec![unequip_job(Return::Cursor)]);

        // Then the cursor keeps its stick and the chestplate goes into the inventory
        assert_eq!(host.cursor, Some(stack("minecraft:stick")));
        assert_eq!(host.carried(), vec![(0, chestplate())]);
    }

    #[test]
    fn full_inventory_drops_the_unequipped_item() {
        // Given a level-1 player wearing a worn diamond chestplate, with a full inventory
        let _serial = exclusive();
        let (mut host, _) = setup(false);
        let mut worn = chestplate();
        worn.damage = 17;
        host.fill_inventory();
        host.wear(EquipmentSlot::Chest, worn.clone());

        // When it is taken off
        run(&mut host, vec![unequip_job(Return::Inventory)]);

        // Then it is dropped at their feet as it was, never lost
        assert_eq!(host.worn(EquipmentSlot::Chest), None);
        assert_eq!(host.summoned.len(), 1);
        let (dimension, kind, pos, nbt) = &host.summoned[0];
        assert_eq!(
            (dimension.as_str(), kind.as_str(), *pos),
            ("minecraft:overworld", "minecraft:item", PLAYER_POSITION)
        );
        assert_eq!(nbt.get(&"PickupDelay".into()), Some(&Tag::Short(40)));
        assert_eq!(host.item_entities, vec![(SUMMONED_ID, worn)]);
    }

    #[test]
    fn unequipped_item_stays_on_when_it_cannot_be_dropped() {
        // Given a full inventory, and a world that refuses the item entity
        let _serial = exclusive();
        let (mut host, _) = setup(false);
        host.fill_inventory();
        host.wear(EquipmentSlot::Chest, chestplate());
        host.summon_fails = true;

        // When it is taken off
        run(&mut host, vec![unequip_job(Return::Inventory)]);

        // Then it goes back on rather than vanish
        assert_eq!(host.worn(EquipmentSlot::Chest), Some(&chestplate()));
    }

    #[test]
    fn unequip_leaves_a_slot_no_longer_locked() {
        // Given an unequip queued, then the player raising Defense to 30 before the tick
        let _serial = exclusive();
        let (mut host, _) = setup(false);
        host.wear(EquipmentSlot::Chest, chestplate());
        player(&mut host, true);

        // When the tick runs
        run(&mut host, vec![unequip_job(Return::Inventory)]);

        // Then the chestplate stays on
        assert_eq!(host.worn(EquipmentSlot::Chest), Some(&chestplate()));
        assert!(host.carried().is_empty());
    }

    #[test]
    fn reload_rechecks_equipment() {
        // Given a level-1 player wearing a diamond chestplate and holding another, while no lock applies to it
        let _serial = exclusive();
        let mut host = TestHost::default();
        let defense =
            json!({"format": 1, "id": "reskilled:defense", "name": "n", "icon": "minecraft:stone"});
        load(
            &mut host,
            &[("skills", vec![("reskilled:defense", defense.clone())])],
        );
        let actor = player(&mut host, false);
        host.slots = vec![Some(chestplate())];
        host.wear(EquipmentSlot::Chest, chestplate());
        check_equipment(&mut host, &actor);
        let before = host.worn(EquipmentSlot::Chest).cloned();

        // When a data pack adds the lock and the data reloads, and the equipment check runs
        with_locks(&mut host);
        check_equipment(&mut host, &actor);

        // Then the worn chestplate comes off into the inventory; the one merely held stays in the hand
        assert_eq!(before, Some(chestplate()));
        assert_eq!(host.worn(EquipmentSlot::Chest), None);
        assert_eq!(host.carried(), vec![(0, chestplate()), (1, chestplate())]);
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
