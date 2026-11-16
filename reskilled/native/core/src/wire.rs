//! The save format of a player's state and the messages between client and server.
//!
//! Everything is JSON. Definitions can be bigger than one network message, so the server sends them as numbered
//! chunks of one send and the client puts the newest complete send back together.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::defs::{Action, DefError, Target};
use crate::locks::{Requirements, labels_by_level};
use crate::progress::{PlayerState, Request, STATE_FORMAT};
use crate::registry::Defs;
use crate::rules::Rules;

/// Names a bad saved state in a [`DefError`].
const STATE_SOURCE: &str = "player state";
const FORMAT_FIELD: &str = "format";
/// The most bytes one character takes once escaped in a JSON string (`\u001f`).
const MAX_ESCAPED_CHAR: usize = 6;
/// A send claiming more chunks than this is ignored: 1,024 chunks of 30,000 bytes is 30 MB of definitions.
const MAX_CHUNKS: u32 = 1_024;

/// Client to server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ToServer {
    Act(Request),
    /// Item ids whose requirements the client wants for its tooltips.
    Need {
        items: Vec<String>,
    },
    /// This client runs the mod: it draws lock warnings itself, so the server leaves out the action-bar line.
    Hello,
    /// An operator changed the server's rules on the options screen.
    SetRules(Rules),
}

/// Server to client, apart from definition chunks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ToClient {
    /// Requirements per item id, answering a `Need`.
    Reqs { items: Vec<(String, Requirements)> },
    /// Open the skills screen.
    Open,
    /// A lock refused an action for want of this level in this skill: the client shows its warning and plays its
    /// sound. Sent only to clients that said `Hello`.
    Refused {
        skill: String,
        level: u32,
        action: Action,
        /// The enchantment on the item (id, and the level the item has it at) that brings the need, when the item
        /// alone would not ask it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        because: Option<(String, u32)>,
    },
    /// The player this client plays as, so it can read its own synced state.
    You { player: u128 },
    /// The player's home (respawn point): its dimension and block, or none; perks' areas are drawn around it.
    Home { home: Option<(String, [i32; 3])> },
    /// The player's XP level, and whether locks are off for them (creative or spectator, by the settings): the
    /// client has no other way to read either. Sent when one changes.
    Status { levels: u32, exempt: bool },
    /// The server's rules, and whether this player may change them (an operator).
    Rules { rules: Rules, editable: bool },
}

/// What clients get of the definitions: no lock matchers (only the server can test tags), and the lock labels per
/// skill and level for the unlock preview.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientDefs {
    pub defs: Defs,
    pub labels: BTreeMap<String, BTreeMap<u32, Vec<String>>>,
    /// The item drawn for each lock label on its skill's road.
    #[serde(default)]
    pub label_icons: BTreeMap<String, String>,
}

/// One piece of a definitions send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefsChunk {
    /// Grows with every send; a client keeps only the newest.
    pub send_id: u32,
    pub index: u32,
    pub total: u32,
    pub data: String,
}

pub fn encode_state(s: &PlayerState) -> String {
    serde_json::to_string(s).expect("a player state always encodes: string keys and plain values")
}

/// Reads a saved state, refusing formats this version does not know.
pub fn decode_state(json: &str) -> Result<PlayerState, DefError> {
    let refused = |reason: String| DefError {
        file: STATE_SOURCE.to_string(),
        reason,
    };
    let mut value: Value = serde_json::from_str(json).map_err(|e| refused(e.to_string()))?;
    let format = value.get(FORMAT_FIELD).cloned().unwrap_or(Value::Null);
    if format.as_u64() == Some(FIRST_STATE_FORMAT) {
        from_first_format(&mut value);
    } else if format.as_u64() != Some(u64::from(STATE_FORMAT)) {
        return Err(refused(format!(
            "unsupported format {format} (this version reads format {STATE_FORMAT})"
        )));
    }
    serde_json::from_value(value).map_err(|e| refused(e.to_string()))
}

/// The first save format, which kept one focus area (or `null`) where this one keeps a list.
const FIRST_STATE_FORMAT: u64 = 1;

