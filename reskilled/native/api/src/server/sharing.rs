//! Shared skills: under the server's rule, everyone online (or the members of a party) shares skill levels. When
//! one of them reaches a level, the others are lifted to it on the next tick, and so is a member who joins later.
//! Perk points follow the levels; perks stay each player's own. Parties and the shared levels are kept on the
//! world; `/skills party` makes and joins them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, Compound, Holder, Host, Tag, Text,
};
use serde::{Deserialize, Serialize};
use skills_core::progress::{PlayerState, level};
use skills_core::rules::Sharing;

use super::data::current;
use super::queue::{self, Job};
use super::state::{name_of, online_players, player_named, set_state, state_of};
use super::{lock, rules};
use crate::text::{
    NO_PLAYER, PARTY_ALREADY, PARTY_CREATED, PARTY_INVITE, PARTY_INVITED, PARTY_JOINED, PARTY_LEFT,
    PARTY_MEMBERS, PARTY_NO_INVITE, PARTY_NONE, say,
};

/// The parties, and the levels shared by each and by everyone.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Shared {
    parties: Vec<Party>,
    /// The levels everyone shares, under the rule that the whole server does.
    everyone: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Party {
    members: BTreeSet<u128>,
    /// The levels the party shares.
    levels: BTreeMap<String, u32>,
}

const JSON_TAG: &str = "json";

static SAVED: AttachmentType<Shared> = AttachmentType {
    namespace: "reskilled",
    name: "shared",
    default: Shared::default,
    codec: Some(AttachmentCodec { encode, decode }),
    copy_on_death: false,
    sync: false,
};

fn encode(shared: &Shared) -> Result<Compound, AttachmentError> {
    let json = serde_json::to_string(shared).expect("shared levels always encode");
    Ok([(JSON_TAG.into(), Tag::String(json.as_str().into()))].into())
}

fn decode(saved: &Compound) -> Result<Shared, AttachmentError> {
    let Some(Tag::String(json)) = saved.get(&JSON_TAG.into()) else {
        return Err(AttachmentError::Decode);
    };
    serde_json::from_str(&json.to_string_lossy()).map_err(|error| {
        log::warn!("Reskilled cannot read the saved parties: {error}");
        AttachmentError::Decode
    })
}

/// Who invited each player to their party, until they join.
static INVITES: Mutex<BTreeMap<u128, u128>> = Mutex::new(BTreeMap::new());

fn load(host: &mut dyn Host) -> Shared {
    SAVED.get(host, Holder::Level).unwrap_or_default()
}

fn save(host: &mut dyn Host, shared: Shared) {
    if let Err(error) = SAVED.set(host, Holder::Level, shared) {
        log::warn!("Reskilled could not keep the parties: {error:?}");
    }
}

/// The levels the player shares, and who shares them, under the rule in force.
fn group(
    shared: &mut Shared,
    sharing: Sharing,
    player: u128,
) -> Option<(&mut BTreeMap<String, u32>, Vec<u128>)> {
    match sharing {
        Sharing::Off => None,
        Sharing::Server => Some((&mut shared.everyone, online_players())),
        Sharing::Party => shared
            .parties
            .iter_mut()
            .find(|party| party.members.contains(&player))
            .map(|party| (&mut party.levels, party.members.iter().copied().collect())),
    }
}

/// The player's skills changed: the levels they share rise to theirs, and the others sharing them are lifted on
/// the next tick.
pub fn record(host: &mut dyn Host, player: u128, state: &PlayerState) {
    let sharing = rules::current(host).sharing;
    let Some(data) = current() else { return };
    let mut shared = load(host);
    let Some((levels, members)) = group(&mut shared, sharing, player) else {
        return;
    };
    for skill in state
        .skills
        .keys()
        .filter(|skill| data.defs.skill(skill).is_some())
    {
        let now = level(state, &data.defs, skill);
        let kept = levels.entry(skill.clone()).or_default();
        *kept = (*kept).max(now);
    }
    save(host, shared);
    let online = online_players();
    for member in members
        .into_iter()
        .filter(|m| *m != player && online.contains(m))
    {
        queue::queue(Job::LiftShared(member));
    }
}

