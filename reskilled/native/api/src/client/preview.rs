//! The shipped mod as a player sees it: the default data files and the English text, drawn at the game's smallest
//! screen and a usual one. The tests check nothing leaves the window; with `SKILLS_PREVIEW_DIR` set they also
//! write every scene's drawing calls there as JSON, which `tools/preview.py` renders to pictures.

use std::fs;
use std::path::{Path, PathBuf};

use horizon_client::mod_api::{
    Host, HudBatch, ItemStack, Text, dispatch_gui_render_post, dispatch_item_decorations,
    dispatch_tooltip,
};
use skills_core::defs::{Action, DefKind};
use skills_core::locks::Requirements;
use skills_core::progress::{PlayerState, SkillProgress};
use skills_core::registry::{SourceFile, build};
use skills_core::wire::{ClientDefs, client_defs_json};

use super::cache::{Tab, WARNING_TICKS, Warning, with};
use super::screen::draw;
use crate::MOD_ID;
use crate::test_host::{Paint, TestHost, exclusive, mods, spans_of};

const SMALLEST: [u32; 2] = [320, 240];
const USUAL: [u32; 2] = [427, 240];
const FOLDERS: [(&str, DefKind); 5] = [
    ("skills", DefKind::Skill),
    ("perks", DefKind::Perk),
    ("focus", DefKind::Focus),
    ("locks", DefKind::Lock),
    ("settings", DefKind::Settings),
];
const AWAY: [f32; 2] = [1.0, 1.0];

fn skill(path: &str) -> String {
    format!("reskilled:{path}")
}

