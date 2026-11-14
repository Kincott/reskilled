//! The server's rules (`skills_core::rules::Rules`), kept with the world and changed by operators on the options
//! screen. Every player is sent them, with whether they may change them.

use std::sync::Mutex;

use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, Compound, Holder, Host, Tag,
};
use skills_core::rules::Rules;
use skills_core::wire::ToClient;

use super::queue::{self, Job};
use super::{lock, state};
use crate::MSG_CHANNEL;
use crate::text::{NOT_ALLOWED, say};

/// Vanilla's operator level for commands that change the game (`/gamerule`).
const OPERATOR_LEVEL: u8 = 2;
const JSON_TAG: &str = "json";

/// The rules, kept on the world.
static SAVED: AttachmentType<Rules> = AttachmentType {
    namespace: "reskilled",
    name: "rules",
    default: Rules::default,
    codec: Some(AttachmentCodec { encode, decode }),
    copy_on_death: false,
    sync: false,
};

fn encode(rules: &Rules) -> Result<Compound, AttachmentError> {
    let json = serde_json::to_string(rules).expect("rules always encode");
    Ok([(JSON_TAG.into(), Tag::String(json.as_str().into()))].into())
}

fn decode(saved: &Compound) -> Result<Rules, AttachmentError> {
    let Some(Tag::String(json)) = saved.get(&JSON_TAG.into()) else {
        return Err(AttachmentError::Decode);
    };
    serde_json::from_str(&json.to_string_lossy()).map_err(|error| {
        log::warn!("Reskilled cannot read the saved rules, so plays by the defaults: {error}");
        AttachmentError::Decode
    })
}

/// The rules in force, read from the world once and kept.
static CURRENT: Mutex<Option<Rules>> = Mutex::new(None);
/// Rules operators asked for, applied in their own tick.
static PENDING: Mutex<Vec<(u128, Rules)>> = Mutex::new(Vec::new());

/// The rules in force.
pub fn current(host: &mut dyn Host) -> Rules {
    if let Some(rules) = lock(&CURRENT).clone() {
        return rules;
    }
    let rules = SAVED.get(host, Holder::Level).unwrap_or_default();
    *lock(&CURRENT) = Some(rules.clone());
    rules
}

/// A player asked to change the rules; their own tick decides, where their operator level can be read.
pub fn request(player: u128, rules: Rules) {
    lock(&PENDING).push((player, rules));
}

/// The player's own tick: their change is applied when they are an operator, refused otherwise.
pub fn run_for(host: &mut dyn Host, player: u128) {
    let asked: Vec<Rules> = {
        let mut pending = lock(&PENDING);
        let (theirs, others) = std::mem::take(&mut *pending)
            .into_iter()
            .partition(|(p, _)| *p == player);
        *pending = others;
        theirs.into_iter().map(|(_, rules)| rules).collect()
    };
    for rules in asked {
        if host.player_permission(player).unwrap_or(0) < OPERATOR_LEVEL {
            host.action_bar(player, say(&NOT_ALLOWED, &[]));
            continue;
        }
        if let Err(error) = SAVED.set(host, Holder::Level, rules.clone()) {
            log::warn!("Reskilled could not keep the new rules: {error:?}");
            continue;
        }
        *lock(&CURRENT) = Some(rules);
        for online in state::online_players() {
            queue::queue(Job::SendRules(online));
        }
    }
}

/// Sends the player the rules and whether they may change them.
pub fn send(host: &mut dyn Host, player: u128) {
    let editable = host.player_permission(player).unwrap_or(0) >= OPERATOR_LEVEL;
    let message = ToClient::Rules {
        rules: current(host),
        editable,
    };
    let data = serde_json::to_vec(&message).expect("rules always encode");
    queue::send(player, MSG_CHANNEL, data);
}

/// Puts rules in force without an operator or a world, for tests.
#[cfg(test)]
pub fn force(rules: Rules) {
    *lock(&CURRENT) = Some(rules);
}

/// Forgets the rules read from the world, as a server that stops does; the next read takes them from the world.
#[cfg(test)]
pub fn forget_cached() {
    *lock(&CURRENT) = None;
}

#[cfg(test)]
pub(crate) fn reset() {
    forget_cached();
    lock(&PENDING).clear();
}

#[cfg(test)]
mod tests {
    use skills_core::rules::{Progression, Rules, Sharing};
    use skills_core::wire::ToClient;

    use super::*;
    use crate::MSG_CHANNEL;
    use crate::server::queue::{run, take_queued};
    use crate::server::state::logged_in;
    use crate::test_host::{TestHost, exclusive};

    const OP: u128 = 0x0a;
    const PLAYER: u128 = 0x0b;

    fn told(host: &TestHost, player: u128) -> Vec<ToClient> {
        host.sent_to(player, MSG_CHANNEL)
            .iter()
            .map(|m| serde_json::from_slice(m).unwrap())
            .filter(|m| matches!(m, ToClient::Rules { .. }))
            .collect()
    }

    fn changed() -> Rules {
        Rules {
            sharing: Sharing::Server,
            progression: Progression::Both,
            ..Rules::default()
        }
    }

    #[test]
    fn an_operators_change_is_kept_with_the_world_and_sent_to_everyone() {
        // Given an operator and a player online
        let _serial = exclusive();
        let mut host = TestHost::default();
        host.permissions.insert(OP, 2);
        logged_in(OP, "Op");
        logged_in(PLAYER, "Pat");
        take_queued();

        // When the operator changes the rules, their tick runs, then the server's
        request(OP, changed());
        run_for(&mut host, OP);
        run(&mut host, take_queued());

        // Then the rules change, each player is sent them (only the operator may edit), and they are kept with the
        // world: read again after the server forgot them, they are the same
        assert_eq!(current(&mut host), changed());
        let to_op = ToClient::Rules {
            rules: changed(),
            editable: true,
        };
        let to_player = ToClient::Rules {
            rules: changed(),
            editable: false,
        };
        assert_eq!(told(&host, OP), vec![to_op]);
        assert_eq!(told(&host, PLAYER), vec![to_player]);
        forget_cached();
        assert_eq!(current(&mut host), changed());
    }

    #[test]
    fn a_player_who_is_not_an_operator_cannot_change_them() {
        // Given a player who is not an operator
        let _serial = exclusive();
        let mut host = TestHost::default();
        logged_in(PLAYER, "Pat");
        take_queued();

        // When they send changed rules and their tick runs
        request(PLAYER, changed());
        run_for(&mut host, PLAYER);

        // Then nothing changes and they read why
        assert_eq!(current(&mut host), Rules::default());
        assert_eq!(host.action_bars_of(PLAYER), vec!["You may not do that."]);
    }

    #[test]
    fn a_world_without_saved_rules_plays_by_the_defaults_and_joining_players_are_sent_them() {
        // Given a new world
        let _serial = exclusive();
        let mut host = TestHost::default();

        // When a player joins and the tick runs
        logged_in(PLAYER, "Pat");
        run(&mut host, take_queued());

        // Then the rules are the defaults, and the player is sent them
        assert_eq!(current(&mut host), Rules::default());
        let sent = ToClient::Rules {
            rules: Rules::default(),
            editable: false,
        };
        assert_eq!(told(&host, PLAYER), vec![sent]);
    }
}
