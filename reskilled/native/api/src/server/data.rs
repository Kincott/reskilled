//! The data files: Horizon's reload listeners hand them over, and every data load builds the effective definitions.
//! Lock requirements are worked out per id on first use, with the server's tags and the items' repair materials.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use horizon_client::mod_api::{Host, ItemStack, NativeReloadListener};
use serde_json::{Map, Value};
use skills_core::defs::DefKind;
use skills_core::locks::{Facts, LockCache, Requirements, TargetKind};
use skills_core::registry::{Defs, SourceFile, build};
use skills_core::wire::{ToClient, chunk_defs, client_defs_json};

use super::lock;
use super::queue;
use super::state::online_players;
use crate::{DEFS_CHANNEL, MAX_MESSAGE_BYTES, MSG_CHANNEL};

const NAMESPACE: &str = "reskilled";
/// The data folders under `data/<namespace>/reskilled/`, in the order their files are merged.
const FOLDERS: [(&str, DefKind); 5] = [
    ("skills", DefKind::Skill),
    ("perks", DefKind::Perk),
    ("focus", DefKind::Focus),
    ("locks", DefKind::Lock),
    ("settings", DefKind::Settings),
];
const REPAIRABLE: &str = "minecraft:repairable";
const REPAIRABLE_ITEMS: &str = "items";

/// What one data load produced.
pub struct ServerData {
    pub defs: Arc<Defs>,
    /// Counts data loads; clients keep the definitions of the newest.
    pub generation: u64,
    pub locks: Mutex<LockCache>,
    /// The definitions as clients get them, worked out once per load.
    pub client_defs: String,
}

static SERVER: RwLock<Option<Arc<ServerData>>> = RwLock::new(None);
/// The files this data load's listeners handed over, per folder, until the load is done.
static PENDING: Mutex<BTreeMap<&'static str, Vec<(String, Value)>>> = Mutex::new(BTreeMap::new());
/// Item id to the `items` of its default `minecraft:repairable` component (a `#tag` or an item id).
static REPAIR_MATERIALS: RwLock<BTreeMap<String, String>> = RwLock::new(BTreeMap::new());

/// The newest data load, if the server has loaded its data yet.
pub fn current() -> Option<Arc<ServerData>> {
    SERVER
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn listener_id(folder: &str) -> String {
    format!("{NAMESPACE}:{folder}")
}

/// One JSON listener per data folder.
pub fn add_listeners(listeners: &mut Vec<NativeReloadListener>) {
    for (folder, _) in FOLDERS {
        listeners.push(NativeReloadListener::json(
            &listener_id(folder),
            &format!("{NAMESPACE}/{folder}"),
        ));
    }
}

/// Keeps a listener's files until the load is done. Every mod hears every listener, so others' are ignored.
pub fn collect(listener: &NativeReloadListener, files: &[(String, Value)]) {
    if let Some((folder, _)) = FOLDERS
        .iter()
        .find(|(folder, _)| listener.id == listener_id(folder))
    {
        lock(&PENDING).insert(folder, files.to_vec());
    }
}

/// Builds the definitions from the files collected for this load, logs each skipped file, and refreshes every
/// online player.
pub fn load() {
    let pending = std::mem::take(&mut *lock(&PENDING));
    let mut sources = Vec::new();
    for (folder, kind) in FOLDERS {
        for (id, json) in pending.get(folder).into_iter().flatten() {
            let (namespace, path) = id.split_once(':').unwrap_or((NAMESPACE, id));
            let path = format!("{namespace}:{NAMESPACE}/{folder}/{path}");
            sources.push(SourceFile {
                path,
                order: sources.len(),
                kind,
                json: json.to_string(),
            });
        }
    }
    let (defs, errors) = build(sources);
    for error in &errors {
        log::warn!("Reskilled skipped {error}");
    }
    log::info!(
        "Reskilled loaded {} skills, {} perks, {} focus areas and {} lock entries",
        defs.skills.len(),
        defs.perks.len(),
        defs.focus.len(),
        defs.locks.len()
    );
    let generation = current().map_or(0, |data| data.generation) + 1;
    let client_defs = client_defs_json(&defs);
    let data = ServerData {
        defs: Arc::new(defs),
        generation,
        locks: Mutex::new(LockCache::default()),
        client_defs,
    };
    *SERVER.write().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(data));
    for player in online_players() {
        queue::refresh(player);
    }
}

