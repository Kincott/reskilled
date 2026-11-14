//! The skills screen: a window over the world when the mod's own screen is open. It has two views, every skill as a
//! card and one skill with its perks; its clicks become requests the next tick sends (the screen hooks cannot send).

use horizon_client::mod_api::{Flow, Host, HudBatch, Text};
use skills_core::defs::{PerkDef, Reward};
use skills_core::progress::{
    PlayerState, Request, apply_under, earn_threshold, focus_cost, level, mastered,
    points_available, raise_cost, switched_off, total_points,
};
use skills_core::registry::Defs;
use skills_core::rules::{CostUnit, Respec, Rules};
use skills_core::wire::{ClientDefs, ToServer};

use super::cache::{CLICK_SOUND, ClientCache, View, with};
use super::layout::{
    Button, Card, Detail, GEAR_ROOM, HEADER_H, PerkRow, Rect, Scroll, detail, hit_detail,
    hit_overview, last_first_perk, last_first_row, overview, panel,
};
use super::paint::{self, DIM, GOLD, GRAY, GREEN, Look, RED, WHITE};
use crate::MOD_ID;
use crate::text::{
    ALL_UNLOCKED, AT_TOP, BACK, COST, COST_POINTS, FOCUS, FOCUS_ACTIVE, FOCUS_ADD, FOCUS_ADD_FREE,
    FOCUS_GIVES, GET, LEVEL, LEVEL_OF, LEVEL_UP, MASTERED, MISSING_PERK, NEEDS, NEXT_UNLOCKS,
    NOT_ENOUGH_POINTS, OPEN_HINT, OWNED, PERK_COST, PERK_POINTS, PERKS, PROGRESS, RAISE_BY,
    RAISE_CTRL, RAISE_SHIFT, RAISE_TO, REFUND, REFUND_COST, REFUND_FREE, REWARD_BLOCK_XP,
    REWARD_BREAK, REWARD_BREED, REWARD_KILL, REWARD_SMELT, REWARD_TAME, REWARD_TRAVEL, SETTINGS,
    SWITCHED_OFF, TITLE, UNLOCK, VAL_OFF, VAL_ON, WAITING, XP, focus_name, label_name, number,
    perk_description, perk_name, say, skill_name,
};

/// The mouse buttons the screen answers, by GLFW index.
pub const LEFT_BUTTON: i32 = 0;
pub const RIGHT_BUTTON: i32 = 1;
const ICON: f32 = 16.0;
/// Raises one Shift-click asks for.
const SHIFT_RAISES: u32 = 5;
const BIG_ICON: f32 = 32.0;
/// Where a header's text sits below the window's top, and its side margin.
const HEADER_TEXT: f32 = 7.0;
const MARGIN: f32 = 8.0;
/// Where a skill's name starts in the skill view's header, after the back button.
const NAME_AFTER_BACK: f32 = 26.0;
/// A card shows at most this many perk points as diamonds.
const MOST_POINTS: usize = 8;
/// The mastery shine crosses a card once every this many ticks.
const SHINE_TICKS: f64 = 60.0;
/// Laid over the cards of skills outside the focus area under the mouse.
const FADED: u32 = 0xA014_161C;
/// A perk's description wraps at this width in its tooltip.
const TIP_WIDTH: f32 = 170.0;
/// The unlock preview's heading takes 10 pixels, each line 9; two lines fit.
const PREVIEW_HEADING: f32 = 10.0;
const PREVIEW_LINE: f32 = 9.0;
const PREVIEW_LINES: usize = 2;

/// A perk as the screen shows it.
enum PerkStatus {
    Owned,
    Buyable,
    Locked(Text),
}

/// The selected skill's perks, by the level they need, then id.
fn perks_of<'a>(defs: &'a Defs, skill: &str) -> Vec<&'a PerkDef> {
    let mut perks: Vec<&PerkDef> = defs.perks.values().filter(|p| p.skill == skill).collect();
    perks.sort_by(|a, b| (a.min_level, &a.id).cmp(&(b.min_level, &b.id)));
    perks
}

fn owns(state: &PlayerState, perk: &str) -> bool {
    state.skills.values().any(|p| p.perks.contains(perk))
}

fn status(defs: &Defs, state: &PlayerState, perk: &PerkDef) -> PerkStatus {
    if owns(state, &perk.id) {
        return PerkStatus::Owned;
    }
    if level(state, defs, &perk.skill) < perk.min_level {
        let needs = [skill_name(defs, &perk.skill), number(perk.min_level)];
        return PerkStatus::Locked(say(&NEEDS, &needs));
    }
    if let Some(missing) = perk.requires.iter().find(|r| !owns(state, r)) {
        return PerkStatus::Locked(say(&MISSING_PERK, &[perk_name(defs, missing)]));
    }
    if points_available(defs, state, &perk.skill) < perk.cost {
        return PerkStatus::Locked(say(&NOT_ENOUGH_POINTS, &[number(perk.cost)]));
    }
    PerkStatus::Buyable
}

/// What the screen lists, in display order: skills by id, the selected one's perks, the focus areas by id.
struct Shown<'a> {
    skills: Vec<&'a String>,
    selected: usize,
    perks: Vec<&'a PerkDef>,
    focus: Vec<&'a String>,
}

fn shown(defs: &ClientDefs, selected: usize) -> Shown<'_> {
    let skills: Vec<&String> = defs.defs.skills.keys().collect();
    let selected = selected.min(skills.len().saturating_sub(1));
    let perks = skills
        .get(selected)
        .map(|s| perks_of(&defs.defs, s))
        .unwrap_or_default();
    Shown {
        skills,
        selected,
        perks,
        focus: defs.defs.focus.keys().collect(),
    }
}

/// Whether the player has the XP levels to raise the skill.
/// Progress earned by doing toward the skill's next level, in actions: what is earned and what the level takes.
fn earned_of(d: &Defs, state: &PlayerState, skill: &str) -> Option<(u32, u32)> {
    let need = earn_threshold(d, state, skill)?;
    let have = state.skills.get(skill).map_or(0, |p| p.earned);
    Some((have / HUNDREDTHS, need / HUNDREDTHS))
}

/// Earned progress is kept in hundredths of an action.
const HUNDREDTHS: u32 = 100;

fn can_raise(d: &Defs, rules: &Rules, state: &PlayerState, xp: Option<u32>, skill: &str) -> bool {
    if !rules.buying() {
        return false;
    }
    let affordable = |xp: u32, price: u32| match rules.cost_unit {
        CostUnit::Levels => xp >= price,
        CostUnit::Points => total_points(xp) >= price,
    };
    price(d, rules, state, skill).is_some_and(|price| xp.is_some_and(|xp| affordable(xp, price)))
}

/// What the next raise of the skill costs, in the rules' unit: XP levels, or XP points.
fn price(d: &Defs, rules: &Rules, state: &PlayerState, skill: &str) -> Option<u32> {
    let cost = raise_cost(d, state, skill)?;
    Some(match rules.cost_unit {
        CostUnit::Levels => cost,
        CostUnit::Points => cost.saturating_mul(rules.points_per_level),
    })
}

/// "Cost: ..." in the rules' unit.
fn cost_line(rules: &Rules, price: u32) -> Text {
    let unit = match rules.cost_unit {
        CostUnit::Levels => &COST,
        CostUnit::Points => &COST_POINTS,
    };
    say(unit, &[number(price)])
}

/// What a draw needs: where to draw, where the mouse is, and what the player has.
struct Pen<'a> {
    host: &'a dyn Host,
    batch: &'a mut HudBatch,
    mouse: [f32; 2],
    d: &'a Defs,
    state: &'a PlayerState,
    xp: Option<u32>,
    /// The tooltip to draw last, over everything.
    tip: Option<Vec<Text>>,
    /// Client ticks run, for the mastery shine.
    ticks: u64,
    /// The server's rules: whether perks may be switched and refunded.
    rules: &'a Rules,
}

impl Pen<'_> {
    fn over(&self, r: &Rect) -> bool {
        r.contains(self.mouse[0], self.mouse[1])
    }

    fn text(&mut self, text: &str, x: f32, y: f32, rgb: u32) {
        self.host.text(self.batch, text, [x, y], rgb, true);
    }

    fn text_right(&mut self, text: &str, right: f32, y: f32, rgb: u32) {
        paint::text_right(self.host, self.batch, text, right, y, rgb);
    }

    fn fill(&mut self, r: &Rect, argb: u32) {
        paint::fill(self.host, self.batch, r, argb);
    }

    fn button(&mut self, r: &Rect, label: &str, look: Look) {
        let hover = self.over(r);
        paint::button(self.host, self.batch, r, label, look, hover);
    }

    /// How far along its path the mastery shine is, 0 to 1, from the client's tick and the frame's part of it.
    fn sweep(&self) -> f32 {
        let partial = self
            .host
            .client_state()
            .map_or(0.0, |s| s.world_partial_tick);
        let ticks = self.ticks as f64 + partial;
        (ticks.rem_euclid(SHINE_TICKS) / SHINE_TICKS) as f32
    }

    fn width(&self, text: &str) -> f32 {
        self.host.text_width(text)
    }

    /// The window with its title and, at its right, the player's XP levels.
    /// The window with its title and, at its right (`right_room` in from the edge), the player's XP levels.
    fn window(&mut self, window: &Rect, title: &str, title_x: f32, right_room: f32) {
        paint::panel(self.host, self.batch, window, HEADER_H);
        self.text(title, window.x + title_x, window.y + HEADER_TEXT, GOLD);
        if let Some(xp) = self.xp {
            let xp = say(&XP, &[number(xp)]).plain();
            let right = window.x + window.w - MARGIN - right_room;
            self.text_right(&xp, right, window.y + HEADER_TEXT, GREEN);
        }
    }

    fn scroll(&mut self, scroll: Option<Scroll>) {
        if let Some(scroll) = scroll {
            self.fill(&scroll.track, paint::INSET);
            self.fill(&scroll.thumb, paint::FRAME);
        }
    }
}

