//! The skills screen: a window over the world when the mod's own screen is open, with tabs for Home, each skill's
//! road and Options; its clicks become requests the next tick sends (the screen hooks cannot send).

use horizon_client::mod_api::{Flow, Host, HudBatch, Text};
use skills_core::area::area_of;
use skills_core::defs::{PerkDef, Reward};
use skills_core::progress::{
    PlayerState, Request, apply_under, earn_threshold, focus_cost, level, missing_skill, own_level,
    points_available, raise_cost, switched_off, total_points,
};
use skills_core::registry::Defs;
use skills_core::rules::{CostUnit, Rules};
use skills_core::wire::{ClientDefs, ToServer};

use super::cache::{CLICK_SOUND, ClientCache, Tab, with};
use super::layout::{Rect, Scroll};
use super::paint::{self, DIM, GOLD, GRAY, GREEN, Look, RED, WHITE};
use crate::MOD_ID;
use crate::text::{
    AREA_HIDDEN, AREA_SHOWN, AT_TOP, COST, COST_POINTS, FOCUS_ACTIVE, FOCUS_ADD, FOCUS_ADD_FREE,
    FOCUS_GIVES, LEVEL_OF, MASTERED, MISSING_PERK, NEEDS, NOT_ENOUGH_POINTS, OPEN_HINT, OWNED,
    PERK_COST, PERK_OF, PERK_POINTS, RAISE_BY, RAISE_CTRL, RAISE_OWN_BY, RAISE_OWN_TO, RAISE_SHIFT,
    RAISE_TO, REWARD_BLOCK_XP, REWARD_BREAK, REWARD_BREED, REWARD_KILL, REWARD_SMELT, REWARD_TAME,
    REWARD_TRAVEL, SHARED_OWN, SWITCHED_OFF, TIP_NEED, WAITING, focus_name, label_name, number,
    perk_description, perk_name, say, skill_name,
};

/// The mouse buttons the screen answers, by GLFW index.
pub const LEFT_BUTTON: i32 = 0;
pub const RIGHT_BUTTON: i32 = 1;
/// Raises one Shift-click asks for.
const SHIFT_RAISES: u32 = 5;
/// A perk's description wraps at this width in its tooltip.
const TIP_WIDTH: f32 = 190.0;

/// A perk as the screen shows it.
pub(super) enum PerkStatus {
    Owned,
    Buyable,
    Locked(Text),
}

/// The selected skill's perks, by the level they need, then id.
pub(super) fn perks_of<'a>(defs: &'a Defs, skill: &str) -> Vec<&'a PerkDef> {
    let mut perks: Vec<&PerkDef> = defs.perks.values().filter(|p| p.skill == skill).collect();
    perks.sort_by(|a, b| (a.min_level, &a.id).cmp(&(b.min_level, &b.id)));
    perks
}

pub(super) fn owns(state: &PlayerState, perk: &str) -> bool {
    state.skills.values().any(|p| p.perks.contains(perk))
}

pub(super) fn status(defs: &Defs, state: &PlayerState, perk: &PerkDef) -> PerkStatus {
    if owns(state, &perk.id) {
        return PerkStatus::Owned;
    }
    if own_level(state, defs, &perk.skill) < perk.min_level {
        let needs = [skill_name(defs, &perk.skill), number(perk.min_level)];
        return PerkStatus::Locked(say(&NEEDS, &needs));
    }
    if let Some((skill, needed)) = missing_skill(defs, state, perk) {
        return PerkStatus::Locked(say(&NEEDS, &[skill_name(defs, skill), number(*needed)]));
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
}

fn shown(defs: &ClientDefs, selected: usize) -> Shown<'_> {
    let skills: Vec<&String> = defs.defs.skills.keys().collect();
    let selected = selected.min(skills.len().saturating_sub(1));
    Shown { skills, selected }
}

/// Progress earned by doing toward the skill's next level, in actions: what is earned and what the level takes.
pub(super) fn earned_of(d: &Defs, state: &PlayerState, skill: &str) -> Option<(u32, u32)> {
    let need = earn_threshold(d, state, skill)?;
    let have = state.skills.get(skill).map_or(0, |p| p.earned);
    Some((have / HUNDREDTHS, need / HUNDREDTHS))
}

/// Earned progress is kept in hundredths of an action.
const HUNDREDTHS: u32 = 100;

pub(super) fn can_raise(
    d: &Defs,
    rules: &Rules,
    state: &PlayerState,
    xp: Option<u32>,
    skill: &str,
) -> bool {
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
pub(super) fn price(d: &Defs, rules: &Rules, state: &PlayerState, skill: &str) -> Option<u32> {
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
pub(super) struct Pen<'a> {
    pub(super) host: &'a dyn Host,
    pub(super) batch: &'a mut HudBatch,
    pub(super) mouse: [f32; 2],
    pub(super) d: &'a Defs,
    pub(super) state: &'a PlayerState,
    pub(super) xp: Option<u32>,
    /// The tooltip to draw last, over everything.
    pub(super) tip: Option<Vec<Text>>,
    /// The server's rules: whether perks may be switched, how raises are paid.
    pub(super) rules: &'a Rules,
    /// The perks whose area the player chose to see in the world.
    pub(super) shown_areas: &'a std::collections::BTreeSet<String>,
    /// The client's tick, and the tick the player's last stand is ready again.
    pub(super) now: u64,
    pub(super) last_stand_ready: Option<u64>,
}

impl Pen<'_> {
    pub(super) fn over(&self, r: &Rect) -> bool {
        r.contains(self.mouse[0], self.mouse[1])
    }

    pub(super) fn text(&mut self, text: &str, x: f32, y: f32, rgb: u32) {
        self.host.text(self.batch, text, [x, y], rgb, true);
    }

    pub(super) fn text_right(&mut self, text: &str, right: f32, y: f32, rgb: u32) {
        paint::text_right(self.host, self.batch, text, right, y, rgb);
    }

    pub(super) fn fill(&mut self, r: &Rect, argb: u32) {
        paint::fill(self.host, self.batch, r, argb);
    }

    pub(super) fn button(&mut self, r: &Rect, label: &str, look: Look) {
        let hover = self.over(r);
        paint::button(self.host, self.batch, r, label, look, hover);
    }

    pub(super) fn width(&self, text: &str) -> f32 {
        self.host.text_width(text)
    }

    pub(super) fn scroll(&mut self, scroll: Option<Scroll>) {
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
            cache.tab = Tab::Home;
            cache.first_row = 0;
        });
    }
}

