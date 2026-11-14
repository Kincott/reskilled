//! The `/skills` command: a text view and the same requests as the screen, plus operator commands.

use horizon_client::mod_api::{Host, NativeCommand, NativeServerPlayer, Text};
use skills_core::progress::{PlayerState, Request, level, points_available, raise_cost};
use skills_core::registry::Defs;
use skills_core::wire::ToClient;

use super::actions::{Reply, defer};
use super::data::current;
use super::queue;
use super::state::{player_named, set_state, state_of};
use crate::MSG_CHANNEL;
use crate::text::{
    NO_FOCUS, NO_PLAYER, NOT_ALLOWED, PLAYERS_ONLY, RESET_ALL, RESET_ONE, SET_LEVEL, SET_POINTS,
    SKILL_LINE, SKILL_LINE_MAX, SUMMARY, UNAVAILABLE, UNKNOWN_SKILL, USAGE, focus_name, number,
    say, skill_name,
};

const COMMAND: &str = "skills";
/// The id namespace assumed for a skill, perk or focus argument written without one (`mining`).
const DEFAULT_NAMESPACE: &str = "reskilled";
/// Vanilla's level for commands that change other players (`/gamemode`, `/xp`).
const OPERATOR_LEVEL: u8 = 2;
const CONSOLE_LEVEL: u8 = 4;
const PLAYER_USAGES: [&str; 10] = [
    "skills",
    "skills raise <skill:word>",
    "skills perk <perk:word>",
    "skills focus <area:word>",
    "skills open",
    "skills party",
    "skills party create",
    "skills party invite <player:player>",
    "skills party join <player:player>",
    "skills party leave",
];
const OPERATOR_USAGES: [&str; 3] = [
    "skills set <player:player> <skill:word> <level:integer>",
    "skills points <player:player> <skill:word> <points:integer>",
    "skills reset <player:player> [skill:word]",
];

/// Who typed the command.
pub struct Source {
    /// None for the server console.
    pub player: Option<u128>,
    pub xp_levels: u32,
    /// Operator level, 0 to 4.
    pub permission: u8,
}

impl Source {
    pub const CONSOLE: Source = Source {
        player: None,
        xp_levels: 0,
        permission: CONSOLE_LEVEL,
    };

    pub fn player(player: &NativeServerPlayer) -> Self {
        Source {
            player: Some(player.uuid),
            xp_levels: u32::try_from(player.xp_level).unwrap_or(0),
            permission: player.permission,
        }
    }
}

pub fn register(commands: &mut Vec<NativeCommand>) {
    commands.extend(PLAYER_USAGES.map(NativeCommand::new));
    commands.extend(OPERATOR_USAGES.map(|usage| NativeCommand::op(usage, OPERATOR_LEVEL)));
}

/// Runs a `/skills` line (without its slash) and answers the source; false when the line is another command.
pub fn run_command(host: &mut dyn Host, source: &Source, line: &str) -> bool {
    let mut words = line.split_whitespace();
    if words.next() != Some(COMMAND) {
        return false;
    }
    let args: Vec<&str> = words.collect();
    let lines = match current() {
        None => vec![say(&UNAVAILABLE, &[])],
        Some(data) => match args.as_slice() {
            ["set" | "points" | "reset", ..] if source.permission < OPERATOR_LEVEL => {
                vec![say(&NOT_ALLOWED, &[])]
            }
            ["set", name, skill, level] => {
                operate(host, &data.defs, name, |state, defs, target| {
                    set_level(state, defs, &full_id(skill), level, target)
                })
            }
            ["points", name, skill, points] => {
                operate(host, &data.defs, name, |state, defs, target| {
                    set_points(state, defs, &full_id(skill), points, target)
                })
            }
            ["reset", name] => operate(host, &data.defs, name, |_, _, target| {
                Ok((
                    PlayerState::default(),
                    say(&RESET_ALL, &[Text::literal(target)]),
                ))
            }),
            ["reset", name, skill] => operate(host, &data.defs, name, |state, defs, target| {
                reset_skill(state, defs, &full_id(skill), target)
            }),
            _ => match source.player {
                None => vec![say(&PLAYERS_ONLY, &[])],
                Some(player) => for_player(host, &data.defs, player, source.xp_levels, &args),
            },
        },
    };
    for line in lines {
        match source.player {
            Some(player) => {
                host.tell(player, line);
            }
            None => host.feedback(line),
        }
    }
    true
}

