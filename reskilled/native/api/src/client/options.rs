//! Reskilled's options, on the window's Options tab: the player's display settings (saved to their config folder)
//! in sections on the left, the server's rules on the right (operators change them; the server checks), a box
//! that explains the row under the mouse, and Defaults and Done. On a narrow window they make one column that
//! scrolls.

use horizon_client::mod_api::{Flow, Host, HudBatch};
use skills_core::wire::ToServer;

use super::cache::{CLICK_SOUND, ClientCache, Tab as WindowTab, with};
use super::layout::{Rect, Scroll};
use super::paint::{self, DIM, GOLD, Look, WHITE};
use super::screen::{LEFT_BUTTON, RIGHT_BUTTON};
use super::settings::{CHOICES, Choice, RULES, RuleChoice, section};
use crate::text::{DONE, Message, ONLY_OPS, RESET, TAB_DISPLAY, TAB_RULES, VAL_ON, WAITING, say};

/// A row, its pitch, a section's heading, a column's title, the operators' note and the gap between columns.
const ROW_H: f32 = 11.0;
const ROW_PITCH: f32 = 12.0;
const HEADING_H: f32 = 9.0;
const TITLE_H: f32 = 10.0;
const NOTE_H: f32 = 10.0;
const COLUMN_GAP: f32 = 8.0;
const LEFT_SHARE: f32 = 0.48;
/// The help box and the buttons under it.
const HELP_H: f32 = 20.0;
const BUTTONS_H: f32 = 16.0;
const BOTTOM_GAP: f32 = 3.0;
/// Below this body width the options make one column, with room for its scroll bar.
const TWO_COLUMNS: f32 = 300.0;
const SCROLL_ROOM: f32 = 6.0;
/// A value button's least width.
const VALUE_W: f32 = 30.0;
/// Colours.
const BLACK: u32 = 0xFF07_080B;
const CARD: u32 = 0xFF23_2733;
const CARD_HI: u32 = 0xFF2B_3040;
const INSET: u32 = 0xFF0F_1116;
const BRASS_DARK: u32 = 0xFF5A_4116;
/// A rule's label when the player cannot change it.
pub const LOCKED: u32 = 0xA8_ADB8;

/// The words between the rows: a column's title, a section's heading, the operators' note.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Label {
    Title(&'static Message),
    Section(&'static Message),
    OnlyOps,
}

/// Where the options put things: the display rows and the rule rows on show, the words between them, the help box,
/// the two buttons, and the scroll bar of a single column.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionsLayout {
    pub display: Vec<(Choice, Rect)>,
    pub rules: Vec<(RuleChoice, Rect)>,
    pub labels: Vec<(Label, Rect)>,
    pub help: Rect,
    pub defaults: Rect,
    pub done: Rect,
    pub scroll: Option<Scroll>,
}

/// One line of a column.
#[derive(Clone, Copy)]
enum Line {
    Label(Label),
    Display(Choice),
    Rule(RuleChoice),
}

fn height_of(line: &Line) -> f32 {
    match line {
        Line::Label(Label::Title(_)) => TITLE_H,
        Line::Label(Label::Section(_)) => HEADING_H,
        Line::Label(Label::OnlyOps) => NOTE_H,
        Line::Display(_) | Line::Rule(_) => ROW_PITCH,
    }
}

/// The display column: its title, then each section's heading before its rows.
fn display_lines() -> Vec<Line> {
    let mut lines = vec![Line::Label(Label::Title(&TAB_DISPLAY))];
    let mut last: Option<&'static Message> = None;
    for choice in CHOICES {
        let heading = section(choice);
        if !last.is_some_and(|l| l.key == heading.key) {
            lines.push(Line::Label(Label::Section(heading)));
            last = Some(heading);
        }
        lines.push(Line::Display(choice));
    }
    lines
}

