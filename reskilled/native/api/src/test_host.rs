//! A `Host` double for the server hooks' tests, a log capture, and a lock that runs those tests one at a time
//! (the mod's server state is process-wide, as Horizon's trait mods are).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::server::enforce::inventory_index;
use horizon_client::mod_api::{
    AttachmentResult, Attachments, Compound, EntityRef, EquipmentSlot, Holder, Hooks, Host,
    HudBatch, ItemStack, LegacyRandom, LevelRef, NativeBlockReadError, NativeClientState,
    NativeInventoryError, NativeInventoryKind, NativeWorldEpoch, Text,
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
    /// Experience points given per player through `give_experience` (which fails with `give_levels_fails`).
    pub experience_given: HashMap<u128, u32>,
    pub action_bars: Vec<(u128, Text)>,
    pub told: Vec<(u128, Text)>,
    /// Lines answered on the console.
    pub feedback: Vec<Text>,
    /// The item ids the server knows.
    pub items: Vec<String>,
    /// The test player's inventory, as `NativeInventoryKind::Player` numbers it: 0..36 (0..9 the hotbar, 0 held),
    /// 36 feet, 37 legs, 38 chest, 39 head, 40 off hand. Shorter means the rest is empty.
    pub slots: Vec<Option<ItemStack>>,
    /// The stack on the test player's cursor.
    pub cursor: Option<ItemStack>,
    /// The test player's selected hotbar slot.
    pub selected: usize,
    /// This host is a server's game, not the window's.
    pub server: bool,
    /// Whether summoning an item entity fails.
    pub summon_fails: bool,
    /// Item entities summoned: dimension, kind, position, NBT.
    pub summoned: Vec<(String, String, [f64; 3], Compound)>,
    /// Stacks put into item entities, by entity id.
    pub item_entities: Vec<(u32, ItemStack)>,
    /// Commands run as the console.
    pub commands: Vec<String>,
    /// `give_levels` refuses, as it does for a single-player world's owner outside their own hooks.
    pub give_levels_fails: bool,
    /// Every command fails, as inside a player's own hook.
    pub commands_fail: bool,
    /// Commands starting with one of these fail, as when their target is out of reach.
    pub refused_commands: Vec<String>,
    /// Entity id to uuid, for `entity_uuid`.
    pub entity_uuids: HashMap<u32, u128>,
    /// The level's random source, when the test seeds one.
    pub random: Option<LegacyRandom>,
    /// Messages the client sent to the server: channel and data.
    pub sent_to_server: Vec<(String, Vec<u8>)>,
    /// Sounds the client played: event, position, volume, pitch.
    pub sounds: Vec<(String, [f64; 3], f32, f32)>,
    /// Where the client's eyes are.
    pub eye: Option<[f64; 3]>,
    /// The client's own 36 inventory slots.
    pub inventory: Vec<Option<ItemStack>>,
    /// Mod screens the client opened.
    pub screens_opened: Vec<String>,
    /// Mod screens the client closed.
    pub screens_closed: Vec<String>,
    /// The config folder, when the test makes one.
    pub config: Option<std::path::PathBuf>,
    /// The mod whose own screen is open.
    pub open_screen: Option<String>,
    /// Whether the player is playing, with no screen over the world.
    pub playing: bool,
    /// Keys the player bound for mods, by key name.
    pub mod_keys: HashMap<String, String>,
    /// Text drawn: the string, where, and its colour.
    pub drawn: RefCell<Vec<(String, [f32; 2], u32)>>,
    /// Rectangles filled, and their colour.
    pub filled: RefCell<Vec<([f32; 4], u32)>>,
    /// Item icons drawn, and where.
    pub icons: RefCell<Vec<(String, [f32; 2])>>,
    /// Everything drawn, in order: what the preview renders and what order-sensitive tests read.
    pub painted: RefCell<Vec<Paint>>,
    /// The window's size in GUI pixels, for `client_state`.
    pub gui_size: [f32; 2],
    /// Whether a screen other than the world is up, for `client_state`.
    pub screen_up: bool,
    /// The blocks of the overworld the server sees, by position, each with its properties.
    pub blocks: HashMap<[i32; 3], (String, Properties)>,
}

/// A block's properties, as pairs of name and value.
pub type Properties = Vec<(String, String)>;

/// One drawing call, as the test host saw it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Paint {
    Fill {
        rect: [f32; 4],
        argb: u32,
    },
    Text {
        text: String,
        at: [f32; 2],
        rgb: u32,
        shadow: bool,
    },
    Item {
        item: String,
        at: [f32; 2],
        size: f32,
    },
    Quad {
        points: [[f32; 2]; 4],
        argb: u32,
    },
    Tooltip {
        lines: Vec<String>,
        /// Each line's parts with their colours, for the preview.
        spans: Vec<Vec<(String, Option<u32>)>>,
        at: [f32; 2],
    },
}

/// A line's parts with their colours.
pub fn spans_of(line: &Text) -> Vec<(String, Option<u32>)> {
    line.spans
        .iter()
        .map(|span| (span.text.clone(), span.style.color))
        .collect()
}