/// The window as it stands: its tabs for the skills the client knows, scrolled to the cache's first tab.
fn window_now(size: [f32; 2], cache: &ClientCache) -> super::window::WindowLayout {
    let skills = cache.defs.as_ref().map_or(0, |d| d.defs.skills.len());
    super::window::window(size, skills, cache.first_tab)
}

/// Opens a tab: a skill's selects that skill, Options remembers the tab it was opened from.
fn open_tab(cache: &mut ClientCache, tab: Tab) {
    if tab == cache.tab {
        return;
    }
    match tab {
        Tab::Skill(i) => {
            cache.selected = i;
            cache.selected_perk = None;
            cache.road_scroll = 0.0;
        }
        Tab::Options => cache.return_tab = cache.tab,
        Tab::Home => cache.first_row = 0,
    }
    cache.tab = tab;
}

/// A click on the window's tabs or their scroll arrows; true when it was one.
fn click_window(size: [f32; 2], at: [f32; 2]) -> bool {
    with(|cache| {
        let w = window_now(size, cache);
        let in_arrow = |r: Option<Rect>| r.is_some_and(|r| r.contains(at[0], at[1]));
        let clicked = if in_arrow(w.scroll_left) {
            cache.first_tab = cache.first_tab.saturating_sub(1);
            true
        } else if in_arrow(w.scroll_right) {
            cache.first_tab += 1;
            true
        } else if let Some(tab) = super::window::hit_tab(&w, at[0], at[1]) {
            open_tab(cache, tab);
            true
        } else {
            return false;
        };
        if cache.settings.click_sounds {
            cache.sounds.push(CLICK_SOUND);
        }
        clicked
    })
}

/// Draws the screen when it is this mod's that is open.
pub fn draw(host: &dyn Host, batch: &mut HudBatch, size: [f32; 2], mouse: [f32; 2]) {
    if host.mod_screen() != Some(MOD_ID) {
        return;
    }
    host.fill(batch, [0.0, 0.0, size[0], size[1]], paint::SHADE);
    with(|cache| {
        cache.gui_size = size;
        let w = window_now(size, cache);
        super::window::draw_window(
            host,
            batch,
            &w,
            cache.tab,
            cache.defs.as_ref(),
            &cache.state,
        );
    });
    if with(|cache| cache.tab) == Tab::Options {
        super::options::draw(host, batch, size, mouse);
        return;
    }
    with(|cache| {
        let Some(defs) = &cache.defs else {
            let body = window_now(size, cache).body;
            paint::text_centred(host, batch, &say(&WAITING, &[]).plain(), &body, GRAY);
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
            rules: &cache.rules,
            shown_areas: &cache.settings.shown_areas,
            now: cache.ticks,
            last_stand_ready: cache.last_stand_ready,
        };
        let shown = shown(defs, cache.selected);
        match cache.tab {
            Tab::Home => {
                let body = window_now(size, cache).body;
                let groups = super::home::groups(&defs.defs);
                let l = super::home::home(&body, &groups, cache.first_row);
                super::home::draw(&mut pen, &l, &shown.skills, cache.my_name.as_deref());
            }
            Tab::Skill(_) => {
                if let Some(skill) = shown.skills.get(shown.selected) {
                    let body = window_now(size, cache).body;
                    let r =
                        super::road::road_of(defs, &cache.state, skill, &body, cache.road_scroll);
                    let chosen = cache.selected_perk.as_deref();
                    super::road::draw(&mut pen, defs, skill, &r, chosen);
                }
            }
            Tab::Options => {}
        }
        if let Some(lines) = pen.tip.take() {
            host.gui_tooltip(pen.batch, &lines, mouse);
        }
    });
}

/// A mastered skill's page: a gold plaque with the crown and "Mastered" where the raise button was.
pub(super) fn draw_mastered_plaque(pen: &mut Pen<'_>, r: &Rect) {
    paint::plaque(pen.host, pen.batch, r);
    let label = say(&MASTERED, &[]).plain();
    let width = paint::CROWN_SIZE[0] + 3.0 + pen.width(&label);
    let x = (r.x + (r.w - width) / 2.0).round();
    let y = (r.y + (r.h - paint::GLYPH) / 2.0).round();
    paint::crown(pen.host, pen.batch, [x, y]);
    pen.text(&label, x + paint::CROWN_SIZE[0] + 3.0, y, GOLD);
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
    Some((own_level(&outcome.state, d, skill), price))
}

pub(super) fn raise_tip(pen: &Pen<'_>, skill: &str) -> Vec<Text> {
    let (d, state) = (pen.d, pen.state);
    let Some(cost) = price(d, pen.rules, state, skill) else {
        return vec![say(&AT_TOP, &[]).with_color(GOLD)];
    };
    let now = own_level(state, d, skill);
    let lifted = now < level(state, d, skill);
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
            say(
                if lifted { &RAISE_OWN_BY } else { &RAISE_BY },
                &[number(reach), number(reach - now)],
            )
            .with_color(WHITE),
            cost_line(pen.rules, price).with_color(if paid { GREEN } else { RED }),
        ];
    }
    let colour = if can_raise(d, pen.rules, state, pen.xp, skill) {
        GREEN
    } else {
        RED
    };
    vec![
        say(
            if lifted { &RAISE_OWN_TO } else { &RAISE_TO },
            &[number(now + 1)],
        )
        .with_color(WHITE),
        cost_line(pen.rules, cost).with_color(colour),
        say(&RAISE_SHIFT, &[]).with_color(GRAY),
        say(&RAISE_CTRL, &[]).with_color(GRAY),
    ]
}

