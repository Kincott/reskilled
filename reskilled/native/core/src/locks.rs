//! Which lock entries apply to an item, block or entity, and whether a player's levels meet them.
//!
//! Tags and repair materials are answered by the game through [`Facts`], since only the server knows what a tag
//! holds. Requirements are worked out per id on first use and kept in a [`LockCache`] until the next data load.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

pub use crate::defs::TargetKind;
use crate::defs::{Action, LockEntry, Matcher, Target};
use crate::registry::Defs;

/// What the game knows about ids that data files cannot say.
pub trait Facts {
    /// Whether the item is in the tag (written without its `#`).
    fn item_in_tag(&self, item: &str, tag: &str) -> bool;
    /// Whether the block is in the tag (written without its `#`).
    fn block_in_tag(&self, block: &str, tag: &str) -> bool;
    /// The `items` of the item's default `minecraft:repairable` component: a `#tag` or an item id.
    fn item_repairable(&self, item: &str) -> Option<String>;
}

/// Skill levels needed per action on one target. Empty means nothing is locked.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Requirements {
    pub by_action: BTreeMap<Action, BTreeMap<String, u32>>,
}

/// The first requirement a player does not meet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmet {
    pub skill: String,
    pub level: u32,
}

/// Whether the matcher picks this id. The kind decides which tags are asked. Entity tags and repair materials on
/// anything but items are refused when the data files load, so they never reach this.
pub fn matches(m: &Matcher, kind: TargetKind, id: &str, facts: &dyn Facts) -> bool {
    match m {
        Matcher::Id(wanted) => wanted == id,
        Matcher::Pattern(pattern) => wildcard_match(pattern, id),
        Matcher::Tag(tag) => {
            let tag = tag.strip_prefix('#').unwrap_or(tag);
            match kind {
                TargetKind::Item => facts.item_in_tag(id, tag),
                TargetKind::Block => facts.block_in_tag(id, tag),
                TargetKind::Entity | TargetKind::Enchantment | TargetKind::Ability => false,
            }
        }
        Matcher::Repairable(material) => facts.item_repairable(id).as_deref() == Some(material),
        Matcher::AllOf(all) => all.iter().all(|m| matches(m, kind, id, facts)),
        Matcher::AnyOf(any) => any.iter().any(|m| matches(m, kind, id, facts)),
    }
}