/// Vanilla's default font advances for the printable ASCII characters from the space on, in GUI pixels, so text
/// lays out in tests as it does in the game.
const ADVANCES: [u8; 95] = [
    4, 2, 4, 6, 6, 6, 6, 2, 4, 4, 4, 6, 2, 6, 2, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 2, 2, 5, 6, 5, 6,
    7, 6, 6, 6, 6, 6, 6, 6, 6, 4, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 4, 6, 4, 6, 6,
    3, 6, 6, 6, 6, 6, 5, 6, 6, 2, 6, 5, 3, 6, 6, 6, 6, 6, 6, 6, 4, 6, 6, 6, 6, 6, 6, 4, 2, 4, 7,
];

/// Where the test player stands.
pub const PLAYER_POSITION: [f64; 3] = [1.5, 64.0, -2.5];
/// The one level the test host has.
pub const OVERWORLD: &str = "minecraft:overworld";
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

    fn give_experience(&mut self, uuid: u128, points: u32) -> bool {
        if self.give_levels_fails {
            return false;
        }
        *self.experience_given.entry(uuid).or_default() += points;
        true
    }

    fn give_levels(&mut self, uuid: u128, levels: i32) -> bool {
        if self.give_levels_fails {
            return false;
        }
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

    fn entity_level(&self, _entity: EntityRef) -> Option<LevelRef> {
        Some(LevelRef::new(
            NativeWorldEpoch::new(EPOCH),
            "minecraft:overworld",
        ))
    }

    fn entity_position(&self, _entity: EntityRef) -> Option<[f64; 3]> {
        Some(PLAYER_POSITION)
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

    fn player_stacks(&self, _uuid: u128) -> Option<Vec<Option<ItemStack>>> {
        Some(
            (0..MAIN_SLOTS)
                .map(|i| self.slots.get(i).cloned().flatten())
                .collect(),
        )
    }

    fn held_stack(&self, _uuid: u128) -> Option<ItemStack> {
        self.slots.get(self.selected).cloned().flatten()
    }

    fn player_equipment(&self, _uuid: u128, slot: EquipmentSlot) -> Option<ItemStack> {
        let index = if slot == EquipmentSlot::MainHand {
            Some(self.selected)
        } else {
            inventory_index(slot)
        };
        self.slots.get(index?).cloned().flatten()
    }

    fn set_inventory_slot_typed(
        &mut self,
        _uuid: u128,
        kind: NativeInventoryKind,
        slot: usize,
        expected: Option<&ItemStack>,
        new: Option<ItemStack>,
    ) -> Result<Option<ItemStack>, NativeInventoryError> {
        let at = match kind {
            NativeInventoryKind::Player if slot < PLAYER_SLOTS => {
                self.slots.resize(PLAYER_SLOTS.max(self.slots.len()), None);
                &mut self.slots[slot]
            }
            NativeInventoryKind::Cursor if slot == 0 => &mut self.cursor,
            _ => return Err(NativeInventoryError::NoSuchSlot),
        };
        if at.as_ref() != expected {
            return Err(NativeInventoryError::StackChanged);
        }
        Ok(std::mem::replace(at, new))
    }

    fn run_command(&mut self, line: &str) -> bool {
        self.commands.push(line.to_string());
        !self.commands_fail
            && !self
                .refused_commands
                .iter()
                .any(|r| line.starts_with(r.as_str()))
    }

    fn run_command_captured(&mut self, line: &str) -> horizon_client::mod_api::NativeCommandResult {
        let ran = self.run_command(line);
        let mut result = horizon_client::mod_api::NativeCommandResult::default();
        result.ran = ran;
        result.result = i32::from(ran);
        if !ran {
            result.output = vec![Text::literal("No player was found")];
        }
        result
    }

    fn entity_uuid(&mut self, entity: EntityRef) -> Option<u128> {
        self.entity_uuids.get(&entity.id()).copied()
    }

    fn level(&self, dimension: &str) -> Option<LevelRef> {
        (dimension == OVERWORLD).then(|| LevelRef::new(NativeWorldEpoch::new(EPOCH), OVERWORLD))
    }

    fn level_block(&self, _level: LevelRef, pos: [i32; 3]) -> Option<String> {
        self.blocks.get(&pos).map(|(block, _)| block.clone())
    }

    fn level_block_property(&self, _level: LevelRef, pos: [i32; 3], key: &str) -> Option<String> {
        let (_, properties) = self.blocks.get(&pos)?;
        properties
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, value)| value.clone())
    }

    fn level_random(&mut self, _dimension: &str) -> Option<&mut LegacyRandom> {
        self.random.as_mut()
    }

    fn window(&self) -> bool {
        !self.server
    }

    fn net_send_to_server(&mut self, channel: &str, data: &[u8]) -> Result<(), String> {
        self.sent_to_server
            .push((channel.to_string(), data.to_vec()));
        Ok(())
    }

    fn play_local_sound(
        &mut self,
        sound: &str,
        at: [f64; 3],
        volume: f32,
        pitch: f32,
        _delay: i32,
    ) -> bool {
        self.sounds.push((sound.to_string(), at, volume, pitch));
        true
    }

    fn client_state(&self) -> Option<NativeClientState> {
        let mut state = NativeClientState::default();
        state.eye_position = self.eye;
        state.gui_size = self.gui_size;
        if self.screen_up {
            state.screen = horizon_client::mod_api::NativeScreenKind::Other;
        }
        Some(state)
    }

    fn own_inventory(&mut self) -> Option<Vec<Option<ItemStack>>> {
        Some(self.inventory.clone())
    }

    fn mod_screen_open(&mut self, mod_id: &str) -> bool {
        self.screens_opened.push(mod_id.to_string());
        true
    }

    fn mod_screen_close(&mut self, mod_id: &str) {
        self.screens_closed.push(mod_id.to_string());
    }

    fn config_dir(&self) -> Result<std::path::PathBuf, String> {
        self.config
            .clone()
            .ok_or_else(|| "no config folder in this test".to_string())
    }

    fn mod_screen(&self) -> Option<&str> {
        self.open_screen.as_deref()
    }

    fn playing(&self) -> bool {
        self.playing
    }

    fn mod_key(&self, _mod_id: &str, name: &str, default: &str) -> String {
        self.mod_keys
            .get(name)
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }

    fn text(&self, _batch: &mut HudBatch, text: &str, at: [f32; 2], rgb: u32, shadow: bool) {
        self.drawn.borrow_mut().push((text.to_string(), at, rgb));
        self.painted.borrow_mut().push(Paint::Text {
            text: text.to_string(),
            at,
            rgb,
            shadow,
        });
    }

    fn text_width(&self, text: &str) -> f32 {
        let advance = |c: char| match u32::from(c).checked_sub(32) {
            Some(n) if n < 95 => ADVANCES[n as usize],
            // The symbols the mod draws from vanilla's font are 8 wide; anything else about a letter.
            _ => 8,
        };
        text.chars().map(|c| f32::from(advance(c))).sum()
    }

    fn fill(&self, _batch: &mut HudBatch, rect: [f32; 4], argb: u32) {
        self.filled.borrow_mut().push((rect, argb));
        self.painted.borrow_mut().push(Paint::Fill { rect, argb });
    }

    fn item(&self, _batch: &mut HudBatch, item: &str, at: [f32; 2], size: f32) -> bool {
        self.icons.borrow_mut().push((item.to_string(), at));
        self.painted.borrow_mut().push(Paint::Item {
            item: item.to_string(),
            at,
            size,
        });
        true
    }

    fn quad(&self, _batch: &mut HudBatch, points: [[f32; 2]; 4], argb: u32) {
        self.painted.borrow_mut().push(Paint::Quad { points, argb });
    }

    fn gui_tooltip(&self, _batch: &mut HudBatch, lines: &[Text], at: [f32; 2]) {
        self.painted.borrow_mut().push(Paint::Tooltip {
            lines: lines.iter().map(Text::plain).collect(),
            spans: lines.iter().map(spans_of).collect(),
            at,
        });
    }

    /// Players cannot be built in a test (`NativeServerPlayer` is `#[non_exhaustive]`): nobody is listed.
    fn server_players(&self) -> Vec<horizon_client::mod_api::NativeServerPlayer> {
        Vec::new()
    }
}

