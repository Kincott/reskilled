//! The areas of perks the player chose to see (Hearth, Home Ward, Safe Haven, Green Aura), drawn in the world as a
//! box with glowing edges and faint walls in the perk's colour. The box is the ground the server tests.

use horizon_client::mod_api::{Host, NativeWorldDraw};
use skills_core::area::{Around, area_box, area_of};
use skills_core::progress::{owns, switched_off};

use super::cache::{ClientCache, with};

/// A white texel of a vanilla GUI sprite (the blast furnace's flame, 24 by 16 pixels, white at 17 5): the quads
/// take their colour from it times their tint. UVs are in 16ths of the sprite's sides.
const WHITE_SPRITE: &str = "gui/sprites/container/blast_furnace/burn_progress";
const WHITE_UV: [f32; 4] = [11.64, 5.47, 11.69, 5.53];
/// The walls are faint, the edges bright and this thick.
const WALL_ALPHA: u8 = 34;
const EDGE_ALPHA: u8 = 220;
const EDGE_WIDTH: f32 = 0.08;
/// A standing player's eyes above their feet.
const EYE_HEIGHT: f64 = 1.62;

/// One box to draw: its lowest and highest corners in world coordinates, and its colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub rgb: [u8; 3],
}

/// One coloured quad, its corners relative to the camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub corners: [[f32; 3]; 4],
    pub color: [u8; 4],
}

/// The boxes to draw now: each shown perk the player owns and has switched on, around their home when it is in
/// `dimension`, or around the block at their feet (from their `eye` position).
pub fn shown_boxes(cache: &ClientCache, dimension: &str, eye: Option<[f64; 3]>) -> Vec<Shown> {
    let Some(defs) = &cache.defs else {
        return Vec::new();
    };
    let state = &cache.state;
    cache
        .settings
        .shown_areas
        .iter()
        .filter(|id| owns(state, id) && !switched_off(state, id))
        .filter_map(|id| defs.defs.perks.get(id).and_then(area_of))
        .filter_map(|area| {
            let centre = match area.around {
                Around::Home => match &cache.home {
                    Some((home_dimension, block)) if home_dimension == dimension => *block,
                    _ => return None,
                },
                Around::Player => {
                    let [x, y, z] = eye?;
                    [x, y - EYE_HEIGHT, z].map(|v| v.floor() as i32)
                }
            };
            let (min, max) = area_box(&area, centre);
            Some(Shown {
                min,
                max,
                rgb: area.rgb,
            })
        })
        .collect()
}

/// The quads of a box seen from `camera`: its six walls, faint, and its twelve edges as bright crossed strips, each
/// drawn from both sides.
pub fn quads(shown: &Shown, camera: [f64; 3]) -> Vec<Quad> {
    let at = |p: [f64; 3]| -> [f32; 3] { [0, 1, 2].map(|i| (p[i] - camera[i]) as f32) };
    let [r, g, b] = shown.rgb;
    let (lo, hi) = (shown.min, shown.max);
    let corner = |x: bool, y: bool, z: bool| {
        at([
            if x { hi[0] } else { lo[0] },
            if y { hi[1] } else { lo[1] },
            if z { hi[2] } else { lo[2] },
        ])
    };
    let mut out = Vec::new();
    let mut both_sides = |corners: [[f32; 3]; 4], color: [u8; 4]| {
        out.push(Quad { corners, color });
        let [a, b, c, d] = corners;
        out.push(Quad {
            corners: [d, c, b, a],
            color,
        });
    };
    let wall = [r, g, b, WALL_ALPHA];
    let walls = [
        [
            corner(false, false, false),
            corner(false, true, false),
            corner(true, true, false),
            corner(true, false, false),
        ],
        [
            corner(false, false, true),
            corner(true, false, true),
            corner(true, true, true),
            corner(false, true, true),
        ],
        [
            corner(false, false, false),
            corner(false, false, true),
            corner(false, true, true),
            corner(false, true, false),
        ],
        [
            corner(true, false, false),
            corner(true, true, false),
            corner(true, true, true),
            corner(true, false, true),
        ],
        [
            corner(false, false, false),
            corner(true, false, false),
            corner(true, false, true),
            corner(false, false, true),
        ],
        [
            corner(false, true, false),
            corner(false, true, true),
            corner(true, true, true),
            corner(true, true, false),
        ],
    ];
    for walls in walls {
        both_sides(walls, wall);
    }
    let edge = [r, g, b, EDGE_ALPHA];
    let half = f64::from(EDGE_WIDTH) / 2.0;
    // Each edge runs along one axis at a corner of the other two; its two strips lie across those two.
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for (at_u, at_v) in [
            (lo[u], lo[v]),
            (hi[u], lo[v]),
            (lo[u], hi[v]),
            (hi[u], hi[v]),
        ] {
            let point = |along: f64, du: f64, dv: f64| {
                let mut p = [0.0; 3];
                p[axis] = along;
                p[u] = at_u + du;
                p[v] = at_v + dv;
                at(p)
            };
            let (start, end) = (lo[axis], hi[axis]);
            both_sides(
                [
                    point(start, -half, 0.0),
                    point(end, -half, 0.0),
                    point(end, half, 0.0),
                    point(start, half, 0.0),
                ],
                edge,
            );
            both_sides(
                [
                    point(start, 0.0, -half),
                    point(end, 0.0, -half),
                    point(end, 0.0, half),
                    point(start, 0.0, half),
                ],
                edge,
            );
        }
    }
    out
}

