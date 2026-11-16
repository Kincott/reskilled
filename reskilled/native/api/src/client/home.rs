//! Home: the player in a stone alcove beside their skills, grouped by focus area, each a row with a mini road and
//! its raise button.

use std::collections::BTreeSet;

use horizon_client::mod_api::Text;
use skills_core::defs::{Reward, skill_rgb};
use skills_core::progress::{Request, focus_cost, level, mastered, own_level, points_available};
use skills_core::registry::Defs;

use super::cache::ClientCache;
use super::layout::{Rect, Scroll};
use super::paint::{self, DIM, GOLD, GRAY, Look, WHITE};
use super::screen::{
    Outcome, Pen, can_raise, earned_of, focus_tip, owns, perks_of, price, raise_tip, skill_tip,
};
use super::steve;
use crate::text::{
    BONUS_BLOCK_XP, BONUS_BREAK, BONUS_KILL, BONUS_TRAVEL, FOCUS, FOCUSED, LEVEL, MASTERED, OTHER,
    OUT_OF, ROW_LEVEL, STAT_MASTERED, STAT_PERKS, STAT_POINTS, TOTAL_LEVEL, focus_name, number,
    say, skill_name,
};

/// The player's panel: its width, the alcove's height, Steve's scale and the stat lines' pitch.
const PANEL_W: f32 = 104.0;
const PANEL_PAD: f32 = 8.0;
const ALCOVE_TOP: f32 = 23.0;
const ALCOVE_H: f32 = 116.0;
const STEVE_SCALE: f32 = 3.0;
const STAT_PITCH: f32 = 11.0;
/// The skills: the gap after the panel, rows, headings and the gap between groups.
const PANEL_GAP: f32 = 6.0;
const ROW_H: f32 = 16.0;
const ROW_GAP: f32 = 1.0;
const HEAD_H: f32 = 12.0;
const GROUP_GAP: f32 = 3.0;
/// A row's raise button and a heading's Focus button.
const RAISE_W: f32 = 35.0;
const RAISE_H: f32 = 13.0;
const FOCUS_W: f32 = 56.0;
/// Room for the scroll bar right of the rows.
const SCROLL_ROOM: f32 = 6.0;
/// A row's parts, from its left: the name, the level's right edge, the mini road's start, and how much room the
/// road leaves at the right for the points and the raise button; the road is left out when narrower than this.
const NAME_X: f32 = 26.0;
const LEVEL_RIGHT: f32 = 108.0;
const ROAD_X: f32 = 116.0;
const ROAD_RIGHT_ROOM: f32 = 62.0;
const LEAST_ROAD: f32 = 24.0;
/// Colours of this page beyond the shared ones.
const BLACK: u32 = 0xFF07_080B;
const CARD: u32 = 0xFF23_2733;
const CARD_HI: u32 = 0xFF2B_3040;
const INSET: u32 = 0xFF0F_1116;
const MASTERED_ROW: u32 = 0xFF33_2B1D;
const BRASS_LIGHT: u32 = 0xFFE6_C36A;
const BRASS: u32 = 0xFFAD_8436;
const BRASS_DARK: u32 = 0xFF5A_4116;
const ROAD: u32 = 0xFF3A_4152;
const ROAD_DONE: u32 = 0xFFF2_C14E;
const PERK_OWNED: u32 = 0xFFF2_C14E;
const PERK_OPEN: u32 = 0xFF67_C14A;
const PERK_LOCKED: u32 = 0xFF2A_2F3B;
const SHADOW: u32 = 0x6600_0000;

/// A group of skills on Home: a focus area's, by id, or those in no area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub focus: Option<String>,
    /// Skills by their place in id order.
    pub skills: Vec<usize>,
}

/// The focus areas in id order with their skills as the area lists them, then every skill in no area.
pub fn groups(defs: &Defs) -> Vec<Group> {
    let ids: Vec<&String> = defs.skills.keys().collect();
    let mut placed = BTreeSet::new();
    let mut out = Vec::new();
    for (id, area) in &defs.focus {
        let skills: Vec<usize> = area
            .skills
            .iter()
            .filter_map(|s| ids.iter().position(|known| *known == s))
            .filter(|i| placed.insert(*i))
            .collect();
        if !skills.is_empty() {
            out.push(Group {
                focus: Some(id.clone()),
                skills,
            });
        }
    }
    let rest: Vec<usize> = (0..ids.len()).filter(|i| !placed.contains(i)).collect();
    if !rest.is_empty() {
        out.push(Group {
            focus: None,
            skills: rest,
        });
    }
    out
}

