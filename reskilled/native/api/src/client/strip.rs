//! The skill strip: a column of the player's skills, each its icon and level, beside their inventory and the
//! crafting table, so they see what they can use while they craft. A display setting; hovering a skill names it.

use horizon_client::mod_api::{Host, HudBatch};
use skills_core::progress::level;

use super::cache::with;
use super::layout::Rect;
use super::paint::{self, CONTAINER_SLOT, GOLD, GRAY, WHITE};
use crate::text::{LEVEL_OF, number, say, skill_name};

/// The windows that get the strip: the player's own inventory (no menu) and the crafting table.
const CRAFTING: &str = "minecraft:crafting";
/// Between the strip's panel and the window's, as the recipe book leaves.
const GAP: f32 = 2.0;
/// From the panel's edge to its slots.
const PAD: f32 = 4.0;
const ICON: f32 = 16.0;

/// Draws the strip left of the window's panel when the screen is the inventory or a crafting table, the player
/// has it on, and there is room: a small panel in the window's own style, one slot per skill with its icon, and
/// the level where a stack's count goes.
pub fn draw_beside(
    host: &dyn Host,
    batch: &mut HudBatch,
    screen: &str,
    menu: Option<&str>,
    panel: [f32; 4],
    mouse: [f32; 2],
) {
    if screen != "inventory" || !matches!(menu, None | Some(CRAFTING)) {
        return;
    }
    let width = CONTAINER_SLOT + 2.0 * PAD;
    let x = panel[0] - GAP - width;
    if x < GAP {
        return;
    }
    with(|cache| {
        let (Some(defs), true) = (&cache.defs, cache.settings.skill_strip) else {
            return;
        };
        let d = &defs.defs;
        let fits = ((panel[3] - 2.0 * PAD) / CONTAINER_SLOT).floor().max(0.0) as usize;
        let shown = d.skills.len().min(fits);
        if shown == 0 {
            return;
        }
        let back = Rect {
            x,
            y: panel[1],
            w: width,
            h: shown as f32 * CONTAINER_SLOT + 2.0 * PAD,
        };
        paint::container_panel(host, batch, &back);
        let mut tip = None;
        for (n, (id, skill)) in d.skills.iter().take(shown).enumerate() {
            let at = [x + PAD, back.y + PAD + n as f32 * CONTAINER_SLOT];
            let slot = Rect {
                x: at[0],
                y: at[1],
                w: CONTAINER_SLOT,
                h: CONTAINER_SLOT,
            };
            let hovered = slot.contains(mouse[0], mouse[1]);
            let now = level(&cache.state, d, id);
            let most = d.max_level(id);
            paint::container_slot(host, batch, at);
            host.item(batch, skill.icon_at(now), [at[0] + 1.0, at[1] + 1.0], ICON);
            if hovered {
                paint::container_slot_highlight(host, batch, at);
            }
            paint::text_right(
                host,
                batch,
                &now.to_string(),
                at[0] + CONTAINER_SLOT,
                at[1] + CONTAINER_SLOT - 8.0,
                if now >= most { GOLD } else { WHITE },
            );
            if hovered {
                tip = Some(vec![
                    skill_name(d, id).with_color(GOLD),
                    say(&LEVEL_OF, &[number(now), number(most)]).with_color(GRAY),
                ]);
            }
        }
        if let Some(lines) = tip {
            host.gui_tooltip(batch, &lines, mouse);
        }
    });
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::HudBatch;
    use skills_core::defs::SkillDef;
    use skills_core::progress::{PlayerState, SkillProgress};
    use skills_core::registry::Defs;
    use skills_core::wire::ClientDefs;

    use super::*;
    use crate::client::cache::with;
    use crate::test_host::{Paint, TestHost, exclusive};

    /// The inventory's panel on a 427 by 240 screen: 176 by 166, centred.
    const PANEL: [f32; 4] = [125.0, 37.0, 176.0, 166.0];

    fn skill(path: &str) -> (String, SkillDef) {
        let id = format!("reskilled:{path}");
        let def = SkillDef {
            id: id.clone(),
            priority: 0,
            name: format!("reskilled.skill.{path}"),
            icon: "minecraft:wooden_pickaxe".into(),
            max_level: None,
            cost: None,
            points_every: None,
            icon_levels: [(8, "minecraft:iron_pickaxe".to_string())].into(),
            earned_by: Vec::new(),
        };
        (id, def)
    }

    /// Mining at 12 and Melee at 3.
    fn known() {
        let mut state = PlayerState::default();
        let at = |level| SkillProgress {
            level,
            ..SkillProgress::default()
        };
        state.skills.insert("reskilled:mining".into(), at(12));
        state.skills.insert("reskilled:melee".into(), at(3));
        with(|cache| {
            let defs = Defs {
                skills: [skill("melee"), skill("mining")].into(),
                ..Defs::default()
            };
            cache.defs = Some(ClientDefs {
                defs,
                labels: Default::default(),
            });
            cache.state = state;
        });
    }

    fn painted(
        host: &TestHost,
        menu: Option<&str>,
        panel: [f32; 4],
        mouse: [f32; 2],
    ) -> Vec<Paint> {
        host.painted.borrow_mut().clear();
        draw_beside(
            host,
            &mut HudBatch::new([427, 240]),
            "inventory",
            menu,
            panel,
            mouse,
        );
        host.painted.borrow().clone()
    }

    #[test]
    fn the_inventory_gets_a_column_of_skills_with_their_levels() {
        // Given a player at Mining 12 and Melee 3
        let _serial = exclusive();
        let host = TestHost::default();
        known();

        // When their own inventory is drawn
        let painted = painted(&host, None, PANEL, [0.0, 0.0]);

        // Then left of the panel each skill shows its icon for its level and the level, top to bottom
        let items: Vec<(String, [f32; 2])> = painted
            .iter()
            .filter_map(|p| match p {
                Paint::Item { item, at, .. } => Some((item.clone(), *at)),
                _ => None,
            })
            .collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0, "minecraft:wooden_pickaxe");
        assert_eq!(items[1].0, "minecraft:iron_pickaxe");
        assert!(items.iter().all(|(_, at)| at[0] + 16.0 < PANEL[0]));
        assert!(items[1].1[1] > items[0].1[1]);
        assert_eq!(host.drawn_text(), vec!["3", "12"]);
    }

    #[test]
    fn the_strip_is_drawn_like_the_inventory_beside_it() {
        // Given a player at Mining 12 and Melee 3
        let _serial = exclusive();
        let host = TestHost::default();
        known();

        // When their inventory is drawn
        let painted = painted(&host, None, PANEL, [0.0, 0.0]);

        // Then a panel of the inventory's grey sits left of it, top aligned, and the icons sit in slots 18 apart
        let grey = painted.iter().find_map(|p| match p {
            Paint::Fill { rect, argb } if *argb == paint::CONTAINER => Some(*rect),
            _ => None,
        });
        let grey = grey.expect("an inventory-grey panel");
        assert!(grey[0] + grey[2] < PANEL[0] && grey[1] > PANEL[1] - 1.0);
        let ys: Vec<f32> = painted
            .iter()
            .filter_map(|p| match p {
                Paint::Item { at, .. } => Some(at[1]),
                _ => None,
            })
            .collect();
        assert_eq!(ys[1] - ys[0], 18.0);
    }

    #[test]
    fn the_crafting_table_gets_it_and_other_windows_do_not() {
        // Given a player at Mining 12
        let _serial = exclusive();
        let host = TestHost::default();
        known();

        // When a crafting table, a chest and a furnace are drawn
        let table = painted(&host, Some("minecraft:crafting"), PANEL, [0.0, 0.0]);
        let chest = painted(&host, Some("minecraft:generic_9x3"), PANEL, [0.0, 0.0]);
        let furnace = painted(&host, Some("minecraft:furnace"), PANEL, [0.0, 0.0]);

        // Then only the crafting table shows the strip
        assert!(!table.is_empty());
        assert!(chest.is_empty() && furnace.is_empty());
    }

    #[test]
    fn hovering_a_skill_names_it_and_the_setting_hides_the_strip() {
        // Given a player at Mining 12
        let _serial = exclusive();
        let host = TestHost::default();
        known();
        let first = painted(&host, None, PANEL, [0.0, 0.0]);
        let melee_at = first
            .iter()
            .find_map(|p| match p {
                Paint::Item { at, .. } => Some(*at),
                _ => None,
            })
            .unwrap();

        // When the mouse is over Melee, then the strip is turned off in the settings
        let hovered = painted(&host, None, PANEL, [melee_at[0] + 4.0, melee_at[1] + 4.0]);
        with(|cache| cache.settings.skill_strip = false);
        let hidden = painted(&host, None, PANEL, [0.0, 0.0]);

        // Then the tooltip names Melee and its level, and with the setting off nothing is drawn
        let tip = hovered.iter().find_map(|p| match p {
            Paint::Tooltip { lines, .. } => Some(lines.clone()),
            _ => None,
        });
        assert_eq!(
            tip,
            Some(vec!["Melee".to_string(), "Level 3 of 32".to_string()])
        );
        assert!(hidden.is_empty());
    }

    #[test]
    fn no_strip_when_there_is_no_room_left_of_the_panel() {
        // Given a player at Mining 12, and a panel against the screen's left edge
        let _serial = exclusive();
        let host = TestHost::default();
        known();

        // When their inventory is drawn there
        let painted = painted(&host, None, [4.0, 37.0, 176.0, 166.0], [0.0, 0.0]);

        // Then nothing is drawn off the screen
        assert!(painted.is_empty());
    }
}