/// Opens the screen on its overview.
pub fn open(host: &mut dyn Host) {
    if host.mod_screen_open(MOD_ID) {
        with(|cache| {
            cache.view = View::Overview;
            cache.first_row = 0;
        });
    }
}

/// Draws the screen when it is this mod's that is open.
pub fn draw(host: &dyn Host, batch: &mut HudBatch, size: [f32; 2], mouse: [f32; 2]) {
    if host.mod_screen() != Some(MOD_ID) {
        return;
    }
    if with(|cache| cache.view) == View::Settings {
        super::options::draw(host, batch, size, mouse);
        return;
    }
    host.fill(batch, [0.0, 0.0, size[0], size[1]], paint::SHADE);
    with(|cache| {
        let Some(defs) = &cache.defs else {
            let window = panel(size);
            paint::panel(host, batch, &window, HEADER_H);
            let title = say(&TITLE, &[]).plain();
            host.text(
                batch,
                &title,
                [window.x + MARGIN, window.y + HEADER_TEXT],
                GOLD,
                true,
            );
            paint::text_centred(host, batch, &say(&WAITING, &[]).plain(), &window, GRAY);
            return;
        };
        let mut pen = Pen {
            host,
            batch,
            mouse,
            d: &defs.defs,
            state: &cache.state,
            xp: cache.xp_levels,
            tip: None,
            ticks: cache.ticks,
            rules: &cache.rules,
        };
        let shown = shown(defs, cache.selected);
        match cache.view {
            View::Overview => draw_overview(&mut pen, size, cache, &shown),
            View::Skill => draw_skill(&mut pen, size, cache, defs, &shown),
            View::Settings => {}
        }
        if let Some(lines) = pen.tip.take() {
            host.gui_tooltip(pen.batch, &lines, mouse);
        }
    });
}

fn draw_overview(pen: &mut Pen<'_>, size: [f32; 2], cache: &ClientCache, shown: &Shown<'_>) {
    let o = overview(size, shown.skills.len(), cache.first_row, shown.focus.len());
    pen.window(&o.panel, &say(&TITLE, &[]).plain(), MARGIN, GEAR_ROOM);
    pen.button(&o.gear, "", Look::Plain);
    paint::gear(
        pen.host,
        pen.batch,
        [o.gear.x + 3.0, o.gear.y + 3.0],
        paint::WHITE_ARGB,
    );
    if pen.over(&o.gear) {
        pen.tip = Some(vec![say(&SETTINGS, &[]).with_color(WHITE)]);
    }
    let hovered = shown
        .focus
        .iter()
        .zip(&o.focus_buttons)
        .find(|(_, button)| pen.over(button));
    let marked = hovered.and_then(|(focus, _)| pen.d.focus.get(focus.as_str()));
    for card in &o.cards {
        let skill = shown.skills[card.index];
        draw_card(pen, card, skill);
        match marked {
            Some(focus) if focus.skills.iter().any(|s| s == skill) => {
                paint::edge(pen.host, pen.batch, &card.rect, paint::FRAME_LIGHT);
                paint::edge(pen.host, pen.batch, &card.rect.shrunk(1.0), paint::FRAME);
            }
            Some(_) => pen.fill(&card.rect, FADED),
            None => {}
        }
    }
    if let Some(first) = o.focus_buttons.first() {
        let label = say(&FOCUS, &[]).plain();
        pen.text(&label, o.panel.x + MARGIN, first.y + 5.0, GRAY);
    }
    for (focus, button) in shown.focus.iter().zip(&o.focus_buttons) {
        let current = pen.state.focus.contains(focus.as_str());
        let name = paint::fitted(pen.host, &focus_name(pen.d, focus).plain(), button.w - 6.0);
        pen.button(button, &name, if current { Look::Go } else { Look::Plain });
        if pen.over(button) {
            pen.tip = Some(focus_tip(pen, focus, current));
        }
    }
    pen.scroll(o.scroll);
}

fn draw_card(pen: &mut Pen<'_>, card: &Card, skill: &str) {
    let (d, state) = (pen.d, pen.state);
    let hover = pen.over(&card.rect);
    pen.fill(
        &card.rect,
        if hover {
            paint::CARD_HOVER
        } else {
            paint::CARD
        },
    );
    let rim = if hover {
        paint::FRAME
    } else {
        paint::CARD_EDGE
    };
    paint::edge(pen.host, pen.batch, &card.rect, rim);
    paint::slot(pen.host, pen.batch, &card.slot);
    if let Some(icon) = d.skill(skill).map(|s| s.icon_at(level(state, d, skill))) {
        pen.host.item(
            pen.batch,
            icon,
            [card.slot.x + 2.0, card.slot.y + 2.0],
            ICON,
        );
    }
    let (now, most) = (level(state, d, skill), d.max_level(skill));
    let levels = say(&LEVEL, &[number(now), number(most)]).plain();
    let right = card.rect.x + card.rect.w - 6.0;
    let top = card.rect.y + 5.0;
    pen.text_right(&levels, right, top, if now >= most { GOLD } else { GRAY });
    let room = right - pen.width(&levels) - 4.0 - card.bar.x;
    let name = paint::fitted(pen.host, &skill_name(d, skill).plain(), room);
    pen.text(&name, card.bar.x, top, WHITE);
    paint::bar(pen.host, pen.batch, &card.bar, now, most);
    let points = points_available(d, state, skill) as usize;
    if !mastered(d, state, skill) {
        let diamonds = paint::POINT.repeat(points.min(MOST_POINTS));
        pen.text(&diamonds, card.bar.x, card.bar.y + 7.0, GOLD);
    }
    if !pen.rules.buying() {
        if let Some((have, need)) = earned_of(d, state, skill) {
            let line = say(&LEVEL, &[number(have), number(need)]).plain();
            paint::text_centred(pen.host, pen.batch, &line, &card.raise, GRAY);
        }
    } else if let Some(cost) = price(d, pen.rules, state, skill) {
        let look = if can_raise(d, pen.rules, state, pen.xp, skill) {
            Look::Go
        } else {
            Look::Off
        };
        let hover = pen.over(&card.raise);
        paint::arrow_button(
            pen.host,
            pen.batch,
            &card.raise,
            &cost.to_string(),
            look,
            hover,
        );
    }
    if mastered(d, state, skill) {
        draw_mastery(pen, card);
    }
    if hover {
        pen.tip = Some(if pen.over(&card.raise) {
            raise_tip(pen, skill)
        } else {
            skill_tip(pen, skill)
        });
    }
}

/// What a mastered skill's card wears: a gold double frame, the crown on its icon, a band of light sweeping across
/// every few seconds, and "Mastered" where its raise button was.
fn draw_mastery(pen: &mut Pen<'_>, card: &Card) {
    let along = pen.sweep();
    paint::shine(pen.host, pen.batch, &card.rect, along);
    paint::edge(pen.host, pen.batch, &card.rect, paint::FRAME_LIGHT);
    paint::edge(pen.host, pen.batch, &card.rect.shrunk(1.0), paint::FRAME);
    let at = [
        card.slot.x + card.slot.w - paint::CROWN_SIZE[0] + 2.0,
        card.slot.y - 3.0,
    ];
    paint::crown(pen.host, pen.batch, at);
    let label = say(&MASTERED, &[]).plain();
    pen.text(&label, card.bar.x, card.bar.y + 6.0, GOLD);
}

/// XP levels enough to price any run of raises, without overflowing the points they stand for.
const PRICING_XP: u32 = 10_000;

/// Where `times` raises of the skill lead and their whole price in the rules' unit, as the server would grant them
/// to a player with `xp` levels; None when not even one would be granted.
fn raises_price(
    d: &Defs,
    rules: &Rules,
    state: &PlayerState,
    xp: u32,
    skill: &str,
    times: u32,
) -> Option<(u32, u32)> {
    let request = Request::Raise {
        skill: skill.to_string(),
        times,
    };
    let outcome = apply_under(d, rules, state, xp, &request).ok()?;
    let price = match rules.cost_unit {
        CostUnit::Levels => outcome.levels_spent,
        CostUnit::Points => outcome.points_spent,
    };
    Some((level(&outcome.state, d, skill), price))
}

fn raise_tip(pen: &Pen<'_>, skill: &str) -> Vec<Text> {
    let (d, state) = (pen.d, pen.state);
    let Some(cost) = price(d, pen.rules, state, skill) else {
        return vec![say(&AT_TOP, &[]).with_color(GOLD)];
    };
    let now = level(state, d, skill);
    let held = raises_wanted(pen.host);
    if held > 1 {
        let xp = pen.xp.unwrap_or(0);
        // Shift prices five raises whatever the XP; Control the most the XP pays for, or one when none.
        let priced = if held == u32::MAX {
            raises_price(d, pen.rules, state, xp, skill, held)
                .map(|(reach, price)| (reach, price, true))
        } else {
            raises_price(d, pen.rules, state, PRICING_XP, skill, held).map(|(reach, price)| {
                let paid = raises_price(d, pen.rules, state, xp, skill, held)
                    .is_some_and(|(got, _)| got == reach);
                (reach, price, paid)
            })
        };
        let (reach, price, paid) = priced.unwrap_or((now + 1, cost, false));
        return vec![
            say(&RAISE_BY, &[number(reach), number(reach - now)]).with_color(WHITE),
            cost_line(pen.rules, price).with_color(if paid { GREEN } else { RED }),
        ];
    }
    let colour = if can_raise(d, pen.rules, state, pen.xp, skill) {
        GREEN
    } else {
        RED
    };
    vec![
        say(&RAISE_TO, &[number(level(state, d, skill) + 1)]).with_color(WHITE),
        cost_line(pen.rules, cost).with_color(colour),
        say(&RAISE_SHIFT, &[]).with_color(GRAY),
        say(&RAISE_CTRL, &[]).with_color(GRAY),
    ]
}