/// Where Home puts things: the player's panel, its alcove and stat lines, then the headings (with their Focus
/// buttons) and the skill rows (with their raise buttons) that fit from `first_row`, and the scroll bar.
#[derive(Debug, Clone, PartialEq)]
pub struct HomeLayout {
    pub panel: Rect,
    pub alcove: Rect,
    pub stats: [Rect; 3],
    pub headings: Vec<(Option<String>, Rect, Option<Rect>)>,
    pub rows: Vec<(usize, Rect, Rect)>,
    pub scroll: Option<Scroll>,
}

/// One line of the skills' list: a group's heading or a skill's row, with the room it takes below it.
enum Line {
    Heading(usize),
    Row(usize),
}

/// The list's lines; a list of one group in no area has no heading.
fn lines(groups: &[Group]) -> Vec<(Line, f32)> {
    let headed = !(groups.len() == 1 && groups[0].focus.is_none());
    let mut out = Vec::new();
    for (g, group) in groups.iter().enumerate() {
        if headed {
            out.push((Line::Heading(g), HEAD_H + ROW_GAP));
        }
        for (i, skill) in group.skills.iter().enumerate() {
            let last = i + 1 == group.skills.len();
            out.push((
                Line::Row(*skill),
                ROW_H + ROW_GAP + if last { GROUP_GAP } else { 0.0 },
            ));
        }
    }
    out
}

fn height_of(line: &Line) -> f32 {
    match line {
        Line::Heading(_) => HEAD_H,
        Line::Row(_) => ROW_H,
    }
}

/// How many lines from `first` fit in `room`.
fn fitting(lines: &[(Line, f32)], first: usize, room: f32) -> usize {
    let mut y = 0.0;
    let mut count = 0;
    for (line, step) in &lines[first.min(lines.len())..] {
        if y + height_of(line) > room {
            break;
        }
        y += step;
        count += 1;
    }
    count
}

/// The last line the list may start from: the first from which every line after fits.
pub fn last_first(body: &Rect, groups: &[Group]) -> usize {
    let lines = lines(groups);
    (0..lines.len())
        .find(|&first| first + fitting(&lines, first, body.h) == lines.len())
        .unwrap_or(0)
}

/// Home on a body of `body`, the list from line `first_row`.
pub fn home(body: &Rect, groups: &[Group], first_row: usize) -> HomeLayout {
    let panel = Rect {
        w: PANEL_W,
        ..*body
    };
    let alcove = Rect {
        x: panel.x + PANEL_PAD,
        y: panel.y + ALCOVE_TOP,
        w: panel.w - 2.0 * PANEL_PAD,
        h: ALCOVE_H,
    };
    let stats_top = alcove.y + alcove.h + 9.0;
    let stats = [0.0, 1.0, 2.0].map(|i| Rect {
        x: panel.x + PANEL_PAD,
        y: stats_top + i * STAT_PITCH,
        w: panel.w - 2.0 * PANEL_PAD,
        h: paint::GLYPH,
    });
    let lines = lines(groups);
    let first = first_row.min(last_first(body, groups));
    let shown = fitting(&lines, first, body.h);
    let x = panel.x + panel.w + PANEL_GAP;
    let scrolled = shown < lines.len();
    let w = body.x + body.w - x - if scrolled { SCROLL_ROOM } else { 0.0 };
    let list = Rect {
        x,
        y: body.y,
        w,
        h: body.h,
    };
    let scroll = super::layout::scroll(&list, first, shown, lines.len());
    let (mut headings, mut rows) = (Vec::new(), Vec::new());
    let mut y = body.y;
    for (line, step) in &lines[first..first + shown] {
        match line {
            Line::Heading(g) => {
                let heading = Rect { x, y, w, h: HEAD_H };
                let focus = groups[*g].focus.clone();
                let button = focus.as_ref().map(|_| Rect {
                    x: x + w - FOCUS_W,
                    y,
                    w: FOCUS_W,
                    h: HEAD_H - 1.0,
                });
                headings.push((focus, heading, button));
            }
            Line::Row(skill) => {
                let row = Rect { x, y, w, h: ROW_H };
                let raise = Rect {
                    x: x + w - RAISE_W - 5.0,
                    y: y + ((ROW_H - RAISE_H) / 2.0).floor(),
                    w: RAISE_W,
                    h: RAISE_H,
                };
                rows.push((*skill, row, raise));
            }
        }
        y += step;
    }
    HomeLayout {
        panel,
        alcove,
        stats,
        headings,
        rows,
        scroll,
    }
}

