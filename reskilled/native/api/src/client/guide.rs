//! The guide page: one perk of the shown skill, played as a 3D scene when a guide file explains it, else shown as a
//! card of its exact values. Opened with the guide key over a perk row on the skill view.

use std::collections::BTreeMap;

use horizon_client::mod_api::{Flow, Host, HudBatch, NativeSceneBlock, NativeSceneView, Text};
use skills_core::card::{CardChance, CardLine, card_lines, with_modifier};
use skills_core::defs::{DamageKind, Direction, GuideDef, Operation, Overlay, PerkDef};
use skills_core::guide::{Frame, caption_values, frame_at, yaw_at};
use skills_core::progress::{PlayerState, active_perks};
use skills_core::registry::Defs;

use super::cache::{CLICK_SOUND, ClientCache, View, with};
use super::layout::{GUIDE_FOOTER_Y, GUIDE_RULE_Y, GuidePage, HEADER_H, Rect, page};
use super::paint::{self, GOLD, GRAY, Look, RED, WHITE};
use super::scene::scene_blocks;
use super::screen::{LEFT_BUTTON, PerkStatus, RIGHT_BUTTON, need_lines, owns, shown, status};
use crate::text::{
    BACK, CARD_ATTRIBUTE, CARD_ATTRIBUTE_CHANGE, CARD_CROP_GROWTH, CARD_DAMAGE_DEALT,
    CARD_DAMAGE_TAKEN, CARD_EXTRA_DROP, CARD_MOUNT, DAMAGE_ANY, DAMAGE_ENDER_PEARL,
    DAMAGE_EXPLOSION, DAMAGE_FALL, DAMAGE_FIRE, DAMAGE_MAGIC, DAMAGE_MELEE, DAMAGE_PROJECTILE,
    LOCKED, Message, OWNED, REPLAY, attribute_name, caption_text, card_value, perk_description,
    perk_name, say,
};

const TICKS_PER_SECOND: f32 = 20.0;
/// A line of text and the gap under it.
const LINE: f32 = 9.0;
/// Where the title starts after the Back button, and the header text's top.
const TITLE_GAP: f32 = 6.0;
const HEADER_TEXT: f32 = 7.0;
/// The inset of a card's text in the scene box.
const CARD_INSET: f32 = 6.0;
/// The progress bar's height, and its gap before Replay.
const PROGRESS_H: f32 = 5.0;
const PROGRESS_GAP: f32 = 6.0;
/// Overlay icons: items and effect icons are drawn this size, centred on their point.
const ICON: f32 = 16.0;
const EFFECT_SPRITE: f32 = 18.0;
/// The heart bar: a heart sprite's size, the step between hearts, and the hearts a full bar has at least.
const HEART: f32 = 9.0;
const HEART_STEP: f32 = 8.0;
const MIN_HEARTS: u32 = 10;
const HEART_CONTAINER: &str = "gui/sprites/hud/heart/container";
const HEART_FULL: &str = "gui/sprites/hud/heart/full";
const HEART_HALF: &str = "gui/sprites/hud/heart/half";
const EFFECT_SPRITES: &str = "gui/sprites/mob_effect/";
/// The space between the footer's parts.
const FOOTER_GAP: f32 = 10.0;
/// Card lines whose values are percents (already times 100), and the one whose first value is health points.
const PERCENT_VALUES: [&str; 5] = [
    "reskilled.card.enchanted_weapon",
    "reskilled.card.enchanted_armor",
    "reskilled.card.shorter_effects",
    "reskilled.card.xp_gain",
    "reskilled.card.anvil_cost",
];
const HEALTH_VALUE: &str = "reskilled.card.last_stand";

/// The guide page's perk: the shown skill's perk at `guide_perk`, and how many perks the skill has.
fn current(cache: &ClientCache) -> Option<(&Defs, &PerkDef, usize)> {
    let defs = cache.defs.as_ref()?;
    let perks = shown(defs, cache.selected).perks;
    let perk = perks.get(cache.guide_perk)?;
    Some((&defs.defs, perk, perks.len()))
}

/// Reads the client player's values of the attributes the guide page's card shows (the screen's drawing hooks
/// cannot read them).
pub fn read_attributes(host: &mut dyn Host) {
    let names: Vec<String> = with(|cache| {
        let Some((_, perk, _)) = current(cache) else {
            return Vec::new();
        };
        card_lines(perk)
            .into_iter()
            .filter_map(|line| match line {
                CardLine::Attribute {
                    attribute,
                    mount: false,
                    ..
                } => Some(attribute),
                _ => None,
            })
            .collect()
    });
    let values: BTreeMap<String, f64> = names
        .into_iter()
        .filter_map(|name| host.local_attribute(&name).map(|value| (name, value)))
        .collect();
    with(|cache| cache.attributes = values);
}

