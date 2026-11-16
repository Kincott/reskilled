//! What the client knows: the definitions (put back together from chunks), item requirements for tooltips, its own
//! synced state, and its settings. Emptied when it leaves a server, whose definition sends may number from 1 again.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{LazyLock, Mutex};

use horizon_client::mod_api::{Holder, Host, ItemStack};
use serde::{Deserialize, Serialize};
use skills_core::defs::Action;
use skills_core::locks::{Requirements, enchantment_requirements};
use skills_core::progress::{PlayerState, level, mastered};
use skills_core::rules::Rules;
use skills_core::wire::{ClientDefs, DefsChunk, Reassembler, ToClient, ToServer};

use crate::server::lock;
use crate::server::state::STATE;
use crate::{ACT_CHANNEL, DEFS_CHANNEL, MSG_CHANNEL};

/// The low note a refused action plays.
pub const REFUSED_SOUND: &str = "minecraft:block.note_block.bass";
const SOUND_VOLUME: f32 = 1.0;
const SOUND_PITCH: f32 = 1.0;
/// What the screen plays, each with its volume: a button's tick (as quiet as vanilla's), a skill raised, a perk
/// unlocked.
pub const CLICK_SOUND: (&str, f32) = ("minecraft:ui.button.click", 0.25);
const RAISED_SOUND: (&str, f32) = ("minecraft:entity.player.levelup", 0.5);
const PERK_SOUND: (&str, f32) = ("minecraft:block.enchantment_table.use", 0.6);
const MASTERED_SOUND: (&str, f32) = ("minecraft:ui.toast.challenge_complete", 0.6);
/// After a request leaves, the progress it brings still counts as the player's own doing for this many ticks.
pub const AWAITING_TICKS: u32 = 40;
/// The client settings file, in the game's config folder.
pub const SETTINGS_FILE: &str = "reskilled-client.json";
/// Item ids asked for in one request, one request a tick.
const NEED_BATCH: usize = 64;

/// The player's own display choices, from `config/reskilled-client.json`, changed on the screen's settings page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClientSettings {
    /// Where the lock warning shows.
    pub warning: WarningPlace,
    /// How long the lock warning stays, in seconds.
    pub warning_seconds: u32,
    /// A padlock over the icons of items the player cannot fully use yet.
    pub lock_icon: bool,
    pub lock_corner: Corner,
    /// Locked items' icons are darkened behind the padlock.
    pub dim_locked: bool,
    /// Tooltips show only the requirements not met yet.
    pub hide_met_requirements: bool,
    /// Tooltips show the player's own level beside a requirement they miss.
    pub show_own_level: bool,
    /// The skill strip beside the inventory and the crafting table.
    pub skill_strip: bool,
    /// The screen's button clicks.
    pub click_sounds: bool,
    /// The sounds of raising a skill and unlocking a perk.
    pub progress_sounds: bool,
    /// The low note when a lock refuses something.
    pub refusal_sound: bool,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            warning: WarningPlace::AboveHotbar,
            warning_seconds: 3,
            lock_icon: true,
            lock_corner: Corner::BottomLeft,
            dim_locked: true,
            hide_met_requirements: false,
            show_own_level: true,
            skill_strip: true,
            click_sounds: true,
            progress_sounds: true,
            refusal_sound: true,
        }
    }
}

/// Where the lock warning shows: above the hotbar (at the top while a screen is up), always at the top, or not at
/// all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningPlace {
    AboveHotbar,
    Top,
    Off,
}

/// A corner of an item's icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    BottomLeft,
    TopLeft,
    TopRight,
    BottomRight,
}

impl ClientSettings {
    /// Writes the settings to the config folder, keeping them for the next game; a failure is logged.
    pub fn save(&self, host: &dyn Host) {
        let Ok(dir) = host.config_dir() else {
            return;
        };
        let path = dir.join(SETTINGS_FILE);
        let text = serde_json::to_string_pretty(self).expect("settings always encode");
        if let Err(error) = std::fs::write(&path, text + "\n") {
            log::warn!("Reskilled could not save {}: {error}", path.display());
        }
    }

    /// The settings file, or the defaults when it is missing or broken (logged).
    pub fn load(host: &dyn Host) -> Self {
        let Ok(dir) = host.config_dir() else {
            return Self::default();
        };
        let path = dir.join(SETTINGS_FILE);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_else(|error| {
            log::warn!("Reskilled ignores {}: {error}", path.display());
            Self::default()
        })
    }
}

