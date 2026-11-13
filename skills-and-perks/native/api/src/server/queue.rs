//! Work the hooks leave for the server tick, and the messages waiting to be sent, at most 32 a player a tick.
//! Some helpers refuse to run inside a player's own hook, so hooks queue work and the tick does it.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use horizon_client::mod_api::{EquipmentSlot, Host};

use super::{data, lock, state};

/// Horizon sends at most this many mod messages to one player in a tick.
const MESSAGES_PER_TICK: usize = 32;

/// Work for the next server tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    SendDefs(u128),
    SyncAttributes(u128),
    CheckEquipment(u128),
    #[expect(dead_code, reason = "lock enforcement (Task 10) queues it")]
    Unequip {
        player: u128,
        slot: EquipmentSlot,
    },
    ApplyDeath(u128),
}

static JOBS: Mutex<Vec<Job>> = Mutex::new(Vec::new());
/// One player's waiting messages, oldest first, each with its channel.
type Waiting = VecDeque<(&'static str, Vec<u8>)>;

static OUTBOX: Mutex<BTreeMap<u128, Waiting>> = Mutex::new(BTreeMap::new());

pub fn queue(job: Job) {
    lock(&JOBS).push(job);
}

/// What a player needs after joining or a data load: the definitions, their perks' attributes, and a check of what
/// they wear.
pub fn refresh(player: u128) {
    lock(&JOBS).extend([
        Job::SendDefs(player),
        Job::SyncAttributes(player),
        Job::CheckEquipment(player),
    ]);
}

/// The queued jobs, emptying the queue.
pub fn take_queued() -> Vec<Job> {
    std::mem::take(&mut *lock(&JOBS))
}

/// Queues a message for the player; it goes out on a coming tick, in order.
pub fn send(player: u128, channel: &'static str, data: Vec<u8>) {
    lock(&OUTBOX)
        .entry(player)
        .or_default()
        .push_back((channel, data));
}

/// Drops the player's waiting messages on the channel, which a newer send makes stale.
pub fn drop_waiting(player: u128, channel: &str) {
    if let Some(waiting) = lock(&OUTBOX).get_mut(&player) {
        waiting.retain(|(c, _)| *c != channel);
    }
}

/// Drops everything waiting for a player who left.
pub fn forget(player: u128) {
    lock(&OUTBOX).remove(&player);
}

/// Does the jobs, then sends what is waiting, at most 32 messages a player.
pub fn run(host: &mut dyn Host, jobs: Vec<Job>) {
    for job in jobs {
        match job {
            Job::SendDefs(player) => data::send_defs(player),
            Job::ApplyDeath(player) => state::apply_death(host, player),
            // Lock enforcement and perk effects take these over in later versions of this module.
            Job::SyncAttributes(_) | Job::CheckEquipment(_) | Job::Unequip { .. } => {
                log::debug!("Skills & Perks has no handler for {job:?} yet");
            }
        }
    }
    flush(host);
}

fn flush(host: &mut dyn Host) {
    let mut outbox = lock(&OUTBOX);
    for (player, waiting) in outbox.iter_mut() {
        for (channel, data) in waiting.drain(..waiting.len().min(MESSAGES_PER_TICK)) {
            if let Err(reason) = host.net_send_to_player(*player, channel, &data) {
                log::warn!(
                    "Skills & Perks could not send to player {player:032x} on {channel}: {reason}"
                );
            }
        }
    }
    outbox.retain(|_, waiting| !waiting.is_empty());
}

#[cfg(test)]
pub(crate) fn reset() {
    lock(&JOBS).clear();
    lock(&OUTBOX).clear();
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use skills_core::wire::{DefsChunk, Reassembler, client_defs_json};

    use super::*;
    use crate::DEFS_CHANNEL;
    use crate::server::data::current;
    use crate::server::test_host::{TestHost, exclusive, mods};

    /// Any channel other than the definitions'.
    const MSG_CHANNEL: &str = "skills_and_perks:msg";

    /// What the `server_tick` hook does. The hook itself cannot be dispatched in a test: its context is
    /// `#[non_exhaustive]` with no constructor.
    fn tick(host: &mut TestHost) {
        run(host, take_queued());
    }

    #[test]
    fn at_most_32_messages_a_player_a_tick() {
        // Given 40 messages waiting for one player and 3 for another
        let _serial = exclusive();
        let mut host = TestHost::default();
        for n in 0..40u8 {
            send(1, MSG_CHANNEL, vec![n]);
        }
        for n in 0..3u8 {
            send(2, MSG_CHANNEL, vec![n]);
        }

        // When two ticks pass
        tick(&mut host);
        let after_one = (
            host.sent_to(1, MSG_CHANNEL).len(),
            host.sent_to(2, MSG_CHANNEL).len(),
        );
        tick(&mut host);

        // Then the first tick sends 32 and 3, the second the last 8, in order
        assert_eq!(after_one, (32, 3));
        let first: Vec<u8> = host.sent_to(1, MSG_CHANNEL).iter().map(|m| m[0]).collect();
        assert_eq!(first, (0..40).collect::<Vec<u8>>());
    }

    #[test]
    fn send_defs_sends_the_current_definitions_in_chunks() {
        // Given loaded definitions and a SendDefs job
        let _serial = exclusive();
        let mut host = TestHost::default();
        let mining = json!({"format": 1, "id": "skills_and_perks:mining", "name": "n", "icon": "minecraft:stone"});
        crate::server::data::tests::load(
            &mut host,
            &[("skills", vec![("skills_and_perks:mining", mining)])],
        );
        take_queued();
        queue(Job::SendDefs(7));

        // When the tick runs
        tick(&mut host);

        // Then the player gets the definitions for clients, numbered with the data generation
        let data = current().unwrap();
        let mut reassembler = Reassembler::default();
        let mut whole = None;
        for message in host.sent_to(7, DEFS_CHANNEL) {
            let chunk: DefsChunk = serde_json::from_slice(message).unwrap();
            assert_eq!(u64::from(chunk.send_id), data.generation);
            whole = reassembler.push(chunk).or(whole);
        }
        assert_eq!(whole, Some(client_defs_json(&data.defs)));
    }

    #[test]
    fn a_newer_defs_send_replaces_queued_chunks() {
        // Given two SendDefs jobs for one player before any tick, with a chat message queued between them
        let _serial = exclusive();
        let mut host = TestHost::default();
        let mining = json!({"format": 1, "id": "skills_and_perks:mining", "name": "n", "icon": "minecraft:stone"});
        crate::server::data::tests::load(
            &mut host,
            &[("skills", vec![("skills_and_perks:mining", mining)])],
        );
        take_queued();
        queue(Job::SendDefs(7));
        tick(&mut host);
        let one_send = host.sent_to(7, DEFS_CHANNEL).len();
        host.sent.clear();
        send(7, MSG_CHANNEL, vec![1]);
        queue(Job::SendDefs(7));
        queue(Job::SendDefs(7));

        // When the tick runs
        tick(&mut host);

        // Then only one send's chunks go out, and the other message still does
        assert_eq!(host.sent_to(7, DEFS_CHANNEL).len(), one_send);
        assert_eq!(host.sent_to(7, MSG_CHANNEL).len(), 1);
    }

    #[test]
    fn logout_drops_waiting_messages() {
        // Given messages waiting for a player
        let _serial = exclusive();
        let mut host = TestHost::default();
        horizon_client::mod_api::dispatch_player_logged_in(&mut host, &mods(), 3, "Cy");
        take_queued();
        send(3, MSG_CHANNEL, vec![1]);

        // When they log out before the tick
        horizon_client::mod_api::dispatch_player_logged_out(&mut host, &mods(), 3, "Cy");
        tick(&mut host);

        // Then nothing is sent
        assert!(host.sent.is_empty());
    }
}
