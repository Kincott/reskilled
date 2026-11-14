//! Requests from a player's client (raise, buy, focus, item requirements), checked as the untrusted input they are.

use std::sync::Mutex;

use horizon_client::mod_api::{Host, Text};
use skills_core::locks::{Requirements, TargetKind};
use skills_core::progress::{PlayerState, Request, apply_under, level, levels_for_points};
use skills_core::registry::Defs;
use skills_core::wire::{ToClient, ToServer};

use super::data::{current, requirements};
use super::lock;
use super::queue::{self, Job};
use super::state::{self, set_state, state_of};
use super::uuid_text;
use crate::text::{
    FOCUS_ADDED, PERK_BOUGHT, PERK_REFUNDED, PERK_SWITCHED, RAISED, UNAVAILABLE, focus_name,
    number, perk_name, refusal, say, skill_name,
};
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
    state::noted_mod(player);
    match message {
        ToServer::Act(request) => {
            let Some(xp_levels) = xp_levels(host, player) else {
                log::debug!("Reskilled got a request from {player:032x}, who is not on the server");
                return;
            };
            defer(player, xp_levels, request, Reply::ActionBar);
        }
        ToServer::Need { items } => answer_need(host, player, items),
        ToServer::Hello => {}
        ToServer::SetRules(rules) => super::rules::request(player, rules),
    }
}

/// Where the outcome of a deferred request is said: the screen's refusals on the action bar (its successes show
/// on the screen), a command's outcome in chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    ActionBar,
    Chat,
}

/// Requests waiting for the server tick: who asked, their XP level then, what, and where to answer.
static PENDING: Mutex<Vec<(u128, u32, Request, Reply)>> = Mutex::new(Vec::new());

/// Carries the request out in the player's next own tick (`player_tick`), where Horizon reads that player:
/// `give_levels` reaches them there. Elsewhere it does not reach a single-player world's owner, whose id Horizon
/// takes for the server thread's own (absent) player, and `/xp` from the console fails the same way.
pub fn defer(player: u128, xp_levels: u32, request: Request, reply: Reply) {
    lock(&PENDING).push((player, xp_levels, request, reply));
}

/// The player's own tick: their waiting requests, in order, judged against their XP level now (the one they had
/// when asking if the server no longer lists them).
pub fn run_pending_for(host: &mut dyn Host, player: u128) {
    let pending: Vec<(u128, u32, Request, Reply)> = {
        let mut all = lock(&PENDING);
        let (theirs, others) = std::mem::take(&mut *all)
            .into_iter()
            .partition(|(p, ..)| *p == player);
        *all = others;
        theirs
    };
    for (player, asked_with, request, reply) in pending {
        let xp_levels = xp_levels(host, player).unwrap_or(asked_with);
        let outcome = act(host, player, xp_levels, &request);
        match (outcome, reply) {
            (Ok(state), Reply::Chat) => {
                if let Some(line) = current().map(|data| done_line(&data.defs, &request, &state)) {
                    host.tell(player, line);
                }
            }
            (Ok(_), Reply::ActionBar) => {}
            (Err(why), Reply::ActionBar) => {
                host.action_bar(player, why);
            }
            (Err(why), Reply::Chat) => {
                host.tell(player, why);
            }
        }
    }
}

/// What a granted request did, in words.
fn done_line(defs: &Defs, request: &Request, state: &PlayerState) -> Text {
    match request {
        Request::Raise { skill, .. } => say(
            &RAISED,
            &[skill_name(defs, skill), number(level(state, defs, skill))],
        ),
        Request::BuyPerk { perk } => say(&PERK_BOUGHT, &[perk_name(defs, perk)]),
        Request::SwitchPerk { perk } => say(&PERK_SWITCHED, &[perk_name(defs, perk)]),
        Request::RefundPerk { perk } => say(&PERK_REFUNDED, &[perk_name(defs, perk)]),
        Request::AddFocus { focus } => say(&FOCUS_ADDED, &[focus_name(defs, focus)]),
    }
}

/// A player left: their waiting requests are dropped.
pub fn forget(player: u128) {
    lock(&PENDING).retain(|(p, ..)| *p != player);
}