/// The screen's two views.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum View {
    /// Every skill as a card.
    #[default]
    Overview,
    /// One skill with its perks.
    Skill,
    /// The player's display settings.
    Settings,
}

/// A lock just refused an action: what the warning over the HUD says, and for how many more ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub skill: String,
    pub level: u32,
    pub action: Action,
    /// The enchantment (id and level) that brings the need, when the item alone would not ask it.
    pub because: Option<(String, u32)>,
    pub ticks_left: u32,
}

/// A warning stays up three seconds by default.
#[cfg(test)]
pub const WARNING_TICKS: u32 = 60;
const TICKS_PER_SECOND: u32 = 20;

#[derive(Default)]
pub struct ClientCache {
    /// The player this client plays as, once the server says.
    pub me: Option<u128>,
    pub defs: Option<ClientDefs>,
    /// Requirements per item id, as the server answered.
    pub reqs: BTreeMap<String, Requirements>,
    /// This player's state, as last synced.
    pub state: PlayerState,
    /// This player's XP level, as the server last said.
    pub xp_levels: Option<u32>,
    /// Locks are off for this player (creative or spectator, by the server's settings), as the server last said.
    pub exempt: bool,
    /// The skill the screen's skill view shows, by its place in id order.
    pub selected: usize,
    /// Which of the screen's views is up.
    pub view: View,
    /// The first row of skill cards on show, and the first perk row, when there are more than fit.
    pub first_row: usize,
    pub first_perk: usize,
    /// Requests the screen made, sent on the next tick (its hooks cannot send).
    pub outgoing: Vec<ToServer>,
    /// The lock warning on show, if one is.
    pub warning: Option<Warning>,
    /// The options' tab on show.
    pub options_tab: super::options::Tab,
    /// The server's rules, and whether this player may change them.
    pub rules: Rules,
    pub rules_editable: bool,
    /// Sounds to play on the next tick (the screen's hooks cannot play them), each with its volume.
    pub sounds: Vec<(&'static str, f32)>,
    /// Ticks left in which new progress is taken for the answer to a request made here, and so chimes.
    awaiting: u32,
    /// Client ticks run, for the screen's animations.
    pub ticks: u64,
    pub settings: ClientSettings,
    settings_loaded: bool,
    /// Items to ask about on the next tick.
    pending: BTreeSet<String>,
    /// Items asked about, not answered yet.
    asked: BTreeSet<String>,
    reassembler: Reassembler,
}

impl ClientCache {
    /// A tooltip found no answer for the item: ask on the next tick, once.
    pub fn missed(&mut self, item: &str) {
        if !self.reqs.contains_key(item) && !self.asked.contains(item) {
            self.pending.insert(item.to_string());
        }
    }

    /// What the stack needs: its item's requirements, as the server answered, and those of its enchantments while
    /// the server's rules lock them. None until the server has answered for the item (it is asked for then).
    pub fn requirements_of(&mut self, stack: &ItemStack) -> Option<Requirements> {
        let defs = self.defs.as_ref()?;
        let Some(reqs) = self.reqs.get(&*stack.item) else {
            self.missed(&stack.item);
            return None;
        };
        if !self.rules.enchantment_locks || stack.enchantments().is_empty() {
            return Some(reqs.clone());
        }
        let enchanted: Vec<(String, u32)> = stack
            .enchantments()
            .iter()
            .map(|(id, level)| (id.to_string(), *level))
            .collect();
        Some(
            reqs.clone()
                .merged(&enchantment_requirements(&defs.defs.locks, &enchanted)),
        )
    }

    /// The screen asks the server for something: it goes out on the next tick, and the progress it brings chimes.
    pub fn asked_for(&mut self, request: ToServer) {
        self.outgoing.push(request);
        self.awaiting = AWAITING_TICKS;
    }

