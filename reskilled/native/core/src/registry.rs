//! Merges every data file from every namespace into the effective definitions.
//!
//! Skills, perks and focus areas are keyed by id: the highest `priority` wins, a tie goes to the file later in load
//! order. Lock files add up in load order, and an entry with `replace` drops the earlier entries for an equal
//! target. Settings merge field by field. A file or entry that is broken, or names something that does not exist,
//! is skipped with one [`DefError`] and never stops the rest from loading.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::defs::{
    CostCurve, DefError, DefFile, DefKind, FocusDef, LockEntry, LockFile, PerkDef, Settings,
    SettingsPatch, SkillDef, parse_file,
};

/// One data file as the game read it.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// `namespace:folder/name`.
    pub path: String,
    /// Position in load order: a later file wins ties.
    pub order: usize,
    pub kind: DefKind,
    pub json: String,
}

/// The effective definitions after every file is merged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Defs {
    pub skills: BTreeMap<String, SkillDef>,
    pub perks: BTreeMap<String, PerkDef>,
    pub focus: BTreeMap<String, FocusDef>,
    pub locks: Vec<LockEntry>,
    pub settings: Settings,
}

impl Defs {
    pub fn skill(&self, id: &str) -> Option<&SkillDef> {
        self.skills.get(id)
    }

    pub fn max_level(&self, skill: &str) -> u32 {
        self.skill(skill)
            .and_then(|s| s.max_level)
            .unwrap_or(self.settings.max_level)
    }

    pub fn cost_curve(&self, skill: &str) -> CostCurve {
        self.skill(skill)
            .and_then(|s| s.cost)
            .unwrap_or(self.settings.cost)
    }

    pub fn points_every(&self, skill: &str) -> u32 {
        self.skill(skill)
            .and_then(|s| s.points_every)
            .unwrap_or(self.settings.points_every)
    }

    /// The focus area the skill belongs to, if any.
    pub fn focus_of(&self, skill: &str) -> Option<&str> {
        self.focus
            .values()
            .find(|f| f.skills.iter().any(|s| s == skill))
            .map(|f| f.id.as_str())
    }
}

/// A definition with where it came from, while ids are resolved.
struct Ranked<T> {
    def: T,
    priority: i32,
    order: usize,
    path: String,
}

impl<T> Ranked<T> {
    fn outranks(&self, other: &Self) -> bool {
        (self.priority, self.order) > (other.priority, other.order)
    }

    fn error(&self, reason: String) -> DefError {
        DefError {
            file: self.path.clone(),
            reason,
        }
    }
}

/// Merges the files into effective definitions, with one error for each file or entry it skipped.
pub fn build(files: Vec<SourceFile>) -> (Defs, Vec<DefError>) {
    let mut errors = Vec::new();
    let mut skills = BTreeMap::new();
    let mut perks = BTreeMap::new();
    let mut focus = BTreeMap::new();
    let mut lock_files = Vec::new();
    let mut patches = Vec::new();

    for file in files {
        let parsed = match parse_file(file.kind, &file.json) {
            Ok(parsed) => parsed,
            Err(error) => {
                errors.push(DefError {
                    file: file.path,
                    ..error
                });
                continue;
            }
        };
        match parsed {
            DefFile::Skill(def) => {
                keep_highest(&mut skills, def.id.clone(), def.priority, def, &file)
            }
            DefFile::Perk(def) => {
                keep_highest(&mut perks, def.id.clone(), def.priority, def, &file)
            }
            DefFile::Focus(def) => {
                keep_highest(&mut focus, def.id.clone(), def.priority, def, &file)
            }
            DefFile::Lock(def) => lock_files.push(ranked(def, 0, &file)),
            DefFile::Settings(def) => {
                let priority = def.priority;
                patches.push(ranked(def, priority, &file));
            }
        }
    }

    let skills: BTreeMap<String, SkillDef> =
        skills.into_iter().map(|(id, r)| (id, r.def)).collect();
    let perks = resolve_perks(perks, &skills, &mut errors);
    let focus = resolve_focus(focus, &skills, &mut errors);
    let locks = merge_locks(lock_files, &skills, &mut errors);
    let settings = merge_settings(patches);
    (
        Defs {
            skills,
            perks,
            focus,
            locks,
            settings,
        },
        errors,
    )
}

