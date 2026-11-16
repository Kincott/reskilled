//! A skill's page: a road from level 1 to the top, the gear each level opens marked above it and the perks in
//! advancement frames below it, on vanilla's stone backdrop; a header with the level and its raise; and a dock that
//! tells about one perk and unlocks it.

use horizon_client::mod_api::Text;
use skills_core::area::area_of;
use skills_core::defs::{Effect, PerkDef, skill_rgb};
use skills_core::progress::{
    PlayerState, Request, level, mastered, own_level, points_available, switched_off,
};
use skills_core::wire::ClientDefs;

use super::cache::ClientCache;
use super::layout::Rect;
use super::paint::{self, DIM, GOLD, GRAY, GREEN, Look, WHITE};
use super::screen::{
    Outcome, Pen, PerkStatus, can_raise, draw_mastered_plaque, earned_of, marked, met_colour, owns,
    perk_tip, perks_of, price, raise_tip, skill_tip, status,
};
use crate::text::{
    AREA, AT_LEVEL, AT_TOP, GET, LEVEL, LEVEL_OF, LEVEL_UP_COST, MASTERED, OWNED, PERK_AT,
    PERK_COST, PERK_POINTS_N, PROGRESS, READY, READY_IN, SHARED_OWN, SWITCHED_OFF, TIP_NEED,
    VAL_OFF, VAL_ON, XP_CHIP, label_name, number, perk_description, perk_name, say, skill_name,
};

/// The header's height, the road panel's top below the body's and its height, and the gap before the dock.
const HEADER_H: f32 = 26.0;
const ROAD_TOP: f32 = 33.0;
const ROAD_H: f32 = 96.0;
const DOCK_GAP: f32 = 5.0;
/// The road's ends sit this far in from the panel's sides; a level takes at least this many pixels, the road
/// scrolling when that is wider than the panel.
const ROAD_INSET: f32 = 15.0;
const LEAST_STEP: f32 = 4.0;
/// The road line's height below the panel's top.
const LINE_Y: f32 = 45.0;
/// Perk nodes (advancement frames) and the gear marks above the road, and how far apart those sharing a level sit.
const NODE: f32 = 26.0;
const NODE_PITCH: f32 = 28.0;
const MARK: f32 = 18.0;
/// The least gap between two marks' slots, and between a slot and the mastery frame.
const MARK_GAP: f32 = 2.0;
/// The header's raise button (narrower on a narrow body, where it shows only the price), the dock's button and its
/// area button.
const RAISE_W: f32 = 112.0;
const NARROW_RAISE_W: f32 = 72.0;
const NARROW_BODY: f32 = 340.0;
const RAISE_H: f32 = 20.0;
const DOCK_BUTTON_W: f32 = 64.0;
const DOCK_AREA_W: f32 = 56.0;
const DOCK_BUTTON_H: f32 = 16.0;
/// How far one notch of the wheel moves a long road.
const WHEEL_STEP: f32 = 24.0;
/// Colours.
const BLACK: u32 = 0xFF07_080B;
const CARD: u32 = 0xFF23_2733;
const INSET: u32 = 0xFF0F_1116;
const INNER_SHADOW: u32 = 0x6600_0000;
pub const ROAD: u32 = 0xFF3A_4152;
const ROAD_DONE: u32 = 0xFFF2_C14E;
const ROAD_SHINE: u32 = 0xFFFF_E79A;
const TICK_DONE: u32 = 0xFF8A_6B2A;
const TICK_AHEAD: u32 = 0xFF2A_2F3B;
const PLATE_HERE: u32 = 0xFFF2_C14E;
const PLATE_PAST: u32 = 0xFF4A_3A16;
const PLATE_AHEAD: u32 = 0xFF1B_1E26;
const PLATE_HERE_TEXT: u32 = 0x1A_1408;
const MARKER: u32 = 0xFFFF_FFFF;
const DIMMED: u32 = 0xA810_1218;
pub const PERK_OPEN: u32 = 0xFF67_C14A;
const PERK_OWNED: u32 = 0xFFF2_C14E;
/// The line joining a perk to one it requires: gold once that one is owned.
pub const PERK_OWNED_LINE: u32 = 0xFFF2_C14E;
const PERK_NEEDED_LINE: u32 = 0xFF3A_4152;
const SELECTED: u32 = 0xFFFF_FFFF;
const FRAMES: &str = "gui/sprites/advancements/";
const UNTINTED: u32 = 0xFFFF_FFFF;

/// Where a skill's page puts things. `nodes` are the perks (by id) and `marks` the gear (by label and level) in
/// road order; the road runs from `origin` (level 1) a `step` a level, scrolled when longer than the panel.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadLayout {
    pub header: Rect,
    pub raise: Rect,
    pub road: Rect,
    pub dock: Rect,
    pub dock_button: Rect,
    pub dock_area: Rect,
    pub nodes: Vec<(String, Rect)>,
    pub marks: Vec<(String, u32, Rect)>,
    pub mastery: Rect,
    pub top: u32,
    pub origin: f32,
    pub step: f32,
    /// The road's whole length and the length the panel shows.
    pub span: f32,
    pub shown: f32,
}

/// Where level `level` sits along the road.
pub fn level_x(r: &RoadLayout, level: u32) -> f32 {
    (r.origin + (level.clamp(1, r.top) - 1) as f32 * r.step).round()
}

/// How far a long road can scroll; none when it fits.
pub fn max_scroll(r: &RoadLayout) -> f32 {
    (r.span - r.shown).max(0.0)
}

/// Things that share a level sit side by side, `pitch` apart, centred on the level.
fn side_by_side<T: Clone>(
    items: &[(T, u32)],
    pitch: f32,
    at: impl Fn(u32) -> f32,
) -> Vec<(T, u32, f32)> {
    let mut out = Vec::new();
    for (i, (item, lvl)) in items.iter().enumerate() {
        let same: Vec<usize> = (0..items.len()).filter(|j| items[*j].1 == *lvl).collect();
        let place = same.iter().position(|j| *j == i).unwrap_or(0) as f32;
        let offset = (place - (same.len() - 1) as f32 / 2.0) * pitch;
        out.push((item.clone(), *lvl, (at(*lvl) + offset).round()));
    }
    out
}

/// Pushes each of `xs` (left edges, in road order) right until it is at least `width` and a 2-pixel gap past the
/// one before, so neighbours a level or two apart on a short road never overlap.
fn spread(xs: &mut [f32], width: f32) {
    for i in 1..xs.len() {
        xs[i] = xs[i].max(xs[i - 1] + width + 2.0);
    }
}