/// Draws Home: the player's panel, the headings and the rows, and the tooltip under the mouse.
pub(super) fn draw(pen: &mut Pen<'_>, l: &HomeLayout, skills: &[&String], name: Option<&str>) {
    draw_panel(pen, l, skills, name);
    for (focus, heading, button) in &l.headings {
        draw_heading(pen, focus.as_deref(), heading, button.as_ref());
    }
    for (skill, row, raise) in &l.rows {
        if let Some(id) = skills.get(*skill) {
            draw_row(pen, id, row, raise);
        }
    }
    pen.scroll(l.scroll);
}

fn draw_panel(pen: &mut Pen<'_>, l: &HomeLayout, skills: &[&String], name: Option<&str>) {
    let (d, state) = (pen.d, pen.state);
    let p = &l.panel;
    pen.fill(p, BLACK);
    pen.fill(&p.shrunk(1.0), CARD);
    let total: u32 = skills.iter().map(|s| level(state, d, s)).sum();
    let total = say(&TOTAL_LEVEL, &[number(total)]).plain();
    let right = p.x + p.w - PANEL_PAD;
    pen.text_right(&total, right, p.y + 6.0, GOLD);
    if let Some(name) = name {
        let room = right - pen.width(&total) - 4.0 - (p.x + PANEL_PAD);
        let name = paint::fitted(pen.host, name, room);
        pen.text(&name, p.x + PANEL_PAD, p.y + 6.0, WHITE);
    }
    let rule = |y: f32| Rect {
        x: p.x + PANEL_PAD,
        y,
        w: p.w - 2.0 * PANEL_PAD,
        h: 1.0,
    };
    pen.fill(&rule(p.y + 18.0), BRASS_DARK);
    let a = &l.alcove;
    pen.fill(a, INSET);
    paint::stone(pen.host, pen.batch, &a.shrunk(1.0));
    let middle = (a.x + a.w / 2.0).floor();
    pen.fill(
        &Rect {
            x: middle - 26.0,
            y: a.y + a.h - 10.0,
            w: 52.0,
            h: 5.0,
        },
        SHADOW,
    );
    let at = [middle - 8.0 * STEVE_SCALE, a.y + 9.0];
    steve::draw_steve(pen.host, pen.batch, at, STEVE_SCALE, pen.mouse);
    pen.fill(&rule(l.stats[0].y - 5.0), BRASS_DARK);
    let owned = d.perks.keys().filter(|p| owns(state, p)).count() as u32;
    let done = skills.iter().filter(|s| mastered(d, state, s)).count() as u32;
    let points: u32 = skills.iter().map(|s| points_available(d, state, s)).sum();
    let out_of = |a: u32, b: u32| say(&OUT_OF, &[number(a), number(b)]).plain();
    let values = [
        (&STAT_PERKS, out_of(owned, d.perks.len() as u32), WHITE),
        (&STAT_MASTERED, out_of(done, skills.len() as u32), GOLD),
        (
            &STAT_POINTS,
            points.to_string(),
            if points > 0 { GOLD } else { GRAY },
        ),
    ];
    for ((label, value, colour), r) in values.into_iter().zip(&l.stats) {
        pen.text(&say(label, &[]).plain(), r.x, r.y, DIM);
        pen.text_right(&value, r.x + r.w, r.y, colour);
    }
}

/// What a focus area adds, in a few words, from its first reward.
fn bonus(d: &Defs, focus: &str) -> Option<Text> {
    let percent = |p: u32| Text::literal(format!("{p}%"));
    Some(match d.focus.get(focus)?.rewards.first()? {
        Reward::Kill { bonus_percent } => say(&BONUS_KILL, &[percent(*bonus_percent)]),
        Reward::BlockXp { bonus_percent } => say(&BONUS_BLOCK_XP, &[percent(*bonus_percent)]),
        Reward::Travel { .. } => say(&BONUS_TRAVEL, &[]),
        _ => say(&BONUS_BREAK, &[]),
    })
}

