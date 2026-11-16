//! A player's skills, perks and focus, what raising and buying cost, and the requests that change them.
//!
//! Every function here is pure: a request yields a new state and the XP levels to take, or a refusal, and the
//! caller applies it. Saved entries for skills or perks a pack removed are kept as they are and ignored, so adding
//! them back restores them.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::defs::{PerkDef, Reward};
use crate::registry::Defs;
use crate::rules::{CostUnit, Respec, Rules};

/// The save format of [`PlayerState`]: 2 keeps several focus areas.
pub const STATE_FORMAT: u32 = 2;
const MIN_LEVEL: u32 = 1;
const MIN_COST: u32 = 1;

/// Everything Reskilled keeps for one player.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub format: u32,
    /// Skill id to progress. Absent means level 1, nothing bought.
    pub skills: BTreeMap<String, SkillProgress>,
    /// The active focus areas.
    pub focus: BTreeSet<String>,
    /// Whether a focus area was ever added, so only the first is free.
    pub focus_chosen_before: bool,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            format: STATE_FORMAT,
            skills: BTreeMap::new(),
            focus: BTreeSet::new(),
            focus_chosen_before: false,
        }
    }
}

/// One skill's progress.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillProgress {
    pub level: u32,
    pub points_spent: u32,
    /// Points an operator granted on top of the ones levels earn.
    #[serde(default)]
    pub bonus_points: u32,
    pub perks: BTreeSet<String>,
    /// Owned perks the player switched off: still theirs, not working.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub disabled: BTreeSet<String>,
    /// Progress earned by doing the skill's work toward its next level, in hundredths of an action.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub earned: u32,
    /// The level the player reached themselves, while a group they share skills with lifts `level` above it. What
    /// they pay, earn and buy perks with; `level` (what locks check) is this one again when the lift ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own: Option<u32>,
}

impl SkillProgress {
    /// Sets the level the player reached themselves: the shown level follows, unless a lift keeps it higher.
    pub fn set_own(&mut self, own: u32) {
        match self.own {
            Some(_) if own < self.level => self.own = Some(own),
            _ => {
                self.level = own;
                self.own = None;
            }
        }
    }
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn one() -> u32 {
    1
}

fn is_one(n: &u32) -> bool {
    *n == 1
}

/// What a player asks the server to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Raises the skill `times` times in a row (1 when not said), or as many as the player can pay for up to its
    /// top level when that is more. Only the first may be refused; the rest stop where the XP or the skill runs out.
    Raise {
        skill: String,
        #[serde(default = "one", skip_serializing_if = "is_one")]
        times: u32,
    },
    BuyPerk {
        perk: String,
    },
    /// Unlocks a focus area for good.
    AddFocus {
        focus: String,
    },
    /// Switches an owned perk's effect off, or back on.
    SwitchPerk {
        perk: String,
    },
    /// Gives an owned perk back for its points.
    RefundPerk {
        perk: String,
    },
}

/// Why a request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    UnknownSkill,
    UnknownPerk,
    UnknownFocus,
    AtMax,
    NotEnoughLevels {
        needed: u32,
    },
    NotEnoughXpPoints {
        needed: u32,
    },
    NotEnoughPoints {
        needed: u32,
    },
    BelowMinLevel {
        needed: u32,
    },
    /// The perk also needs this level of the player's own in another skill.
    BelowSkillLevel {
        skill: String,
        needed: u32,
    },
    MissingPerk {
        perk: String,
    },
    AlreadyOwned,
    AlreadyFocused,
    NotOwned,
    BuyingOff,
    SwitchesOff,
    RefundsOff,
    /// Another owned perk needs this one.
    NeededBy {
        perk: String,
    },
}

/// A granted request: the new state and the XP levels to take for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub state: PlayerState,
    pub levels_spent: u32,
    /// XP points to take, under the rule that raises are paid in points.
    pub points_spent: u32,
}

/// The skill's level: 1 when never raised, and never above the skill's current maximum.
pub fn level(state: &PlayerState, defs: &Defs, skill: &str) -> u32 {
    let saved = state
        .skills
        .get(skill)
        .map_or(MIN_LEVEL, |progress| progress.level);
    saved.clamp(MIN_LEVEL, defs.max_level(skill).max(MIN_LEVEL))
}

/// The skill's level the player reached themselves, below a level a group lifts them to; as [`level`] otherwise.
pub fn own_level(state: &PlayerState, defs: &Defs, skill: &str) -> u32 {
    let saved = state
        .skills
        .get(skill)
        .map_or(MIN_LEVEL, |progress| progress.own.unwrap_or(progress.level));
    saved.clamp(MIN_LEVEL, defs.max_level(skill).max(MIN_LEVEL))
}

/// XP levels the next level of the skill costs, at the player's own level; `None` at the maximum or for an unknown
/// skill.
pub fn raise_cost(defs: &Defs, state: &PlayerState, skill: &str) -> Option<u32> {
    defs.skill(skill)?;
    let current = own_level(state, defs, skill);
    if current >= defs.max_level(skill) {
        return None;
    }
    let curve = defs.cost_curve(skill);
    let cost = curve.base.saturating_add((current - 1) / curve.every);
    Some(cost.max(MIN_COST))
}

/// XP levels adding one more focus area costs: nothing for the first ever when the settings say so, else the
/// settings' cost for each area that will be active.
pub fn focus_cost(defs: &Defs, state: &PlayerState) -> u32 {
    if defs.settings.focus_first_free && !state.focus_chosen_before {
        return 0;
    }
    let active = u32::try_from(state.focus.len()).unwrap_or(u32::MAX);
    defs.settings
        .focus_cost
        .saturating_mul(active.saturating_add(1))
}

/// The rewards of the player's active focus areas, in area id order; areas a pack removed give none.
pub fn active_rewards<'a>(defs: &'a Defs, state: &PlayerState) -> Vec<&'a Reward> {
    defs.focus
        .values()
        .filter(|area| state.focus.contains(&area.id))
        .flat_map(|area| &area.rewards)
        .collect()
}

/// Whether the skill is mastered: at its top level with every one of its perks bought.
pub fn mastered(defs: &Defs, state: &PlayerState, skill: &str) -> bool {
    defs.skill(skill).is_some()
        && own_level(state, defs, skill) >= defs.max_level(skill)
        && defs
            .perks
            .values()
            .filter(|perk| perk.skill == skill)
            .all(|perk| owns(state, &perk.id))
}

/// Perk points earned by the skill's level, plus granted ones, minus spent ones.
pub fn points_available(defs: &Defs, state: &PlayerState, skill: &str) -> u32 {
    let earned = own_level(state, defs, skill) / defs.points_every(skill);
    let (spent, bonus) = state
        .skills
        .get(skill)
        .map_or((0, 0), |p| (p.points_spent, p.bonus_points));
    earned.saturating_add(bonus).saturating_sub(spent)
}

