//! What an item's tooltip says of its locks: a heading, then one line per action and skill with a tick or a
//! cross, red while unmet (with the player's own level) and green once met. A locked item's tooltip also gets a red
//! border.

use horizon_client::mod_api::Text;
use skills_core::defs::Action;
use skills_core::progress::level;

use super::cache::with;
use crate::text::{
    ACTION_ATTACK, ACTION_BREAK, ACTION_EQUIP, ACTION_INTERACT, ACTION_MOUNT, ACTION_OPEN,
    ACTION_PLACE, ACTION_USE, Message, REQUIREMENTS, REQUIRES, YOU_HAVE, number, say, skill_name,
};

/// A tooltip line's colour that keeps the text's own colours.
pub const OWN: u32 = u32::MAX;
/// Vanilla's text colours, as a text's style takes them.
pub const UNMET: u32 = 0xFF_5555;
pub const MET: u32 = 0x55_FF55;
const HEADING: u32 = 0xAA_AAAA;
const ASIDE: u32 = 0x55_5555;
/// Vanilla's font has both marks.
const TICK: &str = "\u{2714}";
const CROSS: &str = "\u{2718}";
/// A locked item's tooltip border, top and bottom, as ARGB.
const LOCKED_BORDER: [u32; 2] = [0xFFE0_5A4A, 0xFF7A_2018];

/// The lines to add under the item's tooltip; none until the server has answered for it (it is asked for then),
/// and none for an item nothing locks.
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
                let have = level(&cache.state, &defs.defs, skill);
                let met = have >= *needed;
                if met && cache.settings.hide_met_requirements {
                    continue;
                }
                let (mark, colour) = if met { (TICK, MET) } else { (CROSS, UNMET) };
                let parts = [
                    say(action_name(*action), &[]),
                    skill_name(&defs.defs, skill).with_color(colour),
                    number(*needed).with_color(colour),
                ];
                let mut line = Text::colored(format!("{mark} "), colour)
                    .append(say(&REQUIRES, &parts).with_color(HEADING));
                if !met && cache.settings.show_own_level {
                    let own = say(&YOU_HAVE, &[number(have)]).with_color(ASIDE);
                    line = line.append(Text::literal(" ")).append(own);
                }
                lines.push((line, OWN));
            }
        }
        if !lines.is_empty() {
            let heading = say(&REQUIREMENTS, &[]).with_color(HEADING);
            lines.splice(0..0, [(Text::literal(""), OWN), (heading, OWN)]);
        }
        lines
    })
}

/// The border colours, top and bottom, of the tooltip of an item the player cannot fully use yet.
pub fn locked_border(item: &str) -> Option<[u32; 2]> {
    with(|cache| cache.lacks(item, |_| true)).then_some(LOCKED_BORDER)
}