/// A skill's page on a body of `body`: its road to `top_level`, `perks` (id and level) below it and `marks`
/// (label and level) above it, scrolled `scroll` pixels along.
pub fn road(
    body: &Rect,
    top_level: u32,
    perks: &[(String, u32)],
    marks: &[(String, u32)],
    scroll: f32,
) -> RoadLayout {
    let header = Rect {
        h: HEADER_H,
        ..*body
    };
    let raise_w = if body.w >= NARROW_BODY {
        RAISE_W
    } else {
        NARROW_RAISE_W
    };
    let raise = Rect {
        x: body.x + body.w - raise_w,
        y: body.y + 3.0,
        w: raise_w,
        h: RAISE_H,
    };
    let road = Rect {
        y: body.y + ROAD_TOP,
        h: ROAD_H,
        ..*body
    };
    let dock_y = road.y + road.h + DOCK_GAP;
    let dock = Rect {
        y: dock_y,
        h: body.y + body.h - dock_y,
        ..*body
    };
    let dock_button = Rect {
        x: dock.x + dock.w - DOCK_BUTTON_W - 6.0,
        y: dock.y + 7.0,
        w: DOCK_BUTTON_W,
        h: DOCK_BUTTON_H,
    };
    let dock_area = Rect {
        x: dock_button.x - DOCK_AREA_W - 4.0,
        w: DOCK_AREA_W,
        ..dock_button
    };
    let top = top_level.max(2);
    let shown = road.w - 2.0 * ROAD_INSET;
    let step = (shown / (top - 1) as f32).max(LEAST_STEP);
    let span = step * (top - 1) as f32;
    let scroll = scroll.clamp(0.0, (span - shown).max(0.0));
    let mut r = RoadLayout {
        header,
        raise,
        road,
        dock,
        dock_button,
        dock_area,
        nodes: Vec::new(),
        marks: Vec::new(),
        mastery: Rect::default(),
        top,
        origin: road.x + ROAD_INSET - scroll,
        step,
        span,
        shown,
    };
    let line = road.y + LINE_Y;
    let at = |lvl: u32| level_x(&r, lvl);
    let nodes = side_by_side(perks, NODE_PITCH, at)
        .into_iter()
        .map(|(id, _, x)| {
            let rect = Rect {
                x: x - NODE / 2.0,
                y: line + 14.0,
                w: NODE,
                h: NODE,
            };
            (id, rect)
        })
        .collect();
    let mastery = Rect {
        x: at(top) - NODE / 2.0,
        y: road.y + 5.0,
        w: NODE,
        h: NODE,
    };
    // Each mark sits over its level; one that would crowd the slot before it, or run into the mastery frame,
    // shares that slot instead
    let mut slots: Vec<(String, u32, Rect)> = Vec::new();
    for (label, lvl) in marks {
        let ideal = at(*lvl) - MARK / 2.0;
        let last = slots.last().map(|(.., m)| m.x);
        let crowded = last.is_some_and(|x| ideal < x + MARK + MARK_GAP);
        let past_top = ideal + MARK > mastery.x - MARK_GAP;
        let x = match last {
            Some(x) if crowded || past_top => x,
            _ if past_top => mastery.x - MARK_GAP - MARK,
            _ => ideal,
        };
        let rect = Rect {
            x,
            y: road.y + 7.0,
            w: MARK,
            h: MARK,
        };
        slots.push((label.clone(), *lvl, rect));
    }
    let marks = slots;
    let mut nodes: Vec<(String, Rect)> = nodes;
    let marks: Vec<(String, u32, Rect)> = marks;
    let mut xs: Vec<f32> = nodes.iter().map(|(_, n)| n.x).collect();
    spread(&mut xs, NODE);
    for ((_, n), x) in nodes.iter_mut().zip(xs) {
        n.x = x;
    }
    r.nodes = nodes;
    r.marks = marks;
    r.mastery = mastery;
    r
}

/// The road of `skill` as the client knows it: its perks, and its labelled locks that have an icon as marks.
pub fn road_of(
    defs: &ClientDefs,
    _state: &PlayerState,
    skill: &str,
    body: &Rect,
    scroll: f32,
) -> RoadLayout {
    let d = &defs.defs;
    let perks: Vec<(String, u32)> = perks_of(d, skill)
        .into_iter()
        .map(|p| (p.id.clone(), p.min_level))
        .collect();
    let marks: Vec<(String, u32)> = defs
        .labels
        .get(skill)
        .into_iter()
        .flatten()
        .flat_map(|(lvl, labels)| {
            labels
                .iter()
                .filter(|label| defs.label_icons.contains_key(*label))
                .map(|label| (label.clone(), *lvl))
        })
        .collect();
    road(body, d.max_level(skill), &perks, &marks, scroll)
}

/// The wheel on a skill's road: a notch moves it `WHEEL_STEP` pixels, within its ends.
pub fn scrolled(r: &RoadLayout, now: f32, amount: f64) -> f32 {
    let moved = if amount > 0.0 {
        now - WHEEL_STEP
    } else if amount < 0.0 {
        now + WHEEL_STEP
    } else {
        now
    };
    moved.clamp(0.0, max_scroll(r))
}

/// Whether `r` lies across the road panel's visible width.
fn visible(road: &Rect, r: &Rect) -> bool {
    r.x >= road.x + 1.0 && r.x + r.w <= road.x + road.w - 1.0
}

/// The perk the dock tells about: the one under the mouse, else the one chosen, else the next to unlock (the
/// skill's last when all are owned).
fn dock_perk<'a>(
    d: &'a skills_core::registry::Defs,
    state: &PlayerState,
    r: &RoadLayout,
    chosen: Option<&str>,
    mouse: Option<[f32; 2]>,
) -> Option<&'a PerkDef> {
    let hovered = mouse.and_then(|m| r.nodes.iter().find(|(_, n)| n.contains(m[0], m[1])));
    let by_id = |id: &str| d.perks.get(id);
    hovered
        .and_then(|(id, _)| by_id(id))
        .or_else(|| {
            chosen
                .and_then(by_id)
                .filter(|p| r.nodes.iter().any(|(id, _)| *id == p.id))
        })
        .or_else(|| {
            let ids = r.nodes.iter().map(|(id, _)| id);
            let mut perks = ids.filter_map(|id| by_id(id));
            perks.find(|p| !owns(state, &p.id))
        })
        .or_else(|| r.nodes.last().and_then(|(id, _)| by_id(id)))
}

/// Draws a skill's page.
pub(super) fn draw(
    pen: &mut Pen<'_>,
    defs: &ClientDefs,
    skill: &str,
    r: &RoadLayout,
    chosen: Option<&str>,
) {
    draw_header(pen, skill, r);
    draw_road(pen, defs, skill, r, chosen);
    let mouse = Some(pen.mouse);
    let perk = dock_perk(pen.d, pen.state, r, chosen, mouse);
    draw_dock(pen, skill, r, perk);
}

fn frame(pen: &mut Pen<'_>, kind: &str, r: &Rect) {
    let name = format!("{FRAMES}{kind}");
    pen.host
        .gui_nine_slice(pen.batch, &name, r.array(), UNTINTED);
}

