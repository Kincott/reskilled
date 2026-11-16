//! What a player's active perks do, and the `/attribute` commands that bring the game in line with them.

use serde::{Deserialize, Serialize};

use crate::defs::{DamageKind, Direction, Effect, Matcher, Operation, ReplantCrop, Reward};
use crate::progress::{PlayerState, active_perks, active_rewards};
use crate::registry::Defs;

/// The modifier that stops a player digging while they hold a tool they may not break with.
pub const DIG_LOCK: &str = "reskilled:dig_lock";
const DIG_LOCK_ATTRIBUTE: &str = "minecraft:block_break_speed";
/// With `add_multiplied_total`, -1 takes the whole speed away.
const DIG_LOCK_AMOUNT: f64 = -1.0;
/// Namespace of every modifier this mod adds.
const MODIFIER_NAMESPACE: &str = "reskilled";
const NO_CHANGE: f64 = 1.0;

/// One attribute modifier, identified by its id. Serializable so the server can remember what it applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttributeMod {
    pub id: String,
    pub attribute: String,
    pub amount: f64,
    pub operation: Operation,
}

/// Everything the active perks do, gathered by kind.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveEffects {
    pub attributes: Vec<AttributeMod>,
    pub mount_attributes: Vec<AttributeMod>,
    /// Product of every XP gain multiplier; 1.0 when none.
    pub xp_gain: f64,
    /// Product of every anvil cost multiplier; 1.0 when none.
    pub anvil_cost: f64,
    pub damage: Vec<(Direction, DamageKind, f64)>,
    pub drops: Vec<(Matcher, f64)>,
    /// What the active focus areas give XP for.
    pub rewards: Vec<Reward>,
    /// The readiest last stand: the lowest health it needs and the shortest rest.
    pub last_stand: Option<(f32, u32)>,
    /// Potion effects for kills: effect, seconds, amplifier, which mobs.
    pub on_kill: Vec<(String, u32, u8, Matcher)>,
    /// The widest crop growth: radius, interval in seconds, chance.
    /// Radius, interval in seconds, chance, and the blocks it grows.
    pub crop_growth: Option<(u32, u32, f64, Matcher)>,
    /// Crops broken ripe are planted again.
    pub replant: Vec<ReplantCrop>,
    /// The biggest enchanted weapon bonus: per enchantment, most.
    pub enchanted_weapon: Option<(f64, f64)>,
    /// The biggest enchanted armor ward: per enchantment, most.
    pub enchanted_armor: Option<(f64, f64)>,
    /// Potion effects that wear off sooner, each with the shortest multiplier.
    pub shorter_effects: Vec<(String, f64)>,
    /// The quickest rune repair: interval in seconds, points.
    pub rune_repair: Option<(u32, u32)>,
    pub salvage: bool,
    /// Effects a full sleep clears; empty when sleep does nothing.
    pub restful_sleep: Vec<String>,
    /// Effects near home: radius, effect, amplifier.
    pub near_home: Vec<(u32, String, u8)>,
    pub home_ward: Option<u32>,
    pub safe_haven: Option<u32>,
}

impl Default for ActiveEffects {
    fn default() -> Self {
        Self {
            attributes: Vec::new(),
            mount_attributes: Vec::new(),
            xp_gain: NO_CHANGE,
            anvil_cost: NO_CHANGE,
            damage: Vec::new(),
            drops: Vec::new(),
            rewards: Vec::new(),
            last_stand: None,
            on_kill: Vec::new(),
            crop_growth: None,
            replant: Vec::new(),
            enchanted_weapon: None,
            enchanted_armor: None,
            shorter_effects: Vec::new(),
            rune_repair: None,
            salvage: false,
            restful_sleep: Vec::new(),
            near_home: Vec::new(),
            home_ward: None,
            safe_haven: None,
        }
    }
}

/// The bigger of two (per enchantment, most) pairs, by their most.
fn bigger(now: Option<(f64, f64)>, new: (f64, f64)) -> Option<(f64, f64)> {
    match now {
        Some(old) if old.1 >= new.1 => Some(old),
        _ => Some(new),
    }
}