pub(super) fn skill_tip(pen: &Pen<'_>, skill: &str) -> Vec<Text> {
    let (d, state) = (pen.d, pen.state);
    let levels = [number(level(state, d, skill)), number(d.max_level(skill))];
    let points = points_available(d, state, skill);
    let own = own_level(state, d, skill);
    let shared =
        (own < level(state, d, skill)).then(|| say(&SHARED_OWN, &[number(own)]).with_color(GREEN));
    vec![
        skill_name(d, skill).with_color(GOLD),
        say(&LEVEL_OF, &levels).with_color(WHITE),
    ]
    .into_iter()
    .chain(shared)
    .chain([
        say(&PERK_POINTS, &[number(points)]).with_color(if points > 0 { GOLD } else { GRAY }),
        say(&OPEN_HINT, &[]).with_color(DIM),
    ])
    .collect()
}

/// A focus area's tooltip: its name, each task it gives XP for (once), and what a click does.
pub(super) fn focus_tip(pen: &Pen<'_>, focus: &str, active: bool) -> Vec<Text> {
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

/// A perk's tooltip: its name in bold and its skill; a bullet for each thing it does; then, after a gap, either
/// that it is owned or its cost and needs, each marked met or not.
pub(super) fn perk_tip(pen: &Pen<'_>, perk: &PerkDef, status: &PerkStatus) -> Vec<Text> {
    let (d, state) = (pen.d, pen.state);
    let mut lines = vec![
        perk_name(d, &perk.id).with_color(GOLD).bold(),
        say(&PERK_OF, &[skill_name(d, &perk.skill)]).with_color(SUBTLE),
    ];
    let does = description_lines(pen.host, &perk_description(perk).plain());
    if !does.is_empty() {
        lines.push(Text::literal(""));
        lines.extend(does);
    }
    lines.push(Text::literal(""));
    if matches!(status, PerkStatus::Owned) {
        lines.push(marked(true, say(&OWNED, &[])).with_color(GOLD));
        if switched_off(state, &perk.id) {
            lines.push(say(&SWITCHED_OFF, &[]).with_color(DIM));
        }
        if area_of(perk).is_some() {
            let line = if pen.shown_areas.contains(&perk.id) {
                &AREA_SHOWN
            } else {
                &AREA_HIDDEN
            };
            lines.push(say(line, &[]).with_color(GRAY));
        }
        return lines;
    }
    let enough = points_available(d, state, &perk.skill) >= perk.cost;
    let cost =
        Text::literal(format!("{} ", paint::POINT)).append(say(&PERK_COST, &[number(perk.cost)]));
    lines.push(cost.with_color(met_colour(enough)));
    let mut need = |reached: bool, what: Text| {
        lines.push(marked(reached, what).with_color(met_colour(reached)))
    };
    if perk.min_level > 1 {
        let reached = own_level(state, d, &perk.skill) >= perk.min_level;
        need(
            reached,
            say(
                &TIP_NEED,
                &[skill_name(d, &perk.skill), number(perk.min_level)],
            ),
        );
    }
    for (skill, needed) in &perk.also_needs {
        let reached = own_level(state, d, skill) >= *needed;
        need(
            reached,
            say(&TIP_NEED, &[skill_name(d, skill), number(*needed)]),
        );
    }
    for other in &perk.requires {
        need(owns(state, other), perk_name(d, other));
    }
    lines
}

pub(super) fn met_colour(met: bool) -> u32 {
    if met { GREEN } else { RED }
}

/// The line with a tick before it when met, a cross when not.
pub(super) fn marked(met: bool, what: Text) -> Text {
    let mark = if met { paint::TICK } else { CROSS };
    Text::literal(format!("{mark} ")).append(what)
}

/// A perk tooltip's second line: quieter than the description, still easy to read.
const SUBTLE: u32 = 0x8C_92A0;
const CROSS: &str = "\u{2718}";
const BULLET: &str = "\u{2022} ";
/// Lines after a bullet's first start under its words.
const BULLET_INDENT: &str = "  ";

/// A perk's description for its tooltip: a gold bullet for each sentence, its wrapped lines indented under it, and
/// its numbers in white among gray words.
fn description_lines(host: &dyn Host, text: &str) -> Vec<Text> {
    let room = TIP_WIDTH - host.text_width(BULLET);
    let mut lines = Vec::new();
    for sentence in sentences(text) {
        for (at, line) in paint::wrapped(host, &sentence, room)
            .into_iter()
            .enumerate()
        {
            let lead = if at == 0 {
                Text::literal(BULLET).with_color(GOLD)
            } else {
                Text::literal(BULLET_INDENT)
            };
            lines.push(picked_out(lead, &line));
        }
    }
    lines
}

/// The text's sentences, each keeping its full stop.
pub(super) fn sentences(text: &str) -> Vec<String> {
    let parts: Vec<&str> = text.trim().split(". ").filter(|p| !p.is_empty()).collect();
    let last = parts.len().saturating_sub(1);
    parts
        .iter()
        .enumerate()
        .map(|(at, part)| {
            if at < last {
                format!("{part}.")
            } else {
                (*part).to_string()
            }
        })
        .collect()
}

/// `lead` followed by the words, those with a digit in white and the rest in gray.
fn picked_out(lead: Text, words: &str) -> Text {
    words.split(' ').enumerate().fold(lead, |line, (at, word)| {
        let gap = if at == 0 { "" } else { " " };
        let colour = if word.chars().any(|c| c.is_ascii_digit()) {
            WHITE
        } else {
            GRAY
        };
        line.append(Text::literal(format!("{gap}{word}")).with_color(colour))
    })
}

/// What a click on the screen leads to.
pub(super) enum Outcome {
    Nothing,
    Ask(Request),
    Open(usize),
    Back,
    /// Show the owned perk's area in the world, or stop.
    ToggleArea(String),
    /// Tell about this perk in the skill page's dock.
    Choose(String),
}

/// A click on the screen: opens a skill, goes back, or queues a raise, perk or focus request the player can pay
/// for. Every click on this mod's screen is taken.
pub fn click(host: &dyn Host, size: [f32; 2], at: [f32; 2], button: i32) -> Flow {
    if host.mod_screen() != Some(MOD_ID) {
        return Flow::Continue;
    }
    if with(|cache| cache.tab) == Tab::Options {
        if button == LEFT_BUTTON && click_window(size, at) {
            return Flow::Cancel;
        }
        return super::options::click(host, size, at, button);
    }
    let clicked = with(|cache| {
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
                cache.tab = Tab::Skill(cache.selected);
            }
            Outcome::Back => cache.tab = Tab::Home,
            Outcome::Choose(perk) => cache.selected_perk = Some(perk),
            Outcome::ToggleArea(perk) => {
                let shown = &mut cache.settings.shown_areas;
                if !shown.remove(&perk) {
                    shown.insert(perk);
                }
                cache.settings.save(host);
            }
        }
        if clicked && cache.settings.click_sounds {
            cache.sounds.push(CLICK_SOUND);
        }
        clicked
    });
    if !clicked && button == LEFT_BUTTON {
        click_window(size, at);
    }
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
    let shown = shown(defs, cache.selected);
    match (cache.tab, button) {
        (Tab::Skill(_), RIGHT_BUTTON) => Outcome::Back,
        (Tab::Home, LEFT_BUTTON) => {
            let body = window_now(size, cache).body;
            let groups = super::home::groups(&defs.defs);
            let l = super::home::home(&body, &groups, cache.first_row);
            super::home::click(cache, &defs.defs, &l, &shown.skills, at, times)
        }
        (Tab::Skill(_), LEFT_BUTTON) => {
            let Some(skill) = shown.skills.get(shown.selected) else {
                return Outcome::Nothing;
            };
            let body = window_now(size, cache).body;
            let r = super::road::road_of(defs, &cache.state, skill, &body, cache.road_scroll);
            super::road::click(cache, defs, skill, &r, at, times)
        }
        _ => Outcome::Nothing,
    }
}

