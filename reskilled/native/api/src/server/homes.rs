//! Each player's home: their respawn point (a bed or a respawn anchor), as the game last told it. Kept on the
//! world, since Horizon gives mods the point only when it changes; perks that act near home read it here.

use std::collections::BTreeMap;
use std::sync::Mutex;

use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, Compound, Holder, Host, Tag,
};

use super::lock;

const JSON_TAG: &str = "json";

/// A home: the dimension and the block of the bed or anchor.
pub type Home = (String, [i32; 3]);

/// Every player's home, by player.
type Homes = BTreeMap<u128, Home>;

static SAVED: AttachmentType<Homes> = AttachmentType {
    namespace: "reskilled",
    name: "homes",
    default: Homes::new,
    codec: Some(AttachmentCodec { encode, decode }),
    copy_on_death: false,
    sync: false,
};

fn encode(homes: &Homes) -> Result<Compound, AttachmentError> {
    let json = serde_json::to_string(homes).expect("homes always encode");
    Ok([(JSON_TAG.into(), Tag::String(json.as_str().into()))].into())
}

fn decode(saved: &Compound) -> Result<Homes, AttachmentError> {
    let Some(Tag::String(json)) = saved.get(&JSON_TAG.into()) else {
        return Err(AttachmentError::Decode);
    };
    serde_json::from_str(&json.to_string_lossy()).map_err(|error| {
        log::warn!("Reskilled cannot read the saved homes: {error}");
        AttachmentError::Decode
    })
}

/// The homes read from the world, once; None until then.
static CURRENT: Mutex<Option<Homes>> = Mutex::new(None);

/// Reads the homes from the world if this server has not yet.
pub fn ensure_loaded(host: &mut dyn Host) {
    let mut current = lock(&CURRENT);
    if current.is_none() {
        *current = Some(SAVED.get(host, Holder::Level).unwrap_or_default());
    }
}

/// The player's respawn point changed (`death_reset` clears it).
pub fn set(host: &mut dyn Host, player: u128, dimension: &str, pos: [i32; 3], cause: &str) {
    ensure_loaded(host);
    let homes = {
        let mut current = lock(&CURRENT);
        let homes = current.get_or_insert_with(Homes::new);
        if cause == "death_reset" {
            homes.remove(&player);
        } else {
            homes.insert(player, (dimension.to_string(), pos));
        }
        homes.clone()
    };
    if let Err(error) = SAVED.set(host, Holder::Level, homes) {
        log::warn!("Reskilled could not keep the homes: {error:?}");
    }
}

/// The player's home, once the homes are read.
pub fn home_of(player: u128) -> Option<Home> {
    lock(&CURRENT).as_ref()?.get(&player).cloned()
}

/// Whether `at` in `dimension` lies within `radius` blocks of `home` (a cube around it).
pub fn near(home: &Home, dimension: &str, at: [f64; 3], radius: u32) -> bool {
    let r = f64::from(radius);
    home.0 == dimension
        && (0..3).all(|axis| (at[axis] - (f64::from(home.1[axis]) + 0.5)).abs() <= r + 0.5)
}

/// Forgets what this module keeps for the running world, as the server stops.
pub(crate) fn reset() {
    *lock(&CURRENT) = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_host::{TestHost, exclusive};

    const PLAYER: u128 = 0x1234;

    #[test]
    fn a_home_is_kept_with_the_world_and_cleared_by_a_reset() {
        // Given a player who set their bed at 10 64 10
        let _serial = exclusive();
        let mut host = TestHost::default();
        set(
            &mut host,
            PLAYER,
            "minecraft:overworld",
            [10, 64, 10],
            "bed",
        );

        // When the server stops and starts again, then their point is reset by death with no bed
        reset();
        ensure_loaded(&mut host);
        let after_restart = home_of(PLAYER);
        set(
            &mut host,
            PLAYER,
            "minecraft:overworld",
            [0, 0, 0],
            "death_reset",
        );

        // Then the bed was kept, and the reset forgets it
        assert_eq!(
            after_restart,
            Some(("minecraft:overworld".to_string(), [10, 64, 10]))
        );
        assert_eq!(home_of(PLAYER), None);
    }

    #[test]
    fn near_means_within_the_radius_in_the_same_dimension() {
        // Given a home at 10 64 10 in the overworld
        let home = ("minecraft:overworld".to_string(), [10, 64, 10]);

        // When places 16 and 17 blocks away, and one in the nether, are checked with a radius of 16
        let close = near(&home, "minecraft:overworld", [26.5, 64.0, 10.5], 16);
        let far = near(&home, "minecraft:overworld", [27.6, 64.0, 10.5], 16);
        let nether = near(&home, "minecraft:the_nether", [10.5, 64.0, 10.5], 16);

        // Then only the first is near
        assert_eq!((close, far, nether), (true, false, false));
    }
}