/// Opens the guide of the perk under the mouse on the skill view; false when there is none.
pub fn open_hovered() -> bool {
    with(|cache| match (cache.view, cache.hovered_perk) {
        (View::Skill, Some(perk)) => {
            cache.view = View::Guide;
            cache.guide_perk = perk;
            cache.guide_started = cache.ticks;
            cache.attributes.clear();
            true
        }
        _ => false,
    })
}

/// Draws the guide page.
pub fn draw(host: &dyn Host, batch: &mut HudBatch, size: [f32; 2], mouse: [f32; 2]) {
    host.fill(batch, [0.0, 0.0, size[0], size[1]], paint::SHADE);
    with(|cache| {
        let Some((d, perk, count)) = current(cache) else {
            return;
        };
        let p = page(size);
        let over = |r: &Rect| r.contains(mouse[0], mouse[1]);
        let mut tip: Option<Vec<Text>> = None;
        paint::panel(host, batch, &p.window, HEADER_H);
        paint::button(
            host,
            batch,
            &p.back,
            &say(&BACK, &[]).plain(),
            Look::Plain,
            over(&p.back),
        );
        let title_x = p.back.x + p.back.w + TITLE_GAP;
        let title = paint::fitted(
            host,
            &perk_name(d, &perk.id).plain(),
            p.prev.x - TITLE_GAP - title_x,
        );
        host.text(
            batch,
            &title,
            [title_x, p.window.y + HEADER_TEXT],
            GOLD,
            true,
        );
        let perks = shown(
            cache.defs.as_ref().expect("current found them"),
            cache.selected,
        )
        .perks;
        let at = cache.guide_perk;
        for (button, label, neighbour) in [
            (&p.prev, "<", at.checked_sub(1)),
            (&p.next, ">", Some(at + 1).filter(|n| *n < count)),
        ] {
            let look = if neighbour.is_some() {
                Look::Plain
            } else {
                Look::Off
            };
            paint::button(host, batch, button, label, look, over(button));
            if let (true, Some(n)) = (over(button), neighbour) {
                tip = Some(vec![perk_name(d, &perks[n].id).with_color(WHITE)]);
            }
        }
        let partial = host.client_state().map_or(0.0, |s| s.world_partial_tick);
        let ticks = cache.ticks.saturating_sub(cache.guide_started) as f64 + partial;
        let seconds = (ticks / f64::from(TICKS_PER_SECOND)) as f32;
        // The live values include the perk's modifier only while the server applies it.
        let applied = active_perks(d, &cache.state)
            .iter()
            .any(|active| active.id == perk.id);
        let lines = card_texts(perk, &cache.attributes, applied);
        paint::slot(host, batch, &p.scene);
        match d.guides.get(&perk.id) {
            Some(guide) => {
                draw_scene(host, batch, &p, guide, perk, seconds);
                paint::button(
                    host,
                    batch,
                    &p.replay,
                    &say(&REPLAY, &[]).plain(),
                    Look::Plain,
                    over(&p.replay),
                );
                draw_lines(host, batch, &p.lines, &lines);
            }
            None => {
                let card = p.scene.shrunk(CARD_INSET);
                let name = perk_name(d, &perk.id).plain();
                host.text(batch, &name, [card.x, card.y], GOLD, true);
                let below = Rect {
                    y: card.y + LINE + 3.0,
                    h: card.h - LINE - 3.0,
                    ..card
                };
                draw_lines(host, batch, &below, &lines);
            }
        }
        draw_footer(host, batch, &p, d, &cache.state, perk);
        if let Some(lines) = tip {
            host.gui_tooltip(batch, &lines, mouse);
        }
    });
}