fn draw_header(pen: &mut Pen<'_>, skill: &str, r: &RoadLayout) {
    let (d, state) = (pen.d, pen.state);
    let h = &r.header;
    let icon_frame = Rect {
        w: NODE,
        h: NODE,
        ..*h
    };
    frame(pen, "goal_frame_obtained", &icon_frame);
    let now = level(state, d, skill);
    if let Some(def) = d.skill(skill) {
        pen.host
            .item(pen.batch, def.icon_at(now), [h.x + 5.0, h.y + 5.0], 16.0);
    }
    let name = skill_name(d, skill).plain().to_uppercase();
    pen.text(&name, h.x + 32.0, h.y + 3.0, GOLD);
    let levels = say(&LEVEL_OF, &[number(now), number(d.max_level(skill))]).plain();
    pen.text(&levels, h.x + 32.0, h.y + 15.0, GRAY);
    if pen.over(&icon_frame) {
        pen.tip = Some(skill_tip(pen, skill));
    }
    let column = h.x + 32.0 + pen.width(&levels).max(pen.width(&name)) + 10.0;
    let points = points_available(d, state, skill);
    let points_line = say(&PERK_POINTS_N, &[number(points)]).plain();
    let own = own_level(state, d, skill);
    let lifted = own < now;
    let points_y = if lifted { h.y + 3.0 } else { h.y + 9.0 };
    pen.text(
        &points_line,
        column,
        points_y,
        if points > 0 { GOLD } else { GRAY },
    );
    let mut taken = column + pen.width(&points_line);
    if lifted {
        let shared = say(&SHARED_OWN, &[number(own)]).plain();
        pen.text(&shared, column, h.y + 15.0, GREEN);
        taken = taken.max(column + pen.width(&shared));
    }
    let buying = pen.rules.buying();
    let earned = earned_of(d, state, skill).filter(|_| pen.rules.earning());
    // The chip left of the button: the XP to spend, or the progress by doing when skills grow both ways
    let chip = match (earned, buying) {
        (Some((have, need)), true) => {
            Some((say(&LEVEL, &[number(have), number(need)]).plain(), GRAY))
        }
        _ if buying => pen
            .xp
            .map(|xp| (say(&XP_CHIP, &[number(xp)]).plain(), GREEN)),
        _ => None,
    };
    if let Some((label, colour)) = chip {
        let w = pen.width(&label) + 8.0;
        let at = Rect {
            x: r.raise.x - 6.0 - w,
            y: h.y + 6.0,
            w,
            h: 13.0,
        };
        if at.x >= taken + 6.0 {
            pen.fill(&at, BLACK);
            pen.fill(&at.shrunk(1.0), INSET);
            pen.text(&label, at.x + 4.0, at.y + 3.0, colour);
            if let (Some((have, need)), true) = (earned, pen.over(&at)) {
                let line = say(&PROGRESS, &[number(have), number(need)]);
                pen.tip = Some(vec![line.with_color(GRAY)]);
            }
        }
    }
    draw_raise(pen, skill, &r.raise, earned);
}

fn draw_raise(pen: &mut Pen<'_>, skill: &str, raise: &Rect, earned: Option<(u32, u32)>) {
    let (d, state) = (pen.d, pen.state);
    if mastered(d, state, skill) {
        draw_mastered_plaque(pen, raise);
        return;
    }
    if !pen.rules.buying() {
        if let Some((have, need)) = earned {
            let line = say(&PROGRESS, &[number(have), number(need)]).plain();
            paint::text_centred(pen.host, pen.batch, &line, raise, GRAY);
        } else {
            pen.button(raise, &say(&AT_TOP, &[]).plain(), Look::Off);
        }
        return;
    }
    let Some(cost) = price(d, pen.rules, state, skill) else {
        pen.button(raise, &say(&AT_TOP, &[]).plain(), Look::Off);
        return;
    };
    let look = if can_raise(d, pen.rules, state, pen.xp, skill) {
        Look::Go
    } else {
        Look::Off
    };
    // "Level up · 12 XP", or just "12 XP" when the long label would not fit beside the arrow
    let long = say(&LEVEL_UP_COST, &[number(cost)]).plain();
    let room = raise.w - paint::ARROW_SIZE[0] - 3.0 - 6.0;
    let label = if pen.width(&long) <= room {
        long
    } else {
        say(&XP_CHIP, &[number(cost)]).plain()
    };
    let hover = pen.over(raise);
    paint::arrow_button(pen.host, pen.batch, raise, &label, look, hover);
    if hover {
        pen.tip = Some(raise_tip(pen, skill));
    }
}

