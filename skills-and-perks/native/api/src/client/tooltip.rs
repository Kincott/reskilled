//! Requirement lines in item tooltips: one per action and skill, red while unmet, green once met.

use horizon_client::mod_api::Text;
use skills_core::defs::Action;
use skills_core::progress::level;

use super::cache::with;
use crate::text::{
    ACTION_ATTACK, ACTION_BREAK, ACTION_EQUIP, ACTION_INTERACT, ACTION_MOUNT, ACTION_OPEN,
    ACTION_PLACE, ACTION_USE, Message, REQUIRES, number, say, skill_name,
};

/// Vanilla's red (255, 85, 85) and green (85, 255, 85) in Horizon's tooltip colour packing, `rgba(r, g, b, a)` =
/// a<<24 | b<<16 | g<<8 | r (`horizon-client-render/src/hud.rs` at the pinned commit).
pub const UNMET: u32 = 0xFF55_55FF;
pub const MET: u32 = 0xFF55_FF55;

/// The lines to add under the item's tooltip; none until the server has answered for it (it is asked for then).
pub fn requirement_lines(item: &str) -> Vec<(Text, u32)> {
    with(|cache| {
        if cache.defs.is_none() {
            return Vec::new();
        }
        if !cache.reqs.contains_key(item) {
            cache.missed(item);
            return Vec::new();
        }
        let (Some(defs), Some(reqs)) = (&cache.defs, cache.reqs.get(item)) else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        for (action, skills) in &reqs.by_action {
            for (skill, needed) in skills {
                let met = level(&cache.state, &defs.defs, skill) >= *needed;
                if met && cache.settings.hide_met_requirements {
                    continue;
                }
                let action = say(action_name(*action), &[]);
                let line = say(
                    &REQUIRES,
                    &[action, skill_name(&defs.defs, skill), number(*needed)],
                );
                lines.push((line, if met { MET } else { UNMET }));
            }
        }
        lines
    })
}