/// Turns a first-format state into this format: its one focus area becomes the only active one.
fn from_first_format(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let focus = match object.remove("focus") {
        Some(Value::String(area)) => vec![Value::String(area)],
        _ => Vec::new(),
    };
    object.insert("focus".into(), Value::Array(focus));
    object.insert(FORMAT_FIELD.into(), Value::from(STATE_FORMAT));
}

/// The [`ClientDefs`] for these definitions, as JSON.
pub fn client_defs_json(defs: &Defs) -> String {
    let labels = defs
        .skills
        .keys()
        .map(|skill| (skill.clone(), labels_by_level(defs, skill)))
        .filter(|(_, by_level)| !by_level.is_empty())
        .collect();
    let client = ClientDefs {
        defs: Defs {
            locks: defs
                .locks
                .iter()
                .filter(|entry| matches!(entry.target, Target::Enchantment(_)))
                .cloned()
                .collect(),
            ..defs.clone()
        },
        labels,
        label_icons: defs
            .locks
            .iter()
            .filter_map(|entry| Some((entry.label.clone()?, entry.icon.clone()?)))
            .collect(),
    };
    serde_json::to_string(&client)
        .expect("definitions always encode: string keys and finite numbers")
}

/// Splits the JSON into chunks whose own JSON encoding is at most `max_bytes`. Empty JSON still makes one chunk, so
/// the client learns the send is complete.
pub fn chunk_defs(defs_json: &str, send_id: u32, max_bytes: usize) -> Vec<DefsChunk> {
    let envelope = DefsChunk {
        send_id,
        index: u32::MAX,
        total: u32::MAX,
        data: String::new(),
    };
    let envelope_len = serde_json::to_vec(&envelope)
        .expect("a chunk always encodes")
        .len();
    let budget = max_bytes
        .checked_sub(envelope_len)
        .filter(|budget| *budget >= MAX_ESCAPED_CHAR)
        .expect(
            "max_bytes must leave room for at least one character after the chunk's own fields",
        );
    let mut pieces = Vec::new();
    let (mut start, mut used) = (0, 0);
    for (at, ch) in defs_json.char_indices() {
        let size = escaped_len(ch);
        if used + size > budget {
            pieces.push(&defs_json[start..at]);
            (start, used) = (at, 0);
        }
        used += size;
    }
    pieces.push(&defs_json[start..]);
    let total = u32::try_from(pieces.len()).expect("fewer than 2^32 chunks");
    (0..total)
        .zip(pieces)
        .map(|(index, data)| DefsChunk {
            send_id,
            index,
            total,
            data: data.to_string(),
        })
        .collect()
}

/// Bytes the character takes inside a JSON string as serde_json writes it.
fn escaped_len(ch: char) -> usize {
    match ch {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        c if u32::from(c) < 0x20 => MAX_ESCAPED_CHAR,
        c => c.len_utf8(),
    }
}

/// Puts chunks back together, keeping only the newest send.
#[derive(Debug, Default)]
pub struct Reassembler {
    send_id: Option<u32>,
    /// The newest send's parts; emptied once it completes, so its replays no longer fit.
    parts: Vec<Option<String>>,
}

