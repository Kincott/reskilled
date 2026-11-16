//! The anvil's check: a result the player cannot use yet is not taken on the first click. A warning over the anvil
//! says what it needs, a second click on the result takes it anyway, and the player can turn the warning off.

use horizon_client::mod_api::{Flow, Host, HudBatch, ItemStack};
use skills_core::progress::level;

use super::cache::{CLICK_SOUND, ClientCache, with};
use super::layout::Rect;
use super::paint;
use super::screen::{LEFT_BUTTON, RIGHT_BUTTON};
use crate::text::{ANVIL_AGAIN, ANVIL_KEEP, ANVIL_TITLE, NEEDS, number, say, skill_name};

/// The anvil's menu type, its result's slot and where that slot sits in its window.
pub const ANVIL: &str = "minecraft:anvil";
const RESULT_SLOT: usize = 2;
const RESULT_AT: [f32; 2] = [133.0, 46.0];
const SLOT: f32 = 18.0;

/// The result slot's box on a screen whose window is `panel`.
pub fn result_slot(panel: [f32; 4]) -> Rect {
    Rect {
        x: panel[0] + RESULT_AT[0],
        y: panel[1] + RESULT_AT[1],
        w: SLOT,
        h: SLOT,
    }
}

/// The warning: its height, and the toggle's place and width in it.
const WARNING_H: f32 = 50.0;
const TOGGLE_AT: [f32; 2] = [4.0, 35.0];
const TOGGLE_W: f32 = 120.0;
const TOGGLE_H: f32 = 11.0;
const PAD: f32 = 6.0;

/// The warning's box: as wide as the anvil's window, just above it (from the top of the screen when there is no
/// room above).
pub fn warning_rect(panel: [f32; 4]) -> Rect {
    Rect {
        x: panel[0],
        y: (panel[1] - WARNING_H - 2.0).max(2.0),
        w: panel[2],
        h: WARNING_H,
    }
}

/// The "Warn me next time" toggle on the warning over a window `panel`.
pub fn warning_toggle(panel: [f32; 4]) -> Rect {
    let w = warning_rect(panel);
    Rect {
        x: w.x + TOGGLE_AT[0],
        y: w.y + TOGGLE_AT[1],
        w: TOGGLE_W,
        h: TOGGLE_H,
    }
}

/// Reads the anvil's result when an anvil is open, asking the server what it needs when that is not known yet;
/// a warning about a result that is no longer there goes.
pub fn on_tick(host: &mut dyn Host) {
    let open = with(|cache| cache.open_menu.as_deref() == Some(ANVIL));
    let result = if open {
        host.menu_stacks()
            .and_then(|stacks| stacks.get(RESULT_SLOT).cloned().flatten())
    } else {
        None
    };
    with(|cache| {
        if let Some(result) = &result {
            cache.requirements_of(result);
        }
        if cache.anvil_prompt != result {
            cache.anvil_prompt = None;
        }
        cache.anvil_result = result;
    });
}

/// Notes the screen drawn, so the tick knows an anvil is open.
pub fn note_screen(menu: Option<&str>) {
    with(|cache| cache.open_menu = menu.map(str::to_string));
}

/// A click on a screen. On an anvil, a click on a result the player cannot use yet is held back and the warning
/// shown; while it shows, a click on the result takes it, a click on its toggle turns the warning off (or on), a
/// click elsewhere on it does nothing, and any other click closes it and goes ahead.
pub fn click(
    host: &dyn Host,
    menu: Option<&str>,
    panel: [f32; 4],
    at: [f32; 2],
    button: i32,
) -> Flow {
    if menu != Some(ANVIL) {
        return Flow::Continue;
    }
    let on = |r: Rect| r.contains(at[0], at[1]);
    with(|cache| {
        if cache.anvil_prompt.is_some() {
            if on(warning_toggle(panel)) {
                cache.settings.anvil_warning = !cache.settings.anvil_warning;
                cache.settings.save(host);
                if cache.settings.click_sounds {
                    cache.sounds.push(CLICK_SOUND);
                }
                return Flow::Cancel;
            }
            if on(warning_rect(panel)) {
                return Flow::Cancel;
            }
            cache.anvil_prompt = None;
            return Flow::Continue;
        }
        if button != LEFT_BUTTON && button != RIGHT_BUTTON {
            return Flow::Continue;
        }
        if !on(result_slot(panel)) || !cache.settings.anvil_warning {
            return Flow::Continue;
        }
        let Some(result) = cache.anvil_result.clone() else {
            return Flow::Continue;
        };
        if cache.lacks(&result, |_| true) {
            cache.anvil_prompt = Some(result);
            return Flow::Cancel;
        }
        Flow::Continue
    })
}

