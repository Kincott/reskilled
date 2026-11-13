//! A `Host` double for the server hooks' tests, a log capture, and a lock that runs those tests one at a time
//! (the mod's server state is process-wide, as Horizon's trait mods are).

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use horizon_client::mod_api::{
    AttachmentResult, Attachments, Compound, EntityRef, EquipmentSlot, Holder, Hooks, Host,
    ItemStack, LegacyRandom, LevelRef, LivingRef, NativeBlockReadError, NativeWorldEpoch, Text,
};

/// Records what the server sends and answers the tag helpers from its tables.
#[derive(Default)]
pub struct TestHost {
    pub attachments: HashMap<Holder, Attachments>,
    pub sent: Vec<(u128, String, Vec<u8>)>,
    /// (item, tag) pairs, tags without `#`.
    pub item_tags: Vec<(String, String)>,
    /// (block, tag) pairs, tags without `#`.
    pub block_tags: Vec<(String, String)>,
    /// XP levels given (or, negative, taken) per player through `give_levels`.
    pub levels_given: HashMap<u128, i32>,
    pub action_bars: Vec<(u128, Text)>,
    pub told: Vec<(u128, Text)>,
    /// Lines answered on the console.
    pub feedback: Vec<Text>,
    /// The item ids the server knows.
    pub items: Vec<String>,
    /// What the test player wears and holds, by slot.
    pub equipment: Vec<(EquipmentSlot, ItemStack)>,
    /// Whether the test player's inventory is full, so `give_stack` hands everything back.
    pub inventory_full: bool,
    /// Whether summoning an item entity fails.
    pub summon_fails: bool,
    /// Item entities summoned: dimension, kind, position, NBT.
    pub summoned: Vec<(String, String, [f64; 3], Compound)>,
    /// Stacks put into item entities, by entity id.
    pub item_entities: Vec<(u32, ItemStack)>,
    /// Stacks given to players.
    pub given: Vec<(u128, ItemStack)>,
    /// Commands run as the console.
    pub commands: Vec<String>,
    /// Entity id to uuid, for `entity_uuid`.
    pub entity_uuids: HashMap<u32, u128>,
    /// The level's random source, when the test seeds one.
    pub random: Option<LegacyRandom>,
}

/// Where the test player stands.
pub const PLAYER_POSITION: [f64; 3] = [1.5, 64.0, -2.5];
/// The id a summoned item entity gets.
pub const SUMMONED_ID: u32 = 900;
const EPOCH: u64 = 1;

impl Host for TestHost {
    fn attachments(&mut self, holder: Holder) -> AttachmentResult<&mut Attachments> {
        Ok(self.attachments.entry(holder).or_default())
    }

    fn net_send_to_player(
        &mut self,
        player: u128,
        channel: &str,
        data: &[u8],
    ) -> Result<(), String> {
        self.sent.push((player, channel.to_string(), data.to_vec()));
        Ok(())
    }

    fn server_item_in_tag(
        &self,
        stack: &ItemStack,
        tag: &str,
    ) -> Result<bool, NativeBlockReadError> {
        Ok(self
            .item_tags
            .iter()
            .any(|(i, t)| **i == *stack.item && t == tag))
    }

    fn block_name_in_tag(&self, block: &str, tag: &str) -> Result<bool, NativeBlockReadError> {
        Ok(self.block_tags.iter().any(|(b, t)| b == block && t == tag))
    }

    fn give_levels(&mut self, uuid: u128, levels: i32) -> bool {
        *self.levels_given.entry(uuid).or_default() += levels;
        true
    }

    fn action_bar(&mut self, uuid: u128, line: Text) -> bool {
        self.action_bars.push((uuid, line));
        true
    }

    fn tell(&mut self, uuid: u128, line: Text) -> bool {
        self.told.push((uuid, line));
        true
    }

    fn feedback(&mut self, line: Text) {
        self.feedback.push(line);
    }

    fn max_stack_size(&self, item: &str) -> Option<u32> {
        self.items.iter().any(|known| known == item).then_some(64)
    }

    fn entity(&self, id: u32) -> Option<EntityRef> {
        Some(EntityRef::new(NativeWorldEpoch::new(EPOCH), id))
    }