fn draw_heading(pen: &mut Pen<'_>, focus: Option<&str>, h: &Rect, button: Option<&Rect>) {
    let d = pen.d;
    let active = focus.is_some_and(|f| pen.state.focus.contains(f));
    let title = match focus {
        Some(f) => focus_name(d, f).plain().to_uppercase(),
        None => say(&OTHER, &[]).plain().to_uppercase(),
    };
    let right = h.x + h.w - FOCUS_W - 6.0;
    pen.text(
        &title,
        h.x + 2.0,
        h.y + 2.0,
        if active { GOLD } else { GRAY },
    );
    let mut x = h.x + 4.0 + pen.width(&title);
    if let Some(bonus) = focus.and_then(|f| bonus(d, f)) {
        let bonus = bonus.plain();
        if x + 4.0 + pen.width(&bonus) <= right {
            pen.text(&bonus, x + 4.0, h.y + 2.0, DIM);
            x += 4.0 + pen.width(&bonus);
        }
    }
    if right - (x + 4.0) >= 4.0 {
        let line = Rect {
            x: x + 4.0,
            y: h.y + 6.0,
            w: right - x - 4.0,
            h: 1.0,
        };
        pen.fill(&line, if active { BRASS } else { ROAD });
    }
    if active {
        pen.text_right(
            &say(&FOCUSED, &[]).plain(),
            h.x + h.w - 4.0,
            h.y + 2.0,
            GOLD,
        );
    } else if let Some(button) = button {
        pen.button(button, &say(&FOCUS, &[]).plain(), Look::Plain);
    }
    if let Some(f) = focus
        && pen.over(h)
    {
        pen.tip = Some(focus_tip(pen, f, active));
    }
}

fn draw_row(pen: &mut Pen<'_>, skill: &str, row: &Rect, raise: &Rect) {
    let (d, state) = (pen.d, pen.state);
    let done = mastered(d, state, skill);
    let hover = pen.over(row);
    pen.fill(row, BLACK);
    let face = if done {
        MASTERED_ROW
    } else if hover {
        CARD_HI
    } else {
        CARD
    };
    pen.fill(&row.shrunk(1.0), face);
    let accent = d.skill(skill).map_or(0x8F_A6B8, skill_rgb);
    pen.fill(
        &Rect {
            x: row.x + 1.0,
            y: row.y + 1.0,
            w: 2.0,
            h: row.h - 2.0,
        },
        0xFF00_0000 | accent,
    );
    if hover {
        paint::edge(pen.host, pen.batch, row, BRASS_LIGHT);
    }
    let now = level(state, d, skill);
    if let Some(def) = d.skill(skill) {
        pen.host
            .item(pen.batch, def.icon_at(now), [row.x + 6.0, row.y], 16.0);
    }
    let ty = row.y + ((row.h - paint::GLYPH) / 2.0).floor() + 1.0;
    let lv = say(&ROW_LEVEL, &[number(now)]).plain();
    let lv_right = row.x + LEVEL_RIGHT;
    pen.text_right(&lv, lv_right, ty, if done { GOLD } else { WHITE });
    let room = lv_right - pen.width(&lv) - 4.0 - (row.x + NAME_X);
    let name = paint::fitted(pen.host, &skill_name(d, skill).plain(), room);
    pen.text(&name, row.x + NAME_X, ty, WHITE);
    let right = row.x + row.w;
    let mastered_label = say(&MASTERED, &[]).plain();
    let label_x = right - 6.0 - pen.width(&mastered_label);
    let crown_x = label_x - paint::CROWN_SIZE[0] - 2.0;
    // A mastered row's road stops short of its crown
    let road_end = if done {
        (right - ROAD_RIGHT_ROOM).min(crown_x - 6.0)
    } else {
        right - ROAD_RIGHT_ROOM
    };
    let road = Rect {
        x: row.x + ROAD_X,
        y: row.y + (row.h / 2.0).floor() - 1.0,
        w: road_end - (row.x + ROAD_X),
        h: 2.0,
    };
    if road.w >= LEAST_ROAD {
        draw_mini_road(pen, skill, &road);
    }
    if done {
        paint::crown(pen.host, pen.batch, [crown_x, ty]);
        pen.text(&mastered_label, label_x, ty, GOLD);
    } else {
        let points = points_available(d, state, skill);
        if points > 0 {
            let label = format!("{}{points}", paint::POINT);
            pen.text_right(&label, raise.x - 4.0, ty, GOLD);
        }
        draw_raise(pen, skill, raise);
    }
    if hover {
        let over_raise = !done && pen.rules.buying() && pen.over(raise);
        pen.tip = Some(if over_raise {
            raise_tip(pen, skill)
        } else {
            skill_tip(pen, skill)
        });
    }
}