/// Grants the request against the player's XP levels, or says why not. The state passed in is never changed.
pub fn apply(
    defs: &Defs,
    state: &PlayerState,
    xp_levels: u32,
    req: &Request,
) -> Result<Outcome, Refusal> {
    apply_under(defs, &Rules::default(), state, xp_levels, req)
}

/// [`apply`] under the server's rules, which decide whether perks may be switched and refunded.
pub fn apply_under(
    defs: &Defs,
    rules: &Rules,
    state: &PlayerState,
    xp_levels: u32,
    req: &Request,
) -> Result<Outcome, Refusal> {
    match req {
        Request::Raise { skill, times } => {
            raise_times(defs, rules, state, xp_levels, skill, *times)
        }
        Request::BuyPerk { perk } => buy_perk(defs, state, perk),
        Request::AddFocus { focus } => add_focus(defs, state, xp_levels, focus),
        Request::SwitchPerk { perk } => switch_perk(rules, state, perk),
        Request::RefundPerk { perk } => refund_perk(defs, rules, state, xp_levels, perk),
    }
}

/// The skill whose progress holds the owned perk.
fn holder_of(state: &PlayerState, perk: &str) -> Option<String> {
    state
        .skills
        .iter()
        .find(|(_, progress)| progress.perks.contains(perk))
        .map(|(skill, _)| skill.clone())
}

fn switch_perk(rules: &Rules, state: &PlayerState, perk: &str) -> Result<Outcome, Refusal> {
    if !rules.perk_switches {
        return Err(Refusal::SwitchesOff);
    }
    let skill = holder_of(state, perk).ok_or(Refusal::NotOwned)?;
    let mut next = state.clone();
    let progress = next
        .skills
        .get_mut(&skill)
        .expect("the holder is in the state");
    if !progress.disabled.remove(perk) {
        progress.disabled.insert(perk.to_string());
    }
    Ok(Outcome {
        state: next,
        levels_spent: 0,
        points_spent: 0,
    })
}

fn refund_perk(
    defs: &Defs,
    rules: &Rules,
    state: &PlayerState,
    xp_levels: u32,
    perk: &str,
) -> Result<Outcome, Refusal> {
    let cost = match rules.respec {
        Respec::Off => return Err(Refusal::RefundsOff),
        Respec::Free => 0,
        Respec::Paid => rules.respec_cost,
    };
    let skill = holder_of(state, perk).ok_or(Refusal::NotOwned)?;
    let needing = defs
        .perks
        .values()
        .find(|other| other.requires.iter().any(|r| r == perk) && owns(state, &other.id));
    if let Some(other) = needing {
        return Err(Refusal::NeededBy {
            perk: other.id.clone(),
        });
    }
    if xp_levels < cost {
        return Err(Refusal::NotEnoughLevels { needed: cost });
    }
    let points = defs.perks.get(perk).map_or(0, |p| p.cost);
    let mut next = state.clone();
    let progress = next
        .skills
        .get_mut(&skill)
        .expect("the holder is in the state");
    progress.perks.remove(perk);
    progress.disabled.remove(perk);
    progress.points_spent = progress.points_spent.saturating_sub(points);
    Ok(Outcome {
        state: next,
        levels_spent: cost,
        points_spent: 0,
    })
}

/// [`raise`] up to `times` times: the first must be granted, then each next one while the XP left pays for it.
fn raise_times(
    defs: &Defs,
    rules: &Rules,
    state: &PlayerState,
    xp_levels: u32,
    skill: &str,
    times: u32,
) -> Result<Outcome, Refusal> {
    let mut done = raise(defs, rules, state, xp_levels, skill, (0, 0))?;
    for _ in 1..times.max(1) {
        let spent = (done.levels_spent, done.points_spent);
        match raise(defs, rules, &done.state, xp_levels, skill, spent) {
            Ok(next) => done = next,
            Err(_) => break,
        }
    }
    Ok(done)
}

/// One raise for a player with `xp_levels` who already owes `spent` (levels, points) for raises before it in the
/// same request; the outcome's costs include them.
fn raise(
    defs: &Defs,
    rules: &Rules,
    state: &PlayerState,
    xp_levels: u32,
    skill: &str,
    spent: (u32, u32),
) -> Result<Outcome, Refusal> {
    defs.skill(skill).ok_or(Refusal::UnknownSkill)?;
    if !rules.buying() {
        return Err(Refusal::BuyingOff);
    }
    let cost = raise_cost(defs, state, skill).ok_or(Refusal::AtMax)?;
    let (levels_spent, points_spent) = match rules.cost_unit {
        CostUnit::Levels => {
            if xp_levels.saturating_sub(spent.0) < cost {
                return Err(Refusal::NotEnoughLevels { needed: cost });
            }
            (spent.0 + cost, spent.1)
        }
        CostUnit::Points => {
            let points = cost.saturating_mul(rules.points_per_level);
            if total_points(xp_levels).saturating_sub(spent.1) < points {
                return Err(Refusal::NotEnoughXpPoints { needed: points });
            }
            (spent.0, spent.1 + points)
        }
    };
    let mut next = state.clone();
    let own = own_level(state, defs, skill) + 1;
    next.skills
        .entry(skill.to_string())
        .or_default()
        .set_own(own);
    Ok(Outcome {
        state: next,
        levels_spent,
        points_spent,
    })
}

/// Hundredths of an action the skill's next level takes when earned by doing: ten actions for each level it has.
/// None at its top level.
pub fn earn_threshold(defs: &Defs, state: &PlayerState, skill: &str) -> Option<u32> {
    defs.skill(skill)?;
    let now = own_level(state, defs, skill);
    (now < defs.max_level(skill)).then_some(now.saturating_mul(ACTIONS_PER_LEVEL * HUNDREDTHS))
}

/// Ten actions a level, counted in hundredths so a pace below 100% adds up.
const ACTIONS_PER_LEVEL: u32 = 10;
const HUNDREDTHS: u32 = 100;

/// Adds `hundredths` of progress to the skill, climbing as many levels as it fills, the rest carried; a skill at
/// its top keeps nothing. Gives the new state and the levels gained.
pub fn earn(defs: &Defs, state: &PlayerState, skill: &str, hundredths: u32) -> (PlayerState, u32) {
    let mut next = state.clone();
    let mut gained = 0;
    let mut pool = state
        .skills
        .get(skill)
        .map_or(0, |p| p.earned)
        .saturating_add(hundredths);
    while let Some(needed) = earn_threshold(defs, &next, skill) {
        if pool < needed {
            break;
        }
        pool -= needed;
        gained += 1;
        let own = own_level(&next, defs, skill) + 1;
        next.skills
            .entry(skill.to_string())
            .or_default()
            .set_own(own);
    }
    if earn_threshold(defs, &next, skill).is_none() {
        pool = 0;
    }
    if defs.skill(skill).is_some() {
        next.skills.entry(skill.to_string()).or_default().earned = pool;
    }
    (next, gained)
}

/// The XP points it takes to go from `level` to the next, as vanilla counts them.
fn points_to_next(level: u32) -> u32 {
    match level {
        0..=15 => 2 * level + 7,
        16..=30 => 5 * level - 38,
        _ => 9 * level - 158,
    }
}

