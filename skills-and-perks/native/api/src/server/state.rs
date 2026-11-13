//! Each player's skills, saved with the player (kept through death) and synced to their own client.

use std::collections::BTreeMap;
use std::sync::Mutex;

use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, Compound, Holder, Host, Tag,
};
use skills_core::progress::{PlayerState, on_death};
use skills_core::wire::{decode_state, encode_state};

use super::data::current;
use super::lock;
use super::queue::{self, Job};

/// The one tag the saved state is kept in, as the JSON `encode_state` writes.
const JSON_TAG: &str = "json";

/// A player's skills, perks and focus.
pub static STATE: AttachmentType<PlayerState> = AttachmentType {
    namespace: "skills_and_perks",
    name: "state",
    default: PlayerState::default,
    codec: Some(AttachmentCodec {
        encode: encode_attachment,
        decode: decode_attachment,
    }),
    copy_on_death: true,
    sync: true,
};

fn encode_attachment(state: &PlayerState) -> Result<Compound, AttachmentError> {
    Ok([(
        JSON_TAG.into(),
        Tag::String(encode_state(state).as_str().into()),
    )]
    .into())
}

fn decode_attachment(saved: &Compound) -> Result<PlayerState, AttachmentError> {
    let Some(Tag::String(json)) = saved.get(&JSON_TAG.into()) else {
        return Err(AttachmentError::Decode);
    };
    decode_state(&json.to_string_lossy()).map_err(|error| {
        log::warn!("Skills & Perks cannot read a saved state, left as it is: {error}");
        AttachmentError::Decode
    })
}

/// The players on the server now and their names, by login and logout.
static ONLINE: Mutex<BTreeMap<u128, String>> = Mutex::new(BTreeMap::new());

/// The player's state. An unreadable save is an error, never a fresh state that would be saved over it.
pub fn state_of(host: &mut dyn Host, player: u128) -> Result<PlayerState, AttachmentError> {
    STATE.get(host, Holder::Player(player))
}

pub fn set_state(
    host: &mut dyn Host,
    player: u128,
    state: PlayerState,
) -> Result<(), AttachmentError> {
    STATE.set(host, Holder::Player(player), state)
}

pub fn online_players() -> Vec<u128> {
    lock(&ONLINE).keys().copied().collect()
}

/// The online player with this name (any case, as Minecraft names are), and the name as they spell it.
pub fn player_named(name: &str) -> Option<(u128, String)> {
    let online = lock(&ONLINE);
    let (id, spelled) = online
        .iter()
        .find(|(_, spelled)| spelled.eq_ignore_ascii_case(name))?;
    Some((*id, spelled.clone()))
}

pub fn logged_in(player: u128, name: &str) {
    lock(&ONLINE).insert(player, name.to_string());
    queue::refresh(player);
}

pub fn logged_out(player: u128) {
    lock(&ONLINE).remove(&player);
    queue::forget(player);
}

/// Takes the settings' death penalty off a respawned player, then brings their perks' attributes in line.
pub fn apply_death(host: &mut dyn Host, player: u128) {
    let Some(data) = current() else { return };
    let applied = state_of(host, player)
        .and_then(|state| set_state(host, player, on_death(&data.defs, &state)));
    match applied {
        Ok(()) => queue::queue(Job::SyncAttributes(player)),
        Err(error) => log::warn!(
            "Skills & Perks could not apply the death penalty to {player:032x}: {error:?}"
        ),
    }
}