fn wider(now: Option<u32>, new: u32) -> Option<u32> {
    Some(now.map_or(new, |old| old.max(new)))
}

/// Gathers the effects of every active perk. Modifier ids are `reskilled:<perk namespace>/<perk path>/<effect
/// index>`, so they stay stable and two namespaces' perks never share one.
pub fn active_effects(defs: &Defs, state: &PlayerState) -> ActiveEffects {
    let mut effects = ActiveEffects {
        rewards: active_rewards(defs, state).into_iter().cloned().collect(),
        ..ActiveEffects::default()
    };
    for perk in active_perks(defs, state) {
        let base = perk.id.replacen(':', "/", 1);
        for (index, effect) in perk.effects.iter().enumerate() {
            let id = || format!("{MODIFIER_NAMESPACE}:{base}/{index}");
            match effect {
                Effect::Attribute {
                    attribute,
                    amount,
                    operation,
                } => effects.attributes.push(AttributeMod {
                    id: id(),
                    attribute: attribute.clone(),
                    amount: *amount,
                    operation: *operation,
                }),
                Effect::MountAttribute {
                    attribute,
                    amount,
                    operation,
                } => effects.mount_attributes.push(AttributeMod {
                    id: id(),
                    attribute: attribute.clone(),
                    amount: *amount,
                    operation: *operation,
                }),
                Effect::DropBonus { target, chance } => {
                    effects.drops.push((target.clone(), *chance))
                }
                Effect::Damage {
                    direction,
                    kind,
                    multiplier,
                } => effects.damage.push((*direction, *kind, *multiplier)),
                Effect::XpGain { multiplier } => effects.xp_gain *= multiplier,
                Effect::AnvilCost { multiplier } => effects.anvil_cost *= multiplier,
                Effect::LastStand {
                    above,
                    cooldown_seconds,
                } => {
                    let (above, rest) = match effects.last_stand {
                        Some((a, r)) => (a.min(*above), r.min(*cooldown_seconds)),
                        None => (*above, *cooldown_seconds),
                    };
                    effects.last_stand = Some((above, rest));
                }
                Effect::EffectOnKill {
                    effect,
                    seconds,
                    amplifier,
                    mobs,
                } => effects
                    .on_kill
                    .push((effect.clone(), *seconds, *amplifier, mobs.clone())),
                Effect::CropGrowth {
                    radius,
                    interval_seconds,
                    chance,
                    blocks,
                } => {
                    let wider = effects
                        .crop_growth
                        .as_ref()
                        .is_none_or(|(r, ..)| *radius > *r);
                    if wider {
                        effects.crop_growth =
                            Some((*radius, *interval_seconds, *chance, blocks.clone()));
                    }
                }
                Effect::Replant { crops } => {
                    for crop in crops {
                        if !effects.replant.contains(crop) {
                            effects.replant.push(crop.clone());
                        }
                    }
                }
                Effect::EnchantedWeapon {
                    per_enchantment,
                    max,
                } => {
                    effects.enchanted_weapon =
                        bigger(effects.enchanted_weapon, (*per_enchantment, *max))
                }
                Effect::EnchantedArmor {
                    per_enchantment,
                    max,
                } => {
                    effects.enchanted_armor =
                        bigger(effects.enchanted_armor, (*per_enchantment, *max))
                }
                Effect::ShorterEffects {
                    effects: ids,
                    multiplier,
                } => {
                    for id in ids {
                        match effects.shorter_effects.iter_mut().find(|(e, _)| e == id) {
                            Some((_, m)) => *m = m.min(*multiplier),
                            None => effects.shorter_effects.push((id.clone(), *multiplier)),
                        }
                    }
                }
                Effect::RuneRepair {
                    interval_seconds,
                    points,
                } => {
                    let quicker = effects
                        .rune_repair
                        .is_none_or(|(i, p)| (*points, *interval_seconds) > (p, i));
                    if quicker {
                        effects.rune_repair = Some((*interval_seconds, *points));
                    }
                }
                Effect::Salvage => effects.salvage = true,
                Effect::RestfulSleep { clears } => {
                    for id in clears {
                        if !effects.restful_sleep.contains(id) {
                            effects.restful_sleep.push(id.clone());
                        }
                    }
                }
                Effect::NearHome {
                    radius,
                    effect,
                    amplifier,
                } => effects
                    .near_home
                    .push((*radius, effect.clone(), *amplifier)),
                Effect::HomeWard { radius } => {
                    effects.home_ward = wider(effects.home_ward, *radius)
                }
                Effect::SafeHaven { radius } => {
                    effects.safe_haven = wider(effects.safe_haven, *radius)
                }
            }
        }
    }
    effects
}

