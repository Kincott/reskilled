//! What the client knows: the definitions (put back together from chunks), item requirements for tooltips, its own
//! synced state, and its settings. Emptied when it leaves a server, whose definition sends may number from 1 again.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{LazyLock, Mutex};

use horizon_client::mod_api::{Holder, Host};
use serde::Deserialize;
use skills_core::locks::Requirements;
use skills_core::progress::PlayerState;
use skills_core::wire::{ClientDefs, DefsChunk, Reassembler, ToClient, ToServer};

use crate::server::lock;
use crate::server::state::STATE;
use crate::{ACT_CHANNEL, DEFS_CHANNEL, MOD_ID, MSG_CHANNEL};

/// The low note a refused action plays.
pub const REFUSED_SOUND: &str = "minecraft:block.note_block.bass";
const SOUND_VOLUME: f32 = 1.0;
const SOUND_PITCH: f32 = 1.0;
/// The client settings file, in the game's config folder.
pub const SETTINGS_FILE: &str = "skills_and_perks-client.json";
/// Item ids asked for in one request, one request a tick.
const NEED_BATCH: usize = 64;

/// The player's own choices, from `config/skills_and_perks-client.json`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClientSettings {
    /// Tooltips show only the requirements not met yet.
    pub hide_met_requirements: bool,
}

impl ClientSettings {
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
            log::warn!("Skills & Perks ignores {}: {error}", path.display());
            Self::default()
        })
    }
}

#[derive(Default)]
pub struct ClientCache {
    /// The player this client plays as, once the server says.
    pub me: Option<u128>,
    pub defs: Option<ClientDefs>,
    /// Requirements per item id, as the server answered.
    pub reqs: BTreeMap<String, Requirements>,
    /// This player's state, as last synced.
    pub state: PlayerState,
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
            log::warn!("Skills & Perks got a definitions message it cannot read");
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
            Err(error) => log::warn!("Skills & Perks got definitions it cannot read: {error}"),
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
        Ok(ToClient::Open) => {
            host.mod_screen_open(MOD_ID);
        }
        Ok(ToClient::Refused { .. }) => {
            if let Some(at) = host.client_state().and_then(|state| state.eye_position) {
                host.play_local_sound(REFUSED_SOUND, at, SOUND_VOLUME, SOUND_PITCH, 0);
            }
        }
        Ok(ToClient::You { player }) => with(|cache| cache.me = Some(player)),
        Err(error) => log::warn!("Skills & Perks got a message it cannot read: {error}"),
    }
}

/// Each client tick: settings once, this player's synced state, and one request for the requirements of items
/// tooltips missed or the inventory holds.
pub fn on_tick(host: &mut dyn Host) {
    if !host.window() {
        return;
    }
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
        with(|cache| cache.state = state);
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
    if batch.is_empty() {
        return;
    }
    let message =
        serde_json::to_vec(&ToServer::Need { items: batch }).expect("a request always encodes");
    if let Err(error) = host.net_send_to_server(ACT_CHANNEL, &message) {
        log::debug!("Skills & Perks could not ask for requirements: {error}");
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
    use skills_core::progress::{PlayerState, SkillProgress};
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
            let id = format!("skills_and_perks:skill_{n:04}");
            let skill = SkillDef {
                id: id.clone(),
                priority: 0,
                name: format!(
                    "skills_and_perks.skill.a_long_translation_key_to_fill_the_chunks_{n:04}"
                ),
                icon: "minecraft:stone".into(),
                max_level: None,
                cost: None,
                points_every: None,
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
            .map(|(_, data)| match serde_json::from_slice(data).unwrap() {
                ToServer::Need { items } => items,
                other => panic!("{other:?}"),
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
        let pickaxe = reqs("skills_and_perks:skill_0000", 16);
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
                skill: "skills_and_perks:mining".into(),
                level: 16,
            },
        );

        // Then the low note plays there
        assert_eq!(
            host.sounds,
            vec![(REFUSED_SOUND.to_string(), [3.0, 70.5, -8.0], 1.0, 1.0)]
        );
        assert_eq!(REFUSED_SOUND, "minecraft:block.note_block.bass");
    }

    #[test]
    fn open_opens_the_screen() {
        // Given a client
        let _serial = exclusive();
        let mut host = TestHost::default();

        // When the server asks it to open the skills screen (/skills open)
        send_msg(&mut host, &ToClient::Open);

        // Then the mod's screen opens
        assert_eq!(host.screens_opened, vec!["skills_and_perks"]);
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
            "skills_and_perks:skill_0000".into(),
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
        let root =
            std::env::temp_dir().join(format!("skills_and_perks_test_{}", std::process::id()));
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