/// The scene at `seconds`, its overlays, its caption and its progress.
fn draw_scene(
    host: &dyn Host,
    batch: &mut HudBatch,
    p: &GuidePage,
    guide: &GuideDef,
    perk: &PerkDef,
    seconds: f32,
) {
    let frame = frame_at(guide, seconds);
    let states = scene_blocks(host, &frame);
    let blocks: Vec<NativeSceneBlock<'_>> = states
        .iter()
        .map(|(pos, state)| NativeSceneBlock {
            pos: *pos,
            state: *state,
            entity: None,
        })
        .collect();
    let view = NativeSceneView {
        center: guide.view.center,
        yaw: yaw_at(guide, seconds),
        pitch: guide.view.pitch,
        scale: guide.view.scale,
    };
    let box_ = p.scene.shrunk(1.0);
    let corners = [box_.x, box_.y, box_.x + box_.w, box_.y + box_.h];
    host.draw_scene(batch, corners, &blocks, view);
    draw_overlays(host, batch, &box_, &frame);
    if let Some(caption) = &frame.caption {
        let text = caption_text(caption, &caption_values(perk, caption)).plain();
        let rows = paint::wrapped(host, &text, p.caption.w);
        let fit = (p.caption.h / LINE).floor() as usize;
        for (n, row) in rows.iter().take(fit).enumerate() {
            let y = p.caption.y + n as f32 * LINE;
            host.text(batch, row, [p.caption.x, y], WHITE, true);
        }
    }
    let bar = Rect {
        x: p.caption.x,
        y: p.replay.y + ((p.replay.h - PROGRESS_H) / 2.0).round(),
        w: p.replay.x - PROGRESS_GAP - p.caption.x,
        h: PROGRESS_H,
    };
    let thousandths = (frame.progress * 1000.0).round() as u32;
    paint::bar(host, batch, &bar, thousandths, 1000);
}

/// Item and effect icons centred on their points of the box, and the heart bar along its top left.
fn draw_overlays(host: &dyn Host, batch: &mut HudBatch, box_: &Rect, frame: &Frame) {
    for Overlay { item, effect, at } in &frame.icons {
        // Centred on its point, but never past the box's edges.
        let half = ICON / 2.0;
        let centre = [
            (box_.x + at[0] * box_.w).clamp(box_.x + half, box_.x + box_.w - half),
            (box_.y + at[1] * box_.h).clamp(box_.y + half, box_.y + box_.h - half),
        ];
        if let Some(item) = item {
            let corner = [centre[0] - ICON / 2.0, centre[1] - ICON / 2.0];
            host.item(batch, item, corner, ICON);
        }
        if let Some(effect) = effect {
            let path = effect.split_once(':').map_or(effect.as_str(), |(_, p)| p);
            let name = format!("{EFFECT_SPRITES}{path}");
            let dst = [centre[0] - ICON / 2.0, centre[1] - ICON / 2.0, ICON, ICON];
            host.sprite(batch, &name, [0.0, 0.0, EFFECT_SPRITE, EFFECT_SPRITE], dst);
        }
    }
    if let Some(half_hearts) = frame.hearts {
        let hearts = half_hearts.div_ceil(2).max(MIN_HEARTS);
        let src = [0.0, 0.0, HEART, HEART];
        for n in 0..hearts {
            let dst = [
                box_.x + CARD_INSET + n as f32 * HEART_STEP,
                box_.y + CARD_INSET,
                HEART,
                HEART,
            ];
            host.sprite(batch, HEART_CONTAINER, src, dst);
            let filled = half_hearts.saturating_sub(2 * n);
            if filled >= 2 {
                host.sprite(batch, HEART_FULL, src, dst);
            } else if filled == 1 {
                host.sprite(batch, HEART_HALF, src, dst);
            }
        }
    }
}

/// Lines of text, wrapped to the area, as many as fit.
fn draw_lines(host: &dyn Host, batch: &mut HudBatch, area: &Rect, lines: &[(String, u32)]) {
    let fit = (area.h / LINE).floor() as usize;
    let rows = lines.iter().flat_map(|(text, rgb)| {
        paint::wrapped(host, text, area.w)
            .into_iter()
            .map(move |row| (row, *rgb))
    });
    for (n, (row, rgb)) in rows.take(fit).enumerate() {
        host.text(batch, &row, [area.x, area.y + n as f32 * LINE], rgb, true);
    }
}

/// The footer: "Owned", or what the perk costs and needs, each part green once met, with "Locked" at its end
/// while the player cannot unlock it.
fn draw_footer(
    host: &dyn Host,
    batch: &mut HudBatch,
    p: &GuidePage,
    d: &Defs,
    state: &PlayerState,
    perk: &PerkDef,
) {
    let inner = p.window.shrunk(3.0);
    host.fill(
        batch,
        [inner.x, p.window.y + GUIDE_RULE_Y, inner.w, 1.0],
        paint::CARD_EDGE,
    );
    let y = p.window.y + GUIDE_FOOTER_Y;
    let (left, right) = (p.lines.x, p.lines.x + p.lines.w);
    if owns(state, &perk.id) {
        host.text(batch, &say(&OWNED, &[]).plain(), [left, y], GOLD, true);
        return;
    }
    let right = match status(d, state, perk) {
        PerkStatus::Locked(_) => {
            let locked = say(&LOCKED, &[]).plain();
            paint::text_right(host, batch, &locked, right, y, RED);
            right - host.text_width(&locked) - FOOTER_GAP
        }
        PerkStatus::Owned | PerkStatus::Buyable => right,
    };
    let mut x = left;
    for part in need_lines(d, state, perk) {
        let rgb = part
            .spans
            .first()
            .and_then(|s| s.style.color)
            .unwrap_or(GRAY);
        let text = paint::fitted(host, &part.plain(), right - x);
        if text.starts_with("...") || x >= right {
            break;
        }
        host.text(batch, &text, [x, y], rgb, true);
        x += host.text_width(&text) + FOOTER_GAP;
    }
}

