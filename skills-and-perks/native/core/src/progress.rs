//! A player's skills, perks and focus, what raising and buying cost, and the requests that change them.
//!
//! Every function here is pure: a request yields a new state and the XP levels to take, or a refusal, and the
//! caller applies it. Saved entries for skills or perks a pack removed are kept as they are and ignored, so adding
//! them back restores them.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::defs::PerkDef;
use crate::registry::Defs;

/// The save format of [`PlayerState`].
pub const STATE_FORMAT: u32 = 1;
const PERCENT: u64 = 100;
const MIN_LEVEL: u32 = 1;
const MIN_COST: u32 = 1;

/// Everything Skills & Perks keeps for one player.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub format: u32,
    /// Skill id to progress. Absent means level 1, nothing bought.
    pub skills: BTreeMap<String, SkillProgress>,
    pub focus: Option<String>,
    /// Whether a focus was ever picked, so only the first pick is free.
    pub focus_chosen_before: bool,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            format: STATE_FORMAT,
            skills: BTreeMap::new(),
            focus: None,
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
}

/// What a player asks the server to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Raise { skill: String },
    BuyPerk { perk: String },
    SetFocus { focus: String },
}

/// Why a request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    UnknownSkill,
    UnknownPerk,
    UnknownFocus,
    AtMax,
    NotEnoughLevels { needed: u32 },
    NotEnoughPoints { needed: u32 },
    BelowMinLevel { needed: u32 },
    MissingPerk { perk: String },
    AlreadyOwned,
    AlreadyFocused,
}

/// A granted request: the new state and the XP levels to take for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub state: PlayerState,
    pub levels_spent: u32,
}

/// The skill's level: 1 when never raised, and never above the skill's current maximum.
pub fn level(state: &PlayerState, defs: &Defs, skill: &str) -> u32 {
    let saved = state
        .skills
        .get(skill)
        .map_or(MIN_LEVEL, |progress| progress.level);
    saved.clamp(MIN_LEVEL, defs.max_level(skill).max(MIN_LEVEL))
}

/// XP levels the next level of the skill costs, after the focus discount; `None` at the maximum or for an unknown
/// skill.
pub fn raise_cost(defs: &Defs, state: &PlayerState, skill: &str) -> Option<u32> {
    defs.skill(skill)?;
    let current = level(state, defs, skill);
    if current >= defs.max_level(skill) {
        return None;
    }
    let curve = defs.cost_curve(skill);
    let cost = curve.base.saturating_add((current - 1) / curve.every);
    let focused = state.focus.is_some() && state.focus.as_deref() == defs.focus_of(skill);
    let cost = if focused {
        discounted(cost, defs.settings.focus_discount_percent)
    } else {
        cost
    };
    Some(cost.max(MIN_COST))
}

/// The cost minus `percent` of it, the discount rounded up.
fn discounted(cost: u32, percent: u32) -> u32 {
    let discount = (u64::from(cost) * u64::from(percent)).div_ceil(PERCENT);
    cost.saturating_sub(u32::try_from(discount).unwrap_or(u32::MAX))
}

/// Perk points earned by the skill's level, plus granted ones, minus spent ones.
pub fn points_available(defs: &Defs, state: &PlayerState, skill: &str) -> u32 {
    let earned = level(state, defs, skill) / defs.points_every(skill);
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
    match req {
        Request::Raise { skill } => raise(defs, state, xp_levels, skill),
        Request::BuyPerk { perk } => buy_perk(defs, state, perk),
        Request::SetFocus { focus } => set_focus(defs, state, xp_levels, focus),
    }
}

fn raise(
    defs: &Defs,
    state: &PlayerState,
    xp_levels: u32,
    skill: &str,
) -> Result<Outcome, Refusal> {
    defs.skill(skill).ok_or(Refusal::UnknownSkill)?;
    let cost = raise_cost(defs, state, skill).ok_or(Refusal::AtMax)?;
    if xp_levels < cost {
        return Err(Refusal::NotEnoughLevels { needed: cost });
    }
    let mut next = state.clone();
    next.skills.entry(skill.to_string()).or_default().level = level(state, defs, skill) + 1;
    Ok(Outcome {
        state: next,
        levels_spent: cost,
    })
}