/// Queues the current definitions for the player, replacing any older send still waiting.
pub fn send_defs(player: u128) {
    let Some(data) = current() else {
        log::debug!("Reskilled has no definitions to send yet");
        return;
    };
    queue::drop_waiting(player, DEFS_CHANNEL);
    let you = serde_json::to_vec(&ToClient::You { player }).expect("You always encodes");
    queue::send(player, MSG_CHANNEL, you);
    // Send ids only need to grow within one server run; 2^32 data loads never happen.
    let send_id = data.generation as u32;
    for chunk in chunk_defs(&data.client_defs, send_id, MAX_MESSAGE_BYTES) {
        let message = serde_json::to_vec(&chunk).expect("a definitions chunk always encodes");
        queue::send(player, DEFS_CHANNEL, message);
    }
}

/// The target's lock requirements, through this load's cache; nothing is locked before the first load.
pub fn requirements(host: &dyn Host, kind: TargetKind, id: &str) -> Requirements {
    let Some(data) = current() else {
        return Requirements::default();
    };
    lock(&data.locks)
        .get(&data.defs, kind, id, &HostFacts(host))
        .clone()
}

/// Records the item's repair material from its default components. Only reads them: the event lets a mod change
/// defaults, and this mod changes none.
pub fn note_repair_material(item: &str, components: &Map<String, Value>) {
    let material = match components
        .get(REPAIRABLE)
        .and_then(|r| r.get(REPAIRABLE_ITEMS))
    {
        Some(Value::String(material)) => Some(material.clone()),
        Some(Value::Array(materials)) if materials.len() == 1 => {
            materials[0].as_str().map(str::to_string)
        }
        _ => None,
    };
    let mut known = REPAIR_MATERIALS
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    match material {
        Some(material) => known.insert(item.to_string(), material),
        None => known.remove(item),
    };
}

/// The server's answers to what lock matchers ask.
pub struct HostFacts<'a>(pub &'a dyn Host);

fn single(item: &str) -> ItemStack {
    ItemStack {
        item: item.into(),
        count: 1,
        damage: 0,
        extra: None,
    }
}

impl Facts for HostFacts<'_> {
    fn item_in_tag(&self, item: &str, tag: &str) -> bool {
        self.0
            .server_item_in_tag(&single(item), tag)
            .unwrap_or_else(|error| {
                log::warn!("Reskilled could not test {item} against #{tag}: {error:?}");
                false
            })
    }

    fn block_in_tag(&self, block: &str, tag: &str) -> bool {
        self.0
            .block_name_in_tag(block, tag)
            .unwrap_or_else(|error| {
                log::warn!("Reskilled could not test {block} against #{tag}: {error:?}");
                false
            })
    }

    fn item_repairable(&self, item: &str) -> Option<String> {
        REPAIR_MATERIALS
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(item)
            .cloned()
    }
}

#[cfg(test)]
pub(crate) fn reset() {
    *SERVER.write().unwrap_or_else(PoisonError::into_inner) = None;
    lock(&PENDING).clear();
    REPAIR_MATERIALS
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
}

#[cfg(test)]
pub(crate) mod tests {
    use horizon_client::mod_api::{
        dispatch_add_server_reload_listeners, dispatch_modify_default_components,
        dispatch_server_data_loaded, dispatch_server_reload_listener_apply,
    };
    use serde_json::{Value, json};
    use skills_core::defs::Action;