/// The raise button, or, when skills only grow by doing, the progress toward the next level in its place.
fn draw_raise(pen: &mut Pen<'_>, skill: &str, raise: &Rect) {
    let (d, state) = (pen.d, pen.state);
    if !pen.rules.buying() {
        if let Some((have, need)) = earned_of(d, state, skill) {
            let line = say(&LEVEL, &[number(have), number(need)]).plain();
            paint::text_centred(pen.host, pen.batch, &line, raise, GRAY);
        }
    } else if let Some(cost) = price(d, pen.rules, state, skill) {
        let look = if can_raise(d, pen.rules, state, pen.xp, skill) {
            Look::Go
        } else {
            Look::Off
        };
        let hover = pen.over(raise);
        paint::arrow_button(pen.host, pen.batch, raise, &cost.to_string(), look, hover);
    }
}

/// The skill's road in small: a track filled to its level, with a mark at each perk's level, gold when owned,
/// green when reached, dark when not.
fn draw_mini_road(pen: &mut Pen<'_>, skill: &str, road: &Rect) {
    let (d, state) = (pen.d, pen.state);
    let top = d.max_level(skill).max(2);
    let at = |lv: u32| (road.x + (lv.clamp(1, top) - 1) as f32 * road.w / (top - 1) as f32).round();
    pen.fill(
        &Rect {
            x: road.x - 1.0,
            y: road.y - 1.0,
            w: road.w + 2.0,
            h: road.h + 2.0,
        },
        BLACK,
    );
    pen.fill(road, ROAD);
    let reached = own_level(state, d, skill);
    pen.fill(
        &Rect {
            w: at(reached.max(1)) - road.x,
            ..*road
        },
        ROAD_DONE,
    );
    for perk in perks_of(d, skill) {
        let colour = if owns(state, &perk.id) {
            PERK_OWNED
        } else if reached >= perk.min_level {
            PERK_OPEN
        } else {
            PERK_LOCKED
        };
        let x = at(perk.min_level);
        pen.fill(
            &Rect {
                x: x - 3.0,
                y: road.y - 3.0,
                w: 7.0,
                h: 8.0,
            },
            BLACK,
        );
        pen.fill(
            &Rect {
                x: x - 2.0,
                y: road.y - 2.0,
                w: 5.0,
                h: 6.0,
            },
            colour,
        );
    }
}