/// The product of the damage rules for this direction whose kind is one of the hit's kinds (a fireball is both fire
/// and projectile); rules for `Any` apply to every hit.
pub fn damage_multiplier(
    effects: &ActiveEffects,
    direction: Direction,
    kinds: &[DamageKind],
) -> f64 {
    effects
        .damage
        .iter()
        .filter(|(d, k, _)| *d == direction && (*k == DamageKind::Any || kinds.contains(k)))
        .map(|(_, _, multiplier)| multiplier)
        .product()
}

/// The modifier [`DIG_LOCK`] names.
pub fn dig_lock() -> AttributeMod {
    AttributeMod {
        id: DIG_LOCK.to_string(),
        attribute: DIG_LOCK_ATTRIBUTE.to_string(),
        amount: DIG_LOCK_AMOUNT,
        operation: Operation::AddMultipliedTotal,
    }
}

/// Vanilla `/attribute` commands (without the slash) turning `before` into `after` on `target`: removals first,
/// then additions, each sorted by id. A modifier whose values changed is removed and added again, since vanilla
/// refuses to add an id the entity already has.
pub fn attribute_commands(
    target: &str,
    before: &[AttributeMod],
    after: &[AttributeMod],
) -> Vec<String> {
    let mut removed: Vec<&AttributeMod> = before.iter().filter(|m| !after.contains(m)).collect();
    let mut added: Vec<&AttributeMod> = after.iter().filter(|m| !before.contains(m)).collect();
    removed.sort_by(|a, b| a.id.cmp(&b.id));
    added.sort_by(|a, b| a.id.cmp(&b.id));
    let removals = removed.into_iter().map(|m| {
        format!(
            "attribute {target} {} modifier remove {}",
            m.attribute, m.id
        )
    });
    let additions = added.into_iter().map(|m| {
        format!(
            "attribute {target} {} modifier add {} {} {}",
            m.attribute,
            m.id,
            m.amount,
            operation_name(m.operation)
        )
    });
    removals.chain(additions).collect()
}