/// `NativeInventoryKind::Player`'s size, and its main part.
const PLAYER_SLOTS: usize = 41;
const MAIN_SLOTS: usize = 36;

impl TestHost {
    /// Puts the stack in the test player's equipment slot.
    pub fn wear(&mut self, slot: EquipmentSlot, stack: ItemStack) {
        let index = inventory_index(slot).expect("a worn slot");
        self.slots.resize(PLAYER_SLOTS.max(self.slots.len()), None);
        self.slots[index] = Some(stack);
    }

    /// What the test player wears in the slot.
    pub fn worn(&self, slot: EquipmentSlot) -> Option<&ItemStack> {
        self.slots.get(inventory_index(slot)?)?.as_ref()
    }

    /// Fills every main inventory slot with cobblestone.
    pub fn fill_inventory(&mut self) {
        self.slots.resize(PLAYER_SLOTS.max(self.slots.len()), None);
        for slot in &mut self.slots[..MAIN_SLOTS] {
            *slot = Some(ItemStack {
                item: "minecraft:cobblestone".into(),
                count: 64,
                damage: 0,
                extra: None,
            });
        }
    }

    /// The main inventory slots holding something, by index.
    pub fn carried(&self) -> Vec<(usize, ItemStack)> {
        self.slots
            .iter()
            .take(MAIN_SLOTS)
            .enumerate()
            .filter_map(|(i, s)| s.clone().map(|s| (i, s)))
            .collect()
    }

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

    /// Every string drawn, in order.
    pub fn drawn_text(&self) -> Vec<String> {
        self.drawn
            .borrow()
            .iter()
            .map(|(text, _, _)| text.clone())
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
    crate::server::data::reset();
    crate::server::effects::reset();
    crate::server::enforce::reset();
    crate::server::queue::reset();
    crate::server::state::reset();
    crate::server::focus::reset();
    crate::server::actions::reset();
    crate::client::cache::reset();
    crate::text::shipped_english::reset();
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