    use super::*;
    use crate::server::queue::{Job, take_queued};
    use crate::test_host::{TestHost, exclusive, logged, mods};

    const MINING: &str = "reskilled:mining";

    fn skill(id: &str) -> Value {
        json!({"format": 1, "id": id, "name": "n", "icon": "minecraft:stone"})
    }

    /// One data load as the game runs it: listeners listed, each handed its folder's files, then the load is done.
    pub fn load(host: &mut TestHost, folders: &[(&str, Vec<(&str, Value)>)]) {
        load_with(host, folders, |_| {});
    }

    /// A data load where `between` runs after this mod's listeners got their files and before the load is done.
    fn load_with(
        host: &mut TestHost,
        folders: &[(&str, Vec<(&str, Value)>)],
        between: impl FnOnce(&mut TestHost),
    ) {
        let mods = mods();
        let mut listeners = Vec::new();
        dispatch_add_server_reload_listeners(host, &mods, &mut listeners);
        for listener in &listeners {
            let files: Vec<(String, Value)> = folders
                .iter()
                .filter(|(folder, _)| listener.folder == format!("reskilled/{folder}"))
                .flat_map(|(_, files)| {
                    files
                        .iter()
                        .map(|(id, json)| (id.to_string(), json.clone()))
                })
                .collect();
            dispatch_server_reload_listener_apply(host, &mods, listener, &files);
        }
        between(host);
        dispatch_server_data_loaded(host, &mods, true);
    }

    #[test]
    fn listens_to_the_five_data_folders() {
        // Given the mod
        let _serial = exclusive();
        let mut listeners = Vec::new();

        // When the game asks for reload listeners
        dispatch_add_server_reload_listeners(&mut TestHost::default(), &mods(), &mut listeners);

        // Then there is one per data folder, under the mod's namespace
        let listed: Vec<(&str, &str)> = listeners
            .iter()
            .map(|l| (l.id.as_str(), l.folder.as_str()))
            .collect();
        assert_eq!(
            listed,
            vec![
                ("reskilled:skills", "reskilled/skills"),
                ("reskilled:perks", "reskilled/perks"),
                ("reskilled:focus", "reskilled/focus"),
                ("reskilled:locks", "reskilled/locks"),
                ("reskilled:settings", "reskilled/settings"),
            ]
        );
    }