/// The commands a player runs on themself.
fn for_player(
    host: &mut dyn Host,
    defs: &Defs,
    player: u128,
    xp_levels: u32,
    args: &[&str],
) -> Vec<Text> {
    match args {
        [] => summary(host, defs, player, xp_levels),
        ["open"] => {
            let open = serde_json::to_vec(&ToClient::Open).expect("Open always encodes");
            queue::send(player, MSG_CHANNEL, open);
            Vec::new()
        }
        ["raise", skill] => ask(
            player,
            xp_levels,
            Request::Raise {
                skill: full_id(skill),
                times: 1,
            },
        ),
        ["perk", perk] => ask(
            player,
            xp_levels,
            Request::BuyPerk {
                perk: full_id(perk),
            },
        ),
        ["focus", area] => ask(
            player,
            xp_levels,
            Request::AddFocus {
                focus: full_id(area),
            },
        ),
        ["party", rest @ ..] => match super::sharing::party(host, player, rest) {
            lines if lines.is_empty() => vec![say(&USAGE, &[])],
            lines => lines,
        },
        _ => vec![say(&USAGE, &[])],
    }
}

/// A request the player typed, carried out on the server tick and answered in chat then.
fn ask(player: u128, xp_levels: u32, request: Request) -> Vec<Text> {
    defer(player, xp_levels, request, Reply::Chat);
    Vec::new()
}

/// `/skills`: XP levels and focus, then each skill's level, raise cost and perk points.
fn summary(host: &mut dyn Host, defs: &Defs, player: u128, xp_levels: u32) -> Vec<Text> {
    let Ok(state) = state_of(host, player) else {
        return vec![say(&UNAVAILABLE, &[])];
    };
    let names: Vec<String> = state
        .focus
        .iter()
        .map(|f| focus_name(defs, f).plain())
        .collect();
    let focus = if names.is_empty() {
        say(&NO_FOCUS, &[])
    } else {
        Text::literal(names.join(", "))
    };
    let mut lines = vec![say(&SUMMARY, &[number(xp_levels), focus])];
    for skill in defs.skills.keys() {
        let (name, level, max) = (
            skill_name(defs, skill),
            level(&state, defs, skill),
            defs.max_level(skill),
        );
        let points = number(points_available(defs, &state, skill));
        lines.push(match raise_cost(defs, &state, skill) {
            Some(cost) => say(
                &SKILL_LINE,
                &[name, number(level), number(max), number(cost), points],
            ),
            None => say(&SKILL_LINE_MAX, &[name, number(level), number(max), points]),
        });
    }
    lines
}

/// What an operator command does to a state: the new state and the line confirming it, or why not.
type Change = Result<(PlayerState, Text), Text>;

/// Runs an operator change on the named online player, saves it and resyncs their attributes.
fn operate(
    host: &mut dyn Host,
    defs: &Defs,
    name: &str,
    change: impl FnOnce(PlayerState, &Defs, &str) -> Change,
) -> Vec<Text> {
    let Some((target, spelled)) = player_named(name) else {
        return vec![say(&NO_PLAYER, &[Text::literal(name)])];
    };
    let Ok(state) = state_of(host, target) else {
        return vec![say(&UNAVAILABLE, &[])];
    };
    let line = change(state, defs, &spelled).and_then(|(state, done)| {
        set_state(host, target, state).map_err(|_| say(&UNAVAILABLE, &[]))?;
        queue::queue(queue::Job::LiftShared(target));
        queue::recheck(target);
        Ok(done)
    });
    vec![line.unwrap_or_else(|why| why)]
}

fn set_level(
    mut state: PlayerState,
    defs: &Defs,
    skill: &str,
    level: &str,
    target: &str,
) -> Change {
    defs.skill(skill).ok_or_else(|| say(&UNKNOWN_SKILL, &[]))?;
    let level: u32 = level.parse().map_err(|_| say(&USAGE, &[]))?;
    let level = level.clamp(1, defs.max_level(skill));
    state.skills.entry(skill.to_string()).or_default().level = level;
    let done = say(
        &SET_LEVEL,
        &[
            Text::literal(target),
            skill_name(defs, skill),
            number(level),
        ],
    );
    Ok((state, done))
}

