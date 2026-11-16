//! Checks the default data the mod ships (reskilled/content) against the spec's numbers.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use skills_core::defs::{Action, DefKind, Effect, Matcher, Reward, Settings};
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
const POINTS_AT_MAX: u32 = 16;
const CAPSTONE_LEVEL: u32 = 24;

const MELEE: &str = "reskilled:melee";
const RANGED: &str = "reskilled:ranged";
const DEFENSE: &str = "reskilled:defense";
const MINING: &str = "reskilled:mining";
const GATHERING: &str = "reskilled:gathering";
const AGILITY: &str = "reskilled:agility";
const ARCANE: &str = "reskilled:arcane";
const BUILDING: &str = "reskilled:building";

fn content() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(CONTENT)
}

/// Every data file under `content/data/<namespace>/reskilled/<folder>/`, in path order.
fn shipped_files() -> Vec<SourceFile> {
    let mut files = Vec::new();
    let data = content().join("data");
    for namespace in fs::read_dir(&data).expect("content/data exists") {
        let namespace = namespace.unwrap().file_name().into_string().unwrap();
        for (folder, kind) in FOLDERS {
            let dir = data.join(&namespace).join("reskilled").join(folder);
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                let name = path.file_stem().unwrap().to_str().unwrap().to_string();
                let json = fs::read_to_string(&path).unwrap();
                files.push(SourceFile {
                    path: format!("{namespace}:reskilled/{folder}/{name}"),
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

    // Then nothing is refused, and the settings file restates the built-in defaults (the lists, empty when built in,
    // are what the file adds; see shipped_settings_lists_cover_vanilla)
    assert_eq!(errors, vec![]);
    let scalars = Settings {
        not_creatures: Vec::new(),
        projectile_damage: Vec::new(),
        equip_slots: Vec::new(),
        ..defs.settings.clone()
    };
    assert_eq!(scalars, Settings::default());
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

    // Then they are the spec's eight, with their level-1 icons
    let expected: BTreeMap<&str, &str> = [
        (MELEE, "minecraft:wooden_sword"),
        (RANGED, "minecraft:bow"),
        (DEFENSE, "minecraft:leather_chestplate"),
        (MINING, "minecraft:wooden_pickaxe"),
        (GATHERING, "minecraft:wooden_axe"),
        (AGILITY, "minecraft:feather"),
        (ARCANE, "minecraft:book"),
        (BUILDING, "minecraft:bricks"),
    ]
    .into();
    assert_eq!(icons, expected);
}

#[test]
fn a_maxed_skill_earns_exactly_enough_points_for_all_its_perks() {
    // Given the shipped data
    let defs = defaults();

    for skill in defs.skills.keys() {
        // When the skill's perks are gathered by the level they unlock at
        let mut perks: Vec<_> = defs.perks.values().filter(|p| &p.skill == skill).collect();
        perks.sort_by_key(|p| p.min_level);
        let costs: Vec<u32> = perks.iter().map(|p| p.cost).collect();

        // Then there are five, cheap early and dearer later, each affordable at its level, and together they cost
        // exactly the 16 points a maxed skill earns
        assert_eq!(costs, vec![2, 2, 2, 4, 6], "{skill}");
        assert!(perks.iter().all(|p| p.cost <= p.min_level / 2), "{skill}");
        assert_eq!(costs.iter().sum::<u32>(), POINTS_AT_MAX, "{skill}");
        assert_eq!(defs.settings.points_every, 2);
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
        ("reskilled:fighting", [MELEE, RANGED, DEFENSE].into()),
        ("reskilled:working", [MINING, GATHERING, ARCANE].into()),
        ("reskilled:roaming", [AGILITY, BUILDING].into()),
    ]
    .into();
    assert_eq!(areas, expected);
}

#[test]
fn each_focus_area_gives_xp_for_its_tasks() {
    // Given the shipped data
    let defs = defaults();
    let tasks = |area: &str| -> Vec<&Reward> { defs.focus[area].rewards.iter().collect() };

    // When each area's rewards are read
    let fighting = tasks("reskilled:fighting");
    let working = tasks("reskilled:working");
    let roaming = tasks("reskilled:roaming");

    // Then Fighting gives half again the XP of kills; Working half again that of ores, and some for mining,
    // digging, chopping, grown crops and smelting; Roaming some for travelling, taming and breeding
    assert_eq!(fighting, vec![&Reward::Kill { bonus_percent: 50 }]);
    assert!(working.contains(&&Reward::BlockXp { bonus_percent: 50 }));
    let broken: Vec<&Matcher> = working
        .iter()
        .filter_map(|reward| match reward {
            Reward::Break { blocks, .. } => Some(blocks),
            _ => None,
        })
        .collect();
    for tag in [
        "#minecraft:mineable/pickaxe",
        "#minecraft:mineable/shovel",
        "#minecraft:logs",
    ] {
        assert!(broken.contains(&&Matcher::Tag(tag.into())), "{tag}");
    }
    let grown = working.iter().any(|reward| {
        matches!(reward, Reward::Break { blocks: Matcher::AnyOf(crops), state, .. }
            if crops.contains(&Matcher::Id("minecraft:wheat".into())) && state.get("age").map(String::as_str) == Some("7"))
    });
    assert!(grown);
    assert!(
        working
            .iter()
            .any(|reward| matches!(reward, Reward::Smelt { .. }))
    );
    for task in ["travel", "tame", "breed"] {
        let found = roaming.iter().any(|reward| {
            matches!(
                (task, reward),
                ("travel", Reward::Travel { .. })
                    | ("tame", Reward::Tame { .. })
                    | ("breed", Reward::Breed { .. })
            )
        });
        assert!(found, "{task}");
    }
    assert_eq!(defs.settings.focus_cost, 10);
}

#[test]
fn raising_costs_one_level_more_each_time_starting_at_1() {
    // Given the shipped data
    let defs = defaults();

    // When the cost curve is read
    let curve = defs.settings.cost;

    // Then a raise from level L costs L: 1 at first, 31 for the last
    assert_eq!((curve.base, curve.every), (1, 1));
}

#[test]
fn skill_icons_climb_the_gear_tiers() {
    // Given the shipped data
    let defs = defaults();
    let icon = |skill: &str, level: u32| defs.skills[skill].icon_at(level).to_string();

    // When the tool skills' icons are read at each tier
    // Then each shows the gear its level unlocks: wood, copper at 4, iron at 8, diamond at 16, netherite at 24
    for (skill, kind) in [(MINING, "pickaxe"), (GATHERING, "axe"), (MELEE, "sword")] {
        let tiers = [1, 4, 8, 16, 24].map(|level| icon(skill, level));
        let expected = ["wooden", "copper", "iron", "diamond", "netherite"]
            .map(|tier| format!("minecraft:{tier}_{kind}"));
        assert_eq!(tiers, expected, "{skill}");
    }
    assert_eq!(icon(DEFENSE, 1), "minecraft:leather_chestplate");
    assert_eq!(icon(DEFENSE, 24), "minecraft:netherite_chestplate");
    assert_eq!(icon(RANGED, 16), "minecraft:trident");
    assert_eq!(icon(AGILITY, 16), "minecraft:elytra");
    for skill in [
        MELEE, RANGED, DEFENSE, MINING, GATHERING, AGILITY, ARCANE, BUILDING,
    ] {
        assert_ne!(icon(skill, 1), icon(skill, 32), "{skill} never improves");
    }
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
fn chainmail_armor_needs_defense_4_and_golden_armor_defense_2_and_arcane_4() {
    // Given the shipped data
    let defs = defaults();

    for piece in ["helmet", "chestplate", "leggings", "boots"] {
        // When each piece of chainmail and golden armor is checked
        let equip = |tier: &str, skill: &str| {
            let id = format!("minecraft:{tier}_{piece}");
            needed(&defs, TargetKind::Item, &id, Action::Equip, skill)
        };

        // Then chainmail needs Defense 4; golden needs Defense 2 and Arcane 4, as RLCraft set them
        assert_eq!(equip("chainmail", DEFENSE), 4, "{piece}");
        assert_eq!(
            (equip("golden", DEFENSE), equip("golden", ARCANE)),
            (2, 4),
            "{piece}"
        );
    }
}

#[test]
fn locked_tools_cannot_be_used_as_weapons() {
    // Given the shipped data
    let defs = defaults();

    for (tier, level) in [
        ("copper", 4),
        ("iron", 8),
        ("diamond", 16),
        ("netherite", 24),
    ] {
        // When each tier's pickaxe, shovel and hoe are swung at a mob
        let attack = |kind: &str, skill: &str| {
            needed(
                &defs,
                TargetKind::Item,
                &format!("minecraft:{tier}_{kind}"),
                Action::Attack,
                skill,
            )
        };

        // Then each needs the level its own work needs: Mining for a pickaxe, Gathering for the others
        assert_eq!(
            [
                attack("pickaxe", MINING),
                attack("shovel", GATHERING),
                attack("hoe", GATHERING)
            ],
            [level; 3],
            "{tier}"
        );
    }
}

#[test]
fn golden_tools_need_their_skill_at_2_and_arcane_at_4() {
    // Given the shipped data
    let defs = defaults();
    let need = |kind: &str, action: Action, skill: &str| {
        needed(
            &defs,
            TargetKind::Item,
            &format!("minecraft:golden_{kind}"),
            action,
            skill,
        )
    };

    // When each golden tool is checked for its work and for Arcane
    // Then the work needs 2 in its skill and Arcane 4, as RLCraft set them
    for (kind, action, skill) in [
        ("pickaxe", Action::Break, MINING),
        ("axe", Action::Break, GATHERING),
        ("shovel", Action::Break, GATHERING),
        ("hoe", Action::Break, GATHERING),
        ("sword", Action::Attack, MELEE),
        ("spear", Action::Attack, MELEE),
    ] {
        assert_eq!(
            (need(kind, action, skill), need(kind, action, ARCANE)),
            (2, 4),
            "{kind}"
        );
    }
}

#[test]
fn enchantments_need_arcane_three_levels_a_step() {
    // Given the shipped data
    let defs = defaults();
    let need = |enchantment: &str, level: u32, action: Action| {
        let reqs = skills_core::locks::enchantment_requirements(
            &defs.locks,
            &[(enchantment.to_string(), level)],
        );
        reqs.by_action.get(&action).cloned().unwrap_or_default()
    };
    let arcane = |enchantment: &str, level: u32, action: Action| {
        need(enchantment, level, action)
            .get(ARCANE)
            .copied()
            .unwrap_or(0)
    };

    // When enchantments are checked at each level
    // Then level I is free and each level above it needs 3 more Arcane; Fortune and Looting climb twice as fast;
    // Mending needs Arcane 12; and no enchantment asks any other skill
    for (enchantment, action, top) in [
        ("minecraft:sharpness", Action::Attack, 5),
        ("minecraft:smite", Action::Attack, 5),
        ("minecraft:bane_of_arthropods", Action::Attack, 5),
        ("minecraft:power", Action::Use, 5),
        ("minecraft:protection", Action::Equip, 4),
        ("minecraft:blast_protection", Action::Equip, 4),
        ("minecraft:unbreaking", Action::Break, 3),
    ] {
        for level in 1..=top {
            assert_eq!(
                arcane(enchantment, level, action),
                3 * (level - 1),
                "{enchantment} {level}"
            );
        }
    }
    for (enchantment, action) in [
        ("minecraft:fortune", Action::Break),
        ("minecraft:looting", Action::Attack),
    ] {
        let ladder: Vec<u32> = (1..=3)
            .map(|level| arcane(enchantment, level, action))
            .collect();
        assert_eq!(ladder, vec![0, 6, 12], "{enchantment}");
    }
    assert_eq!(arcane("minecraft:mending", 1, Action::Equip), 12);
    assert_eq!(arcane("minecraft:unbreaking", 3, Action::Equip), 6);
    for entry in defs
        .locks
        .iter()
        .filter(|e| matches!(e.target, skills_core::defs::Target::Enchantment(_)))
    {
        assert_eq!(
            entry.requires.keys().collect::<Vec<_>>(),
            vec![ARCANE],
            "{:?}",
            entry.label
        );
    }
}

#[test]
fn advanced_blocks_need_building_to_place_and_arcane_to_use() {
    // Given the shipped data
    let defs = defaults();
    let place = |id: &str| needed(&defs, TargetKind::Block, id, Action::Place, BUILDING);

    // When advanced blocks are placed and the anvil is opened
    let opened = needed(
        &defs,
        TargetKind::Block,
        "minecraft:anvil",
        Action::Open,
        ARCANE,
    );

    // Then each tier needs its Building level, and using the anvil still needs Arcane 12
    assert_eq!(place("minecraft:scaffolding"), 4);
    assert_eq!(place("minecraft:piston"), 8);
    assert_eq!(place("minecraft:brewing_stand"), 8);
    assert_eq!(place("minecraft:anvil"), 12);
    assert_eq!(place("minecraft:enchanting_table"), 12);
    assert_eq!(place("minecraft:beacon"), 16);
    assert_eq!(opened, 12);
}

#[test]
fn specials_match_spec() {
    // Given the shipped data
    let defs = defaults();
    let item =
        |id: &str, action: Action, skill: &str| needed(&defs, TargetKind::Item, id, action, skill);
    let block = |id: &str| needed(&defs, TargetKind::Block, id, Action::Open, ARCANE);
    let mount = |id: &str| needed(&defs, TargetKind::Entity, id, Action::Mount, AGILITY);

    // When every special item, block and mount is checked
    // Then each needs the spec's level
    assert_eq!(item("minecraft:mace", Action::Attack, MELEE), 20);
    assert_eq!(item("minecraft:trident", Action::Use, RANGED), 16);
    assert_eq!(item("minecraft:trident", Action::Attack, RANGED), 16);
    assert_eq!(item("minecraft:crossbow", Action::Use, RANGED), 6);
    assert_eq!(item("minecraft:bow", Action::Use, AGILITY), 2);
    assert_eq!(item("minecraft:elytra", Action::Equip, AGILITY), 32);
    assert_eq!(item("minecraft:shield", Action::Equip, DEFENSE), 12);
    assert_eq!(item("minecraft:shield", Action::Use, DEFENSE), 12);
    assert_eq!(item("minecraft:shears", Action::Break, GATHERING), 2);
    assert_eq!(item("minecraft:shears", Action::Use, GATHERING), 2);
    assert_eq!(item("minecraft:ender_pearl", Action::Use, ARCANE), 8);
    assert_eq!(item("minecraft:ender_eye", Action::Use, ARCANE), 16);
    assert_eq!(item("minecraft:firework_rocket", Action::Use, AGILITY), 4);
    assert_eq!(item("minecraft:lead", Action::Use, AGILITY), 4);
    assert_eq!(item("minecraft:wolf_armor", Action::Use, AGILITY), 6);
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
    assert_eq!(block("minecraft:enchanting_table"), 12);
    for anvil in [
        "minecraft:anvil",
        "minecraft:chipped_anvil",
        "minecraft:damaged_anvil",
    ] {
        assert_eq!(block(anvil), 12, "{anvil}");
    }
    assert_eq!(block("minecraft:brewing_stand"), 8);
}

#[test]
fn free_tiers_are_unlocked() {
    // Given the shipped data
    let defs = defaults();
    let facts = Vanilla::new();

    // When the free tiers' gear is checked
    let tools = ["wooden", "stone"].iter().flat_map(|tier| {
        ["pickaxe", "sword", "spear", "axe", "shovel", "hoe"]
            .map(|kind| format!("minecraft:{tier}_{kind}"))
    });
    let armor = ["leather"].iter().flat_map(|tier| {
        ["helmet", "chestplate", "leggings", "boots"]
            .map(|piece| format!("minecraft:{tier}_{piece}"))
    });
    let free = [
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
    let lang_path = content().join("assets/reskilled/lang/en_us.json");
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
        defs.focus
            .values()
            .flat_map(|f| &f.rewards)
            .filter_map(|reward| match reward {
                Reward::Break { label, .. } => label.clone(),
                _ => None,
            }),
    );
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
        .map(|m| format!("reskilled.msg.{m}")),
    );

    // Then each has non-empty English text, and data keys follow the naming scheme
    for key in &keys {
        assert!(
            english.get(key).is_some_and(|text| !text.is_empty()),
            "missing English text for {key}"
        );
        assert!(key.starts_with("reskilled."), "{key}");
    }
    for perk in defs.perks.values() {
        let path = perk.id.split_once(':').unwrap().1;
        assert_eq!(perk.name, format!("reskilled.perk.{path}"));
        assert_eq!(perk.description, format!("reskilled.perk.{path}.desc"));
    }
}

#[test]
fn each_shipped_skill_grows_by_its_own_work() {
    // Given the shipped data
    let defs = defaults();
    let rules = |skill: &str| defs.skills[skill].earned_by.clone();
    let tag = |t: &str| Matcher::Tag(t.into());

    // When each skill's rules are read
    // Then they are what the mod grew each skill by before the rules were data
    use skills_core::defs::EarnRule;
    assert_eq!(
        rules(MINING),
        vec![EarnRule::Break {
            with: tag("#minecraft:pickaxes"),
            worth: 1.0
        }]
    );
    assert_eq!(
        rules(GATHERING),
        vec![EarnRule::Break {
            with: Matcher::AnyOf(vec![
                Matcher::Id("minecraft:shears".into()),
                tag("#minecraft:axes"),
                tag("#minecraft:shovels"),
                tag("#minecraft:hoes"),
            ]),
            worth: 1.0
        }]
    );
    assert_eq!(rules(MELEE), vec![EarnRule::Attack { worth: 1.0 }]);
    assert_eq!(rules(RANGED), vec![EarnRule::ProjectileHit { worth: 1.0 }]);
    assert_eq!(rules(DEFENSE), vec![EarnRule::Hurt { worth: 1.0 }]);
    assert_eq!(rules(AGILITY), vec![EarnRule::Travel { per_block: 0.05 }]);
    assert_eq!(rules(ARCANE), vec![EarnRule::Enchant { worth: 5.0 }]);
    assert_eq!(rules(BUILDING), vec![EarnRule::Place { worth: 1.0 }]);
}

#[test]
fn shipped_settings_lists_cover_vanilla() {
    // Given the shipped data
    let defs = defaults();
    let s = &defs.settings;
    let picks = |list: &[Matcher], id: &str| {
        list.iter().any(|m| {
            skills_core::locks::matches(m, TargetKind::Entity, id, &skills_core::locks::NoFacts)
        })
    };

    // When vanilla's decorations, vehicles, creatures and damage types are checked
    // Then decorations and vehicles are not creatures, mobs are; projectile damage is told from the rest
    for thing in [
        "minecraft:armor_stand",
        "minecraft:item_frame",
        "minecraft:glow_item_frame",
        "minecraft:painting",
        "minecraft:end_crystal",
        "minecraft:leash_knot",
        "minecraft:interaction",
        "minecraft:item",
        "minecraft:oak_boat",
        "minecraft:cherry_chest_boat",
        "minecraft:bamboo_raft",
        "minecraft:bamboo_chest_raft",
        "minecraft:minecart",
        "minecraft:hopper_minecart",
    ] {
        assert!(picks(&s.not_creatures, thing), "{thing}");
    }
    for creature in [
        "minecraft:zombie",
        "minecraft:cow",
        "minecraft:player",
        "minecraft:ender_dragon",
    ] {
        assert!(!picks(&s.not_creatures, creature), "{creature}");
    }
    for projectile in [
        "minecraft:arrow",
        "minecraft:trident",
        "minecraft:thrown",
        "minecraft:fireball",
        "minecraft:unattributed_fireball",
        "minecraft:wind_charge",
    ] {
        assert!(picks(&s.projectile_damage, projectile), "{projectile}");
    }
    for other in [
        "minecraft:player_attack",
        "minecraft:fall",
        "minecraft:magic",
    ] {
        assert!(!picks(&s.projectile_damage, other), "{other}");
    }

    // And shift-clicks put on armor by its slot tag, the elytra on the chest and the shield in the off hand
    let slot_of = |item: &str| {
        s.equip_slots
            .iter()
            .find(|rule| {
                skills_core::locks::matches(&rule.items, TargetKind::Item, item, &Vanilla::new())
            })
            .map(|rule| rule.slot)
    };
    use skills_core::defs::WornSlot;
    assert_eq!(
        slot_of("minecraft:diamond_chestplate"),
        Some(WornSlot::Chest)
    );
    assert_eq!(slot_of("minecraft:iron_helmet"), Some(WornSlot::Head));
    assert_eq!(slot_of("minecraft:elytra"), Some(WornSlot::Chest));
    assert_eq!(slot_of("minecraft:shield"), Some(WornSlot::Offhand));
    assert_eq!(slot_of("minecraft:stick"), None);
}

#[test]
fn every_shipped_skill_has_its_colour() {
    let defs = defaults();
    let colour = |skill: &str| skills_core::defs::skill_rgb(&defs.skills[skill]);
    assert_eq!(
        [MINING, MELEE, DEFENSE, ARCANE, GATHERING, AGILITY, BUILDING, RANGED].map(colour),
        [0x8FA6B8, 0xD9534A, 0x6E8FD9, 0xB07CF2, 0x7FC75A, 0x5AD1C7, 0xD98C4A, 0xD9C24A]
    );
    assert!(defs.skills.values().all(|s| s.color.is_some()));
}

#[test]
fn every_shipped_perk_has_an_icon() {
    let defs = defaults();
    for perk in defs.perks.values() {
        let icon = perk.icon.as_deref().unwrap_or_else(|| panic!("{} has no icon", perk.id));
        assert!(icon.starts_with("minecraft:"), "{}: {icon}", perk.id);
    }
}

#[test]
fn every_labelled_lock_has_an_icon() {
    let defs = defaults();
    for entry in defs.locks.iter().filter(|e| e.label.is_some()) {
        let icon = entry.icon.as_deref().unwrap_or_else(|| panic!("{:?} has no icon", entry.label));
        assert!(icon.starts_with("minecraft:"), "{:?}: {icon}", entry.label);
    }
}