fn draw_road(
    pen: &mut Pen<'_>,
    defs: &ClientDefs,
    skill: &str,
    r: &RoadLayout,
    chosen: Option<&str>,
) {
    let (d, state) = (pen.d, pen.state);
    let panel = &r.road;
    pen.fill(panel, BLACK);
    let inner = panel.shrunk(1.0);
    paint::stone(pen.host, pen.batch, &inner);
    pen.fill(&Rect { h: 2.0, ..inner }, INNER_SHADOW);
    pen.fill(&Rect { w: 2.0, ..inner }, INNER_SHADOW);
    let reached = own_level(state, d, skill);
    let now = level(state, d, skill);
    let line = panel.y + LINE_Y;
    // Everything on the road is cut to the panel's inside
    let clip = |x0: f32, x1: f32| (x0.max(inner.x), x1.min(inner.x + inner.w));
    let span = |pen: &mut Pen<'_>, x0: f32, x1: f32, y: f32, h: f32, argb: u32| {
        let (a, b) = clip(x0, x1);
        if b > a {
            pen.fill(
                &Rect {
                    x: a,
                    y,
                    w: b - a,
                    h,
                },
                argb,
            );
        }
    };
    let (first, last) = (level_x(r, 1), level_x(r, r.top));
    span(pen, first - 5.0, last + 5.0, line - 1.0, 5.0, BLACK);
    span(pen, first - 4.0, last + 4.0, line, 3.0, ROAD);
    let here = level_x(r, reached.max(1));
    span(pen, first - 4.0, here, line, 3.0, ROAD_DONE);
    span(pen, first - 4.0, here, line, 1.0, ROAD_SHINE);
    if r.step >= 3.0 {
        for lvl in 1..=r.top {
            let x = level_x(r, lvl);
            if x > inner.x && x < inner.x + inner.w {
                let colour = if lvl <= reached {
                    TICK_DONE
                } else {
                    TICK_AHEAD
                };
                pen.fill(
                    &Rect {
                        x,
                        y: line + 3.0,
                        w: 1.0,
                        h: 2.0,
                    },
                    colour,
                );
            }
        }
    }
    // The gear each level opens, above the road: a slot for each, or for each crowd of them, with how many
    let mut slots: Vec<(Rect, Vec<(&String, u32)>)> = Vec::new();
    for (label, lvl, rect) in &r.marks {
        match slots.iter_mut().find(|(m, _)| m == rect) {
            Some((_, members)) => members.push((label, *lvl)),
            None => slots.push((*rect, vec![(label, *lvl)])),
        }
    }
    for (rect, members) in &slots {
        if !visible(panel, rect) {
            continue;
        }
        let (first, first_level) = members[0];
        let got = first_level <= reached;
        let x = level_x(r, first_level)
            .min(rect.x + rect.w - 1.0)
            .max(rect.x);
        let stem = rect.y + rect.h;
        pen.fill(
            &Rect {
                x,
                y: stem,
                w: 1.0,
                h: line - 1.0 - stem,
            },
            if got { ROAD_DONE } else { ROAD },
        );
        paint::slot(pen.host, pen.batch, rect);
        if let Some(icon) = defs.label_icons.get(first) {
            pen.host
                .item(pen.batch, icon, [rect.x + 1.0, rect.y + 1.0], 16.0);
        }
        if !got {
            pen.fill(&rect.shrunk(1.0), DIMMED);
            paint::padlock(pen.host, pen.batch, [rect.x + 9.0, rect.y + 7.0]);
        }
        if members.len() > 1 {
            let count = members.len().to_string();
            let at = [
                rect.x + rect.w - 1.0 - pen.width(&count),
                rect.y + rect.h - 8.0,
            ];
            pen.host.text(pen.batch, &count, at, WHITE, true);
        }
        if pen.over(rect) {
            let mut tip = Vec::new();
            for (label, lvl) in members {
                let met = *lvl <= reached;
                tip.push(label_name(label).with_color(GOLD));
                tip.push(say(&AT_LEVEL, &[number(*lvl)]).with_color(if met {
                    GREEN
                } else {
                    GRAY
                }));
            }
            pen.tip = Some(tip);
        }
    }
    // Mastery at the top of the road
    if visible(panel, &r.mastery) {
        let done = mastered(d, state, skill);
        let x = level_x(r, r.top);
        let stem = r.mastery.y + r.mastery.h;
        pen.fill(
            &Rect {
                x,
                y: stem,
                w: 1.0,
                h: line - 1.0 - stem,
            },
            if done { ROAD_DONE } else { ROAD },
        );
        frame(
            pen,
            if done {
                "challenge_frame_obtained"
            } else {
                "challenge_frame_unobtained"
            },
            &r.mastery,
        );
        if let Some(def) = d.skill(skill) {
            let at = [r.mastery.x + 5.0, r.mastery.y + 5.0];
            pen.host.item(pen.batch, def.icon_at(r.top), at, 16.0);
        }
        if !done {
            pen.fill(
                &Rect {
                    x: r.mastery.x + 5.0,
                    y: r.mastery.y + 5.0,
                    w: 16.0,
                    h: 16.0,
                },
                DIMMED,
            );
        }
        if pen.over(&r.mastery) {
            pen.tip = Some(vec![say(&MASTERED, &[]).with_color(GOLD)]);
        }
    }
    // The level plates: at 1, every fourth level, the player's and the top, leaving out any that would crowd
    let mut plates: Vec<u32> = (1..=r.top)
        .filter(|l| *l == 1 || l % 4 == 0 || *l == r.top || *l == now)
        .collect();
    plates.dedup();
    let mut last_right = f32::MIN;
    for lvl in plates {
        let text = lvl.to_string();
        let w = pen.width(&text) + 5.0;
        let x = level_x(r, lvl) - (w / 2.0).floor();
        let plate = Rect {
            x,
            y: line - 4.0,
            w,
            h: 11.0,
        };
        if x < last_right + 1.0 && lvl != now {
            continue;
        }
        if !visible(panel, &plate) {
            continue;
        }
        last_right = x + w;
        let (face, ink) = if lvl == now {
            (PLATE_HERE, PLATE_HERE_TEXT)
        } else if lvl < now {
            (PLATE_PAST, GOLD)
        } else {
            (PLATE_AHEAD, DIM)
        };
        pen.fill(&plate, BLACK);
        pen.fill(&plate.shrunk(1.0), face);
        pen.host
            .text(pen.batch, &text, [x + 3.0, line - 2.0], ink, false);
    }
    // The player's marker over their level
    let x = level_x(r, now);
    if x > inner.x + 3.0 && x < inner.x + inner.w - 3.0 {
        for i in 0..4 {
            let i = i as f32;
            pen.fill(
                &Rect {
                    x: x - 3.0 + i,
                    y: line - 9.0 + i,
                    w: 7.0 - 2.0 * i,
                    h: 1.0,
                },
                MARKER,
            );
        }
    }
    let shown = dock_perk(d, state, r, chosen, Some(pen.mouse)).map(|p| p.id.clone());
    // Each perk joined to the perks it requires, at the nodes' middle height, behind the frames
    for (id, rect) in &r.nodes {
        let Some(perk) = d.perks.get(id) else {
            continue;
        };
        for needed in &perk.requires {
            let Some((_, other)) = r.nodes.iter().find(|(o, _)| o == needed) else {
                continue;
            };
            let (a, b) = (
                other.x + (other.w / 2.0).floor(),
                rect.x + (rect.w / 2.0).floor(),
            );
            let (from, to) = (a.min(b), a.max(b));
            let (from, to) = (from.max(inner.x), to.min(inner.x + inner.w - 1.0));
            if to <= from {
                continue;
            }
            let colour = if owns(state, needed) {
                PERK_OWNED_LINE
            } else {
                PERK_NEEDED_LINE
            };
            pen.fill(
                &Rect {
                    x: from,
                    y: rect.y + 13.0,
                    w: to - from + 1.0,
                    h: 1.0,
                },
                colour,
            );
        }
    }
    let capstone = r.nodes.last().map(|(id, _)| id.clone());
    for (id, rect) in &r.nodes {
        let Some(perk) = d.perks.get(id) else {
            continue;
        };
        if !visible(panel, rect) {
            continue;
        }
        draw_node(
            pen,
            perk,
            rect,
            line,
            capstone.as_deref() == Some(id.as_str()),
            shown.as_deref() == Some(id),
        );
    }
}

