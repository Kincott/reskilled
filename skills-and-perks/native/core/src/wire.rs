//! The save format of a player's state and the messages between client and server.
//!
//! Everything is JSON. Definitions can be bigger than one network message, so the server sends them as numbered
//! chunks of one send and the client puts the newest complete send back together.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::defs::DefError;
use crate::locks::{Requirements, labels_by_level};
use crate::progress::{PlayerState, Request, STATE_FORMAT};
use crate::registry::Defs;

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
}

/// Server to client, apart from definition chunks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ToClient {
    /// Requirements per item id, answering a `Need`.
    Reqs { items: Vec<(String, Requirements)> },
    /// Open the skills screen.
    Open,
    /// A lock refused an action: the client plays its sound.
    Refused { skill: String, level: u32 },
    /// The player this client plays as, so it can read its own synced state.
    You { player: u128 },
}

/// What clients get of the definitions: no lock matchers (only the server can test tags), and the lock labels per
/// skill and level for the unlock preview.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientDefs {
    pub defs: Defs,
    pub labels: BTreeMap<String, BTreeMap<u32, Vec<String>>>,
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
    let value: Value = serde_json::from_str(json).map_err(|e| refused(e.to_string()))?;
    let format = value.get(FORMAT_FIELD).cloned().unwrap_or(Value::Null);
    if format.as_u64() != Some(u64::from(STATE_FORMAT)) {
        return Err(refused(format!(
            "unsupported format {format} (this version reads format {STATE_FORMAT})"
        )));
    }
    serde_json::from_value(value).map_err(|e| refused(e.to_string()))
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
            locks: Vec::new(),
            ..defs.clone()
        },
        labels,
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

    const MINING: &str = "skills_and_perks:mining";
    const LIMIT: usize = 30_000;

    fn mining() -> SkillDef {
        SkillDef {
            id: MINING.into(),
            priority: 0,
            name: "skills_and_perks.skill.mining".into(),
            icon: "minecraft:iron_pickaxe".into(),
            max_level: None,
            cost: None,
            points_every: None,
        }
    }

    #[test]
    fn state_round_trips() {
        // Given a state with a level, spent and granted points, a perk and a focus
        let mut state = PlayerState {
            focus: Some("skills_and_perks:working".into()),
            ..PlayerState::default()
        };
        state.focus_chosen_before = true;
        state.skills.insert(
            MINING.into(),
            SkillProgress {
                level: 12,
                points_spent: 2,
                bonus_points: 1,
                perks: ["skills_and_perks:mining_sure_grip".to_string()].into(),
            },
        );

        // When it is encoded and decoded
        let decoded = decode_state(&encode_state(&state));

        // Then it comes back equal
        assert_eq!(decoded, Ok(state));
    }

    #[test]
    fn decode_refuses_format_2() {
        // Given a state from a later format, and text that is not a state
        let later = r#"{"format": 2, "skills": {}, "focus": null, "focus_chosen_before": false}"#;

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
            }),
            ToServer::Act(Request::BuyPerk {
                perk: "skills_and_perks:mining_sure_grip".into(),
            }),
            ToServer::Need {
                items: vec!["minecraft:diamond_pickaxe".into()],
            },
        ];
        let to_client = vec![
            ToClient::Reqs {
                items: vec![("minecraft:diamond_pickaxe".into(), requirements)],
            },
            ToClient::Open,
            ToClient::Refused {
                skill: MINING.into(),
                level: 16,
            },
            ToClient::You {
                player: u128::MAX - 7,
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
    fn client_defs_have_no_locks_but_have_labels() {
        // Given definitions with a labelled lock on the diamond pickaxe
        let defs = Defs {
            skills: [(MINING.into(), mining())].into(),
            locks: vec![LockEntry {
                target: Target::Item(Matcher::Id("minecraft:diamond_pickaxe".into())),
                actions: vec![Action::Break],
                requires: [(MINING.to_string(), 16)].into(),
                label: Some("skills_and_perks.lock.diamond_tools".into()),
                replace: false,
            }],
            ..Defs::default()
        };

        // When they are prepared for clients
        let json = client_defs_json(&defs);
        let client: ClientDefs = serde_json::from_str(&json).unwrap();

        // Then the locks are gone, the skills and the labels by level are there
        assert!(client.defs.locks.is_empty());
        assert!(!json.contains("minecraft:diamond_pickaxe"), "{json}");
        assert_eq!(client.defs.skills[MINING], mining());
        assert_eq!(
            client.labels[MINING][&16],
            vec!["skills_and_perks.lock.diamond_tools".to_string()]
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