fn buy_perk(defs: &Defs, state: &PlayerState, id: &str) -> Result<Outcome, Refusal> {
    let perk = defs.perks.get(id).ok_or(Refusal::UnknownPerk)?;
    if owns(state, id) {
        return Err(Refusal::AlreadyOwned);
    }
    if level(state, defs, &perk.skill) < perk.min_level {
        return Err(Refusal::BelowMinLevel {
            needed: perk.min_level,
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
    })
}

fn set_focus(
    defs: &Defs,
    state: &PlayerState,
    xp_levels: u32,
    focus: &str,
) -> Result<Outcome, Refusal> {
    if !defs.focus.contains_key(focus) {
        return Err(Refusal::UnknownFocus);
    }
    if state.focus.as_deref() == Some(focus) {
        return Err(Refusal::AlreadyFocused);
    }
    let free = defs.settings.focus_first_free && !state.focus_chosen_before;
    let cost = if free {
        0
    } else {
        defs.settings.focus_switch_cost
    };
    if xp_levels < cost {
        return Err(Refusal::NotEnoughLevels { needed: cost });
    }
    let mut next = state.clone();
    next.focus = Some(focus.to_string());
    next.focus_chosen_before = true;
    Ok(Outcome {
        state: next,
        levels_spent: cost,
    })
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
            progress.level = level(state, defs, skill)
                .saturating_sub(penalty)
                .max(MIN_LEVEL);
        }
    }
    next
}