    fn living(&self, entity: EntityRef) -> Option<LivingRef> {
        Some(LivingRef::new(entity))
    }

    fn entity_level(&self, _entity: EntityRef) -> Option<LevelRef> {
        Some(LevelRef::new(
            NativeWorldEpoch::new(EPOCH),
            "minecraft:overworld",
        ))
    }

    fn entity_position(&self, _entity: EntityRef) -> Option<[f64; 3]> {
        Some(PLAYER_POSITION)
    }

    fn living_equipment(&self, _living: LivingRef, slot: EquipmentSlot) -> Option<ItemStack> {
        self.equipment
            .iter()
            .find(|(s, _)| *s == slot)
            .map(|(_, stack)| stack.clone())
    }

    fn living_set_equipment(
        &mut self,
        _living: LivingRef,
        slot: EquipmentSlot,
        stack: Option<ItemStack>,
    ) -> bool {
        self.equipment.retain(|(s, _)| *s != slot);
        self.equipment.extend(stack.map(|stack| (slot, stack)));
        true
    }

    fn give_stack(&mut self, uuid: u128, stack: ItemStack) -> Option<ItemStack> {
        if self.inventory_full {
            return Some(stack);
        }
        self.given.push((uuid, stack));
        None
    }

    fn summon(&mut self, dimension: &str, kind: &str, pos: [f64; 3], nbt: Compound) -> Option<u32> {
        if self.summon_fails {
            return None;
        }
        self.summoned
            .push((dimension.to_string(), kind.to_string(), pos, nbt));
        Some(SUMMONED_ID)
    }

    fn set_item_entity(&mut self, _dimension: &str, id: u32, stack: ItemStack) -> bool {
        self.item_entities.push((id, stack));
        true
    }

    fn run_command(&mut self, line: &str) -> bool {
        self.commands.push(line.to_string());
        true
    }

    fn entity_uuid(&mut self, entity: EntityRef) -> Option<u128> {
        self.entity_uuids.get(&entity.id()).copied()
    }

    fn level_random(&mut self, _dimension: &str) -> Option<&mut LegacyRandom> {
        self.random.as_mut()
    }

    /// Players cannot be built in a test (`NativeServerPlayer` is `#[non_exhaustive]`): nobody is listed.
    fn server_players(&self) -> Vec<horizon_client::mod_api::NativeServerPlayer> {
        Vec::new()
    }
}

impl TestHost {
    /// What the player was told in chat, as plain English text.
    pub fn told_to(&self, player: u128) -> Vec<String> {
        self.told
            .iter()
            .filter(|(p, _)| *p == player)
            .map(|(_, t)| t.plain())
            .collect()
    }

    /// The player's action-bar lines, as plain English text.
    pub fn action_bars_of(&self, player: u128) -> Vec<String> {
        self.action_bars
            .iter()
            .filter(|(p, _)| *p == player)
            .map(|(_, t)| t.plain())
            .collect()
    }

    /// The messages sent to the player on the channel, in order.
    pub fn sent_to(&self, player: u128, channel: &str) -> Vec<&[u8]> {
        self.sent
            .iter()
            .filter(|(p, c, _)| *p == player && c == channel)
            .map(|(_, _, d)| d.as_slice())
            .collect()
    }
}

/// The mod, as the game's dispatch sees it.
pub fn mods() -> [Box<dyn Hooks>; 1] {
    [Box::new(crate::Mod)]
}

static SERIAL: Mutex<()> = Mutex::new(());

/// Runs the calling test alone, with the mod's server state and the captured log emptied.
pub fn exclusive() -> MutexGuard<'static, ()> {
    let guard = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    super::data::reset();
    super::effects::reset();
    super::enforce::reset();
    super::queue::reset();
    super::state::reset();
    logged();
    guard
}

struct Capture;

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static CAPTURE: Capture = Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        LINES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!("{} {}", record.level(), record.args()));
    }
    fn flush(&self) {}
}

/// The log lines since the last call, as `LEVEL message`.
pub fn logged() -> Vec<String> {
    if log::set_logger(&CAPTURE).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    std::mem::take(&mut *LINES.lock().unwrap_or_else(PoisonError::into_inner))
}