#[cfg(test)]
pub(crate) fn reset() {
    lock(&ONLINE).clear();
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::{Holder, Tag, dispatch_player_clone, dispatch_player_logged_in};
    use serde_json::json;
    use skills_core::progress::{SkillProgress, level};

    use super::*;
    use crate::server::data::current;
    use crate::server::queue::{Job, run, take_queued};
    use crate::server::test_host::{TestHost, exclusive, mods};

    const MINING: &str = "skills_and_perks:mining";
    const PLAYER: u128 = 0x1234;

    fn mining_at(level: u32) -> PlayerState {
        let mut state = PlayerState::default();
        state.skills.insert(
            MINING.into(),
            SkillProgress {
                level,
                ..SkillProgress::default()
            },
        );
        state
    }

    #[test]
    fn state_round_trips_through_the_attachment_codec() {
        // Given a player's state
        let _serial = exclusive();
        let mut host = TestHost::default();
        let state = mining_at(12);

        // When it is saved, then read back, and its saved form is looked at
        set_state(&mut host, PLAYER, state.clone()).unwrap();
        let read = state_of(&mut host, PLAYER).unwrap();
        let saved = (STATE.codec.as_ref().unwrap().encode)(&state).unwrap();

        // Then it comes back equal, stored as one JSON string tag
        assert_eq!(read, state);
        assert!(matches!(saved.get(&"json".into()), Some(Tag::String(_))));
        assert_eq!(saved.len(), 1);
        assert!(STATE.copy_on_death && STATE.sync);
    }

    #[test]
    fn unreadable_save_is_never_replaced() {
        // Given a player whose saved state is from a later format
        let _serial = exclusive();
        let mut host = TestHost::default();
        let later = json!({"format": 2, "skills": {}, "focus": null, "focus_chosen_before": false})
            .to_string();
        let value: Compound = [("json".into(), Tag::String(later.as_str().into()))].into();
        let saved: Compound = [("value".into(), Tag::Compound(value))].into();
        let names: Compound = [("state".into(), Tag::Compound(saved))].into();
        let raw: Compound = [("skills_and_perks".into(), Tag::Compound(names))].into();
        host.attachments.insert(
            Holder::Player(PLAYER),
            horizon_client::mod_api::Attachments::from_nbt(raw),
        );

        // When it is read
        let read = state_of(&mut host, PLAYER);

        // Then it is an error, not a fresh default that would be saved over it
        assert!(read.is_err(), "{read:?}");
    }

    #[test]
    fn login_queues_defs_attributes_and_equipment() {
        // Given a player joining
        let _serial = exclusive();
        let mut host = TestHost::default();

        // When they log in
        dispatch_player_logged_in(&mut host, &mods(), PLAYER, "Ann");

        // Then they get definitions, an attribute sync and an equipment check, and count as online
        assert_eq!(
            take_queued(),
            vec![
                Job::SendDefs(PLAYER),
                Job::SyncAttributes(PLAYER),
                Job::CheckEquipment(PLAYER)
            ]
        );
        assert_eq!(online_players(), vec![PLAYER]);
    }

    #[test]
    fn death_applies_the_penalty_setting() {
        // Given a 2-level death penalty and a player at Mining 10
        let _serial = exclusive();
        let mut host = TestHost::default();
        let mining = json!({"format": 1, "id": MINING, "name": "n", "icon": "minecraft:stone"});
        let settings = json!({"format": 1, "death_penalty_levels": 2});
        crate::server::data::tests::load(
            &mut host,
            &[
                ("skills", vec![(MINING, mining)]),
                ("settings", vec![("skills_and_perks:hard", settings)]),
            ],
        );
        set_state(&mut host, PLAYER, mining_at(10)).unwrap();
        take_queued();

        // When they die and respawn, then return from the End, and the tick runs after each
        dispatch_player_clone(&mut host, &mods(), PLAYER, "Ann", true);
        let after_death = take_queued();
        run(&mut host, after_death.clone());
        let after_run = take_queued();
        dispatch_player_clone(&mut host, &mods(), PLAYER, "Ann", false);

        // Then the death costs 2 levels and asks for an attribute sync, and the End exit costs nothing
        let defs = &current().unwrap().defs;
        assert_eq!(after_death, vec![Job::ApplyDeath(PLAYER)]);
        assert_eq!(
            level(&state_of(&mut host, PLAYER).unwrap(), defs, MINING),
            8
        );
        assert_eq!(after_run, vec![Job::SyncAttributes(PLAYER)]);
        assert_eq!(take_queued(), vec![]);
    }
}