/// What the result needs that the player lacks, as "Needs Melee 16" lines.
fn needs(cache: &mut ClientCache, stack: &ItemStack) -> Vec<String> {
    let Some(reqs) = cache.requirements_of(stack) else {
        return Vec::new();
    };
    let Some(defs) = &cache.defs else {
        return Vec::new();
    };
    let mut lines: Vec<String> = Vec::new();
    for skills in reqs.by_action.values() {
        for (skill, needed) in skills {
            if level(&cache.state, &defs.defs, skill) >= *needed {
                continue;
            }
            let line = say(&NEEDS, &[skill_name(&defs.defs, skill), number(*needed)]).plain();
            if !lines.contains(&line) {
                lines.push(line);
            }
        }
    }
    lines
}

/// Draws the warning over the anvil while it is up.
pub fn draw(
    host: &dyn Host,
    batch: &mut HudBatch,
    menu: Option<&str>,
    panel: [f32; 4],
    mouse: [f32; 2],
) {
    if menu != Some(ANVIL) {
        return;
    }
    with(|cache| {
        let Some(warned) = cache.anvil_prompt.clone() else {
            return;
        };
        let r = warning_rect(panel);
        paint::container_panel(host, batch, &r);
        let inner = r.w - 2.0 * PAD;
        let title = say(&ANVIL_TITLE, &[]).plain();
        host.text(batch, &title, [r.x + PAD, r.y + 5.0], DARK_RED, false);
        let lines = needs(cache, &warned).join(", ");
        let lines = paint::fitted(host, &lines, inner);
        host.text(batch, &lines, [r.x + PAD, r.y + 15.0], DARK_TEXT, false);
        let again = paint::fitted(host, &say(&ANVIL_AGAIN, &[]).plain(), inner);
        host.text(batch, &again, [r.x + PAD, r.y + 25.0], DARK_TEXT, false);
        let toggle = warning_toggle(panel);
        let boxed = Rect {
            x: toggle.x + 2.0,
            y: toggle.y + 1.0,
            w: 9.0,
            h: 9.0,
        };
        paint::slot(host, batch, &boxed);
        if cache.settings.anvil_warning {
            host.text(
                batch,
                paint::TICK,
                [boxed.x + 1.0, boxed.y + 1.0],
                GREEN_TICK,
                false,
            );
        }
        let over = toggle.contains(mouse[0], mouse[1]);
        let keep = say(&ANVIL_KEEP, &[]).plain();
        host.text(
            batch,
            &keep,
            [boxed.x + boxed.w + 3.0, toggle.y + 2.0],
            if over { HOVER_TEXT } else { DARK_TEXT },
            false,
        );
    });
}

/// The warning's colours, in the game's container style.
const DARK_RED: u32 = 0xA0_2020;
const DARK_TEXT: u32 = 0x40_4040;
const HOVER_TEXT: u32 = 0x20_20A0;
const GREEN_TICK: u32 = 0x3A_A02A;

#[cfg(test)]
mod tests {
    use skills_core::defs::{Action, SkillDef};
    use skills_core::locks::Requirements;
    use skills_core::progress::{PlayerState, SkillProgress};
    use skills_core::registry::Defs;
    use skills_core::wire::ClientDefs;

    use super::*;
    use crate::client::cache::with;
    use crate::test_host::{TestHost, exclusive};

    const MELEE: &str = "reskilled:melee";
    const PANEL: [f32; 4] = [125.0, 37.0, 176.0, 166.0];
    const LEFT: i32 = 0;

