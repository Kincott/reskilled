//! Turning a guide scene's block state strings into the states Horizon draws.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use horizon_client::mod_api::{BlockStateId, Host};
use skills_core::defs::parse_state;
use skills_core::guide::Frame;

use crate::server::lock;

/// What was resolved so far, and the sprite version it was resolved under.
struct Resolved {
    version: u64,
    states: BTreeMap<String, Option<BlockStateId>>,
}

static RESOLVED: Mutex<Resolved> = Mutex::new(Resolved {
    version: 0,
    states: BTreeMap::new(),
});

/// The state strings already warned about.
static WARNED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// The state `state` names, among `candidates` (all the states of its block, the first being the one to take when
/// `state` gives no properties): the first whose properties match every one given. `None` when `state` is not a
/// state string, its block has no states, or no state matches.
pub fn resolve(
    state: &str,
    candidates: impl Fn(&str) -> Vec<BlockStateId>,
    property: impl Fn(BlockStateId, &str) -> Option<String>,
) -> Option<BlockStateId> {
    let (block, properties) = parse_state(state)?;
    candidates(&block).into_iter().find(|candidate| {
        properties
            .iter()
            .all(|(key, value)| property(*candidate, key).as_deref() == Some(value.as_str()))
    })
}

/// The states of `frame`'s blocks, by position. A block that names no state is left out, with one warning for its
/// state string.
pub fn scene_blocks(host: &dyn Host, frame: &Frame) -> Vec<([i32; 3], BlockStateId)> {
    let version = host.block_sprites_version();
    {
        let mut resolved = lock(&RESOLVED);
        if resolved.version != version {
            resolved.version = version;
            resolved.states.clear();
        }
    }
    let mut blocks = Vec::new();
    for (position, state) in &frame.blocks {
        let cached = lock(&RESOLVED).states.get(state).copied();
        let id = match cached {
            Some(id) => id,
            None => {
                let id = match parse_state(state) {
                    // A bare block is its default state: no need to search every state.
                    Some((block, properties)) if properties.is_empty() => {
                        host.default_state(&block)
                    }
                    _ => resolve(
                        state,
                        |block| block_candidates(host, block),
                        |id, key| host.block_state_property(id, key).map(str::to_owned),
                    ),
                };
                if id.is_none() && lock(&WARNED).insert(state.clone()) {
                    log::warn!("a perk guide shows {state}, which names no block state");
                }
                lock(&RESOLVED).states.insert(state.clone(), id);
                id
            }
        };
        if let Some(id) = id {
            blocks.push((*position, id));
        }
    }
    blocks
}

/// Takes `id` as what `state` names, as if it had been resolved: for tests whose host has no table of block states.
#[cfg(test)]
pub fn remember(state: &str, id: BlockStateId) {
    lock(&RESOLVED).states.insert(state.to_string(), Some(id));
}

/// Every state of `block`: its default state first, then the rest by id.
fn block_candidates(host: &dyn Host, block: &str) -> Vec<BlockStateId> {
    let default = host.default_state(block);
    let mut candidates: Vec<BlockStateId> = default.into_iter().collect();
    candidates.extend(
        host.block_states()
            .iter()
            .filter(|(id, facts)| &*facts.name == block && Some(*id) != default)
            .map(|(id, _)| id),
    );
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheat(block: &str) -> Vec<BlockStateId> {
        if block == "minecraft:wheat" {
            vec![BlockStateId(10), BlockStateId(11), BlockStateId(12)]
        } else {
            Vec::new()
        }
    }

    fn age(state: BlockStateId, key: &str) -> Option<String> {
        match (state.0, key) {
            (10, "age") => Some("0".into()),
            (11, "age") => Some("1".into()),
            (12, "age") => Some("7".into()),
            _ => None,
        }
    }

    #[test]
    fn resolve_picks_the_state_with_matching_properties() {
        // Given three wheat states with age 0, 1 and 7
        // When a state string gives age 7, and another gives no properties
        let with_age = resolve("minecraft:wheat[age=7]", wheat, age);
        let bare = resolve("minecraft:wheat", wheat, age);
        // Then the first is the age 7 state and the second is the first candidate
        assert_eq!(with_age, Some(BlockStateId(12)));
        assert_eq!(bare, Some(BlockStateId(10)));
    }

    #[test]
    fn an_unmatched_block_is_left_out() {
        // Given three wheat states with age 0, 1 and 7
        // When a state string gives an age none has
        let none = resolve("minecraft:wheat[age=9]", wheat, age);
        // Then nothing resolves
        assert_eq!(none, None);
    }

    #[test]
    fn an_unknown_block_or_a_bad_string_resolves_to_nothing() {
        // Given a block with no states and a malformed state string
        // When each is resolved
        // Then neither resolves
        assert_eq!(resolve("minecraft:nothing", wheat, age), None);
        assert_eq!(resolve("minecraft:wheat[age=", wheat, age), None);
    }

    #[test]
    fn every_given_property_must_match() {
        // Given states where only the age matches
        // When the state string also gives a property no state has
        let state = resolve("minecraft:wheat[age=7,wet=true]", wheat, age);
        // Then nothing resolves
        assert_eq!(state, None);
    }
}