/// The perk's card lines in words, each with its colour.
fn card_texts(
    perk: &PerkDef,
    attributes: &BTreeMap<String, f64>,
    owned: bool,
) -> Vec<(String, u32)> {
    card_lines(perk)
        .into_iter()
        .map(|line| match line {
            CardLine::Description => (perk_description(perk).plain(), GRAY),
            line => (card_line(&line, attributes, owned).plain(), WHITE),
        })
        .collect()
}

/// One card line in words.
fn card_line(line: &CardLine, attributes: &BTreeMap<String, f64>, owned: bool) -> Text {
    match line {
        CardLine::Attribute {
            attribute,
            amount,
            operation,
            mount,
        } => {
            let name = attribute_name(attribute);
            let change = Text::literal(change(*amount, *operation));
            match (mount, attributes.get(attribute)) {
                (true, _) => say(&CARD_MOUNT, &[name, change]),
                (false, None) => say(&CARD_ATTRIBUTE_CHANGE, &[name, change]),
                (false, Some(now)) => {
                    let (without, with) = if owned {
                        (without_modifier(*now, *amount, *operation), *now)
                    } else {
                        (*now, with_modifier(*now, *now, *amount, *operation))
                    };
                    let values = [Text::literal(value(without)), Text::literal(value(with))];
                    let [without, with] = values;
                    say(&CARD_ATTRIBUTE, &[name, without, with, change])
                }
            }
        }
        CardLine::Damage {
            direction,
            kind,
            multiplier,
        } => {
            let message = match direction {
                Direction::Dealt => &CARD_DAMAGE_DEALT,
                Direction::Taken => &CARD_DAMAGE_TAKEN,
            };
            let kind = say(damage_kind(*kind), &[]);
            say(message, &[kind, Text::literal(decimal(*multiplier))])
        }
        CardLine::Chance { what, chance } => {
            let message = match what {
                CardChance::ExtraDrop => &CARD_EXTRA_DROP,
                CardChance::CropGrowth => &CARD_CROP_GROWTH,
            };
            say(message, &[Text::literal(percent(chance * 100.0))])
        }
        CardLine::Value { key, args } => {
            let args: Vec<Text> = args
                .iter()
                .enumerate()
                .map(|(n, arg)| {
                    Text::literal(if PERCENT_VALUES.contains(key) {
                        percent(*arg)
                    } else if *key == HEALTH_VALUE && n == 0 {
                        decimal(arg / 2.0)
                    } else {
                        decimal(*arg)
                    })
                })
                .collect();
            card_value(key, &args)
        }
        CardLine::Description => Text::empty(),
    }
}

fn damage_kind(kind: DamageKind) -> &'static Message {
    match kind {
        DamageKind::Melee => &DAMAGE_MELEE,
        DamageKind::Projectile => &DAMAGE_PROJECTILE,
        DamageKind::Fall => &DAMAGE_FALL,
        DamageKind::Fire => &DAMAGE_FIRE,
        DamageKind::Explosion => &DAMAGE_EXPLOSION,
        DamageKind::EnderPearl => &DAMAGE_ENDER_PEARL,
        DamageKind::Magic => &DAMAGE_MAGIC,
        DamageKind::Any => &DAMAGE_ANY,
    }
}

/// An owned perk's attribute without its modifier, from the value with it. The base of `add_multiplied_base` is
/// taken as the value without the perk.
fn without_modifier(now: f64, amount: f64, operation: Operation) -> f64 {
    match operation {
        Operation::AddValue => now - amount,
        Operation::AddMultipliedBase | Operation::AddMultipliedTotal if amount > -1.0 => {
            now / (1.0 + amount)
        }
        Operation::AddMultipliedBase | Operation::AddMultipliedTotal => now,
    }
}

/// What a modifier changes: "+0.5" for an added value, "+10%" for the others.
fn change(amount: f64, operation: Operation) -> String {
    let sign = if amount >= 0.0 { "+" } else { "" };
    match operation {
        Operation::AddValue => format!("{sign}{}", decimal(amount)),
        Operation::AddMultipliedBase | Operation::AddMultipliedTotal => {
            format!("{sign}{}", percent(amount * 100.0))
        }
    }
}

