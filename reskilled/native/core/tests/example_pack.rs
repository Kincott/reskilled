//! The example pack in `reskilled/claude-skill/reskilled-modding/example-pack` (the one the modding guide points to) loads beside the
//! shipped data with no error and does what the guide says, so its examples can be copied as they are.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use skills_core::defs::{DefError, DefKind, EarnRule, Effect, Matcher, WornSlot};
use skills_core::registry::{Defs, SourceFile, build};

const FOLDERS: [(&str, DefKind); 5] = [
    ("skills", DefKind::Skill),
    ("perks", DefKind::Perk),
    ("focus", DefKind::Focus),
    ("locks", DefKind::Lock),
    ("settings", DefKind::Settings),
];

fn root(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// Every data file under `<pack>/data/<namespace>/reskilled/<folder>/`, in path order.
fn files_of(pack: &Path) -> Vec<SourceFile> {
    let mut files = Vec::new();
    let data = pack.join("data");
    for namespace in fs::read_dir(&data).expect("the pack has a data folder") {
        let namespace = namespace.unwrap().file_name().into_string().unwrap();
        for (folder, kind) in FOLDERS {
            let Ok(entries) = fs::read_dir(data.join(&namespace).join("reskilled").join(folder))
            else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                let name = path.file_stem().unwrap().to_str().unwrap().to_string();
                files.push(SourceFile {
                    path: format!("{namespace}:reskilled/{folder}/{name}"),
                    order: 0,
                    kind,
                    json: fs::read_to_string(&path).unwrap(),
                });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

/// The shipped data, then the example pack, as a game with both installed loads them.
fn with_example() -> (Defs, Vec<DefError>) {
    let mut files = files_of(&root("../../content"));
    files.extend(files_of(&root(
        "../../claude-skill/reskilled-modding/example-pack",
    )));
    for (order, file) in files.iter_mut().enumerate() {
        file.order = order;
    }
    build(files)
}

#[test]
fn the_example_pack_loads_without_errors() {
    let (_, errors) = with_example();
    assert_eq!(errors, vec![]);
}

#[test]
fn the_example_pack_does_what_the_guide_says() {
    let (defs, _) = with_example();

    // A new skill that grows by digging with shovels or the mod's drill, with its two perks
    let digging = &defs.skills["examplemod:digging"];
    assert_eq!(skills_core::defs::skill_rgb(digging), 0xC08A4F);
    assert!(
        defs.locks
            .iter()
            .any(|e| e.icon.as_deref() == Some("examplemod:drill"))
    );
    assert_eq!(
        defs.perks["examplemod:digging_quick_spade"].icon.as_deref(),
        Some("minecraft:iron_shovel")
    );
    assert!(matches!(&digging.earned_by[..], [EarnRule::Break { .. }]));
    assert!(defs.perks.contains_key("examplemod:digging_quick_spade"));
    assert!(defs.perks.contains_key("examplemod:digging_deep_roots"));

    // A shipped perk retuned (Light Step: 10% instead of 5%) and one removed (Keen Eye)
    let light_step = &defs.perks["reskilled:agility_light_step"].effects[0];
    assert!(matches!(light_step, Effect::Attribute { amount, .. } if (*amount - 0.1).abs() < 1e-9));
    assert!(!defs.perks.contains_key("reskilled:ranged_keen_eye"));

    // The shipped Working focus, restated in full with Digging added
    assert!(
        defs.focus["reskilled:working"]
            .skills
            .contains(&"examplemod:digging".to_string())
    );
    assert_eq!(defs.focus["reskilled:working"].rewards.len(), 8);

    // Locks on the mod's own things, and the shipped shield lock replaced
    let shields: Vec<&BTreeMap<String, u32>> = defs
        .locks
        .iter()
        .filter(|e| e.label.as_deref() == Some("reskilled.lock.shield"))
        .map(|e| &e.requires)
        .collect();
    assert_eq!(
        shields,
        vec![&[("reskilled:defense".to_string(), 8)].into()]
    );
    assert!(
        defs.locks
            .iter()
            .any(|e| e.label.as_deref() == Some("examplemod.lock.mammoth"))
    );

    // The settings lists gained the mod's entries beside the shipped ones
    let s = &defs.settings;
    assert!(
        s.not_creatures
            .contains(&Matcher::Id("examplemod:scarecrow".into()))
    );
    assert!(
        s.projectile_damage
            .contains(&Matcher::Id("examplemod:dart".into()))
    );
    assert!(s.equip_slots.iter().any(|r| r.slot == WornSlot::Chest
        && r.items == Matcher::Pattern("examplemod:*_backpack".into())));
    assert!(
        s.not_creatures.len() > 1,
        "the shipped entries are still there"
    );
}

#[test]
fn every_key_the_example_pack_names_has_english() {
    let (defs, _) = with_example();
    let read = |path: &str| -> BTreeMap<String, String> {
        serde_json::from_str(&fs::read_to_string(root(path)).unwrap()).unwrap()
    };
    let mut lang = read("../../content/assets/reskilled/lang/en_us.json");
    lang.extend(read(
        "../../claude-skill/reskilled-modding/example-pack/assets/examplemod/lang/en_us.json",
    ));
    let mut keys: Vec<&str> = Vec::new();
    for skill in defs
        .skills
        .values()
        .filter(|s| s.id.starts_with("examplemod:"))
    {
        keys.push(&skill.name);
    }
    for perk in defs
        .perks
        .values()
        .filter(|p| p.id.starts_with("examplemod:") || p.priority > 0)
    {
        keys.push(&perk.name);
        keys.push(&perk.description);
    }
    for entry in &defs.locks {
        keys.extend(entry.label.as_deref());
    }
    let missing: Vec<&&str> = keys
        .iter()
        .filter(|key| !lang.contains_key(**key))
        .collect();
    assert!(missing.is_empty(), "no English for {missing:?}");
}
