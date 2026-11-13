//! Checks the default data the mod ships (skills-and-perks/content) against the spec's numbers.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use skills_core::defs::{Action, DefKind, Effect, Settings};
use skills_core::locks::{Facts, TargetKind, requirements_for};
use skills_core::registry::{Defs, SourceFile, build};

const CONTENT: &str = "../../content";
const FOLDERS: [(&str, DefKind); 5] = [
    ("skills", DefKind::Skill),
    ("perks", DefKind::Perk),
    ("focus", DefKind::Focus),
    ("locks", DefKind::Lock),
    ("settings", DefKind::Settings),
];
const POINTS_AT_MAX: u32 = 8;
const CAPSTONE_LEVEL: u32 = 24;

const MELEE: &str = "skills_and_perks:melee";
const RANGED: &str = "skills_and_perks:ranged";
const DEFENSE: &str = "skills_and_perks:defense";
const MINING: &str = "skills_and_perks:mining";
const GATHERING: &str = "skills_and_perks:gathering";
const AGILITY: &str = "skills_and_perks:agility";
const ARCANE: &str = "skills_and_perks:arcane";
const TAMING: &str = "skills_and_perks:taming";

fn content() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(CONTENT)
}

/// Every data file under `content/data/<namespace>/skills_and_perks/<folder>/`, in path order.
fn shipped_files() -> Vec<SourceFile> {
    let mut files = Vec::new();
    let data = content().join("data");
    for namespace in fs::read_dir(&data).expect("content/data exists") {
        let namespace = namespace.unwrap().file_name().into_string().unwrap();
        for (folder, kind) in FOLDERS {
            let dir = data.join(&namespace).join("skills_and_perks").join(folder);
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                let name = path.file_stem().unwrap().to_str().unwrap().to_string();
                let json = fs::read_to_string(&path).unwrap();
                files.push(SourceFile {
                    path: format!("{namespace}:skills_and_perks/{folder}/{name}"),
                    order: 0,
                    kind,
                    json,
                });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    for (order, file) in files.iter_mut().enumerate() {
        file.order = order;
    }
    files
}

fn defaults() -> Defs {
    let (defs, errors) = build(shipped_files());
    assert_eq!(errors, vec![], "shipped data must load cleanly");
    defs
}

/// Vanilla 26.3's item tags and default `minecraft:repairable` components for the gear the locks name, read from the
/// game data Horizon's launcher prepared (`data/minecraft/tags/item/*.json`, `items.json`) on 2026-11-13.
struct Vanilla {
    item_tags: BTreeMap<String, BTreeSet<String>>,
    block_tags: BTreeMap<String, BTreeSet<String>>,
    repairable: BTreeMap<String, String>,
}

impl Vanilla {
    fn new() -> Self {
        let mut v = Vanilla {
            item_tags: BTreeMap::new(),
            block_tags: BTreeMap::new(),
            repairable: BTreeMap::new(),
        };
        let tool_tiers = [
            ("wooden", "wooden"),
            ("stone", "stone"),
            ("copper", "copper"),
            ("golden", "gold"),
            ("iron", "iron"),
            ("diamond", "diamond"),
            ("netherite", "netherite"),
        ];
        for (tier, material) in tool_tiers {
            for kind in ["pickaxe", "sword", "spear", "axe", "shovel", "hoe"] {
                v.item(
                    &format!("{tier}_{kind}"),
                    &format!("{kind}s"),
                    &format!("#minecraft:{material}_tool_materials"),
                );
            }
        }
        let armor_tiers = [
            ("leather", "leather"),
            ("copper", "copper"),
            ("chainmail", "chain"),
            ("golden", "gold"),
            ("iron", "iron"),
            ("diamond", "diamond"),
            ("netherite", "netherite"),
        ];
        for (tier, material) in armor_tiers {
            for (piece, tag) in [
                ("helmet", "head_armor"),
                ("chestplate", "chest_armor"),
                ("leggings", "leg_armor"),
                ("boots", "foot_armor"),
            ] {
                v.item(
                    &format!("{tier}_{piece}"),
                    tag,
                    &format!("#minecraft:repairs_{material}_armor"),
                );
            }
        }
        v.item(
            "turtle_helmet",
            "head_armor",
            "#minecraft:repairs_turtle_helmet",
        );
        v.repairable
            .insert("minecraft:mace".into(), "minecraft:breeze_rod".into());
        v.repairable.insert(
            "minecraft:elytra".into(),
            "minecraft:phantom_membrane".into(),
        );
        v.repairable.insert(
            "minecraft:shield".into(),
            "#minecraft:wooden_tool_materials".into(),
        );
        v.repairable.insert(
            "minecraft:wolf_armor".into(),
            "#minecraft:repairs_wolf_armor".into(),
        );
        v.block_tags.insert(
            "minecraft:anvil".into(),
            [
                "minecraft:anvil",
                "minecraft:chipped_anvil",
                "minecraft:damaged_anvil",
            ]
            .map(String::from)
            .into(),
        );
        v
    }

    fn item(&mut self, item: &str, tag: &str, material: &str) {
        let item = format!("minecraft:{item}");
        self.item_tags
            .entry(format!("minecraft:{tag}"))
            .or_default()
            .insert(item.clone());
        self.repairable.insert(item, material.to_string());
    }
}

impl Facts for Vanilla {
    fn item_in_tag(&self, item: &str, tag: &str) -> bool {
        self.item_tags
            .get(tag)
            .is_some_and(|items| items.contains(item))
    }
    fn block_in_tag(&self, block: &str, tag: &str) -> bool {
        self.block_tags
            .get(tag)
            .is_some_and(|blocks| blocks.contains(block))
    }
    fn item_repairable(&self, item: &str) -> Option<String> {
        self.repairable.get(item).cloned()
    }
}

/// The level the target needs for the action in the skill, 0 when that skill does not lock it.
fn needed(defs: &Defs, kind: TargetKind, id: &str, action: Action, skill: &str) -> u32 {
    let reqs = requirements_for(defs, kind, id, &Vanilla::new());
    reqs.by_action
        .get(&action)
        .and_then(|levels| levels.get(skill))
        .copied()
        .unwrap_or(0)
}

#[test]
fn defaults_load_without_errors() {
    // Given the shipped data files
    // When they are merged
    let (defs, errors) = build(shipped_files());

    // Then nothing is refused, and the settings file restates the built-in defaults
    assert_eq!(errors, vec![]);
    assert_eq!(defs.settings, Settings::default());
}

#[test]
fn eight_skills_with_spec_ids() {
    // Given the shipped data
    let defs = defaults();

    // When the skills and their icons are listed
    let icons: BTreeMap<&str, &str> = defs
        .skills
        .values()
        .map(|s| (s.id.as_str(), s.icon.as_str()))
        .collect();

    // Then they are the spec's eight, with the plan's icons
    let expected: BTreeMap<&str, &str> = [
        (MELEE, "minecraft:iron_sword"),
        (RANGED, "minecraft:bow"),
        (DEFENSE, "minecraft:shield"),
        (MINING, "minecraft:iron_pickaxe"),
        (GATHERING, "minecraft:iron_axe"),
        (AGILITY, "minecraft:feather"),
        (ARCANE, "minecraft:enchanted_book"),
        (TAMING, "minecraft:saddle"),
    ]
    .into();
    assert_eq!(icons, expected);
}

#[test]
fn each_skill_has_5_perks_costing_more_than_8_points() {
    // Given the shipped data
    let defs = defaults();

    for skill in defs.skills.keys() {
        // When the skill's perks are gathered
        let perks: Vec<_> = defs.perks.values().filter(|p| &p.skill == skill).collect();
        let total: u32 = perks.iter().map(|p| p.cost).sum();

        // Then there are five, each costing 1 to 3 points, more in all than the 8 points a maxed skill earns
        assert_eq!(perks.len(), 5, "{skill}");
        assert!(perks.iter().all(|p| (1..=3).contains(&p.cost)), "{skill}");
        assert!(total > POINTS_AT_MAX, "{skill}: {total}");
    }
}

#[test]
fn each_skill_has_one_capstone_at_level_24_or_more() {
    // Given the shipped data
    let defs = defaults();

    for skill in defs.skills.keys() {
        // When the perks needing level 24 or more are counted
        let capstones = defs
            .perks
            .values()
            .filter(|p| &p.skill == skill && p.min_level >= CAPSTONE_LEVEL)
            .count();

        // Then there is exactly one
        assert_eq!(capstones, 1, "{skill}");
    }
}

#[test]
fn perk_attributes_are_vanilla_attributes() {
    // Given vanilla 26.3's attribute ids (`attributes.json` of the prepared game data, 2026-11-13)
    let vanilla: BTreeSet<&str> = [
        "armor",
        "armor_toughness",
        "attack_damage",
        "attack_knockback",
        "attack_speed",
        "block_break_speed",
        "block_interaction_range",
        "burning_time",
        "entity_interaction_range",
        "explosion_knockback_resistance",
        "fall_damage_multiplier",
        "jump_strength",
        "knockback_resistance",
        "luck",
        "max_health",
        "mining_efficiency",
        "movement_efficiency",
        "movement_speed",
        "oxygen_bonus",
        "safe_fall_distance",
        "sneaking_speed",
        "step_height",
        "submerged_mining_speed",
        "sweeping_damage_ratio",
        "water_movement_efficiency",
    ]
    .into();
    let defs = defaults();

    // When every attribute a perk names is collected
    let named: Vec<&str> = defs
        .perks
        .values()
        .flat_map(|p| &p.effects)
        .filter_map(|e| match e {
            Effect::Attribute { attribute, .. } | Effect::MountAttribute { attribute, .. } => {
                Some(attribute.as_str())
            }
            _ => None,
        })
        .collect();

    // Then each one exists in vanilla
    assert!(!named.is_empty());
    for attribute in named {
        let path = attribute
            .strip_prefix("minecraft:")
            .unwrap_or("not a minecraft attribute");
        assert!(vanilla.contains(path), "{attribute}");
    }
}

#[test]
fn three_focus_areas_cover_all_skills_once() {
    // Given the shipped data
    let defs = defaults();

    // When the focus areas are listed with their skills
    let areas: BTreeMap<&str, BTreeSet<&str>> = defs
        .focus
        .values()
        .map(|f| (f.id.as_str(), f.skills.iter().map(String::as_str).collect()))
        .collect();

    // Then they are the spec's three, and every skill is in exactly one
    let expected: BTreeMap<&str, BTreeSet<&str>> = [
        ("skills_and_perks:fighting", [MELEE, RANGED, DEFENSE].into()),
        (
            "skills_and_perks:working",
            [MINING, GATHERING, ARCANE].into(),
        ),
        ("skills_and_perks:roaming", [AGILITY, TAMING].into()),
    ]
    .into();
    assert_eq!(areas, expected);
}

#[test]
fn tier_ladder_matches_spec() {
    // Given the shipped data
    let defs = defaults();

    for (tier, level) in [
        ("copper", 4),
        ("iron", 8),
        ("diamond", 16),
        ("netherite", 24),
    ] {
        // When each tier's pickaxe, sword, chestplate and spear are checked
        let item = |name: &str| format!("minecraft:{tier}_{name}");
        let pickaxe = needed(
            &defs,
            TargetKind::Item,
            &item("pickaxe"),
            Action::Break,
            MINING,
        );
        let sword = needed(
            &defs,
            TargetKind::Item,
            &item("sword"),
            Action::Attack,
            MELEE,
        );
        let chestplate = needed(
            &defs,
            TargetKind::Item,
            &item("chestplate"),
            Action::Equip,
            DEFENSE,
        );
        let spear_attack = needed(
            &defs,
            TargetKind::Item,
            &item("spear"),
            Action::Attack,
            MELEE,
        );
        let spear_use = needed(&defs, TargetKind::Item, &item("spear"), Action::Use, MELEE);
        let axe_break = needed(
            &defs,
            TargetKind::Item,
            &item("axe"),
            Action::Break,
            GATHERING,
        );
        let axe_attack = needed(&defs, TargetKind::Item, &item("axe"), Action::Attack, MELEE);

        // Then each needs the tier's level in its skill
        assert_eq!(
            [
                pickaxe,
                sword,
                chestplate,
                spear_attack,
                spear_use,
                axe_break,
                axe_attack
            ],
            [level; 7],
            "{tier}"
        );
    }
}

#[test]
fn specials_match_spec() {
    // Given the shipped data
    let defs = defaults();
    let item =
        |id: &str, action: Action, skill: &str| needed(&defs, TargetKind::Item, id, action, skill);
    let block = |id: &str| needed(&defs, TargetKind::Block, id, Action::Open, ARCANE);
    let mount = |id: &str| needed(&defs, TargetKind::Entity, id, Action::Mount, TAMING);

    // When every special item, block and mount is checked
    // Then each needs the spec's level
    assert_eq!(item("minecraft:mace", Action::Attack, MELEE), 20);
    assert_eq!(item("minecraft:trident", Action::Use, RANGED), 16);
    assert_eq!(item("minecraft:trident", Action::Attack, RANGED), 16);
    assert_eq!(item("minecraft:crossbow", Action::Use, RANGED), 6);
    assert_eq!(item("minecraft:elytra", Action::Equip, AGILITY), 16);
    assert_eq!(item("minecraft:shield", Action::Equip, DEFENSE), 4);
    assert_eq!(item("minecraft:shield", Action::Use, DEFENSE), 4);
    assert_eq!(item("minecraft:wolf_armor", Action::Use, TAMING), 6);
    assert_eq!(mount("minecraft:happy_ghast"), 12);
    for other in [
        "minecraft:horse",
        "minecraft:donkey",
        "minecraft:mule",
        "minecraft:skeleton_horse",
        "minecraft:zombie_horse",
        "minecraft:camel",
        "minecraft:camel_husk",
        "minecraft:llama",
        "minecraft:trader_llama",
        "minecraft:pig",
        "minecraft:strider",
        "minecraft:nautilus",
        "minecraft:zombie_nautilus",
    ] {
        assert_eq!(mount(other), 4, "{other}");
    }
    assert_eq!(block("minecraft:enchanting_table"), 8);
    for anvil in [
        "minecraft:anvil",
        "minecraft:chipped_anvil",
        "minecraft:damaged_anvil",
    ] {
        assert_eq!(block(anvil), 6, "{anvil}");
    }
    assert_eq!(block("minecraft:brewing_stand"), 4);
}

#[test]
fn free_tiers_are_unlocked() {
    // Given the shipped data
    let defs = defaults();
    let facts = Vanilla::new();

    // When the free tiers' gear is checked
    let tools = ["wooden", "stone", "golden"].iter().flat_map(|tier| {
        ["pickaxe", "sword", "spear", "axe", "shovel", "hoe"]
            .map(|kind| format!("minecraft:{tier}_{kind}"))
    });
    let armor = ["leather", "golden", "chainmail"].iter().flat_map(|tier| {
        ["helmet", "chestplate", "leggings", "boots"]
            .map(|piece| format!("minecraft:{tier}_{piece}"))
    });
    let free = [
        "minecraft:bow",
        "minecraft:turtle_helmet",
        "minecraft:golden_horse_armor",
        "minecraft:leather_horse_armor",
    ]
    .map(String::from);

    // Then nothing locks them
    for id in tools.chain(armor).chain(free) {
        let reqs = requirements_for(&defs, TargetKind::Item, &id, &facts);
        assert!(reqs.by_action.is_empty(), "{id}: {reqs:?}");
    }
}

#[test]
fn every_text_key_exists_in_en_us() {
    // Given the shipped data and the English text
    let defs = defaults();
    let lang_path = content().join("assets/skills_and_perks/lang/en_us.json");
    let english: BTreeMap<String, String> =
        serde_json::from_str(&fs::read_to_string(lang_path).unwrap()).unwrap();

    // When every key the data and the server's refusal messages use is collected
    let mut keys: Vec<String> = Vec::new();
    keys.extend(defs.skills.values().map(|s| s.name.clone()));
    keys.extend(
        defs.perks
            .values()
            .flat_map(|p| [p.name.clone(), p.description.clone()]),
    );
    keys.extend(defs.focus.values().map(|f| f.name.clone()));
    keys.extend(defs.locks.iter().filter_map(|l| l.label.clone()));
    keys.extend(
        [
            "needs",
            "unknown_skill",
            "unknown_perk",
            "unknown_focus",
            "at_max",
            "not_enough_levels",
            "not_enough_points",
            "below_min_level",
            "missing_perk",
            "already_owned",
            "already_focused",
        ]
        .map(|m| format!("skills_and_perks.msg.{m}")),
    );

    // Then each has non-empty English text, and data keys follow the naming scheme
    for key in &keys {
        assert!(
            english.get(key).is_some_and(|text| !text.is_empty()),
            "missing English text for {key}"
        );
        assert!(key.starts_with("skills_and_perks."), "{key}");
    }
    for perk in defs.perks.values() {
        let path = perk.id.split_once(':').unwrap().1;
        assert_eq!(perk.name, format!("skills_and_perks.perk.{path}"));
        assert_eq!(
            perk.description,
            format!("skills_and_perks.perk.{path}.desc")
        );
    }
}