fn ranked<T>(def: T, priority: i32, file: &SourceFile) -> Ranked<T> {
    Ranked {
        def,
        priority,
        order: file.order,
        path: file.path.clone(),
    }
}

fn keep_highest<T>(
    into: &mut BTreeMap<String, Ranked<T>>,
    id: String,
    priority: i32,
    def: T,
    file: &SourceFile,
) {
    let candidate = ranked(def, priority, file);
    match into.get(&id) {
        Some(kept) if !candidate.outranks(kept) => {}
        _ => {
            into.insert(id, candidate);
        }
    }
}

/// Drops perks whose skill is unknown, then, until nothing changes, perks requiring a perk that is not there.
fn resolve_perks(
    mut perks: BTreeMap<String, Ranked<PerkDef>>,
    skills: &BTreeMap<String, SkillDef>,
    errors: &mut Vec<DefError>,
) -> BTreeMap<String, PerkDef> {
    perks.retain(|id, perk| {
        let unknown = std::iter::once(&perk.def.skill)
            .chain(perk.def.also_needs.keys())
            .find(|skill| !skills.contains_key(*skill));
        if let Some(unknown) = unknown {
            errors.push(perk.error(format!("perk {id:?} names unknown skill {unknown:?}")));
        }
        unknown.is_none()
    });
    loop {
        let present: BTreeSet<String> = perks.keys().cloned().collect();
        let before = perks.len();
        perks.retain(
            |id, perk| match perk.def.requires.iter().find(|r| !present.contains(*r)) {
                Some(missing) => {
                    errors
                        .push(perk.error(format!("perk {id:?} requires unknown perk {missing:?}")));
                    false
                }
                None => true,
            },
        );
        if perks.len() == before {
            return perks.into_iter().map(|(id, r)| (id, r.def)).collect();
        }
    }
}

/// Drops focus areas naming an unknown skill, then, in load order, any area claiming a skill an earlier one has.
fn resolve_focus(
    focus: BTreeMap<String, Ranked<FocusDef>>,
    skills: &BTreeMap<String, SkillDef>,
    errors: &mut Vec<DefError>,
) -> BTreeMap<String, FocusDef> {
    let mut areas: Vec<Ranked<FocusDef>> = focus.into_values().collect();
    areas.sort_by_key(|area| area.order);
    let mut claimed: BTreeMap<String, String> = BTreeMap::new();
    let mut kept = BTreeMap::new();
    for area in areas {
        let id = &area.def.id;
        if let Some(unknown) = area.def.skills.iter().find(|s| !skills.contains_key(*s)) {
            errors.push(area.error(format!("focus {id:?} names unknown skill {unknown:?}")));
            continue;
        }
        if let Some((skill, owner)) = area
            .def
            .skills
            .iter()
            .find_map(|s| claimed.get(s).map(|owner| (s, owner)))
        {
            errors.push(area.error(format!(
                "focus {id:?} claims skill {skill:?}, already in focus {owner:?}"
            )));
            continue;
        }
        for skill in &area.def.skills {
            claimed.insert(skill.clone(), id.clone());
        }
        kept.insert(id.clone(), area.def);
    }
    kept
}

/// Applies lock files in load order. An entry naming an unknown skill is dropped before it can replace anything.
fn merge_locks(
    mut files: Vec<Ranked<LockFile>>,
    skills: &BTreeMap<String, SkillDef>,
    errors: &mut Vec<DefError>,
) -> Vec<LockEntry> {
    files.sort_by_key(|file| file.order);
    let mut locks: Vec<LockEntry> = Vec::new();
    for file in &files {
        for (index, entry) in file.def.entries.iter().enumerate() {
            if let Some(unknown) = entry.requires.keys().find(|s| !skills.contains_key(*s)) {
                errors.push(file.error(format!(
                    "lock entry {index} requires unknown skill {unknown:?}"
                )));
                continue;
            }
            if entry.replace {
                locks.retain(|earlier| earlier.target != entry.target);
            }
            locks.push(entry.clone());
        }
    }
    locks
}