/// The rules column: its title, the rules, and the operators' note.
fn rule_lines() -> Vec<Line> {
    let mut lines = vec![Line::Label(Label::Title(&TAB_RULES))];
    lines.extend(RULES.iter().map(|r| Line::Rule(*r)));
    lines.push(Line::Label(Label::OnlyOps));
    lines
}

/// How many lines from `first` fit in `room`.
fn fitting(lines: &[Line], first: usize, room: f32) -> usize {
    let mut y = 0.0;
    let mut count = 0;
    for line in &lines[first.min(lines.len())..] {
        let h = height_of(line);
        if y + h > room {
            break;
        }
        y += h;
        count += 1;
    }
    count
}

/// The room a single column has above the help box.
fn column_room(body: &Rect) -> f32 {
    body.h - HELP_H - BUTTONS_H - 2.0 * BOTTOM_GAP
}

/// The line a single column may start from at most; 0 when the options make two columns.
pub fn last_first(body: &Rect) -> usize {
    if body.w >= TWO_COLUMNS {
        return 0;
    }
    let mut lines = display_lines();
    lines.extend(rule_lines());
    let room = column_room(body);
    (0..lines.len())
        .find(|&first| first + fitting(&lines, first, room) == lines.len())
        .unwrap_or(0)
}

/// Places `lines` down from (`x`, `y`), `w` wide.
fn place(lines: &[Line], x: f32, mut y: f32, w: f32, l: &mut OptionsLayout) {
    for line in lines {
        let h = height_of(line);
        match *line {
            Line::Label(label) => l.labels.push((label, Rect { x, y, w, h })),
            Line::Display(choice) => l.display.push((choice, Rect { x, y, w, h: ROW_H })),
            Line::Rule(choice) => l.rules.push((choice, Rect { x, y, w, h: ROW_H })),
        }
        y += h;
    }
}

/// The options on a body of `body`; a single column starts from line `first`.
pub fn options_layout(body: &Rect, first: usize) -> OptionsLayout {
    let bottom = body.y + body.h;
    let two = body.w >= TWO_COLUMNS;
    // The display column needs less room than the rules' note, so it takes a little under half
    let half = ((body.w - COLUMN_GAP) * LEFT_SHARE).floor();
    let (box_x, box_w) = if two {
        (body.x + half + COLUMN_GAP, body.w - half - COLUMN_GAP)
    } else {
        (body.x, body.w)
    };
    let help = Rect {
        x: box_x,
        y: bottom - BUTTONS_H - BOTTOM_GAP - HELP_H,
        w: box_w,
        h: HELP_H,
    };
    let left_w = (box_w / 2.0).floor() - 2.0;
    let defaults = Rect {
        x: box_x,
        y: bottom - BUTTONS_H,
        w: left_w,
        h: BUTTONS_H,
    };
    let done = Rect {
        x: box_x + left_w + 4.0,
        w: box_w - left_w - 4.0,
        ..defaults
    };
    let mut l = OptionsLayout {
        display: Vec::new(),
        rules: Vec::new(),
        labels: Vec::new(),
        help,
        defaults,
        done,
        scroll: None,
    };
    if two {
        place(&display_lines(), body.x, body.y, half, &mut l);
        place(&rule_lines(), box_x, body.y, box_w, &mut l);
        return l;
    }
    let mut lines = display_lines();
    lines.extend(rule_lines());
    let room = column_room(body);
    let first = first.min(last_first(body));
    let shown = fitting(&lines, first, room);
    let w = body.w - SCROLL_ROOM;
    place(&lines[first..first + shown], body.x, body.y, w, &mut l);
    let list = Rect {
        x: body.x,
        y: body.y,
        w,
        h: room,
    };
    l.scroll = super::layout::scroll(&list, first, shown, lines.len());
    l
}

/// The body the options sit in for a GUI of `size`.
fn body_of(size: [f32; 2], cache: &ClientCache) -> Rect {
    let skills = cache.defs.as_ref().map_or(0, |d| d.defs.skills.len());
    super::window::window(size, skills, cache.first_tab).body
}

