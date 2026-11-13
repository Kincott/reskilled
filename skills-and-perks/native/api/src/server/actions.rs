//! Requests from a player's client (raise, buy, focus, item requirements), checked as the untrusted input they are.

use horizon_client::mod_api::{Host, Text};
use skills_core::locks::{Requirements, TargetKind};
use skills_core::progress::{PlayerState, Request, apply};
use skills_core::wire::{ToClient, ToServer};

use super::data::{current, requirements};
use super::queue::{self, Job};
use super::state::{set_state, state_of};
use super::text::{UNAVAILABLE, refusal, say};
use crate::{ACT_CHANNEL, MAX_MESSAGE_BYTES, MSG_CHANNEL};

/// Item ids answered in one `Reqs` message.
const NEED_BATCH: usize = 64;
/// Item ids answered for one `Need`; the rest of a longer list is ignored.
const NEED_LIMIT: usize = 256;

/// The request in a client's message, if it is on this mod's channel and reads as one.
pub fn decode(channel: &str, data: &[u8]) -> Option<ToServer> {
    if channel != ACT_CHANNEL {
        return None;
    }
    serde_json::from_slice(data).ok()
}

/// Handles a message from the player's client. A refused request is explained on their action bar.
pub fn handle(host: &mut dyn Host, player: u128, message: ToServer) {
    match message {
        ToServer::Act(request) => {
            let Some(xp_levels) = xp_levels(host, player) else {
                log::debug!(
                    "Skills & Perks got a request from {player:032x}, who is not on the server"
                );
                return;
            };
            if let Err(why) = act(host, player, xp_levels, &request) {
                host.action_bar(player, why);
            }
        }
        ToServer::Need { items } => answer_need(host, player, items),
    }
}

/// The player's XP level, from the server's player list.
fn xp_levels(host: &dyn Host, player: u128) -> Option<u32> {
    let listed = host
        .server_players()
        .into_iter()
        .find(|p| p.uuid == player)?;
    Some(u32::try_from(listed.xp_level).unwrap_or(0))
}

/// Grants the request if the rules allow it, taking its XP levels, and returns the new state; otherwise the line
/// saying why not.
pub fn act(
    host: &mut dyn Host,
    player: u128,
    xp_levels: u32,
    request: &Request,
) -> Result<PlayerState, Text> {
    let unavailable = || say(&UNAVAILABLE, &[]);
    let data = current().ok_or_else(unavailable)?;
    let state = state_of(host, player).map_err(|error| {
        log::warn!("Skills & Perks cannot read the state of {player:032x}: {error:?}");
        unavailable()
    })?;
    let outcome = apply(&data.defs, &state, xp_levels, request)
        .map_err(|r| refusal(&data.defs, &r, request))?;
    let cost = i32::try_from(outcome.levels_spent).unwrap_or(i32::MAX);
    if cost > 0 && !host.give_levels(player, -cost) {
        log::warn!("Skills & Perks could not take {cost} levels from {player:032x}");
        return Err(unavailable());
    }
    if let Err(error) = set_state(host, player, outcome.state.clone()) {
        log::warn!(
            "Skills & Perks could not save the state of {player:032x}, levels given back: {error:?}"
        );
        if cost > 0 {
            host.give_levels(player, cost);
        }
        return Err(unavailable());
    }
    queue::queue(Job::SyncAttributes(player));
    Ok(outcome.state)
}

/// Queues the requirements of the items the client asked about, for its tooltips. Only real items are answered, at
/// most 256 a request, in messages of up to 64 that fit Horizon's message size.
pub fn answer_need(host: &dyn Host, player: u128, items: Vec<String>) {
    let answers: Vec<(String, Requirements)> = items
        .into_iter()
        .take(NEED_LIMIT)
        .filter(|id| host.max_stack_size(id).is_some())
        .map(|id| {
            let reqs = requirements(host, TargetKind::Item, &id);
            (id, reqs)
        })
        .collect();
    for batch in answers.chunks(NEED_BATCH) {
        send_reqs(player, batch);
    }
}