/// Applies settings patches lowest priority first, ties in load order, so the highest priority sets each field; the
/// list fields add up instead.
fn merge_settings(mut patches: Vec<Ranked<SettingsPatch>>) -> Settings {
    patches.sort_by_key(|patch| (patch.priority, patch.order));
    let mut settings = Settings::default();
    for Ranked { def: patch, .. } in patches {
        let s = &mut settings;
        s.max_level = patch.max_level.unwrap_or(s.max_level);
        s.cost = patch.cost.unwrap_or(s.cost);
        s.points_every = patch.points_every.unwrap_or(s.points_every);
        s.focus_first_free = patch.focus_first_free.unwrap_or(s.focus_first_free);
        s.focus_cost = patch.focus_cost.unwrap_or(s.focus_cost);
        s.death_penalty_levels = patch.death_penalty_levels.unwrap_or(s.death_penalty_levels);
        s.creative_ignores_locks = patch
            .creative_ignores_locks
            .unwrap_or(s.creative_ignores_locks);
        s.lock_message_cooldown_ticks = patch
            .lock_message_cooldown_ticks
            .unwrap_or(s.lock_message_cooldown_ticks);
        s.not_creatures.extend(patch.not_creatures);
        s.projectile_damage.extend(patch.projectile_damage);
        s.equip_slots.extend(patch.equip_slots);
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::{Action, EquipSlotRule, Matcher, Target, WornSlot};

    fn source(path: &str, order: usize, kind: DefKind, json: &str) -> SourceFile {
        SourceFile {
            path: path.into(),
            order,
            kind,
            json: json.into(),
        }
    }

    fn skill(path: &str, order: usize, id: &str, priority: i32, icon: &str) -> SourceFile {
        let json = format!(
            r#"{{"format": 1, "id": "{id}", "priority": {priority}, "name": "n", "icon": "{icon}"}}"#
        );
        source(path, order, DefKind::Skill, &json)
    }

    fn perk(path: &str, order: usize, id: &str, skill: &str, requires: &str) -> SourceFile {
        let json = format!(
            r#"{{"format": 1, "id": "{id}", "skill": "{skill}", "name": "n", "description": "d", "cost": 1,
                "min_level": 1, "requires": [{requires}], "effects": []}}"#
        );
        source(path, order, DefKind::Perk, &json)
    }

    fn focus(path: &str, order: usize, id: &str, skills: &str) -> SourceFile {
        let json = format!(r#"{{"format": 1, "id": "{id}", "name": "n", "skills": [{skills}]}}"#);
        source(path, order, DefKind::Focus, &json)
    }

    fn lock(
        path: &str,
        order: usize,
        item: &str,
        skill: &str,
        level: u32,
        replace: bool,
    ) -> SourceFile {
        let json = format!(
            r#"{{"format": 1, "entries": [{{"target": {{"item": {{"id": "{item}"}}}}, "actions": ["use"],
                "requires": {{"{skill}": {level}}}, "replace": {replace}}}]}}"#
        );
        source(path, order, DefKind::Lock, &json)
    }

    const MINING: &str = "reskilled:mining";
    const MELEE: &str = "reskilled:melee";

    #[test]
    fn higher_priority_wins() {
        // Given the same skill twice, the later file with the lower priority
        let files = vec![
            skill(
                "pack:reskilled/skills/mining",
                0,
                MINING,
                5,
                "minecraft:diamond_pickaxe",
            ),
            skill(
                "reskilled:reskilled/skills/mining",
                1,
                MINING,
                0,
                "minecraft:iron_pickaxe",
            ),
        ];

        // When they are merged
        let (defs, errors) = build(files);

        // Then the higher priority one is kept, with no error
        assert_eq!(
            defs.skill(MINING).map(|s| s.icon.as_str()),
            Some("minecraft:diamond_pickaxe")
        );
        assert_eq!(errors, vec![]);
    }

    #[test]
    fn tie_goes_to_later_file() {
        // Given the same skill twice at the same priority
        let files = vec![
            skill(
                "b:reskilled/skills/mining",
                7,
                MINING,
                0,
                "minecraft:golden_pickaxe",
            ),
            skill(
                "a:reskilled/skills/mining",
                3,
                MINING,
                0,
                "minecraft:iron_pickaxe",
            ),
        ];

        // When they are merged
        let (defs, _) = build(files);

        // Then the one later in load order is kept, whatever the vector's order
        assert_eq!(
            defs.skill(MINING).map(|s| s.icon.as_str()),
            Some("minecraft:golden_pickaxe")
        );
    }

    #[test]
    fn replace_drops_earlier_entries_for_the_same_target() {
        // Given a mining skill, two locks on the mace, then a replacing lock on it, and a lock on another item
        let files = vec![
            skill("s:reskilled/skills/mining", 0, MINING, 0, "minecraft:stone"),
            skill("s:reskilled/skills/melee", 1, MELEE, 0, "minecraft:stone"),
            lock(
                "a:reskilled/locks/one",
                2,
                "minecraft:mace",
                MELEE,
                20,
                false,
            ),
            lock(
                "a:reskilled/locks/two",
                3,
                "minecraft:mace",
                MINING,
                5,
                false,
            ),
            lock(
                "a:reskilled/locks/other",
                4,
                "minecraft:trident",
                MELEE,
                16,
                false,
            ),
            lock(
                "b:reskilled/locks/three",
                5,
                "minecraft:mace",
                MELEE,
                10,
                true,
            ),
        ];

        // When they are merged
        let (defs, _) = build(files);

        // Then only the replacing entry is left for the mace, and the trident keeps its own
        let mace = Target::Item(Matcher::Id("minecraft:mace".into()));
        let mace_entries: Vec<_> = defs.locks.iter().filter(|e| e.target == mace).collect();
        assert_eq!(mace_entries.len(), 1);
        assert_eq!(mace_entries[0].requires, [(MELEE.to_string(), 10)].into());
        assert_eq!(mace_entries[0].actions, vec![Action::Use]);
        assert_eq!(defs.locks.len(), 2);
    }

    #[test]
    fn settings_merge_field_by_field_by_priority() {
        // Given three settings files: a high-priority one, a low one loaded later, and a same-priority later one
        let files = vec![
            source(
                "pack:reskilled/settings/hard",
                0,
                DefKind::Settings,
                r#"{"format": 1, "priority": 10, "max_level": 50, "focus_cost": 20}"#,
            ),
            source(
                "reskilled:reskilled/settings/defaults",
                1,
                DefKind::Settings,
                r#"{"format": 1, "max_level": 32, "death_penalty_levels": 1, "focus_cost": 5}"#,
            ),
            source(
                "late:reskilled/settings/late",
                2,
                DefKind::Settings,
                r#"{"format": 1, "priority": 10, "focus_cost": 30}"#,
            ),
        ];

        // When they are merged
        let (defs, _) = build(files);

        // Then each field comes from the highest-priority file that sets it, ties going to the later file,
        // and fields nobody sets keep the defaults
        assert_eq!(defs.settings.max_level, 50);
        assert_eq!(defs.settings.focus_cost, 30);
        assert_eq!(defs.settings.death_penalty_levels, 1);
        assert_eq!(defs.settings.points_every, Settings::default().points_every);
    }

    #[test]
    fn settings_lists_add_up_across_files() {
        // Given the mod's defaults and another mod's settings file, each adding to the lists
        let files = vec![
            source(
                "reskilled:reskilled/settings/defaults",
                0,
                DefKind::Settings,
                r##"{"format": 1,
                    "not_creatures": [{"id": "minecraft:armor_stand"}],
                    "projectile_damage": [{"pattern": "*:*arrow*"}],
                    "equip_slots": [{"items": {"tag": "#minecraft:head_armor"}, "slot": "head"}]}"##,
            ),
            source(
                "othermod:reskilled/settings/othermod",
                1,
                DefKind::Settings,
                r#"{"format": 1, "priority": 5,
                    "not_creatures": [{"id": "othermod:statue"}],
                    "projectile_damage": [{"id": "othermod:dart"}],
                    "equip_slots": [{"items": {"id": "othermod:backpack"}, "slot": "chest"}]}"#,
            ),
        ];

        // When they are merged
        let (defs, errors) = build(files);

        // Then each list holds both files' entries, the mod's first
        assert_eq!(errors, vec![]);
        let s = &defs.settings;
        assert_eq!(
            s.not_creatures,
            vec![
                Matcher::Id("minecraft:armor_stand".into()),
                Matcher::Id("othermod:statue".into())
            ]
        );
        assert_eq!(
            s.projectile_damage,
            vec![
                Matcher::Pattern("*:*arrow*".into()),
                Matcher::Id("othermod:dart".into())
            ]
        );
        assert_eq!(
            s.equip_slots,
            vec![
                EquipSlotRule {
                    items: Matcher::Tag("#minecraft:head_armor".into()),
                    slot: WornSlot::Head
                },
                EquipSlotRule {
                    items: Matcher::Id("othermod:backpack".into()),
                    slot: WornSlot::Chest
                },
            ]
        );
    }

    #[test]
    fn bad_settings_list_entries_are_refused() {
        let refused = |json: &str| {
            let (_, errors) = build(vec![source(
                "p:reskilled/settings/x",
                0,
                DefKind::Settings,
                json,
            )]);
            errors
                .into_iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert!(
            refused(r##"{"format": 1, "not_creatures": [{"tag": "#minecraft:boats"}]}"##)
                .contains("tag")
        );
        assert!(
            refused(r#"{"format": 1, "equip_slots": [{"items": {"id": "a:b"}, "slot": "tail"}]}"#)
                .contains("tail")
        );
    }

    #[test]
    fn perk_with_unknown_skill_is_dropped_with_error() {
        // Given a perk naming a skill nobody defines
        let files = vec![perk("p:reskilled/perks/lost", 0, "p:lost", "p:nothing", "")];

        // When they are merged
        let (defs, errors) = build(files);

        // Then the perk is dropped and the error names the file and the skill
        assert!(defs.perks.is_empty());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "p:reskilled/perks/lost");
        assert!(
            errors[0].reason.contains("\"p:nothing\""),
            "{}",
            errors[0].reason
        );
    }

    #[test]
    fn perk_requiring_unknown_perk_is_dropped_with_error() {
        // Given a perk requiring a missing perk, and a perk requiring that first perk
        let files = vec![
            skill("s:reskilled/skills/mining", 0, MINING, 0, "minecraft:stone"),
            perk("p:reskilled/perks/a", 1, "p:a", MINING, r#""p:missing""#),
            perk("p:reskilled/perks/b", 2, "p:b", MINING, r#""p:a""#),
            perk("p:reskilled/perks/c", 3, "p:c", MINING, ""),
        ];

        // When they are merged
        let (defs, errors) = build(files);

        // Then both are dropped, each with its own error, and the unrelated perk stays
        assert_eq!(defs.perks.keys().collect::<Vec<_>>(), vec!["p:c"]);
        let files: Vec<_> = errors.iter().map(|e| e.file.as_str()).collect();
        assert_eq!(files, vec!["p:reskilled/perks/a", "p:reskilled/perks/b"]);
        assert!(
            errors[0].reason.contains("\"p:missing\""),
            "{}",
            errors[0].reason
        );
        assert!(errors[1].reason.contains("\"p:a\""), "{}", errors[1].reason);
    }

    #[test]
    fn perk_also_needing_an_unknown_skill_is_dropped_with_error() {
        // Given a perk that also needs a skill nobody defines
        let json = format!(
            r#"{{"format": 1, "id": "p:a", "skill": "{MINING}", "name": "n", "description": "d", "cost": 1,
                "min_level": 1, "also_needs": {{"p:nothing": 4}}, "effects": []}}"#
        );
        let files = vec![
            skill("s:reskilled/skills/mining", 0, MINING, 0, "minecraft:stone"),
            source("p:reskilled/perks/a", 1, DefKind::Perk, &json),
        ];

        // When they are merged
        let (defs, errors) = build(files);

        // Then it is dropped, saying which skill is unknown
        assert!(defs.perks.is_empty());
        assert!(
            errors[0].reason.contains("\"p:nothing\""),
            "{}",
            errors[0].reason
        );
    }

    #[test]
    fn focus_with_unknown_skill_is_dropped_with_error() {
        // Given a focus area naming a skill nobody defines
        let files = vec![
            skill("s:reskilled/skills/mining", 0, MINING, 0, "minecraft:stone"),
            focus(
                "f:reskilled/focus/working",
                1,
                "f:working",
                &format!(r#""{MINING}", "f:nothing""#),
            ),
        ];

        // When they are merged
        let (defs, errors) = build(files);

        // Then the focus area is dropped and the error names the skill
        assert!(defs.focus.is_empty());
        assert_eq!(defs.focus_of(MINING), None);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].reason.contains("\"f:nothing\""),
            "{}",
            errors[0].reason
        );
    }

    #[test]
    fn lock_requiring_unknown_skill_is_dropped_with_error() {
        // Given a lock entry requiring an unknown skill, set to replace a valid lock on the same item
        let files = vec![
            skill("s:reskilled/skills/melee", 0, MELEE, 0, "minecraft:stone"),
            lock(
                "a:reskilled/locks/mace",
                1,
                "minecraft:mace",
                MELEE,
                20,
                false,
            ),
            lock(
                "b:reskilled/locks/bad",
                2,
                "minecraft:mace",
                "b:nothing",
                3,
                true,
            ),
        ];

        // When they are merged
        let (defs, errors) = build(files);

        // Then the bad entry is dropped before it can replace anything, with an error naming the skill
        assert_eq!(defs.locks.len(), 1);
        assert_eq!(defs.locks[0].requires, [(MELEE.to_string(), 20)].into());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "b:reskilled/locks/bad");
        assert!(
            errors[0].reason.contains("\"b:nothing\""),
            "{}",
            errors[0].reason
        );
    }

    #[test]
    fn skill_in_two_focus_areas_is_an_error() {
        // Given two focus areas that both claim Mining, the second loaded later
        let files = vec![
            skill("s:reskilled/skills/mining", 0, MINING, 0, "minecraft:stone"),
            skill("s:reskilled/skills/melee", 1, MELEE, 0, "minecraft:stone"),
            focus(
                "f:reskilled/focus/working",
                2,
                "f:working",
                &format!(r#""{MINING}""#),
            ),
            focus(
                "f:reskilled/focus/fighting",
                3,
                "f:fighting",
                &format!(r#""{MELEE}", "{MINING}""#),
            ),
        ];

        // When they are merged
        let (defs, errors) = build(files);

        // Then the later focus area is dropped, with an error naming the shared skill
        assert_eq!(defs.focus.keys().collect::<Vec<_>>(), vec!["f:working"]);
        assert_eq!(defs.focus_of(MINING), Some("f:working"));
        assert_eq!(defs.focus_of(MELEE), None);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "f:reskilled/focus/fighting");
        assert!(errors[0].reason.contains(MINING), "{}", errors[0].reason);
    }

    #[test]
    fn bad_file_is_reported_and_others_still_load() {
        // Given a broken file between two good ones
        let files = vec![
            skill("s:reskilled/skills/mining", 0, MINING, 0, "minecraft:stone"),
            source("s:reskilled/skills/broken", 1, DefKind::Skill, "{ not json"),
            skill("s:reskilled/skills/melee", 2, MELEE, 0, "minecraft:stone"),
        ];

        // When they are merged
        let (defs, errors) = build(files);

        // Then the error names the broken file, and both good skills load
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "s:reskilled/skills/broken");
        assert!(!errors[0].reason.is_empty());
        assert_eq!(defs.skills.len(), 2);
    }

    #[test]
    fn per_skill_values_fall_back_to_settings() {
        // Given one skill overriding max level, cost and points, and one overriding nothing
        let files = vec![
            source(
                "s:reskilled/skills/mining",
                0,
                DefKind::Skill,
                &format!(
                    r#"{{"format": 1, "id": "{MINING}", "name": "n", "icon": "minecraft:stone", "max_level": 40,
                        "cost": {{"base": 3, "every": 5}}, "points_every": 5}}"#
                ),
            ),
            skill("s:reskilled/skills/melee", 1, MELEE, 0, "minecraft:stone"),
        ];

        // When they are merged
        let (defs, _) = build(files);

        // Then the overrides apply to Mining only, and Melee uses the settings
        assert_eq!(defs.max_level(MINING), 40);
        assert_eq!(defs.cost_curve(MINING), CostCurve { base: 3, every: 5 });
        assert_eq!(defs.points_every(MINING), 5);
        assert_eq!(defs.max_level(MELEE), defs.settings.max_level);
        assert_eq!(defs.cost_curve(MELEE), defs.settings.cost);
        assert_eq!(defs.points_every(MELEE), defs.settings.points_every);
    }
}
