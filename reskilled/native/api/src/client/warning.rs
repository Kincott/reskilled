//! The lock warning: a small panel over the game when a lock refuses something, saying what was refused, which
//! skill and level it needs, and how far the player is. It sits above the hotbar while playing and at the top while
//! a screen is up (an inventory would hide it), for the three seconds the cache keeps it.

use horizon_client::mod_api::{Host, HudBatch, NativeScreenKind};
use skills_core::progress::level;

use super::cache::{WarningPlace, with};
use super::layout::Rect;
use super::paint::{self, PADLOCK_SIZE, RED, WHITE};
use super::tooltip::action_name;
use crate::text::{LEVEL, REQUIRES, number, say, skill_name};

const HEIGHT: f32 = 28.0;
/// Where the text starts, after the skill's icon.
const TEXT_X: f32 = 32.0;
const PAD_RIGHT: f32 = 7.0;
/// The text's room at least, so the bar under it has some length.
const LEAST_TEXT: f32 = 96.0;
/// The panel's bottom is this far above the screen's while playing: clear of the hotbar, hearts and armor.
const ABOVE_HOTBAR: f32 = 62.0;
/// The panel's top is this far below the screen's while a screen is up.
const FROM_TOP: f32 = 6.0;
const SLOT: f32 = 20.0;
const ICON: f32 = 16.0;

/// Where the warning goes on a screen of `size` for a line `text_width` wide.
pub fn place(size: [f32; 2], playing: bool, text_width: f32) -> Rect {
    let w = TEXT_X + text_width.max(LEAST_TEXT) + PAD_RIGHT;
    Rect {
        x: ((size[0] - w) / 2.0).floor(),
        y: if playing {
            size[1] - ABOVE_HOTBAR - HEIGHT
        } else {
            FROM_TOP
        },
        w,
        h: HEIGHT,
    }
}