fn set_points(
    mut state: PlayerState,
    defs: &Defs,
    skill: &str,
    points: &str,
    target: &str,
) -> Change {
    defs.skill(skill).ok_or_else(|| say(&UNKNOWN_SKILL, &[]))?;
    let points: u32 = points.parse().map_err(|_| say(&USAGE, &[]))?;
    state
        .skills
        .entry(skill.to_string())
        .or_default()
        .bonus_points = points;
    let done = say(
        &SET_POINTS,
        &[
            Text::literal(target),
            skill_name(defs, skill),
            number(points),
        ],
    );
    Ok((state, done))
}

fn reset_skill(mut state: PlayerState, defs: &Defs, skill: &str, target: &str) -> Change {
    defs.skill(skill).ok_or_else(|| say(&UNKNOWN_SKILL, &[]))?;
    state.skills.remove(skill);
    Ok((
        state,
        say(
            &RESET_ONE,
            &[Text::literal(target), skill_name(defs, skill)],
        ),
    ))
}

/// `mining` means `reskilled:mining`; an id with a namespace is kept as typed.
fn full_id(arg: &str) -> String {
    if arg.contains(':') {
        arg.to_string()
    } else {
        format!("{DEFAULT_NAMESPACE}:{arg}")
    }
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::{
        NativeCommand, dispatch_player_logged_in, dispatch_register_commands,
    };
    use serde_json::json;
    use skills_core::progress::{PlayerState, SkillProgress, level, points_available};
    use skills_core::wire::ToClient;

    use super::*;
    use crate::MSG_CHANNEL;
    use crate::server::data::current;
    use crate::server::data::tests::load;
    use crate::server::queue::{Job, run, take_queued};
    use crate::server::state::{set_state, state_of};
    use crate::test_host::{TestHost, exclusive, mods};

    const MINING: &str = "reskilled:mining";
    const MELEE: &str = "reskilled:melee";
    const ANN: u128 = 0xa;
    const BO: u128 = 0xb;

    /// Mining and Melee, both players online, Ann at Mining 12 and Melee 5 with Working as her focus.
    fn world(host: &mut TestHost) {
        let skill = |id: &str, name: &str| json!({"format": 1, "id": id, "name": name, "icon": "minecraft:stone"});
        let working = json!({"format": 1, "id": "reskilled:working", "name": "reskilled.focus.working",
            "skills": [MINING]});
        let fighting = json!({"format": 1, "id": "reskilled:fighting", "name": "reskilled.focus.fighting",
            "skills": [MELEE]});
        load(
            host,
            &[
                (
                    "skills",
                    vec![
                        (MINING, skill(MINING, "reskilled.skill.mining")),
                        (MELEE, skill(MELEE, "reskilled.skill.melee")),
                    ],
                ),
                (
                    "focus",
                    vec![
                        ("reskilled:working", working),
                        ("reskilled:fighting", fighting),
                    ],
                ),
            ],
        );
        for (player, name) in [(ANN, "Ann"), (BO, "Bo")] {
            dispatch_player_logged_in(host, &mods(), player, name);
        }
        let mut ann = PlayerState {
            focus: ["reskilled:working".to_string()].into(),
            ..PlayerState::default()
        };
        ann.focus_chosen_before = true;
        ann.skills.insert(
            MINING.into(),
            SkillProgress {
                level: 12,
                points_spent: 1,
                ..SkillProgress::default()
            },
        );
        ann.skills.insert(
            MELEE.into(),
            SkillProgress {
                level: 5,
                ..SkillProgress::default()
            },
        );
        set_state(host, ANN, ann).unwrap();
        take_queued();
    }

    fn player(id: u128, xp_levels: u32, permission: u8) -> Source {
        Source {
            player: Some(id),
            xp_levels,
            permission,
        }
    }

    fn console() -> Source {
        Source {
            player: None,
            xp_levels: 0,
            permission: 4,
        }
    }

    #[test]
    fn registers_player_and_operator_usages() {
        // Given the mod
        let _serial = exclusive();
        let mut commands: Vec<NativeCommand> = Vec::new();

        // When the game asks for commands
        dispatch_register_commands(&mut TestHost::default(), &mods(), &mut commands);

        // Then players get the reading and spending usages, and operators of level 2 the rest
        let usages: Vec<(&str, u8)> = commands
            .iter()
            .map(|c| (c.usage.as_str(), c.level))
            .collect();
        assert_eq!(
            usages,
            vec![
                ("skills", 0),
                ("skills raise <skill:word>", 0),
                ("skills perk <perk:word>", 0),
                ("skills focus <area:word>", 0),
                ("skills open", 0),
                ("skills party", 0),
                ("skills party create", 0),
                ("skills party invite <player:player>", 0),
                ("skills party join <player:player>", 0),
                ("skills party leave", 0),
                ("skills set <player:player> <skill:word> <level:integer>", 2),
                (
                    "skills points <player:player> <skill:word> <points:integer>",
                    2
                ),
                ("skills reset <player:player> [skill:word]", 2),
            ]
        );
    }

    #[test]
    fn skills_command_lists_every_skill() {
        // Given Ann with 23 XP levels
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);

        // When she types /skills
        let ran = run_command(&mut host, &player(ANN, 23, 0), "skills");

        // Then she reads her XP and focus, then one line per skill with its level, raise cost and points
        assert!(ran);
        assert_eq!(
            host.told_to(ANN),
            vec![
                "XP levels: 23. Focus: Working.",
                "Melee 5/32: raise for 5 levels, 2 perk points.",
                "Mining 12/32: raise for 12 levels, 5 perk points.",
            ]
        );
    }

    #[test]
    fn focus_areas_are_added_by_command() {
        // Given Ann focused on Working, with 25 XP levels
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);

        // When she adds Fighting, lists her skills, then adds Working again
        run_command(&mut host, &player(ANN, 25, 0), "skills focus fighting");
        crate::server::actions::run_pending_for(&mut host, ANN);
        run_command(&mut host, &player(ANN, 5, 0), "skills");
        run_command(&mut host, &player(ANN, 5, 0), "skills focus working");
        crate::server::actions::run_pending_for(&mut host, ANN);

        // Then Fighting costs 20 for her second area, both show in her summary, and Working is already hers
        let told = host.told_to(ANN);
        assert_eq!(host.levels_given.get(&ANN), Some(&-20));
        assert_eq!(told[0], "Fighting is now one of your focus areas.");
        assert_eq!(told[1], "XP levels: 5. Focus: Fighting, Working.");
        assert_eq!(
            told.last().map(String::as_str),
            Some("That focus is already active.")
        );
    }

    #[test]
    fn raise_command_matches_the_request_path() {
        // Given Bo at Mining 1 with 10 XP levels
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);

        // When he raises Mining by its short name, then by its full id, then asks for a skill that does not exist
        let short = run_command(&mut host, &player(BO, 10, 0), "skills raise mining");
        let full = run_command(
            &mut host,
            &player(BO, 8, 0),
            "skills raise reskilled:mining",
        );
        let unknown = run_command(&mut host, &player(BO, 6, 0), "skills raise juggling");
        let before_tick = host.levels_given.get(&BO).copied();
        crate::server::actions::run_pending_for(&mut host, BO);

        // Then nothing happens until the server tick; then both raises take 2 levels each, as the screen's request
        // does, and the unknown one is refused
        assert_eq!(before_tick, None);
        let defs = &current().unwrap().defs;
        assert!(short && full && unknown);
        assert_eq!(host.levels_given.get(&BO), Some(&-3));
        assert_eq!(level(&state_of(&mut host, BO).unwrap(), defs, MINING), 3);
        assert_eq!(
            host.told_to(BO),
            vec![
                "Mining is now level 2.",
                "Mining is now level 3.",
                "There is no such skill."
            ]
        );
    }

    #[test]
    fn admin_commands_need_level_2() {
        // Given Bo (level 0) and an operator (level 2)
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);

        // When each tries to set Ann's Mining
        let by_bo = run_command(&mut host, &player(BO, 0, 0), "skills set Ann mining 20");
        let after_bo = level(
            &state_of(&mut host, ANN).unwrap(),
            &current().unwrap().defs,
            MINING,
        );
        let by_op = run_command(&mut host, &player(BO, 0, 2), "skills set Ann mining 20");

        // Then Bo is refused and the operator's change goes through, with an attribute sync for Ann
        let defs = &current().unwrap().defs;
        assert!(by_bo && by_op);
        assert_eq!(after_bo, 12);
        assert_eq!(level(&state_of(&mut host, ANN).unwrap(), defs, MINING), 20);
        assert_eq!(
            host.told_to(BO),
            vec!["You may not do that.", "Set Ann's Mining to level 20."]
        );
        assert_eq!(
            take_queued(),
            vec![
                Job::LiftShared(ANN),
                Job::SyncAttributes(ANN),
                Job::CheckEquipment(ANN)
            ]
        );
    }

    #[test]
    fn points_command_grants_bonus_points() {
        // Given Ann with 5 perk points in Mining
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);

        // When an operator sets her bonus points in Mining to 3, twice
        run_command(&mut host, &player(BO, 0, 2), "skills points Ann mining 3");
        run_command(&mut host, &player(BO, 0, 2), "skills points Ann mining 3");

        // Then the bonus is 3, not 6: she has 8
        let defs = &current().unwrap().defs;
        assert_eq!(
            points_available(defs, &state_of(&mut host, ANN).unwrap(), MINING),
            8
        );
        assert_eq!(
            host.told_to(BO),
            vec!["Set Ann's bonus points in Mining to 3."; 2]
        );
    }

    #[test]
    fn reset_clears_one_skill_or_all() {
        // Given Ann at Mining 12, Melee 5, focused on Working
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);
        let op = player(BO, 0, 2);

        // When an operator resets her Mining, then everything
        run_command(&mut host, &op, "skills reset Ann mining");
        let after_one = state_of(&mut host, ANN).unwrap();
        run_command(&mut host, &op, "skills reset Ann");
        let after_all = state_of(&mut host, ANN).unwrap();

        // Then Mining is back to level 1 with Melee and the focus kept, and then nothing is left
        let defs = &current().unwrap().defs;
        assert_eq!(
            (
                level(&after_one, defs, MINING),
                level(&after_one, defs, MELEE)
            ),
            (1, 5)
        );
        assert_eq!(after_one.focus, ["reskilled:working".to_string()].into());
        assert_eq!(after_all, PlayerState::default());
        assert_eq!(
            host.told_to(BO),
            vec!["Reset Ann's Mining.", "Reset all of Ann's skills."]
        );
    }

    #[test]
    fn open_command_asks_the_client_to_open_the_screen() {
        // Given Ann
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);

        // When she types /skills open
        run_command(&mut host, &player(ANN, 0, 0), "skills open");
        run(&mut host, take_queued());

        // Then her client is told to open the screen
        let sent: Vec<ToClient> = host
            .sent_to(ANN, MSG_CHANNEL)
            .iter()
            .map(|m| serde_json::from_slice(m).unwrap())
            .collect();
        assert_eq!(sent, vec![ToClient::Open]);
    }

    #[test]
    fn console_runs_operator_commands_but_not_player_ones() {
        // Given the server console
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);

        // When it sets a level, names a player who is not online, and tries to raise its own skill
        run_command(&mut host, &console(), "skills set Bo melee 4");
        run_command(&mut host, &console(), "skills set Cy melee 4");
        run_command(&mut host, &console(), "skills raise mining");

        // Then the first works, and the console reads why the others did not
        let defs = &current().unwrap().defs;
        assert_eq!(level(&state_of(&mut host, BO).unwrap(), defs, MELEE), 4);
        let lines: Vec<String> = host.feedback.iter().map(|t| t.plain()).collect();
        assert_eq!(
            lines,
            vec![
                "Set Bo's Melee to level 4.",
                "No player named Cy is online.",
                "Only players can do that."
            ]
        );
    }

    #[test]
    fn other_commands_are_left_alone() {
        // Given a line for another command, and a /skills line that is not a usage
        let _serial = exclusive();
        let mut host = TestHost::default();
        world(&mut host);

        // When they are run
        let other = run_command(&mut host, &player(ANN, 0, 0), "skillset 1");
        let wrong = run_command(&mut host, &player(ANN, 0, 0), "skills dance");

        // Then the other command is not taken, and the wrong usage is answered with the usage
        assert!(!other && wrong);
        assert_eq!(
            host.told_to(ANN),
            vec!["Usage: /skills [raise <skill> | perk <perk> | focus <area> | party | open]"]
        );
    }
}