/// The tick's part: the player is lifted to the levels they share, never lowered.
pub fn lift(host: &mut dyn Host, player: u128) {
    let sharing = rules::current(host).sharing;
    let Some(data) = current() else { return };
    let mut shared = load(host);
    let Some((levels, _)) = group(&mut shared, sharing, player) else {
        return;
    };
    let levels = levels.clone();
    let Ok(state) = state_of(host, player) else {
        return;
    };
    let mut next = state.clone();
    for (skill, shared_level) in &levels {
        if data.defs.skill(skill).is_none() || level(&state, &data.defs, skill) >= *shared_level {
            continue;
        }
        next.skills.entry(skill.clone()).or_default().level =
            (*shared_level).min(data.defs.max_level(skill));
    }
    if next != state {
        match set_state(host, player, next) {
            Ok(()) => queue::queue(Job::SyncAttributes(player)),
            Err(error) => {
                log::warn!("Reskilled could not lift {player:032x} to the shared levels: {error:?}")
            }
        }
    }
}

fn named(player: u128) -> Text {
    Text::literal(name_of(player).unwrap_or_else(|| format!("{player:032x}")))
}

/// `/skills party ...`: list, create, invite, join and leave. Returns the lines for the player.
pub fn party(host: &mut dyn Host, player: u128, args: &[&str]) -> Vec<Text> {
    let mut shared = load(host);
    let mine = shared
        .parties
        .iter()
        .position(|p| p.members.contains(&player));
    let line = match (args, mine) {
        ([], None) | (["leave"], None) | (["invite", _], None) => say(&PARTY_NONE, &[]),
        ([], Some(at)) => {
            let names: Vec<String> = shared.parties[at]
                .members
                .iter()
                .map(|m| named(*m).plain())
                .collect();
            say(&PARTY_MEMBERS, &[Text::literal(names.join(", "))])
        }
        (["create"], Some(_)) | (["join", _], Some(_)) => say(&PARTY_ALREADY, &[]),
        (["create"], None) => {
            shared.parties.push(Party {
                members: [player].into(),
                levels: BTreeMap::new(),
            });
            say(&PARTY_CREATED, &[])
        }
        (["invite", name], Some(_)) => match player_named(name) {
            None => say(&NO_PLAYER, &[Text::literal(*name)]),
            Some((invited, spelled)) => {
                lock(&INVITES).insert(invited, player);
                host.tell(invited, say(&PARTY_INVITE, &[named(player), named(player)]));
                say(&PARTY_INVITED, &[Text::literal(spelled)])
            }
        },
        (["join", name], None) => {
            let inviter = player_named(name).map(|(id, _)| id);
            let invited = inviter.filter(|id| lock(&INVITES).get(&player) == Some(id));
            let party =
                invited.and_then(|id| shared.parties.iter().position(|p| p.members.contains(&id)));
            match party {
                Some(at) => {
                    lock(&INVITES).remove(&player);
                    shared.parties[at].members.insert(player);
                    queue::queue(Job::LiftShared(player));
                    say(&PARTY_JOINED, &[named(inviter.unwrap_or_default())])
                }
                None => say(&PARTY_NO_INVITE, &[Text::literal(*name)]),
            }
        }
        (["leave"], Some(at)) => {
            shared.parties[at].members.remove(&player);
            shared.parties.retain(|p| !p.members.is_empty());
            say(&PARTY_LEFT, &[])
        }
        _ => return Vec::new(),
    };
    save(host, shared);
    vec![line]
}