/// The XP points a player at `level` (and no progress past it) holds.
pub fn total_points(level: u32) -> u32 {
    // The game's closed forms of the sum of `points_to_next` below the level, doubled to stay in whole numbers.
    let l = u128::from(level);
    let doubled = match level {
        0..=16 => 2 * (l * l + 6 * l),
        17..=31 => 5 * l * l + 720 - 81 * l,
        _ => 9 * l * l + 4440 - 325 * l,
    };
    u32::try_from(doubled / 2).unwrap_or(u32::MAX)
}

/// A player at `level` with `progress` (0 to 1) toward the next pays `points`: their new level, progress and total
/// points, as the game keeps them; none when they hold fewer.
pub fn after_points(level: u32, progress: f32, points: u32) -> Option<(u32, f32, u32)> {
    let past = f64::from(progress.clamp(0.0, 1.0)) * f64::from(points_to_next(level));
    let held = total_points(level).saturating_add(past.floor() as u32);
    let left = held.checked_sub(points)?;
    let new_level = (0..=level)
        .take_while(|l| total_points(*l) <= left)
        .last()
        .unwrap_or(0);
    let over = left - total_points(new_level);
    let progress = over as f32 / points_to_next(new_level) as f32;
    Some((new_level, progress, left))
}

/// How to take `points` from a player at `level`: whole levels from the top, and the points given back after; none
/// when they hold fewer.
pub fn levels_for_points(level: u32, points: u32) -> Option<(u32, u32)> {
    let mut taken = (0, 0);
    while taken.1 < points {
        let below = level.checked_sub(taken.0 + 1)?;
        taken = (taken.0 + 1, taken.1 + points_to_next(below));
    }
    Some((taken.0, taken.1 - points))
}

/// The first other skill the perk needs that the player's own level falls short of, with the level it needs.
pub fn missing_skill<'a>(
    defs: &Defs,
    state: &PlayerState,
    perk: &'a PerkDef,
) -> Option<(&'a String, &'a u32)> {
    perk.also_needs
        .iter()
        .find(|(skill, needed)| own_level(state, defs, skill) < **needed)
}

fn buy_perk(defs: &Defs, state: &PlayerState, id: &str) -> Result<Outcome, Refusal> {
    let perk = defs.perks.get(id).ok_or(Refusal::UnknownPerk)?;
    if owns(state, id) {
        return Err(Refusal::AlreadyOwned);
    }
    if own_level(state, defs, &perk.skill) < perk.min_level {
        return Err(Refusal::BelowMinLevel {
            needed: perk.min_level,
        });
    }
    if let Some((skill, needed)) = missing_skill(defs, state, perk) {
        return Err(Refusal::BelowSkillLevel {
            skill: skill.clone(),
            needed: *needed,
        });
    }
    if let Some(missing) = perk.requires.iter().find(|required| !owns(state, required)) {
        return Err(Refusal::MissingPerk {
            perk: missing.clone(),
        });
    }
    if points_available(defs, state, &perk.skill) < perk.cost {
        return Err(Refusal::NotEnoughPoints { needed: perk.cost });
    }
    let mut next = state.clone();
    let progress = next.skills.entry(perk.skill.clone()).or_default();
    progress.points_spent += perk.cost;
    progress.perks.insert(id.to_string());
    Ok(Outcome {
        state: next,
        levels_spent: 0,
        points_spent: 0,
    })
}

fn add_focus(
    defs: &Defs,
    state: &PlayerState,
    xp_levels: u32,
    focus: &str,
) -> Result<Outcome, Refusal> {
    if !defs.focus.contains_key(focus) {
        return Err(Refusal::UnknownFocus);
    }
    if state.focus.contains(focus) {
        return Err(Refusal::AlreadyFocused);
    }
    let cost = focus_cost(defs, state);
    if xp_levels < cost {
        return Err(Refusal::NotEnoughLevels { needed: cost });
    }
    let mut next = state.clone();
    next.focus.insert(focus.to_string());
    next.focus_chosen_before = true;
    Ok(Outcome {
        state: next,
        levels_spent: cost,
        points_spent: 0,
    })
}

/// Whether the player switched the owned perk off.
pub fn switched_off(state: &PlayerState, perk: &str) -> bool {
    state
        .skills
        .values()
        .any(|progress| progress.disabled.contains(perk))
}

/// Whether the perk was bought, under whichever skill it was saved.
fn owns(state: &PlayerState, perk: &str) -> bool {
    state
        .skills
        .values()
        .any(|progress| progress.perks.contains(perk))
}

/// The state after a death: each known skill loses the settings' penalty, never below level 1.
pub fn on_death(defs: &Defs, state: &PlayerState) -> PlayerState {
    let penalty = defs.settings.death_penalty_levels;
    let mut next = state.clone();
    for (skill, progress) in &mut next.skills {
        if defs.skill(skill).is_some() {
            let own = own_level(state, defs, skill)
                .saturating_sub(penalty)
                .max(MIN_LEVEL);
            progress.set_own(own);
        }
    }
    next
}

