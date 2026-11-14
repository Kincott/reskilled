//! Playing a perk guide's timeline: what the diorama holds at a moment of the scene. No game dependency; the
//! client turns a [`Frame`] into drawing.

use std::collections::BTreeMap;

use crate::defs::{
    Caption, ClearWhat, GuideAction, GuideDef, Overlay, PerkDef, ShownAs, effect_field,
};

/// Everything the scene shows at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// The block state string at each scene position.
    pub blocks: BTreeMap<[i32; 3], String>,
    pub caption: Option<Caption>,
    /// Item and effect overlays, in the order their steps ran.
    pub icons: Vec<Overlay>,
    /// Half hearts of the heart bar, if one is shown.
    pub hearts: Option<u32>,
    /// How far through the loop, 0 to 1.
    pub progress: f32,
}

/// The scene `seconds_since_start` after it began. The timeline loops at `guide.seconds`, each loop starting again
/// from an empty diorama, and every step at or before that moment is applied in file order.
pub fn frame_at(guide: &GuideDef, seconds_since_start: f32) -> Frame {
    let length = guide.seconds;
    let t = if length > 0.0 && seconds_since_start.is_finite() {
        seconds_since_start.rem_euclid(length)
    } else {
        0.0
    };
    let mut frame = Frame {
        blocks: BTreeMap::new(),
        caption: None,
        icons: Vec::new(),
        hearts: None,
        progress: if length > 0.0 {
            (t / length).clamp(0.0, 1.0)
        } else {
            0.0
        },
    };
    for step in guide.steps.iter().filter(|step| step.at <= t) {
        match &step.action {
            GuideAction::Place(placed) => {
                for block in placed {
                    frame.blocks.insert(block.pos, block.block.clone());
                }
            }
            GuideAction::Remove(positions) => {
                for pos in positions {
                    frame.blocks.remove(pos);
                }
            }
            GuideAction::Caption(caption) => frame.caption = Some(caption.clone()),
            GuideAction::Icon(overlay) | GuideAction::Effect(overlay) => {
                frame.icons.push(overlay.clone())
            }
            GuideAction::Hearts(half_hearts) => frame.hearts = Some(*half_hearts),
            GuideAction::Clear(ClearWhat::Icons) => frame.icons.clear(),
            GuideAction::Clear(ClearWhat::Hearts) => frame.hearts = None,
        }
    }
    frame
}

/// The yaw the scene is looked at from. The time is not looped, so the turn stays smooth across loops.
pub fn yaw_at(guide: &GuideDef, seconds_since_start: f32) -> f32 {
    guide.view.yaw + guide.view.spin * seconds_since_start
}

/// The text each of a caption's args fills its `%s` with, read from the perk's own effects so the scene never states
/// a number the perk data does not. A value the perk lacks reads `?`.
pub fn caption_values(perk: &PerkDef, caption: &Caption) -> Vec<String> {
    caption
        .args
        .iter()
        .map(|arg| match effect_field(perk, &arg.effect, &arg.field) {
            None => "?".to_string(),
            Some(value) => match arg.shown_as {
                ShownAs::Number => number(value),
                ShownAs::Percent => format!("{}%", number(value * 100.0)),
                ShownAs::Seconds => format!("{} s", number(value)),
                ShownAs::Blocks => format!("{} blocks", number(value)),
                ShownAs::Hearts => format!("{} hearts", number(value / 2.0)),
            },
        })
        .collect()
}

