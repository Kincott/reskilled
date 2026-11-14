//! Card lines: what a number perk's card shows, one line per effect, built from the perk's data. No game
//! dependency; the client turns a [`CardLine`] into text, and looks up the player's attribute values itself.

use crate::defs::{DamageKind, Direction, Effect, Operation, PerkDef};

/// One line of a perk's card.
#[derive(Debug, Clone, PartialEq)]
pub enum CardLine {
    /// An attribute modifier, on the player or, with `mount`, on the mount they ride.
    Attribute {
        attribute: String,
        amount: f64,
        operation: Operation,
        mount: bool,
    },
    /// Damage dealt or taken, scaled by `multiplier`.
    Damage {
        direction: Direction,
        kind: DamageKind,
        multiplier: f64,
    },
    /// A chance (0 to 1) of something happening.
    Chance { what: CardChance, chance: f64 },
    /// A line of numbers: `key` is a translation key under `reskilled.card.`, and `args` fill it in order.
    /// Percents are already multiplied by 100.
    Value { key: &'static str, args: Vec<f64> },
    /// The perk's own description: it has no effect with a line of its own.
    Description,
}

/// What a [`CardLine::Chance`] is a chance of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardChance {
    /// An extra drop when breaking a matching block.
    ExtraDrop,
    /// A crop growing a stage.
    CropGrowth,
}

/// The card lines of a perk, one per effect in order. A perk with no effects, or only effects with no line of their
/// own, gives just [`CardLine::Description`].
pub fn card_lines(perk: &PerkDef) -> Vec<CardLine> {
    let lines: Vec<CardLine> = perk.effects.iter().filter_map(effect_line).collect();
    if lines.is_empty() {
        vec![CardLine::Description]
    } else {
        lines
    }
}

/// Vanilla's player base values of the attributes the shipped perks change with `add_multiplied_base`. A held item's
/// modifiers move the live value but not the base, so the base cannot be read off the live value.
const PLAYER_BASES: [(&str, f64); 6] = [
    ("minecraft:attack_speed", 4.0),
    ("minecraft:movement_speed", 0.1),
    ("minecraft:jump_strength", 0.42),
    ("minecraft:block_break_speed", 1.0),
    ("minecraft:submerged_mining_speed", 0.2),
    ("minecraft:sneaking_speed", 0.3),
];

/// A player's base value of `attribute` in vanilla; `None` for an attribute not in [`PLAYER_BASES`].
pub fn player_base(attribute: &str) -> Option<f64> {
    PLAYER_BASES
        .iter()
        .find(|(name, _)| *name == attribute)
        .map(|(_, base)| *base)
}

/// An attribute's value once a modifier is added to it: `now` is the value now, `base` its base value.
/// Vanilla's three operations: `add_value` adds `amount`, `add_multiplied_base` adds `base * amount`, and
/// `add_multiplied_total` multiplies the running total by `1 + amount`.
pub fn with_modifier(now: f64, base: f64, amount: f64, operation: Operation) -> f64 {
    match operation {
        Operation::AddValue => now + amount,
        Operation::AddMultipliedBase => now + base * amount,
        Operation::AddMultipliedTotal => now * (1.0 + amount),
    }
}