/// A number with the fewest decimals that keep it, at most two: 5, 0.5, 2.25.
fn decimal(n: f64) -> String {
    let text = format!("{n:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0" } else { text }.to_string()
}

/// An attribute's value, with at least one decimal: 4.0, 4.4, 0.11.
fn value(n: f64) -> String {
    let text = decimal(n);
    if text.contains('.') {
        text
    } else {
        format!("{text}.0")
    }
}

fn percent(n: f64) -> String {
    format!("{}%", decimal(n))
}

/// A click on the guide page: Back (or the right button) to the skill view, the arrows to the skill's previous and
/// next perk, Replay to start the scene again. Every click on the page is taken.
pub fn click(_host: &dyn Host, size: [f32; 2], at: [f32; 2], button: i32) -> Flow {
    with(|cache| {
        let p = page(size);
        let hit = |r: &Rect| r.contains(at[0], at[1]);
        let Some((d, perk, count)) = current(cache) else {
            if button == RIGHT_BUTTON || hit(&p.back) {
                cache.view = View::Skill;
            }
            return;
        };
        let scene = d.guides.contains_key(&perk.id);
        let clicked = match button {
            RIGHT_BUTTON => {
                cache.view = View::Skill;
                true
            }
            LEFT_BUTTON if hit(&p.back) => {
                cache.view = View::Skill;
                true
            }
            LEFT_BUTTON if hit(&p.prev) && cache.guide_perk > 0 => {
                step_to(cache, cache.guide_perk - 1);
                true
            }
            LEFT_BUTTON if hit(&p.next) && cache.guide_perk + 1 < count => {
                step_to(cache, cache.guide_perk + 1);
                true
            }
            LEFT_BUTTON if hit(&p.replay) && scene => {
                cache.guide_started = cache.ticks;
                true
            }
            _ => false,
        };
        if clicked && cache.settings.click_sounds {
            cache.sounds.push(CLICK_SOUND);
        }
    });
    Flow::Cancel
}