/// Forgets what this module keeps for the running world, as the server stops.
pub(crate) fn reset() {
    lock(&INVITES).clear();
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use skills_core::progress::{PlayerState, SkillProgress, level};
    use skills_core::rules::{Rules, Sharing};

    use super::*;
    use crate::server::data::current;
    use crate::server::data::tests::load;
    use crate::server::queue::{run, take_queued};
    use crate::server::rules::force;
    use crate::server::state::{logged_in, set_state, state_of};
    use crate::test_host::{TestHost, exclusive};

    const ANN: u128 = 0xa;
    const BO: u128 = 0xb;
    const CY: u128 = 0xc;
    const MINING: &str = "reskilled:mining";

    /// Mining known; Ann, Bo and Cy online, all at Mining 1.
    fn world(sharing: Sharing) -> TestHost {
        let mut host = TestHost::default();
        let mining = json!({"format": 1, "id": MINING, "name": "n", "icon": "minecraft:stone"});
        load(&mut host, &[("skills", vec![(MINING, mining)])]);
        for (player, name) in [(ANN, "Ann"), (BO, "Bo"), (CY, "Cy")] {
            logged_in(player, name);
        }
        force(Rules {
            sharing,
            ..Rules::default()
        });
        take_queued();
        host
    }

    fn mining_of(host: &mut TestHost, player: u128) -> u32 {
        level(
            &state_of(host, player).unwrap(),
            &current().unwrap().defs,
            MINING,
        )
    }

    /// Ann reaches Mining `to`, as a raise would leave her, and the tick runs.
    fn ann_reaches(host: &mut TestHost, to: u32) {
        let mut state = PlayerState::default();
        let progress = SkillProgress {
            level: to,
            ..SkillProgress::default()
        };
        state.skills.insert(MINING.into(), progress);
        set_state(host, ANN, state.clone()).unwrap();
        record(host, ANN, &state);
        run(host, take_queued());
    }

    #[test]
    fn with_the_whole_server_sharing_everyone_is_lifted() {
        // Given everyone online sharing
        let _serial = exclusive();
        let mut host = world(Sharing::Server);

        // When Ann reaches Mining 5
        ann_reaches(&mut host, 5);

        // Then Bo and Cy are at 5 too
        assert_eq!([BO, CY].map(|p| mining_of(&mut host, p)), [5, 5]);
    }

    #[test]
    fn with_parties_only_her_party_is_lifted() {
        // Given parties sharing, Ann and Bo in one, Cy alone
        let _serial = exclusive();
        let mut host = world(Sharing::Party);
        party(&mut host, ANN, &["create"]);
        party(&mut host, ANN, &["invite", "Bo"]);
        party(&mut host, BO, &["join", "Ann"]);

        // When Ann reaches Mining 5
        ann_reaches(&mut host, 5);

        // Then Bo is lifted and Cy is not
        assert_eq!([BO, CY].map(|p| mining_of(&mut host, p)), [5, 1]);
    }

    #[test]
    fn with_sharing_off_nobody_is_lifted_and_a_newcomer_catches_up() {
        // Given sharing off, then the whole server sharing after Ann reached 5
        let _serial = exclusive();
        let mut host = world(Sharing::Off);
        ann_reaches(&mut host, 5);
        let off = mining_of(&mut host, BO);
        force(Rules {
            sharing: Sharing::Server,
            ..Rules::default()
        });
        ann_reaches(&mut host, 6);

        // When a new player joins and the tick runs
        logged_in(0xd, "Di");
        run(&mut host, take_queued());

        // Then nobody was lifted while it was off, and the newcomer starts at the shared 6
        assert_eq!(off, 1);
        assert_eq!(mining_of(&mut host, 0xd), 6);
    }

    #[test]
    fn a_shared_level_never_lowers_anyone() {
        // Given everyone sharing, and Bo already at Mining 9
        let _serial = exclusive();
        let mut host = world(Sharing::Server);
        let mut bo = PlayerState::default();
        bo.skills.insert(
            MINING.into(),
            SkillProgress {
                level: 9,
                ..SkillProgress::default()
            },
        );
        set_state(&mut host, BO, bo).unwrap();

        // When Ann reaches Mining 5
        ann_reaches(&mut host, 5);

        // Then Bo stays at 9
        assert_eq!(mining_of(&mut host, BO), 9);
    }

    #[test]
    fn party_commands_answer_in_words() {
        // Given three players
        let _serial = exclusive();
        let mut host = world(Sharing::Party);

        // When Ann makes a party, invites Bo, Cy joins without an invite, Bo joins, Ann lists it, and Bo leaves
        let lines: Vec<String> = [
            (ANN, vec!["create"]),
            (ANN, vec!["invite", "Bo"]),
            (CY, vec!["join", "Ann"]),
            (BO, vec!["join", "Ann"]),
            (ANN, vec![]),
            (BO, vec!["leave"]),
        ]
        .into_iter()
        .flat_map(|(player, args)| party(&mut host, player, &args))
        .map(|line| line.plain())
        .collect();

        // Then each says what happened, and Bo was told of the invite
        assert_eq!(
            lines,
            vec![
                "You made a party. Invite others with /skills party invite <player>.",
                "Invited Bo.",
                "Ann has not invited you.",
                "You joined Ann's party.",
                "Your party: Ann, Bo.",
                "You left your party."
            ]
        );
        assert_eq!(
            host.told_to(BO),
            vec!["Ann invites you to their party: /skills party join Ann"]
        );
    }
}