/// The operation as `/attribute` spells it.
fn operation_name(operation: Operation) -> &'static str {
    match operation {
        Operation::AddValue => "add_value",
        Operation::AddMultipliedBase => "add_multiplied_base",
        Operation::AddMultipliedTotal => "add_multiplied_total",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::{Effect, PerkDef, SkillDef};
    use crate::progress::{SkillProgress, on_death};

    const MINING: &str = "reskilled:mining";
    const PLAYER: &str = "Steve";

    fn perk(id: &str, min_level: u32, effects: Vec<Effect>) -> PerkDef {
        PerkDef {
            id: id.into(),
            priority: 0,
            skill: MINING.into(),
            name: "n".into(),
            description: "d".into(),
            cost: 1,
            min_level,
            requires: vec![],
            also_needs: Default::default(),
            icon: None,
            effects,
        }
    }

    /// Mining and the given perks; the player owns every perk, at the given Mining level.
    fn owning(perks: Vec<PerkDef>, mining_level: u32) -> (Defs, PlayerState) {
        let mining = SkillDef {
            id: MINING.into(),
            priority: 0,
            name: "n".into(),
            icon: "minecraft:stone".into(),
            max_level: None,
            cost: None,
            points_every: None,
            icon_levels: Default::default(),
            earned_by: Vec::new(),
            color: None,
        };
        let mut state = PlayerState::default();
        let owned = perks.iter().map(|p| p.id.clone()).collect();
        state.skills.insert(
            MINING.into(),
            SkillProgress {
                level: mining_level,
                perks: owned,
                ..SkillProgress::default()
            },
        );
        let defs = Defs {
            skills: [(MINING.into(), mining)].into(),
            perks: perks.into_iter().map(|p| (p.id.clone(), p)).collect(),
            ..Defs::default()
        };
        (defs, state)
    }

    fn modifier(id: &str, attribute: &str, amount: f64, operation: Operation) -> AttributeMod {
        AttributeMod {
            id: id.into(),
            attribute: attribute.into(),
            amount,
            operation,
        }
    }

    #[test]
    fn rune_and_home_effects_are_gathered() {
        // Given perks with a weapon bonus, an armor ward, a shortening, rune repair and every home effect
        let perks = vec![perk(
            "reskilled:a",
            1,
            vec![
                Effect::EnchantedWeapon {
                    per_enchantment: 0.03,
                    max: 0.15,
                },
                Effect::EnchantedArmor {
                    per_enchantment: 0.01,
                    max: 0.08,
                },
                Effect::ShorterEffects {
                    effects: vec!["minecraft:poison".into()],
                    multiplier: 0.6,
                },
                Effect::RuneRepair {
                    interval_seconds: 30,
                    points: 10,
                },
                Effect::Salvage,
                Effect::RestfulSleep {
                    clears: vec!["minecraft:poison".into()],
                },
                Effect::NearHome {
                    radius: 16,
                    effect: "minecraft:regeneration".into(),
                    amplifier: 0,
                },
                Effect::HomeWard { radius: 24 },
                Effect::SafeHaven { radius: 32 },
            ],
        )];
        let (defs, state) = owning(perks, 1);

        // When the active effects are gathered
        let e = active_effects(&defs, &state);

        // Then each is there
        assert_eq!(e.enchanted_weapon, Some((0.03, 0.15)));
        assert_eq!(e.enchanted_armor, Some((0.01, 0.08)));
        assert_eq!(
            e.shorter_effects,
            vec![("minecraft:poison".to_string(), 0.6)]
        );
        assert_eq!(e.rune_repair, Some((30, 10)));
        assert!(e.salvage);
        assert_eq!(e.restful_sleep, vec!["minecraft:poison".to_string()]);
        assert_eq!(
            e.near_home,
            vec![(16, "minecraft:regeneration".to_string(), 0)]
        );
        assert_eq!((e.home_ward, e.safe_haven), (Some(24), Some(32)));
    }

    #[test]
    fn mechanic_effects_are_gathered() {
        // Given two perks with last stands (one readier), a kill effect, crop growth and replanting
        let crop = crate::defs::ReplantCrop {
            block: "minecraft:wheat".into(),
            seed: "minecraft:wheat_seeds".into(),
            ripe_age: 7,
        };
        let perks = vec![
            perk(
                "reskilled:a",
                1,
                vec![Effect::LastStand {
                    above: 6.0,
                    cooldown_seconds: 300,
                }],
            ),
            perk(
                "reskilled:b",
                1,
                vec![
                    Effect::LastStand {
                        above: 4.0,
                        cooldown_seconds: 120,
                    },
                    Effect::EffectOnKill {
                        effect: "minecraft:strength".into(),
                        seconds: 5,
                        amplifier: 0,
                        mobs: crate::defs::Matcher::Id("minecraft:zombie".into()),
                    },
                    Effect::CropGrowth {
                        radius: 5,
                        interval_seconds: 10,
                        chance: 0.25,
                        blocks: Matcher::Tag("#minecraft:crops".into()),
                    },
                    Effect::Replant {
                        crops: vec![crop.clone()],
                    },
                ],
            ),
        ];
        let (defs, state) = owning(perks, 1);

        // When the active effects are gathered
        let effects = active_effects(&defs, &state);

        // Then the readier last stand is kept, and the others are all there
        assert_eq!(effects.last_stand, Some((4.0, 120)));
        assert_eq!(effects.on_kill.len(), 1);
        assert_eq!(
            effects.crop_growth,
            Some((5, 10, 0.25, Matcher::Tag("#minecraft:crops".into())))
        );
        assert_eq!(effects.replant, vec![crop]);
    }

    #[test]
    fn effects_are_collected_by_type() {
        // Given one perk with an attribute, a mount attribute and a drop bonus, from another namespace
        let (defs, state) = owning(
            vec![perk(
                "pack:deep_roots",
                1,
                vec![
                    Effect::Attribute {
                        attribute: "minecraft:block_break_speed".into(),
                        amount: 0.1,
                        operation: Operation::AddMultipliedBase,
                    },
                    Effect::MountAttribute {
                        attribute: "minecraft:movement_speed".into(),
                        amount: 0.05,
                        operation: Operation::AddValue,
                    },
                    Effect::DropBonus {
                        target: Matcher::Tag("#minecraft:coal_ores".into()),
                        chance: 0.25,
                    },
                ],
            )],
            1,
        );

        // When the active effects are read
        let effects = active_effects(&defs, &state);

        // Then each lands in its list, and modifier ids carry the perk's namespace, path and effect index
        assert_eq!(
            effects.attributes,
            vec![modifier(
                "reskilled:pack/deep_roots/0",
                "minecraft:block_break_speed",
                0.1,
                Operation::AddMultipliedBase
            )]
        );
        assert_eq!(
            effects.mount_attributes,
            vec![modifier(
                "reskilled:pack/deep_roots/1",
                "minecraft:movement_speed",
                0.05,
                Operation::AddValue
            )]
        );
        assert_eq!(
            effects.drops,
            vec![(Matcher::Tag("#minecraft:coal_ores".into()), 0.25)]
        );
    }

    #[test]
    fn multipliers_multiply() {
        // Given two perks raising XP gain by 10% and 20% and lowering anvil costs to 90% and 50%
        let (defs, state) = owning(
            vec![
                perk(
                    "reskilled:a",
                    1,
                    vec![
                        Effect::XpGain { multiplier: 1.1 },
                        Effect::AnvilCost { multiplier: 0.9 },
                    ],
                ),
                perk(
                    "reskilled:b",
                    1,
                    vec![
                        Effect::XpGain { multiplier: 1.2 },
                        Effect::AnvilCost { multiplier: 0.5 },
                    ],
                ),
            ],
            1,
        );

        // When the active effects are read, with the perks and with none
        let effects = active_effects(&defs, &state);
        let none = active_effects(&defs, &PlayerState::default());

        // Then the multipliers are the products, and 1.0 with no perk
        assert!((effects.xp_gain - 1.32).abs() < 1e-9, "{}", effects.xp_gain);
        assert!(
            (effects.anvil_cost - 0.45).abs() < 1e-9,
            "{}",
            effects.anvil_cost
        );
        assert_eq!((none.xp_gain, none.anvil_cost), (1.0, 1.0));
    }

    #[test]
    fn any_damage_rule_applies_to_every_kind() {
        // Given +50% melee damage dealt, +20% of any damage dealt, and half fall damage taken
        let (defs, state) = owning(
            vec![perk(
                "reskilled:fighter",
                1,
                vec![
                    Effect::Damage {
                        direction: Direction::Dealt,
                        kind: DamageKind::Melee,
                        multiplier: 1.5,
                    },
                    Effect::Damage {
                        direction: Direction::Dealt,
                        kind: DamageKind::Any,
                        multiplier: 1.2,
                    },
                    Effect::Damage {
                        direction: Direction::Taken,
                        kind: DamageKind::Fall,
                        multiplier: 0.5,
                    },
                ],
            )],
            1,
        );
        let effects = active_effects(&defs, &state);

        // When multipliers are read per direction and kinds, a fireball being both fire and projectile, and a hit
        // of no kind the rules name
        let dealt_melee = damage_multiplier(&effects, Direction::Dealt, &[DamageKind::Melee]);
        let dealt_projectile =
            damage_multiplier(&effects, Direction::Dealt, &[DamageKind::Projectile]);
        let taken_fall = damage_multiplier(&effects, Direction::Taken, &[DamageKind::Fall]);
        let taken_melee = damage_multiplier(&effects, Direction::Taken, &[DamageKind::Melee]);
        let dealt_both = damage_multiplier(
            &effects,
            Direction::Dealt,
            &[DamageKind::Fire, DamageKind::Melee],
        );
        let dealt_none = damage_multiplier(&effects, Direction::Dealt, &[]);

        // Then `any` rules join every kind in their direction, a rule counts once when its kind is listed, and
        // nothing else changes
        assert!((dealt_melee - 1.8).abs() < 1e-9, "{dealt_melee}");
        assert!((dealt_projectile - 1.2).abs() < 1e-9, "{dealt_projectile}");
        assert_eq!((taken_fall, taken_melee), (0.5, 1.0));
        assert!((dealt_both - 1.8).abs() < 1e-9, "{dealt_both}");
        assert!((dealt_none - 1.2).abs() < 1e-9, "{dealt_none}");
    }

    #[test]
    fn attribute_commands_remove_then_add_sorted() {
        // Given modifiers b (changed), c (gone) and d (kept) before, and a (new), b (changed) and d after
        let before = vec![
            modifier("reskilled:c", "minecraft:luck", 1.0, Operation::AddValue),
            modifier("reskilled:b", "minecraft:armor", 2.0, Operation::AddValue),
            modifier("reskilled:d", "minecraft:armor", 1.0, Operation::AddValue),
        ];
        let after = vec![
            modifier("reskilled:d", "minecraft:armor", 1.0, Operation::AddValue),
            modifier("reskilled:b", "minecraft:armor", 2.5, Operation::AddValue),
            modifier(
                "reskilled:a",
                "minecraft:attack_speed",
                0.15,
                Operation::AddMultipliedTotal,
            ),
        ];

        // When the commands are worked out
        let commands = attribute_commands(PLAYER, &before, &after);

        // Then removals come first, then additions, each sorted by id, and the unchanged one is left alone
        assert_eq!(
            commands,
            vec![
                "attribute Steve minecraft:armor modifier remove reskilled:b",
                "attribute Steve minecraft:luck modifier remove reskilled:c",
                "attribute Steve minecraft:attack_speed modifier add reskilled:a 0.15 add_multiplied_total",
                "attribute Steve minecraft:armor modifier add reskilled:b 2.5 add_value",
            ]
        );
    }

    #[test]
    fn no_change_gives_no_commands() {
        // Given the same modifiers before and after, in another order
        let one = modifier("reskilled:a", "minecraft:luck", 1.0, Operation::AddValue);
        let two = modifier("reskilled:b", "minecraft:armor", 2.0, Operation::AddValue);

        // When the commands are worked out
        let commands = attribute_commands(PLAYER, &[one.clone(), two.clone()], &[two, one]);

        // Then there are none
        assert_eq!(commands, Vec::<String>::new());
    }

    #[test]
    fn inactive_perk_yields_remove_command() {
        // Given a player at Mining 9 owning a level-8 armor perk, and a 2-level death penalty
        let (mut defs, state) = owning(
            vec![perk(
                "reskilled:mining_hard_hat",
                8,
                vec![Effect::Attribute {
                    attribute: "minecraft:armor".into(),
                    amount: 2.0,
                    operation: Operation::AddValue,
                }],
            )],
            9,
        );
        defs.settings.death_penalty_levels = 2;
        let before = active_effects(&defs, &state).attributes;

        // When they die (Mining 7) and the attributes are brought in line
        let after = active_effects(&defs, &on_death(&defs, &state)).attributes;
        let commands = attribute_commands(PLAYER, &before, &after);

        // Then the perk's modifier is removed
        assert_eq!(
            commands,
            vec![
                "attribute Steve minecraft:armor modifier remove reskilled:reskilled/mining_hard_hat/0"
            ]
        );
    }

    #[test]
    fn dig_lock_constants_match_spec() {
        // Given the dig lock modifier
        let lock = dig_lock();

        // When it is added to a player
        let commands = attribute_commands(PLAYER, &[], std::slice::from_ref(&lock));

        // Then it takes 100% off the block break speed under its own id
        assert_eq!(DIG_LOCK, "reskilled:dig_lock");
        assert_eq!(
            lock,
            modifier(
                DIG_LOCK,
                "minecraft:block_break_speed",
                -1.0,
                Operation::AddMultipliedTotal
            )
        );
        assert_eq!(
            commands,
            vec![
                "attribute Steve minecraft:block_break_speed modifier add reskilled:dig_lock -1 add_multiplied_total"
            ]
        );
    }
}