fn skill_tip(pen: &Pen<'_>, skill: &str) -> Vec<Text> {
    let (d, state) = (pen.d, pen.state);
    let levels = [number(level(state, d, skill)), number(d.max_level(skill))];
    let points = points_available(d, state, skill);
    vec![
        skill_name(d, skill).with_color(GOLD),
        say(&LEVEL_OF, &levels).with_color(WHITE),
        say(&PERK_POINTS, &[number(points)]).with_color(if points > 0 { GOLD } else { GRAY }),
        say(&OPEN_HINT, &[]).with_color(DIM),
    ]
}

/// A focus area's tooltip: its name, each task it gives XP for (once), and what a click does.
fn focus_tip(pen: &Pen<'_>, focus: &str, active: bool) -> Vec<Text> {
    let (d, state) = (pen.d, pen.state);
    let rewards = d
        .focus
        .get(focus)
        .map(|f| f.rewards.as_slice())
        .unwrap_or_default();
    let mut lines = vec![
        focus_name(d, focus).with_color(GOLD),
        say(&FOCUS_GIVES, &[]).with_color(GRAY),
    ];
    for reward in rewards {
        let line = reward_line(reward).with_color(WHITE);
        if !lines.iter().any(|known| known.plain() == line.plain()) {
            lines.push(line);
        }
    }
    let cost = focus_cost(d, state);
    lines.push(if active {
        say(&FOCUS_ACTIVE, &[]).with_color(GREEN)
    } else if cost == 0 {
        say(&FOCUS_ADD_FREE, &[]).with_color(GREEN)
    } else {
        let can = pen.xp.is_some_and(|xp| xp >= cost);
        say(&FOCUS_ADD, &[number(cost)]).with_color(if can { GREEN } else { RED })
    });
    lines
}

/// One task a focus area gives XP for, in words.
fn reward_line(reward: &Reward) -> Text {
    let percent = |p: u32| Text::literal(format!("{p}%"));
    match reward {
        Reward::Kill { bonus_percent } => say(&REWARD_KILL, &[percent(*bonus_percent)]),
        Reward::BlockXp { bonus_percent } => say(&REWARD_BLOCK_XP, &[percent(*bonus_percent)]),
        Reward::Break {
            label: Some(key), ..
        } => label_name(key),
        Reward::Break { label: None, .. } => say(&REWARD_BREAK, &[]),
        Reward::Smelt { .. } => say(&REWARD_SMELT, &[]),
        Reward::Travel { .. } => say(&REWARD_TRAVEL, &[]),
        Reward::Tame { .. } => say(&REWARD_TAME, &[]),
        Reward::Breed { .. } => say(&REWARD_BREED, &[]),
    }
}

fn draw_skill(
    pen: &mut Pen<'_>,
    size: [f32; 2],
    cache: &ClientCache,
    defs: &ClientDefs,
    shown: &Shown<'_>,
) {
    let Some(skill) = shown.skills.get(shown.selected).map(|s| s.as_str()) else {
        return;
    };
    let (d, state) = (pen.d, pen.state);
    let view = detail(size, shown.perks.len(), cache.first_perk);
    pen.window(
        &view.panel,
        &skill_name(d, skill).plain(),
        NAME_AFTER_BACK,
        0.0,
    );
    pen.button(&view.back, "<", Look::Plain);
    if pen.over(&view.back) {
        pen.tip = Some(vec![say(&BACK, &[]).with_color(WHITE)]);
    }
    draw_summary(pen, &view, skill);
    let heading = view.panel.y + 72.0;
    pen.text(
        &say(&PERKS, &[]).plain(),
        view.panel.x + MARGIN,
        heading,
        GOLD,
    );
    let rule = [
        view.panel.x + MARGIN,
        heading + 10.0,
        view.panel.w - 2.0 * MARGIN,
        1.0,
    ];
    pen.host.fill(pen.batch, rule, paint::CARD_EDGE);
    for row in &view.rows {
        draw_perk(pen, row, shown.perks[row.index]);
    }
    draw_preview(pen, defs, level(state, d, skill), skill, &view);
    pen.scroll(view.scroll);
}

/// The top of the skill view: the big icon, the level and its bar, the perk points, and the raise button.
fn draw_summary(pen: &mut Pen<'_>, view: &Detail, skill: &str) {
    let (d, state) = (pen.d, pen.state);
    paint::slot(pen.host, pen.batch, &view.slot);
    if let Some(icon) = d.skill(skill).map(|s| s.icon_at(level(state, d, skill))) {
        pen.host.item(
            pen.batch,
            icon,
            [view.slot.x + 2.0, view.slot.y + 2.0],
            BIG_ICON,
        );
    }
    let (now, most) = (level(state, d, skill), d.max_level(skill));
    let levels = say(&LEVEL_OF, &[number(now), number(most)]).plain();
    pen.text(&levels, view.bar.x, view.slot.y + 2.0, WHITE);
    paint::bar(pen.host, pen.batch, &view.bar, now, most);
    let points = points_available(d, state, skill);
    let line = say(&PERK_POINTS, &[number(points)]).plain();
    pen.text(
        &line,
        view.bar.x,
        view.bar.y + 11.0,
        if points > 0 { GOLD } else { GRAY },
    );
    let earned = earned_of(d, state, skill).filter(|_| pen.rules.earning());
    if let (Some((have, need)), false) = (earned, pen.rules.buying()) {
        let bar = Rect {
            y: view.raise.y + 6.0,
            h: 6.0,
            ..view.raise
        };
        paint::bar(pen.host, pen.batch, &bar, have, need);
        let under = Rect {
            y: view.raise.y + view.raise.h + 3.0,
            h: paint::GLYPH,
            ..view.raise
        };
        let line = say(&PROGRESS, &[number(have), number(need)]).plain();
        paint::text_centred(pen.host, pen.batch, &line, &under, GRAY);
        return;
    }
    if let Some((have, need)) = earned {
        let below = Rect {
            y: view.raise.y + view.raise.h + 13.0,
            h: paint::GLYPH,
            ..view.raise
        };
        let line = say(&PROGRESS, &[number(have), number(need)]).plain();
        paint::text_centred(pen.host, pen.batch, &line, &below, GRAY);
    }
    let Some(cost) = price(d, pen.rules, state, skill).filter(|_| pen.rules.buying()) else {
        if mastered(d, state, skill) {
            let label = say(&MASTERED, &[]).plain();
            paint::text_centred(pen.host, pen.batch, &label, &view.raise, GOLD);
            paint::crown(
                pen.host,
                pen.batch,
                [
                    view.slot.x + view.slot.w - paint::CROWN_SIZE[0] + 2.0,
                    view.slot.y - 3.0,
                ],
            );
        } else {
            pen.button(&view.raise, &say(&AT_TOP, &[]).plain(), Look::Off);
        }
        return;
    };
    let can = can_raise(d, pen.rules, state, pen.xp, skill);
    let hover = pen.over(&view.raise);
    let look = if can { Look::Go } else { Look::Off };
    let label = say(&LEVEL_UP, &[]).plain();
    paint::arrow_button(pen.host, pen.batch, &view.raise, &label, look, hover);
    let under = Rect {
        y: view.raise.y + view.raise.h + 3.0,
        h: paint::GLYPH,
        ..view.raise
    };
    let cost = cost_line(pen.rules, cost).plain();
    paint::text_centred(
        pen.host,
        pen.batch,
        &cost,
        &under,
        if can { GREEN } else { RED },
    );
    if pen.over(&view.raise) {
        pen.tip = Some(raise_tip(pen, skill));
    }
}

fn draw_perk(pen: &mut Pen<'_>, row: &PerkRow, perk: &PerkDef) {
    let (d, state) = (pen.d, pen.state);
    let status = status(d, state, perk);
    let hover = pen.over(&row.rect);
    pen.fill(
        &row.rect,
        if hover {
            paint::CARD_HOVER
        } else {
            paint::CARD
        },
    );
    let (x, y) = (row.rect.x, row.rect.y + 5.0);
    match status {
        PerkStatus::Owned => pen.text(paint::TICK, x + 3.0, y, GOLD),
        PerkStatus::Buyable => pen.text(paint::POINT, x + 4.0, y, GREEN),
        PerkStatus::Locked(_) => paint::padlock(pen.host, pen.batch, [x + 4.0, row.rect.y + 3.0]),
    }
    let name = perk_name(d, &perk.id).plain();
    let locked = matches!(status, PerkStatus::Locked(_));
    pen.text(&name, x + 16.0, y, if locked { GRAY } else { WHITE });
    let price_x = x + 16.0 + pen.width(&name) + 4.0;
    let price = paint::POINT.repeat(perk.cost as usize);
    let owned = matches!(status, PerkStatus::Owned);
    pen.text(&price, price_x, y, if owned { DIM } else { GOLD });
    let right = row.rect.x + row.rect.w - 6.0;
    match &status {
        PerkStatus::Owned => draw_owned(pen, row, perk, right, y),
        PerkStatus::Buyable => pen.button(&row.buy, &say(&GET, &[]).plain(), Look::Go),
        PerkStatus::Locked(why) => {
            let room = right - (price_x + pen.width(&price) + 6.0);
            let why = paint::fitted(pen.host, &why.plain(), room);
            pen.text_right(&why, right, y, RED);
        }
    }
    if hover {
        pen.tip = Some(perk_tip(pen, perk, &status));
    }
}

/// What a refund costs under the rules, in XP levels.
fn refund_cost(rules: &Rules) -> u32 {
    match rules.respec {
        Respec::Paid => rules.respec_cost,
        Respec::Off | Respec::Free => 0,
    }
}

