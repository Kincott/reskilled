//! Reskilled's options, opened from the skills screen's gear: an interface of its own in the style of the game's
//! options screens. The Display tab holds the player's own settings (saved to their config folder); the Server
//! rules tab shows the server's rules, which operators change (the server checks).

use horizon_client::mod_api::{Flow, Host, HudBatch, Text};
use skills_core::rules::Rules;
use skills_core::wire::ToServer;

use super::cache::{CLICK_SOUND, ClientSettings, View, with};
use super::layout::Rect;
use super::paint::{GLYPH, fitted};
use super::screen::{LEFT_BUTTON, RIGHT_BUTTON};
use super::settings::{CHOICES, RULES};
use crate::text::{
    CLICK_TO_CHANGE, DONE, NAMED_VALUE, ONLY_OPS, OPTIONS_TITLE, RESET, TAB_DISPLAY, TAB_RULES, say,
};

/// The game's button and tab sprites.
const BUTTON: &str = "widget/button";
const BUTTON_HOVER: &str = "widget/button_highlighted";
const BUTTON_OFF: &str = "widget/button_disabled";
const TAB: &str = "widget/tab";
const TAB_HOVER: &str = "widget/tab_highlighted";
const TAB_SELECTED: &str = "widget/tab_selected";
const UNTINTED: u32 = 0xFFFF_FFFF;
const WHITE: u32 = 0xFF_FFFF;
const GRAY: u32 = 0xA0_A0A0;
/// Vanilla's options layout: 150 by 20 buttons in two columns 10 apart, a row every 24 pixels.
const BUTTON_W: f32 = 150.0;
const BUTTON_H: f32 = 20.0;
const COLUMN_GAP: f32 = 10.0;
const ROW_STEP: f32 = 24.0;
const TITLE_Y: f32 = 8.0;
const TABS_Y: f32 = 22.0;
const TAB_W: f32 = 100.0;
const TAB_H: f32 = 24.0;
const ROWS_Y: f32 = 54.0;
const BOTTOM_GAP: f32 = 27.0;

/// The options' two tabs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Display,
    Rules,
}

/// Where the interface puts things on a screen of `size`, for `count` options.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub tabs: [Rect; 2],
    pub buttons: Vec<Rect>,
    pub reset: Rect,
    pub done: Rect,
}

pub fn page(size: [f32; 2], count: usize) -> Page {
    let centre = (size[0] / 2.0).floor();
    let left = centre - BUTTON_W - COLUMN_GAP / 2.0;
    let right = centre + COLUMN_GAP / 2.0;
    let rect = |x: f32, y: f32, w: f32, h: f32| Rect { x, y, w, h };
    let buttons = (0..count)
        .map(|i| {
            let x = if i % 2 == 0 { left } else { right };
            rect(x, ROWS_Y + (i / 2) as f32 * ROW_STEP, BUTTON_W, BUTTON_H)
        })
        .collect();
    let bottom = size[1] - BOTTOM_GAP;
    Page {
        tabs: [
            rect(centre - TAB_W, TABS_Y, TAB_W, TAB_H),
            rect(centre, TABS_Y, TAB_W, TAB_H),
        ],
        buttons,
        reset: rect(left, bottom, BUTTON_W, BUTTON_H),
        done: rect(right, bottom, BUTTON_W, BUTTON_H),
    }
}

/// One option as the tab shows it.
struct Shown {
    label: String,
    help: Vec<Text>,
}

fn shown(tab: Tab, settings: &ClientSettings, rules: &Rules, editable: bool) -> Vec<Shown> {
    let hint = |can: bool| say(if can { &CLICK_TO_CHANGE } else { &ONLY_OPS }, &[]);
    match tab {
        Tab::Display => CHOICES
            .iter()
            .map(|c| Shown {
                label: say(&NAMED_VALUE, &[say(c.name(), &[]), c.value(settings)]).plain(),
                help: vec![say(c.name(), &[]), say(c.help(), &[]), hint(true)],
            })
            .collect(),
        Tab::Rules => RULES
            .iter()
            .map(|r| Shown {
                label: say(&NAMED_VALUE, &[say(r.name(), &[]), r.value(rules)]).plain(),
                help: vec![say(r.name(), &[]), say(r.help(), &[]), hint(editable)],
            })
            .collect(),
    }
}