    #[test]
    fn reload_builds_defs_and_logs_bad_files() {
        // Given a good skill file, a broken one, and another mod's listener handing over files for the same folder
        // after this mod's did
        let _serial = exclusive();
        let mut host = TestHost::default();
        let foreign = horizon_client::mod_api::NativeReloadListener::json(
            "othermod:things",
            "reskilled/skills",
        );
        let sneaky = [("othermod:sneaky".to_string(), skill("othermod:sneaky"))];

        // When the data loads twice
        load_with(
            &mut host,
            &[(
                "skills",
                vec![
                    (MINING, skill(MINING)),
                    ("reskilled:broken", json!({"format": 2})),
                ],
            )],
            |host| dispatch_server_reload_listener_apply(host, &mods(), &foreign, &sneaky),
        );
        let first = current().unwrap().generation;
        load(&mut host, &[("skills", vec![(MINING, skill(MINING))])]);

        // Then only our listener's good file is loaded, the broken one is logged with its path, and each load counts
        let data = current().unwrap();
        assert_eq!(data.defs.skills.keys().collect::<Vec<_>>(), vec![MINING]);
        assert_eq!((first, data.generation), (1, 2));
        let warnings: Vec<String> = logged()
            .into_iter()
            .filter(|l| l.starts_with("WARN"))
            .collect();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("reskilled:reskilled/skills/broken"),
            "{}",
            warnings[0]
        );
    }

    #[test]
    fn reload_queues_jobs_for_online_players() {
        // Given two players online
        let _serial = exclusive();
        let mut host = TestHost::default();
        for (player, name) in [(1, "Ann"), (2, "Bo")] {
            horizon_client::mod_api::dispatch_player_logged_in(&mut host, &mods(), player, name);
        }
        take_queued();

        // When the data reloads
        load(&mut host, &[("skills", vec![(MINING, skill(MINING))])]);

        // Then each gets the definitions, the rules, an attribute sync and an equipment check
        assert_eq!(
            take_queued(),
            vec![
                Job::SendDefs(1),
                Job::SendRules(1),
                Job::LiftShared(1),
                Job::SyncAttributes(1),
                Job::CheckEquipment(1),
                Job::SendDefs(2),
                Job::SendRules(2),
                Job::LiftShared(2),
                Job::SyncAttributes(2),
                Job::CheckEquipment(2),
            ]
        );
    }

    #[test]
    fn host_facts_read_tags_and_repairable() {
        // Given a server where the diamond pickaxe is a pickaxe and stone is mineable, and the game's item defaults
        // (read as it builds its item table) say the pickaxe is repaired with diamonds and the stick with nothing
        let _serial = exclusive();
        let host = TestHost {
            item_tags: vec![(
                "minecraft:diamond_pickaxe".into(),
                "minecraft:pickaxes".into(),
            )],
            block_tags: vec![(
                "minecraft:stone".into(),
                "minecraft:mineable/pickaxe".into(),
            )],
            ..TestHost::default()
        };
        let mut pickaxe = json!({"minecraft:max_damage": 1561,
            "minecraft:repairable": {"items": "#minecraft:diamond_tool_materials"}});
        let untouched = pickaxe.clone();
        let mut stick = json!({"minecraft:max_stack_size": 64});
        dispatch_modify_default_components(
            &mods(),
            "minecraft:diamond_pickaxe",
            pickaxe.as_object_mut().unwrap(),
        );
        dispatch_modify_default_components(
            &mods(),
            "minecraft:stick",
            stick.as_object_mut().unwrap(),
        );
        let facts = HostFacts(&host);

        // When the facts are asked
        let answers = (
            facts.item_in_tag("minecraft:diamond_pickaxe", "minecraft:pickaxes"),
            facts.item_in_tag("minecraft:diamond_sword", "minecraft:pickaxes"),
            facts.block_in_tag("minecraft:stone", "minecraft:mineable/pickaxe"),
            facts.item_repairable("minecraft:diamond_pickaxe"),
            facts.item_repairable("minecraft:stick"),
        );

        // Then they come from the server's tags and the item's default component, which is only read, never changed
        assert_eq!(
            answers,
            (
                true,
                false,
                true,
                Some("#minecraft:diamond_tool_materials".to_string()),
                None
            )
        );
        assert_eq!(pickaxe, untouched);
    }

    #[test]
    fn requirements_are_empty_before_the_first_load_then_cached() {
        // Given no data yet, then a lock on the mace
        let _serial = exclusive();
        let mut host = TestHost::default();
        let before = requirements(&host, TargetKind::Item, "minecraft:mace");
        let lock = json!({"format": 1, "entries": [{"target": {"item": {"id": "minecraft:mace"}},
            "actions": ["attack"], "requires": {"reskilled:mining": 20}}]});
        load(
            &mut host,
            &[
                ("skills", vec![(MINING, skill(MINING))]),
                ("locks", vec![("reskilled:specials", lock)]),
            ],
        );

        // When the mace's requirements are read, twice
        let after = requirements(&host, TargetKind::Item, "minecraft:mace");
        let again = requirements(&host, TargetKind::Item, "minecraft:mace");

        // Then nothing is locked before the load, and the lock applies after it
        assert_eq!(before, Requirements::default());
        assert_eq!(after.by_action[&Action::Attack][MINING], 20);
        assert_eq!(again, after);
    }
}