fn draw_node(
    pen: &mut Pen<'_>,
    perk: &PerkDef,
    rect: &Rect,
    line: f32,
    capstone: bool,
    in_dock: bool,
) {
    let (d, state) = (pen.d, pen.state);
    let status = status(d, state, perk);
    let colour = match status {
        PerkStatus::Owned => PERK_OWNED,
        PerkStatus::Buyable => PERK_OPEN,
        PerkStatus::Locked(_) => ROAD,
    };
    let x = rect.x + (rect.w / 2.0).floor();
    pen.fill(
        &Rect {
            x,
            y: line + 5.0,
            w: 1.0,
            h: rect.y - line - 5.0,
        },
        colour,
    );
    if matches!(status, PerkStatus::Buyable) {
        let around = Rect {
            x: rect.x - 1.0,
            y: rect.y - 1.0,
            w: rect.w + 2.0,
            h: rect.h + 2.0,
        };
        paint::edge(pen.host, pen.batch, &around, PERK_OPEN);
    }
    let owned = matches!(status, PerkStatus::Owned);
    let kind = match (capstone, owned) {
        (true, true) => "challenge_frame_obtained",
        (true, false) => "challenge_frame_unobtained",
        (false, true) => "task_frame_obtained",
        (false, false) => "task_frame_unobtained",
    };
    frame(pen, kind, rect);
    pen.host.item(
        pen.batch,
        perk_icon(pen, perk),
        [rect.x + 5.0, rect.y + 5.0],
        16.0,
    );
    let off = owned && switched_off(state, &perk.id);
    match status {
        PerkStatus::Locked(_) => {
            pen.fill(
                &Rect {
                    x: rect.x + 5.0,
                    y: rect.y + 5.0,
                    w: 16.0,
                    h: 16.0,
                },
                DIMMED,
            );
            paint::padlock(pen.host, pen.batch, [rect.x + 17.0, rect.y + 15.0]);
        }
        PerkStatus::Owned => {
            if off {
                pen.fill(
                    &Rect {
                        x: rect.x + 5.0,
                        y: rect.y + 5.0,
                        w: 16.0,
                        h: 16.0,
                    },
                    DIMMED,
                );
            }
            pen.fill(
                &Rect {
                    x: rect.x + 17.0,
                    y: rect.y + 17.0,
                    w: 8.0,
                    h: 8.0,
                },
                BLACK,
            );
            pen.host.text(
                pen.batch,
                paint::TICK,
                [rect.x + 18.0, rect.y + 17.0],
                GOLD,
                false,
            );
        }
        PerkStatus::Buyable => {}
    }
    if !owned {
        let label = if perk.cost <= 3 {
            paint::POINT.repeat(perk.cost as usize)
        } else {
            format!("{}{}", paint::POINT, perk.cost)
        };
        let under = Rect {
            x: rect.x - 7.0,
            y: rect.y + rect.h + 1.0,
            w: rect.w + 14.0,
            h: paint::GLYPH,
        };
        let ink = if matches!(status, PerkStatus::Buyable) {
            GOLD
        } else {
            DIM
        };
        paint::text_centred(pen.host, pen.batch, &label, &under, ink);
    }
    if in_dock {
        let around = Rect {
            x: rect.x - 3.0,
            y: rect.y - 3.0,
            w: rect.w + 6.0,
            h: rect.h + 6.0,
        };
        paint::edge(pen.host, pen.batch, &around, SELECTED);
    }
    if pen.over(rect) {
        pen.tip = Some(perk_tip(pen, perk, &status));
    }
}

/// The item a perk shows: its own icon, else its skill's.
fn perk_icon<'a>(pen: &Pen<'a>, perk: &'a PerkDef) -> &'a str {
    perk.icon.as_deref().unwrap_or_else(|| {
        pen.d
            .skill(&perk.skill)
            .map_or("minecraft:barrier", |s| s.icon.as_str())
    })
}

fn draw_dock(pen: &mut Pen<'_>, skill: &str, r: &RoadLayout, perk: Option<&PerkDef>) {
    let (d, state) = (pen.d, pen.state);
    let dock = &r.dock;
    pen.fill(dock, BLACK);
    pen.fill(&dock.shrunk(1.0), CARD);
    let accent = d.skill(skill).map_or(0x8F_A6B8, skill_rgb);
    pen.fill(
        &Rect {
            x: dock.x + 1.0,
            y: dock.y + 1.0,
            w: 2.0,
            h: dock.h - 2.0,
        },
        0xFF00_0000 | accent,
    );
    let Some(perk) = perk else { return };
    let status = status(d, state, perk);
    let owned = matches!(status, PerkStatus::Owned);
    let icon_frame = Rect {
        x: dock.x + 8.0,
        y: dock.y + 7.0,
        w: NODE,
        h: NODE,
    };
    frame(
        pen,
        if owned {
            "task_frame_obtained"
        } else {
            "task_frame_unobtained"
        },
        &icon_frame,
    );
    pen.host.item(
        pen.batch,
        perk_icon(pen, perk),
        [icon_frame.x + 5.0, icon_frame.y + 5.0],
        16.0,
    );
    let left = dock.x + 40.0;
    let area_shown = owned && area_of(perk).is_some();
    let right = if area_shown {
        r.dock_area.x
    } else {
        r.dock_button.x
    } - 6.0;
    let name = paint::fitted(pen.host, &perk_name(d, &perk.id).plain(), right - left);
    pen.text(&name, left, dock.y + 6.0, GOLD);
    let about = say(&PERK_AT, &[skill_name(d, skill), number(perk.min_level)]).plain();
    let about_x = left + pen.width(&name) + 8.0;
    if about_x + pen.width(&about) <= right {
        pen.host
            .text(pen.batch, &about, [about_x, dock.y + 6.0], DIM, false);
    }
    draw_description(
        pen,
        &perk_description(perk).plain(),
        left,
        dock.y + 18.0,
        right - left,
    );
    draw_needs(
        pen,
        perk,
        &status,
        left,
        dock.y + dock.h - 13.0,
        dock.x + dock.w - 6.0 - left,
    );
    // The dock's buttons
    match status {
        PerkStatus::Buyable => pen.button(&r.dock_button, &say(&GET, &[]).plain(), Look::Go),
        PerkStatus::Locked(ref why) => {
            pen.button(&r.dock_button, &say(&GET, &[]).plain(), Look::Off);
            if pen.over(&r.dock_button) {
                pen.tip = Some(vec![why.clone().with_color(super::paint::RED)]);
            }
        }
        PerkStatus::Owned if pen.rules.perk_switches => {
            let on = !switched_off(state, &perk.id);
            let label = say(if on { &VAL_ON } else { &VAL_OFF }, &[]).plain();
            pen.button(
                &r.dock_button,
                &label,
                if on { Look::Go } else { Look::Plain },
            );
        }
        PerkStatus::Owned => {}
    }
    if area_shown {
        let look = if pen.shown_areas.contains(&perk.id) {
            Look::Go
        } else {
            Look::Plain
        };
        pen.button(&r.dock_area, &say(&AREA, &[]).plain(), look);
    }
}

/// The description's sentences as bullets, wrapped to `width`, numbers in white among gray words; two lines at most.
fn draw_description(pen: &mut Pen<'_>, text: &str, x: f32, y: f32, width: f32) {
    const LINES: usize = 2;
    let bullet = "\u{2022} ";
    let indent = pen.width(bullet);
    let mut lines: Vec<(bool, String)> = Vec::new();
    for sentence in super::screen::sentences(text) {
        for (at, line) in paint::wrapped(pen.host, &sentence, width - indent)
            .into_iter()
            .enumerate()
        {
            lines.push((at == 0, line));
        }
    }
    let cut = lines.len() > LINES;
    lines.truncate(LINES);
    let count = lines.len();
    for (n, (first, line)) in lines.into_iter().enumerate() {
        let line_y = y + n as f32 * 10.0;
        if first {
            pen.text(bullet.trim_end(), x, line_y, GOLD);
        }
        let line = if cut && n + 1 == count {
            paint::fitted(pen.host, &format!("{line}..."), width - indent)
        } else {
            line
        };
        let mut at = x + indent;
        for (i, word) in line.split(' ').enumerate() {
            let word = if i == 0 {
                word.to_string()
            } else {
                format!(" {word}")
            };
            let colour = if word.chars().any(|c| c.is_ascii_digit()) {
                WHITE
            } else {
                GRAY
            };
            pen.host.text(pen.batch, &word, [at, line_y], colour, false);
            at += pen.width(&word);
        }
    }
}