#[cfg(test)]
pub(crate) fn reset() {
    lock(&PENDING).clear();
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
        log::warn!("Reskilled cannot read the state of {player:032x}: {error:?}");
        unavailable()
    })?;
    let rules = super::rules::current(host);
    let outcome = apply_under(&data.defs, &rules, &state, xp_levels, request)
        .map_err(|r| refusal(&data.defs, &r, request))?;
    let (taken, given_back) = if outcome.points_spent > 0 {
        levels_for_points(xp_levels, outcome.points_spent).ok_or_else(unavailable)?
    } else {
        (outcome.levels_spent, 0)
    };
    let cost = i32::try_from(taken).unwrap_or(i32::MAX);
    if cost > 0 && !change_levels(host, player, -cost) {
        log::warn!("Reskilled could not take {cost} levels from {player:032x}");
        return Err(unavailable());
    }
    if let Err(error) = set_state(host, player, outcome.state.clone()) {
        log::warn!(
            "Reskilled could not save the state of {player:032x}, levels given back: {error:?}"
        );
        if cost > 0 {
            change_levels(host, player, cost);
        }
        return Err(unavailable());
    }
    super::sharing::record(host, player, &outcome.state);
    if given_back > 0 && !host.give_experience(player, given_back) {
        log::warn!("Reskilled could not give {player:032x} back {given_back} XP points of a raise");
    }
    queue::queue(Job::SyncAttributes(player));
    Ok(outcome.state)
}