/// An owned perk's end of the row: its switch and its refund button when the rules allow them, else "Owned".
fn draw_owned(pen: &mut Pen<'_>, row: &PerkRow, perk: &PerkDef, right: f32, y: f32) {
    let (switches, refunds) = (pen.rules.perk_switches, pen.rules.respec != Respec::Off);
    if !switches && !refunds {
        pen.text_right(&say(&OWNED, &[]).plain(), right, y, GOLD);
        return;
    }
    if switches {
        let on = !switched_off(pen.state, &perk.id);
        let label = say(if on { &VAL_ON } else { &VAL_OFF }, &[]).plain();
        pen.button(&row.buy, &label, if on { Look::Go } else { Look::Plain });
    }
    if refunds {
        let affordable = pen.xp.is_some_and(|xp| xp >= refund_cost(pen.rules));
        let look = if affordable { Look::Plain } else { Look::Off };
        let at = if switches { &row.refund } else { &row.buy };
        pen.button(at, &say(&REFUND, &[]).plain(), look);
    }
}

/// A perk's tooltip: its name, what it does, what it costs, and what it needs, each need green once met.
fn perk_tip(pen: &Pen<'_>, perk: &PerkDef, status: &PerkStatus) -> Vec<Text> {
    let (d, state) = (pen.d, pen.state);
    let met = |met: bool| if met { GREEN } else { RED };
    let mut lines = vec![perk_name(d, &perk.id).with_color(GOLD)];
    let does = perk_description(perk).plain();
    let wrapped = paint::wrapped(pen.host, &does, TIP_WIDTH);
    lines.extend(
        wrapped
            .into_iter()
            .map(|line| Text::literal(line).with_color(GRAY)),
    );
    if matches!(status, PerkStatus::Owned) {
        lines.push(say(&OWNED, &[]).with_color(GOLD));
        if switched_off(state, &perk.id) {
            lines.push(say(&SWITCHED_OFF, &[]).with_color(DIM));
        }
        match (pen.rules.respec, refund_cost(pen.rules)) {
            (Respec::Off, _) => {}
            (_, 0) => lines.push(say(&REFUND_FREE, &[]).with_color(GRAY)),
            (_, cost) => lines.push(say(&REFUND_COST, &[number(cost)]).with_color(GRAY)),
        }
        return lines;
    }
    let enough = points_available(d, state, &perk.skill) >= perk.cost;
    lines.push(say(&PERK_COST, &[number(perk.cost)]).with_color(met(enough)));
    if perk.min_level > 1 {
        let reached = level(state, d, &perk.skill) >= perk.min_level;
        let needs = [skill_name(d, &perk.skill), number(perk.min_level)];
        lines.push(say(&NEEDS, &needs).with_color(met(reached)));
    }
    for other in &perk.requires {
        let first = say(&MISSING_PERK, &[perk_name(d, other)]);
        lines.push(first.with_color(met(owns(state, other))));
    }
    lines
}

/// What the next levels of the skill unlock, as many lines as fit.
fn draw_preview(pen: &mut Pen<'_>, defs: &ClientDefs, now: u32, skill: &str, view: &Detail) {
    let area = &view.preview;
    pen.text(&say(&NEXT_UNLOCKS, &[]).plain(), area.x, area.y, GOLD);
    let by_level = defs.labels.get(skill);
    let next_tier = by_level.and_then(|levels| levels.range(now + 1..).next().map(|(l, _)| *l));
    if let Some(next) = next_tier {
        let reached = by_level.and_then(|levels| levels.range(..=now).next_back().map(|(l, _)| *l));
        let from = reached.unwrap_or(1);
        paint::bar(pen.host, pen.batch, &view.next_bar, now - from, next - from);
        let count = say(&LEVEL, &[number(now), number(next)]).plain();
        pen.text_right(&count, area.x + area.w, area.y, GRAY);
    }
    let next = defs
        .labels
        .get(skill)
        .map(|by_level| by_level.range(now + 1..));
    let mut lines = next.into_iter().flatten().take(PREVIEW_LINES).peekable();
    if lines.peek().is_none() {
        let none = say(&ALL_UNLOCKED, &[]).plain();
        pen.text(&none, area.x, area.y + PREVIEW_HEADING, DIM);
    }
    for (n, (needed, labels)) in lines.enumerate() {
        let names: Vec<String> = labels.iter().map(|key| label_name(key).plain()).collect();
        let line = say(&UNLOCK, &[number(*needed), Text::literal(names.join(", "))]).plain();
        let line = paint::fitted(pen.host, &line, area.w);
        let y = area.y + PREVIEW_HEADING + n as f32 * PREVIEW_LINE;
        pen.text(&line, area.x, y, GRAY);
    }
}

/// What a click on the screen leads to.
enum Outcome {
    Nothing,
    Ask(Request),
    Open(usize),
    Back,
    OpenSettings,
}

/// A click on the screen: opens a skill, goes back, or queues a raise, perk or focus request the player can pay
/// for. Every click on this mod's screen is taken.
pub fn click(host: &dyn Host, size: [f32; 2], at: [f32; 2], button: i32) -> Flow {
    if host.mod_screen() != Some(MOD_ID) {
        return Flow::Continue;
    }
    if with(|cache| cache.view) == View::Settings {
        return super::options::click(host, size, at, button);
    }
    with(|cache| {
        let outcome = match &cache.defs {
            Some(defs) => outcome(cache, defs, size, at, button, raises_wanted(host)),
            None => Outcome::Nothing,
        };
        let clicked = !matches!(outcome, Outcome::Nothing);
        match outcome {
            Outcome::Nothing => {}
            Outcome::Ask(request) => cache.asked_for(ToServer::Act(request)),
            Outcome::Open(skill) => {
                cache.selected = skill;
                cache.first_perk = 0;
                cache.view = View::Skill;
            }
            Outcome::Back => cache.view = View::Overview,
            Outcome::OpenSettings => cache.view = View::Settings,
        }
        if clicked && cache.settings.click_sounds {
            cache.sounds.push(CLICK_SOUND);
        }
    });
    Flow::Cancel
}

/// How many raises a click on a raise button asks for: five with Shift held, as many as the player can pay for
/// with Control, else one.
fn raises_wanted(host: &dyn Host) -> u32 {
    if host.has_control_down() {
        u32::MAX
    } else if host.has_shift_down() {
        SHIFT_RAISES
    } else {
        1
    }
}

fn outcome(
    cache: &ClientCache,
    defs: &ClientDefs,
    size: [f32; 2],
    at: [f32; 2],
    button: i32,
    times: u32,
) -> Outcome {
    let (d, state, xp) = (&defs.defs, &cache.state, cache.xp_levels);
    let shown = shown(defs, cache.selected);
    let raise = |skill: Option<&&String>| match skill {
        Some(skill) if can_raise(d, &cache.rules, state, xp, skill) => {
            Outcome::Ask(Request::Raise {
                skill: (*skill).clone(),
                times,
            })
        }
        _ => Outcome::Nothing,
    };
    let hit = match (cache.view, button) {
        (View::Skill, RIGHT_BUTTON) => Some(Button::Back),
        (View::Overview, LEFT_BUTTON) => {
            let o = overview(size, shown.skills.len(), cache.first_row, shown.focus.len());
            hit_overview(&o, at[0], at[1])
        }
        (View::Skill, LEFT_BUTTON) => {
            let view = detail(size, shown.perks.len(), cache.first_perk);
            hit_detail(&view, at[0], at[1])
        }
        _ => None,
    };
    match hit {
        Some(Button::Open(skill)) => Outcome::Open(skill),
        Some(Button::Back) => Outcome::Back,
        Some(Button::Settings) => Outcome::OpenSettings,
        Some(Button::Raise(skill)) => raise(shown.skills.get(skill)),
        Some(Button::RaiseShown) => raise(shown.skills.get(shown.selected)),
        Some(Button::SetFocus(focus)) => match shown.focus.get(focus) {
            Some(focus) if state.focus.contains(focus.as_str()) => Outcome::Nothing,
            Some(focus) if xp.is_some_and(|xp| xp >= focus_cost(d, state)) => {
                Outcome::Ask(Request::AddFocus {
                    focus: (*focus).clone(),
                })
            }
            _ => Outcome::Nothing,
        },
        Some(Button::BuyPerk(perk)) => match shown.perks.get(perk) {
            Some(perk) if matches!(status(d, state, perk), PerkStatus::Buyable) => {
                Outcome::Ask(Request::BuyPerk {
                    perk: perk.id.clone(),
                })
            }
            Some(perk) if owns(state, &perk.id) && cache.rules.perk_switches => {
                Outcome::Ask(Request::SwitchPerk {
                    perk: perk.id.clone(),
                })
            }
            Some(perk) if owns(state, &perk.id) && refunds_at(&cache.rules, xp, false) => {
                Outcome::Ask(Request::RefundPerk {
                    perk: perk.id.clone(),
                })
            }
            _ => Outcome::Nothing,
        },
        Some(Button::RefundPerk(perk)) => match shown.perks.get(perk) {
            Some(perk)
                if owns(state, &perk.id)
                    && refunds_at(&cache.rules, xp, cache.rules.perk_switches) =>
            {
                Outcome::Ask(Request::RefundPerk {
                    perk: perk.id.clone(),
                })
            }
            _ => Outcome::Nothing,
        },
        None => Outcome::Nothing,
    }
}

/// Whether a refund can be asked for: refunds on, the player able to pay, and the button where the click was (the
/// refund button sits before the switch when there is one, else in the switch's place).
fn refunds_at(rules: &Rules, xp: Option<u32>, beside_switch: bool) -> bool {
    rules.respec != Respec::Off
        && rules.perk_switches == beside_switch
        && xp.is_some_and(|xp| xp >= refund_cost(rules))
}