/// A row: its label at the left and its value on a button at the right; greyed when the player cannot change it.
fn draw_row(
    host: &dyn Host,
    batch: &mut HudBatch,
    r: &Rect,
    label: &str,
    value: &str,
    enabled: bool,
    hover: bool,
) {
    paint::fill(host, batch, r, BLACK);
    let face = if hover && enabled { CARD_HI } else { CARD };
    paint::fill(host, batch, &r.shrunk(1.0), face);
    let value_w = (host.text_width(value) + 10.0).max(VALUE_W).min(r.w / 2.0);
    let button = Rect {
        x: r.x + r.w - value_w - 1.0,
        y: r.y + 1.0,
        w: value_w,
        h: r.h - 2.0,
    };
    let look = if !enabled {
        Look::Off
    } else if value == say(&VAL_ON, &[]).plain() {
        Look::Go
    } else {
        Look::Plain
    };
    let value = paint::fitted(host, value, button.w - 4.0);
    paint::button(host, batch, &button, &value, look, false);
    let label = paint::fitted(host, label, button.x - 4.0 - (r.x + 5.0));
    host.text(
        batch,
        &label,
        [r.x + 5.0, r.y + 2.0],
        if enabled { WHITE } else { LOCKED },
        enabled,
    );
}

/// Draws the options in the window's body.
pub fn draw(host: &dyn Host, batch: &mut HudBatch, size: [f32; 2], mouse: [f32; 2]) {
    let over = |r: &Rect| r.contains(mouse[0], mouse[1]);
    with(|cache| {
        let body = body_of(size, cache);
        if cache.defs.is_none() {
            paint::text_centred(host, batch, &say(&WAITING, &[]).plain(), &body, paint::GRAY);
            return;
        }
        let l = options_layout(&body, cache.options_first);
        for (label, r) in &l.labels {
            match label {
                Label::Title(name) => {
                    let title = say(name, &[]).plain().to_uppercase();
                    host.text(batch, &title, [r.x, r.y], GOLD, true);
                    let after = r.x + host.text_width(&title) + 5.0;
                    let rule = Rect {
                        x: after,
                        y: r.y + 4.0,
                        w: r.x + r.w - after,
                        h: 1.0,
                    };
                    if rule.w > 0.0 {
                        paint::fill(host, batch, &rule, BRASS_DARK);
                    }
                }
                Label::Section(name) => {
                    host.text(batch, &say(name, &[]).plain(), [r.x, r.y + 1.0], DIM, false);
                }
                Label::OnlyOps if !cache.rules_editable => {
                    paint::padlock(host, batch, [r.x, r.y]);
                    let note = paint::fitted(host, &say(&ONLY_OPS, &[]).plain(), r.w - 11.0);
                    host.text(batch, &note, [r.x + 11.0, r.y + 1.0], DIM, false);
                }
                Label::OnlyOps => {}
            }
        }
        let mut hovered: Option<(&'static Message, &'static Message)> = None;
        for (choice, r) in &l.display {
            let label = say(choice.name(), &[]).plain();
            let value = choice.value(&cache.settings).plain();
            draw_row(host, batch, r, &label, &value, true, over(r));
            if over(r) {
                hovered = Some((choice.name(), choice.help()));
            }
        }
        for (choice, r) in &l.rules {
            let label = say(choice.name(), &[]).plain();
            let value = choice.value(&cache.rules).plain();
            draw_row(
                host,
                batch,
                r,
                &label,
                &value,
                cache.rules_editable,
                over(r),
            );
            if over(r) {
                hovered = Some((choice.name(), choice.help()));
            }
        }
        paint::fill(host, batch, &l.help, BLACK);
        paint::fill(host, batch, &l.help.shrunk(1.0), INSET);
        if let Some((name, help)) = hovered {
            let room = l.help.w - 10.0;
            let name = paint::fitted(host, &say(name, &[]).plain(), room);
            host.text(batch, &name, [l.help.x + 5.0, l.help.y + 2.0], GOLD, false);
            let help = paint::fitted(host, &say(help, &[]).plain(), room);
            host.text(
                batch,
                &help,
                [l.help.x + 5.0, l.help.y + 11.0],
                paint::GRAY,
                false,
            );
        }
        let reset = say(&RESET, &[]).plain();
        paint::button(
            host,
            batch,
            &l.defaults,
            &reset,
            Look::Plain,
            over(&l.defaults),
        );
        let done = say(&DONE, &[]).plain();
        paint::button(host, batch, &l.done, &done, Look::Go, over(&l.done));
        if let Some(scroll) = l.scroll {
            paint::fill(host, batch, &scroll.track, paint::INSET);
            paint::fill(host, batch, &scroll.thumb, paint::FRAME);
        }
    });
}