/// What the perk needs, each ticked or crossed, or that it is owned, on one line from `x`, as much as fits.
fn draw_needs(pen: &mut Pen<'_>, perk: &PerkDef, status: &PerkStatus, x: f32, y: f32, width: f32) {
    let (d, state) = (pen.d, pen.state);
    let mut parts: Vec<(Text, u32)> = Vec::new();
    if matches!(status, PerkStatus::Owned) {
        parts.push((marked(true, say(&OWNED, &[])), GOLD));
        if perk
            .effects
            .iter()
            .any(|e| matches!(e, Effect::LastStand { .. }))
        {
            parts.push(last_stand_line(pen));
        }
        if switched_off(state, &perk.id) {
            parts.push((say(&SWITCHED_OFF, &[]), DIM));
        }
    } else {
        let enough = points_available(d, state, &perk.skill) >= perk.cost;
        let cost = Text::literal(format!("{} ", paint::POINT))
            .append(say(&PERK_COST, &[number(perk.cost)]));
        parts.push((cost, met_colour(enough)));
        if perk.min_level > 1 {
            let met = own_level(state, d, &perk.skill) >= perk.min_level;
            let what = say(
                &TIP_NEED,
                &[skill_name(d, &perk.skill), number(perk.min_level)],
            );
            parts.push((marked(met, what), met_colour(met)));
        }
        for (other, needed) in &perk.also_needs {
            let met = own_level(state, d, other) >= *needed;
            let what = say(&TIP_NEED, &[skill_name(d, other), number(*needed)]);
            parts.push((marked(met, what), met_colour(met)));
        }
        for other in &perk.requires {
            let met = owns(state, other);
            parts.push((marked(met, perk_name(d, other)), met_colour(met)));
        }
    }
    let mut at = x;
    for (part, colour) in parts {
        let text = part.plain();
        let w = pen.width(&text);
        if at + w > x + width {
            break;
        }
        pen.host.text(pen.batch, &text, [at, y], colour, false);
        at += w + 10.0;
    }
}

/// Whether the player's last stand is ready, or how long it still rests ("Ready in 4:32").
fn last_stand_line(pen: &Pen<'_>) -> (Text, u32) {
    let left = pen
        .last_stand_ready
        .map_or(0, |ready| ready.saturating_sub(pen.now));
    if left == 0 {
        return (say(&READY, &[]), GREEN);
    }
    let seconds = left.div_ceil(20);
    let clock = format!("{}:{:02}", seconds / 60, seconds % 60);
    (say(&READY_IN, &[Text::literal(clock)]), GRAY)
}