/// The mouse wheel on the screen: moves the cards or the perk rows when there are more than fit.
pub fn scroll(host: &dyn Host, amount: f64) -> Flow {
    if host.mod_screen() != Some(MOD_ID) {
        return Flow::Continue;
    }
    with(|cache| {
        let Some(defs) = &cache.defs else { return };
        let shown = shown(defs, cache.selected);
        let stepped = |now: usize, last: usize| {
            if amount > 0.0 {
                now.min(last).saturating_sub(1)
            } else if amount < 0.0 {
                (now + 1).min(last)
            } else {
                now
            }
        };
        let size = if cache.gui_size[0] > 0.0 {
            cache.gui_size
        } else {
            [427.0, 240.0]
        };
        match cache.tab {
            Tab::Home => {
                let body = window_now(size, cache).body;
                let last = super::home::last_first(&body, &super::home::groups(&defs.defs));
                cache.first_row = stepped(cache.first_row, last);
            }
            Tab::Skill(_) => {
                if let Some(skill) = shown.skills.get(shown.selected) {
                    let body = window_now(size, cache).body;
                    let r =
                        super::road::road_of(defs, &cache.state, skill, &body, cache.road_scroll);
                    cache.road_scroll = super::road::scrolled(&r, cache.road_scroll, amount);
                }
            }
            Tab::Options => super::options::scroll(cache, size, amount),
        }
    });
    Flow::Cancel
}

#[cfg(test)]
pub(crate) mod tests {
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