impl Reassembler {
    /// Adds a chunk; returns the whole JSON when this chunk completes the newest send. Chunks of an older send, of
    /// a send already completed, or that do not fit their send are ignored.
    pub fn push(&mut self, c: DefsChunk) -> Option<String> {
        match self.send_id {
            Some(current) if c.send_id < current => return None,
            Some(current) if c.send_id == current => {}
            _ if c.total == 0 || c.total > MAX_CHUNKS => return None,
            _ => {
                self.send_id = Some(c.send_id);
                self.parts = vec![None; c.total as usize];
            }
        }
        if c.total as usize != self.parts.len() {
            return None;
        }
        *self.parts.get_mut(c.index as usize)? = Some(c.data);
        if !self.parts.iter().all(Option::is_some) {
            return None;
        }
        Some(self.parts.drain(..).flatten().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::{Action, LockEntry, Matcher, SkillDef, Target};
    use crate::progress::SkillProgress;

    const MINING: &str = "reskilled:mining";
    const LIMIT: usize = 30_000;

    fn mining() -> SkillDef {
        SkillDef {
            id: MINING.into(),
            priority: 0,
            name: "reskilled.skill.mining".into(),
            icon: "minecraft:iron_pickaxe".into(),
            max_level: None,
            cost: None,
            points_every: None,
            icon_levels: Default::default(),
            earned_by: Vec::new(),
            color: None,
        }
    }

    #[test]
    fn state_round_trips() {
        // Given a state with a level, spent and granted points, a perk and a focus
        let mut state = PlayerState {
            focus: ["reskilled:working".to_string()].into(),
            ..PlayerState::default()
        };
        state.focus_chosen_before = true;
        state.skills.insert(
            MINING.into(),
            SkillProgress {
                level: 12,
                points_spent: 2,
                bonus_points: 1,
                perks: ["reskilled:mining_sure_grip".to_string()].into(),
                disabled: Default::default(),
                earned: 0,
                own: Some(9),
            },
        );

        // When it is encoded and decoded
        let decoded = decode_state(&encode_state(&state));

        // Then it comes back equal
        assert_eq!(decoded, Ok(state));
    }

    #[test]
    fn a_first_format_state_keeps_its_one_focus() {
        // Given states saved by the first version, with a focus and without one
        let focused = r#"{"format": 1, "skills": {}, "focus": "reskilled:working", "focus_chosen_before": true}"#;
        let unfocused =
            r#"{"format": 1, "skills": {}, "focus": null, "focus_chosen_before": false}"#;

        // When they are decoded
        let focused = decode_state(focused).unwrap();
        let unfocused = decode_state(unfocused).unwrap();

        // Then the focus is the one active area, or none, in this version's format
        assert_eq!(focused.focus, ["reskilled:working".to_string()].into());
        assert!(focused.focus_chosen_before);
        assert!(unfocused.focus.is_empty());
        assert_eq!(focused.format, STATE_FORMAT);
    }

    #[test]
    fn decode_refuses_a_later_format() {
        // Given a state from a later format, and text that is not a state
        let later = r#"{"format": 3, "skills": {}, "focus": [], "focus_chosen_before": false}"#;

        // When they are decoded
        let later = decode_state(later).unwrap_err();
        let garbage = decode_state("not json").unwrap_err();

        // Then both are refused, the first because of its format
        assert!(later.reason.contains("format"), "{}", later.reason);
        assert!(!garbage.reason.is_empty());
    }

    #[test]
    fn wire_round_trips() {
        // Given one of each message
        let requirements = Requirements {
            by_action: [(Action::Break, [(MINING.to_string(), 16)].into())].into(),
        };
        let to_server = vec![
            ToServer::Act(Request::Raise {
                skill: MINING.into(),
                times: 1,
            }),
            ToServer::Act(Request::BuyPerk {
                perk: "reskilled:mining_sure_grip".into(),
            }),
            ToServer::Need {
                items: vec!["minecraft:diamond_pickaxe".into()],
            },
            ToServer::Hello,
            ToServer::SetRules(Rules::default()),
        ];
        let to_client = vec![
            ToClient::Reqs {
                items: vec![("minecraft:diamond_pickaxe".into(), requirements)],
            },
            ToClient::Open,
            ToClient::Refused {
                skill: MINING.into(),
                level: 16,
                action: Action::Break,
                because: None,
            },
            ToClient::Refused {
                skill: "reskilled:melee".into(),
                level: 12,
                action: Action::Attack,
                because: Some(("minecraft:sharpness".into(), 3)),
            },
            ToClient::You {
                player: u128::MAX - 7,
            },
            ToClient::Status {
                levels: 31,
                exempt: true,
            },
            ToClient::Rules {
                rules: Rules::default(),
                editable: true,
            },
        ];

        // When each is encoded and decoded
        let back_to_server: Vec<ToServer> = to_server
            .iter()
            .map(|m| serde_json::from_slice(&serde_json::to_vec(m).unwrap()).unwrap())
            .collect();
        let back_to_client: Vec<ToClient> = to_client
            .iter()
            .map(|m| serde_json::from_slice(&serde_json::to_vec(m).unwrap()).unwrap())
            .collect();

        // Then they come back equal
        assert_eq!(back_to_server, to_server);
        assert_eq!(back_to_client, to_client);
    }

    #[test]
    fn a_refusal_without_a_cause_reads_as_before() {
        // Given a refusal as servers sent it before causes were named
        let old = br#"{"refused": {"skill": "reskilled:mining", "level": 16, "action": "break"}}"#;

        // When it is read, and a refusal with no cause is written
        let read: ToClient = serde_json::from_slice(old).unwrap();
        let written = serde_json::to_string(&read).unwrap();

        // Then it has no cause, and none is written
        assert!(matches!(read, ToClient::Refused { because: None, .. }));
        assert!(!written.contains("because"), "{written}");
    }

    #[test]
    fn client_defs_keep_only_enchantment_locks_but_have_labels() {
        // Given definitions with a labelled lock on the diamond pickaxe and a lock on Sharpness V
        let sharpness = LockEntry {
            target: Target::Enchantment(Matcher::Id("minecraft:sharpness".into())),
            actions: vec![Action::Attack],
            requires: [(MINING.to_string(), 20)].into(),
            label: None,
            replace: false,
            enchantment_level: Some(5),
            icon: None,
        };
        let defs = Defs {
            skills: [(MINING.into(), mining())].into(),
            locks: vec![
                LockEntry {
                    target: Target::Item(Matcher::Id("minecraft:diamond_pickaxe".into())),
                    actions: vec![Action::Break],
                    requires: [(MINING.to_string(), 16)].into(),
                    label: Some("reskilled.lock.diamond_tools".into()),
                    replace: false,
                    enchantment_level: None,
                    icon: None,
                },
                sharpness.clone(),
            ],
            ..Defs::default()
        };

        // When they are prepared for clients
        let json = client_defs_json(&defs);
        let client: ClientDefs = serde_json::from_str(&json).unwrap();

        // Then the item locks are gone (the client cannot test tags) and the enchantment lock stays, so the client
        // can read an item's enchantments itself; the skills and the labels by level are there
        assert_eq!(client.defs.locks, vec![sharpness]);
        assert!(!json.contains("minecraft:diamond_pickaxe"), "{json}");
        assert_eq!(client.defs.skills[MINING], mining());
        assert_eq!(
            client.labels[MINING][&16],
            vec!["reskilled.lock.diamond_tools".to_string()]
        );
    }

    #[test]
    fn client_defs_carry_lock_icons_by_label() {
        // Given a labelled lock with an icon, and a labelled lock without one
        let entry = |label: &str, icon: Option<&str>| LockEntry {
            target: Target::Item(Matcher::Id("minecraft:diamond_axe".into())),
            actions: vec![Action::Break],
            requires: [(MINING.to_string(), 16)].into(),
            label: Some(label.into()),
            replace: false,
            enchantment_level: None,
            icon: icon.map(String::from),
        };
        let defs = Defs {
            skills: [(MINING.into(), mining())].into(),
            locks: vec![
                entry("with_icon", Some("minecraft:netherite_axe")),
                entry("without", None),
            ],
            ..Defs::default()
        };

        // When they are prepared for clients
        let client: ClientDefs = serde_json::from_str(&client_defs_json(&defs)).unwrap();

        // Then only the first label has its icon
        assert_eq!(
            client.label_icons,
            [(
                "with_icon".to_string(),
                "minecraft:netherite_axe".to_string()
            )]
            .into()
        );
    }

    /// About 100 KB of JSON with quotes, backslashes and non-ASCII text, which grow or vary when encoded.
    fn big_json() -> String {
        let entry = r#"{"name": "Pioche \"de\" diamant \\ ünïcödé ⛏", "level": 16},"#;
        format!("[{}{{}}]", entry.repeat(1_600))
    }

    #[test]
    fn chunks_fit_the_limit() {
        // Given about 100 KB of definitions
        let json = big_json();

        // When they are chunked for 30,000-byte messages
        let chunks = chunk_defs(&json, 7, LIMIT);

        // Then every encoded chunk fits, they are numbered in order, and together they are the original
        assert!(chunks.len() > 3, "{}", chunks.len());
        for (i, chunk) in chunks.iter().enumerate() {
            assert!(serde_json::to_vec(chunk).unwrap().len() <= LIMIT);
            assert_eq!(
                (chunk.send_id, chunk.index, chunk.total),
                (7, i as u32, chunks.len() as u32)
            );
        }
        assert_eq!(
            chunks.iter().map(|c| c.data.as_str()).collect::<String>(),
            json
        );
    }

    #[test]
    fn empty_defs_still_send_one_chunk() {
        // Given nothing to send
        // When it is chunked
        let chunks = chunk_defs("", 1, LIMIT);

        // Then one empty chunk still tells the client the send is complete
        assert_eq!(
            chunks,
            vec![DefsChunk {
                send_id: 1,
                index: 0,
                total: 1,
                data: String::new()
            }]
        );
    }

    #[test]
    fn defs_reassemble_in_any_order() {
        // Given the chunks of one send, delivered backwards
        let json = big_json();
        let mut chunks = chunk_defs(&json, 3, LIMIT);
        chunks.reverse();
        let replay = chunks[0].clone();
        let mut reassembler = Reassembler::default();

        // When they are pushed one by one, then one of them again
        let results: Vec<Option<String>> =
            chunks.into_iter().map(|c| reassembler.push(c)).collect();
        let replayed = reassembler.push(replay);

        // Then only the last push completes, with the original JSON, and the replay completes nothing
        let (last, earlier) = results.split_last().unwrap();
        assert!(earlier.iter().all(Option::is_none));
        assert_eq!(last.as_deref(), Some(json.as_str()));
        assert_eq!(replayed, None);
    }

    #[test]
    fn stale_chunks_are_ignored() {
        // Given send 1 and send 2 of different definitions
        let old = big_json();
        let new = big_json().replace("diamant", "netherite");
        let old_chunks = chunk_defs(&old, 1, LIMIT);
        let new_chunks = chunk_defs(&new, 2, LIMIT);
        let mut reassembler = Reassembler::default();

        // When half of send 1 arrives, then all of send 2 interleaved with the rest of send 1, then send 1 again
        let half = old_chunks.len() / 2;
        let mut results = Vec::new();
        for chunk in old_chunks[..half].iter().cloned() {
            results.push(reassembler.push(chunk));
        }
        for (new_chunk, old_chunk) in new_chunks
            .iter()
            .cloned()
            .zip(old_chunks[half..].iter().cloned().cycle())
        {
            results.push(reassembler.push(old_chunk));
            results.push(reassembler.push(new_chunk));
        }
        let replayed: Vec<Option<String>> = old_chunks
            .into_iter()
            .map(|c| reassembler.push(c))
            .collect();

        // Then only send 2 ever completes, once, and the replayed old send changes nothing
        let completed: Vec<&String> = results.iter().flatten().collect();
        assert_eq!(completed, vec![&new]);
        assert!(replayed.iter().all(Option::is_none));
    }

    #[test]
    fn absurd_totals_are_ignored() {
        // Given a chunk claiming four billion parts, one claiming none, then a real one-chunk send
        let mut reassembler = Reassembler::default();
        let huge = DefsChunk {
            send_id: 5,
            index: 0,
            total: u32::MAX,
            data: "x".into(),
        };
        let empty = DefsChunk {
            send_id: 6,
            index: 0,
            total: 0,
            data: "x".into(),
        };
        let real = DefsChunk {
            send_id: 4,
            index: 0,
            total: 1,
            data: "{}".into(),
        };

        // When they are pushed in that order
        let results = [huge, empty, real].map(|c| reassembler.push(c));

        // Then the absurd ones are dropped without taking over, and the real send completes
        assert_eq!(results, [None, None, Some("{}".to_string())]);
    }
}