/// A game-style button with its label centred, cut to fit.
fn button(
    host: &dyn Host,
    batch: &mut HudBatch,
    r: &Rect,
    label: &str,
    enabled: bool,
    hover: bool,
) {
    let sprite = match (enabled, hover) {
        (false, _) => BUTTON_OFF,
        (true, true) => BUTTON_HOVER,
        (true, false) => BUTTON,
    };
    host.gui_nine_slice(batch, sprite, r.array(), UNTINTED);
    let label = fitted(host, label, r.w - 8.0);
    let x = r.x + ((r.w - host.text_width(&label)) / 2.0).round();
    let y = r.y + ((r.h - GLYPH) / 2.0).round();
    host.text(
        batch,
        &label,
        [x, y],
        if enabled { WHITE } else { GRAY },
        true,
    );
}

/// Draws the options over the whole screen.
pub fn draw(host: &dyn Host, batch: &mut HudBatch, size: [f32; 2], mouse: [f32; 2]) {
    host.draw_menu_background(batch, [0.0, 0.0, size[0], size[1]], true);
    let over = |r: &Rect| r.contains(mouse[0], mouse[1]);
    with(|cache| {
        let title = say(&OPTIONS_TITLE, &[]).plain();
        let x = ((size[0] - host.text_width(&title)) / 2.0).round();
        host.text(batch, &title, [x, TITLE_Y], WHITE, true);
        let options = shown(
            cache.options_tab,
            &cache.settings,
            &cache.rules,
            cache.rules_editable,
        );
        let p = page(size, options.len());
        for ((tab, name), r) in [(Tab::Display, &TAB_DISPLAY), (Tab::Rules, &TAB_RULES)]
            .into_iter()
            .zip(&p.tabs)
        {
            let sprite = if tab == cache.options_tab {
                TAB_SELECTED
            } else if over(r) {
                TAB_HOVER
            } else {
                TAB
            };
            host.gui_nine_slice(batch, sprite, r.array(), UNTINTED);
            let label = say(name, &[]).plain();
            let x = r.x + ((r.w - host.text_width(&label)) / 2.0).round();
            host.text(batch, &label, [x, r.y + 8.0], WHITE, true);
        }
        let editable = cache.options_tab == Tab::Display || cache.rules_editable;
        let mut tip = None;
        for (option, r) in options.iter().zip(&p.buttons) {
            button(host, batch, r, &option.label, editable, over(r));
            if over(r) {
                tip = Some(option.help.clone());
            }
        }
        button(
            host,
            batch,
            &p.reset,
            &say(&RESET, &[]).plain(),
            editable,
            over(&p.reset),
        );
        button(
            host,
            batch,
            &p.done,
            &say(&DONE, &[]).plain(),
            true,
            over(&p.done),
        );
        if let Some(lines) = tip {
            host.gui_tooltip(batch, &lines, mouse);
        }
    });
}