pub(super) fn action_name(action: Action) -> &'static Message {
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

    const MINING: &str = "reskilled:mining";
    const MELEE: &str = "reskilled:melee";
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
            icon_levels: Default::default(),
        };
        let defs = Defs {
            skills: [
                (MINING.into(), skill(MINING, "reskilled.skill.mining")),
                (MELEE.into(), skill(MELEE, "reskilled.skill.melee")),
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

    fn tooltip_lines(host: &TestHost, item: &str) -> Vec<(Text, u32)> {
        let stack = ItemStack {
            item: item.into(),
            count: 1,
            damage: 0,
            extra: None,
        };
        let mut lines: Vec<(Text, u32)> = vec![(Text::literal("Diamond Pickaxe"), WHITE)];
        dispatch_tooltip(host, &mods(), &stack, &mut lines);
        lines
    }

    fn tooltip(host: &TestHost, item: &str) -> Vec<String> {
        tooltip_lines(host, item)
            .into_iter()
            .map(|(text, _)| text.plain())
            .collect()
    }

    /// The colour of the part of the line that holds `part`.
    fn colour_of(line: &Text, part: &str) -> Option<u32> {
        line.spans
            .iter()
            .find(|span| span.text.contains(part))?
            .style
            .color
    }

    const WHITE: u32 = 0xFFFF_FFFF;

    #[test]
    fn tooltip_lists_each_requirement_with_a_tick_or_a_cross() {
        // Given the pickaxe's requirements and the player's levels (Melee 10, Mining 3)
        let _serial = exclusive();
        let host = TestHost::default();
        known();

        // When its tooltip is drawn
        let lines = tooltip_lines(&host, PICKAXE);
        let text: Vec<String> = lines.iter().map(|(text, _)| text.plain()).collect();

        // Then the game's lines stay first; then a gap, a heading, and one line per action: the met one ticked,
        // the unmet one crossed with the player's own level
        assert_eq!(
            text,
            vec![
                "Diamond Pickaxe",
                "",
                "Requirements",
                "\u{2714} Attack: Melee 8",
                "\u{2718} Break: Mining 16 (you: 3)",
            ]
        );
        assert_eq!(lines[0].1, WHITE);
        assert!(lines[1..].iter().all(|(_, colour)| *colour == OWN));
    }

    #[test]
    fn met_requirements_are_green_and_unmet_ones_red() {
        // Given the pickaxe's requirements and the player's levels
        let _serial = exclusive();
        let host = TestHost::default();
        known();

        // When its tooltip is drawn
        let lines = tooltip_lines(&host, PICKAXE);
        let (met, unmet) = (&lines[3].0, &lines[4].0);

        // Then the met line's mark, skill and level are vanilla's green, the unmet one's vanilla's red, and the
        // action and the player's own level stay gray
        assert_eq!((MET, UNMET), (0x55_FF55, 0xFF_5555));
        for part in ["\u{2714}", "Melee", "8"] {
            assert_eq!(colour_of(met, part), Some(MET), "{part}");
        }
        for part in ["\u{2718}", "Mining", "16"] {
            assert_eq!(colour_of(unmet, part), Some(UNMET), "{part}");
        }
        assert_eq!(colour_of(unmet, "Break"), Some(0xAA_AAAA));
        assert_eq!(colour_of(unmet, "you"), Some(0x55_5555));
    }

    #[test]
    fn tooltip_hides_met_when_set() {
        // Given the client setting that hides met requirements
        let _serial = exclusive();
        let host = TestHost::default();
        known();
        with(|cache| cache.settings.hide_met_requirements = true);

        // When the tooltip is drawn, for the pickaxe and then with every requirement met
        let lines = tooltip(&host, PICKAXE);
        with(|cache| cache.state.skills.get_mut(MINING).unwrap().level = 16);
        let all_met = tooltip(&host, PICKAXE);

        // Then only the unmet line is listed, and nothing at all once none is left
        assert_eq!(
            lines,
            vec![
                "Diamond Pickaxe",
                "",
                "Requirements",
                "\u{2718} Break: Mining 16 (you: 3)"
            ]
        );
        assert_eq!(all_met, vec!["Diamond Pickaxe"]);
    }

    #[test]
    fn the_players_own_level_can_be_left_out() {
        // Given a player who turned off their own level in tooltips
        let _serial = exclusive();
        let host = TestHost::default();
        known();
        with(|cache| cache.settings.show_own_level = false);

        // When the pickaxe's tooltip is drawn
        let lines = tooltip(&host, PICKAXE);

        // Then the unmet line has no "(you: ...)"
        assert_eq!(
            lines.last().map(String::as_str),
            Some("\u{2718} Break: Mining 16")
        );
    }

    #[test]
    fn a_locked_items_tooltip_gets_a_red_border() {
        // Given the pickaxe the player cannot break with yet, and a stick nothing locks
        let _serial = exclusive();
        let host = TestHost::default();
        known();
        with(|cache| {
            cache
                .reqs
                .insert("minecraft:stick".into(), Requirements::default());
        });
        let border = |item: &str| {
            let stack = ItemStack {
                item: item.into(),
                count: 1,
                damage: 0,
                extra: None,
            };
            let (mut background, mut top, mut bottom) = (1, 2, 3);
            horizon_client::mod_api::dispatch_tooltip_color(
                &host,
                &mods(),
                Some(&stack),
                &mut background,
                &mut top,
                &mut bottom,
            );
            [background, top, bottom]
        };

        // When each tooltip's colours are asked for
        // Then the pickaxe's border turns red and the stick's is left alone, as is the background
        assert_eq!(border(PICKAXE), [1, 0xFFE0_5A4A, 0xFF7A_2018]);
        assert_eq!(border("minecraft:stick"), [1, 2, 3]);
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
}
