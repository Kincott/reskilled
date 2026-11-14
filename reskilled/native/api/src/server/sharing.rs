//! Shared skills: under the server's rule, everyone (or the members of a party) shares skill levels. Each member is
//! shown the highest level any member reached by themselves, for as long as they share; their own level is kept
//! apart (`SkillProgress::own`) and is theirs again when they leave the party or sharing is turned off. Perks and
//! perk points follow the own level. Parties and each member's own levels are kept on the world; `/skills party`
//! makes and joins them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, Compound, Holder, Host, Tag, Text,
};
use serde::{Deserialize, Serialize};
use skills_core::progress::{PlayerState, own_level};
use skills_core::rules::Sharing;

use super::data::current;
use super::queue::{self, Job};
use super::state::{name_of, online_players, player_named, set_state, state_of};
use super::{lock, rules};
use crate::text::{
    NO_PLAYER, PARTY_ALREADY, PARTY_CREATED, PARTY_INVITE, PARTY_INVITED, PARTY_JOINED, PARTY_LEFT,
    PARTY_MEMBERS, PARTY_NO_INVITE, PARTY_NONE, say,
};

/// Each member's own levels, by skill.
type OwnLevels = BTreeMap<u128, BTreeMap<String, u32>>;

/// The parties, and the own levels of everyone who has shared under the rule that the whole server does.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Shared {
    parties: Vec<Party>,
    everyone_own: OwnLevels,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Party {
    members: BTreeSet<u128>,
    /// The members' own levels.
    own: OwnLevels,
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

/// The own levels of the group the player shares with under the rule in force, and its members online.
fn group(
    shared: &mut Shared,
    sharing: Sharing,
    player: u128,
) -> Option<(&mut OwnLevels, Vec<u128>)> {
    match sharing {
        Sharing::Off => None,
        Sharing::Server => Some((&mut shared.everyone_own, online_players())),
        Sharing::Party => {
            let online = online_players();
            shared
                .parties
                .iter_mut()
                .find(|party| party.members.contains(&player))
                .map(|party| {
                    let members = party
                        .members
                        .iter()
                        .copied()
                        .filter(|m| online.contains(m))
                        .collect();
                    (&mut party.own, members)
                })
        }
    }
}

/// The player's skills changed: their group learns their own levels and everyone in it is shown the best.
pub fn record(host: &mut dyn Host, player: u128, _state: &PlayerState) {
    lift(host, player);
}

/// Shows the player, per skill, the higher of their own level and the best own level in their group, or their own
/// level alone when they share with nobody. Their own levels are noted in the group first; when that changed it,
/// the other members online are looked at again on the next tick.
pub fn lift(host: &mut dyn Host, player: u128) {
    let sharing = rules::current(host).sharing;
    let Some(data) = current() else { return };
    let Ok(state) = state_of(host, player) else {
        return;
    };
    let defs = &data.defs;
    let own: BTreeMap<String, u32> = defs
        .skills
        .keys()
        .map(|skill| (skill.clone(), own_level(&state, defs, skill)))
        .collect();
    let mut shared = load(host);
    let mut best = BTreeMap::new();
    if let Some((levels, members)) = group(&mut shared, sharing, player) {
        let changed = levels.get(&player) != Some(&own);
        levels.insert(player, own.clone());
        for member_levels in levels.values() {
            for (skill, at) in member_levels {
                let kept = best.entry(skill.clone()).or_insert(0);
                *kept = (*kept).max(*at);
            }
        }
        if changed {
            for member in members.into_iter().filter(|m| *m != player) {
                queue::queue(Job::LiftShared(member));
            }
            save(host, shared);
        }
    }
    let mut next = state.clone();
    for (skill, mine) in &own {
        let shown = best
            .get(skill)
            .copied()
            .unwrap_or(0)
            .min(defs.max_level(skill))
            .max(*mine);
        let lifted = shown > *mine;
        let progress = match next.skills.get_mut(skill) {
            Some(progress) => progress,
            None if lifted => next.skills.entry(skill.clone()).or_default(),
            None => continue,
        };
        if lifted {
            progress.level = shown;
            progress.own = Some(*mine);
        } else {
            progress.level = *mine;
            progress.own = None;
        }
    }
    if next != state {
        match set_state(host, player, next) {
            Ok(()) => queue::recheck(player),
            Err(error) => {
                log::warn!("Reskilled could not set {player:032x}'s shared levels: {error:?}")
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
                own: BTreeMap::new(),
            });
            queue::queue(Job::LiftShared(player));
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
            let left = &mut shared.parties[at];
            left.members.remove(&player);
            left.own.remove(&player);
            let online = online_players();
            for member in left.members.iter().filter(|m| online.contains(m)) {
                queue::queue(Job::LiftShared(*member));
            }
            shared.parties.retain(|p| !p.members.is_empty());
            queue::queue(Job::LiftShared(player));
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
        settle(host);
    }

    /// Runs ticks until no job is left.
    fn settle(host: &mut TestHost) {
        for _ in 0..10 {
            let jobs = take_queued();
            if jobs.is_empty() {
                return;
            }
            run(host, jobs);
        }
        panic!("jobs keep coming");
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

    /// Ann makes a party and Bo joins it, and the tick runs.
    fn ann_and_bo_in_a_party(host: &mut TestHost) {
        party(host, ANN, &["create"]);
        party(host, ANN, &["invite", "Bo"]);
        party(host, BO, &["join", "Ann"]);
        run(host, take_queued());
    }

    #[test]
    fn leaving_a_party_takes_back_the_levels_it_lent() {
        // Given Ann at Mining 20, and Bo (Mining 1) in her party, lifted to 20
        let _serial = exclusive();
        let mut host = world(Sharing::Party);
        ann_reaches(&mut host, 20);
        ann_and_bo_in_a_party(&mut host);
        let in_party = mining_of(&mut host, BO);

        // When Bo leaves and the tick runs
        party(&mut host, BO, &["leave"]);
        run(&mut host, take_queued());

        // Then Bo is back at his own level 1, and Ann keeps her 20
        assert_eq!(in_party, 20);
        assert_eq!([ANN, BO].map(|p| mining_of(&mut host, p)), [20, 1]);
    }

    #[test]
    fn when_the_best_member_leaves_the_others_drop_to_their_own() {
        // Given Ann at Mining 20 and Bo, at his own 3, lifted to 20 in her party
        let _serial = exclusive();
        let mut host = world(Sharing::Party);
        ann_reaches(&mut host, 20);
        let mut bo = PlayerState::default();
        bo.skills.insert(
            MINING.into(),
            SkillProgress {
                level: 3,
                ..SkillProgress::default()
            },
        );
        set_state(&mut host, BO, bo).unwrap();
        ann_and_bo_in_a_party(&mut host);

        // When Ann leaves and the tick runs
        party(&mut host, ANN, &["leave"]);
        run(&mut host, take_queued());

        // Then Bo is back at his own 3
        assert_eq!(mining_of(&mut host, BO), 3);
    }

    #[test]
    fn when_the_best_member_dies_the_group_drops_with_them() {
        // Given a 2-level death penalty, everyone sharing, Ann at Mining 20 and Bo lifted to it
        let _serial = exclusive();
        let mut host = world(Sharing::Server);
        let mining = json!({"format": 1, "id": MINING, "name": "n", "icon": "minecraft:stone"});
        let settings = json!({"format": 1, "death_penalty_levels": 2});
        load(
            &mut host,
            &[
                ("skills", vec![(MINING, mining)]),
                ("settings", vec![("reskilled:hard", settings)]),
            ],
        );
        take_queued();
        ann_reaches(&mut host, 20);
        let bo_before = mining_of(&mut host, BO);

        // When Ann dies and the ticks run
        crate::server::state::apply_death(&mut host, ANN);
        settle(&mut host);

        // Then Ann is at 18 and so is Bo: the dead player's lost levels are not given back by the group
        assert_eq!(bo_before, 20);
        assert_eq!([ANN, BO].map(|p| mining_of(&mut host, p)), [18, 18]);
    }

    #[test]
    fn turning_sharing_off_takes_back_lent_levels() {
        // Given everyone sharing, Ann at Mining 8 and Bo lifted to it
        let _serial = exclusive();
        let mut host = world(Sharing::Server);
        ann_reaches(&mut host, 8);
        let shared = mining_of(&mut host, BO);

        // When sharing is turned off and Bo's levels are looked at again
        force(Rules::default());
        lift(&mut host, BO);

        // Then Bo is back at 1
        assert_eq!((shared, mining_of(&mut host, BO)), (8, 1));
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