/// What a left click on Home does: a heading's Focus button adds its area when the player can pay, a raise
/// button raises `times` when it can be paid, and anywhere else on a row opens that skill.
pub(super) fn click(
    cache: &ClientCache,
    d: &Defs,
    l: &HomeLayout,
    skills: &[&String],
    at: [f32; 2],
    times: u32,
) -> Outcome {
    let (state, rules, xp) = (&cache.state, &cache.rules, cache.xp_levels);
    let hit = |r: &Rect| r.contains(at[0], at[1]);
    for (focus, _, button) in &l.headings {
        if let (Some(focus), Some(button)) = (focus, button)
            && hit(button)
        {
            let affordable = xp.is_some_and(|xp| xp >= focus_cost(d, state));
            return if !state.focus.contains(focus) && affordable {
                Outcome::Ask(Request::AddFocus {
                    focus: focus.clone(),
                })
            } else {
                Outcome::Nothing
            };
        }
    }
    for (skill, row, raise) in &l.rows {
        let Some(id) = skills.get(*skill) else {
            continue;
        };
        if hit(raise) && !mastered(d, state, id) && rules.buying() {
            return if can_raise(d, rules, state, xp, id) {
                Outcome::Ask(Request::Raise {
                    skill: (*id).clone(),
                    times,
                })
            } else {
                Outcome::Nothing
            };
        }
        if hit(row) {
            return Outcome::Open(*skill);
        }
    }
    Outcome::Nothing
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::HudBatch;
    use skills_core::defs::Reward;
    use skills_core::progress::{PlayerState, Request, SkillProgress};
    use skills_core::registry::Defs;
    use skills_core::rules::Progression;
    use skills_core::wire::ClientDefs;

    use super::*;
    use crate::MOD_ID;
    use crate::client::cache::{Tab, with};
    use crate::client::layout::Rect;
    use crate::client::screen::tests::{focus, perk, sent_acts, skill};
    use crate::client::screen::{LEFT_BUTTON, click, draw};
    use crate::client::window::window;
    use crate::test_host::{TestHost, exclusive};

    const SIZES: [[f32; 2]; 2] = [[320.0, 240.0], [427.0, 240.0]];
    const SIZE: [f32; 2] = [427.0, 240.0];
    const AWAY: [f32; 2] = [1.0, 1.0];
    /// The eight skills, in id order.
    const NAMES: [&str; 8] = [
        "agility",
        "arcane",
        "building",
        "defense",
        "gathering",
        "melee",
        "mining",
        "ranged",
    ];
    const MELEE: usize = 5;
    const MINING: usize = 6;

    fn id(name: &str) -> String {
        format!("reskilled:{name}")
    }

    /// Eight skills with five perks each (levels 4, 8, 12, 16, 24), in Fighting, Roaming and Working as shipped.
    fn eight() -> Defs {
        let skills = NAMES.iter().map(|n| (id(n), skill(&id(n)))).collect();
        let perks = NAMES
            .iter()
            .flat_map(|n| {
                [4, 8, 12, 16, 24].map(|lv| {
                    let mut p = perk(&format!("{n}_{lv}"), lv, &[]);
                    p.skill = id(n);
                    p
                })
            })
            .map(|p| (p.id.clone(), p))
            .collect();
        let mut fighting = focus(
            "reskilled:fighting",
            &[&id("melee"), &id("ranged"), &id("defense")],
        );
        fighting.1.rewards = vec![Reward::Kill { bonus_percent: 50 }];
        let mut roaming = focus("reskilled:roaming", &[&id("agility"), &id("building")]);
        roaming.1.rewards = vec![Reward::Travel { points: 1.0 }];
        let mut working = focus(
            "reskilled:working",
            &[&id("mining"), &id("gathering"), &id("arcane")],
        );
        working.1.rewards = vec![Reward::BlockXp { bonus_percent: 50 }];
        Defs {
            skills,
            perks,
            focus: [fighting, roaming, working].into(),
            ..Defs::default()
        }
    }

    /// The player at Mining 12 with 23 XP levels, Working their focus.
    fn player() -> PlayerState {
        let mut state = PlayerState {
            focus: [id("working")].into(),
            focus_chosen_before: true,
            ..PlayerState::default()
        };
        state.skills.insert(
            id("mining"),
            SkillProgress {
                level: 12,
                ..SkillProgress::default()
            },
        );
        state
    }

    fn open(defs: Defs, state: PlayerState) -> TestHost {
        with(|cache| {
            cache.defs = Some(ClientDefs {
                defs,
                labels: Default::default(),
                label_icons: Default::default(),
            });
            cache.state = state;
            cache.xp_levels = Some(23);
            cache.tab = Tab::Home;
            cache.first_row = 0;
            cache.my_name = Some("Ann".into());
        });
        TestHost {
            open_screen: Some(MOD_ID.into()),
            ..TestHost::default()
        }
    }

    fn laid_out() -> HomeLayout {
        with(|cache| {
            let d = &cache.defs.as_ref().unwrap().defs;
            home(&window(SIZE, d.skills.len(), 0).body, &groups(d), 0)
        })
    }

    fn texts(host: &TestHost) -> Vec<String> {
        draw(host, &mut HudBatch::new([427, 240]), SIZE, AWAY);
        host.drawn_text()
    }

    fn row(skill: usize) -> (Rect, Rect) {
        let (_, row, raise) = *laid_out().rows.iter().find(|(s, ..)| *s == skill).unwrap();
        (row, raise)
    }

    fn inside(outer: &Rect, inner: &Rect) -> bool {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.x + inner.w <= outer.x + outer.w
            && inner.y + inner.h <= outer.y + outer.h
    }

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    #[test]
    fn groups_follow_the_focus_areas_then_other() {
        // Given the shipped areas and a ninth skill in none, and the same skills with no areas at all
        let mut extra = eight();
        extra.skills.insert(id("zz_extra"), skill(&id("zz_extra")));
        let bare = Defs {
            focus: Default::default(),
            ..eight()
        };

        // When they are grouped
        let shipped = groups(&extra);
        let ungrouped = groups(&bare);

        // Then the areas come in id order with their skills as listed, the ninth last under no area; without
        // areas, one group holds every skill
        let areas: Vec<Option<&str>> = shipped.iter().map(|g| g.focus.as_deref()).collect();
        assert_eq!(
            areas,
            [
                Some("reskilled:fighting"),
                Some("reskilled:roaming"),
                Some("reskilled:working"),
                None
            ]
        );
        assert_eq!(shipped[0].skills, vec![MELEE, 7, 3]);
        assert_eq!(shipped[3].skills, vec![8]);
        assert_eq!(ungrouped.len(), 1);
        assert_eq!(ungrouped[0].focus, None);
        assert_eq!(ungrouped[0].skills, (0..8).collect::<Vec<_>>());
    }

    #[test]
    fn home_fits_both_sizes() {
        for size in SIZES {
            // Given eight skills in three areas
            let body = window(size, 8, 0).body;

            // When Home is laid out
            let l = home(&body, &groups(&eight()), 0);

            // Then the panel, alcove and stats are inside the body, every row and heading too and right of the
            // panel, none overlapping, all eight rows shown with no scroll bar
            assert!(inside(&body, &l.panel), "{size:?}");
            assert!(inside(&l.panel, &l.alcove), "{size:?}");
            assert!(l.stats.iter().all(|s| inside(&l.panel, s)), "{size:?}");
            assert_eq!((l.rows.len(), l.headings.len()), (8, 3), "{size:?}");
            let mut boxes: Vec<Rect> = l.rows.iter().map(|(_, r, _)| *r).collect();
            boxes.extend(l.headings.iter().map(|(_, h, _)| *h));
            for (i, a) in boxes.iter().enumerate() {
                assert!(inside(&body, a), "{size:?}: {a:?}");
                assert!(a.x >= l.panel.x + l.panel.w, "{size:?}: {a:?}");
                for b in &boxes[i + 1..] {
                    assert!(!overlap(a, b), "{size:?}: {a:?} and {b:?}");
                }
            }
            for (_, row, raise) in &l.rows {
                assert!(inside(row, raise), "{size:?}");
            }
            for (_, heading, button) in &l.headings {
                assert!(button.is_none_or(|b| inside(heading, &b)), "{size:?}");
            }
            assert!(l.scroll.is_none(), "{size:?}");
        }

        // And fourteen skills scroll
        let mut many = eight();
        for i in 0..6 {
            many.skills
                .insert(id(&format!("zz_{i}")), skill(&id(&format!("zz_{i}"))));
        }
        let l = home(&window(SIZE, 14, 0).body, &groups(&many), 0);
        assert!(l.scroll.is_some());
    }

    #[test]
    fn the_stats_count_perks_mastery_and_points() {
        // Given Mining at the top with its five perks and Melee at 8 with two, 12 points left in all
        let _serial = exclusive();
        let mut state = player();
        let mining = state.skills.get_mut(&id("mining")).unwrap();
        mining.level = 32;
        mining.points_spent = 4;
        mining.perks = [4, 8, 12, 16, 24]
            .iter()
            .map(|lv| id(&format!("mining_{lv}")))
            .collect();
        state.skills.insert(
            id("melee"),
            SkillProgress {
                level: 8,
                points_spent: 4,
                perks: [id("melee_4"), id("melee_8")].into(),
                ..SkillProgress::default()
            },
        );
        let host = open(eight(), state);

        // When Home is drawn
        let drawn = texts(&host);

        // Then it counts 7 of 40 perks, 1 of 8 skills mastered and 12 points, beside the player's name
        for expected in [
            "Ann", "7 / 40", "1 / 8", "12", "Perks", "Mastered", "Points",
        ] {
            assert!(drawn.iter().any(|t| t == expected), "{expected}: {drawn:?}");
        }
    }

    #[test]
    fn a_row_raises_and_opens_its_skill() {
        // Given Home with Mining at 12 and 23 XP levels
        let _serial = exclusive();
        let mut host = open(eight(), player());
        let (mining, raise) = row(MINING);

        // When the raise button is clicked, with Shift, with Control, and the row elsewhere
        click(&host, SIZE, [raise.x + 3.0, raise.y + 3.0], LEFT_BUTTON);
        host.shift = true;
        click(&host, SIZE, [raise.x + 3.0, raise.y + 3.0], LEFT_BUTTON);
        host.shift = false;
        host.control = true;
        click(&host, SIZE, [raise.x + 3.0, raise.y + 3.0], LEFT_BUTTON);
        host.control = false;
        let acts = sent_acts(&mut host);
        click(&host, SIZE, [mining.x + 30.0, mining.y + 4.0], LEFT_BUTTON);

        // Then it asks for one, five and every raise, and the row opens Mining's tab
        let raise = |times| Request::Raise {
            skill: id("mining"),
            times,
        };
        assert_eq!(acts, vec![raise(1), raise(5), raise(u32::MAX)]);
        assert_eq!(with(|cache| cache.tab), Tab::Skill(MINING));

        // And with no XP the raise asks nothing more
        with(|cache| {
            cache.tab = Tab::Home;
            cache.xp_levels = Some(0);
        });
        let (_, raise) = row(MINING);
        click(&host, SIZE, [raise.x + 3.0, raise.y + 3.0], LEFT_BUTTON);
        assert_eq!(sent_acts(&mut host).len(), 3, "nothing more is asked");
    }

    #[test]
    fn a_group_heading_adds_its_focus() {
        // Given Home with Working the player's focus
        let _serial = exclusive();
        let mut host = open(eight(), player());
        let drawn = texts(&host);
        let headings = laid_out().headings;
        let button = |area: &str| {
            headings
                .iter()
                .find(|(f, ..)| f.as_deref() == Some(area))
                .and_then(|(.., b)| *b)
                .unwrap()
        };

        // When Working's button spot and Roaming's Focus button are clicked
        let working = button("reskilled:working");
        click(&host, SIZE, [working.x + 3.0, working.y + 3.0], LEFT_BUTTON);
        let roaming = button("reskilled:roaming");
        click(&host, SIZE, [roaming.x + 3.0, roaming.y + 3.0], LEFT_BUTTON);

        // Then Roaming is asked for, Working is marked focused with no button, and the other two have one
        assert_eq!(
            sent_acts(&mut host),
            vec![Request::AddFocus {
                focus: id("roaming")
            }]
        );
        assert!(drawn.iter().any(|t| t == "\u{2714} Focused"), "{drawn:?}");
        assert_eq!(
            drawn.iter().filter(|t| *t == "Focus").count(),
            2,
            "{drawn:?}"
        );
        for heading in [
            "FIGHTING",
            "+50% kill XP",
            "ROAMING",
            "travel XP",
            "WORKING",
            "+50% ore XP",
        ] {
            assert!(drawn.iter().any(|t| t == heading), "{heading}: {drawn:?}");
        }
    }

    #[test]
    fn mastered_and_earned_rows() {
        // Given Mining mastered and Melee at 12 with 37 of its 120 actions
        let _serial = exclusive();
        let mut state = player();
        let mining = state.skills.get_mut(&id("mining")).unwrap();
        mining.level = 32;
        mining.perks = [4, 8, 12, 16, 24]
            .iter()
            .map(|lv| id(&format!("mining_{lv}")))
            .collect();
        state.skills.insert(
            id("melee"),
            SkillProgress {
                level: 12,
                earned: 3_700,
                ..SkillProgress::default()
            },
        );
        let mut host = open(eight(), state);

        // When Home is drawn, Mining's raise spot clicked, then Home drawn with skills growing only by doing
        let buying = texts(&host);
        let (_, raise) = row(MINING);
        click(&host, SIZE, [raise.x + 3.0, raise.y + 3.0], LEFT_BUTTON);
        let opened = with(|cache| cache.tab);
        with(|cache| {
            cache.tab = Tab::Home;
            cache.rules.progression = Progression::Earn;
        });
        host.drawn.borrow_mut().clear();
        let earning = texts(&host);

        // Then Mining says Mastered and its spot opens it rather than raising; earning, Melee shows 37/120
        assert!(buying.iter().any(|t| t == "Mastered"), "{buying:?}");
        assert_eq!(opened, Tab::Skill(MINING));
        assert!(sent_acts(&mut host).is_empty());
        assert!(earning.iter().any(|t| t == "37/120"), "{earning:?}");
    }
}