fn effect_line(effect: &Effect) -> Option<CardLine> {
    Some(match effect {
        Effect::Attribute {
            attribute,
            amount,
            operation,
        } => CardLine::Attribute {
            attribute: attribute.clone(),
            amount: *amount,
            operation: *operation,
            mount: false,
        },
        Effect::MountAttribute {
            attribute,
            amount,
            operation,
        } => CardLine::Attribute {
            attribute: attribute.clone(),
            amount: *amount,
            operation: *operation,
            mount: true,
        },
        Effect::Damage {
            direction,
            kind,
            multiplier,
        } => CardLine::Damage {
            direction: *direction,
            kind: *kind,
            multiplier: *multiplier,
        },
        Effect::DropBonus { chance, .. } => CardLine::Chance {
            what: CardChance::ExtraDrop,
            chance: *chance,
        },
        Effect::CropGrowth { chance, .. } => CardLine::Chance {
            what: CardChance::CropGrowth,
            chance: *chance,
        },
        Effect::EnchantedWeapon {
            per_enchantment,
            max,
        } => value(
            "reskilled.card.enchanted_weapon",
            [percent(*per_enchantment), percent(*max)],
        ),
        Effect::EnchantedArmor {
            per_enchantment,
            max,
        } => value(
            "reskilled.card.enchanted_armor",
            [percent(*per_enchantment), percent(*max)],
        ),
        Effect::ShorterEffects { multiplier, .. } => {
            value("reskilled.card.shorter_effects", [percent(*multiplier)])
        }
        Effect::RuneRepair {
            interval_seconds,
            points,
        } => value(
            "reskilled.card.rune_repair",
            [f64::from(*points), f64::from(*interval_seconds)],
        ),
        Effect::LastStand {
            above,
            cooldown_seconds,
        } => value(
            "reskilled.card.last_stand",
            [f64::from(*above), f64::from(*cooldown_seconds)],
        ),
        Effect::HomeWard { radius } => value("reskilled.card.home_ward", [f64::from(*radius)]),
        Effect::SafeHaven { radius } => value("reskilled.card.safe_haven", [f64::from(*radius)]),
        Effect::NearHome { radius, .. } => value("reskilled.card.near_home", [f64::from(*radius)]),
        Effect::XpGain { multiplier } => value("reskilled.card.xp_gain", [percent(*multiplier)]),
        Effect::AnvilCost { multiplier } => {
            value("reskilled.card.anvil_cost", [percent(*multiplier)])
        }
        Effect::Salvage
        | Effect::RestfulSleep { .. }
        | Effect::Replant { .. }
        | Effect::EffectOnKill { .. } => return None,
    })
}

fn value<const N: usize>(key: &'static str, args: [f64; N]) -> CardLine {
    CardLine::Value {
        key,
        args: args.to_vec(),
    }
}