/// Shows the skill's perk at `perk`, its scene from the start; its attributes are read on the next tick.
fn step_to(cache: &mut ClientCache, perk: usize) {
    cache.guide_perk = perk;
    cache.guide_started = cache.ticks;
    cache.attributes.clear();
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::{Host, HudBatch, dispatch_key};
    use skills_core::card::{CardChance, CardLine};
    use skills_core::defs::{
        DamageKind, DefFile, DefKind, Direction, Operation, SkillDef, parse_file,
    };
    use skills_core::progress::{PlayerState, SkillProgress};
    use skills_core::registry::Defs;
    use skills_core::wire::ClientDefs;

    use super::{card_line, card_texts};
    use crate::MOD_ID;
    use crate::client::cache::{View, with};
    use crate::client::layout::{detail, page};
    use crate::client::screen::{self, LEFT_BUTTON, RIGHT_BUTTON};
    use crate::test_host::{Paint, TestHost, exclusive, mods};

    const MELEE: &str = "reskilled:melee";
    const SIZE: [f32; 2] = [427.0, 240.0];
    const AWAY: [f32; 2] = [1.0, 1.0];
    const G: &str = "key.keyboard.g";

    const STEADY_HAND: &str = r#"{"format": 1, "id": "reskilled:melee_steady_hand", "skill": "reskilled:melee",
        "name": "reskilled.perk.melee_steady_hand", "description": "reskilled.perk.melee_steady_hand.desc",
        "cost": 2, "min_level": 4, "requires": [],
        "effects": [{"type": "attribute", "attribute": "minecraft:attack_speed", "amount": 0.1,
                     "operation": "add_multiplied_base"}]}"#;
    const SALVAGE: &str = r#"{"format": 1, "id": "reskilled:melee_salvage", "skill": "reskilled:melee",
        "name": "reskilled.perk.melee_salvage", "description": "reskilled.perk.melee_salvage.desc",
        "cost": 1, "min_level": 8, "requires": [], "effects": [{"type": "salvage"}]}"#;
    /// Stone at a quarter second, dirt on it at 0.4, in a four second loop turning 10 degrees a second.
    const SALVAGE_GUIDE: &str = r#"{"format": 1, "perk": "reskilled:melee_salvage", "seconds": 4,
        "view": {"center": [0.5, 0.5, 0.5], "yaw": 30, "pitch": 30, "scale": 20, "spin": 10},
        "steps": [
          {"at": 0, "caption": {"key": "reskilled.guide.test.1", "text": "A block is placed."}},
          {"at": 0.25, "place": [{"pos": [0, 0, 0], "block": "minecraft:stone"}]},
          {"at": 0.4, "place": [{"pos": [0, 1, 0], "block": "minecraft:dirt"}]}
        ]}"#;
    const STONE: u32 = 1;
    const DIRT: u32 = 2;

    /// The screen open on Melee's view: Steady Hand (row 0, a number perk) and Salvage (row 1, with a scene), the
    /// player at Melee 10 with no perks, attack speed 4.
    fn melee_open() -> TestHost {
        let perk = |json: &str| match parse_file(DefKind::Perk, json).unwrap() {
            DefFile::Perk(perk) => (perk.id.clone(), perk),
            _ => unreachable!(),
        };
        let DefFile::Guide(guide) = parse_file(DefKind::Guide, SALVAGE_GUIDE).unwrap() else {
            unreachable!()
        };
        let skill = SkillDef {
            id: MELEE.into(),
            priority: 0,
            name: "reskilled.skill.melee".into(),
            icon: "minecraft:iron_sword".into(),
            max_level: None,
            cost: None,
            points_every: None,
            icon_levels: Default::default(),
        };
        let defs = Defs {
            skills: [(MELEE.into(), skill)].into(),
            perks: [perk(STEADY_HAND), perk(SALVAGE)].into(),
            guides: [(guide.perk.clone(), guide)].into(),
            ..Defs::default()
        };
        let mut state = PlayerState::default();
        state.skills.insert(
            MELEE.into(),
            SkillProgress {
                level: 10,
                ..SkillProgress::default()
            },
        );
        with(|cache| {
            cache.defs = Some(ClientDefs {
                defs,
                labels: Default::default(),
            });
            cache.state = state;
            cache.xp_levels = Some(5);
            cache.selected = 0;
            cache.view = View::Skill;
            cache.ticks = 100;
        });
        let mut host = TestHost {
            open_screen: Some(MOD_ID.into()),
            ..TestHost::default()
        };
        host.local_attributes
            .insert("minecraft:attack_speed".into(), 4.0);
        host.default_states.insert("minecraft:stone".into(), STONE);
        host.default_states.insert("minecraft:dirt".into(), DIRT);
        host
    }

    fn row(index: usize) -> [f32; 2] {
        let rect = detail(SIZE, 2, 0).rows[index].rect;
        [rect.x + 30.0, rect.y + 4.0]
    }

    /// Draws the screen with the mouse at `mouse`, then presses the guide key.
    fn press_g_over(host: &mut TestHost, mouse: [f32; 2]) {
        screen::draw(host, &mut HudBatch::new([427, 240]), SIZE, mouse);
        dispatch_key(host, &mods(), G, true);
    }

    fn painted(host: &TestHost) -> Vec<Paint> {
        host.painted.borrow_mut().clear();
        host.drawn.borrow_mut().clear();
        screen::draw(host, &mut HudBatch::new([427, 240]), SIZE, AWAY);
        host.painted.borrow().clone()
    }

    /// A scene as drawn: its blocks' places and states, and its turn.
    type Drawn = (Vec<([i32; 3], u32)>, f32);

    fn scenes(host: &TestHost) -> Vec<Drawn> {
        painted(host)
            .into_iter()
            .filter_map(|paint| match paint {
                Paint::Scene { blocks, yaw, .. } => Some((blocks, yaw)),
                _ => None,
            })
            .collect()
    }

    fn inside(r: &crate::client::layout::Rect) -> [f32; 2] {
        [r.x + 2.0, r.y + 2.0]
    }

    #[test]
    fn g_over_a_perk_opens_its_guide() {
        // Given Melee's view with the mouse over Salvage's row
        let _serial = exclusive();
        let mut host = melee_open();

        // When G is pressed
        press_g_over(&mut host, row(1));

        // Then the guide page is up on Salvage, its scene starting now
        assert_eq!(
            with(|cache| (cache.view, cache.guide_perk, cache.guide_started)),
            (View::Guide, 1, 100)
        );
    }

    #[test]
    fn g_with_no_perk_under_the_mouse_does_nothing() {
        // Given Melee's view with the mouse away from the rows, and the overview with the mouse where a row would be
        let _serial = exclusive();
        let mut host = melee_open();

        // When G is pressed on each
        press_g_over(&mut host, AWAY);
        let on_skill = with(|cache| cache.view);
        with(|cache| cache.view = View::Overview);
        press_g_over(&mut host, row(1));

        // Then neither view changes
        assert_eq!(on_skill, View::Skill);
        assert_eq!(with(|cache| cache.view), View::Overview);
    }

    #[test]
    fn a_mechanic_perk_plays_its_scene() {
        // Given Salvage's guide open, half a second in
        let _serial = exclusive();
        let mut host = melee_open();
        press_g_over(&mut host, row(1));
        with(|cache| cache.ticks = cache.guide_started + 10);

        // When the page is drawn
        let scenes = scenes(&host);

        // Then one scene shows the stone and the dirt on it, turned 5 degrees past its start, under its caption
        assert_eq!(
            scenes,
            vec![(vec![([0, 0, 0], STONE), ([0, 1, 0], DIRT)], 35.0)]
        );
        assert!(
            host.drawn_text().iter().any(|t| t == "A block is placed."),
            "{:?}",
            host.drawn_text()
        );
    }

    #[test]
    fn a_number_perk_shows_its_card() {
        // Given Steady Hand's guide open for a player whose attack speed is 4
        let _serial = exclusive();
        let mut host = melee_open();
        press_g_over(&mut host, row(0));

        // When the page is drawn
        let scenes = scenes(&host);

        // Then there is no scene, and a card line shows the attack speed with the perk
        assert!(scenes.is_empty());
        let drawn = host.drawn_text();
        assert!(drawn.iter().any(|t| t.contains("4.4")), "{drawn:?}");
    }

    #[test]
    fn back_returns_to_the_skill_page() {
        // Given Salvage's guide open
        let _serial = exclusive();
        let mut host = melee_open();
        press_g_over(&mut host, row(1));

        // When Back is clicked, and once open again the right button is pressed
        screen::click(&host, SIZE, inside(&page(SIZE).back), LEFT_BUTTON);
        let after_back = with(|cache| cache.view);
        press_g_over(&mut host, row(1));
        screen::click(&host, SIZE, AWAY, RIGHT_BUTTON);

        // Then both return to the skill page
        assert_eq!(after_back, View::Skill);
        assert_eq!(with(|cache| cache.view), View::Skill);
    }

    #[test]
    fn stepping_or_replaying_restarts_the_scene() {
        // Given Steady Hand's page open, 50 ticks on
        let _serial = exclusive();
        let mut host = melee_open();
        press_g_over(&mut host, row(0));
        with(|cache| cache.ticks = 150);

        // When the next perk's arrow is clicked, and later Replay
        screen::click(&host, SIZE, inside(&page(SIZE).next), LEFT_BUTTON);
        let stepped = with(|cache| (cache.guide_perk, cache.guide_started));
        let first = scenes(&host);
        with(|cache| cache.ticks = 170);
        let played = scenes(&host);
        screen::click(&host, SIZE, inside(&page(SIZE).replay), LEFT_BUTTON);
        let replayed = with(|cache| cache.guide_started);
        let again = scenes(&host);

        // Then each starts Salvage's scene at the current tick, from an empty diorama
        assert_eq!(stepped, (1, 150));
        assert_eq!(first, vec![(vec![], 30.0)]);
        assert_eq!(played[0].0.len(), 2);
        assert_eq!(replayed, 170);
        assert_eq!(again, vec![(vec![], 30.0)]);
    }

    #[test]
    fn the_tooltip_says_how_to_open_the_guide() {
        // Given Melee's view
        let _serial = exclusive();
        let host = melee_open();

        // When the mouse is over Steady Hand's row
        host.painted.borrow_mut().clear();
        screen::draw(&host, &mut HudBatch::new([427, 240]), SIZE, row(0));

        // Then its tooltip ends with the guide key's hint
        let tip = host
            .painted
            .borrow()
            .iter()
            .find_map(|paint| match paint {
                Paint::Tooltip { lines, .. } => Some(lines.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            tip.last().map(String::as_str),
            Some("Press G to see it in action")
        );
    }

    #[test]
    fn card_lines_read_in_words() {
        // Given an owned Steady Hand at attack speed 4.4, a damage line, a drop chance and a last stand above 6
        // health points
        let attributes = [("minecraft:attack_speed".to_string(), 4.4)].into();
        let steady = CardLine::Attribute {
            attribute: "minecraft:attack_speed".into(),
            amount: 0.1,
            operation: Operation::AddMultipliedBase,
            mount: false,
        };
        let mount = CardLine::Attribute {
            attribute: "minecraft:movement_speed".into(),
            amount: 0.5,
            operation: Operation::AddValue,
            mount: true,
        };
        let damage = CardLine::Damage {
            direction: Direction::Dealt,
            kind: DamageKind::Projectile,
            multiplier: 1.08,
        };
        let drop = CardLine::Chance {
            what: CardChance::ExtraDrop,
            chance: 0.25,
        };
        let stand = CardLine::Value {
            key: "reskilled.card.last_stand",
            args: vec![6.0, 300.0],
        };
        let weapon = CardLine::Value {
            key: "reskilled.card.enchanted_weapon",
            args: vec![3.0, 15.0],
        };

        // When each is put in words, with the shipped English
        let _serial = exclusive();
        crate::text::shipped_english::load();
        let words: Vec<String> = [steady, mount, damage, drop, stand, weapon]
            .iter()
            .map(|line| card_line(line, &attributes, true).plain())
            .collect();

        // Then the owned attribute shows its value without the perk first, the rest their own numbers
        assert_eq!(
            words,
            [
                "Attack speed: 4.0 -> 4.4 (+10%)",
                "Movement speed of your mount: +0.5",
                "Projectile damage dealt x1.08",
                "25% chance of an extra drop",
                "Above 3 hearts, a killing blow leaves half a heart; then it rests 300 seconds",
                "+3% damage per enchantment on your weapon, up to 15%",
            ]
        );
    }

    #[test]
    fn a_perk_with_no_card_line_shows_its_description() {
        // Given Salvage, whose only effect has no card line
        let _serial = exclusive();
        let _host = melee_open();
        let perk = with(|cache| {
            cache.defs.as_ref().unwrap().defs.perks["reskilled:melee_salvage"].clone()
        });

        // When its card is put in words
        let lines = card_texts(&perk, &Default::default(), false);

        // Then it is its description alone (empty here: the test has no English for it)
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn the_page_fits_the_window_at_both_sizes() {
        // Given both perks' pages, a scene and a card, with the shipped English
        let _serial = exclusive();
        crate::text::shipped_english::load();
        let host = melee_open();
        for size in [[320.0, 240.0], [427.0, 240.0]] {
            for perk in [0, 1] {
                with(|cache| {
                    cache.view = View::Guide;
                    cache.guide_perk = perk;
                });

                // When the page is drawn
                host.painted.borrow_mut().clear();
                let batch_size = [size[0] as u32, size[1] as u32];
                screen::draw(&host, &mut HudBatch::new(batch_size), size, AWAY);

                // Then every text, icon, sprite and the scene lie inside the window, clear of its frame
                let window = page(size).window.shrunk(3.0);
                for paint in host.painted.borrow().iter() {
                    let (x, y, w, h) = match paint {
                        Paint::Text { text, at, .. } => (at[0], at[1], host.text_width(text), 8.0),
                        Paint::Item { at, size, .. } => (at[0], at[1], *size, *size),
                        Paint::Sprite { rect, .. } => (rect[0], rect[1], rect[2], rect[3]),
                        Paint::Scene { rect, .. } => {
                            (rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1])
                        }
                        _ => continue,
                    };
                    let fits = x >= window.x
                        && y >= window.y
                        && x + w <= window.x + window.w
                        && y + h <= window.y + window.h;
                    assert!(fits, "{size:?} perk {perk}: {paint:?} leaves {window:?}");
                }
            }
        }
    }

    #[test]
    fn a_switched_off_perk_shows_the_numbers_it_would_give() {
        // Given Steady Hand owned but switched off, so the live attack speed of 4 lacks its modifier
        let _serial = exclusive();
        let mut host = melee_open();
        with(|cache| {
            let melee = cache.state.skills.get_mut(MELEE).unwrap();
            melee.perks.insert("reskilled:melee_steady_hand".into());
            melee.points_spent = 2;
            melee.disabled.insert("reskilled:melee_steady_hand".into());
        });

        // When its page is drawn
        press_g_over(&mut host, row(0));
        painted(&host);

        // Then the card reads 4 without the perk and 4.4 with it
        let drawn = host.drawn_text();
        assert!(drawn.iter().any(|t| t.contains("4.0 -> 4.4")), "{drawn:?}");
    }

    #[test]
    fn the_footer_says_owned_or_locked() {
        // Given Steady Hand owned and Salvage needing more perk points than the player has
        let _serial = exclusive();
        let mut host = melee_open();
        with(|cache| {
            let melee = cache.state.skills.get_mut(MELEE).unwrap();
            melee.perks.insert("reskilled:melee_steady_hand".into());
            melee.points_spent = 2;
            cache
                .defs
                .as_mut()
                .unwrap()
                .defs
                .perks
                .get_mut("reskilled:melee_salvage")
                .unwrap()
                .cost = 9;
        });

        // When each page is drawn
        press_g_over(&mut host, row(0));
        painted(&host);
        let owned = host.drawn_text();
        with(|cache| cache.guide_perk = 1);
        painted(&host);
        let locked = host.drawn_text();

        // Then the first says Owned and the second Locked
        assert!(owned.iter().any(|t| t == "Owned"), "{owned:?}");
        assert!(locked.iter().any(|t| t == "Locked"), "{locked:?}");
    }
}