/// The tab Done and the right button lead back to: the one the options were opened from.
fn back(cache: &ClientCache) -> WindowTab {
    match cache.return_tab {
        WindowTab::Options => WindowTab::Home,
        tab => tab,
    }
}

/// A click on the options: a row changes its value (a rule only for operators), Defaults resets the display
/// options, Done and the right button go back.
pub fn click(host: &dyn Host, size: [f32; 2], at: [f32; 2], mouse_button: i32) -> Flow {
    with(|cache| {
        if mouse_button == RIGHT_BUTTON {
            cache.tab = back(cache);
            return;
        }
        if mouse_button != LEFT_BUTTON || cache.defs.is_none() {
            return;
        }
        let l = options_layout(&body_of(size, cache), cache.options_first);
        let on = |r: &Rect| r.contains(at[0], at[1]);
        let mut clicked = true;
        if on(&l.done) {
            cache.tab = back(cache);
        } else if on(&l.defaults) {
            let areas = std::mem::take(&mut cache.settings.shown_areas);
            cache.settings = Default::default();
            cache.settings.shown_areas = areas;
            cache.settings.save(host);
        } else if let Some((choice, _)) = l.display.iter().find(|(_, r)| on(r)) {
            choice.next(&mut cache.settings);
            cache.settings.save(host);
        } else if let Some((choice, _)) = l.rules.iter().find(|(_, r)| on(r)) {
            if cache.rules_editable {
                choice.next(&mut cache.rules);
                cache.outgoing.push(ToServer::SetRules(cache.rules.clone()));
            } else {
                clicked = false;
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

/// The wheel on a single column moves it a line.
pub fn scroll(cache: &mut ClientCache, size: [f32; 2], amount: f64) {
    let last = last_first(&body_of(size, cache));
    cache.options_first = if amount > 0.0 {
        cache.options_first.min(last).saturating_sub(1)
    } else if amount < 0.0 {
        (cache.options_first + 1).min(last)
    } else {
        cache.options_first
    };
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::HudBatch;
    use skills_core::rules::Rules;
    use skills_core::wire::ToServer;

    use super::*;
    use crate::MOD_ID;
    use crate::client::cache::{ClientSettings, Corner, Tab as WindowTab, WarningPlace, with};
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
        with(|cache| {
            cache.tab = WindowTab::Options;
            cache.defs = Some(Default::default());
        });
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

    fn laid(size: [f32; 2]) -> OptionsLayout {
        options_layout(&crate::client::window::window(size, 0, 0).body, 0)
    }

    fn row(l: &OptionsLayout, choice: Choice) -> Rect {
        l.display.iter().find(|(c, _)| *c == choice).unwrap().1
    }

    fn rule(l: &OptionsLayout, choice: RuleChoice) -> Rect {
        l.rules.iter().find(|(c, _)| *c == choice).unwrap().1
    }

    fn within(outer: &Rect, inner: &Rect) -> bool {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.x + inner.w <= outer.x + outer.w
            && inner.y + inner.h <= outer.y + outer.h
    }

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    #[test]
    fn options_fit_both_sizes() {
        // Given the window's body at the usual size and the smallest
        let usual_body = crate::client::window::window(SIZE, 0, 0).body;
        let small_body = crate::client::window::window(SMALLEST, 0, 0).body;

        // When the options are laid out on each
        let usual = laid(SIZE);
        let small = laid(SMALLEST);

        // Then the usual size has every display row left of every rule row, all inside the body, nothing
        // overlapping and no scroll bar
        assert_eq!((usual.display.len(), usual.rules.len()), (11, 7));
        let mut boxes: Vec<Rect> = usual.display.iter().map(|(_, r)| *r).collect();
        boxes.extend(usual.rules.iter().map(|(_, r)| *r));
        boxes.extend([usual.help, usual.defaults, usual.done]);
        for (i, a) in boxes.iter().enumerate() {
            assert!(within(&usual_body, a), "{a:?}");
            for b in &boxes[i + 1..] {
                assert!(!overlap(a, b), "{a:?} and {b:?}");
            }
        }
        let display_right = usual
            .display
            .iter()
            .map(|(_, r)| r.x + r.w)
            .fold(0.0, f32::max);
        assert!(usual.rules.iter().all(|(_, r)| r.x > display_right));
        assert!(usual.scroll.is_none());

        // And the smallest has one column that scrolls, what shows inside the body
        assert!(small.scroll.is_some());
        let x = small.display[0].1.x;
        let column = small
            .display
            .iter()
            .map(|(_, r)| r)
            .chain(small.rules.iter().map(|(_, r)| r));
        assert!(column.clone().all(|r| r.x == x));
        for r in column {
            assert!(within(&small_body, r), "{r:?}");
        }
        for r in [small.help, small.defaults, small.done] {
            assert!(within(&small_body, &r), "{r:?}");
        }
    }

    #[test]
    fn a_display_row_cycles_its_value() {
        // Given the options open
        let _serial = exclusive();
        let (host, dir) = open("change");
        let l = laid(SIZE);

        // When the padlock corner's row is clicked
        click(
            &host,
            SIZE,
            inside(&row(&l, Choice::LockCorner)),
            LEFT_BUTTON,
        );
        let saved = ClientSettings::load(&host);
        std::fs::remove_dir_all(&dir).unwrap();

        // Then the padlock moves to the top left, in the game and in the file
        let now = with(|cache| cache.settings.clone());
        assert_eq!(now.lock_corner, Corner::TopLeft);
        assert_eq!(saved, now);
    }

    fn rules_sent(host: &TestHost) -> Vec<ToServer> {
        host.sent_to_server
            .iter()
            .map(|(_, data)| serde_json::from_slice(data).unwrap())
            .filter(|m| matches!(m, ToServer::SetRules(_)))
            .collect()
    }

    #[test]
    fn an_operator_changes_a_rule() {
        // Given an operator's options
        let _serial = exclusive();
        let (mut host, dir) = open("rules");
        with(|cache| cache.rules_editable = true);
        let l = laid(SIZE);

        // When perk switches are clicked, then a tick passes
        click(
            &host,
            SIZE,
            inside(&rule(&l, RuleChoice::PerkSwitches)),
            LEFT_BUTTON,
        );
        horizon_client::mod_api::dispatch_tick(&mut host, &mods());
        std::fs::remove_dir_all(&dir).unwrap();

        // Then perk switches turn off, and the server is asked for the new rules
        let wanted = Rules {
            perk_switches: false,
            ..Rules::default()
        };
        assert_eq!(rules_sent(&host), vec![ToServer::SetRules(wanted.clone())]);
        assert_eq!(with(|cache| cache.rules.clone()), wanted);
    }

    #[test]
    fn a_player_cannot() {
        // Given a player's options
        let _serial = exclusive();
        let (mut host, dir) = open("player");
        let l = laid(SIZE);

        // When they are drawn, perk switches clicked, and a tick passes
        let painted = drawn(&host, AWAY);
        click(
            &host,
            SIZE,
            inside(&rule(&l, RuleChoice::PerkSwitches)),
            LEFT_BUTTON,
        );
        horizon_client::mod_api::dispatch_tick(&mut host, &mods());
        std::fs::remove_dir_all(&dir).unwrap();

        // Then the rules are greyed, the note says only operators can change them, and nothing is asked
        let greyed = painted
            .iter()
            .filter(|p| matches!(p, Paint::Text { text, rgb, .. } if text == "Perk switches" && *rgb == LOCKED))
            .count();
        assert_eq!(greyed, 1);
        assert!(
            texts(&painted)
                .iter()
                .any(|t| t == "Only operators can change these.")
        );
        assert!(rules_sent(&host).is_empty());
    }

    #[test]
    fn the_help_box_explains_the_hovered_row() {
        // Given the options open
        let _serial = exclusive();
        let (host, dir) = open("help");
        let l = laid(SIZE);

        // When the mouse is over the padlock corner's row
        let painted = drawn(&host, inside(&row(&l, Choice::LockCorner)));
        std::fs::remove_dir_all(&dir).unwrap();

        // Then the help box names it and says what it does
        let in_help: Vec<String> = painted
            .iter()
            .filter_map(|p| match p {
                Paint::Text { text, at, .. } if l.help.contains(at[0], at[1]) => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(in_help[0], "Padlock corner");
        assert!(in_help.len() >= 2, "{in_help:?}");
    }

    #[test]
    fn done_returns_where_the_player_was() {
        // Given the options opened from Mining's road, the padlock moved and a rule changed
        let _serial = exclusive();
        let (host, dir) = open("done");
        with(|cache| {
            cache.return_tab = WindowTab::Skill(2);
            cache.settings.lock_corner = Corner::TopLeft;
            cache.rules.perk_switches = false;
            cache.rules_editable = true;
        });
        let l = laid(SIZE);

        // When Defaults is clicked, then Done
        click(&host, SIZE, inside(&l.defaults), LEFT_BUTTON);
        let after_defaults = with(|cache| (cache.settings.lock_corner, cache.rules.perk_switches));
        click(&host, SIZE, inside(&l.done), LEFT_BUTTON);
        std::fs::remove_dir_all(&dir).unwrap();

        // Then the display options are back to their defaults and the rules untouched, and Done returns to the
        // road
        assert_eq!(
            after_defaults,
            (ClientSettings::default().lock_corner, false)
        );
        assert!(rules_sent(&host).is_empty());
        assert_eq!(with(|cache| cache.tab), WindowTab::Skill(2));
    }

    #[test]
    fn every_label_fits_its_row() {
        // Given the options at both sizes, with the longest values
        let _serial = exclusive();
        let (host, dir) = open("fit");
        with(|cache| {
            cache.settings.warning = WarningPlace::AboveHotbar;
            cache.settings.lock_corner = Corner::BottomLeft;
        });
        for size in [SIZE, SMALLEST] {
            // When they are drawn
            host.painted.borrow_mut().clear();
            draw(&host, &mut HudBatch::new([427, 240]), size, AWAY);
            let l = laid(size);

            // Then each row's texts lie inside it
            let rows = l
                .display
                .iter()
                .map(|(_, r)| r)
                .chain(l.rules.iter().map(|(_, r)| r));
            for r in rows {
                for p in host.painted.borrow().iter() {
                    if let Paint::Text { text, at, .. } = p
                        && r.contains(at[0], at[1] + 1.0)
                    {
                        assert!(
                            at[0] + host.text_width(text) <= r.x + r.w,
                            "{size:?}: {text}"
                        );
                    }
                }
            }
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