/// Bought perks whose skill level still meets their minimum, in perk id order.
pub fn active_perks<'a>(defs: &'a Defs, state: &PlayerState) -> Vec<&'a PerkDef> {
    defs.perks
        .values()
        .filter(|perk| owns(state, &perk.id) && level(state, defs, &perk.skill) >= perk.min_level)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::{CostCurve, FocusDef, PerkDef, SkillDef};

    const MINING: &str = "skills_and_perks:mining";
    const MELEE: &str = "skills_and_perks:melee";
    const WORKING: &str = "skills_and_perks:working";
    const FIGHTING: &str = "skills_and_perks:fighting";
    const SURE_GRIP: &str = "skills_and_perks:mining_sure_grip";
    const DEEP_VEINS: &str = "skills_and_perks:mining_deep_veins";
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
            effects: vec![],
        }
    }

    /// Mining (in Working) and Melee (in Fighting) with default settings, and two Mining perks: Sure Grip (1 point,
    /// level 1) and Deep Veins (2 points, level 8, needs Sure Grip).
    fn defs() -> Defs {
        let focus = |id: &str, skill: &str| FocusDef {
            id: id.into(),
            priority: 0,
            name: "n".into(),
            skills: vec![skill.into()],
        };
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
        Request::Raise {
            skill: skill.into(),
        }
    }

    fn buy(perk: &str) -> Request {
        Request::BuyPerk { perk: perk.into() }
    }

    fn set_focus(focus: &str) -> Request {
        Request::SetFocus {
            focus: focus.into(),
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

        // Then it is 2 for levels 1 to 4, 3 at level 5, and 9 for levels 29 to 31
        assert_eq!(costs[0..4], [2, 2, 2, 2]);
        assert_eq!(costs[4], 3);
        assert_eq!(costs[28..31], [9, 9, 9]);
    }

    #[test]
    fn total_to_max_is_167() {
        // Given a new player with plenty of XP levels
        let defs = defs();
        let mut state = PlayerState::default();
        let mut spent = 0;

        // When Mining is raised until it refuses
        while let Ok(outcome) = apply(&defs, &state, PLENTY, &raise(MINING)) {
            spent += outcome.levels_spent;
            state = outcome.state;
        }

        // Then Mining is at 32 and it cost 167 levels in all
        assert_eq!(level(&state, &defs, MINING), 32);
        assert_eq!(spent, 167);
    }

    #[test]
    fn focus_discount_rounds_the_discount_up() {
        // Given flat costs of 2, 3, 4, 5 and 9, and a player focused on Working (Mining)
        let flat = |base: u32| {
            let mut defs = defs();
            defs.settings.cost = CostCurve { base, every: 1000 };
            defs
        };
        let mut focused = at_level(MINING, 1);
        focused.focus = Some(WORKING.into());

        // When the Mining and Melee raise costs are read
        let mining: Vec<u32> = [2, 3, 4, 5, 9]
            .map(|c| raise_cost(&flat(c), &focused, MINING).unwrap())
            .to_vec();
        let melee: Vec<u32> = [2, 3, 4, 5, 9]
            .map(|c| raise_cost(&flat(c), &focused, MELEE).unwrap())
            .to_vec();

        // Then the focused skill costs 25% less, the discount rounded up; the other skill pays full price
        assert_eq!(mining, vec![1, 2, 3, 3, 6]);
        assert_eq!(melee, vec![2, 3, 4, 5, 9]);
    }

    #[test]
    fn cost_is_never_below_1() {
        // Given a pack whose cost curve starts at 0, and a full discount
        let mut defs = defs();
        defs.settings.cost = CostCurve {
            base: 0,
            every: 1000,
        };
        defs.settings.focus_discount_percent = 100;
        let mut focused = at_level(MINING, 1);
        focused.focus = Some(WORKING.into());

        // When the costs are read
        let plain = raise_cost(&defs, &at_level(MELEE, 1), MELEE);
        let discounted = raise_cost(&defs, &focused, MINING);

        // Then both still cost 1
        assert_eq!((plain, discounted), (Some(1), Some(1)));
    }

    #[test]
    fn points_every_fourth_level() {
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

        // Then a point comes every 4th level, plus the granted ones, minus the spent ones
        assert_eq!(points, [0, 1, 8, 7]);
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

        // Then it costs 3 levels, Mining is at 6, and Melee is untouched
        assert_eq!(outcome.levels_spent, 3);
        assert_eq!(level(&outcome.state, &defs, MINING), 6);
        assert_eq!(level(&outcome.state, &defs, MELEE), 3);
    }

    #[test]
    fn raise_without_levels_is_refused_and_takes_nothing() {
        // Given a new player with 0 XP levels, then with 1 (the cost is 2)
        let defs = defs();
        let state = PlayerState::default();

        // When raise requests are spammed
        let with_none: Vec<_> = (0..100)
            .map(|_| apply(&defs, &state, 0, &raise(MINING)))
            .collect();
        let with_one = apply(&defs, &state, 1, &raise(MINING));

        // Then every one is refused with the levels needed, and the state stays at level 1
        assert!(
            with_none
                .iter()
                .all(|r| *r == Err(Refusal::NotEnoughLevels { needed: 2 }))
        );
        assert_eq!(with_one, Err(Refusal::NotEnoughLevels { needed: 2 }));
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
            &raise("skills_and_perks:nothing"),
        );

        // Then the first two are at the max (the saved 40 reads as 32) and the last is unknown
        assert_eq!(at_max, Err(Refusal::AtMax));
        assert_eq!(above_max, Err(Refusal::AtMax));
        assert_eq!(level(&at_level(MINING, 40), &defs, MINING), 32);
        assert_eq!(unknown, Err(Refusal::UnknownSkill));
    }

    #[test]
    fn perk_needs_points_min_level_and_required_perks() {
        // Given Mining at 7 with 1 point, at 8 with 2 points and nothing bought, and at 8 owning Sure Grip
        let defs = defs();
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
        let unknown = apply(&defs, &owns_grip, 0, &buy("skills_and_perks:nothing"));

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

    #[test]
    fn first_focus_is_free_then_switching_costs_10() {
        // Given a player who never picked a focus, with no XP levels
        let defs = defs();
        let state = PlayerState::default();

        // When they pick Working, then try Fighting with 9 levels and with 10, then Fighting again, then nonsense
        let first = apply(&defs, &state, 0, &set_focus(WORKING)).unwrap();
        let short = apply(&defs, &first.state, 9, &set_focus(FIGHTING));
        let switched = apply(&defs, &first.state, 10, &set_focus(FIGHTING)).unwrap();
        let same = apply(&defs, &switched.state, PLENTY, &set_focus(FIGHTING));
        let unknown = apply(
            &defs,
            &switched.state,
            PLENTY,
            &set_focus("skills_and_perks:nothing"),
        );

        // Then the first pick is free, a switch costs 10, and the rest are refused
        assert_eq!(first.levels_spent, 0);
        assert_eq!(first.state.focus.as_deref(), Some(WORKING));
        assert!(first.state.focus_chosen_before);
        assert_eq!(short, Err(Refusal::NotEnoughLevels { needed: 10 }));
        assert_eq!(switched.levels_spent, 10);
        assert_eq!(switched.state.focus.as_deref(), Some(FIGHTING));
        assert_eq!(same, Err(Refusal::AlreadyFocused));
        assert_eq!(unknown, Err(Refusal::UnknownFocus));
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