/// What a click on a skill's page does: the right button goes Home; the raise raises `times` when it can be paid;
/// a perk is chosen for the dock; the dock's button unlocks or switches its perk, its area button shows the area.
pub(super) fn click(
    cache: &ClientCache,
    defs: &ClientDefs,
    skill: &str,
    r: &RoadLayout,
    at: [f32; 2],
    times: u32,
) -> Outcome {
    let (d, state, rules) = (&defs.defs, &cache.state, &cache.rules);
    let hit = |rect: &Rect| rect.contains(at[0], at[1]);
    if hit(&r.raise) {
        return if rules.buying() && can_raise(d, rules, state, cache.xp_levels, skill) {
            Outcome::Ask(Request::Raise {
                skill: skill.to_string(),
                times,
            })
        } else {
            Outcome::Nothing
        };
    }
    if let Some((id, _)) = r.nodes.iter().find(|(_, n)| hit(n) && visible(&r.road, n)) {
        return Outcome::Choose(id.clone());
    }
    let Some(perk) = dock_perk(d, state, r, cache.selected_perk.as_deref(), None) else {
        return Outcome::Nothing;
    };
    let status = status(d, state, perk);
    if hit(&r.dock_button) {
        return match status {
            PerkStatus::Buyable => Outcome::Ask(Request::BuyPerk {
                perk: perk.id.clone(),
            }),
            PerkStatus::Owned if rules.perk_switches => Outcome::Ask(Request::SwitchPerk {
                perk: perk.id.clone(),
            }),
            _ => Outcome::Nothing,
        };
    }
    if hit(&r.dock_area) && matches!(status, PerkStatus::Owned) && area_of(perk).is_some() {
        return Outcome::ToggleArea(perk.id.clone());
    }
    Outcome::Nothing
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::HudBatch;
    use skills_core::progress::Request;

    use super::*;
    use crate::client::cache::{Tab, with};
    use crate::client::layout::Rect;
    use crate::client::preview::player;
    use crate::client::screen::tests::{mining_open, perk, sent_acts};
    use crate::client::screen::{LEFT_BUTTON, click, draw, scroll};
    use crate::client::window::window;
    use crate::test_host::{Paint, TestHost, exclusive};

    const SMALLEST: [f32; 2] = [320.0, 240.0];
    const USUAL: [f32; 2] = [427.0, 240.0];
    const FRAMES: &str = "gui/sprites/advancements/";
    /// The padlock's outline colour.
    const PADLOCK_EDGE: u32 = 0xFF1A_1206;

    fn inside(outer: &Rect, inner: &Rect) -> bool {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.x + inner.w <= outer.x + outer.w
            && inner.y + inner.h <= outer.y + outer.h
    }

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    /// The open skill's road as the screen lays it out at the usual size.
    fn laid_out() -> RoadLayout {
        with(|cache| {
            let defs = cache.defs.as_ref().unwrap();
            let body = window(USUAL, defs.defs.skills.len(), 0).body;
            let skill = defs.defs.skills.keys().nth(cache.selected).unwrap().clone();
            road_of(defs, &cache.state, &skill, &body, cache.road_scroll)
        })
    }

    fn node(r: &RoadLayout, perk: &str) -> Rect {
        r.nodes.iter().find(|(id, _)| id.ends_with(perk)).unwrap().1
    }

    fn middle(r: &Rect) -> [f32; 2] {
        [r.x + r.w / 2.0, r.y + r.h / 2.0]
    }

    fn sprites(host: &TestHost) -> Vec<String> {
        host.painted
            .borrow()
            .iter()
            .filter_map(|p| match p {
                Paint::Sprite { name, .. } => name.strip_prefix(FRAMES).map(str::to_string),
                _ => None,
            })
            .collect()
    }

    /// Whether a fill of `argb` lies inside `r`.
    fn filled_in(host: &TestHost, r: &Rect, argb: u32) -> bool {
        host.painted.borrow().iter().any(|p| match p {
            Paint::Fill { rect, argb: a } => {
                *a == argb
                    && inside(
                        r,
                        &Rect {
                            x: rect[0],
                            y: rect[1],
                            w: rect[2],
                            h: rect[3],
                        },
                    )
            }
            _ => false,
        })
    }

    #[test]
    fn the_road_fits_and_places_levels() {
        for size in [SMALLEST, USUAL] {
            // Given the window's body at both sizes, and a skill to 32
            let body = window(size, 8, 0).body;

            // When its road is laid out
            let r = road(&body, 32, &[], &[], 0.0);

            // Then the header, road and dock are inside the body, apart; level 1 and 32 are on the road; and
            // level 12 is 11/31 of the way along
            for part in [r.header, r.road, r.dock] {
                assert!(inside(&body, &part), "{size:?}: {part:?}");
            }
            assert!(
                !overlap(&r.header, &r.road) && !overlap(&r.road, &r.dock),
                "{size:?}"
            );
            assert!(
                inside(&r.dock, &r.dock_button) && inside(&r.dock, &r.dock_area),
                "{size:?}"
            );
            assert!(inside(&r.header, &r.raise), "{size:?}");
            let on_road = |x: f32| x >= r.road.x && x <= r.road.x + r.road.w;
            assert!(
                on_road(level_x(&r, 1)) && on_road(level_x(&r, 32)),
                "{size:?}"
            );
            let span = level_x(&r, 32) - level_x(&r, 1);
            let twelve = level_x(&r, 12) - level_x(&r, 1);
            assert!(
                (twelve - span * 11.0 / 31.0).abs() <= 1.0,
                "{size:?}: {twelve} of {span}"
            );
        }
    }

    #[test]
    fn perks_at_one_level_sit_side_by_side() {
        // Given two perks at level 8
        let body = window(USUAL, 8, 0).body;
        let perks = [("a:one".to_string(), 8), ("a:two".to_string(), 8)];

        // When the road is laid out
        let r = road(&body, 32, &perks, &[], 0.0);

        // Then their nodes sit 28 apart, side by side
        let (one, two) = (r.nodes[0].1, r.nodes[1].1);
        assert_eq!(two.x - one.x, 28.0);
        assert_eq!(one.y, two.y);
        assert!(!overlap(&one, &two));
    }

    #[test]
    fn neighbouring_levels_never_overlap() {
        // Given gear at levels 2, 3 and 4 and perks at 4 and 5, on the smallest body
        let body = window(SMALLEST, 8, 0).body;
        let marks = [
            ("a".to_string(), 2),
            ("b".to_string(), 3),
            ("c".to_string(), 4),
        ];
        let perks = [("p:one".to_string(), 4), ("p:two".to_string(), 5)];

        // When the road is laid out
        let r = road(&body, 32, &perks, &marks, 0.0);

        // Then no two marks' slots overlap (crowded marks share one), and no two nodes do
        let mut slots: Vec<Rect> = Vec::new();
        for (_, _, m) in &r.marks {
            if !slots.contains(m) {
                slots.push(*m);
            }
        }
        for (i, a) in slots.iter().enumerate() {
            for b in &slots[i + 1..] {
                assert!(!overlap(a, b), "{a:?} and {b:?}");
            }
        }
        assert!(!overlap(&r.nodes[0].1, &r.nodes[1].1));
    }

    #[test]
    fn many_marks_share_slots_and_stay_on_the_road() {
        // Given twenty marks over levels 1 to 32 on the smallest body
        let body = window(SMALLEST, 8, 0).body;
        let marks: Vec<(String, u32)> = (0..20)
            .map(|i| (format!("m{i}"), 1 + i * 31 / 19))
            .collect();

        // When the road is laid out
        let r = road(&body, 32, &[], &marks, 0.0);

        // Then every mark has a slot on the road, short of the mastery frame
        for (label, _, m) in &r.marks {
            assert!(
                m.x + m.w <= r.mastery.x,
                "{label}: {m:?} runs into {:?}",
                r.mastery
            );
            assert!(m.x >= r.road.x, "{label}: {m:?}");
        }
    }

    #[test]
    fn a_long_road_scrolls() {
        // Given a skill to level 100 at the smallest size
        let body = window(SMALLEST, 1, 0).body;

        // When its road is laid out from the start, and scrolled to its end
        let start = road(&body, 100, &[], &[], 0.0);
        let end = road(&body, 100, &[], &[], max_scroll(&start));

        // Then the top is past the road's right edge at first, and on it at the end
        assert!(level_x(&start, 100) > start.road.x + start.road.w);
        assert!(level_x(&end, 100) <= end.road.x + end.road.w);
        assert!(level_x(&end, 1) < end.road.x);

        // And the wheel moves the road on its tab, never past either end
        let _serial = exclusive();
        let host = mining_open();
        with(|cache| {
            let mining = cache
                .defs
                .as_mut()
                .unwrap()
                .defs
                .skills
                .get_mut(MINING_ID)
                .unwrap();
            mining.max_level = Some(100);
            cache.gui_size = SMALLEST;
        });
        for _ in 0..200 {
            scroll(&host, -1.0);
        }
        let far = with(|cache| cache.road_scroll);
        for _ in 0..200 {
            scroll(&host, 1.0);
        }
        let back = with(|cache| cache.road_scroll);
        let most = with(|cache| {
            let defs = cache.defs.as_ref().unwrap();
            let body = window(SMALLEST, defs.defs.skills.len(), 0).body;
            max_scroll(&road_of(defs, &cache.state, MINING_ID, &body, 0.0))
        });
        assert!(most > 0.0);
        assert_eq!((far, back), (most, 0.0));
    }

    #[test]
    fn an_empty_road_draws() {
        // Given Mining with no perks and nothing marked
        let _serial = exclusive();
        let host = mining_open();
        with(|cache| {
            let defs = cache.defs.as_mut().unwrap();
            defs.defs.perks.clear();
            defs.labels.clear();
        });

        // When its page is drawn
        draw(&host, &mut HudBatch::new([427, 240]), USUAL, [1.0, 1.0]);

        // Then the header and road show, and no perk frame
        assert!(
            host.drawn_text().iter().any(|t| t == "MINING"),
            "{:?}",
            host.drawn_text()
        );
        assert!(
            host.painted
                .borrow()
                .iter()
                .any(|p| matches!(p, Paint::Fill { argb, .. } if *argb == ROAD))
        );
        assert!(
            !sprites(&host).iter().any(|s| s.starts_with("task_frame")),
            "{:?}",
            sprites(&host)
        );
    }

    #[test]
    fn nodes_show_their_state() {
        // Given Mining at 12: Sure Grip owned (4), Rich Seams ready (8), Deep Veins locked and the last (16)
        let _serial = exclusive();
        let host = mining_open();
        let r = laid_out();

        // When the page is drawn
        draw(&host, &mut HudBatch::new([427, 240]), USUAL, [1.0, 1.0]);

        // Then Sure Grip wears the obtained frame, Rich Seams a green outline, Deep Veins the challenge frame with
        // a padlock, and the top of the road the mastery's challenge frame
        let frames = sprites(&host);
        assert!(
            frames.contains(&"task_frame_obtained".to_string()),
            "{frames:?}"
        );
        assert!(
            frames.contains(&"task_frame_unobtained".to_string()),
            "{frames:?}"
        );
        let challenges = frames
            .iter()
            .filter(|f| *f == "challenge_frame_unobtained")
            .count();
        assert_eq!(challenges, 2, "{frames:?}");
        let seams = node(&r, "rich_seams");
        let around = Rect {
            x: seams.x - 1.0,
            y: seams.y - 1.0,
            w: seams.w + 2.0,
            h: seams.h + 2.0,
        };
        assert!(filled_in(&host, &around, PERK_OPEN));
        assert!(filled_in(&host, &node(&r, "deep_veins"), PADLOCK_EDGE));
        assert!(!filled_in(&host, &node(&r, "sure_grip"), PADLOCK_EDGE));
    }

    #[test]
    fn a_required_perk_is_joined_to_its_perk() {
        // Given Mining's road, where Rich Seams requires Sure Grip (owned)
        let _serial = exclusive();
        let host = mining_open();
        let r = laid_out();
        let (grip, seams) = (node(&r, "sure_grip"), node(&r, "rich_seams"));

        // When the page is drawn
        draw(&host, &mut HudBatch::new([427, 240]), USUAL, [1.0, 1.0]);

        // Then a gold line runs between their centres at the nodes' middle height
        let from = grip.x + (grip.w / 2.0).floor();
        let to = seams.x + (seams.w / 2.0).floor();
        let joined = host.painted.borrow().iter().any(|p| {
            matches!(p, Paint::Fill { rect, argb } if *argb == PERK_OWNED_LINE
                && rect[0] == from && rect[0] + rect[2] == to + 1.0 && rect[1] == grip.y + 13.0)
        });
        assert!(joined);
    }

    #[test]
    fn the_dock_unlocks_switches_and_shows_areas() {
        // Given Mining's page with perk switches allowed and an owned Home Ward with an area
        let _serial = exclusive();
        let mut host = mining_open();
        with(|cache| {
            cache.rules.perk_switches = true;
            let defs = cache.defs.as_mut().unwrap();
            let mut ward = perk("mining_home_ward", 4, &[]);
            ward.effects = vec![Effect::HomeWard { radius: 24 }];
            defs.defs.perks.insert(ward.id.clone(), ward);
            let mining = cache.state.skills.get_mut(MINING_ID).unwrap();
            mining.perks.insert("reskilled:mining_home_ward".into());
        });
        let r = laid_out();
        let press = |at: [f32; 2]| click(&host, USUAL, at, LEFT_BUTTON);

        // When Rich Seams is chosen and unlocked, Sure Grip chosen and switched, its area spot clicked, Home Ward
        // chosen and its area shown, and Deep Veins (locked) chosen and its Unlock clicked
        press(middle(&node(&r, "rich_seams")));
        let chosen = with(|cache| cache.selected_perk.clone());
        press(middle(&r.dock_button));
        press(middle(&node(&r, "sure_grip")));
        press(middle(&r.dock_button));
        press(middle(&r.dock_area));
        let grip_area = with(|cache| cache.settings.shown_areas.clone());
        press(middle(&node(&r, "home_ward")));
        press(middle(&r.dock_area));
        let ward_area = with(|cache| cache.settings.shown_areas.clone());
        press(middle(&node(&r, "deep_veins")));
        press(middle(&r.dock_button));

        // Then Rich Seams is bought, Sure Grip switched, Home Ward's area shown and nothing else asked
        assert_eq!(chosen.as_deref(), Some("reskilled:mining_rich_seams"));
        assert_eq!(
            sent_acts(&mut host),
            vec![
                Request::BuyPerk {
                    perk: "reskilled:mining_rich_seams".into()
                },
                Request::SwitchPerk {
                    perk: "reskilled:mining_sure_grip".into()
                },
            ]
        );
        assert!(grip_area.is_empty());
        assert!(ward_area.contains("reskilled:mining_home_ward"));
        assert_eq!(with(|cache| cache.tab), Tab::Skill(1));
    }

    #[test]
    fn an_owned_last_stand_shows_its_cooldown_in_the_dock() {
        // Given Mining's road with Sure Grip owned and given a last stand, chosen for the dock
        let _serial = exclusive();
        let host = mining_open();
        with(|cache| {
            let grip = cache
                .defs
                .as_mut()
                .unwrap()
                .defs
                .perks
                .get_mut("reskilled:mining_sure_grip")
                .unwrap();
            grip.effects = vec![Effect::LastStand {
                above: 0.0,
                cooldown_seconds: 300,
            }];
            cache.selected_perk = Some("reskilled:mining_sure_grip".into());
            cache.ticks = 100;
        });

        // When the page is drawn while it is ready, and again with 272 seconds left
        draw(&host, &mut HudBatch::new([427, 240]), USUAL, [1.0, 1.0]);
        let ready = host.drawn_text();
        host.drawn.borrow_mut().clear();
        with(|cache| cache.last_stand_ready = Some(100 + 272 * 20));
        draw(&host, &mut HudBatch::new([427, 240]), USUAL, [1.0, 1.0]);
        let resting = host.drawn_text();

        // Then the dock says it is ready, then how long is left
        assert!(ready.iter().any(|t| t == "Ready"), "{ready:?}");
        assert!(resting.iter().any(|t| t == "Ready in 4:32"), "{resting:?}");
    }

    #[test]
    fn marks_above_the_road() {
        // Given the shipped mod, Mining at 12
        let _serial = exclusive();
        let host = player([427, 240]);
        with(|cache| {
            let defs = cache.defs.as_ref().unwrap();
            cache.selected = defs
                .defs
                .skills
                .keys()
                .position(|id| id == MINING_ID)
                .unwrap();
            cache.tab = Tab::Skill(cache.selected);
        });
        let r = laid_out();

        // When its page is drawn
        draw(&host, &mut HudBatch::new([427, 240]), USUAL, [1.0, 1.0]);

        // Then the pickaxe tiers are marked at 2, 4, 8, 16 and 24, the one at 16 (not reached) with a padlock and
        // the one at 8 without
        let mut levels: Vec<u32> = r.marks.iter().map(|(_, level, _)| *level).collect();
        levels.dedup();
        assert_eq!(levels, vec![2, 4, 8, 16, 24]);
        let mark = |level: u32| r.marks.iter().find(|(_, l, _)| *l == level).unwrap().2;
        assert!(filled_in(&host, &mark(16), PADLOCK_EDGE));
        assert!(!filled_in(&host, &mark(8), PADLOCK_EDGE));
    }

    const MINING_ID: &str = "reskilled:mining";
}
