//! The ground a perk acts on, for showing it in the world: around the player's home (Hearth, Home Ward, Safe Haven)
//! or around the player (Green Aura). The boxes are the ones the server tests: around home, every block within
//! `radius` of the home block on each axis; around the player, the crops within `radius` across and one block up
//! or down from the block at their feet.

use crate::defs::{Effect, PerkDef};

/// What the area is centred on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Around {
    /// The player's respawn point (bed or respawn anchor).
    Home,
    /// The block at the player's feet.
    Player,
}

/// A perk's area: its centre, its reach in blocks, and its colour as RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub around: Around,
    pub radius: u32,
    pub rgb: [u8; 3],
}

const HEARTH: [u8; 3] = [255, 150, 60];
const HOME_WARD: [u8; 3] = [90, 160, 255];
const SAFE_HAVEN: [u8; 3] = [120, 230, 120];
const GREEN_AURA: [u8; 3] = [200, 240, 90];

/// The area of the perk's first effect that has one; None for a perk that acts nowhere in particular.
pub fn area_of(perk: &PerkDef) -> Option<Area> {
    perk.effects.iter().find_map(|effect| {
        let (around, radius, rgb) = match effect {
            Effect::NearHome { radius, .. } => (Around::Home, *radius, HEARTH),
            Effect::HomeWard { radius } => (Around::Home, *radius, HOME_WARD),
            Effect::SafeHaven { radius } => (Around::Home, *radius, SAFE_HAVEN),
            Effect::CropGrowth { radius, .. } => (Around::Player, *radius, GREEN_AURA),
            _ => return None,
        };
        Some(Area {
            around,
            radius,
            rgb,
        })
    })
}

/// The box the area covers around the centre block, as its lowest and highest corners in world coordinates.
pub fn area_box(area: &Area, centre: [i32; 3]) -> ([f64; 3], [f64; 3]) {
    let r = f64::from(area.radius);
    let [x, y, z] = centre.map(f64::from);
    match area.around {
        Around::Home => (
            [x - r, y - r, z - r],
            [x + r + 1.0, y + r + 1.0, z + r + 1.0],
        ),
        Around::Player => ([x - r, y - 1.0, z - r], [x + r + 1.0, y + 2.0, z + r + 1.0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::{DefFile, DefKind, parse_file};

    fn perk(effect: &str) -> PerkDef {
        let json = format!(
            r#"{{"format": 1, "id": "a:p", "skill": "a:s", "name": "n", "description": "d", "cost": 1,
                "min_level": 1, "effects": [{effect}]}}"#
        );
        match parse_file(DefKind::Perk, &json) {
            Ok(DefFile::Perk(perk)) => perk,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn perks_that_act_on_ground_have_an_area_and_the_others_none() {
        // Given a home ward, a near-home effect, a crop growth and an attribute perk
        let ward = perk(r#"{"type": "home_ward", "radius": 24}"#);
        let hearth = perk(
            r#"{"type": "near_home", "radius": 16, "effect": "minecraft:regeneration", "amplifier": 0}"#,
        );
        let aura =
            perk(r#"{"type": "crop_growth", "radius": 5, "interval_seconds": 10, "chance": 0.25}"#);
        let speed = perk(
            r#"{"type": "attribute", "attribute": "minecraft:movement_speed", "amount": 0.1, "operation": "add_value"}"#,
        );

        // When their areas are read
        let areas = [&ward, &hearth, &aura, &speed].map(area_of);

        // Then the first two sit around home, the crop growth around the player, and the attribute has none
        assert_eq!(
            areas[0].map(|a| (a.around, a.radius)),
            Some((Around::Home, 24))
        );
        assert_eq!(
            areas[1].map(|a| (a.around, a.radius)),
            Some((Around::Home, 16))
        );
        assert_eq!(
            areas[2].map(|a| (a.around, a.radius)),
            Some((Around::Player, 5))
        );
        assert_eq!(areas[3], None);
        assert_ne!(areas[0].unwrap().rgb, areas[1].unwrap().rgb);
    }

    #[test]
    fn the_box_is_the_ground_the_server_tests() {
        // Given a 24-block home area and a 5-block area around the player
        let home = Area {
            around: Around::Home,
            radius: 24,
            rgb: HOME_WARD,
        };
        let player = Area {
            around: Around::Player,
            radius: 5,
            rgb: GREEN_AURA,
        };

        // When their boxes around block 10 64 -3 are read
        let (home_box, player_box) = (
            area_box(&home, [10, 64, -3]),
            area_box(&player, [10, 64, -3]),
        );

        // Then home covers 24 blocks each way from the block (the block's edges, so 49 blocks across), and the
        // player's from one block below their feet to one above, 5 across each way
        assert_eq!(home_box, ([-14.0, 40.0, -27.0], [35.0, 89.0, 22.0]));
        assert_eq!(player_box, ([5.0, 63.0, -8.0], [16.0, 66.0, 3.0]));
    }
}