/// Draws the warning while one is up, unless the HUD is hidden.
pub fn draw(host: &dyn Host, batch: &mut HudBatch, hidden: bool) {
    if hidden {
        return;
    }
    let size = [batch.width, batch.height];
    let playing = host
        .client_state()
        .is_none_or(|state| state.screen == NativeScreenKind::Playing);
    with(|cache| {
        let (Some(warning), Some(defs)) = (&cache.warning, &cache.defs) else {
            return;
        };
        let above_hotbar = match cache.settings.warning {
            WarningPlace::Off => return,
            WarningPlace::Top => false,
            WarningPlace::AboveHotbar => playing,
        };
        let d = &defs.defs;
        let needs = [
            say(action_name(warning.action), &[]),
            skill_name(d, &warning.skill),
            number(warning.level),
        ];
        let line = say(&REQUIRES, &needs).plain();
        let have = level(&cache.state, d, &warning.skill);
        let count = say(&LEVEL, &[number(have), number(warning.level)]).plain();
        let r = place(size, above_hotbar, host.text_width(&line));
        paint::fill(host, batch, &r, paint::OUTLINE);
        let inner = r.shrunk(1.0);
        paint::fill(host, batch, &inner, paint::BODY);
        host.fill(batch, [inner.x, inner.y, 3.0, inner.h], paint::ALERT);
        let slot = Rect {
            x: r.x + 7.0,
            y: r.y + 4.0,
            w: SLOT,
            h: SLOT,
        };
        paint::slot(host, batch, &slot);
        if let Some(icon) = d.skill(&warning.skill).map(|s| s.icon_at(have)) {
            host.item(batch, icon, [slot.x + 2.0, slot.y + 2.0], ICON);
        }
        let corner = [
            slot.x + SLOT - PADLOCK_SIZE[0] + 1.0,
            slot.y + SLOT - PADLOCK_SIZE[1] + 1.0,
        ];
        paint::padlock(host, batch, corner);
        host.text(batch, &line, [r.x + TEXT_X, r.y + 5.0], WHITE, true);
        let right = r.x + r.w - PAD_RIGHT;
        paint::text_right(host, batch, &count, right, r.y + 16.0, RED);
        let bar = Rect {
            x: r.x + TEXT_X,
            y: r.y + 17.0,
            w: right - host.text_width(&count) - 4.0 - (r.x + TEXT_X),
            h: 5.0,
        };
        paint::bar(host, batch, &bar, have, warning.level);
    });
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::{HudBatch, dispatch_gui_render_post};
    use skills_core::defs::{Action, SkillDef};
    use skills_core::progress::{PlayerState, SkillProgress};
    use skills_core::registry::Defs;
    use skills_core::wire::ClientDefs;

    use super::*;
    use crate::client::cache::{WARNING_TICKS, Warning, with};
    use crate::test_host::{Paint, TestHost, exclusive, mods};

    const DEFENSE: &str = "reskilled:defense";

    /// Defense known, the player at Defense 3, and a warning up: wearing something needs Defense 16.
    fn warned() {
        let defense = SkillDef {
            id: DEFENSE.into(),
            priority: 0,
            name: "reskilled.skill.defense".into(),
            icon: "minecraft:shield".into(),
            max_level: None,
            cost: None,
            points_every: None,
            icon_levels: Default::default(),
        };
        let mut state = PlayerState::default();
        let progress = SkillProgress {
            level: 3,
            ..SkillProgress::default()
        };
        state.skills.insert(DEFENSE.into(), progress);
        with(|cache| {
            let defs = Defs {
                skills: [(DEFENSE.into(), defense)].into(),
                ..Defs::default()
            };
            cache.defs = Some(ClientDefs {
                defs,
                labels: Default::default(),
            });
            cache.state = state;
            cache.warning = Some(Warning {
                skill: DEFENSE.into(),
                level: 16,
                action: Action::Equip,
                ticks_left: WARNING_TICKS,
            });
        });
    }

    fn painted(host: &TestHost, hidden: bool) -> Vec<Paint> {
        dispatch_gui_render_post(host, &mods(), &mut HudBatch::new([427, 240]), hidden, true);
        host.painted.borrow().clone()
    }

    /// The panel: the first thing filled.
    fn panel_of(painted: &[Paint]) -> [f32; 4] {
        match painted.first() {
            Some(Paint::Fill { rect, .. }) => *rect,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_warning_says_what_is_needed_above_the_hotbar() {
        // Given a warning up while the world is played
        let _serial = exclusive();
        let host = TestHost::default();
        warned();

        // When the GUI is drawn
        let painted = painted(&host, false);

        // Then a centred panel above the hotbar shows the skill's icon, what was refused and needs what, and how
        // far the player is, all of it inside the panel
        let [x, y, w, h] = panel_of(&painted);
        assert_eq!((y + h, h), (178.0, 28.0));
        assert!((x + w / 2.0 - 213.5).abs() <= 0.5, "{x} {w}");
        assert_eq!(host.drawn_text(), vec!["Wear: Defense 16", "3/16"]);
        assert_eq!(
            *host.icons.borrow(),
            vec![("minecraft:shield".to_string(), [x + 9.0, y + 6.0])]
        );
        for paint in &painted {
            let (px, py, pw, ph) = match paint {
                Paint::Fill { rect, .. } => (rect[0], rect[1], rect[2], rect[3]),
                Paint::Text { text, at, .. } => (at[0], at[1], host.text_width(text), 8.0),
                Paint::Item { at, size, .. } => (at[0], at[1], *size, *size),
                Paint::Tooltip { .. } | Paint::Quad { .. } => panic!("{paint:?}"),
            };
            let fits = px >= x && py >= y && px + pw <= x + w && py + ph <= y + h;
            assert!(fits, "{paint:?} leaves the panel");
        }
    }

    #[test]
    fn over_a_screen_the_warning_moves_to_the_top() {
        // Given a warning up while an inventory is open
        let _serial = exclusive();
        let host = TestHost {
            screen_up: true,
            ..TestHost::default()
        };
        warned();

        // When the GUI is drawn
        let painted = painted(&host, false);

        // Then the panel is at the top, where the inventory does not cover it
        assert_eq!(panel_of(&painted)[1], 6.0);
    }

    #[test]
    fn the_warning_goes_where_the_player_put_it() {
        // Given a warning up while the world is played
        let _serial = exclusive();
        let host = TestHost::default();
        warned();

        // When it is drawn with its place set to the top, then off
        with(|cache| cache.settings.warning = crate::client::cache::WarningPlace::Top);
        let top = panel_of(&painted(&host, false))[1];
        host.painted.borrow_mut().clear();
        with(|cache| cache.settings.warning = crate::client::cache::WarningPlace::Off);
        let off = painted(&host, false);

        // Then it is at the top, then not drawn at all
        assert_eq!(top, 6.0);
        assert!(off.is_empty());
    }

    #[test]
    fn nothing_is_drawn_without_a_warning_or_with_the_hud_hidden() {
        // Given a warning up
        let _serial = exclusive();
        let host = TestHost::default();
        warned();

        // When the GUI is drawn with the HUD hidden, and again shown once the warning is gone
        let hidden = painted(&host, true);
        with(|cache| cache.warning = None);
        let gone = painted(&host, false);

        // Then nothing is drawn either time
        assert!(hidden.is_empty() && gone.is_empty());
    }
}