    pub(crate) fn skill(id: &str) -> SkillDef {
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
            earned_by: Vec::new(),
            color: None,
        }
    }

    pub(crate) fn perk(path: &str, min_level: u32, requires: &[&str]) -> PerkDef {
        PerkDef {
            id: format!("reskilled:{path}"),
            priority: 0,
            skill: MINING.into(),
            name: format!("reskilled.perk.{path}"),
            description: format!("reskilled.perk.{path}.desc"),
            cost: 1,
            min_level,
            requires: requires.iter().map(|r| format!("reskilled:{r}")).collect(),
            also_needs: Default::default(),
            icon: None,
            effects: vec![],
        }
    }

    pub(crate) fn focus(id: &str, skills: &[&str]) -> (String, FocusDef) {
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
    /// levels, on Home.
    pub(crate) fn screen_open() -> TestHost {
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
            cache.defs = Some(ClientDefs {
                defs,
                labels,
                label_icons: Default::default(),
            });
            cache.state = state;
            cache.xp_levels = Some(23);
        });
        TestHost {
            open_screen: Some(MOD_ID.into()),
            ..TestHost::default()
        }
    }

    /// The screen open on Mining's own view (skills are listed by id: Melee, then Mining).
    pub(crate) fn mining_open() -> TestHost {
        let host = screen_open();
        with(|cache| {
            cache.selected = 1;
            cache.tab = Tab::Skill(cache.selected);
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

    /// A skill's row on Home and its raise button.
    #[derive(Debug, Clone, Copy)]
    struct Row {
        rect: Rect,
        raise: Rect,
    }

    fn home_now() -> super::super::home::HomeLayout {
        with(|cache| {
            let d = &cache.defs.as_ref().unwrap().defs;
            let body = super::super::window::window(SIZE, d.skills.len(), 0).body;
            super::super::home::home(&body, &super::super::home::groups(d), 0)
        })
    }

    /// Each skill's row on Home, by the skill's place in id order.
    fn rows() -> Vec<Row> {
        let l = home_now();
        let mut rows: Vec<(usize, Row)> = l
            .rows
            .iter()
            .map(|(i, rect, raise)| {
                (
                    *i,
                    Row {
                        rect: *rect,
                        raise: *raise,
                    },
                )
            })
            .collect();
        rows.sort_by_key(|(i, _)| *i);
        rows.into_iter().map(|(_, row)| row).collect()
    }

    /// The focus areas' buttons on Home, in id order (an active area's spot takes no click).
    fn focus_buttons() -> Vec<Rect> {
        home_now()
            .headings
            .iter()
            .filter_map(|(.., b)| *b)
            .collect()
    }

    /// Mining's road at the usual size.
    fn mining_road() -> super::super::road::RoadLayout {
        with(|cache| {
            let defs = cache.defs.as_ref().unwrap();
            let body = super::super::window::window(SIZE, defs.defs.skills.len(), 0).body;
            super::super::road::road_of(defs, &cache.state, MINING, &body, 0.0)
        })
    }

    /// A Mining perk's node on its road, by the end of its id.
    fn node_of(perk: &str) -> Rect {
        mining_road()
            .nodes
            .iter()
            .find(|(id, _)| id.ends_with(perk))
            .unwrap()
            .1
    }

    /// Whether something drawn at height `y` is in the tabs' row above the frame, which the old views' tests
    /// leave out until those views go.
    pub(crate) fn in_tab_row(y: f32) -> bool {
        y < super::super::window::window(SIZE, 0, 0).frame.y
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
                .filter(|(_, at)| !in_tab_row(at[1]))
                .map(|(item, _)| item.clone())
                .filter(|i| i.contains("pickaxe"))
                .collect()
        };

        // When Home is drawn, then Mining's own view, then Home again at level 16
        drawn(&host, AWAY);
        let row = shown(&host);
        host.icons.borrow_mut().clear();
        with(|cache| {
            cache.selected = 1;
            cache.tab = Tab::Skill(cache.selected);
        });
        drawn(&host, AWAY);
        let view = shown(&host);
        host.icons.borrow_mut().clear();
        with(|cache| {
            cache.tab = Tab::Home;
            cache.state.skills.get_mut(MINING).unwrap().level = 16;
        });
        drawn(&host, AWAY);

        // Then the row and the view show the iron pickaxe, and the diamond one once Mining reaches 16
        assert!(
            row.contains(&"minecraft:iron_pickaxe".to_string()),
            "{row:?}"
        );
        assert_eq!(
            view.first().map(String::as_str),
            Some("minecraft:iron_pickaxe")
        );
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
    fn a_mastered_skills_page_says_so() {
        // Given Melee mastered
        let _serial = exclusive();
        let host = screen_open();
        melee_mastered();
        with(|cache| cache.tab = Tab::Skill(cache.selected));

        // When its page is drawn
        let drawn = drawn(&host, AWAY);

        // Then it says it is mastered
        assert!(drawn.iter().any(|t| t == "Mastered"), "{drawn:?}");
    }

    #[test]
    fn a_row_opens_its_skills_road() {
        // Given the screen open on Home
        let _serial = exclusive();
        let host = screen_open();

        // When Mining's row is clicked and the screen is drawn
        let flow = click(&host, SIZE, inside(&rows()[1].rect), LEFT_BUTTON);
        let drawn = drawn(&host, AWAY);

        // Then Mining's road is up: its level, its points, its raise, and the dock telling of the next perk
        assert_eq!(flow, Flow::Cancel);
        assert_eq!(
            with(|cache| (cache.tab, cache.selected)),
            (Tab::Skill(1), 1)
        );
        contains(
            &drawn,
            &[
                "MINING",
                "Level 12 of 32",
                "\u{25c6} 5 perk points",
                "Level up \u{b7} 12 XP",
                "Mining rich seams",
                "Unlock",
            ],
        );
    }

    #[test]
    fn raise_buttons_show_a_drawn_arrow_beside_the_cost() {
        // Given Home
        let _serial = exclusive();
        let host = screen_open();
        let raise = rows()[1].raise;

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
    fn a_description_is_a_bullet_per_sentence_with_its_numbers_picked_out() {
        // Given a description of two sentences, the first too long for one line
        let host = TestHost::default();
        let text =
            "+3% melee damage per enchantment on your weapon, up to 15% in all. -30% fall damage.";

        // When it is laid out for a perk's tooltip
        let lines = description_lines(&host, text);

        // Then each sentence starts a bullet, a wrapped line is indented under it, and the numbers stand out in white
        let plain: Vec<String> = lines.iter().map(Text::plain).collect();
        let last = plain.len() - 1;
        assert!(last >= 2, "{plain:?}");
        assert!(plain[0].starts_with("\u{2022} +3% melee"), "{plain:?}");
        assert!(
            plain[1..last].iter().all(|l| l.starts_with("  ")),
            "{plain:?}"
        );
        assert!(
            plain[1..last].iter().any(|l| l.contains("15%")),
            "{plain:?}"
        );
        assert_eq!(plain[last], "\u{2022} -30% fall damage.");
        let colour_of = |line: &Text, word: &str| {
            crate::test_host::spans_of(line)
                .into_iter()
                .find(|(t, _)| t.contains(word))
                .and_then(|(_, c)| c)
        };
        assert_eq!(colour_of(&lines[last], "-30%"), Some(WHITE));
        assert_eq!(colour_of(&lines[last], "fall"), Some(GRAY));
        assert_eq!(colour_of(&lines[0], "\u{2022}"), Some(GOLD));
    }

    #[test]
    fn hovering_a_perk_shows_what_it_costs_and_needs() {
        // Given Mining's view, the mouse over its third perk by level (Deep Veins, level 16)
        let _serial = exclusive();
        let host = mining_open();
        let deep = node_of("deep_veins");

        // When the screen is drawn
        drawn(&host, inside(&deep));

        // Then one tooltip names it and its skill, then, after a gap, its cost and the level it needs, each marked
        // met or not
        assert_eq!(
            tooltip(&host),
            vec![
                "Mining deep veins",
                "Mining perk",
                "",
                "\u{25C6} 1 perk points",
                "\u{2718} Mining 16"
            ]
        );
    }

    #[test]
    fn a_perk_lists_the_other_skills_it_needs() {
        // Given Mining's view, its third perk (Deep Veins) also needing Melee 20, the player at Melee 1
        let _serial = exclusive();
        let host = mining_open();
        with(|cache| {
            let perks = &mut cache.defs.as_mut().unwrap().defs.perks;
            let deep = perks.values_mut().find(|p| p.min_level == 16).unwrap();
            deep.also_needs = [(MELEE.to_string(), 20)].into();
        });
        let deep = node_of("deep_veins");

        // When the mouse is over it
        drawn(&host, inside(&deep));

        // Then its tooltip also says it needs Melee 20
        assert_eq!(
            tooltip(&host).last().map(String::as_str),
            Some("\u{2718} Melee 20")
        );
    }

    #[test]
    fn hovering_a_raise_button_shows_the_level_and_cost() {
        // Given Home, the mouse over Mining's raise button
        let _serial = exclusive();
        let host = screen_open();

        // When the screen is drawn
        drawn(&host, inside(&rows()[1].raise));

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
    fn a_lifted_skill_raises_and_names_the_own_level() {
        // Given Mining shown at 12 through a party, the player's own at 4 (a raise from 4 costs 4)
        let _serial = exclusive();
        let host = screen_open();
        with(|cache| cache.state.skills.get_mut(MINING).unwrap().own = Some(4));

        // When the mouse is over Mining's raise button, then over its row
        drawn(&host, inside(&rows()[1].raise));
        let raise = tooltip(&host);
        host.painted.borrow_mut().clear();
        drawn(&host, inside(&rows()[1].rect));
        let row = tooltip(&host);

        // Then the raise is from the own level at its price, and the row says the 12 is shared
        assert_eq!(
            raise[..2],
            ["Raise your own level to 5", "Cost: 4 XP levels"]
        );
        assert!(
            row.contains(&"Shared level. Yours: 4".to_string()),
            "{row:?}"
        );
    }

    #[test]
    fn holding_shift_shows_where_five_raises_lead_and_their_price() {
        // Given Mining at 12 (raises cost 12, 13, 14, 15, 16) and 23 XP levels, the mouse on its raise button
        let _serial = exclusive();
        let mut host = screen_open();
        host.shift = true;

        // When the screen is drawn with Shift held
        drawn(&host, inside(&rows()[1].raise));

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
        drawn(&host, inside(&rows()[1].raise));

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
        // Given Home, the player focused on Working; Fighting gives more XP from kills and some for
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
        let buttons = focus_buttons();

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
                "Click to add: 20 XP levels."
            ]
        );
        assert_eq!(tooltip(&host), vec!["Working", "Gives XP for:", "Active."]);
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
        let flow = click(&host, SIZE, [SIZE[0] / 2.0, SIZE[1] / 2.0], LEFT_BUTTON);

        // Then it says it is waiting, and the click is taken without doing anything
        contains(&drawn, &["Waiting for the server..."]);
        assert_eq!(flow, Flow::Cancel);
        assert_eq!(with(|cache| cache.tab), Tab::Home);
    }

    #[test]
    fn raise_buttons_send_a_raise_request() {
        // Given the screen open
        let _serial = exclusive();
        let mut host = screen_open();

        // When Mining's raise button is clicked on its card, then on its own view, then a tick passes
        let flow = click(&host, SIZE, inside(&rows()[1].raise), LEFT_BUTTON);
        let still = with(|cache| cache.tab);
        click(&host, SIZE, inside(&rows()[1].rect), LEFT_BUTTON);
        click(&host, SIZE, inside(&mining_road().raise), LEFT_BUTTON);
        let sent = requests_sent(&mut host);

        // Then both clicks are taken and ask the server, and the card's button does not open the skill
        let raise = ToServer::Act(Request::Raise {
            skill: MINING.into(),
            times: 1,
        });
        assert_eq!(flow, Flow::Cancel);
        assert_eq!(still, Tab::Home);
        assert_eq!(sent, vec![raise.clone(), raise]);
    }

    #[test]
    fn shift_raises_five_and_control_raises_all_that_can_be_paid() {
        // Given the screen open
        let _serial = exclusive();
        let mut host = screen_open();

        // When Mining's raise is clicked with Shift held, then with Control held
        host.shift = true;
        click(&host, SIZE, inside(&rows()[1].raise), LEFT_BUTTON);
        host.shift = false;
        host.control = true;
        click(&host, SIZE, inside(&rows()[1].raise), LEFT_BUTTON);
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
        drawn(&host, inside(&rows()[1].raise));

        // Then its tooltip ends with the hint
        assert_eq!(
            tooltip(&host)[2..],
            ["Shift: +5 levels", "Ctrl: all you can pay for"]
        );
    }

    #[test]
    fn clicks_are_silent_when_turned_off() {
        // Given the screen open with clicks turned off
        let _serial = exclusive();
        let mut host = screen_open();
        host.eye = Some([3.0, 70.5, -8.0]);
        with(|cache| cache.settings.click_sounds = false);

        // When Mining's row is clicked and a tick passes
        click(&host, SIZE, inside(&rows()[1].rect), LEFT_BUTTON);
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

        // When bare window is clicked and a tick passes, then Mining's row and another tick
        let body = super::super::window::window(SIZE, 2, 0).body;
        click(
            &host,
            SIZE,
            [body.x + 2.0, body.y + body.h - 2.0],
            LEFT_BUTTON,
        );
        horizon_client::mod_api::dispatch_tick(&mut host, &mods());
        let after_nothing = host.sounds.len();
        click(&host, SIZE, inside(&rows()[1].rect), LEFT_BUTTON);
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

        // When Home is drawn, then Mining's page with the mouse on its raise
        let home = drawn(&host, AWAY);
        host.drawn.borrow_mut().clear();
        with(|cache| {
            cache.selected = 1;
            cache.tab = Tab::Skill(cache.selected);
        });
        let page = drawn(&host, inside(&mining_road().raise));

        // Then Mining's row asks 240, its page's raise too, and its tip says the cost is 240 XP points
        contains(&home, &["240"]);
        contains(&page, &["Level up \u{b7} 240 XP"]);
        assert!(tooltip(&host).iter().any(|t| t == "Cost: 240 XP points"));
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

        // When Home is drawn, then Mining's page, and Mining's row is clicked where its raise was
        let home = drawn(&host, AWAY);
        click(&host, SIZE, inside(&rows()[1].raise), LEFT_BUTTON);
        host.drawn.borrow_mut().clear();
        with(|cache| {
            cache.selected = 1;
            cache.tab = Tab::Skill(cache.selected);
        });
        let page = drawn(&host, AWAY);

        // Then the row shows 50/120 and no price, the page shows the progress and no level-up button, and nothing
        // is bought
        contains(&home, &["50/120"]);
        assert!(!home.iter().any(|t| t == "12"), "{home:?}");
        contains(&page, &["Progress: 50/120"]);
        assert!(!page.iter().any(|t| t.starts_with("Level up")), "{page:?}");
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
        contains(&page, &["Level up \u{b7} 12 XP", "50/120"]);
    }

    #[test]
    fn a_raise_the_player_cannot_pay_for_is_not_asked() {
        // Given the screen open and only 2 XP levels, when Mining's raise costs 12
        let _serial = exclusive();
        let mut host = screen_open();
        with(|cache| cache.xp_levels = Some(2));

        // When Mining's raise button is clicked, and Melee's, which costs 2
        click(&host, SIZE, inside(&rows()[1].raise), LEFT_BUTTON);
        click(&host, SIZE, inside(&rows()[0].raise), LEFT_BUTTON);

        // Then only the one they can pay for is asked
        let melee = ToServer::Act(Request::Raise {
            skill: MELEE.into(),
            times: 1,
        });
        assert_eq!(requests_sent(&mut host), vec![melee]);
    }

    #[test]
    fn a_focus_button_unlocks_an_area_and_an_unlocked_one_stays() {
        // Given Home, the player focused on Working
        let _serial = exclusive();
        let mut host = screen_open();
        host.eye = Some([0.0, 64.0, 0.0]);
        let buttons = focus_buttons();

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
        // Given Home and 19 XP levels, when a second area costs 20
        let _serial = exclusive();
        let mut host = screen_open();
        with(|cache| cache.xp_levels = Some(19));
        let fighting = focus_buttons()[0];

        // When Fighting is clicked
        click(&host, SIZE, inside(&fighting), LEFT_BUTTON);

        // Then nothing is asked
        assert!(requests_sent(&mut host).is_empty());
    }

    #[test]
    fn a_right_click_returns_home() {
        // Given Mining's road
        let _serial = exclusive();
        let host = mining_open();

        // When the right button is pressed anywhere
        click(&host, SIZE, AWAY, RIGHT_BUTTON);

        // Then Home is open
        assert_eq!(with(|cache| cache.tab), Tab::Home);
    }

    pub(crate) fn sent_acts(host: &mut TestHost) -> Vec<Request> {
        requests_sent(host)
            .into_iter()
            .filter_map(|m| match m {
                ToServer::Act(request) => Some(request),
                _ => None,
            })
            .collect()
    }

    /// Where the tab for `tab` is, at the usual size.
    fn tab_at(tab: Tab) -> [f32; 2] {
        let skills = with(|cache| cache.defs.as_ref().map_or(0, |d| d.defs.skills.len()));
        let w = super::super::window::window(SIZE, skills, 0);
        let rect = w.tabs.iter().find(|(t, _)| *t == tab).unwrap().1;
        [rect.x + 4.0, rect.y + 6.0]
    }

    #[test]
    fn clicking_a_tab_opens_it() {
        // Given the screen open on Home
        let _serial = exclusive();
        let host = screen_open();

        // When Mining's tab (the second skill) is clicked, then Options
        click(&host, SIZE, tab_at(Tab::Skill(1)), LEFT_BUTTON);
        let after_skill = with(|cache| (cache.tab, cache.selected));
        click(&host, SIZE, tab_at(Tab::Options), LEFT_BUTTON);

        // Then Mining's tab opens, then Options, which remembers the tab before it
        assert_eq!(after_skill, (Tab::Skill(1), 1));
        assert_eq!(
            with(|cache| (cache.tab, cache.return_tab)),
            (Tab::Options, Tab::Skill(1))
        );
    }

    #[test]
    fn the_tabs_are_drawn_with_a_pip_for_points() {
        // Given Mining with points to spend and Melee with none
        let _serial = exclusive();
        let host = screen_open();

        // When the screen is drawn
        drawn(&host, AWAY);

        // Then each skill tab shows its icon, and a gold pip marks Mining's tab only
        let icons = host.icons.borrow().clone();
        assert!(
            icons
                .iter()
                .any(|(item, _)| item == "minecraft:iron_pickaxe")
        );
        let pips = host
            .painted
            .borrow()
            .iter()
            .filter(|p| matches!(p, Paint::Fill { argb, .. } if *argb == paint::PIP))
            .count();
        assert_eq!(pips, 1);
    }

    #[test]
    fn before_the_definitions_every_tab_waits() {
        // Given the screen open before the server sent the definitions
        let _serial = exclusive();
        let mut host = TestHost {
            open_screen: Some(MOD_ID.into()),
            ..TestHost::default()
        };
        with(|cache| cache.defs = None);

        // When it is drawn on Home and on Options, and the body is clicked
        let home = drawn(&host, AWAY);
        with(|cache| cache.tab = Tab::Options);
        let options = drawn(&host, AWAY);
        click(&host, SIZE, [SIZE[0] / 2.0, SIZE[1] / 2.0], LEFT_BUTTON);

        // Then both wait, and nothing is asked
        assert!(
            home.iter().any(|t| t == "Waiting for the server..."),
            "{home:?}"
        );
        assert!(
            options.iter().any(|t| t == "Waiting for the server..."),
            "{options:?}"
        );
        assert!(sent_acts(&mut host).is_empty());
    }

    #[test]
    fn an_owned_perk_has_a_switch_and_no_refund() {
        // Given Mining's road, Sure Grip owned and chosen for the dock
        let _serial = exclusive();
        let mut host = mining_open();
        click(&host, SIZE, inside(&node_of("sure_grip")), LEFT_BUTTON);

        // When the page is drawn and the dock's switch is clicked
        let drawn = drawn(&host, AWAY);
        click(&host, SIZE, inside(&mining_road().dock_button), LEFT_BUTTON);

        // Then the dock shows "On" and nothing else to click, and the switch is asked for
        contains(&drawn, &["On"]);
        assert!(!drawn.iter().any(|t| t == "Refund" || t == "Area"));
        assert_eq!(
            sent_acts(&mut host),
            vec![Request::SwitchPerk {
                perk: "reskilled:mining_sure_grip".into()
            }]
        );
    }

    #[test]
    fn an_owned_perk_with_an_area_shows_it_in_the_world_at_a_click() {
        // Given Mining's view, Sure Grip owned and given a 24-block area around home
        let _serial = exclusive();
        let mut host = mining_open();
        with(|cache| {
            let defs = cache.defs.as_mut().unwrap();
            let grip = defs
                .defs
                .perks
                .get_mut("reskilled:mining_sure_grip")
                .unwrap();
            grip.effects = vec![skills_core::defs::Effect::HomeWard { radius: 24 }];
        });
        let (grip, area) = (node_of("sure_grip"), mining_road().dock_area);
        click(&host, SIZE, inside(&grip), LEFT_BUTTON);

        // When the page is drawn, the dock's area button is clicked, the page is drawn again, and the button is
        // clicked once more
        let before = drawn(&host, inside(&grip));
        let tip_before = tooltip(&host);
        click(&host, SIZE, inside(&area), LEFT_BUTTON);
        let shown = with(|cache| cache.settings.shown_areas.clone());
        let tip_after = {
            host.painted.borrow_mut().clear();
            drawn(&host, inside(&grip));
            tooltip(&host)
        };
        click(&host, SIZE, inside(&area), LEFT_BUTTON);

        // Then the dock has an Area button; the first click chooses to see the area (the tooltip says so) and the
        // second stops; nothing is asked of the server
        contains(&before, &["Area", "On"]);
        assert!(
            tip_before
                .iter()
                .any(|t| t == "Click Area to see it in the world."),
            "{tip_before:?}"
        );
        assert_eq!(shown, ["reskilled:mining_sure_grip".to_string()].into());
        assert!(
            tip_after.iter().any(|t| t == "Area shown in the world."),
            "{tip_after:?}"
        );
        assert!(with(|cache| cache.settings.shown_areas.is_empty()));
        assert!(sent_acts(&mut host).is_empty());
    }

    #[test]
    fn a_switched_off_perk_reads_off() {
        // Given Mining's road, Sure Grip owned, switched off and chosen for the dock
        let _serial = exclusive();
        let host = mining_open();
        with(|cache| {
            let mining = cache.state.skills.get_mut(MINING).unwrap();
            mining.disabled.insert("reskilled:mining_sure_grip".into());
        });
        click(&host, SIZE, inside(&node_of("sure_grip")), LEFT_BUTTON);

        // When it is drawn
        let drawn = drawn(&host, AWAY);

        // Then its switch reads "Off"
        contains(&drawn, &["Off"]);
    }

    #[test]
    fn the_rules_take_the_switch_away() {
        // Given Mining's view on a server with perk switches off
        let _serial = exclusive();
        let mut host = mining_open();
        with(|cache| cache.rules.perk_switches = false);
        click(&host, SIZE, inside(&node_of("sure_grip")), LEFT_BUTTON);
        let r = mining_road();

        // When it is drawn and the switch's spot and the area's are clicked
        let drawn = drawn(&host, AWAY);
        click(&host, SIZE, inside(&r.dock_button), LEFT_BUTTON);
        click(&host, SIZE, inside(&r.dock_area), LEFT_BUTTON);

        // Then the dock just says it is owned, and nothing is asked or chosen
        contains(&drawn, &["\u{2714} Owned"]);
        assert!(sent_acts(&mut host).is_empty());
        assert!(with(|cache| cache.settings.shown_areas.is_empty()));
    }

    #[test]
    fn clicks_and_the_wheel_pass_through_when_the_screen_is_not_ours() {
        // Given no screen of this mod's open
        let _serial = exclusive();
        let mut host = screen_open();
        host.open_screen = None;

        // When a raise button's spot is clicked and the wheel turns
        let flow = click(&host, SIZE, inside(&rows()[0].raise), LEFT_BUTTON);
        let wheel = scroll(&host, -1.0);

        // Then both are left to others and nothing is sent
        assert_eq!((flow, wheel), (Flow::Continue, Flow::Continue));
        assert!(requests_sent(&mut host).is_empty());
    }
}
