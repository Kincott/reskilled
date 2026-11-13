//! A `Host` double for the server hooks' tests, a log capture, and a lock that runs those tests one at a time
//! (the mod's server state is process-wide, as Horizon's trait mods are).

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use horizon_client::mod_api::{
    AttachmentResult, Attachments, Holder, Hooks, Host, ItemStack, NativeBlockReadError, Text,
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
}

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