/// The world drawing hook: every shown area's quads.
pub fn draw(host: &dyn Host, draw: &mut NativeWorldDraw) {
    let eye = host.client_state().and_then(|state| state.eye_position);
    let boxes = with(|cache| shown_boxes(cache, host.dimension(), eye));
    let camera = draw.camera;
    for shown in &boxes {
        for quad in quads(shown, camera) {
            host.world_quad(draw, WHITE_SPRITE, quad.corners, WHITE_UV, quad.color);
        }
    }
}

#[cfg(test)]
mod tests {
    use skills_core::progress::{PlayerState, SkillProgress};
    use skills_core::wire::ClientDefs;

    use super::*;
    use crate::test_host::exclusive;

    const WARD: &str = "reskilled:building_home_ward";
    const AURA: &str = "reskilled:gathering_foragers_pace";

    /// A cache knowing the two perks (Home Ward 24 blocks, Green Aura 5), the player owning both, home at
    /// 10 64 -3 in the overworld, and `shown` areas chosen.
    fn cache(shown: &[&str], off: &[&str]) -> ClientCache {
        let perk = |id: &str, skill: &str, effect: &str| {
            let json = format!(
                r#"{{"format": 1, "id": "{id}", "skill": "{skill}", "name": "n", "description": "d", "cost": 1,
                    "min_level": 1, "effects": [{effect}]}}"#
            );
            match skills_core::defs::parse_file(skills_core::defs::DefKind::Perk, &json) {
                Ok(skills_core::defs::DefFile::Perk(perk)) => (id.to_string(), perk),
                other => panic!("{other:?}"),
            }
        };
        let mut defs = skills_core::registry::Defs::default();
        defs.perks.extend([
            perk(
                WARD,
                "reskilled:building",
                r#"{"type": "home_ward", "radius": 24}"#,
            ),
            perk(
                AURA,
                "reskilled:gathering",
                r#"{"type": "crop_growth", "radius": 5, "interval_seconds": 10, "chance": 0.25}"#,
            ),
        ]);
        let mut state = PlayerState::default();
        for (skill, perk) in [("reskilled:building", WARD), ("reskilled:gathering", AURA)] {
            let progress = state
                .skills
                .entry(skill.into())
                .or_insert_with(SkillProgress::default);
            progress.perks.insert(perk.into());
            if off.contains(&perk) {
                progress.disabled.insert(perk.into());
            }
        }
        let mut cache = ClientCache::default();
        cache.defs = Some(ClientDefs {
            defs,
            labels: Default::default(),
        });
        cache.state = state;
        cache.home = Some(("minecraft:overworld".into(), [10, 64, -3]));
        cache.settings.shown_areas = shown.iter().map(|s| (*s).to_string()).collect();
        cache
    }

    #[test]
    fn only_shown_owned_switched_on_areas_are_drawn_where_their_centre_is() {
        let _serial = exclusive();
        let eye = Some([0.5, 71.62, 0.5]);

        // Given both shown; only the ward shown; nothing shown; both shown with the ward switched off; and the
        // player in the nether with both shown
        let both = shown_boxes(&cache(&[WARD, AURA], &[]), "minecraft:overworld", eye);
        let ward = shown_boxes(&cache(&[WARD], &[]), "minecraft:overworld", eye);
        let none = shown_boxes(&cache(&[], &[]), "minecraft:overworld", eye);
        let off = shown_boxes(&cache(&[WARD, AURA], &[WARD]), "minecraft:overworld", eye);
        let nether = shown_boxes(&cache(&[WARD, AURA], &[]), "minecraft:the_nether", eye);

        // Then the ward sits around home and the aura around the block at the player's feet (0 70 0); nothing is
        // drawn unless chosen, owned and on; and in the nether only the aura, which follows the player
        let ward_box = Shown {
            min: [-14.0, 40.0, -27.0],
            max: [35.0, 89.0, 22.0],
            rgb: ward[0].rgb,
        };
        assert_eq!(ward, vec![ward_box]);
        assert_eq!(both.len(), 2);
        assert!(
            both.iter()
                .any(|s| s.min == [-5.0, 69.0, -5.0] && s.max == [6.0, 72.0, 6.0])
        );
        assert!(none.is_empty());
        assert_eq!(off.len(), 1);
        assert_eq!(nether.len(), 1);
        assert_eq!(nether[0].min, [-5.0, 69.0, -5.0]);
    }

    #[test]
    fn a_box_is_six_faint_walls_and_twelve_bright_edges_seen_from_both_sides() {
        // Given a 2-block box from 0 0 0 seen from a camera at 100 50 100
        let shown = Shown {
            min: [0.0, 0.0, 0.0],
            max: [2.0, 2.0, 2.0],
            rgb: [90, 160, 255],
        };

        // When its quads are made
        let quads = quads(&shown, [100.0, 50.0, 100.0]);

        // Then there are 6 walls and 24 edge strips, each twice, faint and bright, all inside the box (relative to
        // the camera, edges reaching a strip's half width out)
        let faint = quads.iter().filter(|q| q.color[3] == WALL_ALPHA).count();
        let bright = quads.iter().filter(|q| q.color[3] == EDGE_ALPHA).count();
        assert_eq!((faint, bright), (12, 48));
        let reach = EDGE_WIDTH / 2.0 + 1e-4;
        for corner in quads.iter().flat_map(|q| q.corners) {
            let world = [corner[0] + 100.0, corner[1] + 50.0, corner[2] + 100.0];
            assert!(
                world.iter().all(|v| *v >= -reach && *v <= 2.0 + reach),
                "{world:?}"
            );
        }
    }
}