/// The mouse wheel on the screen: moves the cards or the perk rows when there are more than fit.
pub fn scroll(host: &dyn Host, amount: f64) -> Flow {
    if host.mod_screen() != Some(MOD_ID) {
        return Flow::Continue;
    }
    with(|cache| {
        let Some(defs) = &cache.defs else { return };
        let shown = shown(defs, cache.selected);
        let (skills, perks) = (shown.skills.len(), shown.perks.len());
        let stepped = |now: usize, last: usize| {
            if amount > 0.0 {
                now.min(last).saturating_sub(1)
            } else if amount < 0.0 {
                (now + 1).min(last)
            } else {
                now
            }
        };
        match cache.view {
            View::Overview => cache.first_row = stepped(cache.first_row, last_first_row(skills)),
            View::Skill => cache.first_perk = stepped(cache.first_perk, last_first_perk(perks)),
            View::Settings => {}
        }
    });
    Flow::Cancel
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use horizon_client::mod_api::{Flow, HudBatch};
    use skills_core::defs::{FocusDef, PerkDef, Reward, SkillDef};
    use skills_core::progress::{PlayerState, Request, SkillProgress};
    use skills_core::registry::Defs;
    use skills_core::wire::{ClientDefs, ToServer};

    use super::*;
    use crate::client::cache::with;
    use crate::test_host::{Paint, TestHost, exclusive, mods};
    use crate::{ACT_CHANNEL, MOD_ID};

    const MINING: &str = "reskilled:mining";
    const MELEE: &str = "reskilled:melee";
    const WORKING: &str = "reskilled:working";
    const FIGHTING: &str = "reskilled:fighting";
    const SIZE: [f32; 2] = [427.0, 240.0];
    /// Nowhere near anything.
    const AWAY: [f32; 2] = [1.0, 1.0];

    fn skill(id: &str) -> SkillDef {
        let path = id.split_once(':').unwrap().1;
        SkillDef {
            id: id.into(),
            priority: 0,
            name: format!("reskilled.skill.{path}"),
            icon: "minecraft:iron_pickaxe".into(),
            max_level: None,
            cost: None,
            points_every: None,
            icon_levels: Default::default(),
        }
    }

    fn perk(path: &str, min_level: u32, requires: &[&str]) -> PerkDef {
        PerkDef {
            id: format!("reskilled:{path}"),
            priority: 0,
            skill: MINING.into(),
            name: format!("reskilled.perk.{path}"),
            description: format!("reskilled.perk.{path}.desc"),
            cost: 1,
            min_level,
            requires: requires.iter().map(|r| format!("reskilled:{r}")).collect(),
            effects: vec![],
        }
    }

    fn focus(id: &str, skills: &[&str]) -> (String, FocusDef) {
        let path = id.split_once(':').unwrap().1;
        let def = FocusDef {
            id: id.into(),
            priority: 0,
            name: format!("reskilled.focus.{path}"),
            skills: skills.iter().map(|s| (*s).to_string()).collect(),
            rewards: vec![],
        };
        (id.into(), def)
    }

    /// Melee and Mining; Mining perks Sure Grip (owned), Deep Veins (level 16) and Rich Seams (buyable); focus areas
    /// Fighting (Melee) and Working (Mining, the player's); the player at Mining 12 with 1 point spent and 23 XP
    /// levels, on the overview.
    fn screen_open() -> TestHost {
        let defs = Defs {
            skills: [(MINING.into(), skill(MINING)), (MELEE.into(), skill(MELEE))].into(),
            perks: [
                perk("mining_sure_grip", 4, &[]),
                perk("mining_deep_veins", 16, &[]),
                perk("mining_rich_seams", 8, &["mining_sure_grip"]),
            ]
            .into_iter()
            .map(|p| (p.id.clone(), p))
            .collect(),
            focus: [focus(FIGHTING, &[MELEE]), focus(WORKING, &[MINING])].into(),
            ..Defs::default()
        };
        let labels: BTreeMap<String, BTreeMap<u32, Vec<String>>> = [(
            MINING.to_string(),
            [
                (8, vec!["reskilled.lock.iron_pickaxes".to_string()]),
                (16, vec!["reskilled.lock.diamond_pickaxes".to_string()]),
            ]
            .into(),
        )]
        .into();
        let mut state = PlayerState {
            focus: [WORKING.to_string()].into(),
            focus_chosen_before: true,
            ..PlayerState::default()
        };
        state.skills.insert(
            MINING.into(),
            SkillProgress {
                level: 12,
                points_spent: 1,
                perks: ["reskilled:mining_sure_grip".to_string()].into(),
                ..SkillProgress::default()
            },
        );
        with(|cache| {
            cache.defs = Some(ClientDefs { defs, labels });
            cache.state = state;
            cache.xp_levels = Some(23);
        });
        TestHost {
            open_screen: Some(MOD_ID.into()),
            ..TestHost::default()
        }
    }

    /// The screen open on Mining's own view (skills are listed by id: Melee, then Mining).
    fn mining_open() -> TestHost {
        let host = screen_open();
        with(|cache| {
            cache.selected = 1;
            cache.view = View::Skill;
        });
        host
    }

    fn drawn(host: &TestHost, mouse: [f32; 2]) -> Vec<String> {
        draw(host, &mut HudBatch::new([427, 240]), SIZE, mouse);
        host.drawn_text()
    }

    fn tooltip(host: &TestHost) -> Vec<String> {
        let painted = host.painted.borrow();
        let tips: Vec<&Paint> = painted
            .iter()
            .filter(|p| matches!(p, Paint::Tooltip { .. }))
            .collect();
        match tips.as_slice() {
            [] => Vec::new(),
            [Paint::Tooltip { lines, .. }] => lines.clone(),
            more => panic!("more than one tooltip: {more:?}"),
        }
    }

    fn requests_sent(host: &mut TestHost) -> Vec<ToServer> {
        horizon_client::mod_api::dispatch_tick(host, &mods());
        host.sent_to_server
            .iter()
            .filter(|(channel, _)| channel == ACT_CHANNEL)
            .map(|(_, data)| serde_json::from_slice(data).unwrap())
            .filter(|m| matches!(m, ToServer::Act(_)))
            .collect()
    }

    fn cards() -> Vec<Card> {
        overview(SIZE, 2, 0, 2).cards
    }

    fn mining_view() -> Detail {
        detail(SIZE, 3, 0)
    }

    fn inside(r: &Rect) -> [f32; 2] {
        [r.x + 2.0, r.y + 2.0]
    }

    fn contains(drawn: &[String], expected: &[&str]) {
        for text in expected {
            assert!(drawn.iter().any(|t| t == text), "{text:?} not in {drawn:?}");
        }
    }

    #[test]
    fn the_overview_shows_a_card_for_each_skill() {
        // Given the screen open on the overview
        let _serial = exclusive();
        let host = screen_open();

        // When it is drawn
        let drawn = drawn(&host, AWAY);

        // Then the header shows the XP levels, each skill its level and raise cost, Mining its perk points as
        // diamonds, each focus area its button, and each skill its icon
        contains(
            &drawn,
            &[
                "Reskilled",
                "XP levels: 23",
                "Melee",
                "1/32",
                "1",
                "Mining",
                "12/32",
                "12",
                "\u{25C6}\u{25C6}\u{25C6}\u{25C6}\u{25C6}",
                "Focus",
                "Fighting",
                "Working",
            ],
        );
        assert_eq!(host.icons.borrow().len(), 2);
        assert_eq!(tooltip(&host), Vec::<String>::new());
    }

    #[test]
    fn skill_icons_show_the_tier_the_player_reached() {
        // Given Mining with a wooden pickaxe icon that becomes iron at 8 and diamond at 16, the player at 12
        let _serial = exclusive();
        let host = screen_open();
        with(|cache| {
            let mining = cache
                .defs
                .as_mut()
                .unwrap()
                .defs
                .skills
                .get_mut(MINING)
                .unwrap();
            mining.icon = "minecraft:wooden_pickaxe".into();
            mining.icon_levels = [
                (8, "minecraft:iron_pickaxe".to_string()),
                (16, "minecraft:diamond_pickaxe".to_string()),
            ]
            .into();
        });
        let shown = |host: &TestHost| -> Vec<String> {
            host.icons
                .borrow()
                .iter()
                .map(|(item, _)| item.clone())
                .filter(|i| i.contains("pickaxe"))
                .collect()
        };

        // When the overview is drawn, then Mining's own view, then the overview again at level 16
        drawn(&host, AWAY);
        let card = shown(&host);
        host.icons.borrow_mut().clear();
        with(|cache| {
            cache.selected = 1;
            cache.view = View::Skill;
        });
        drawn(&host, AWAY);
        let view = shown(&host);
        host.icons.borrow_mut().clear();
        with(|cache| {
            cache.view = View::Overview;
            cache.state.skills.get_mut(MINING).unwrap().level = 16;
        });
        drawn(&host, AWAY);

        // Then the card and the view show the iron pickaxe, and the diamond one once Mining reaches 16
        assert!(
            card.contains(&"minecraft:iron_pickaxe".to_string()),
            "{card:?}"
        );
        assert_eq!(view, vec!["minecraft:iron_pickaxe"]);
        assert!(shown(&host).contains(&"minecraft:diamond_pickaxe".to_string()));
    }

    /// Melee mastered: at 32 (it has no perks here).
    fn melee_mastered() {
        with(|cache| {
            let progress = SkillProgress {
                level: 32,
                ..SkillProgress::default()
            };
            cache.state.skills.insert(MELEE.into(), progress);
        });
    }

    #[test]
    fn a_mastered_skill_wears_gold_a_crown_and_says_so() {
        // Given Melee mastered and Mining not
        let _serial = exclusive();
        let host = screen_open();
        melee_mastered();
        let [melee, mining] = [cards()[0], cards()[1]];

        // When the overview is drawn
        let drawn = drawn(&host, AWAY);
        let painted = host.painted.borrow().clone();

        // Then Melee's card has a gold frame, the crown over its icon and "Mastered" where its raise button was;
        // Mining's has none of these
        let gold_top = |r: Rect| {
            let top = [r.x, r.y, r.w, 1.0];
            painted.iter().any(|p| matches!(p, Paint::Fill { rect, argb } if *rect == top && *argb == paint::FRAME_LIGHT))
        };
        let crowned = |r: Rect| {
            painted.iter().any(|p| matches!(p, Paint::Fill { rect, argb } if *argb == paint::CROWN && r.contains(rect[0], rect[1])))
        };
        assert!(gold_top(melee.rect) && !gold_top(mining.rect));
        assert!(crowned(melee.slot) && !crowned(mining.slot));
        assert_eq!(drawn.iter().filter(|t| *t == "Mastered").count(), 1);
        let label = painted.iter().find_map(|p| match p {
            Paint::Text { text, at, .. } if text == "Mastered" => Some(*at),
            _ => None,
        });
        let [x, y] = label.unwrap();
        let inside = x >= melee.rect.x
            && y >= melee.rect.y
            && x + host.text_width("Mastered") <= melee.rect.x + melee.rect.w - 3.0
            && y + 8.0 <= melee.rect.y + melee.rect.h - 3.0;
        assert!(inside, "{label:?} leaves {:?}", melee.rect);
    }

    #[test]
    fn a_mastered_card_shines_with_a_band_that_moves() {
        // Given Melee mastered
        let _serial = exclusive();
        let host = screen_open();
        melee_mastered();
        let melee = cards()[0].rect;
        let band = |host: &TestHost| -> Vec<[[f32; 2]; 4]> {
            host.painted
                .borrow()
                .iter()
                .filter_map(|p| match p {
                    Paint::Quad { points, .. } => Some(*points),
                    _ => None,
                })
                .collect()
        };

        // When the overview is drawn, and again 10 ticks later
        drawn(&host, AWAY);
        let first = band(&host);
        host.painted.borrow_mut().clear();
        with(|cache| cache.ticks += 10);
        drawn(&host, AWAY);
        let later = band(&host);

        // Then one translucent band lies on Melee's card, inside it, and it has moved along
        assert_eq!((first.len(), later.len()), (1, 1));
        for point in first[0].iter().chain(&later[0]) {
            assert!(
                point[0] >= melee.x && point[0] <= melee.x + melee.w,
                "{point:?}"
            );
            assert!(
                point[1] >= melee.y && point[1] <= melee.y + melee.h,
                "{point:?}"
            );
        }
        assert_ne!(first, later);
    }

    #[test]
    fn a_mastered_skills_page_says_so() {
        // Given Melee mastered
        let _serial = exclusive();
        let host = screen_open();
        melee_mastered();
        with(|cache| cache.view = View::Skill);

        // When its page is drawn
        let drawn = drawn(&host, AWAY);

        // Then it says it is mastered
        assert!(drawn.iter().any(|t| t == "Mastered"), "{drawn:?}");
    }

    #[test]
    fn everything_is_drawn_inside_the_window() {
        // Given the screen open, on the overview and then on Mining's view
        let _serial = exclusive();
        let host = screen_open();
        let window = panel(SIZE);

        // When each view is drawn
        draw(&host, &mut HudBatch::new([427, 240]), SIZE, AWAY);
        with(|cache| {
            cache.selected = 1;
            cache.view = View::Skill;
        });
        draw(&host, &mut HudBatch::new([427, 240]), SIZE, AWAY);

        // Then every text and icon lies inside the window
        for paint in host.painted.borrow().iter() {
            let (x, y, w, h) = match paint {
                Paint::Text { text, at, .. } => (at[0], at[1], host.text_width(text), 8.0),
                Paint::Item { at, size, .. } => (at[0], at[1], *size, *size),
                _ => continue,
            };
            let fits = x >= window.x + 3.0
                && y >= window.y + 3.0
                && x + w <= window.x + window.w - 3.0
                && y + h <= window.y + window.h - 3.0;
            assert!(fits, "{paint:?} leaves {window:?}");
        }
    }

    #[test]
    fn a_card_opens_its_skill_with_perks_and_why_each_is_locked() {
        // Given the screen open on the overview
        let _serial = exclusive();
        let host = screen_open();

        // When Mining's card is clicked and the screen is drawn
        let flow = click(&host, SIZE, inside(&cards()[1].rect), LEFT_BUTTON);
        let drawn = drawn(&host, AWAY);

        // Then Mining's view is up: its level, its points, the owned perk, the one to unlock, and the one that
        // needs a higher level
        assert_eq!(flow, Flow::Cancel);
        assert_eq!(with(|cache| (cache.view, cache.selected)), (View::Skill, 1));
        contains(
            &drawn,
            &[
                "Mining",
                "Level 12 of 32",
                "Perk points: 5",
                "Level up",
                "Cost: 12 XP levels",
                "Perks",
                "Mining sure grip",
                "On",
                "Mining rich seams",
                "Unlock",
                "Mining deep veins",
                "Needs Mining 16",
            ],
        );
    }

    #[test]
    fn the_skill_view_lists_only_what_the_next_levels_unlock() {
        // Given Mining at 12 with locks labelled at 8 and 16
        let _serial = exclusive();
        let host = mining_open();

        // When its view is drawn
        let drawn = drawn(&host, AWAY);

        // Then only the level still ahead is listed
        contains(&drawn, &["Next unlocks", "Level 16: Diamond pickaxes"]);
        assert!(
            !drawn.iter().any(|t| t.contains("Iron pickaxes")),
            "{drawn:?}"
        );
    }

    #[test]
    fn raise_buttons_show_a_drawn_arrow_beside_the_cost() {
        // Given the overview
        let _serial = exclusive();
        let host = screen_open();
        let raise = cards()[1].raise;

        // When it is drawn
        let drawn = drawn(&host, AWAY);

        // Then Mining's raise button holds its cost as plain text and an arrow drawn pixel by pixel, no glyph
        assert!(!drawn.iter().any(|t| t.contains('\u{25B2}')), "{drawn:?}");
        let arrow = host
            .painted
            .borrow()
            .iter()
            .filter(|p| matches!(p, Paint::Fill { rect, argb } if *argb == paint::ARROW && raise.contains(rect[0], rect[1])))
            .count();
        assert!(arrow >= 5, "{arrow}");
    }

    #[test]
    fn the_skill_view_shows_how_far_the_next_unlock_is() {
        // Given Mining at 12, between its unlocks at 8 and 16
        let _serial = exclusive();
        let host = mining_open();
        let bar = mining_view().next_bar;

        // When its view is drawn
        let drawn = drawn(&host, AWAY);

        // Then a bar beside "Next unlocks" is half full (4 of the 8 levels from 8 to 16) and reads 12/16
        contains(&drawn, &["Next unlocks", "12/16"]);
        let inner = bar.shrunk(1.0);
        let half = [inner.x, inner.y, (inner.w * 0.5).round(), inner.h];
        let filled = host
            .painted
            .borrow()
            .iter()
            .any(|p| matches!(p, Paint::Fill { rect, argb } if *rect == half && *argb == paint::BAR_FILL));
        assert!(filled, "{:?}", host.painted.borrow());
    }

    #[test]
    fn a_skill_with_nothing_left_to_unlock_says_so() {
        // Given Mining past its last labelled lock
        let _serial = exclusive();
        let host = mining_open();
        with(|cache| cache.state.skills.get_mut(MINING).unwrap().level = 20);

        // When its view is drawn
        let drawn = drawn(&host, AWAY);

        // Then it says nothing more unlocks
        contains(&drawn, &["Nothing more to unlock here."]);
    }

    #[test]
    fn hovering_a_perk_shows_what_it_costs_and_needs() {
        // Given Mining's view, the mouse over its third perk by level (Deep Veins, level 16)
        let _serial = exclusive();
        let host = mining_open();
        let row = mining_view().rows[2];

        // When the screen is drawn
        drawn(&host, inside(&row.rect));

        // Then one tooltip names it, its cost and the level it needs
        assert_eq!(
            tooltip(&host),
            vec![
                "Mining deep veins",
                "Cost: 1 perk points",
                "Needs Mining 16"
            ]
        );
    }

    #[test]
    fn hovering_a_raise_button_shows_the_level_and_cost() {
        // Given the overview, the mouse over Mining's raise button
        let _serial = exclusive();
        let host = screen_open();

        // When the screen is drawn
        drawn(&host, inside(&cards()[1].raise));

        // Then the tooltip says what the raise gives and costs
        assert_eq!(
            tooltip(&host),
            vec![
                "Raise to level 13",
                "Cost: 12 XP levels",
                "Shift: +5 levels",
                "Ctrl: all you can pay for"
            ]
        );
    }

    #[test]
    fn holding_shift_shows_where_five_raises_lead_and_their_price() {
        // Given Mining at 12 (raises cost 12, 13, 14, 15, 16) and 23 XP levels, the mouse on its raise button
        let _serial = exclusive();
        let mut host = screen_open();
        host.shift = true;

        // When the screen is drawn with Shift held
        drawn(&host, inside(&cards()[1].raise));

        // Then the tooltip shows level 17 for 70 levels, in red as 23 do not pay for it
        assert_eq!(
            tooltip(&host),
            vec!["Raise to level 17 (+5)", "Cost: 70 XP levels"]
        );
        assert_eq!(cost_colour(&host), Some(RED));
    }

    #[test]
    fn holding_control_shows_the_highest_level_the_xp_pays_for() {
        // Given Mining at 12 and 40 XP levels (12 + 13 + 14 = 39 fits, 15 more does not)
        let _serial = exclusive();
        let mut host = screen_open();
        host.control = true;
        with(|cache| cache.xp_levels = Some(40));

        // When the screen is drawn with Control held
        drawn(&host, inside(&cards()[1].raise));

        // Then the tooltip shows level 15 for 39 levels, in green
        assert_eq!(
            tooltip(&host),
            vec!["Raise to level 15 (+3)", "Cost: 39 XP levels"]
        );
        assert_eq!(cost_colour(&host), Some(GREEN));
    }

    /// The colour of the tooltip's cost line.
    fn cost_colour(host: &TestHost) -> Option<u32> {
        let painted = host.painted.borrow();
        painted.iter().find_map(|p| match p {
            Paint::Tooltip { spans, .. } => spans
                .get(1)
                .and_then(|line| line.first())
                .and_then(|(_, colour)| *colour),
            _ => None,
        })
    }

    #[test]
    fn hovering_a_focus_shows_what_it_gives_xp_for_and_what_adding_it_costs() {
        // Given the overview, the player focused on Working; Fighting gives more XP from kills and some for
        // chopping logs (labelled) and for breaking anything (not labelled), and for travelling
        let _serial = exclusive();
        let host = screen_open();
        crate::text::shipped_english::load();
        with(|cache| {
            let fighting = cache
                .defs
                .as_mut()
                .unwrap()
                .defs
                .focus
                .get_mut(FIGHTING)
                .unwrap();
            let blocks = skills_core::defs::Matcher::Tag("#minecraft:logs".into());
            let chopping = Reward::Break {
                blocks: blocks.clone(),
                points: 0.25,
                state: Default::default(),
                label: Some("reskilled.reward.chopping".into()),
            };
            let anything = Reward::Break {
                blocks,
                points: 0.1,
                state: Default::default(),
                label: None,
            };
            fighting.rewards = vec![
                Reward::Kill { bonus_percent: 50 },
                chopping.clone(),
                chopping,
                anything,
                Reward::Travel { points: 0.02 },
            ];
        });
        let buttons = overview(SIZE, 2, 0, 2).focus_buttons;

        // When the screen is drawn with the mouse over Fighting, then over Working
        drawn(&host, inside(&buttons[0]));
        let fighting = tooltip(&host);
        host.painted.borrow_mut().clear();
        drawn(&host, inside(&buttons[1]));

        // Then Fighting lists each task once and what adding it as a second area costs; Working says it is active
        assert_eq!(
            fighting,
            vec![
                "Fighting",
                "Gives XP for:",
                "Kills: 50% more XP",
                "Chopping logs",
                "Breaking blocks",
                "Travelling",
                "Click to add it for 20 XP levels."
            ]
        );
        assert_eq!(
            tooltip(&host),
            vec!["Working", "Gives XP for:", "Unlocked: always on."]
        );
    }

    #[test]
    fn hovering_a_focus_marks_its_skills_on_the_cards() {
        // Given the overview, the mouse over Fighting, whose one skill is Melee (the first card)
        let _serial = exclusive();
        let host = screen_open();
        let fighting = overview(SIZE, 2, 0, 2).focus_buttons[0];
        let [melee, mining] = [cards()[0].rect, cards()[1].rect];

        // When the screen is drawn, then drawn again with the mouse away
        drawn(&host, inside(&fighting));
        let hovered = host.painted.borrow().clone();
        host.painted.borrow_mut().clear();
        drawn(&host, AWAY);
        let away = host.painted.borrow().clone();

        // Then Melee's card is outlined in bright gold and Mining's is faded over, and neither happens with the
        // mouse away
        let outlined = |painted: &[Paint], r: Rect| {
            let top = [r.x, r.y, r.w, 1.0];
            painted.iter().any(|p| matches!(p, Paint::Fill { rect, argb } if *rect == top && *argb == paint::FRAME_LIGHT))
        };
        let faded = |painted: &[Paint], r: Rect| {
            painted.iter().any(
                |p| matches!(p, Paint::Fill { rect, argb } if *rect == r.array() && *argb == FADED),
            )
        };
        assert!(outlined(&hovered, melee) && !outlined(&hovered, mining));
        assert!(faded(&hovered, mining) && !faded(&hovered, melee));
        assert!(!outlined(&away, melee) && !faded(&away, mining));
    }

    #[test]
    fn nothing_is_drawn_over_other_screens() {
        // Given another screen open
        let _serial = exclusive();
        let mut host = screen_open();
        host.open_screen = Some("othermod".into());

        // When the screen hook runs
        let drawn = drawn(&host, AWAY);

        // Then nothing is drawn
        assert!(drawn.is_empty() && host.painted.borrow().is_empty());
    }

    #[test]
    fn the_screen_waits_for_the_definitions() {
        // Given the screen open before the server sent its definitions
        let _serial = exclusive();
        let host = screen_open();
        with(|cache| cache.defs = None);

        // When it is drawn and clicked
        let drawn = drawn(&host, AWAY);
        let flow = click(&host, SIZE, inside(&cards()[0].rect), LEFT_BUTTON);

        // Then it says it is waiting, and the click is taken without doing anything
        contains(&drawn, &["Reskilled", "Waiting for the server."]);
        assert_eq!(flow, Flow::Cancel);
        assert_eq!(with(|cache| cache.view), View::Overview);
    }

    #[test]
    fn raise_buttons_send_a_raise_request() {
        // Given the screen open
        let _serial = exclusive();
        let mut host = screen_open();

        // When Mining's raise button is clicked on its card, then on its own view, then a tick passes
        let flow = click(&host, SIZE, inside(&cards()[1].raise), LEFT_BUTTON);
        let still = with(|cache| cache.view);
        click(&host, SIZE, inside(&cards()[1].rect), LEFT_BUTTON);
        click(&host, SIZE, inside(&mining_view().raise), LEFT_BUTTON);
        let sent = requests_sent(&mut host);

        // Then both clicks are taken and ask the server, and the card's button does not open the skill
        let raise = ToServer::Act(Request::Raise {
            skill: MINING.into(),
            times: 1,
        });
        assert_eq!(flow, Flow::Cancel);
        assert_eq!(still, View::Overview);
        assert_eq!(sent, vec![raise.clone(), raise]);
    }

    #[test]
    fn shift_raises_five_and_control_raises_all_that_can_be_paid() {
        // Given the screen open
        let _serial = exclusive();
        let mut host = screen_open();

        // When Mining's raise is clicked with Shift held, then with Control held
        host.shift = true;
        click(&host, SIZE, inside(&cards()[1].raise), LEFT_BUTTON);
        host.shift = false;
        host.control = true;
        click(&host, SIZE, inside(&cards()[1].raise), LEFT_BUTTON);
        let sent = requests_sent(&mut host);

        // Then the first asks for five raises and the second for as many as the server will grant
        let raise = |times| {
            ToServer::Act(Request::Raise {
                skill: MINING.into(),
                times,
            })
        };
        assert_eq!(sent, vec![raise(5), raise(u32::MAX)]);
    }

    #[test]
    fn the_raise_tip_says_what_shift_and_control_do() {
        // Given the screen open
        let _serial = exclusive();
        let host = screen_open();

        // When the mouse is over Mining's raise button
        drawn(&host, inside(&cards()[1].raise));

        // Then its tooltip ends with the hint
        assert_eq!(
            tooltip(&host)[2..],
            ["Shift: +5 levels", "Ctrl: all you can pay for"]
        );
    }

    #[test]
    fn the_gear_opens_the_options() {
        // Given the overview
        let _serial = exclusive();
        let host = screen_open();

        // When the gear is clicked
        click(
            &host,
            SIZE,
            inside(&overview(SIZE, 2, 0, 2).gear),
            LEFT_BUTTON,
        );

        // Then the options take over the screen
        assert_eq!(with(|cache| cache.view), View::Settings);
    }

    #[test]
    fn clicks_are_silent_when_turned_off() {
        // Given the screen open with clicks turned off
        let _serial = exclusive();
        let mut host = screen_open();
        host.eye = Some([3.0, 70.5, -8.0]);
        with(|cache| cache.settings.click_sounds = false);

        // When Mining's card is clicked and a tick passes
        click(&host, SIZE, inside(&cards()[1].rect), LEFT_BUTTON);
        horizon_client::mod_api::dispatch_tick(&mut host, &mods());

        // Then nothing plays
        assert!(host.sounds.is_empty());
    }

    #[test]
    fn a_click_that_does_something_ticks() {
        // Given the screen open, the player's eyes at a spot
        let _serial = exclusive();
        let mut host = screen_open();
        host.eye = Some([3.0, 70.5, -8.0]);

        // When bare window is clicked and a tick passes, then Mining's card and another tick
        click(&host, SIZE, inside(&panel(SIZE)), LEFT_BUTTON);
        horizon_client::mod_api::dispatch_tick(&mut host, &mods());
        let after_nothing = host.sounds.len();
        click(&host, SIZE, inside(&cards()[1].rect), LEFT_BUTTON);
        horizon_client::mod_api::dispatch_tick(&mut host, &mods());

        // Then only the click that opened something plays the button sound, quietly, once
        assert_eq!(after_nothing, 0);
        assert_eq!(
            host.sounds,
            vec![(
                "minecraft:ui.button.click".to_string(),
                [3.0, 70.5, -8.0],
                0.25,
                1.0
            )]
        );
    }

    #[test]
    fn under_the_points_rule_prices_read_in_xp_points() {
        // Given raises paid in XP points (20 a level of cost), Mining at 12 (a raise of 12 levels' worth: 240)
        let _serial = exclusive();
        let host = screen_open();
        with(|cache| cache.rules.cost_unit = skills_core::rules::CostUnit::Points);

        // When the overview is drawn, then Mining's page
        let overview = drawn(&host, AWAY);
        host.drawn.borrow_mut().clear();
        with(|cache| {
            cache.selected = 1;
            cache.view = View::Skill;
        });
        let page = drawn(&host, AWAY);

        // Then Mining's card asks 240 and its page says the cost is 240 XP points
        contains(&overview, &["240"]);
        contains(&page, &["Cost: 240 XP points"]);
    }

    #[test]
    fn when_skills_only_grow_by_doing_progress_replaces_the_raise() {
        // Given skills growing only by doing, Mining at 12 with 50 actions' worth of its 120 earned
        let _serial = exclusive();
        let mut host = screen_open();
        with(|cache| {
            cache.rules.progression = skills_core::rules::Progression::Earn;
            cache.state.skills.get_mut(MINING).unwrap().earned = 5_000;
        });

        // When the overview is drawn, then Mining's page, and Mining's card spot is clicked where its raise was
        let overview = drawn(&host, AWAY);
        click(&host, SIZE, inside(&cards()[1].raise), LEFT_BUTTON);
        host.drawn.borrow_mut().clear();
        with(|cache| {
            cache.selected = 1;
            cache.view = View::Skill;
        });
        let page = drawn(&host, AWAY);

        // Then the card shows 50/120 and no price, the page shows the progress and no level-up button, and nothing
        // is bought
        contains(&overview, &["50/120"]);
        assert!(!overview.iter().any(|t| t == "12"), "{overview:?}");
        contains(&page, &["Progress: 50/120"]);
        assert!(!page.iter().any(|t| t == "Level up"), "{page:?}");
        assert!(requests_sent(&mut host).is_empty());
    }

    #[test]
    fn when_skills_grow_both_ways_the_page_shows_the_raise_and_the_progress() {
        // Given skills growing both ways, Mining at 12 with 50 actions earned
        let _serial = exclusive();
        let host = mining_open();
        with(|cache| {
            cache.rules.progression = skills_core::rules::Progression::Both;
            cache.state.skills.get_mut(MINING).unwrap().earned = 5_000;
        });

        // When Mining's page is drawn
        let page = drawn(&host, AWAY);

        // Then it offers the raise and shows the progress
        contains(&page, &["Level up", "Progress: 50/120"]);
    }

    #[test]
    fn a_raise_the_player_cannot_pay_for_is_not_asked() {
        // Given the screen open and only 2 XP levels, when Mining's raise costs 12
        let _serial = exclusive();
        let mut host = screen_open();
        with(|cache| cache.xp_levels = Some(2));

        // When Mining's raise button is clicked, and Melee's, which costs 2
        click(&host, SIZE, inside(&cards()[1].raise), LEFT_BUTTON);
        click(&host, SIZE, inside(&cards()[0].raise), LEFT_BUTTON);

        // Then only the one they can pay for is asked
        let melee = ToServer::Act(Request::Raise {
            skill: MELEE.into(),
            times: 1,
        });
        assert_eq!(requests_sent(&mut host), vec![melee]);
    }

    #[test]
    fn only_a_perk_ready_to_unlock_is_asked_for() {
        // Given Mining's view: perks by level are Sure Grip (owned), Rich Seams (ready), Deep Veins (level 16)
        let _serial = exclusive();
        let mut host = mining_open();
        let rows = mining_view().rows;

        // When each perk's unlock spot is clicked
        for row in &rows {
            click(&host, SIZE, inside(&row.buy), LEFT_BUTTON);
        }

        // Then the owned one's switch is asked for, the ready one is bought, and the locked one asks nothing
        let switch_grip = ToServer::Act(Request::SwitchPerk {
            perk: "reskilled:mining_sure_grip".into(),
        });
        let rich_seams = ToServer::Act(Request::BuyPerk {
            perk: "reskilled:mining_rich_seams".into(),
        });
        assert_eq!(requests_sent(&mut host), vec![switch_grip, rich_seams]);
    }

    #[test]
    fn a_focus_button_unlocks_an_area_and_an_unlocked_one_stays() {
        // Given the overview, the player focused on Working
        let _serial = exclusive();
        let mut host = screen_open();
        host.eye = Some([0.0, 64.0, 0.0]);
        let buttons = overview(SIZE, 2, 0, 2).focus_buttons;

        // When Working is clicked, then Fighting
        click(&host, SIZE, inside(&buttons[1]), LEFT_BUTTON);
        let after_working = with(|cache| cache.sounds.len());
        click(&host, SIZE, inside(&buttons[0]), LEFT_BUTTON);

        // Then Working's click does nothing (not even a tick), and Fighting is asked for
        let fighting = ToServer::Act(Request::AddFocus {
            focus: FIGHTING.into(),
        });
        assert_eq!(after_working, 0);
        assert_eq!(requests_sent(&mut host), vec![fighting]);
    }

    #[test]
    fn a_focus_the_player_cannot_pay_for_is_not_asked() {
        // Given the overview and 19 XP levels, when a second area costs 20
        let _serial = exclusive();
        let mut host = screen_open();
        with(|cache| cache.xp_levels = Some(19));
        let fighting = overview(SIZE, 2, 0, 2).focus_buttons[0];

        // When Fighting is clicked
        click(&host, SIZE, inside(&fighting), LEFT_BUTTON);

        // Then nothing is asked
        assert!(requests_sent(&mut host).is_empty());
    }

    #[test]
    fn the_back_button_and_a_right_click_return_to_the_overview() {
        // Given Mining's view
        let _serial = exclusive();
        let host = mining_open();

        // When the back button is clicked, the view is opened again, and the right button is pressed anywhere
        click(&host, SIZE, inside(&mining_view().back), LEFT_BUTTON);
        let after_back = with(|cache| cache.view);
        with(|cache| cache.view = View::Skill);
        click(&host, SIZE, AWAY, RIGHT_BUTTON);

        // Then both lead back to the overview
        assert_eq!(after_back, View::Overview);
        assert_eq!(with(|cache| cache.view), View::Overview);
    }

    fn sent_acts(host: &mut TestHost) -> Vec<Request> {
        requests_sent(host)
            .into_iter()
            .filter_map(|m| match m {
                ToServer::Act(request) => Some(request),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_owned_perk_has_a_switch_and_a_refund_button() {
        // Given Mining's view, Sure Grip owned (the first perk), refunds paid at 5 levels
        let _serial = exclusive();
        let mut host = mining_open();
        let grip = mining_view().rows[0];

        // When the view is drawn with the mouse on Sure Grip, its switch is clicked, then its refund button
        let drawn = drawn(&host, inside(&grip.rect));
        let tip = tooltip(&host);
        click(&host, SIZE, inside(&grip.buy), LEFT_BUTTON);
        click(&host, SIZE, inside(&grip.refund), LEFT_BUTTON);

        // Then the row shows "On" and "Refund", the tooltip names the refund's price, and both are asked for
        contains(&drawn, &["On", "Refund"]);
        assert!(tip.iter().any(|t| t == "Refund: 5 XP levels"), "{tip:?}");
        let grip_id = "reskilled:mining_sure_grip".to_string();
        assert_eq!(
            sent_acts(&mut host),
            vec![
                Request::SwitchPerk {
                    perk: grip_id.clone()
                },
                Request::RefundPerk { perk: grip_id }
            ]
        );
    }

    #[test]
    fn a_switched_off_perk_reads_off() {
        // Given Mining's view, Sure Grip owned and switched off
        let _serial = exclusive();
        let host = mining_open();
        with(|cache| {
            let mining = cache.state.skills.get_mut(MINING).unwrap();
            mining.disabled.insert("reskilled:mining_sure_grip".into());
        });

        // When it is drawn
        let drawn = drawn(&host, AWAY);

        // Then its switch reads "Off"
        contains(&drawn, &["Off"]);
    }

    #[test]
    fn the_rules_take_the_switch_and_refund_away() {
        // Given Mining's view on a server with perk switches and refunds off
        let _serial = exclusive();
        let mut host = mining_open();
        with(|cache| {
            cache.rules.perk_switches = false;
            cache.rules.respec = skills_core::rules::Respec::Off;
        });
        let grip = mining_view().rows[0];

        // When it is drawn and both spots are clicked
        let drawn = drawn(&host, AWAY);
        click(&host, SIZE, inside(&grip.buy), LEFT_BUTTON);
        click(&host, SIZE, inside(&grip.refund), LEFT_BUTTON);

        // Then the row just says it is owned, and nothing is asked
        contains(&drawn, &["Owned"]);
        assert!(!drawn.iter().any(|t| t == "Refund"));
        assert!(sent_acts(&mut host).is_empty());
    }

    #[test]
    fn a_paid_refund_the_player_cannot_afford_is_not_asked() {
        // Given Mining's view with 4 XP levels, refunds costing 5
        let _serial = exclusive();
        let mut host = mining_open();
        with(|cache| cache.xp_levels = Some(4));

        // When Sure Grip's refund button is clicked
        click(
            &host,
            SIZE,
            inside(&mining_view().rows[0].refund),
            LEFT_BUTTON,
        );

        // Then nothing is asked
        assert!(sent_acts(&mut host).is_empty());
    }

    #[test]
    fn the_wheel_scrolls_perks_that_do_not_fit() {
        // Given Mining's view with seven perks, five of which fit
        let _serial = exclusive();
        let host = mining_open();
        with(|cache| {
            let defs = cache.defs.as_mut().unwrap();
            for n in 0..4 {
                let extra = perk(&format!("mining_extra_{n}"), 20 + n, &[]);
                defs.defs.perks.insert(extra.id.clone(), extra);
            }
        });

        // When the wheel turns down three times, then up once
        let flows = [-1.0, -1.0, -1.0].map(|turn| scroll(&host, turn));
        let down = with(|cache| cache.first_perk);
        scroll(&host, 1.0);

        // Then the rows stop at the last that fills the list, and come back up one
        assert_eq!(flows, [Flow::Cancel; 3]);
        assert_eq!((down, with(|cache| cache.first_perk)), (2, 1));
    }

    #[test]
    fn clicks_and_the_wheel_pass_through_when_the_screen_is_not_ours() {
        // Given no screen of this mod's open
        let _serial = exclusive();
        let mut host = screen_open();
        host.open_screen = None;

        // When a raise button's spot is clicked and the wheel turns
        let flow = click(&host, SIZE, inside(&cards()[0].raise), LEFT_BUTTON);
        let wheel = scroll(&host, -1.0);

        // Then both are left to others and nothing is sent
        assert_eq!((flow, wheel), (Flow::Continue, Flow::Continue));
        assert!(requests_sent(&mut host).is_empty());
    }
}