    /// Takes the state the server synced, chiming for a level or a perk the player just asked for.
    fn synced(&mut self, state: PlayerState) {
        if self.awaiting > 0 && self.settings.progress_sounds {
            let before = |skill: &str| self.state.skills.get(skill);
            let raised = state
                .skills
                .iter()
                .any(|(id, now)| now.level > before(id).map_or(1, |was| was.level));
            let perks = |state: &PlayerState| -> usize {
                state.skills.values().map(|skill| skill.perks.len()).sum()
            };
            let newly_mastered = self.defs.as_ref().is_some_and(|defs| {
                defs.defs.skills.keys().any(|skill| {
                    mastered(&defs.defs, &state, skill) && !mastered(&defs.defs, &self.state, skill)
                })
            });
            if newly_mastered {
                self.sounds.push(MASTERED_SOUND);
            } else if raised {
                self.sounds.push(RAISED_SOUND);
            } else if perks(&state) > perks(&self.state) {
                self.sounds.push(PERK_SOUND);
            }
        }
        self.state = state;
    }

    /// Whether the player lacks a level the item needs for one of the actions `counts` picks. An item the server
    /// has not answered for is asked about and counts as free until then; so does every item while locks are off
    /// for this player.
    pub fn lacks(&mut self, stack: &ItemStack, counts: impl Fn(Action) -> bool) -> bool {
        if self.exempt {
            return false;
        }
        let (Some(reqs), Some(defs)) = (self.requirements_of(stack), &self.defs) else {
            return false;
        };
        reqs.by_action
            .iter()
            .filter(|(action, _)| counts(**action))
            .flat_map(|(_, needs)| needs)
            .any(|(skill, needed)| level(&self.state, &defs.defs, skill) < *needed)
    }
}

static CACHE: LazyLock<Mutex<ClientCache>> = LazyLock::new(Mutex::default);

/// Runs `f` on the client's cache.
pub fn with<R>(f: impl FnOnce(&mut ClientCache) -> R) -> R {
    f(&mut lock(&CACHE))
}

/// A message from the server.
pub fn on_message(host: &mut dyn Host, channel: &str, data: &[u8]) {
    if channel == DEFS_CHANNEL {
        let Ok(chunk) = serde_json::from_slice::<DefsChunk>(data) else {
            log::warn!("Reskilled got a definitions message it cannot read");
            return;
        };
        let Some(json) = with(|cache| cache.reassembler.push(chunk)) else {
            return;
        };
        match serde_json::from_str::<ClientDefs>(&json) {
            Ok(defs) => with(|cache| {
                cache.defs = Some(defs);
                cache.reqs.clear();
                cache.asked.clear();
            }),
            Err(error) => log::warn!("Reskilled got definitions it cannot read: {error}"),
        }
        return;
    }
    if channel != MSG_CHANNEL {
        return;
    }
    match serde_json::from_slice::<ToClient>(data) {
        Ok(ToClient::Reqs { items }) => with(|cache| {
            for (item, reqs) in items {
                cache.asked.remove(&item);
                cache.reqs.insert(item, reqs);
            }
        }),
        Ok(ToClient::Open) => super::screen::open(host),
        Ok(ToClient::Refused {
            skill,
            level,
            action,
            because,
        }) => {
            let settings = with(|cache| cache.settings.clone());
            if settings.refusal_sound
                && let Some(at) = host.client_state().and_then(|state| state.eye_position)
            {
                host.play_local_sound(REFUSED_SOUND, at, SOUND_VOLUME, SOUND_PITCH, 0);
            }
            with(|cache| {
                cache.warning = Some(Warning {
                    skill,
                    level,
                    action,
                    because,
                    ticks_left: settings
                        .warning_seconds
                        .saturating_mul(TICKS_PER_SECOND)
                        .max(1),
                });
            });
        }
        Ok(ToClient::You { player }) => with(|cache| {
            cache.me = Some(player);
            cache.outgoing.push(ToServer::Hello);
        }),
        Ok(ToClient::Status { levels, exempt }) => with(|cache| {
            cache.xp_levels = Some(levels);
            cache.exempt = exempt;
        }),
        Ok(ToClient::Rules { rules, editable }) => with(|cache| {
            cache.rules = rules;
            cache.rules_editable = editable;
        }),
        Err(error) => log::warn!("Reskilled got a message it cannot read: {error}"),
    }
}

/// Each client tick: settings once, this player's synced state, and one request for the requirements of items
/// tooltips missed or the inventory holds.
pub fn on_tick(host: &mut dyn Host) {
    if !host.window() {
        return;
    }
    with(|cache| cache.ticks += 1);
    with(|cache| {
        cache.warning = cache.warning.take().and_then(|warning| {
            let ticks_left = warning.ticks_left.checked_sub(1).filter(|left| *left > 0)?;
            Some(Warning {
                ticks_left,
                ..warning
            })
        });
    });
    if !with(|cache| cache.settings_loaded) {
        let settings = ClientSettings::load(host);
        with(|cache| {
            cache.settings = settings;
            cache.settings_loaded = true;
        });
    }
    if let Some(me) = with(|cache| cache.me)
        && let Ok(state) = STATE.get(host, Holder::Player(me))
    {
        with(|cache| cache.synced(state));
    }
    let sounds = with(|cache| {
        cache.awaiting = cache.awaiting.saturating_sub(1);
        std::mem::take(&mut cache.sounds)
    });
    if let Some(at) = host.client_state().and_then(|state| state.eye_position) {
        for (sound, volume) in sounds {
            host.play_local_sound(sound, at, volume, SOUND_PITCH, 0);
        }
    }
    if with(|cache| cache.defs.is_none()) {
        return;
    }
    let carried: Vec<String> = host
        .own_inventory()
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .map(|stack| stack.item.to_string())
        .collect();
    let batch: Vec<String> = with(|cache| {
        for item in &carried {
            cache.missed(item);
        }
        let batch: Vec<String> = cache.pending.iter().take(NEED_BATCH).cloned().collect();
        for item in &batch {
            cache.pending.remove(item);
            cache.asked.insert(item.clone());
        }
        batch
    });
    let mut messages = with(|cache| std::mem::take(&mut cache.outgoing));
    if !batch.is_empty() {
        messages.insert(0, ToServer::Need { items: batch });
    }
    for message in messages {
        let data = serde_json::to_vec(&message).expect("a request always encodes");
        if let Err(error) = host.net_send_to_server(ACT_CHANNEL, &data) {
            log::debug!("Reskilled could not send a request: {error}");
        }
    }
}

/// Leaving a server: forget everything about it, keeping the player's settings.
pub fn on_disconnect() {
    with(|cache| {
        let settings = std::mem::take(&mut cache.settings);
        let settings_loaded = cache.settings_loaded;
        *cache = ClientCache {
            settings,
            settings_loaded,
            ..ClientCache::default()
        };
    });
}

#[cfg(test)]
pub(crate) fn reset() {
    with(|cache| *cache = ClientCache::default());
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::{
        Holder, Host, ItemStack, dispatch_client_disconnecting, dispatch_net_from_server,
        dispatch_tick,
    };
    use skills_core::defs::{Action, SkillDef};
    use skills_core::locks::Requirements;
    use skills_core::progress::{PlayerState, Request, SkillProgress};
    use skills_core::registry::Defs;
    use skills_core::wire::{DefsChunk, ToClient, ToServer, chunk_defs, client_defs_json};

    use super::*;
    use crate::server::state::STATE;
    use crate::test_host::{TestHost, exclusive, mods};
    use crate::{ACT_CHANNEL, DEFS_CHANNEL, MAX_MESSAGE_BYTES, MSG_CHANNEL};

    const ME: u128 = 0x5eed;

    /// Definitions with this many skills, each with a long name so they span several chunks.
    fn defs_with(skills: usize) -> Defs {
        let mut defs = Defs::default();
        for n in 0..skills {
            let id = format!("reskilled:skill_{n:04}");
            let skill = SkillDef {
                id: id.clone(),
                priority: 0,
                name: format!("reskilled.skill.a_long_translation_key_to_fill_the_chunks_{n:04}"),
                icon: "minecraft:stone".into(),
                max_level: None,
                cost: None,
                points_every: None,
                icon_levels: Default::default(),
            };
            defs.skills.insert(id, skill);
        }
        defs
    }

    fn send_defs(host: &mut TestHost, defs: &Defs, send_id: u32) {
        for chunk in chunk_defs(&client_defs_json(defs), send_id, MAX_MESSAGE_BYTES) {
            dispatch_net_from_server(
                host,
                &mods(),
                DEFS_CHANNEL,
                &serde_json::to_vec(&chunk).unwrap(),
            );
        }
    }

    fn send_msg(host: &mut TestHost, message: &ToClient) {
        dispatch_net_from_server(
            host,
            &mods(),
            MSG_CHANNEL,
            &serde_json::to_vec(message).unwrap(),
        );
    }

    fn reqs(skill: &str, level: u32) -> Requirements {
        Requirements {
            by_action: [(Action::Break, [(skill.to_string(), level)].into())].into(),
        }
    }

    fn needs_sent(host: &TestHost) -> Vec<Vec<String>> {
        host.sent_to_server
            .iter()
            .filter(|(channel, _)| channel == ACT_CHANNEL)
            .filter_map(|(_, data)| match serde_json::from_slice(data).unwrap() {
                ToServer::Need { items } => Some(items),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn defs_reassemble_across_messages() {
        // Given definitions too big for one message
        let _serial = exclusive();
        let mut host = TestHost::default();
        let defs = defs_with(600);
        assert!(chunk_defs(&client_defs_json(&defs), 1, MAX_MESSAGE_BYTES).len() > 2);

        // When their chunks arrive
        send_defs(&mut host, &defs, 1);

        // Then the client holds the whole set
        let held = with(|cache| cache.defs.as_ref().map(|d| d.defs.skills.len()));
        assert_eq!(held, Some(600));
    }

    #[test]
    fn half_a_send_is_not_used() {
        // Given only the first chunk of a send
        let _serial = exclusive();
        let mut host = TestHost::default();
        let chunks = chunk_defs(&client_defs_json(&defs_with(600)), 1, MAX_MESSAGE_BYTES);
        let first: &DefsChunk = &chunks[0];

        // When it arrives, then garbage on the same channel
        dispatch_net_from_server(
            &mut host,
            &mods(),
            DEFS_CHANNEL,
            &serde_json::to_vec(first).unwrap(),
        );
        dispatch_net_from_server(&mut host, &mods(), DEFS_CHANNEL, b"not json");

        // Then the client still has no definitions
        assert!(with(|cache| cache.defs.is_none()));
    }

    #[test]
    fn reqs_fill_the_cache() {
        // Given definitions
        let _serial = exclusive();
        let mut host = TestHost::default();
        send_defs(&mut host, &defs_with(1), 1);

        // When the server answers with the diamond pickaxe's requirements
        let pickaxe = reqs("reskilled:skill_0000", 16);
        send_msg(
            &mut host,
            &ToClient::Reqs {
                items: vec![("minecraft:diamond_pickaxe".into(), pickaxe.clone())],
            },
        );

        // Then they are cached
        assert_eq!(
            with(|cache| cache.reqs.get("minecraft:diamond_pickaxe").cloned()),
            Some(pickaxe)
        );
    }

    #[test]
    fn new_defs_clear_the_reqs_cache() {
        // Given cached requirements
        let _serial = exclusive();
        let mut host = TestHost::default();
        send_defs(&mut host, &defs_with(1), 1);
        send_msg(
            &mut host,
            &ToClient::Reqs {
                items: vec![("minecraft:stick".into(), Requirements::default())],
            },
        );

        // When a newer send of the definitions completes (a /reload)
        send_defs(&mut host, &defs_with(2), 2);

        // Then the old answers are dropped, to be asked again
        assert!(with(|cache| cache.reqs.is_empty()));
        assert_eq!(
            with(|cache| cache.defs.as_ref().map(|d| d.defs.skills.len())),
            Some(2)
        );
    }

    #[test]
    fn refused_plays_the_sound() {
        // Given the client's eyes at a spot
        let _serial = exclusive();
        let mut host = TestHost {
            eye: Some([3.0, 70.5, -8.0]),
            ..TestHost::default()
        };

        // When the server says a lock refused an action
        send_msg(
            &mut host,
            &ToClient::Refused {
                skill: "reskilled:mining".into(),
                level: 16,
                action: Action::Break,
                because: None,
            },
        );

        // Then the low note plays there, and the warning is up for its three seconds
        assert_eq!(
            host.sounds,
            vec![(REFUSED_SOUND.to_string(), [3.0, 70.5, -8.0], 1.0, 1.0)]
        );
        assert_eq!(REFUSED_SOUND, "minecraft:block.note_block.bass");
        let warning = Warning {
            skill: "reskilled:mining".into(),
            level: 16,
            action: Action::Break,
            because: None,
            ticks_left: 60,
        };
        assert_eq!(with(|cache| cache.warning.clone()), Some(warning));
    }

    #[test]
    fn the_warning_and_the_note_follow_the_players_settings() {
        // Given a player who wants the warning for 5 seconds and no refusal note
        let _serial = exclusive();
        let mut host = TestHost {
            eye: Some([3.0, 70.5, -8.0]),
            ..TestHost::default()
        };
        with(|cache| {
            cache.settings.warning_seconds = 5;
            cache.settings.refusal_sound = false;
        });

        // When a lock refuses them
        let refused = ToClient::Refused {
            skill: "reskilled:mining".into(),
            level: 16,
            action: Action::Break,
            because: None,
        };
        send_msg(&mut host, &refused);

        // Then the warning is up for 100 ticks, and nothing plays
        assert_eq!(
            with(|cache| cache.warning.as_ref().map(|w| w.ticks_left)),
            Some(100)
        );
        assert!(host.sounds.is_empty());
    }

    #[test]
    fn mastering_a_skill_plays_a_fanfare_once() {
        // Given a client that knows who it is, its skill at 31 of 32 (it has no perks)
        let _serial = exclusive();
        let mut host = TestHost {
            eye: Some([3.0, 70.5, -8.0]),
            ..TestHost::default()
        };
        send_defs(&mut host, &defs_with(1), 1);
        send_msg(&mut host, &ToClient::You { player: ME });
        synced(&mut host, 31, &[]);

        // When the player raises it to 32, then a later sync arrives unchanged
        let raise = ToServer::Act(Request::Raise {
            skill: "reskilled:skill_0000".into(),
            times: 1,
        });
        with(|cache| cache.asked_for(raise));
        synced(&mut host, 32, &[]);
        synced(&mut host, 32, &[]);

        // Then the fanfare plays once, in place of the level-up sound
        let played: Vec<&str> = host
            .sounds
            .iter()
            .map(|(sound, ..)| sound.as_str())
            .collect();
        assert_eq!(played, vec!["minecraft:ui.toast.challenge_complete"]);
    }

    #[test]
    fn chimes_are_silent_when_the_player_turned_them_off() {
        // Given a client that knows who it is, its skill at 4, chimes off
        let _serial = exclusive();
        let mut host = TestHost {
            eye: Some([3.0, 70.5, -8.0]),
            ..TestHost::default()
        };
        send_defs(&mut host, &defs_with(1), 1);
        send_msg(&mut host, &ToClient::You { player: ME });
        synced(&mut host, 4, &[]);
        with(|cache| cache.settings.progress_sounds = false);

        // When the player asks for a raise and it arrives
        let raise = ToServer::Act(Request::Raise {
            skill: "reskilled:skill_0000".into(),
            times: 1,
        });
        with(|cache| cache.asked_for(raise));
        synced(&mut host, 5, &[]);

        // Then nothing plays
        assert!(host.sounds.is_empty());
    }

    #[test]
    fn the_client_keeps_the_servers_rules_and_whether_it_may_change_them() {
        // Given a client
        let _serial = exclusive();
        let mut host = TestHost::default();
        let rules = skills_core::rules::Rules {
            respec: skills_core::rules::Respec::Free,
            ..Default::default()
        };

        // When the server sends its rules to an operator
        send_msg(
            &mut host,
            &ToClient::Rules {
                rules: rules.clone(),
                editable: true,
            },
        );

        // Then the client holds them and knows it may change them
        assert_eq!(
            with(|cache| (cache.rules.clone(), cache.rules_editable)),
            (rules, true)
        );
    }

    #[test]
    fn the_warning_goes_after_its_time() {
        // Given a warning just raised
        let _serial = exclusive();
        let mut host = TestHost::default();
        let refused = ToClient::Refused {
            skill: "reskilled:mining".into(),
            level: 16,
            action: Action::Break,
            because: None,
        };
        send_msg(&mut host, &refused);

        // When 59 ticks pass, then one more
        for _ in 0..59 {
            dispatch_tick(&mut host, &mods());
        }
        let after_59 = with(|cache| cache.warning.as_ref().map(|w| w.ticks_left));
        dispatch_tick(&mut host, &mods());

        // Then it is still up with a tick to go, then gone
        assert_eq!(after_59, Some(1));
        assert_eq!(with(|cache| cache.warning.clone()), None);
    }

    #[test]
    fn the_client_says_it_runs_the_mod_once_the_server_names_it() {
        // Given a client that has the definitions
        let _serial = exclusive();
        let mut host = TestHost::default();
        send_defs(&mut host, &defs_with(1), 1);

        // When the server names the player, and two ticks pass
        send_msg(&mut host, &ToClient::You { player: ME });
        dispatch_tick(&mut host, &mods());
        dispatch_tick(&mut host, &mods());

        // Then the client says hello once, so the server leaves lock warnings to it
        let said: Vec<ToServer> = host
            .sent_to_server
            .iter()
            .map(|(_, data)| serde_json::from_slice(data).unwrap())
            .collect();
        assert_eq!(said, vec![ToServer::Hello]);
    }

    #[test]
    fn status_message_sets_the_xp_level_and_whether_locks_are_off() {
        // Given a client
        let _serial = exclusive();
        let mut host = TestHost::default();

        // When the server says the player's XP level and that locks are off for them
        let status = ToClient::Status {
            levels: 31,
            exempt: true,
        };
        send_msg(&mut host, &status);

        // Then the client holds both
        assert_eq!(
            with(|cache| (cache.xp_levels, cache.exempt)),
            (Some(31), true)
        );
    }

    /// Syncs a state for the named player and runs a tick, so the client reads it.
    fn synced(host: &mut TestHost, level: u32, perks: &[&str]) {
        let mut state = PlayerState::default();
        state.skills.insert(
            "reskilled:skill_0000".into(),
            SkillProgress {
                level,
                perks: perks.iter().map(|p| (*p).to_string()).collect(),
                ..SkillProgress::default()
            },
        );
        STATE
            .set(host as &mut dyn Host, Holder::Player(ME), state)
            .unwrap();
        dispatch_tick(host, &mods());
    }

    #[test]
    fn a_raise_and_a_perk_the_player_asked_for_chime() {
        // Given a client that knows who it is, its skill at 4, its eyes at a spot
        let _serial = exclusive();
        let mut host = TestHost {
            eye: Some([3.0, 70.5, -8.0]),
            ..TestHost::default()
        };
        send_defs(&mut host, &defs_with(1), 1);
        send_msg(&mut host, &ToClient::You { player: ME });
        synced(&mut host, 4, &[]);
        let at_login = host.sounds.len();

        // When the player asks for a raise and it arrives, then for a perk and it arrives, then a level arrives
        // long after anything was asked
        let raise = ToServer::Act(Request::Raise {
            skill: "reskilled:skill_0000".into(),
            times: 1,
        });
        with(|cache| cache.asked_for(raise));
        synced(&mut host, 5, &[]);
        let buy = ToServer::Act(Request::BuyPerk {
            perk: "reskilled:perk".into(),
        });
        with(|cache| cache.asked_for(buy));
        synced(&mut host, 5, &["reskilled:perk"]);
        for _ in 0..AWAITING_TICKS {
            dispatch_tick(&mut host, &mods());
        }
        synced(&mut host, 6, &["reskilled:perk"]);

        // Then the first sync is silent, the raise plays the level-up sound, the perk its own, and a level nobody
        // asked for here (an operator's command) is silent
        let played: Vec<(&str, f32)> = host
            .sounds
            .iter()
            .map(|(sound, _, volume, _)| (sound.as_str(), *volume))
            .collect();
        assert_eq!(at_login, 0);
        assert_eq!(
            played,
            vec![
                ("minecraft:entity.player.levelup", 0.5),
                ("minecraft:block.enchantment_table.use", 0.6)
            ]
        );
    }

    #[test]
    fn open_opens_the_screen_on_the_overview() {
        // Given a client whose screen was last on a skill's view
        let _serial = exclusive();
        let mut host = TestHost::default();
        with(|cache| cache.view = View::Skill);

        // When the server asks it to open the skills screen (/skills open)
        send_msg(&mut host, &ToClient::Open);

        // Then the mod's screen opens on the overview
        assert_eq!(host.screens_opened, vec!["reskilled"]);
        assert_eq!(with(|cache| cache.view), View::Overview);
    }

    #[test]
    fn miss_is_requested_once_on_next_tick() {
        // Given definitions, and a tooltip that missed the diamond pickaxe twice
        let _serial = exclusive();
        let mut host = TestHost::default();
        send_defs(&mut host, &defs_with(1), 1);
        with(|cache| {
            cache.missed("minecraft:diamond_pickaxe");
            cache.missed("minecraft:diamond_pickaxe");
        });

        // When two ticks pass
        dispatch_tick(&mut host, &mods());
        dispatch_tick(&mut host, &mods());

        // Then it was asked for once
        assert_eq!(
            needs_sent(&host),
            vec![vec!["minecraft:diamond_pickaxe".to_string()]]
        );
    }

    #[test]
    fn inventory_items_are_asked_for_64_at_a_time() {
        // Given definitions and 100 different items in the inventory
        let _serial = exclusive();
        let mut host = TestHost::default();
        send_defs(&mut host, &defs_with(1), 1);
        host.inventory = (0..100)
            .map(|n| {
                Some(ItemStack {
                    item: format!("minecraft:item_{n:03}").into(),
                    count: 1,
                    damage: 0,
                    extra: None,
                })
            })
            .chain([None])
            .collect();

        // When two ticks pass
        dispatch_tick(&mut host, &mods());
        dispatch_tick(&mut host, &mods());

        // Then they are asked for, 64 then 36, never twice
        let asked: Vec<usize> = needs_sent(&host).iter().map(Vec::len).collect();
        assert_eq!(asked, vec![64, 36]);
    }

    #[test]
    fn nothing_is_asked_before_the_server_sends_definitions() {
        // Given a server without the mod: no definitions ever arrive
        let _serial = exclusive();
        let mut host = TestHost::default();
        with(|cache| cache.missed("minecraft:diamond_pickaxe"));

        // When a tick passes
        dispatch_tick(&mut host, &mods());

        // Then nothing is sent
        assert!(host.sent_to_server.is_empty());
    }

    #[test]
    fn own_state_is_read_from_the_synced_attachment() {
        // Given the server told the client who it is, and synced its state
        let _serial = exclusive();
        let mut host = TestHost::default();
        send_defs(&mut host, &defs_with(1), 1);
        send_msg(&mut host, &ToClient::You { player: ME });
        let mut state = PlayerState::default();
        state.skills.insert(
            "reskilled:skill_0000".into(),
            SkillProgress {
                level: 9,
                ..SkillProgress::default()
            },
        );
        STATE
            .set(
                &mut host as &mut dyn Host,
                Holder::Player(ME),
                state.clone(),
            )
            .unwrap();

        // When a tick passes
        dispatch_tick(&mut host, &mods());

        // Then the client holds that state
        assert_eq!(with(|cache| cache.state.clone()), state);
    }

    #[test]
    fn disconnect_forgets_the_server() {
        // Given definitions, answers, an identity and half of a newer send
        let _serial = exclusive();
        let mut host = TestHost::default();
        send_defs(&mut host, &defs_with(1), 5);
        send_msg(&mut host, &ToClient::You { player: ME });
        send_msg(
            &mut host,
            &ToClient::Reqs {
                items: vec![("minecraft:stick".into(), Requirements::default())],
            },
        );

        // When the client leaves, then joins another server whose sends start again at 1
        dispatch_client_disconnecting(&host, &mods());
        let after_leaving = with(|cache| {
            (
                cache.defs.is_none(),
                cache.reqs.is_empty(),
                cache.me.is_none(),
            )
        });
        send_defs(&mut host, &defs_with(3), 1);

        // Then nothing of the first server is left, and the second server's definitions are taken
        assert_eq!(after_leaving, (true, true, true));
        assert_eq!(
            with(|cache| cache.defs.as_ref().map(|d| d.defs.skills.len())),
            Some(3)
        );
    }

    #[test]
    fn settings_come_from_the_config_file() {
        // Given a config folder with the setting turned on, and one with a broken file
        let _serial = exclusive();
        let root = std::env::temp_dir().join(format!("reskilled_test_{}", std::process::id()));
        let (good, bad) = (root.join("good"), root.join("bad"));
        for (dir, text) in [
            (&good, r#"{"hide_met_requirements": true}"#),
            (&bad, "{ broken"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(SETTINGS_FILE), text).unwrap();
        }

        // When each is read, and a missing one
        let read = [Some(good), Some(bad), None].map(|config| {
            let host = TestHost {
                config,
                ..TestHost::default()
            };
            ClientSettings::load(&host)
        });
        std::fs::remove_dir_all(&root).unwrap();

        // Then the setting is read where it is valid, and defaults (show everything) otherwise
        let hide: Vec<bool> = read.iter().map(|s| s.hide_met_requirements).collect();
        assert_eq!(hide, vec![true, false, false]);
    }
}