/// Gives (or, negative, takes) XP levels. `give_levels` cannot reach a single-player world's owner outside their
/// own hooks (`native_as_player` takes the level's own player for them, and the server thread has none), so vanilla's
/// `/xp` does it then; that in turn fails inside a player's own hook, where `give_levels` works.
fn change_levels(host: &mut dyn Host, player: u128, levels: i32) -> bool {
    if host.give_levels(player, levels) {
        return true;
    }
    let targets = [Some(uuid_text(player)), state::name_of(player)];
    let mut replies = Vec::new();
    for target in targets.into_iter().flatten() {
        let line = format!("xp add {target} {levels} levels");
        let answer = host.run_command_captured(&line);
        if answer.ran && answer.result > 0 {
            return true;
        }
        let said: Vec<String> = answer.output.iter().map(Text::plain).collect();
        replies.push(format!(
            "`{line}`: ran {}, result {}, said {said:?}",
            answer.ran, answer.result
        ));
    }
    log::warn!(
        "Reskilled could not change the levels of {player:032x} by {levels} (window game: {}): \
         give_levels refused; {}",
        host.window(),
        replies.join("; ")
    );
    false
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
            "Reskilled cannot send the requirements of {}: too big for one message",
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
    use crate::test_host::{TestHost, exclusive, mods};
    use crate::{ACT_CHANNEL, MSG_CHANNEL};

    const MINING: &str = "reskilled:mining";
    const PLAYER: u128 = 0xabc;

    fn with_mining(host: &mut TestHost) {
        let mining = json!({"format": 1, "id": MINING, "name": "reskilled.skill.mining", "icon": "minecraft:stone"});
        let lock = json!({"format": 1, "entries": [{"target": {"item": {"id": "minecraft:diamond_pickaxe"}},
            "actions": ["break"], "requires": {MINING: 16}}]});
        load(
            host,
            &[
                ("skills", vec![(MINING, mining)]),
                ("locks", vec![("reskilled:tiers", lock)]),
            ],
        );
        take_queued();
    }

    fn raise() -> Request {
        Request::Raise {
            skill: MINING.into(),
            times: 1,
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
        assert_eq!(host.levels_given.get(&PLAYER), Some(&-1));
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
            "Needs 1 XP levels."
        );
        assert!(host.levels_given.is_empty());
        assert_eq!(
            level(&state_of(&mut host, PLAYER).unwrap(), defs, MINING),
            1
        );
        assert!(take_queued().is_empty());
    }

    #[test]
    fn a_request_waits_for_the_server_tick() {
        // Given a player with 10 XP levels whose client asks to raise Mining, which taking levels cannot do yet
        let _serial = exclusive();
        let mut host = TestHost::default();
        with_mining(&mut host);

        // When the request arrives, then the tick runs
        defer(PLAYER, 10, raise(), Reply::ActionBar);
        let before = (host.levels_given.clone(), host.commands.clone());
        run_pending_for(&mut host, PLAYER);

        // Then nothing is taken on arrival; on the tick the levels are taken and Mining goes up, once
        let defs = &current().unwrap().defs;
        assert!(before.0.is_empty() && before.1.is_empty());
        assert_eq!(host.levels_given.get(&PLAYER), Some(&-1));
        assert_eq!(
            level(&state_of(&mut host, PLAYER).unwrap(), defs, MINING),
            2
        );
        run_pending_for(&mut host, PLAYER);
        assert_eq!(host.levels_given.get(&PLAYER), Some(&-1));
    }

    #[test]
    fn each_players_requests_wait_for_their_own_tick() {
        // Given requests from two players
        let _serial = exclusive();
        let mut host = TestHost::default();
        with_mining(&mut host);
        const OTHER: u128 = 0xdef;
        defer(OTHER, 10, raise(), Reply::ActionBar);
        defer(PLAYER, 10, raise(), Reply::ActionBar);

        // When the first player's own tick runs
        run_pending_for(&mut host, PLAYER);

        // Then only their request is carried out; the other waits for its player's tick
        assert_eq!(host.levels_given.get(&PLAYER), Some(&-1));
        assert_eq!(host.levels_given.get(&OTHER), None);
        run_pending_for(&mut host, OTHER);
        assert_eq!(host.levels_given.get(&OTHER), Some(&-1));
    }

    #[test]
    fn a_deferred_request_answers_where_it_was_asked() {
        // Given a player with 2 XP levels who asks from the screen and, twice, by command
        let _serial = exclusive();
        let mut host = TestHost::default();
        with_mining(&mut host);
        defer(PLAYER, 0, raise(), Reply::ActionBar);
        defer(PLAYER, 2, raise(), Reply::Chat);
        defer(PLAYER, 0, raise(), Reply::Chat);

        // When the tick runs
        run_pending_for(&mut host, PLAYER);

        // Then the screen's refusal goes to the action bar, and the commands' outcome to chat
        assert_eq!(host.action_bars_of(PLAYER), vec!["Needs 1 XP levels."]);
        assert_eq!(
            host.told_to(PLAYER),
            vec!["Mining is now level 2.", "Needs 2 XP levels."]
        );
    }

    #[test]
    fn under_the_points_rule_a_raise_takes_whole_levels_and_gives_back_the_rest() {
        // Given raises paid in XP points (20 a level of cost), Mining at 1, and a player at level 10, the top level
        // of which holds 25 points
        let _serial = exclusive();
        let mut host = TestHost::default();
        with_mining(&mut host);
        crate::server::rules::force(skills_core::rules::Rules {
            cost_unit: skills_core::rules::CostUnit::Points,
            ..Default::default()
        });

        // When they raise Mining
        let result = act(&mut host, PLAYER, 10, &raise());

        // Then one level is taken and the 5 points over the price come back
        assert!(result.is_ok());
        assert_eq!(host.levels_given.get(&PLAYER), Some(&-1));
        assert_eq!(host.experience_given.get(&PLAYER), Some(&5));
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
            .map(|n| format!("reskilled:a_rather_long_skill_name_for_testing_{n:02}"))
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
            &[("skills", skills), ("locks", vec![("reskilled:all", lock)])],
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

    #[test]
    fn raise_takes_levels_by_command_when_give_levels_is_refused() {
        // Given a single-player world's owner, whom `give_levels` cannot reach from a client request, with 10 levels
        let _serial = exclusive();
        let mut host = TestHost {
            give_levels_fails: true,
            ..TestHost::default()
        };
        with_mining(&mut host);

        // When they ask to raise Mining
        let result = act(&mut host, PLAYER, 10, &raise());

        // Then the levels are taken with vanilla's /xp, and Mining goes up
        let defs = &current().unwrap().defs;
        assert!(result.is_ok());
        assert_eq!(
            host.commands,
            vec!["xp add 00000000-0000-0000-0000-000000000abc -1 levels"]
        );
        assert_eq!(
            level(&state_of(&mut host, PLAYER).unwrap(), defs, MINING),
            2
        );
    }

    #[test]
    fn raise_names_the_player_when_their_uuid_does_not_reach_them() {
        // Given a player named Ann whom neither `give_levels` nor an /xp naming her uuid reaches, with 10 levels
        let _serial = exclusive();
        let mut host = TestHost {
            give_levels_fails: true,
            refused_commands: vec!["xp add 00000000-0000-0000-0000-000000000abc".into()],
            ..TestHost::default()
        };
        with_mining(&mut host);
        crate::server::state::logged_in(PLAYER, "Ann");

        // When she asks to raise Mining
        let result = act(&mut host, PLAYER, 10, &raise());

        // Then /xp is tried by uuid, then by her name, which takes the levels
        assert!(result.is_ok());
        assert_eq!(
            host.commands,
            vec![
                "xp add 00000000-0000-0000-0000-000000000abc -1 levels",
                "xp add Ann -1 levels"
            ]
        );
    }

    #[test]
    fn raise_takes_nothing_when_no_way_to_take_levels_works() {
        // Given a player whom neither `give_levels` nor a command can reach
        let _serial = exclusive();
        let mut host = TestHost {
            give_levels_fails: true,
            commands_fail: true,
            ..TestHost::default()
        };
        with_mining(&mut host);

        // When they ask to raise Mining
        let result = act(&mut host, PLAYER, 10, &raise());

        // Then it is refused as unavailable and Mining stays at 1
        let defs = &current().unwrap().defs;
        assert_eq!(
            result.unwrap_err().plain(),
            "That cannot be done right now."
        );
        assert_eq!(
            level(&state_of(&mut host, PLAYER).unwrap(), defs, MINING),
            1
        );
        assert!(take_queued().is_empty());
    }
}