/// A number with the fewest decimals that keep it, at most two: 5, 0.5, 2.25.
fn number(value: f64) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0" } else { text }.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::{DefFile, DefKind, parse_file};

    const REPLANT: &str = r#"{
      "format": 1,
      "perk": "reskilled:gathering_sapling_keeper",
      "seconds": 8,
      "view": {"center": [1.5, 0.5, 1.5], "yaw": 35, "pitch": 30, "scale": 22, "spin": 8},
      "steps": [
        {"at": 0, "place": [{"pos": [0, 0, 0], "block": "minecraft:farmland"},
                            {"pos": [0, 1, 0], "block": "minecraft:wheat[age=7]"}]},
        {"at": 0, "caption": {"key": "reskilled.guide.replant.1", "text": "A ripe crop is broken."}},
        {"at": 3, "remove": [[0, 1, 0]]},
        {"at": 3, "icon": {"item": "minecraft:wheat_seeds", "at": [0.55, 0.4]}},
        {"at": 4, "place": [{"pos": [0, 1, 0], "block": "minecraft:wheat[age=0]"}]},
        {"at": 4, "caption": {"key": "reskilled.guide.replant.2",
                              "text": "It is planted again with one of its own seeds."}}
      ]
    }"#;

    fn guide(json: &str) -> GuideDef {
        match parse_file(DefKind::Guide, json) {
            Ok(DefFile::Guide(guide)) => guide,
            other => panic!("expected a guide, got {other:?}"),
        }
    }

    #[test]
    fn a_frame_holds_what_the_steps_placed_so_far() {
        // Given the replant scene
        let replant = guide(REPLANT);

        // When it is played to 2.0 seconds
        let early = frame_at(&replant, 2.0);

        // Then both blocks stand and the first caption is up
        assert_eq!(early.blocks.len(), 2);
        assert_eq!(early.blocks[&[0, 0, 0]], "minecraft:farmland");
        assert_eq!(early.blocks[&[0, 1, 0]], "minecraft:wheat[age=7]");
        assert_eq!(
            early.caption.as_ref().unwrap().key,
            "reskilled.guide.replant.1"
        );
        assert!(early.icons.is_empty());
        assert_eq!(early.hearts, None);
        assert_eq!(early.progress, 0.25);

        // When it is played to 3.5 seconds
        let broken = frame_at(&replant, 3.5);

        // Then the crop is gone and the seed icon shows
        assert_eq!(broken.blocks.len(), 1);
        assert!(!broken.blocks.contains_key(&[0, 1, 0]));
        assert_eq!(broken.icons.len(), 1);
        assert_eq!(
            broken.icons[0].item.as_deref(),
            Some("minecraft:wheat_seeds")
        );

        // When it is played to 4.5 seconds
        let planted = frame_at(&replant, 4.5);

        // Then young wheat is back and the second caption replaced the first
        assert_eq!(planted.blocks[&[0, 1, 0]], "minecraft:wheat[age=0]");
        assert_eq!(
            planted.caption.as_ref().unwrap().key,
            "reskilled.guide.replant.2"
        );
    }

    #[test]
    fn a_step_at_the_exact_moment_has_happened() {
        // Given the replant scene
        let replant = guide(REPLANT);

        // When it is played to exactly 3 seconds
        let frame = frame_at(&replant, 3.0);

        // Then the step at 3 seconds has already run
        assert!(!frame.blocks.contains_key(&[0, 1, 0]));
        assert_eq!(frame.icons.len(), 1);
    }

    #[test]
    fn the_timeline_loops_from_an_empty_scene() {
        // Given the replant scene, 8 seconds long
        let replant = guide(REPLANT);

        // When it is played half a second past the end of the loop
        let again = frame_at(&replant, replant.seconds + 0.5);

        // Then it is the same as half a second in, with no leftovers of the last loop
        assert_eq!(again, frame_at(&replant, 0.5));
        assert_eq!(again.blocks[&[0, 1, 0]], "minecraft:wheat[age=7]");
        assert!(again.icons.is_empty());
    }

    #[test]
    fn a_timeline_that_has_not_begun_or_has_no_length_is_empty() {
        // Given a scene with a step at zero
        let mut replant = guide(REPLANT);

        // When it is played before it began
        let before = frame_at(&replant, -1.0);

        // Then it loops back from the end, never panicking
        assert_eq!(before, frame_at(&replant, 7.0));

        // When its length is zero
        replant.seconds = 0.0;
        let flat = frame_at(&replant, 5.0);

        // Then it shows the start with no progress
        assert_eq!(flat.progress, 0.0);
        assert_eq!(flat.blocks.len(), 2);
    }

    #[test]
    fn clear_takes_the_icons_off() {
        // Given a scene that shows an icon, an effect and hearts, then clears icons, then hearts
        let json = r#"{
          "format": 1, "perk": "reskilled:x", "seconds": 10,
          "view": {"center": [0, 0, 0], "yaw": 0, "pitch": 0, "scale": 10},
          "steps": [
            {"at": 1, "icon": {"item": "minecraft:apple", "at": [0.1, 0.1]}},
            {"at": 1, "effect": {"effect": "minecraft:strength", "at": [0.9, 0.1]}},
            {"at": 1, "hearts": 20},
            {"at": 2, "clear": "icons"},
            {"at": 3, "hearts": 1},
            {"at": 4, "clear": "hearts"}
          ]
        }"#;
        let scene = guide(json);

        // When it is played while they show
        let shown = frame_at(&scene, 1.5);

        // Then the item and the effect are both icons, over a full bar
        assert_eq!(shown.icons.len(), 2);
        assert_eq!(shown.icons[1].effect.as_deref(), Some("minecraft:strength"));
        assert_eq!(shown.hearts, Some(20));

        // When icons are cleared
        let cleared = frame_at(&scene, 2.5);

        // Then no icons remain but the hearts do
        assert!(cleared.icons.is_empty());
        assert_eq!(cleared.hearts, Some(20));

        // When the hearts drain and are then cleared
        let drained = frame_at(&scene, 3.5);
        let gone = frame_at(&scene, 4.5);

        // Then the bar follows the steps
        assert_eq!(drained.hearts, Some(1));
        assert_eq!(gone.hearts, None);
    }

    #[test]
    fn the_view_turns_by_its_spin() {
        // Given a scene that starts at 35 degrees and turns 8 degrees a second
        let replant = guide(REPLANT);

        // When the yaw is read at the start, after 2 seconds and in the second loop
        // Then it grows with time and is not looped
        assert_eq!(yaw_at(&replant, 0.0), 35.0);
        assert_eq!(yaw_at(&replant, 2.0), 51.0);
        assert_eq!(yaw_at(&replant, 10.0), 115.0);
    }

    fn caption_of(args: &str) -> Caption {
        let json = format!(r#"{{"key": "k", "text": "t", "args": {args}}}"#);
        serde_json::from_str(&json).unwrap()
    }

    fn perk(file: &str) -> PerkDef {
        match parse_file(DefKind::Perk, file) {
            Ok(DefFile::Perk(perk)) => perk,
            other => panic!("expected a perk, got {other:?}"),
        }
    }

    #[test]
    fn caption_values_fill_from_the_perk() {
        // Given Green Aura and Last Breath, as shipped
        let aura = perk(include_str!(
            "../../../content/data/reskilled/reskilled/perks/gathering_foragers_pace.json"
        ));
        let breath = perk(include_str!(
            "../../../content/data/reskilled/reskilled/perks/defense_braced_stance.json"
        ));

        // When a caption names Green Aura's radius, chance and interval
        let caption = caption_of(
            r#"[{"effect": "crop_growth", "field": "radius", "as": "blocks"},
                {"effect": "crop_growth", "field": "chance", "as": "percent"},
                {"effect": "crop_growth", "field": "interval_seconds", "as": "seconds"}]"#,
        );

        // Then each is shown as it was asked
        assert_eq!(caption_values(&aura, &caption), ["5 blocks", "25%", "10 s"]);

        // When a caption names Last Breath's threshold as hearts
        let caption = caption_of(r#"[{"effect": "last_stand", "field": "above", "as": "hearts"}]"#);

        // Then the health points are halved
        assert_eq!(caption_values(&breath, &caption), ["3 hearts"]);
    }

    #[test]
    fn caption_values_format_numbers_and_flag_missing_values() {
        // Given Green Aura
        let aura = perk(include_str!(
            "../../../content/data/reskilled/reskilled/perks/gathering_foragers_pace.json"
        ));

        // When the chance is shown as a plain number, a halved radius as hearts, and a missing field
        let caption = caption_of(
            r#"[{"effect": "crop_growth", "field": "chance", "as": "number"},
                {"effect": "crop_growth", "field": "radius", "as": "number"},
                {"effect": "crop_growth", "field": "radius", "as": "hearts"},
                {"effect": "crop_growth", "field": "speed", "as": "number"},
                {"effect": "replant", "field": "radius", "as": "blocks"}]"#,
        );

        // Then whole numbers have no decimals, others at most two, and missing ones read ?
        assert_eq!(
            caption_values(&aura, &caption),
            ["0.25", "5", "2.5 hearts", "?", "?"]
        );
        assert_eq!(number(2.25), "2.25");
        assert_eq!(number(0.5), "0.5");
        assert_eq!(number(1.0 / 3.0), "0.33");

        // When a caption has no args
        // Then it has no values
        assert!(caption_values(&aura, &caption_of("[]")).is_empty());
    }
}