/// What the server would send a client of the shipped data files.
pub(crate) fn shipped() -> ClientDefs {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/data/reskilled");
    let mut files = Vec::new();
    for (folder, kind) in FOLDERS {
        for entry in fs::read_dir(data.join("reskilled").join(folder)).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_stem().unwrap().to_str().unwrap().to_string();
            files.push(SourceFile {
                path: format!("reskilled:reskilled/{folder}/{name}"),
                order: 0,
                kind,
                json: fs::read_to_string(&path).unwrap(),
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    for (order, file) in files.iter_mut().enumerate() {
        file.order = order;
    }
    let (defs, errors) = build(files);
    assert_eq!(errors, vec![]);
    serde_json::from_str(&client_defs_json(&defs)).unwrap()
}

/// A player a few hours in: Mining 12 with a perk, Melee 9 with a perk, Defense 3, Gathering 6, Arcane 32 (the
/// top), focused on Working, with 23 XP levels. The screen is open and the shipped English is loaded.
pub(crate) fn player(size: [u32; 2]) -> TestHost {
    crate::text::shipped_english::load();
    let at = |level: u32, perks: &[&str]| SkillProgress {
        level,
        points_spent: perks.len() as u32,
        perks: perks.iter().map(|p| skill(p)).collect(),
        ..SkillProgress::default()
    };
    let mut state = PlayerState {
        focus: [skill("working")].into(),
        focus_chosen_before: true,
        ..PlayerState::default()
    };
    state
        .skills
        .insert(skill("mining"), at(12, &["mining_sure_grip"]));
    state
        .skills
        .insert(skill("melee"), at(9, &["melee_steady_hand"]));
    state.skills.insert(skill("defense"), at(3, &[]));
    state.skills.insert(skill("gathering"), at(6, &[]));
    state.skills.insert(
        skill("arcane"),
        SkillProgress {
            level: 32,
            points_spent: 16,
            perks: [
                "arcane_scholars_ink",
                "arcane_frugal_smith",
                "arcane_ember_ward",
                "arcane_deep_lore",
                "arcane_quiet_mind",
            ]
            .iter()
            .map(|p| skill(p))
            .collect(),
            ..SkillProgress::default()
        },
    );
    with(|cache| {
        cache.defs = Some(shipped());
        cache.state = state;
        cache.xp_levels = Some(23);
        cache.my_name = Some("Steve".into());
        // A few ticks in, so the mastery glint is on its bar and a twinkle is lit.
        cache.ticks = 9;
    });
    TestHost {
        open_screen: Some(MOD_ID.into()),
        gui_size: [size[0] as f32, size[1] as f32],
        ..TestHost::default()
    }
}

fn open_skill(path: &str) {
    with(|cache| {
        let defs = cache.defs.as_ref().unwrap();
        cache.selected = defs
            .defs
            .skills
            .keys()
            .position(|id| *id == skill(path))
            .unwrap();
        cache.tab = Tab::Skill(cache.selected);
        cache.first_perk = 0;
    });
}

/// Draws the screen and gives back what was painted, saving it as a scene when previews are asked for.
fn scene(host: &TestHost, name: &str, size: [u32; 2], mouse: [f32; 2]) -> Vec<Paint> {
    host.painted.borrow_mut().clear();
    draw(
        host,
        &mut HudBatch::new(size),
        [size[0] as f32, size[1] as f32],
        mouse,
    );
    save(host, name, size)
}

fn save(host: &TestHost, name: &str, size: [u32; 2]) -> Vec<Paint> {
    let painted = host.painted.borrow().clone();
    if let Some(dir) = std::env::var_os("SKILLS_PREVIEW_DIR") {
        let scene = serde_json::json!({"size": size, "paints": painted});
        let path = PathBuf::from(dir).join(format!("{name}.json"));
        fs::write(path, serde_json::to_string(&scene).unwrap()).unwrap();
    }
    painted
}

/// Every text and icon below the tab row lies inside `window`.
fn assert_inside(host: &TestHost, painted: &[Paint], window: &super::layout::Rect, what: &str) {
    for paint in painted {
        let (x, y, w, h) = match paint {
            Paint::Text { text, at, .. } => (at[0], at[1], host.text_width(text), 8.0),
            Paint::Item { at, .. } if crate::client::screen::tests::in_tab_row(at[1]) => continue,
            Paint::Item { at, size, .. } => (at[0], at[1], *size, *size),
            _ => continue,
        };
        let fits = x >= window.x
            && y >= window.y
            && x + w <= window.x + window.w
            && y + h <= window.y + window.h;
        assert!(fits, "{what}: {paint:?} leaves {window:?}");
    }
}

/// No two texts on the same line run into each other.
fn assert_no_text_overlap(host: &TestHost, painted: &[Paint], what: &str) {
    let texts: Vec<(&String, f32, f32, f32)> = painted
        .iter()
        .filter_map(|paint| match paint {
            Paint::Text { text, at, .. } if !text.is_empty() => {
                Some((text, at[0], at[1], host.text_width(text)))
            }
            _ => None,
        })
        .collect();
    for (n, (a, ax, ay, aw)) in texts.iter().enumerate() {
        for (b, bx, by, bw) in &texts[n + 1..] {
            let apart = (ay - by).abs() >= 8.0 || ax + aw <= *bx || bx + bw <= *ax;
            assert!(apart, "{what}: {a:?} at {ax} runs into {b:?} at {bx}");
        }
    }
}

#[test]
fn the_shipped_home_fits_the_window() {
    for size in [SMALLEST, USUAL] {
        // Given the shipped mod and a player a few hours in
        let _serial = exclusive();
        let host = player(size);

        // When Home is drawn
        let painted = scene(&host, &format!("home_{}", size[0]), size, AWAY);

        // Then its eight skill rows are all there, inside the window's body, no text over another
        let rows = painted
            .iter()
            .filter(|p| matches!(p, Paint::Item { at, .. } if !crate::client::screen::tests::in_tab_row(at[1])))
            .count();
        assert_eq!(rows, 8);
        let body = super::window::window([size[0] as f32, size[1] as f32], 8, 0).body;
        assert_inside(&host, &painted, &body, "home");
        assert_no_text_overlap(&host, &painted, "home");
    }
}

#[test]
fn every_shipped_skill_view_fits_the_window() {
    for size in [SMALLEST, USUAL] {
        // Given the shipped mod and a player a few hours in
        let _serial = exclusive();
        let host = player(size);
        let skills: Vec<String> = with(|cache| {
            cache
                .defs
                .as_ref()
                .unwrap()
                .defs
                .skills
                .keys()
                .cloned()
                .collect()
        });

        for id in &skills {
            // When each skill's view is drawn
            let path = id.split_once(':').unwrap().1;
            open_skill(path);
            let painted = scene(&host, &format!("skill_{path}_{}", size[0]), size, AWAY);

            // Then it is inside the window, no text over another
            let body = super::window::window([size[0] as f32, size[1] as f32], 8, 0).body;
            assert_inside(&host, &painted, &body, path);
            assert_no_text_overlap(&host, &painted, path);
        }
    }
}

#[test]
fn hovering_shows_one_tooltip() {
    // Given the shipped mod on a usual screen
    let _serial = exclusive();
    let host = player(USUAL);
    let size = [USUAL[0] as f32, USUAL[1] as f32];
    let tips = |painted: &[Paint]| {
        painted
            .iter()
            .filter(|p| matches!(p, Paint::Tooltip { .. }))
            .count()
    };

    // When the mouse is over a skill's row, its raise button, a focus button, and in a skill's view over a perk
    let o = with(|cache| {
        let d = &cache.defs.as_ref().unwrap().defs;
        let body = super::window::window(size, 8, 0).body;
        super::home::home(&body, &super::home::groups(d), 0)
    });
    let over = |r: &super::layout::Rect| [r.x + 3.0, r.y + 3.0];
    let (_, row, raise_button) = o.rows[1];
    let card = scene(&host, "hover_row", USUAL, over(&row));
    let raise = scene(&host, "hover_raise", USUAL, over(&raise_button));
    let focus = scene(&host, "hover_focus", USUAL, over(&o.headings[0].2.unwrap()));
    scene(&host, "hover_focus_working", USUAL, over(&o.headings[2].1));
    open_skill("mining");
    let perk = scene(
        &host,
        "hover_perk",
        USUAL,
        over(
            &with(|cache| {
                let defs = cache.defs.as_ref().unwrap();
                let body = super::window::window(size, 8, 0).body;
                super::road::road_of(defs, &cache.state, "reskilled:mining", &body, 0.0)
            })
            .nodes[1]
                .1,
        ),
    );
    with(|cache| cache.tab = Tab::Options);
    let page = super::options::page(size, 11);
    scene(&host, "options", USUAL, over(&page.buttons[3]));
    with(|cache| cache.options_tab = super::options::Tab::Rules);
    scene(
        &host,
        "options_rules",
        USUAL,
        over(&super::options::page(size, 9).buttons[5]),
    );
    scene(&host, "options_320", SMALLEST, AWAY);

    // Then each shows exactly one tooltip
    assert_eq!(
        [tips(&card), tips(&raise), tips(&focus), tips(&perk)],
        [1; 4]
    );
}

#[test]
fn the_skill_strip_sits_beside_the_inventory() {
    // Given the shipped mod, a player a few hours in, and the inventory's panel on a usual screen
    let _serial = exclusive();
    let host = player(USUAL);
    let inventory = [125.0, 37.0, 176.0, 166.0];

    // When the inventory is drawn (a bare panel stands in for it) with the strip, plain and with Mining hovered
    for (name, mouse) in [("strip", AWAY), ("strip_hover", [107.0, 100.0])] {
        host.painted.borrow_mut().clear();
        let mut batch = HudBatch::new(USUAL);
        let [x, y, w, h] = inventory;
        super::paint::container_panel(&host, &mut batch, &super::layout::Rect { x, y, w, h });
        super::strip::draw_beside(&host, &mut batch, "inventory", None, inventory, mouse);
        let painted = save(&host, name, USUAL);

        // Then every skill is there, clear of the inventory and of the screen's edge
        let icons: Vec<[f32; 2]> = painted
            .iter()
            .filter_map(|p| match p {
                Paint::Item { at, .. } => Some(*at),
                _ => None,
            })
            .collect();
        assert_eq!(icons.len(), 8);
        assert!(
            icons
                .iter()
                .all(|at| at[0] > 0.0 && at[0] + 16.0 < inventory[0])
        );
    }
}

#[test]
fn the_warning_the_badge_and_the_item_tooltip_draw() {
    // Given the shipped mod, a refused chestplate, and the requirements of a few items
    let _serial = exclusive();
    let mut host = player(USUAL);
    host.open_screen = None;
    let needs = |action: Action, path: &str, level: u32| Requirements {
        by_action: [(action, [(skill(path), level)].into())].into(),
    };
    with(|cache| {
        cache.warning = Some(Warning {
            skill: skill("defense"),
            level: 16,
            action: Action::Equip,
            because: None,
            ticks_left: WARNING_TICKS,
        });
        let known = [
            (
                "minecraft:diamond_chestplate",
                needs(Action::Equip, "defense", 16),
            ),
            ("minecraft:iron_pickaxe", needs(Action::Break, "mining", 8)),
            (
                "minecraft:diamond_pickaxe",
                needs(Action::Break, "mining", 16),
            ),
            (
                "minecraft:chainmail_helmet",
                needs(Action::Equip, "defense", 4),
            ),
            ("minecraft:bread", Requirements::default()),
        ];
        for (item, reqs) in known {
            cache.reqs.insert(item.into(), reqs);
        }
        let mut axe = needs(Action::Break, "gathering", 16);
        axe.by_action
            .insert(Action::Attack, [(skill("melee"), 8)].into());
        cache.reqs.insert("minecraft:diamond_axe".into(), axe);
    });
    let stack = |item: &str| ItemStack {
        item: item.into(),
        count: 1,
        damage: 0,
        extra: None,
    };
    let mut batch = HudBatch::new(USUAL);

    // When the HUD draws its warning, a row of item icons gets its badges, and an item's tooltip is asked for
    dispatch_gui_render_post(&host, &mods(), &mut batch, false, true);
    let warning = save(&host, "warning", USUAL);
    host.painted.borrow_mut().clear();
    let items = [
        "minecraft:bread",
        "minecraft:iron_pickaxe",
        "minecraft:diamond_pickaxe",
        "minecraft:chainmail_helmet",
        "minecraft:diamond_chestplate",
        "minecraft:diamond_axe",
    ];
    for (n, item) in items.iter().enumerate() {
        let at = [120.0 + n as f32 * 20.0, 100.0];
        host.item(&mut batch, item, at, 16.0);
        dispatch_item_decorations(&host, &mods(), &stack(item), at[0], at[1], &mut batch);
    }
    let mut lines: Vec<(Text, u32)> = vec![(Text::literal("Diamond Axe"), u32::MAX)];
    dispatch_tooltip(&host, &mods(), &stack("minecraft:diamond_axe"), &mut lines);
    host.painted.borrow_mut().push(Paint::Tooltip {
        lines: lines.iter().map(|(line, _)| line.plain()).collect(),
        spans: lines.iter().map(|(line, _)| spans_of(line)).collect(),
        at: [150.0, 130.0],
    });
    let badges = save(&host, "items", USUAL);

    // Then the warning is there, the three locked items are dimmed, and the tooltip lists both of the axe's uses
    assert!(!warning.is_empty());
    let dimmed = badges
        .iter()
        .filter(|p| matches!(p, Paint::Fill { rect, .. } if rect[2] == 16.0 && rect[3] == 16.0))
        .count();
    assert_eq!(dimmed, 4);
    assert_eq!(
        lines
            .iter()
            .map(|(line, _)| line.plain())
            .collect::<Vec<_>>(),
        vec![
            "Diamond Axe",
            "",
            "Requirements",
            "\u{2714} Attack: Melee 8",
            "\u{2718} Break: Gathering 16 (you: 6)",
        ]
    );
}