/// `*` stands for any run of characters, the empty one included; everything else must be equal.
fn wildcard_match(pattern: &str, id: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = id.strip_prefix(first) else {
        return false;
    };
    let middle_and_last: Vec<&str> = parts.collect();
    let Some((last, middle)) = middle_and_last.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// Every lock entry for this kind that matches the id, merged: per action and skill, the highest level.
pub fn requirements_for(
    defs: &Defs,
    kind: TargetKind,
    id: &str,
    facts: &dyn Facts,
) -> Requirements {
    let mut reqs = Requirements::default();
    for entry in &defs.locks {
        let (entry_kind, matcher) = entry.target.parts();
        if entry_kind != kind || !matches(matcher, kind, id, facts) {
            continue;
        }
        for action in &entry.actions {
            let levels = reqs.by_action.entry(*action).or_default();
            for (skill, level) in &entry.requires {
                let kept = levels.entry(skill.clone()).or_default();
                *kept = (*kept).max(*level);
            }
        }
    }
    reqs
}

/// What the enchantments on an item add to its requirements: every enchantment lock whose matcher picks one of
/// them at or above the level it starts at, merged as [`requirements_for`] merges.
pub fn enchantment_requirements(
    entries: &[LockEntry],
    enchantments: &[(String, u32)],
) -> Requirements {
    let mut reqs = Requirements::default();
    for entry in entries {
        let Target::Enchantment(matcher) = &entry.target else {
            continue;
        };
        let from = entry.enchantment_level.unwrap_or(1);
        let applies = enchantments.iter().any(|(id, level)| {
            *level >= from && matches(matcher, TargetKind::Enchantment, id, &NoFacts)
        });
        if applies {
            reqs.add(&entry.actions, &entry.requires);
        }
    }
    reqs
}

/// The enchantment of `enchantments` (id and the level the item has it at) whose lock asks `level` of `skill` for
/// `action`: the reason an enchanted item needs that much. None when no enchantment lock on the item asks it.
pub fn enchantment_cause(
    entries: &[LockEntry],
    enchantments: &[(String, u32)],
    action: Action,
    skill: &str,
    level: u32,
) -> Option<(String, u32)> {
    entries
        .iter()
        .filter(|entry| {
            entry.actions.contains(&action) && entry.requires.get(skill) == Some(&level)
        })
        .find_map(|entry| {
            let Target::Enchantment(matcher) = &entry.target else {
                return None;
            };
            let from = entry.enchantment_level.unwrap_or(1);
            enchantments
                .iter()
                .find(|(id, at)| {
                    *at >= from && matches(matcher, TargetKind::Enchantment, id, &NoFacts)
                })
                .cloned()
        })
}

/// For matchers that ask no tags (enchantments): every question answers no.
struct NoFacts;

impl Facts for NoFacts {
    fn item_in_tag(&self, _item: &str, _tag: &str) -> bool {
        false
    }
    fn block_in_tag(&self, _block: &str, _tag: &str) -> bool {
        false
    }
    fn item_repairable(&self, _item: &str) -> Option<String> {
        None
    }
}

impl Requirements {
    /// Adds the levels for these actions, keeping the highest per action and skill.
    pub fn add(&mut self, actions: &[Action], requires: &BTreeMap<String, u32>) {
        for action in actions {
            let levels = self.by_action.entry(*action).or_default();
            for (skill, level) in requires {
                let kept = levels.entry(skill.clone()).or_default();
                *kept = (*kept).max(*level);
            }
        }
    }

    /// Both sets of requirements together.
    pub fn merged(mut self, other: &Requirements) -> Requirements {
        for (action, levels) in &other.by_action {
            let levels: BTreeMap<String, u32> = levels.clone();
            self.add(&[*action], &levels);
        }
        self
    }
}

/// Requirements per target, worked out on first use. Clear it whenever the data reloads.
#[derive(Debug, Default)]
pub struct LockCache {
    by_target: HashMap<(TargetKind, String), Requirements>,
}

impl LockCache {
    pub fn get(
        &mut self,
        defs: &Defs,
        kind: TargetKind,
        id: &str,
        facts: &dyn Facts,
    ) -> &Requirements {
        self.by_target
            .entry((kind, id.to_string()))
            .or_insert_with(|| requirements_for(defs, kind, id, facts))
    }

    pub fn clear(&mut self) {
        self.by_target.clear();
    }
}

/// Whether the levels meet the action's requirements; if not, the first unmet one in skill id order.
pub fn check(
    reqs: &Requirements,
    action: Action,
    levels: &dyn Fn(&str) -> u32,
) -> Result<(), Unmet> {
    let Some(needed) = reqs.by_action.get(&action) else {
        return Ok(());
    };
    match needed.iter().find(|(skill, level)| levels(skill) < **level) {
        Some((skill, level)) => Err(Unmet {
            skill: skill.clone(),
            level: *level,
        }),
        None => Ok(()),
    }
}

/// The lock labels that need this skill, grouped by the level they need, each label once per level.
pub fn labels_by_level(defs: &Defs, skill: &str) -> BTreeMap<u32, Vec<String>> {
    let mut labels: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for entry in &defs.locks {
        let (Some(label), Some(level)) = (&entry.label, entry.requires.get(skill)) else {
            continue;
        };
        let at_level = labels.entry(*level).or_default();
        if !at_level.contains(label) {
            at_level.push(label.clone());
        }
    }
    labels
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::defs::{LockEntry, Matcher, Target};

    const MINING: &str = "reskilled:mining";
    const MELEE: &str = "reskilled:melee";
    const DEFENSE: &str = "reskilled:defense";

    /// Vanilla facts the tests need, counting tag lookups so the cache can be checked.
    #[derive(Default)]
    struct TestFacts {
        item_tags: Vec<(&'static str, &'static str)>,
        block_tags: Vec<(&'static str, &'static str)>,
        repairable: Vec<(&'static str, &'static str)>,
        lookups: Cell<u32>,
    }

    impl Facts for TestFacts {
        fn item_in_tag(&self, item: &str, tag: &str) -> bool {
            self.lookups.set(self.lookups.get() + 1);
            self.item_tags.iter().any(|(i, t)| *i == item && *t == tag)
        }
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {
            self.lookups.set(self.lookups.get() + 1);
            self.block_tags
                .iter()
                .any(|(b, t)| *b == block && *t == tag)
        }
        fn item_repairable(&self, item: &str) -> Option<String> {
            self.repairable
                .iter()
                .find(|(i, _)| *i == item)
                .map(|(_, m)| m.to_string())
        }
    }

    fn id(value: &str) -> Matcher {
        Matcher::Id(value.into())
    }

    fn entry(
        target: Target,
        actions: &[Action],
        requires: &[(&str, u32)],
        label: Option<&str>,
    ) -> LockEntry {
        LockEntry {
            target,
            actions: actions.to_vec(),
            requires: requires.iter().map(|(s, l)| (s.to_string(), *l)).collect(),
            label: label.map(str::to_string),
            replace: false,
            enchantment_level: None,
        }
    }

    fn defs_with(locks: Vec<LockEntry>) -> Defs {
        Defs {
            locks,
            ..Defs::default()
        }
    }

    #[test]
    fn id_matches_exactly() {
        // Given an id matcher for the mace
        let facts = TestFacts::default();
        let mace = id("minecraft:mace");

        // When it is tested against the mace and near misses
        let results = [
            "minecraft:mace",
            "minecraft:mace2",
            "minecraft:mac",
            "other:mace",
        ]
        .map(|candidate| matches(&mace, TargetKind::Item, candidate, &facts));

        // Then only the exact id matches
        assert_eq!(results, [true, false, false, false]);
    }

    #[test]
    fn pattern_star_matches_any_run() {
        // Given a one-star and a two-star pattern
        let facts = TestFacts::default();
        let spear = Matcher::Pattern("minecraft:*_spear".into());
        let copper = Matcher::Pattern("minecraft:*copper*".into());

        // When they are tested
        let spears = [
            "minecraft:iron_spear",
            "minecraft:_spear",
            "minecraft:iron_spears",
            "other:iron_spear",
        ]
        .map(|candidate| matches(&spear, TargetKind::Item, candidate, &facts));
        let coppers = [
            "minecraft:exposed_copper_door",
            "minecraft:copper",
            "minecraft:iron_door",
        ]
        .map(|candidate| matches(&copper, TargetKind::Item, candidate, &facts));

        // Then `*` stands for any run, the empty one included, and the rest must match exactly
        assert_eq!(spears, [true, true, false, false]);
        assert_eq!(coppers, [true, true, false]);
    }

    #[test]
    fn tag_uses_facts() {
        // Given facts where the diamond pickaxe is a pickaxe and stone is mineable with one
        let facts = TestFacts {
            item_tags: vec![("minecraft:diamond_pickaxe", "minecraft:pickaxes")],
            block_tags: vec![("minecraft:stone", "minecraft:mineable/pickaxe")],
            ..TestFacts::default()
        };
        let pickaxes = Matcher::Tag("#minecraft:pickaxes".into());
        let mineable = Matcher::Tag("#minecraft:mineable/pickaxe".into());

        // When tag matchers are tested on items and blocks
        let diamond = matches(
            &pickaxes,
            TargetKind::Item,
            "minecraft:diamond_pickaxe",
            &facts,
        );
        let sword = matches(
            &pickaxes,
            TargetKind::Item,
            "minecraft:diamond_sword",
            &facts,
        );
        let stone = matches(&mineable, TargetKind::Block, "minecraft:stone", &facts);
        let stone_as_item = matches(&mineable, TargetKind::Item, "minecraft:stone", &facts);

        // Then items ask the item tags and blocks the block tags, with the `#` taken off
        assert_eq!(
            [diamond, sword, stone, stone_as_item],
            [true, false, true, false]
        );
    }

    #[test]
    fn repairable_uses_facts() {
        // Given facts where the diamond sword is repaired with the diamond tool materials tag
        let facts = TestFacts {
            repairable: vec![
                (
                    "minecraft:diamond_sword",
                    "#minecraft:diamond_tool_materials",
                ),
                ("minecraft:elytra", "minecraft:phantom_membrane"),
            ],
            ..TestFacts::default()
        };
        let diamond = Matcher::Repairable("#minecraft:diamond_tool_materials".into());
        let membrane = Matcher::Repairable("minecraft:phantom_membrane".into());

        // When repairable matchers are tested
        let results = [
            matches(
                &diamond,
                TargetKind::Item,
                "minecraft:diamond_sword",
                &facts,
            ),
            matches(&diamond, TargetKind::Item, "minecraft:iron_sword", &facts),
            matches(&membrane, TargetKind::Item, "minecraft:elytra", &facts),
        ];

        // Then the item's repair material must equal the matcher's
        assert_eq!(results, [true, false, true]);
    }

    #[test]
    fn all_of_and_any_of_combine() {
        // Given "a pickaxe repaired with diamonds" and "the mace or any spear"
        let facts = TestFacts {
            item_tags: vec![
                ("minecraft:diamond_pickaxe", "minecraft:pickaxes"),
                ("minecraft:iron_pickaxe", "minecraft:pickaxes"),
            ],
            repairable: vec![
                (
                    "minecraft:diamond_pickaxe",
                    "#minecraft:diamond_tool_materials",
                ),
                (
                    "minecraft:diamond_sword",
                    "#minecraft:diamond_tool_materials",
                ),
            ],
            ..TestFacts::default()
        };
        let diamond_pickaxe = Matcher::AllOf(vec![
            Matcher::Tag("#minecraft:pickaxes".into()),
            Matcher::Repairable("#minecraft:diamond_tool_materials".into()),
        ]);
        let heavy = Matcher::AnyOf(vec![
            id("minecraft:mace"),
            Matcher::Pattern("minecraft:*_spear".into()),
        ]);

        // When they are tested
        let all = [
            "minecraft:diamond_pickaxe",
            "minecraft:iron_pickaxe",
            "minecraft:diamond_sword",
        ]
        .map(|candidate| matches(&diamond_pickaxe, TargetKind::Item, candidate, &facts));
        let any = ["minecraft:mace", "minecraft:gold_spear", "minecraft:bow"]
            .map(|candidate| matches(&heavy, TargetKind::Item, candidate, &facts));

        // Then all_of needs every part and any_of one
        assert_eq!(all, [true, false, false]);
        assert_eq!(any, [true, true, false]);
    }

    #[test]
    fn matchers_respect_target_kind() {
        // Given an item lock on the anvil item and a block lock on the anvil block
        let facts = TestFacts::default();
        let defs = defs_with(vec![
            entry(
                Target::Item(id("minecraft:anvil")),
                &[Action::Use],
                &[(MINING, 3)],
                None,
            ),
            entry(
                Target::Block(id("minecraft:anvil")),
                &[Action::Open],
                &[(MINING, 6)],
                None,
            ),
        ]);

        // When the requirements are read for the item, the block and an entity of the same id
        let item = requirements_for(&defs, TargetKind::Item, "minecraft:anvil", &facts);
        let block = requirements_for(&defs, TargetKind::Block, "minecraft:anvil", &facts);
        let entity = requirements_for(&defs, TargetKind::Entity, "minecraft:anvil", &facts);

        // Then each kind only sees its own lock
        assert_eq!(
            item.by_action.keys().collect::<Vec<_>>(),
            vec![&Action::Use]
        );
        assert_eq!(
            block.by_action.keys().collect::<Vec<_>>(),
            vec![&Action::Open]
        );
        assert_eq!(entity, Requirements::default());
    }

    #[test]
    fn highest_level_wins_across_entries() {
        // Given entries locking the diamond pickaxe's break at Mining 20, then at Mining 16 (a later, lower one),
        // also its attack at Melee 8, and an entry for another item
        let facts = TestFacts {
            item_tags: vec![("minecraft:diamond_pickaxe", "minecraft:pickaxes")],
            ..TestFacts::default()
        };
        let defs = defs_with(vec![
            entry(
                Target::Item(id("minecraft:diamond_pickaxe")),
                &[Action::Break, Action::Attack],
                &[(MINING, 20), (MELEE, 8)],
                None,
            ),
            entry(
                Target::Item(Matcher::Tag("#minecraft:pickaxes".into())),
                &[Action::Break],
                &[(MINING, 16)],
                None,
            ),
            entry(
                Target::Item(id("minecraft:iron_pickaxe")),
                &[Action::Break],
                &[(MINING, 30)],
                None,
            ),
        ]);

        // When the diamond pickaxe's requirements are read
        let reqs = requirements_for(&defs, TargetKind::Item, "minecraft:diamond_pickaxe", &facts);

        // Then each action keeps the highest level per skill from the entries that match
        let expected: BTreeMap<Action, BTreeMap<String, u32>> = [
            (
                Action::Attack,
                [(MINING.to_string(), 20), (MELEE.to_string(), 8)].into(),
            ),
            (
                Action::Break,
                [(MINING.to_string(), 20), (MELEE.to_string(), 8)].into(),
            ),
        ]
        .into();
        assert_eq!(reqs.by_action, expected);
    }

    #[test]
    fn check_reports_first_unmet_in_id_order() {
        // Given a lock needing Melee 8 and Defense 5 to attack
        let reqs = Requirements {
            by_action: [(
                Action::Attack,
                [(MELEE.to_string(), 8), (DEFENSE.to_string(), 5)].into(),
            )]
            .into(),
        };
        let levels = |defense: u32, melee: u32| {
            move |skill: &str| match skill {
                DEFENSE => defense,
                MELEE => melee,
                _ => 1,
            }
        };

        // When it is checked at several levels, and for an action it does not lock
        let both_low = check(&reqs, Action::Attack, &levels(1, 1));
        let melee_low = check(&reqs, Action::Attack, &levels(5, 7));
        let both_met = check(&reqs, Action::Attack, &levels(5, 8));
        let other_action = check(&reqs, Action::Use, &levels(1, 1));

        // Then the first unmet skill in id order is reported, and nothing when all are met
        assert_eq!(
            both_low,
            Err(Unmet {
                skill: DEFENSE.into(),
                level: 5
            })
        );
        assert_eq!(
            melee_low,
            Err(Unmet {
                skill: MELEE.into(),
                level: 8
            })
        );
        assert_eq!(both_met, Ok(()));
        assert_eq!(other_action, Ok(()));
    }

    #[test]
    fn cache_clears() {
        // Given a tag lock and a cache
        let facts = TestFacts {
            item_tags: vec![("minecraft:diamond_pickaxe", "minecraft:pickaxes")],
            ..TestFacts::default()
        };
        let defs = defs_with(vec![entry(
            Target::Item(Matcher::Tag("#minecraft:pickaxes".into())),
            &[Action::Break],
            &[(MINING, 16)],
            None,
        )]);
        let mut cache = LockCache::default();

        // When the same item is read twice, then again after a clear
        let first = cache
            .get(&defs, TargetKind::Item, "minecraft:diamond_pickaxe", &facts)
            .clone();
        let second = cache
            .get(&defs, TargetKind::Item, "minecraft:diamond_pickaxe", &facts)
            .clone();
        let lookups_before_clear = facts.lookups.get();
        cache.clear();
        cache.get(&defs, TargetKind::Item, "minecraft:diamond_pickaxe", &facts);

        // Then the second read is served from the cache, and the clear makes it ask the facts again
        assert_eq!(first, second);
        assert_eq!(first.by_action[&Action::Break][MINING], 16);
        assert_eq!(lookups_before_clear, 1);
        assert_eq!(facts.lookups.get(), 2);
    }

    #[test]
    fn labels_group_by_level() {
        // Given labelled locks at Mining 8 (twice, same label) and 16, one at Defense 16, and an unlabelled one
        let defs = defs_with(vec![
            entry(
                Target::Item(id("minecraft:iron_pickaxe")),
                &[Action::Break],
                &[(MINING, 8)],
                Some("l.iron_tools"),
            ),
            entry(
                Target::Item(id("minecraft:iron_shovel")),
                &[Action::Break],
                &[(MINING, 8)],
                Some("l.iron_tools"),
            ),
            entry(
                Target::Item(id("minecraft:diamond_pickaxe")),
                &[Action::Break],
                &[(MINING, 16)],
                Some("l.diamond"),
            ),
            entry(
                Target::Item(id("minecraft:diamond_helmet")),
                &[Action::Equip],
                &[(DEFENSE, 16)],
                Some("l.armor"),
            ),
            entry(
                Target::Item(id("minecraft:copper_pickaxe")),
                &[Action::Break],
                &[(MINING, 4)],
                None,
            ),
        ]);

        // When Mining's labels are grouped
        let labels = labels_by_level(&defs, MINING);

        // Then each level lists its labels once, and other skills' and unlabelled locks are left out
        let expected: BTreeMap<u32, Vec<String>> = [
            (8, vec!["l.iron_tools".to_string()]),
            (16, vec!["l.diamond".to_string()]),
        ]
        .into();
        assert_eq!(labels, expected);
    }

    #[test]
    fn the_enchantment_behind_a_requirement_is_named() {
        // Given Sharpness locked for attacking at Melee 12 from level III and Melee 20 from level V
        let json = r#"{"format": 1, "entries": [
            {"target": {"enchantment": {"id": "minecraft:sharpness"}}, "enchantment_level": 3, "actions": ["attack"],
             "requires": {"reskilled:melee": 12}},
            {"target": {"enchantment": {"id": "minecraft:sharpness"}}, "enchantment_level": 5, "actions": ["attack"],
             "requires": {"reskilled:melee": 20}}]}"#;
        let crate::defs::DefFile::Lock(file) =
            crate::defs::parse_file(crate::defs::DefKind::Lock, json).unwrap()
        else {
            panic!("a lock file");
        };
        let sword = |level: u32| {
            vec![
                ("minecraft:unbreaking".to_string(), 3),
                ("minecraft:sharpness".to_string(), level),
            ]
        };
        let cause = |enchanted: &[(String, u32)], action, level| {
            enchantment_cause(&file.entries, enchanted, action, "reskilled:melee", level)
        };

        // When the requirement of Melee 12 or 20 to attack is traced on swords with Sharpness IV and V
        // Then the Sharpness that brings it is named, at the level the sword has it; a need no enchantment lock asks
        // (Melee 16, or Melee 12 to use) and a sword below the starting level have none
        assert_eq!(
            cause(&sword(4), Action::Attack, 12),
            Some(("minecraft:sharpness".to_string(), 4))
        );
        assert_eq!(
            cause(&sword(5), Action::Attack, 20),
            Some(("minecraft:sharpness".to_string(), 5))
        );
        assert_eq!(cause(&sword(5), Action::Attack, 16), None);
        assert_eq!(cause(&sword(4), Action::Use, 12), None);
        assert_eq!(cause(&sword(2), Action::Attack, 12), None);
    }

    #[test]
    fn enchantments_add_their_locks_from_the_level_they_name() {
        // Given Sharpness locked for attacking at Melee 12 from level III and Melee 20 from level V, and Mending
        // locked for every use at Arcane 16 from any level
        let json = r#"{"format": 1, "entries": [
            {"target": {"enchantment": {"id": "minecraft:sharpness"}}, "enchantment_level": 3, "actions": ["attack"],
             "requires": {"reskilled:melee": 12}},
            {"target": {"enchantment": {"id": "minecraft:sharpness"}}, "enchantment_level": 5, "actions": ["attack"],
             "requires": {"reskilled:melee": 20}},
            {"target": {"enchantment": {"pattern": "minecraft:mend*"}}, "actions": ["use", "attack", "break", "equip"],
             "requires": {"reskilled:arcane": 16}}]}"#;
        let crate::defs::DefFile::Lock(file) =
            crate::defs::parse_file(crate::defs::DefKind::Lock, json).unwrap()
        else {
            panic!("a lock file");
        };
        let enchanted = |list: &[(&str, u32)]| -> Vec<(String, u32)> {
            list.iter()
                .map(|(id, level)| ((*id).to_string(), *level))
                .collect()
        };

        // When swords with Sharpness II, IV and V and Mending are checked, and an item an enchantment lock does
        // not name
        let two =
            enchantment_requirements(&file.entries, &enchanted(&[("minecraft:sharpness", 2)]));
        let four =
            enchantment_requirements(&file.entries, &enchanted(&[("minecraft:sharpness", 4)]));
        let five_mending = enchantment_requirements(
            &file.entries,
            &enchanted(&[("minecraft:sharpness", 5), ("minecraft:mending", 1)]),
        );
        let item = requirements_for(
            &Defs {
                locks: file.entries.clone(),
                ..Defs::default()
            },
            TargetKind::Item,
            "minecraft:sharpness",
            &TestFacts::default(),
        );

        // Then II needs nothing, IV needs Melee 12, V with Mending needs Melee 20 to attack and Arcane 16 for
        // everything, and an enchantment lock never locks an item by id
        let attack = |r: &Requirements| {
            r.by_action
                .get(&Action::Attack)
                .cloned()
                .unwrap_or_default()
        };
        assert!(two.by_action.is_empty());
        assert_eq!(attack(&four), [("reskilled:melee".to_string(), 12)].into());
        assert_eq!(
            attack(&five_mending),
            [
                ("reskilled:arcane".to_string(), 16),
                ("reskilled:melee".to_string(), 20)
            ]
            .into()
        );
        assert_eq!(
            five_mending.by_action.get(&Action::Equip),
            Some(&[("reskilled:arcane".to_string(), 16)].into())
        );
        assert!(item.by_action.is_empty());
    }

    fn ability_locks() -> Defs {
        let json = r#"{"format": 1, "entries": [
            {"target": {"ability": {"id": "dashmod:dash"}}, "actions": ["use"],
             "requires": {"reskilled:agility": 10}, "label": "dashmod.lock.dash"},
            {"target": {"ability": {"pattern": "jumpmod:*_jump"}}, "actions": ["use"],
             "requires": {"reskilled:agility": 16}}]}"#;
        let crate::defs::DefFile::Lock(file) =
            crate::defs::parse_file(crate::defs::DefKind::Lock, json).unwrap()
        else {
            panic!("a lock file");
        };
        Defs {
            locks: file.entries,
            ..Defs::default()
        }
    }

    #[test]
    fn ability_requirements_are_matched_by_id_and_pattern() {
        // Given a dash at Agility 10 and every jumpmod "*_jump" at Agility 16
        let defs = ability_locks();
        let use_of = |id: &str| {
            requirements_for(&defs, TargetKind::Ability, id, &TestFacts::default())
                .by_action
                .get(&Action::Use)
                .and_then(|levels| levels.get("reskilled:agility"))
                .copied()
        };

        // When the dash, a double jump, another mod's dash and an item named like the dash are checked
        let item = requirements_for(
            &defs,
            TargetKind::Item,
            "dashmod:dash",
            &TestFacts::default(),
        );

        // Then the id and the pattern match their abilities only, and no item is locked by them
        assert_eq!(use_of("dashmod:dash"), Some(10));
        assert_eq!(use_of("jumpmod:double_jump"), Some(16));
        assert_eq!(use_of("othermod:dash"), None);
        assert!(item.by_action.is_empty());
    }

    #[test]
    fn ability_labels_join_the_preview() {
        // Given the dash's labelled lock at Agility 10
        let defs = ability_locks();

        // When Agility's unlock preview is built
        let labels = labels_by_level(&defs, "reskilled:agility");

        // Then the dash shows at level 10
        assert_eq!(
            labels.get(&10),
            Some(&vec!["dashmod.lock.dash".to_string()])
        );
    }
}