/// A fraction as a percent, without the float noise of `0.07 * 100`.
fn percent(fraction: f64) -> f64 {
    (fraction * 100.0 * 1e6).round() / 1e6
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::{DefFile, DefKind, parse_file};

    fn perk(json: &str) -> PerkDef {
        match parse_file(DefKind::Perk, json) {
            Ok(DefFile::Perk(perk)) => perk,
            other => panic!("expected a perk, got {other:?}"),
        }
    }

    fn perk_with(effects: &str) -> PerkDef {
        perk(&format!(
            r#"{{"format": 1, "id": "reskilled:test", "skill": "reskilled:agility",
                "name": "n", "description": "d", "cost": 1, "min_level": 1, "effects": {effects}}}"#
        ))
    }

    #[test]
    fn number_perks_get_a_line_per_effect() {
        // Given Wind Runner, which speeds the player up and cuts fall damage
        let wind_runner = perk(include_str!(
            "../../../content/data/reskilled/reskilled/perks/agility_wind_runner.json"
        ));

        // When its card lines are built
        let lines = card_lines(&wind_runner);

        // Then there is a movement speed line and a fall damage taken line, in order
        assert_eq!(
            lines,
            vec![
                CardLine::Attribute {
                    attribute: "minecraft:movement_speed".into(),
                    amount: 0.05,
                    operation: Operation::AddMultipliedBase,
                    mount: false,
                },
                CardLine::Damage {
                    direction: Direction::Taken,
                    kind: DamageKind::Fall,
                    multiplier: 0.7,
                },
            ]
        );
    }

    #[test]
    fn mount_and_chance_effects_get_their_lines() {
        // Given a perk with a mount attribute, a drop bonus and crop growth
        let perk = perk_with(
            r##"[
              {"type": "mount_attribute", "attribute": "minecraft:movement_speed", "amount": 0.1,
               "operation": "add_multiplied_total"},
              {"type": "drop_bonus", "target": {"tag": "#minecraft:logs"}, "chance": 0.25},
              {"type": "crop_growth", "radius": 4, "interval_seconds": 10, "chance": 0.2}
            ]"##,
        );

        // When its card lines are built
        let lines = card_lines(&perk);

        // Then the mount line is marked, and each chance effect gives only its chance line
        assert_eq!(
            lines,
            vec![
                CardLine::Attribute {
                    attribute: "minecraft:movement_speed".into(),
                    amount: 0.1,
                    operation: Operation::AddMultipliedTotal,
                    mount: true,
                },
                CardLine::Chance {
                    what: CardChance::ExtraDrop,
                    chance: 0.25,
                },
                CardLine::Chance {
                    what: CardChance::CropGrowth,
                    chance: 0.2,
                },
            ]
        );
    }

    #[test]
    fn rune_perks_get_their_values() {
        // Given Runeblade: 3% more damage per enchantment, up to 15%
        let runeblade = perk(include_str!(
            "../../../content/data/reskilled/reskilled/perks/arcane_frugal_smith.json"
        ));

        // When its card lines are built
        let lines = card_lines(&runeblade);

        // Then the line carries both numbers as percents
        assert_eq!(
            lines,
            vec![CardLine::Value {
                key: "reskilled.card.enchanted_weapon",
                args: vec![3.0, 15.0],
            }]
        );
    }

    #[test]
    fn other_effects_get_their_values() {
        // Given a perk with several effects that show as plain numbers
        let perk = perk_with(
            r#"[
              {"type": "shorter_effects", "effects": ["minecraft:poison"], "multiplier": 0.6},
              {"type": "rune_repair", "interval_seconds": 30, "points": 2},
              {"type": "last_stand", "above": 10.0, "cooldown_seconds": 300},
              {"type": "home_ward", "radius": 32},
              {"type": "xp_gain", "multiplier": 1.07}
            ]"#,
        );

        // When its card lines are built
        let lines = card_lines(&perk);

        // Then each gives its own key and numbers, without float noise in the percents
        assert_eq!(
            lines,
            vec![
                CardLine::Value {
                    key: "reskilled.card.shorter_effects",
                    args: vec![60.0],
                },
                CardLine::Value {
                    key: "reskilled.card.rune_repair",
                    args: vec![2.0, 30.0],
                },
                CardLine::Value {
                    key: "reskilled.card.last_stand",
                    args: vec![10.0, 300.0],
                },
                CardLine::Value {
                    key: "reskilled.card.home_ward",
                    args: vec![32.0],
                },
                CardLine::Value {
                    key: "reskilled.card.xp_gain",
                    args: vec![107.0],
                },
            ]
        );
    }

    #[test]
    fn a_perk_with_nothing_to_show_gets_its_description() {
        // Given Salvage, whose only effect has no line, and a perk with no effects at all
        let salvage = perk(include_str!(
            "../../../content/data/reskilled/reskilled/perks/building_salvage.json"
        ));
        let empty = perk_with("[]");

        // When their card lines are built
        // Then each gives just the description
        assert_eq!(card_lines(&salvage), vec![CardLine::Description]);
        assert_eq!(card_lines(&empty), vec![CardLine::Description]);
    }

    #[test]
    fn modifiers_apply_as_the_game_does() {
        // Given a value of 4 now, with a base of 4
        // When each operation adds a modifier of 0.1
        // Then add_multiplied_base adds a tenth of the base
        assert!((with_modifier(4.0, 4.0, 0.1, Operation::AddMultipliedBase) - 4.4).abs() < 1e-9);
        // And add_value adds the amount
        assert_eq!(with_modifier(4.0, 20.0, 20.0, Operation::AddValue), 24.0);
        // And add_multiplied_total multiplies the total by 1 + amount
        assert_eq!(
            with_modifier(10.0, 4.0, 0.5, Operation::AddMultipliedTotal),
            15.0
        );
    }
}