fn action_name(action: Action) -> &'static Message {
    match action {
        Action::Use => &ACTION_USE,
        Action::Attack => &ACTION_ATTACK,
        Action::Break => &ACTION_BREAK,
        Action::Equip => &ACTION_EQUIP,
        Action::Open => &ACTION_OPEN,
        Action::Place => &ACTION_PLACE,
        Action::Mount => &ACTION_MOUNT,
        Action::Interact => &ACTION_INTERACT,
    }
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::{ItemStack, Text, dispatch_tooltip};
    use skills_core::defs::{Action, SkillDef};
    use skills_core::locks::Requirements;
    use skills_core::progress::{PlayerState, SkillProgress};
    use skills_core::registry::Defs;
    use skills_core::wire::{ClientDefs, ToServer};

    use super::*;
    use crate::client::cache::with;
    use crate::test_host::{TestHost, exclusive, mods};

    const MINING: &str = "skills_and_perks:mining";
    const MELEE: &str = "skills_and_perks:melee";
    const PICKAXE: &str = "minecraft:diamond_pickaxe";

    /// Mining and Melee known, the pickaxe needing Mining 16 to break and Melee 8 to attack, and the player at
    /// Mining 3 and Melee 10.
    fn known() {
        let skill = |id: &str, name: &str| SkillDef {
            id: id.into(),
            priority: 0,
            name: name.into(),
            icon: "minecraft:stone".into(),
            max_level: None,
            cost: None,
            points_every: None,
        };
        let defs = Defs {
            skills: [
                (
                    MINING.into(),
                    skill(MINING, "skills_and_perks.skill.mining"),
                ),
                (MELEE.into(), skill(MELEE, "skills_and_perks.skill.melee")),
            ]
            .into(),
            ..Defs::default()
        };
        let pickaxe = Requirements {
            by_action: [
                (Action::Attack, [(MELEE.to_string(), 8)].into()),
                (Action::Break, [(MINING.to_string(), 16)].into()),
            ]
            .into(),
        };
        let mut state = PlayerState::default();
        state.skills.insert(
            MINING.into(),
            SkillProgress {
                level: 3,
                ..SkillProgress::default()
            },
        );
        state.skills.insert(
            MELEE.into(),
            SkillProgress {
                level: 10,
                ..SkillProgress::default()
            },
        );
        with(|cache| {
            cache.defs = Some(ClientDefs {
                defs,
                labels: Default::default(),
            });
            cache.reqs.insert(PICKAXE.into(), pickaxe);
            cache.state = state;
        });
    }

    fn tooltip(host: &TestHost, item: &str) -> Vec<(String, u32)> {
        let stack = ItemStack {
            item: item.into(),
            count: 1,
            damage: 0,
            extra: None,
        };
        let mut lines: Vec<(Text, u32)> = vec![(Text::literal("Diamond Pickaxe"), WHITE)];
        dispatch_tooltip(host, &mods(), &stack, &mut lines);
        lines
            .into_iter()
            .map(|(text, color)| (text.plain(), color))
            .collect()
    }

    const WHITE: u32 = 0xFFFF_FFFF;

    #[test]
    fn tooltip_shows_unmet_in_red_and_met_in_green() {
        // Given the pickaxe's requirements and the player's levels
        let _serial = exclusive();
        let host = TestHost::default();
        known();

        // When its tooltip is drawn
        let lines = tooltip(&host, PICKAXE);

        // Then the game's lines stay first, then one line per action: met in green, unmet in red
        assert_eq!(
            lines,
            vec![
                ("Diamond Pickaxe".to_string(), WHITE),
                ("Attack: Melee 8".to_string(), MET),
                ("Break: Mining 16".to_string(), UNMET),
            ]
        );
    }

    #[test]
    fn tooltip_hides_met_when_set() {
        // Given the client setting that hides met requirements
        let _serial = exclusive();
        let host = TestHost::default();
        known();
        with(|cache| cache.settings.hide_met_requirements = true);

        // When the tooltip is drawn
        let lines = tooltip(&host, PICKAXE);

        // Then only the unmet line is added
        assert_eq!(
            lines,
            vec![
                ("Diamond Pickaxe".to_string(), WHITE),
                ("Break: Mining 16".to_string(), UNMET)
            ]
        );
    }

    #[test]
    fn unlocked_and_unknown_items_add_nothing_but_unknown_ones_are_asked_for() {
        // Given a stick known to be unlocked, and a sword not asked about yet
        let _serial = exclusive();
        let mut host = TestHost::default();
        known();
        with(|cache| {
            cache
                .reqs
                .insert("minecraft:stick".into(), Requirements::default());
        });

        // When their tooltips are drawn, then a tick passes
        let stick = tooltip(&host, "minecraft:stick");
        let sword = tooltip(&host, "minecraft:diamond_sword");
        horizon_client::mod_api::dispatch_tick(&mut host, &mods());

        // Then neither gets a line, and the sword is asked for
        assert_eq!((stick.len(), sword.len()), (1, 1));
        let asked: Vec<ToServer> = host
            .sent_to_server
            .iter()
            .map(|(_, data)| serde_json::from_slice(data).unwrap())
            .collect();
        assert_eq!(
            asked,
            vec![ToServer::Need {
                items: vec!["minecraft:diamond_sword".into()]
            }]
        );
    }

    #[test]
    fn colors_are_vanilla_red_and_green() {
        // Given Horizon's tooltip colour packing, `rgba(r, g, b, a)` = a<<24 | b<<16 | g<<8 | r
        // (horizon-client-render/src/hud.rs at the pinned commit)
        // When the two colours are compared with vanilla's red (255, 85, 85) and green (85, 255, 85)
        // Then they match
        assert_eq!(UNMET, 0xFF55_55FF);
        assert_eq!(MET, 0xFF55_FF55);
    }
}