/// Sends the batch as one message, or in halves while it is too big for one.
fn send_reqs(player: u128, batch: &[(String, Requirements)]) {
    let message = serde_json::to_vec(&ToClient::Reqs {
        items: batch.to_vec(),
    })
    .expect("requirements always encode");
    if message.len() > MAX_MESSAGE_BYTES && batch.len() > 1 {
        let (first, second) = batch.split_at(batch.len() / 2);
        send_reqs(player, first);
        send_reqs(player, second);
    } else if message.len() > MAX_MESSAGE_BYTES {
        log::warn!(
            "Skills & Perks cannot send the requirements of {}: too big for one message",
            batch[0].0
        );
    } else {
        queue::send(player, MSG_CHANNEL, message);
    }
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::dispatch_net_from_client;
    use serde_json::json;
    use skills_core::defs::Action;
    use skills_core::progress::{Request, level};
    use skills_core::wire::{ToClient, ToServer};

    use super::*;
    use crate::server::data::current;
    use crate::server::data::tests::load;
    use crate::server::queue::{Job, run, take_queued};
    use crate::server::state::state_of;
    use crate::server::test_host::{TestHost, exclusive, mods};
    use crate::{ACT_CHANNEL, MSG_CHANNEL};

    const MINING: &str = "skills_and_perks:mining";
    const PLAYER: u128 = 0xabc;

    fn with_mining(host: &mut TestHost) {
        let mining = json!({"format": 1, "id": MINING, "name": "skills_and_perks.skill.mining", "icon": "minecraft:stone"});
        let lock = json!({"format": 1, "entries": [{"target": {"item": {"id": "minecraft:diamond_pickaxe"}},
            "actions": ["break"], "requires": {MINING: 16}}]});
        load(
            host,
            &[
                ("skills", vec![(MINING, mining)]),
                ("locks", vec![("skills_and_perks:tiers", lock)]),
            ],
        );
        take_queued();
    }

    fn raise() -> Request {
        Request::Raise {
            skill: MINING.into(),
        }
    }

    #[test]
    fn raise_takes_levels_and_updates_state() {
        // Given Mining at 1 and a player with 10 XP levels
        let _serial = exclusive();
        let mut host = TestHost::default();
        with_mining(&mut host);

        // When they ask to raise Mining
        let result = act(&mut host, PLAYER, 10, &raise());

        // Then 2 levels are taken, Mining is at 2, the attributes are synced, and nothing is refused
        let defs = &current().unwrap().defs;
        assert!(result.is_ok());
        assert_eq!(host.levels_given.get(&PLAYER), Some(&-2));
        assert_eq!(
            level(&state_of(&mut host, PLAYER).unwrap(), defs, MINING),
            2
        );
        assert_eq!(take_queued(), vec![Job::SyncAttributes(PLAYER)]);
        assert!(host.action_bars.is_empty());
    }

    #[test]
    fn raise_without_levels_is_refused_and_takes_nothing() {
        // Given a player with 0 XP levels
        let _serial = exclusive();
        let mut host = TestHost::default();
        with_mining(&mut host);

        // When their client spams 50 raise requests
        let results: Vec<_> = (0..50)
            .map(|_| act(&mut host, PLAYER, 0, &raise()))
            .collect();

        // Then every one is refused with the levels needed, no level is taken, and Mining stays at 1
        let defs = &current().unwrap().defs;
        assert!(results.iter().all(Result::is_err));
        assert_eq!(
            results[0].as_ref().unwrap_err().plain(),
            "Needs 2 XP levels."
        );
        assert!(host.levels_given.is_empty());
        assert_eq!(
            level(&state_of(&mut host, PLAYER).unwrap(), defs, MINING),
            1
        );
        assert!(take_queued().is_empty());
    }

    #[test]
    fn malformed_message_is_ignored() {
        // Given garbage, an unknown request, a request on another channel, and a real request
        let _serial = exclusive();
        let mut host = TestHost::default();
        with_mining(&mut host);
        let real = serde_json::to_vec(&ToServer::Act(raise())).unwrap();
        let unknown = br#"{"act": {"teleport": {"to": "spawn"}}}"#;

        // When they are read, and each is also handed to the hook
        let read = [
            decode(ACT_CHANNEL, b"\xff not json"),
            decode(ACT_CHANNEL, unknown),
            decode("othermod:act", &real),
            decode(ACT_CHANNEL, &real),
        ];
        for data in [&b"\xff not json"[..], unknown, &real] {
            dispatch_net_from_client(&mut host, &mods(), PLAYER, ACT_CHANNEL, data);
        }

        // Then only the real request on this mod's channel reads, and nothing panics or is sent
        assert_eq!(read, [None, None, None, Some(ToServer::Act(raise()))]);
        assert!(host.sent.is_empty() && host.action_bars.is_empty());
    }

    #[test]
    fn need_answers_in_batches_of_64() {
        // Given a lock on the diamond pickaxe, 150 known items asked for, and 5 ids that are not items
        let _serial = exclusive();
        let mut host = TestHost::default();
        with_mining(&mut host);
        let mut asked: Vec<String> = (0..149).map(|n| format!("minecraft:item_{n}")).collect();
        asked.push("minecraft:diamond_pickaxe".into());
        host.items = asked.clone();
        asked.extend((0..5).map(|n| format!("junk:not_an_item_{n}")));

        // When the client asks for their requirements and the tick sends the answers
        answer_need(&host, PLAYER, asked);
        run(&mut host, take_queued());

        // Then 150 answers come in messages of 64, 64 and 22, unknown ids are left out, and the pickaxe is locked
        let answers: Vec<Vec<(String, Requirements)>> = host
            .sent_to(PLAYER, MSG_CHANNEL)
            .iter()
            .map(|m| match serde_json::from_slice(m).unwrap() {
                ToClient::Reqs { items } => items,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            answers.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![64, 64, 22]
        );
        let all: Vec<&(String, Requirements)> = answers.iter().flatten().collect();
        assert!(all.iter().all(|(id, _)| id.starts_with("minecraft:")));
        let pickaxe = &all
            .iter()
            .find(|(id, _)| id == "minecraft:diamond_pickaxe")
            .unwrap()
            .1;
        assert_eq!(pickaxe.by_action[&Action::Break][MINING], 16);
    }

    #[test]
    fn need_answers_at_most_256_ids() {
        // Given a client asking for 1,000 known items at once
        let _serial = exclusive();
        let mut host = TestHost::default();
        with_mining(&mut host);
        host.items = (0..1000).map(|n| format!("minecraft:item_{n}")).collect();

        // When the request is answered
        answer_need(&host, PLAYER, host.items.clone());
        run(&mut host, take_queued());

        // Then only the first 256 are answered
        let answered: usize = host
            .sent_to(PLAYER, MSG_CHANNEL)
            .iter()
            .map(|m| match serde_json::from_slice(m).unwrap() {
                ToClient::Reqs { items } => items.len(),
                other => panic!("{other:?}"),
            })
            .sum();
        assert_eq!(answered, 256);
    }

    #[test]
    fn oversized_answers_are_split_to_fit_a_message() {
        // Given 40 skills with long ids, all needed for every item, and a client asking about 64 items
        let _serial = exclusive();
        let mut host = TestHost::default();
        let ids: Vec<String> = (0..40)
            .map(|n| format!("skills_and_perks:a_rather_long_skill_name_for_testing_{n:02}"))
            .collect();
        let skills: Vec<(&str, serde_json::Value)> = ids
            .iter()
            .map(|id| {
                (
                    id.as_str(),
                    json!({"format": 1, "id": id, "name": "n", "icon": "minecraft:stone"}),
                )
            })
            .collect();
        let requires: serde_json::Map<String, serde_json::Value> =
            ids.iter().map(|id| (id.clone(), json!(5))).collect();
        let lock = json!({"format": 1, "entries": [{"target": {"item": {"pattern": "minecraft:*"}},
            "actions": ["use", "attack", "break", "equip"], "requires": requires}]});
        load(
            &mut host,
            &[
                ("skills", skills),
                ("locks", vec![("skills_and_perks:all", lock)]),
            ],
        );
        take_queued();
        host.items = (0..64).map(|n| format!("minecraft:item_{n}")).collect();

        // When the request is answered
        answer_need(&host, PLAYER, host.items.clone());
        run(&mut host, take_queued());

        // Then every item is answered, in several messages that each fit Horizon's limit
        let messages = host.sent_to(PLAYER, MSG_CHANNEL);
        let answered: usize = messages
            .iter()
            .map(|m| match serde_json::from_slice(m).unwrap() {
                ToClient::Reqs { items } => items.len(),
                other => panic!("{other:?}"),
            })
            .sum();
        assert_eq!(answered, 64);
        assert!(messages.len() > 2, "{}", messages.len());
        assert!(messages.iter().all(|m| m.len() <= crate::MAX_MESSAGE_BYTES));
    }
}