/// Bought perks whose skill level still meets their minimum, in perk id order.
pub fn active_perks<'a>(defs: &'a Defs, state: &PlayerState) -> Vec<&'a PerkDef> {
    defs.perks
        .values()
        .filter(|perk| {
            owns(state, &perk.id)
                && !switched_off(state, &perk.id)
                && own_level(state, defs, &perk.skill) >= perk.min_level
                && missing_skill(defs, state, perk).is_none()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::{CostCurve, FocusDef, PerkDef, Reward, SkillDef};
    use crate::rules::{CostUnit, Respec, Rules};

    const MINING: &str = "reskilled:mining";
    const MELEE: &str = "reskilled:melee";
    const WORKING: &str = "reskilled:working";
    const FIGHTING: &str = "reskilled:fighting";
    const ROAMING: &str = "reskilled:roaming";
    const SURE_GRIP: &str = "reskilled:mining_sure_grip";
    const DEEP_VEINS: &str = "reskilled:mining_deep_veins";
    const OLD_SKILL: &str = "oldpack:digging";
    const PLENTY: u32 = 10_000;

    fn skill(id: &str) -> SkillDef {
        SkillDef {
            id: id.into(),
            priority: 0,
            name: "n".into(),
            icon: "minecraft:stone".into(),
            max_level: None,
            cost: None,
            points_every: None,
            icon_levels: Default::default(),
            earned_by: Vec::new(),
        }
    }

    fn perk(id: &str, cost: u32, min_level: u32, requires: &[&str]) -> PerkDef {
        PerkDef {
            id: id.into(),
            priority: 0,
            skill: MINING.into(),
            name: "n".into(),
            description: "d".into(),
            cost,
            min_level,
            requires: requires.iter().map(|r| r.to_string()).collect(),
            also_needs: BTreeMap::new(),
            effects: vec![],
        }
    }

    /// Mining (in Working) and Melee (in Fighting) with default settings, and two Mining perks: Sure Grip (1 point,
    /// level 1) and Deep Veins (2 points, level 8, needs Sure Grip).
    fn defs() -> Defs {
        let focus = |id: &str, skill: &str| focus_def(id, &[skill]);
        Defs {
            skills: [(MINING.into(), skill(MINING)), (MELEE.into(), skill(MELEE))].into(),
            perks: [
                (SURE_GRIP.into(), perk(SURE_GRIP, 1, 1, &[])),
                (DEEP_VEINS.into(), perk(DEEP_VEINS, 2, 8, &[SURE_GRIP])),
            ]
            .into(),
            focus: [
                (WORKING.into(), focus(WORKING, MINING)),
                (FIGHTING.into(), focus(FIGHTING, MELEE)),
            ]
            .into(),
            ..Defs::default()
        }
    }

    fn at_level(skill: &str, level: u32) -> PlayerState {
        let mut state = PlayerState::default();
        state.skills.insert(
            skill.into(),
            SkillProgress {
                level,
                ..SkillProgress::default()
            },
        );
        state
    }

    fn raise(skill: &str) -> Request {
        raise_times(skill, 1)
    }

    /// Mining shown at `shared` because a group shares it, the player's own level being `own`.
    fn lifted(shared: u32, own: u32) -> PlayerState {
        let mut state = at_level(MINING, shared);
        state.skills.get_mut(MINING).unwrap().own = Some(own);
        state
    }

    #[test]
    fn a_perk_needing_another_skill_waits_for_its_own_level() {
        // Given a Mining perk that also needs Melee 5, and players at Melee 4, at 5, and lifted to 9 from an own 4
        let mut defs = defs();
        let mut both = perk("reskilled:both", 1, 1, &[]);
        both.also_needs = [(MELEE.to_string(), 5)].into();
        defs.perks.insert(both.id.clone(), both);
        let melee = |level: u32, own: Option<u32>| {
            let mut state = at_level(MINING, 4);
            state.skills.insert(
                MELEE.into(),
                SkillProgress {
                    level,
                    own,
                    ..SkillProgress::default()
                },
            );
            state
        };

        // When each tries to buy it, and the one who bought it at 5 drops to an own Melee 4
        let short = apply(&defs, &melee(4, None), PLENTY, &buy("reskilled:both"));
        let enough = apply(&defs, &melee(5, None), PLENTY, &buy("reskilled:both")).unwrap();
        let lifted = apply(&defs, &melee(9, Some(4)), PLENTY, &buy("reskilled:both"));
        let mut dropped = enough.state.clone();
        dropped.skills.get_mut(MELEE).unwrap().level = 4;

        // Then only Melee 5 of their own buys it, and below it the perk stops working
        let refused = Err(Refusal::BelowSkillLevel {
            skill: MELEE.into(),
            needed: 5,
        });
        assert_eq!(short, refused);
        assert_eq!(lifted, refused);
        assert!(
            active_perks(&defs, &enough.state)
                .iter()
                .any(|p| p.id == "reskilled:both")
        );
        assert!(
            !active_perks(&defs, &dropped)
                .iter()
                .any(|p| p.id == "reskilled:both")
        );
    }

    #[test]
    fn raising_while_lifted_raises_the_own_level_at_its_price() {
        // Given Mining shown at 20 through a party, the player's own at 5 (a raise from 5 costs 5)
        let defs = defs();

        // When they raise it
        let outcome = apply(&defs, &lifted(20, 5), PLENTY, &raise(MINING)).unwrap();

        // Then it costs the own level's price, the own level is 6, and the shown level stays 20
        assert_eq!(outcome.levels_spent, 5);
        assert_eq!(own_level(&outcome.state, &defs, MINING), 6);
        assert_eq!(level(&outcome.state, &defs, MINING), 20);
    }

    #[test]
    fn raising_the_own_level_up_to_the_shared_one_ends_the_lift() {
        // Given Mining shown at 6 through a party, the player's own at 5
        let defs = defs();

        // When they raise it
        let outcome = apply(&defs, &lifted(6, 5), PLENTY, &raise(MINING)).unwrap();

        // Then their own level is 6, shown as it is, with nothing lifted any more
        assert_eq!(level(&outcome.state, &defs, MINING), 6);
        assert_eq!(outcome.state.skills[MINING].own, None);
    }

    #[test]
    fn perks_hang_on_the_own_level() {
        // Given Mining shown at 20 through a party, the player's own at 2 (a point every 2 levels), owning Sure Grip
        // (from level 1) and wanting Deep Veins (from level 8)
        let defs = defs();
        let state = lifted(20, 2);

        // When perk points and the minimum level are checked
        let points = points_available(&defs, &state, MINING);
        let deep_veins = apply(&defs, &state, PLENTY, &buy(DEEP_VEINS));

        // Then the points come from level 2 and Deep Veins waits for the own level 8
        assert_eq!(points, 1);
        assert_eq!(deep_veins, Err(Refusal::BelowMinLevel { needed: 8 }));
    }

    #[test]
    fn death_lowers_the_own_level_under_a_lift() {
        // Given a 2-level death penalty and Mining shown at 20 through a party, the player's own at 10
        let mut defs = defs();
        defs.settings.death_penalty_levels = 2;

        // When they die
        let after = on_death(&defs, &lifted(20, 10));

        // Then their own level is 8; the shared level is left for the group to decide
        assert_eq!(own_level(&after, &defs, MINING), 8);
        assert_eq!(level(&after, &defs, MINING), 20);
    }

    fn buy(perk: &str) -> Request {
        Request::BuyPerk { perk: perk.into() }
    }

    fn add_focus(focus: &str) -> Request {
        Request::AddFocus {
            focus: focus.into(),
        }
    }

    fn focus_def(id: &str, skills: &[&str]) -> FocusDef {
        FocusDef {
            id: id.into(),
            priority: 0,
            name: "n".into(),
            skills: skills.iter().map(|s| (*s).to_string()).collect(),
            rewards: vec![],
        }
    }

    #[test]
    fn cost_follows_default_curve() {
        // Given Mining at each level from 1 to 31, with no focus
        let defs = defs();

        // When the raise cost is read at each level
        let costs: Vec<u32> = (1..=31)
            .map(|l| raise_cost(&defs, &at_level(MINING, l), MINING).unwrap())
            .collect();

        // Then it starts at 1 and grows by 1 with every level, to 31 at the last
        assert_eq!(costs, (1..=31).collect::<Vec<u32>>());
    }

    #[test]
    fn total_to_max_is_496() {
        // Given a new player with plenty of XP levels
        let defs = defs();
        let mut state = PlayerState::default();
        let mut spent = 0;

        // When Mining is raised until it refuses
        while let Ok(outcome) = apply(&defs, &state, PLENTY, &raise(MINING)) {
            spent += outcome.levels_spent;
            state = outcome.state;
        }

        // Then Mining is at 32 and it cost 1 + 2 + ... + 31 = 496 levels in all
        assert_eq!(level(&state, &defs, MINING), 32);
        assert_eq!(spent, 496);
    }

    #[test]
    fn a_focus_never_changes_what_raising_costs() {
        // Given a player focused on Working (Mining), and one with no focus
        let defs = defs();
        let mut focused = at_level(MINING, 1);
        focused.focus.insert(WORKING.into());

        // When Mining's raise is priced for both
        let costs = [&focused, &at_level(MINING, 1)].map(|state| raise_cost(&defs, state, MINING));

        // Then it costs the same
        assert_eq!(costs, [Some(1), Some(1)]);
    }

    #[test]
    fn cost_is_never_below_1() {
        // Given a pack whose cost curve starts at 0
        let mut defs = defs();
        defs.settings.cost = CostCurve {
            base: 0,
            every: 1000,
        };

        // When a raise is priced
        let cost = raise_cost(&defs, &at_level(MELEE, 1), MELEE);

        // Then it still costs 1
        assert_eq!(cost, Some(1));
    }

    #[test]
    fn a_skill_is_mastered_at_its_top_level_with_every_perk() {
        // Given Mining at 32 with both its perks, at 32 with one, and at 31 with both; and Melee (no perks) at 32
        let defs = defs();
        let with = |level: u32, perks: &[&str]| {
            let mut state = at_level(MINING, level);
            let mining = state.skills.get_mut(MINING).unwrap();
            mining.perks = perks.iter().map(|p| (*p).to_string()).collect();
            state
        };
        let melee_top = at_level(MELEE, 32);

        // When each is asked whether it is mastered
        let all = mastered(&defs, &with(32, &[SURE_GRIP, DEEP_VEINS]), MINING);
        let one_short = mastered(&defs, &with(32, &[SURE_GRIP]), MINING);
        let below_top = mastered(&defs, &with(31, &[SURE_GRIP, DEEP_VEINS]), MINING);
        let no_perks = mastered(&defs, &melee_top, MELEE);

        // Then only the top level with every perk counts; a skill with no perks needs only the top level
        assert_eq!(
            (all, one_short, below_top, no_perks),
            (true, false, false, true)
        );
    }

    #[test]
    fn points_every_second_level() {
        // Given Mining at levels 3, 4 and 32, and at 32 with 3 points spent and 2 granted
        let defs = defs();
        let mut spent_and_granted = at_level(MINING, 32);
        let mining = spent_and_granted.skills.get_mut(MINING).unwrap();
        mining.points_spent = 3;
        mining.bonus_points = 2;

        // When the points available are read
        let points = [
            at_level(MINING, 3),
            at_level(MINING, 4),
            at_level(MINING, 32),
            spent_and_granted,
        ]
        .map(|state| points_available(&defs, &state, MINING));

        // Then a point comes every 2nd level, plus the granted ones, minus the spent ones
        assert_eq!(points, [1, 2, 16, 15]);
    }

    #[test]
    fn raise_spends_levels_and_adds_a_level() {
        // Given Mining at 5 and Melee at 3, and 10 XP levels
        let defs = defs();
        let mut state = at_level(MINING, 5);
        state.skills.insert(
            MELEE.into(),
            SkillProgress {
                level: 3,
                ..SkillProgress::default()
            },
        );

        // When Mining is raised
        let outcome = apply(&defs, &state, 10, &raise(MINING)).unwrap();

        // Then it costs 5 levels, Mining is at 6, and Melee is untouched
        assert_eq!(outcome.levels_spent, 5);
        assert_eq!(level(&outcome.state, &defs, MINING), 6);
        assert_eq!(level(&outcome.state, &defs, MELEE), 3);
    }

    fn raise_times(skill: &str, times: u32) -> Request {
        Request::Raise {
            skill: skill.into(),
            times,
        }
    }

    #[test]
    fn paying_points_lands_on_the_right_level_and_progress() {
        // Given a player at level 5 halfway to 6 (55 + 8.5 = 63.5 points), and one at level 2 with nothing past it
        // When 20 points are paid from the first, 63 points from it, and 50 from the second (who has 16)
        let twenty = after_points(5, 0.5, 20);
        let all_but = after_points(5, 0.5, 63);
        let short = after_points(2, 0.0, 50);

        // Then 43 points left is level 4 (40) and 3 of 15 toward 5; 0 points left is level 0; the last is refused
        let (level, progress, total) = twenty.unwrap();
        assert_eq!((level, total), (4, 43));
        assert!((progress - 3.0 / 15.0).abs() < 1e-6, "{progress}");
        assert_eq!(all_but.map(|(l, _, t)| (l, t)), Some((0, 0)));
        assert_eq!(short, None);
    }

    #[test]
    fn total_points_follow_the_game_and_never_overflow() {
        // Given every level up to 1000 summed one level at a time, as the game counts them
        let summed: Vec<u64> = (0..=1000u32)
            .scan(0u64, |sum, level| {
                let here = *sum;
                *sum += u64::from(points_to_next(level));
                Some(here)
            })
            .collect();

        // When the totals are read, and at levels no game reaches
        let totals: Vec<u64> = (0..=1000).map(|l| u64::from(total_points(l))).collect();
        let huge = total_points(u32::MAX);

        // Then they match, and the huge one stops at the largest number instead of overflowing
        assert_eq!(totals, summed);
        assert_eq!(huge, u32::MAX);
    }

    #[test]
    fn raise_five_times_takes_the_five_prices_at_once() {
        // Given Mining at 5 (raises cost 5, 6, 7, 8, 9) and 100 XP levels
        let defs = defs();

        // When Mining is raised five times in one request
        let outcome = apply(&defs, &at_level(MINING, 5), 100, &raise_times(MINING, 5)).unwrap();

        // Then it is at 10 and the five prices are taken together
        assert_eq!(level(&outcome.state, &defs, MINING), 10);
        assert_eq!(outcome.levels_spent, 35);
    }

    #[test]
    fn raising_many_times_stops_where_the_levels_or_the_skill_run_out() {
        // Given Mining at 5 with 20 XP levels (5 + 6 + 7 = 18 fits, an 8 more does not), and Mining at 30
        let defs = defs();

        // When each asks for as many raises as they can get, and one with 4 levels asks for five
        let short = apply(
            &defs,
            &at_level(MINING, 5),
            20,
            &raise_times(MINING, u32::MAX),
        )
        .unwrap();
        let near_max = apply(
            &defs,
            &at_level(MINING, 30),
            PLENTY,
            &raise_times(MINING, u32::MAX),
        )
        .unwrap();
        let refused = apply(&defs, &at_level(MINING, 5), 4, &raise_times(MINING, 5));

        // Then the first gets three raises, the second stops at the max, and the last gets the usual refusal
        assert_eq!(
            (level(&short.state, &defs, MINING), short.levels_spent),
            (8, 18)
        );
        assert_eq!(
            (level(&near_max.state, &defs, MINING), near_max.levels_spent),
            (32, 61)
        );
        assert_eq!(refused, Err(Refusal::NotEnoughLevels { needed: 5 }));
    }

    #[test]
    fn raising_many_times_in_points_counts_every_point() {
        // Given raises paid in points at 20 a level, Mining at 1 (costs 1, 2, 3: 20, 40, 60 points), and 100 points
        let defs = defs();
        let rules = Rules {
            cost_unit: CostUnit::Points,
            ..Rules::default()
        };
        let levels_for_100 = (0..).find(|l| total_points(*l) >= 100).unwrap();
        let xp = levels_for_100 - u32::from(total_points(levels_for_100) > 100);

        // When as many raises as they can get are asked for
        let outcome = apply_under(
            &defs,
            &rules,
            &at_level(MINING, 1),
            xp,
            &raise_times(MINING, u32::MAX),
        )
        .unwrap();

        // Then they get the raises their points pay for, all priced together
        let mut paid = 0;
        let mut raises = 0;
        while paid + (raises + 1) * 20 <= total_points(xp) {
            raises += 1;
            paid += raises * 20;
        }
        assert_eq!(level(&outcome.state, &defs, MINING), 1 + raises);
        assert_eq!(outcome.points_spent, paid);
    }

    #[test]
    fn raise_without_levels_is_refused_and_takes_nothing() {
        // Given a new player with 0 XP levels (the cost is 1), and one at Mining 3 with 2 (the cost is 3)
        let defs = defs();
        let state = PlayerState::default();
        let at_3 = at_level(MINING, 3);

        // When raise requests are spammed
        let with_none: Vec<_> = (0..100)
            .map(|_| apply(&defs, &state, 0, &raise(MINING)))
            .collect();
        let short = apply(&defs, &at_3, 2, &raise(MINING));

        // Then every one is refused with the levels needed, and the states stay as they were
        assert!(
            with_none
                .iter()
                .all(|r| *r == Err(Refusal::NotEnoughLevels { needed: 1 }))
        );
        assert_eq!(short, Err(Refusal::NotEnoughLevels { needed: 3 }));
        assert_eq!(level(&state, &defs, MINING), 1);
    }

    #[test]
    fn raise_at_max_is_refused() {
        // Given Mining at 32, a save with Mining at 40 (a pack lowered the max), and an unknown skill
        let defs = defs();

        // When each is raised
        let at_max = apply(&defs, &at_level(MINING, 32), PLENTY, &raise(MINING));
        let above_max = apply(&defs, &at_level(MINING, 40), PLENTY, &raise(MINING));
        let unknown = apply(
            &defs,
            &PlayerState::default(),
            PLENTY,
            &raise("reskilled:nothing"),
        );

        // Then the first two are at the max (the saved 40 reads as 32) and the last is unknown
        assert_eq!(at_max, Err(Refusal::AtMax));
        assert_eq!(above_max, Err(Refusal::AtMax));
        assert_eq!(level(&at_level(MINING, 40), &defs, MINING), 32);
        assert_eq!(unknown, Err(Refusal::UnknownSkill));
    }

    #[test]
    fn perk_needs_points_min_level_and_required_perks() {
        // Given a point every 4th level, and Mining at 7 with 1 point, at 8 with 2 points and nothing bought, and at
        // 12 owning Sure Grip
        let mut defs = defs();
        defs.settings.points_every = 4;
        let mut owns_grip = at_level(MINING, 12);
        owns_grip
            .skills
            .get_mut(MINING)
            .unwrap()
            .perks
            .insert(SURE_GRIP.into());
        owns_grip.skills.get_mut(MINING).unwrap().points_spent = 1;

        // When perks are bought
        let below_min = apply(&defs, &at_level(MINING, 7), 0, &buy(DEEP_VEINS));
        let missing_grip = apply(&defs, &at_level(MINING, 8), 0, &buy(DEEP_VEINS));
        let grip = apply(&defs, &at_level(MINING, 4), 0, &buy(SURE_GRIP)).unwrap();
        let no_points = apply(&defs, &at_level(MINING, 3), 0, &buy(SURE_GRIP));
        let deep = apply(&defs, &owns_grip, 0, &buy(DEEP_VEINS)).unwrap();
        let again = apply(&defs, &owns_grip, 0, &buy(SURE_GRIP));
        let unknown = apply(&defs, &owns_grip, 0, &buy("reskilled:nothing"));

        // Then each refusal says what is missing, and a purchase spends points but no XP levels
        assert_eq!(below_min, Err(Refusal::BelowMinLevel { needed: 8 }));
        assert_eq!(
            missing_grip,
            Err(Refusal::MissingPerk {
                perk: SURE_GRIP.into()
            })
        );
        assert_eq!(no_points, Err(Refusal::NotEnoughPoints { needed: 1 }));
        assert_eq!(again, Err(Refusal::AlreadyOwned));
        assert_eq!(unknown, Err(Refusal::UnknownPerk));
        assert_eq!(grip.levels_spent, 0);
        assert!(grip.state.skills[MINING].perks.contains(SURE_GRIP));
        assert_eq!(points_available(&defs, &grip.state, MINING), 0);
        assert!(deep.state.skills[MINING].perks.contains(DEEP_VEINS));
        assert_eq!(points_available(&defs, &deep.state, MINING), 0);
    }

    /// Mining at 12 owning Sure Grip (1 point) and Deep Veins (2 points, needs Sure Grip), 3 points spent.
    fn owns_both() -> PlayerState {
        let mut state = at_level(MINING, 12);
        let mining = state.skills.get_mut(MINING).unwrap();
        mining.perks = [SURE_GRIP, DEEP_VEINS].map(String::from).into();
        mining.points_spent = 3;
        state
    }

    fn rules(change: impl FnOnce(&mut Rules)) -> Rules {
        let mut rules = Rules::default();
        change(&mut rules);
        rules
    }

    #[test]
    fn an_owned_perk_switches_off_and_on_and_stops_working_while_off() {
        // Given a player owning both Mining perks
        let defs = defs();
        let state = owns_both();
        let switch = |perk: &str| Request::SwitchPerk { perk: perk.into() };

        // When Deep Veins is switched off, then on again, and a perk they do not own is switched
        let off = apply_under(&defs, &Rules::default(), &state, 0, &switch(DEEP_VEINS)).unwrap();
        let on = apply_under(&defs, &Rules::default(), &off.state, 0, &switch(DEEP_VEINS)).unwrap();
        let not_owned = apply_under(
            &defs,
            &Rules::default(),
            &at_level(MINING, 12),
            0,
            &switch(DEEP_VEINS),
        );

        // Then while off it is still owned but not active, it costs nothing either way, and another's is refused
        let active = |s: &PlayerState| -> Vec<String> {
            active_perks(&defs, s)
                .iter()
                .map(|p| p.id.clone())
                .collect()
        };
        assert!(off.state.skills[MINING].perks.contains(DEEP_VEINS));
        assert_eq!(active(&off.state), vec![SURE_GRIP.to_string()]);
        assert_eq!(
            active(&on.state),
            vec![DEEP_VEINS.to_string(), SURE_GRIP.to_string()]
        );
        assert_eq!((off.levels_spent, on.levels_spent), (0, 0));
        assert_eq!(not_owned, Err(Refusal::NotOwned));
    }

    #[test]
    fn switching_is_refused_when_the_rules_forbid_it() {
        // Given a server whose rules turn perk switches off
        let defs = defs();
        let forbid = rules(|r| r.perk_switches = false);

        // When a player switches an owned perk
        let switched = apply_under(
            &defs,
            &forbid,
            &owns_both(),
            0,
            &Request::SwitchPerk {
                perk: SURE_GRIP.into(),
            },
        );

        // Then it is refused
        assert_eq!(switched, Err(Refusal::SwitchesOff));
    }

    #[test]
    fn a_refund_gives_the_points_back_at_the_rules_price() {
        // Given a player owning both Mining perks, on a server where refunds cost 5 levels, and on one where they
        // are free
        let defs = defs();
        let paid = rules(|r| {
            r.respec = Respec::Paid;
            r.respec_cost = 5;
        });
        let free = rules(|r| r.respec = Respec::Free);
        let refund = |perk: &str| Request::RefundPerk { perk: perk.into() };

        // When Deep Veins is refunded with 4 levels and with 5, and for free
        let short = apply_under(&defs, &paid, &owns_both(), 4, &refund(DEEP_VEINS));
        let bought = apply_under(&defs, &paid, &owns_both(), 5, &refund(DEEP_VEINS)).unwrap();
        let gratis = apply_under(&defs, &free, &owns_both(), 0, &refund(DEEP_VEINS)).unwrap();

        // Then the paid one needs 5 levels, takes them, and gives Deep Veins' 2 points back; the free one takes none
        assert_eq!(short, Err(Refusal::NotEnoughLevels { needed: 5 }));
        assert_eq!(bought.levels_spent, 5);
        assert!(!bought.state.skills[MINING].perks.contains(DEEP_VEINS));
        assert_eq!(bought.state.skills[MINING].points_spent, 1);
        assert_eq!(gratis.levels_spent, 0);
    }

    #[test]
    fn a_refund_is_refused_when_the_rules_forbid_it_or_another_perk_needs_it() {
        // Given a player owning both Mining perks (Deep Veins needs Sure Grip)
        let defs = defs();
        let refund = |perk: &str| Request::RefundPerk { perk: perk.into() };

        // When Sure Grip is refunded, refunds are off, and a perk they do not own is refunded
        let needed = apply_under(
            &defs,
            &Rules::default(),
            &owns_both(),
            PLENTY,
            &refund(SURE_GRIP),
        );
        let off = apply_under(
            &defs,
            &rules(|r| r.respec = Respec::Off),
            &owns_both(),
            PLENTY,
            &refund(DEEP_VEINS),
        );
        let not_owned = apply_under(
            &defs,
            &Rules::default(),
            &at_level(MINING, 12),
            PLENTY,
            &refund(DEEP_VEINS),
        );

        // Then each is refused for its reason
        assert_eq!(
            needed,
            Err(Refusal::NeededBy {
                perk: DEEP_VEINS.into()
            })
        );
        assert_eq!(off, Err(Refusal::RefundsOff));
        assert_eq!(not_owned, Err(Refusal::NotOwned));
    }

    #[test]
    fn xp_points_follow_vanilla_s_level_curve() {
        // Given levels 0, 1, 15, 16, 30, 31 and 32
        // When the XP points they hold are read
        let points = [0, 1, 15, 16, 30, 31, 32].map(total_points);

        // Then they are vanilla's: 7 for the first level, 2n + 7 to 16, 5n - 38 to 31, 9n - 158 after
        assert_eq!(points, [0, 7, 315, 352, 1395, 1507, 1628]);
    }

    #[test]
    fn taking_points_takes_whole_levels_from_the_top_and_gives_back_the_rest() {
        // Given a player at level 10 (levels 9 to 10 hold 25 points, 8 to 9 hold 23)
        // When 30 points are to be taken, then 25, then more than they have
        let thirty = levels_for_points(10, 30);
        let exact = levels_for_points(10, 25);
        let too_many = levels_for_points(10, total_points(10) + 1);

        // Then two levels go and 18 points come back; one level goes exactly; and too many is none
        assert_eq!(thirty, Some((2, 18)));
        assert_eq!(exact, Some((1, 0)));
        assert_eq!(too_many, None);
    }

    #[test]
    fn under_the_points_rule_a_raise_costs_points_the_same_at_any_level() {
        // Given raises paid in XP points, 20 a level of cost; Mining at 1 (a raise costs 1, so 20 points)
        let defs = defs();
        let rules = rules(|r| r.cost_unit = CostUnit::Points);
        let state = at_level(MINING, 1);

        // When a player at level 2 (16 points) and one at level 3 (27 points) raise it
        let short = apply_under(&defs, &rules, &state, 2, &raise(MINING));
        let paid = apply_under(&defs, &rules, &state, 3, &raise(MINING)).unwrap();

        // Then the first lacks points, and the second pays 20 points and no levels as such
        assert_eq!(short, Err(Refusal::NotEnoughXpPoints { needed: 20 }));
        assert_eq!((paid.levels_spent, paid.points_spent), (0, 20));
        assert_eq!(level(&paid.state, &defs, MINING), 2);
    }

    #[test]
    fn earned_progress_fills_to_the_next_level_and_carries_over() {
        // Given Mining at 2 (the next level needs 20 actions' worth, 2000 hundredths)
        let defs = defs();
        let state = at_level(MINING, 2);

        // When 1500 hundredths are earned, then 700 more, and a skill at its top earns some
        let (part, gained_part) = earn(&defs, &state, MINING, 1_500);
        let (over, gained_over) = earn(&defs, &part, MINING, 700);
        let (top, gained_top) = earn(&defs, &at_level(MINING, 32), MINING, 5_000);

        // Then the first stays at 2 with 1500 toward 3; the second reaches 3 with 200 over; the top keeps nothing
        assert_eq!(earn_threshold(&defs, &state, MINING), Some(2_000));
        assert_eq!(
            (
                level(&part, &defs, MINING),
                part.skills[MINING].earned,
                gained_part
            ),
            (2, 1_500, 0)
        );
        assert_eq!(
            (
                level(&over, &defs, MINING),
                over.skills[MINING].earned,
                gained_over
            ),
            (3, 200, 1)
        );
        assert_eq!(
            (
                level(&top, &defs, MINING),
                top.skills[MINING].earned,
                gained_top
            ),
            (32, 0, 0)
        );
    }

    #[test]
    fn a_big_earning_can_climb_several_levels() {
        // Given Mining at 1 (1000, then 2000, then 3000 hundredths to climb)
        let defs = defs();

        // When 6500 hundredths are earned at once
        let (state, gained) = earn(&defs, &at_level(MINING, 1), MINING, 6_500);

        // Then it climbs to 4 with 500 over
        assert_eq!(
            (
                level(&state, &defs, MINING),
                state.skills[MINING].earned,
                gained
            ),
            (4, 500, 3)
        );
    }

    #[test]
    fn buying_is_refused_when_skills_only_grow_by_doing() {
        // Given a server where skills only grow by doing
        let defs = defs();
        let earn_only = rules(|r| r.progression = crate::rules::Progression::Earn);

        // When a player with plenty of levels raises Mining
        let raised = apply_under(
            &defs,
            &earn_only,
            &at_level(MINING, 1),
            PLENTY,
            &raise(MINING),
        );

        // Then it is refused
        assert_eq!(raised, Err(Refusal::BuyingOff));
    }

    #[test]
    fn focus_areas_add_up_each_costing_more() {
        // Given a player who never picked a focus, and a third area
        let mut defs = defs();
        defs.focus.insert(ROAMING.into(), focus_def(ROAMING, &[]));
        let state = PlayerState::default();

        // When they add Working with no XP levels, Fighting with 19 and then 20, and Roaming with 30
        let first = apply(&defs, &state, 0, &add_focus(WORKING)).unwrap();
        let short = apply(&defs, &first.state, 19, &add_focus(FIGHTING));
        let second = apply(&defs, &first.state, 20, &add_focus(FIGHTING)).unwrap();
        let third = apply(&defs, &second.state, 30, &add_focus(ROAMING)).unwrap();

        // Then the first is free, the second costs 20 and the third 30, and all three stay active
        assert_eq!(first.levels_spent, 0);
        assert!(first.state.focus_chosen_before);
        assert_eq!(short, Err(Refusal::NotEnoughLevels { needed: 20 }));
        assert_eq!(second.levels_spent, 20);
        assert_eq!(third.levels_spent, 30);
        assert_eq!(
            third.state.focus,
            [WORKING, FIGHTING, ROAMING].map(String::from).into()
        );
    }

    #[test]
    fn focus_requests_that_make_no_sense_are_refused() {
        // Given a player who unlocked Working
        let defs = defs();
        let mut state = PlayerState::default();
        state.focus.insert(WORKING.into());
        state.focus_chosen_before = true;

        // When they add Working again, and an unknown area
        let again = apply(&defs, &state, PLENTY, &add_focus(WORKING));
        let unknown = apply(&defs, &state, PLENTY, &add_focus("reskilled:nothing"));

        // Then both are refused for their reason, and nothing is taken: an unlocked area stays unlocked for good
        assert_eq!(again, Err(Refusal::AlreadyFocused));
        assert_eq!(unknown, Err(Refusal::UnknownFocus));
    }

    #[test]
    fn rewards_come_from_every_active_focus() {
        // Given Working with a block XP bonus, Fighting with a kill bonus, and a player focused on both plus an
        // area a pack removed
        let mut defs = defs();
        defs.focus.get_mut(WORKING).unwrap().rewards = vec![Reward::BlockXp { bonus_percent: 50 }];
        defs.focus.get_mut(FIGHTING).unwrap().rewards = vec![Reward::Kill { bonus_percent: 25 }];
        let state = PlayerState {
            focus: [WORKING, FIGHTING, "oldpack:gone"].map(String::from).into(),
            ..PlayerState::default()
        };

        // When their rewards are read, and those of a player with no focus
        let rewards = active_rewards(&defs, &state);
        let none = active_rewards(&defs, &PlayerState::default());

        // Then both areas' rewards are there, and none for no focus
        assert_eq!(
            rewards,
            vec![
                &Reward::Kill { bonus_percent: 25 },
                &Reward::BlockXp { bonus_percent: 50 }
            ]
        );
        assert!(none.is_empty());
    }

    #[test]
    fn death_penalty_never_goes_below_1() {
        // Given a 3-level death penalty, Mining at 10, Melee at 2, and a skill the pack removed at 9
        let mut defs = defs();
        defs.settings.death_penalty_levels = 3;
        let mut state = at_level(MINING, 10);
        state.skills.insert(
            MELEE.into(),
            SkillProgress {
                level: 2,
                ..SkillProgress::default()
            },
        );
        state.skills.insert(
            OLD_SKILL.into(),
            SkillProgress {
                level: 9,
                ..SkillProgress::default()
            },
        );

        // When the player dies
        let after = on_death(&defs, &state);

        // Then known skills lose 3 levels but stop at 1 (in the save too), and the removed skill is left as saved
        assert_eq!(level(&after, &defs, MINING), 7);
        assert_eq!(level(&after, &defs, MELEE), 1);
        assert_eq!(after.skills[MELEE].level, 1);
        assert_eq!(after.skills[OLD_SKILL].level, 9);
    }

    #[test]
    fn perk_below_min_level_is_inactive() {
        // Given a player at Mining 9 owning Sure Grip and Deep Veins (needs level 8), and a 2-level death penalty
        let mut defs = defs();
        defs.settings.death_penalty_levels = 2;
        let mut state = at_level(MINING, 9);
        let mining = state.skills.get_mut(MINING).unwrap();
        mining.perks = [SURE_GRIP.to_string(), DEEP_VEINS.to_string()].into();
        mining.points_spent = 3;
        let before: Vec<&str> = active_perks(&defs, &state)
            .iter()
            .map(|p| p.id.as_str())
            .collect();

        // When they die and drop to Mining 7
        let after = on_death(&defs, &state);
        let active: Vec<&str> = active_perks(&defs, &after)
            .iter()
            .map(|p| p.id.as_str())
            .collect();

        // Then Deep Veins stays bought but stops working, and Sure Grip still works
        assert_eq!(before, vec![DEEP_VEINS, SURE_GRIP]);
        assert_eq!(active, vec![SURE_GRIP]);
        assert!(after.skills[MINING].perks.contains(DEEP_VEINS));
        assert_eq!(after.skills[MINING].points_spent, 3);
    }

    #[test]
    fn unknown_saved_skill_is_kept_but_ignored() {
        // Given a save with a skill and a perk the pack removed
        let defs = defs();
        let mut state = at_level(OLD_SKILL, 9);
        state
            .skills
            .get_mut(OLD_SKILL)
            .unwrap()
            .perks
            .insert("oldpack:quick_dig".into());

        // When Mining is raised, the old skill is asked for, and the pack later adds the skill back
        let raised = apply(&defs, &state, PLENTY, &raise(MINING)).unwrap();
        let old_raise = apply(&defs, &state, PLENTY, &raise(OLD_SKILL));
        let mut restored = defs.clone();
        restored.skills.insert(OLD_SKILL.into(), skill(OLD_SKILL));

        // Then the old entry survives the change, cannot be raised, grants no perk, and counts again once restored
        assert_eq!(raised.state.skills[OLD_SKILL], state.skills[OLD_SKILL]);
        assert_eq!(old_raise, Err(Refusal::UnknownSkill));
        assert!(active_perks(&defs, &state).is_empty());
        assert_eq!(level(&raised.state, &restored, OLD_SKILL), 9);
    }
}