    /// A player at Melee 4 at an anvil whose result is `result`; the diamond sword needs Melee 16, the stick nothing.
    fn at_anvil(result: &str) -> TestHost {
        let melee = SkillDef {
            id: MELEE.into(),
            priority: 0,
            name: "reskilled.skill.melee".into(),
            icon: "minecraft:iron_sword".into(),
            max_level: None,
            cost: None,
            points_every: None,
            icon_levels: Default::default(),
            earned_by: Vec::new(),
            color: None,
        };
        let mut state = PlayerState::default();
        state.skills.insert(
            MELEE.into(),
            SkillProgress {
                level: 4,
                ..SkillProgress::default()
            },
        );
        with(|cache| {
            cache.defs = Some(ClientDefs {
                defs: Defs {
                    skills: [(MELEE.into(), melee)].into(),
                    ..Defs::default()
                },
                labels: Default::default(),
                label_icons: Default::default(),
            });
            let sword = Requirements {
                by_action: [(Action::Attack, [(MELEE.to_string(), 16)].into())].into(),
            };
            cache.reqs.insert("minecraft:diamond_sword".into(), sword);
            cache
                .reqs
                .insert("minecraft:stick".into(), Requirements::default());
            cache.state = state;
            cache.settings.anvil_warning = true;
        });
        note_screen(Some(ANVIL));
        let mut host = TestHost {
            menu: Some(vec![None, None, Some(ItemStack::new(result, 1))]),
            ..TestHost::default()
        };
        on_tick(&mut host);
        host
    }

    fn on_result() -> [f32; 2] {
        let r = result_slot(PANEL);
        [r.x + 4.0, r.y + 4.0]
    }

    #[test]
    fn a_locked_result_asks_first_and_the_second_click_takes_it() {
        // Given a diamond sword (Melee 16) in the anvil's result, the player at Melee 4
        let _serial = exclusive();
        let host = at_anvil("minecraft:diamond_sword");

        // When the result is clicked, the screen drawn, and the result clicked again
        let first = click(&host, Some(ANVIL), PANEL, on_result(), LEFT);
        draw(
            &host,
            &mut HudBatch::new([427, 240]),
            Some(ANVIL),
            PANEL,
            [1.0, 1.0],
        );
        let shown = host.drawn_text();
        let second = click(&host, Some(ANVIL), PANEL, on_result(), LEFT);

        // Then the first click is held back and a warning says what it needs; the second takes it
        assert_eq!(first, Flow::Cancel);
        assert!(
            shown.iter().any(|t| t == "You can't use this yet"),
            "{shown:?}"
        );
        assert!(
            shown
                .iter()
                .any(|t| t.contains("Melee") && t.contains("16")),
            "{shown:?}"
        );
        assert_eq!(second, Flow::Continue);
        assert!(with(|cache| cache.anvil_prompt.is_none()));
    }

    #[test]
    fn a_result_the_player_can_use_is_taken_at_once() {
        // Given a stick in the result
        let _serial = exclusive();
        let host = at_anvil("minecraft:stick");

        // When it is clicked
        let flow = click(&host, Some(ANVIL), PANEL, on_result(), LEFT);

        // Then it is taken
        assert_eq!(flow, Flow::Continue);
    }

    #[test]
    fn the_warning_can_be_turned_off_from_it() {
        // Given the warning up for a diamond sword
        let _serial = exclusive();
        let host = at_anvil("minecraft:diamond_sword");
        click(&host, Some(ANVIL), PANEL, on_result(), LEFT);

        // When "Warn me next time" is clicked off, then the result taken, then another locked result clicked
        let toggle = warning_toggle(PANEL);
        let toggled = click(
            &host,
            Some(ANVIL),
            PANEL,
            [toggle.x + 2.0, toggle.y + 2.0],
            LEFT,
        );
        click(&host, Some(ANVIL), PANEL, on_result(), LEFT);
        let later = click(&host, Some(ANVIL), PANEL, on_result(), LEFT);

        // Then the toggle's click is the warning's own, the setting is off, and the next result is taken at once
        assert_eq!(toggled, Flow::Cancel);
        assert!(!with(|cache| cache.settings.anvil_warning));
        assert_eq!(later, Flow::Continue);
    }

    #[test]
    fn a_click_elsewhere_closes_the_warning_and_other_screens_are_untouched() {
        // Given the warning up for a diamond sword
        let _serial = exclusive();
        let host = at_anvil("minecraft:diamond_sword");
        click(&host, Some(ANVIL), PANEL, on_result(), LEFT);

        // When the player clicks an inventory slot, and on another screen clicks where the result would be
        let elsewhere = click(
            &host,
            Some(ANVIL),
            PANEL,
            [PANEL[0] + 10.0, PANEL[1] + 100.0],
            LEFT,
        );
        let closed = with(|cache| cache.anvil_prompt.is_none());
        let other = click(
            &host,
            Some("minecraft:generic_9x3"),
            PANEL,
            on_result(),
            LEFT,
        );

        // Then the warning closes and that click goes ahead, and the chest is left alone
        assert_eq!((elsewhere, closed), (Flow::Continue, true));
        assert_eq!(other, Flow::Continue);
    }
}