/// A click on the options: a tab, an option, reset or done; the right button goes back too.
pub fn click(host: &dyn Host, size: [f32; 2], at: [f32; 2], mouse_button: i32) -> Flow {
    with(|cache| {
        if mouse_button == RIGHT_BUTTON {
            cache.view = View::Overview;
            return;
        }
        if mouse_button != LEFT_BUTTON {
            return;
        }
        let count = match cache.options_tab {
            Tab::Display => CHOICES.len(),
            Tab::Rules => RULES.len(),
        };
        let p = page(size, count);
        let on = |r: &Rect| r.contains(at[0], at[1]);
        let editable = cache.options_tab == Tab::Display || cache.rules_editable;
        let mut clicked = true;
        if on(&p.done) {
            cache.view = View::Overview;
        } else if on(&p.tabs[0]) {
            cache.options_tab = Tab::Display;
        } else if on(&p.tabs[1]) {
            cache.options_tab = Tab::Rules;
        } else if let Some(i) = p.buttons.iter().position(&on).filter(|_| editable) {
            match cache.options_tab {
                Tab::Display => {
                    CHOICES[i].next(&mut cache.settings);
                    cache.settings.save(host);
                }
                Tab::Rules => {
                    RULES[i].next(&mut cache.rules);
                    cache.outgoing.push(ToServer::SetRules(cache.rules.clone()));
                }
            }
        } else if on(&p.reset) && editable {
            match cache.options_tab {
                Tab::Display => {
                    cache.settings = ClientSettings::default();
                    cache.settings.save(host);
                }
                Tab::Rules => {
                    cache.rules = Rules::default();
                    cache.outgoing.push(ToServer::SetRules(Rules::default()));
                }
            }
        } else {
            clicked = false;
        }
        if clicked && cache.settings.click_sounds {
            cache.sounds.push(CLICK_SOUND);
        }
    });
    Flow::Cancel
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::HudBatch;
    use skills_core::rules::{Respec, Rules};
    use skills_core::wire::ToServer;

    use super::*;
    use crate::MOD_ID;
    use crate::client::cache::{ClientSettings, Corner, View, WarningPlace, with};
    use crate::test_host::{Paint, TestHost, exclusive, mods};

    const SIZE: [f32; 2] = [427.0, 240.0];
    const SMALLEST: [f32; 2] = [320.0, 240.0];
    const AWAY: [f32; 2] = [1.0, 1.0];

    fn inside(r: &Rect) -> [f32; 2] {
        [r.x + 2.0, r.y + 2.0]
    }

    /// The options open on their Display tab, saving to a fresh config folder.
    fn open(name: &str) -> (TestHost, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("reskilled_options_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        with(|cache| cache.view = View::Settings);
        let host = TestHost {
            open_screen: Some(MOD_ID.into()),
            config: Some(dir.clone()),
            ..TestHost::default()
        };
        (host, dir)
    }

    fn drawn(host: &TestHost, mouse: [f32; 2]) -> Vec<Paint> {
        host.painted.borrow_mut().clear();
        draw(host, &mut HudBatch::new([427, 240]), SIZE, mouse);
        host.painted.borrow().clone()
    }

    fn texts(painted: &[Paint]) -> Vec<String> {
        painted
            .iter()
            .filter_map(|p| match p {
                Paint::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn tooltip(painted: &[Paint]) -> Vec<String> {
        painted
            .iter()
            .find_map(|p| match p {
                Paint::Tooltip { lines, .. } => Some(lines.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    #[test]
    fn the_options_look_like_the_games_and_fit_the_smallest_screen() {
        // Given the options' layout for ten options, on the smallest screen
        // When it is laid out
        let p = page(SMALLEST, 10);

        // Then two columns of game-size buttons, five rows, the tabs above and reset and done below, all on screen
        // and apart
        let screen = Rect {
            x: 0.0,
            y: 0.0,
            w: SMALLEST[0],
            h: SMALLEST[1],
        };
        let parts: Vec<&Rect> = p
            .tabs
            .iter()
            .chain(&p.buttons)
            .chain([&p.reset, &p.done])
            .collect();
        for (n, part) in parts.iter().enumerate() {
            let on =
                part.x >= screen.x && part.x + part.w <= screen.w && part.y + part.h <= screen.h;
            assert!(on, "{part:?}");
            for other in &parts[n + 1..] {
                let apart = part.x + part.w <= other.x
                    || other.x + other.w <= part.x
                    || part.y + part.h <= other.y
                    || other.y + other.h <= part.y;
                assert!(apart, "{part:?} {other:?}");
            }
        }
        assert_eq!((p.buttons[0].w, p.buttons[0].h), (150.0, 20.0));
        assert_eq!(p.buttons[1].y, p.buttons[0].y);
        assert_eq!(p.buttons[2].y, p.buttons[0].y + 24.0);
    }

    #[test]
    fn the_display_tab_shows_each_setting_with_its_value_on_game_buttons() {
        // Given the options open
        let _serial = exclusive();
        let (host, dir) = open("display");

        // When they are drawn
        let painted = drawn(&host, AWAY);
        std::fs::remove_dir_all(&dir).unwrap();

        // Then the title, both tabs, each setting as "Name: Value" on the game's own buttons, reset and done show,
        // over the game's menu background
        let texts = texts(&painted);
        for expected in [
            "Reskilled Options",
            "Display",
            "Server rules",
            "Warning: Above hotbar",
            "Warning time: 3 s",
            "Padlock: On",
            "Lock corner: Bottom left",
            "Refusal sound: On",
            "Reset to defaults",
            "Done",
        ] {
            assert!(
                texts.iter().any(|t| t == expected),
                "{expected:?} not in {texts:?}"
            );
        }
        let buttons = painted
            .iter()
            .filter(|p| matches!(p, Paint::Sprite { name, .. } if name == "widget/button"))
            .count();
        assert_eq!(buttons, 13);
        assert!(matches!(painted.first(), Some(Paint::Background { .. })));
    }

    #[test]
    fn clicking_a_display_setting_changes_and_keeps_it() {
        // Given the options open
        let _serial = exclusive();
        let (host, dir) = open("change");
        let p = page(SIZE, 11);

        // When the warning is clicked twice and the lock corner once
        click(&host, SIZE, inside(&p.buttons[0]), LEFT_BUTTON);
        click(&host, SIZE, inside(&p.buttons[0]), LEFT_BUTTON);
        click(&host, SIZE, inside(&p.buttons[3]), LEFT_BUTTON);
        let saved = ClientSettings::load(&host);
        std::fs::remove_dir_all(&dir).unwrap();

        // Then the warning is off and the padlock top left, in the game and in the file
        let now = with(|cache| cache.settings.clone());
        assert_eq!(
            (now.warning, now.lock_corner),
            (WarningPlace::Off, Corner::TopLeft)
        );
        assert_eq!(saved, now);
    }

    #[test]
    fn an_operator_changes_the_servers_rules_from_the_second_tab() {
        // Given an operator's options on the Server rules tab
        let _serial = exclusive();
        let (mut host, dir) = open("rules");
        with(|cache| {
            cache.rules_editable = true;
            cache.defs = Some(Default::default());
        });
        click(&host, SIZE, inside(&page(SIZE, 9).tabs[1]), LEFT_BUTTON);
        let p = page(SIZE, 9);

        // When perk refunds are clicked, then a tick passes
        click(&host, SIZE, inside(&p.buttons[5]), LEFT_BUTTON);
        horizon_client::mod_api::dispatch_tick(&mut host, &mods());
        std::fs::remove_dir_all(&dir).unwrap();

        // Then the refund rule moves from paid to off, and the server is asked for the new rules
        let wanted = Rules {
            respec: Respec::Off,
            ..Rules::default()
        };
        let sent: Vec<ToServer> = host
            .sent_to_server
            .iter()
            .map(|(_, data)| serde_json::from_slice(data).unwrap())
            .filter(|m| matches!(m, ToServer::SetRules(_)))
            .collect();
        assert_eq!(sent, vec![ToServer::SetRules(wanted.clone())]);
        assert_eq!(with(|cache| cache.rules.clone()), wanted);
    }

    #[test]
    fn a_player_sees_the_rules_but_cannot_change_them() {
        // Given a player's options on the Server rules tab
        let _serial = exclusive();
        let (mut host, dir) = open("player");
        with(|cache| cache.options_tab = Tab::Rules);
        let p = page(SIZE, 9);

        // When the rules are drawn with the mouse on refunds, refunds are clicked, and a tick passes
        let painted = drawn(&host, inside(&p.buttons[5]));
        click(&host, SIZE, inside(&p.buttons[5]), LEFT_BUTTON);
        horizon_client::mod_api::dispatch_tick(&mut host, &mods());
        std::fs::remove_dir_all(&dir).unwrap();

        // Then each rule shows on a greyed button, the tooltip says only operators may change it, and nothing is
        // asked
        assert!(texts(&painted).iter().any(|t| t == "Perk refunds: Paid"));
        let greyed = painted
            .iter()
            .filter(|p| matches!(p, Paint::Sprite { name, .. } if name == "widget/button_disabled"))
            .count();
        assert_eq!(greyed, 10);
        assert_eq!(
            tooltip(&painted).last().map(String::as_str),
            Some("Only operators can change this.")
        );
        assert!(host.sent_to_server.iter().all(|(_, data)| !matches!(
            serde_json::from_slice(data).unwrap(),
            ToServer::SetRules(_)
        )));
    }

    #[test]
    fn done_and_the_right_button_go_back_to_the_skills() {
        // Given the options open
        let _serial = exclusive();
        let (host, dir) = open("done");

        // When done is clicked, the options are opened again and the right button pressed
        click(&host, SIZE, inside(&page(SIZE, 11).done), LEFT_BUTTON);
        let after_done = with(|cache| cache.view);
        with(|cache| cache.view = View::Settings);
        click(&host, SIZE, AWAY, RIGHT_BUTTON);
        std::fs::remove_dir_all(&dir).unwrap();

        // Then both lead back to the skills
        assert_eq!(after_done, View::Overview);
        assert_eq!(with(|cache| cache.view), View::Overview);
    }

    #[test]
    fn every_label_fits_its_button() {
        // Given the options on each tab, with every option at its longest value
        let _serial = exclusive();
        let (host, dir) = open("fit");
        with(|cache| {
            cache.settings.lock_corner = Corner::BottomRight;
            cache.rules_editable = true;
        });

        for tab in [Tab::Display, Tab::Rules] {
            // When the tab is drawn
            with(|cache| cache.options_tab = tab);
            let painted = drawn(&host, AWAY);
            let count = if tab == Tab::Display { 11 } else { 9 };

            // Then no label is cut short with an ellipsis
            let cut: Vec<String> = texts(&painted)
                .into_iter()
                .filter(|t| t.ends_with("..."))
                .collect();
            assert!(
                cut.is_empty(),
                "{tab:?}: {cut:?} do not fit {:?}",
                page(SIZE, count).buttons[0]
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
