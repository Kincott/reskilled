//! The data files a pack maker writes: skills, perks, focus areas, locks and settings, one JSON object per file.
//!
//! Every file carries `"format": 1`. [`parse_file`] reads one file and checks everything that can be checked
//! without the other files (ids, ranges, which actions fit a target); references between files are the
//! registry's job.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The data file format this version reads.
pub const FORMAT: u64 = 1;
const FORMAT_FIELD: &str = "format";

const DEFAULT_MAX_LEVEL: u32 = 32;
const DEFAULT_COST: CostCurve = CostCurve { base: 1, every: 1 };
const DEFAULT_POINTS_EVERY: u32 = 2;
const DEFAULT_FOCUS_COST: u32 = 10;
const DEFAULT_LOCK_MESSAGE_COOLDOWN_TICKS: u32 = 20;

/// A skill players raise with XP levels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillDef {
    pub id: String,
    #[serde(default)]
    pub priority: i32,
    /// Translation key.
    pub name: String,
    /// Item id drawn as the skill's icon.
    pub icon: String,
    /// Better icons as the skill grows: from each level on, its item replaces the icon.
    #[serde(default)]
    pub icon_levels: BTreeMap<u32, String>,
    /// Overrides the settings' `max_level` for this skill.
    pub max_level: Option<u32>,
    /// Overrides the settings' `cost` for this skill.
    pub cost: Option<CostCurve>,
    /// Overrides the settings' `points_every` for this skill.
    pub points_every: Option<u32>,
    /// The work that grows the skill when the server's rules let skills grow by doing.
    #[serde(default)]
    pub earned_by: Vec<EarnRule>,
    /// The skill's colour on the screens, `#RRGGBB`; without one, [`skill_rgb`] picks one from the id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// The colours a skill without its own takes, chosen by its id.
pub const FALLBACK_RGB: [u32; 8] = [
    0x8FA6B8, 0xD9534A, 0x6E8FD9, 0xB07CF2, 0x7FC75A, 0x5AD1C7, 0xD98C4A, 0xD9C24A,
];

/// The skill's colour as `0xRRGGBB`: its own, else a fallback chosen by a stable hash of its id.
pub fn skill_rgb(skill: &SkillDef) -> u32 {
    if let Some(rgb) = skill.color.as_deref().and_then(parse_rgb) {
        return rgb;
    }
    // FNV-1a, so the choice stays the same across runs and versions.
    let hash = skill.id.bytes().fold(0x811C_9DC5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    FALLBACK_RGB[hash as usize % FALLBACK_RGB.len()]
}

/// `#RRGGBB` as `0xRRGGBB`.
fn parse_rgb(colour: &str) -> Option<u32> {
    let hex = colour.strip_prefix('#')?;
    (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(hex, 16).ok())
        .flatten()
}

fn crops() -> Matcher {
    Matcher::Tag("#minecraft:crops".into())
}

fn one_action() -> f64 {
    1.0
}

/// A kind of work that grows a skill, and what it is worth in actions (a level L takes ten actions times L at the
/// rules' usual pace).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EarnRule {
    /// Breaking a block with an item in the main hand that `with` picks.
    Break {
        with: Matcher,
        #[serde(default = "one_action")]
        worth: f64,
    },
    /// Placing a block, once for each spot.
    Place {
        #[serde(default = "one_action")]
        worth: f64,
    },
    /// A blow on a creature (anything the settings' `not_creatures` does not pick).
    Attack {
        #[serde(default = "one_action")]
        worth: f64,
    },
    /// A projectile the player shot hitting something (a damage type the settings' `projectile_damage` picks).
    ProjectileHit {
        #[serde(default = "one_action")]
        worth: f64,
    },
    /// A hit the player takes from something.
    Hurt {
        #[serde(default = "one_action")]
        worth: f64,
    },
    /// Each block travelled is worth `per_block` actions.
    Travel { per_block: f64 },
    /// Enchanting an item.
    Enchant {
        #[serde(default = "one_action")]
        worth: f64,
    },
}

impl EarnRule {
    fn validate(&self) -> Result<(), String> {
        let (field, worth) = match self {
            EarnRule::Break { with, worth } => {
                with.validate(TargetKind::Item)?;
                ("worth", *worth)
            }
            EarnRule::Travel { per_block } => ("per_block", *per_block),
            EarnRule::Place { worth }
            | EarnRule::Attack { worth }
            | EarnRule::ProjectileHit { worth }
            | EarnRule::Hurt { worth }
            | EarnRule::Enchant { worth } => ("worth", *worth),
        };
        if worth.is_finite() && worth >= 0.0 {
            Ok(())
        } else {
            Err(format!(
                "{field} must be a number of at least 0, found {worth}"
            ))
        }
    }
}

/// Raising a skill from level L to L + 1 costs `base + floor((L - 1) / every)` XP levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostCurve {
    pub base: u32,
    pub every: u32,
}

/// A perk bought with a skill's perk points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerkDef {
    pub id: String,
    #[serde(default)]
    pub priority: i32,
    /// The skill whose points pay for it.
    pub skill: String,
    /// Translation key.
    pub name: String,
    /// Translation key.
    pub description: String,
    /// Perk points it costs.
    pub cost: u32,
    /// Skill level needed to buy it, and to keep it working.
    pub min_level: u32,
    /// Perks that must be bought first.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Levels in other skills needed as well, to buy it and to keep it working.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub also_needs: BTreeMap<String, u32>,
    pub effects: Vec<Effect>,
    /// The item drawn for the perk on the screens; without one, its skill's icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// What a perk does while it is active.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Effect {
    /// An attribute modifier on the player.
    Attribute {
        attribute: String,
        amount: f64,
        operation: Operation,
    },
    /// A chance to drop one more stack when breaking a matching block.
    DropBonus { target: Matcher, chance: f64 },
    /// Multiplies damage the player deals or takes, by kind.
    Damage {
        direction: Direction,
        kind: DamageKind,
        multiplier: f64,
    },
    /// Multiplies XP points the player picks up.
    XpGain { multiplier: f64 },
    /// Multiplies the anvil's level cost.
    AnvilCost { multiplier: f64 },
    /// An attribute modifier on the mount the player rides.
    MountAttribute {
        attribute: String,
        amount: f64,
        operation: Operation,
    },
    /// A blow that would kill the player, taken while their health was at least `above`, leaves them at half a
    /// heart instead; then it rests `cooldown_seconds`.
    LastStand { above: f32, cooldown_seconds: u32 },
    /// Killing one of `mobs` gives the player the potion effect `effect` (`minecraft:strength`) for `seconds`.
    EffectOnKill {
        effect: String,
        seconds: u32,
        amplifier: u8,
        mobs: Matcher,
    },
    /// Every `interval_seconds`, each of `blocks` (by default `#minecraft:crops`) within `radius` blocks grows a
    /// stage with `chance`; a block grows by its `age` property.
    CropGrowth {
        radius: u32,
        interval_seconds: u32,
        chance: f64,
        #[serde(default = "crops")]
        blocks: Matcher,
    },
    /// Breaking one of these crops ripe plants it again, taking one seed from what it drops.
    Replant { crops: Vec<ReplantCrop> },
    /// Melee hits with an enchanted weapon deal `per_enchantment` more for each enchantment on it, up to `max`.
    EnchantedWeapon { per_enchantment: f64, max: f64 },
    /// Each enchantment on the worn armor cuts damage taken by `per_enchantment`, up to `max`.
    EnchantedArmor { per_enchantment: f64, max: f64 },
    /// These potion effects, when they come on the player, last `multiplier` of their time.
    ShorterEffects {
        effects: Vec<String>,
        multiplier: f64,
    },
    /// Every `interval_seconds`, each enchanted item worn or held loses `points` of damage.
    RuneRepair { interval_seconds: u32, points: u32 },
    /// Blocks the player placed drop themselves when they break them.
    Salvage,
    /// In bed, these effects lift (so their damage cannot wake the player); a full night's sleep ends them and
    /// heals the player, and leaving the bed early brings them back as they were.
    RestfulSleep { clears: Vec<String> },
    /// Within `radius` blocks of their respawn point the player has `effect`.
    NearHome {
        radius: u32,
        effect: String,
        amplifier: u8,
    },
    /// Explosions within `radius` blocks of the player's respawn point break no blocks, while they are online.
    HomeWard { radius: u32 },
    /// Monsters do not spawn naturally within `radius` blocks of the player's respawn point, while they are
    /// online.
    SafeHaven { radius: u32 },
}

/// The widest radius of an effect around the player's home, in blocks.
const HOME_RADIUS_MAX: u32 = 64;

/// A crop [`Effect::Replant`] plants again: its block, the seed it takes from the drops, and the `age` it is ripe at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplantCrop {
    pub block: String,
    pub seed: String,
    pub ripe_age: u32,
}

/// The widest [`Effect::CropGrowth`] radius, in blocks.
const CROP_RADIUS_MAX: u32 = 16;

/// An attribute modifier's operation, named as vanilla's `/attribute` command names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    AddValue,
    AddMultipliedBase,
    AddMultipliedTotal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Dealt,
    Taken,
}

/// The kinds of damage a perk can scale. `Any` applies to every kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageKind {
    Melee,
    Projectile,
    Fall,
    Fire,
    Explosion,
    /// The fall at the end of an ender pearl's flight.
    EnderPearl,
    /// Magic: harming potions, witches, evoker fangs, guardian beams.
    Magic,
    Any,
}

/// A group of skills and the tasks that go with them. A player may have several areas active; each gives more XP
/// for its tasks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FocusDef {
    pub id: String,
    #[serde(default)]
    pub priority: i32,
    /// Translation key.
    pub name: String,
    pub skills: Vec<String>,
    /// The XP the area gives for its tasks, on top of what the game gives.
    #[serde(default)]
    pub rewards: Vec<Reward>,
}

/// XP an active focus area gives for a task. Points are vanilla's experience points; fractions add up across
/// tasks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "task", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reward {
    /// This percent more of the XP a mob drops when the player kills it.
    Kill { bonus_percent: u32 },
    /// This percent more of the XP a block drops when the player breaks it (ores, sculk).
    BlockXp { bonus_percent: u32 },
    /// Points for each matching block the player breaks that no player placed; with `state`, only when the
    /// block's properties have these values (a fully grown crop), which a placed block never has.
    Break {
        blocks: Matcher,
        points: f64,
        #[serde(default)]
        state: BTreeMap<String, String>,
        /// Translation key naming the task in the focus tooltip ("Chopping logs").
        label: Option<String>,
    },
    /// Points for each item the player takes out of a furnace.
    Smelt { points: f64 },
    /// Points for each block the player travels, on foot, swimming, gliding or riding.
    Travel { points: f64 },
    /// Points for each animal the player tames.
    Tame { points: f64 },
    /// Points for each pair of animals the player breeds.
    Breed { points: f64 },
}

/// A file of lock entries. Lock files add up instead of overriding each other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockFile {
    pub entries: Vec<LockEntry>,
}

/// Skill levels needed for some actions on a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockEntry {
    pub target: Target,
    pub actions: Vec<Action>,
    /// Skill id to the level needed. Empty with `replace` removes earlier locks on the target.
    pub requires: BTreeMap<String, u32>,
    /// Translation key shown in the unlock preview.
    pub label: Option<String>,
    /// Drops earlier entries for an equal target.
    #[serde(default)]
    pub replace: bool,
    /// For an enchantment target: the enchantment level the lock starts at (1 when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enchantment_level: Option<u32>,
    /// The item drawn for the lock on its skill's road, with its label; without one it is not on the road.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// The three kinds of thing a lock can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetKind {
    Item,
    Block,
    Entity,
    /// An enchantment on the item in use: it adds its locks to the item's own.
    Enchantment,
    /// Something another mod lets players do with no item, block or entity behind it (a dash), by that mod's id.
    Ability,
}

impl fmt::Display for TargetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TargetKind::Item => "item",
            TargetKind::Block => "block",
            TargetKind::Entity => "entity",
            TargetKind::Enchantment => "enchantment",
            TargetKind::Ability => "ability",
        })
    }
}

/// What a lock applies to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Item(Matcher),
    Block(Matcher),
    Entity(Matcher),
    Enchantment(Matcher),
    Ability(Matcher),
}

/// Picks item, block or entity ids. Tags and repair materials are written with a leading `#` when they name a tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Matcher {
    Id(String),
    Tag(String),
    /// An id where `*` stands for any run of characters.
    Pattern(String),
    /// Items whose `minecraft:repairable` component names this tag or item.
    Repairable(String),
    AllOf(Vec<Matcher>),
    AnyOf(Vec<Matcher>),
}

/// What a player does that a lock can refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Use,
    Attack,
    Break,
    Equip,
    Open,
    Place,
    Mount,
    Interact,
}

/// A settings file: the fields it sets override lower-priority files.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsPatch {
    #[serde(default)]
    pub priority: i32,
    pub max_level: Option<u32>,
    pub cost: Option<CostCurve>,
    pub points_every: Option<u32>,
    pub focus_first_free: Option<bool>,
    pub focus_cost: Option<u32>,
    pub death_penalty_levels: Option<u32>,
    pub creative_ignores_locks: Option<bool>,
    pub lock_message_cooldown_ticks: Option<u32>,
    /// Entities a blow on grows no skill; added to every other file's.
    #[serde(default)]
    pub not_creatures: Vec<Matcher>,
    /// Damage types that are a projectile's hit; added to every other file's.
    #[serde(default)]
    pub projectile_damage: Vec<Matcher>,
    /// Items a shift-click puts on, and where; added to every other file's.
    #[serde(default)]
    pub equip_slots: Vec<EquipSlotRule>,
}

/// Items a shift-click in the inventory puts into a worn slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipSlotRule {
    pub items: Matcher,
    pub slot: WornSlot,
}

/// A slot worn rather than held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WornSlot {
    Head,
    Chest,
    Legs,
    Feet,
    Offhand,
}

/// The effective settings, after every settings file is merged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub max_level: u32,
    pub cost: CostCurve,
    /// One perk point at every multiple of this level.
    pub points_every: u32,
    /// The first focus area a player ever adds is free.
    pub focus_first_free: bool,
    /// Adding a focus area costs this many XP levels for each area that will be active.
    pub focus_cost: u32,
    /// Levels lost in each skill on death, never below level 1.
    pub death_penalty_levels: u32,
    pub creative_ignores_locks: bool,
    pub lock_message_cooldown_ticks: u32,
    /// Entities (ids and patterns) a blow on grows no skill: decorations and vehicles.
    #[serde(default)]
    pub not_creatures: Vec<Matcher>,
    /// Damage types (ids and patterns) that count as a projectile's hit.
    #[serde(default)]
    pub projectile_damage: Vec<Matcher>,
    /// The first rule picking an item says which worn slot a shift-click puts it in.
    #[serde(default)]
    pub equip_slots: Vec<EquipSlotRule>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_level: DEFAULT_MAX_LEVEL,
            cost: DEFAULT_COST,
            points_every: DEFAULT_POINTS_EVERY,
            focus_first_free: true,
            focus_cost: DEFAULT_FOCUS_COST,
            death_penalty_levels: 0,
            creative_ignores_locks: true,
            lock_message_cooldown_ticks: DEFAULT_LOCK_MESSAGE_COOLDOWN_TICKS,
            not_creatures: Vec::new(),
            projectile_damage: Vec::new(),
            equip_slots: Vec::new(),
        }
    }
}

/// Which data folder a file came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefKind {
    Skill,
    Perk,
    Focus,
    Lock,
    Settings,
}

/// One parsed data file.
#[derive(Debug, Clone, PartialEq)]
pub enum DefFile {
    Skill(SkillDef),
    Perk(PerkDef),
    Focus(FocusDef),
    Lock(LockFile),
    Settings(SettingsPatch),
    /// `"remove": true`: the skill, perk or focus area with this id is taken out, when this file outranks the
    /// others naming it.
    Removal(Removal),
}

/// A file that takes a skill, perk or focus area out: `{"format": 1, "id": "...", "priority": 1, "remove": true}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Removal {
    pub id: String,
    #[serde(default)]
    pub priority: i32,
    pub remove: bool,
}

/// A data file, or an entry in one, that was skipped.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{file}: {reason}")]
pub struct DefError {
    /// `namespace:folder/name`; empty when [`parse_file`] made the error, since it is not told the path.
    pub file: String,
    pub reason: String,
}

/// Reads one data file of the given kind and checks it on its own.
pub fn parse_file(kind: DefKind, json: &str) -> Result<DefFile, DefError> {
    let mut value: Value = serde_json::from_str(json).map_err(refused)?;
    take_format(&mut value).map_err(refused)?;
    let removes = value.get("remove") == Some(&Value::Bool(true));
    let file = match kind {
        DefKind::Skill | DefKind::Perk | DefKind::Focus if removes => {
            DefFile::Removal(serde_json::from_value(value).map_err(refused)?)
        }
        DefKind::Skill => DefFile::Skill(serde_json::from_value(value).map_err(refused)?),
        DefKind::Perk => DefFile::Perk(serde_json::from_value(value).map_err(refused)?),
        DefKind::Focus => DefFile::Focus(serde_json::from_value(value).map_err(refused)?),
        DefKind::Lock => DefFile::Lock(serde_json::from_value(value).map_err(refused)?),
        DefKind::Settings => DefFile::Settings(serde_json::from_value(value).map_err(refused)?),
    };
    file.validate().map_err(refused)?;
    Ok(file)
}

fn refused(reason: impl ToString) -> DefError {
    DefError {
        file: String::new(),
        reason: reason.to_string(),
    }
}

/// Removes the `format` field, refusing a missing or unknown one.
fn take_format(value: &mut Value) -> Result<(), String> {
    let object = value
        .as_object_mut()
        .ok_or("a data file must be a JSON object")?;
    match object.remove(FORMAT_FIELD) {
        Some(found) if found.as_u64() == Some(FORMAT) => Ok(()),
        Some(found) => Err(format!(
            "unsupported format {found} (this version reads format {FORMAT})"
        )),
        None => Err(format!("missing \"{FORMAT_FIELD}\": {FORMAT}")),
    }
}

impl DefFile {
    fn validate(&self) -> Result<(), String> {
        match self {
            DefFile::Removal(removal) => check_id("id", &removal.id),
            DefFile::Skill(skill) => skill.validate(),
            DefFile::Perk(perk) => perk.validate(),
            DefFile::Focus(focus) => focus.validate(),
            DefFile::Lock(locks) => locks.entries.iter().try_for_each(LockEntry::validate),
            DefFile::Settings(patch) => patch.validate(),
        }
    }
}

impl SkillDef {
    /// The icon for the skill at `level`: the item of the highest `icon_levels` entry reached, else `icon`.
    pub fn icon_at(&self, level: u32) -> &str {
        self.icon_levels
            .range(..=level)
            .next_back()
            .map_or(self.icon.as_str(), |(_, item)| item.as_str())
    }

    fn validate(&self) -> Result<(), String> {
        check_id("id", &self.id)?;
        check_id("icon", &self.icon)?;
        self.icon_levels
            .values()
            .try_for_each(|item| check_id("icon_levels", item))?;
        check_at_least_one("max_level", self.max_level)?;
        check_cost(self.cost)?;
        check_at_least_one("points_every", self.points_every)?;
        if let Some(colour) = &self.color
            && parse_rgb(colour).is_none()
        {
            return Err(format!("color {colour:?} is not #RRGGBB"));
        }
        self.earned_by.iter().try_for_each(EarnRule::validate)
    }
}

impl PerkDef {
    fn validate(&self) -> Result<(), String> {
        check_id("id", &self.id)?;
        check_id("skill", &self.skill)?;
        self.requires
            .iter()
            .try_for_each(|perk| check_id("requires", perk))?;
        if let Some(icon) = &self.icon {
            check_id("icon", icon)?;
        }
        self.effects.iter().try_for_each(Effect::validate)
    }
}

impl Effect {
    fn validate(&self) -> Result<(), String> {
        match self {
            Effect::Attribute {
                attribute, amount, ..
            }
            | Effect::MountAttribute {
                attribute, amount, ..
            } => {
                check_id("attribute", attribute)?;
                check_finite("amount", *amount)
            }
            Effect::DropBonus { target, chance } => {
                target.validate(TargetKind::Block)?;
                if (0.0..=1.0).contains(chance) {
                    Ok(())
                } else {
                    Err(format!("chance must be between 0 and 1, found {chance}"))
                }
            }
            Effect::LastStand { above, .. } => {
                check_finite("above", f64::from(*above))?;
                if *above >= 0.0 {
                    Ok(())
                } else {
                    Err(format!("above must not be negative, found {above}"))
                }
            }
            Effect::EffectOnKill { effect, mobs, .. } => {
                check_id("effect", effect)?;
                mobs.validate(TargetKind::Entity)
            }
            Effect::CropGrowth {
                radius,
                interval_seconds,
                chance,
                blocks,
            } => {
                blocks.validate(TargetKind::Block)?;
                if !(0.0..=1.0).contains(chance) {
                    return Err(format!("chance must be between 0 and 1, found {chance}"));
                }
                if *radius > CROP_RADIUS_MAX {
                    return Err(format!(
                        "radius must be at most {CROP_RADIUS_MAX}, found {radius}"
                    ));
                }
                if *interval_seconds == 0 {
                    return Err("interval_seconds must be at least 1".into());
                }
                Ok(())
            }
            Effect::Replant { crops } => crops.iter().try_for_each(|crop| {
                check_id("block", &crop.block)?;
                check_id("seed", &crop.seed)
            }),
            Effect::EnchantedWeapon {
                per_enchantment,
                max,
            }
            | Effect::EnchantedArmor {
                per_enchantment,
                max,
            } => {
                check_finite("per_enchantment", *per_enchantment)?;
                check_finite("max", *max)?;
                if !(0.0..=1.0).contains(per_enchantment) || !(0.0..=1.0).contains(max) {
                    return Err(format!(
                        "per_enchantment and max must be between 0 and 1, found {per_enchantment} and max {max}"
                    ));
                }
                Ok(())
            }
            Effect::ShorterEffects {
                effects,
                multiplier,
            } => {
                effects.iter().try_for_each(|e| check_id("effects", e))?;
                if (0.0..=1.0).contains(multiplier) {
                    Ok(())
                } else {
                    Err(format!(
                        "multiplier must be between 0 and 1, found {multiplier}"
                    ))
                }
            }
            Effect::RuneRepair {
                interval_seconds, ..
            } => {
                if *interval_seconds == 0 {
                    return Err("interval_seconds must be at least 1".into());
                }
                Ok(())
            }
            Effect::Salvage => Ok(()),
            Effect::RestfulSleep { clears } => {
                clears.iter().try_for_each(|e| check_id("clears", e))
            }
            Effect::NearHome { radius, effect, .. } => {
                check_id("effect", effect)?;
                check_home_radius(*radius)
            }
            Effect::HomeWard { radius } | Effect::SafeHaven { radius } => {
                check_home_radius(*radius)
            }
            Effect::Damage { multiplier, .. }
            | Effect::XpGain { multiplier }
            | Effect::AnvilCost { multiplier } => {
                check_finite("multiplier", *multiplier)?;
                if *multiplier >= 0.0 {
                    Ok(())
                } else {
                    Err(format!(
                        "multiplier must not be negative, found {multiplier}"
                    ))
                }
            }
        }
    }
}

fn check_home_radius(radius: u32) -> Result<(), String> {
    if radius <= HOME_RADIUS_MAX {
        Ok(())
    } else {
        Err(format!(
            "radius must be at most {HOME_RADIUS_MAX}, found {radius}"
        ))
    }
}

impl FocusDef {
    fn validate(&self) -> Result<(), String> {
        check_id("id", &self.id)?;
        self.skills
            .iter()
            .try_for_each(|skill| check_id("skills", skill))?;
        self.rewards.iter().try_for_each(Reward::validate)
    }
}

impl Reward {
    fn validate(&self) -> Result<(), String> {
        let points = match self {
            Reward::Kill { .. } | Reward::BlockXp { .. } => return Ok(()),
            Reward::Break { blocks, points, .. } => {
                blocks.validate(TargetKind::Block)?;
                *points
            }
            Reward::Smelt { points }
            | Reward::Travel { points }
            | Reward::Tame { points }
            | Reward::Breed { points } => *points,
        };
        check_finite("points", points)?;
        if points >= 0.0 {
            Ok(())
        } else {
            Err(format!("points must not be negative, found {points}"))
        }
    }
}

impl LockEntry {
    fn validate(&self) -> Result<(), String> {
        let (kind, matcher) = self.target.parts();
        matcher.validate(kind)?;
        if self.enchantment_level.is_some() && kind != TargetKind::Enchantment {
            return Err("enchantment_level only fits an enchantment target".into());
        }
        if self.actions.is_empty() {
            return Err("actions must not be empty".into());
        }
        if let Some(icon) = &self.icon {
            check_id("icon", icon)?;
        }
        if let Some(action) = self
            .actions
            .iter()
            .find(|action| !self.target.allows(**action))
        {
            let name = serde_json::to_string(action).map_err(|e| e.to_string())?;
            return Err(format!("action {name} does not apply to an {kind} target"));
        }
        self.requires
            .keys()
            .try_for_each(|skill| check_id("requires", skill))
    }
}

impl Target {
    pub fn parts(&self) -> (TargetKind, &Matcher) {
        match self {
            Target::Item(matcher) => (TargetKind::Item, matcher),
            Target::Block(matcher) => (TargetKind::Block, matcher),
            Target::Entity(matcher) => (TargetKind::Entity, matcher),
            Target::Enchantment(matcher) => (TargetKind::Enchantment, matcher),
            Target::Ability(matcher) => (TargetKind::Ability, matcher),
        }
    }

    /// Items are used, swung, dug with and worn; blocks are opened and placed; entities are ridden and used.
    fn allows(&self, action: Action) -> bool {
        match self {
            Target::Item(_) | Target::Enchantment(_) => matches!(
                action,
                Action::Use | Action::Attack | Action::Break | Action::Equip
            ),
            Target::Block(_) => matches!(action, Action::Open | Action::Place),
            Target::Entity(_) => matches!(action, Action::Mount | Action::Interact),
            Target::Ability(_) => action == Action::Use,
        }
    }
}

impl Matcher {
    /// Entities have no tags the server can test, and only items have a repair material.
    fn validate(&self, kind: TargetKind) -> Result<(), String> {
        match self {
            Matcher::Id(id) => check_id("id", id),
            Matcher::Tag(_)
                if matches!(
                    kind,
                    TargetKind::Entity | TargetKind::Enchantment | TargetKind::Ability
                ) =>
            {
                Err(format!("tag matchers do not work on an {kind} target"))
            }
            Matcher::Tag(tag) => check_tag("tag", tag),
            Matcher::Repairable(_) if kind != TargetKind::Item => Err(format!(
                "repairable matchers only work on items, not on a {kind} target"
            )),
            Matcher::Pattern(pattern) => {
                if is_id(&pattern.replace('*', "x")) {
                    Ok(())
                } else {
                    Err(format!(
                        "pattern {pattern:?} is not an id with `*` wildcards"
                    ))
                }
            }
            Matcher::Repairable(material) if material.starts_with('#') => {
                check_tag("repairable", material)
            }
            Matcher::Repairable(material) => check_id("repairable", material),
            Matcher::AllOf(matchers) | Matcher::AnyOf(matchers) => {
                if matchers.is_empty() {
                    return Err("all_of and any_of need at least one matcher".into());
                }
                matchers.iter().try_for_each(|m| m.validate(kind))
            }
        }
    }
}

impl SettingsPatch {
    fn validate(&self) -> Result<(), String> {
        check_at_least_one("max_level", self.max_level)?;
        check_cost(self.cost)?;
        check_at_least_one("points_every", self.points_every)?;
        self.not_creatures
            .iter()
            .chain(&self.projectile_damage)
            .try_for_each(|m| m.validate(TargetKind::Entity))?;
        self.equip_slots
            .iter()
            .try_for_each(|rule| rule.items.validate(TargetKind::Item))
    }
}

/// `namespace:path`, lowercase, as vanilla resource locations: `^[a-z0-9_.-]+:[a-z0-9_./-]+$`.
fn is_id(value: &str) -> bool {
    let Some((namespace, path)) = value.split_once(':') else {
        return false;
    };
    let namespace_char = |b: u8| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-');
    !namespace.is_empty()
        && !path.is_empty()
        && namespace.bytes().all(namespace_char)
        && path.bytes().all(|b| namespace_char(b) || b == b'/')
}

fn check_id(field: &str, value: &str) -> Result<(), String> {
    if is_id(value) {
        Ok(())
    } else {
        Err(format!(
            "{field} {value:?} is not a namespaced id (lowercase namespace:path)"
        ))
    }
}

fn check_tag(field: &str, value: &str) -> Result<(), String> {
    match value.strip_prefix('#') {
        Some(tag) if is_id(tag) => Ok(()),
        _ => Err(format!("{field} {value:?} is not a tag (#namespace:path)")),
    }
}

fn check_at_least_one(field: &str, value: Option<u32>) -> Result<(), String> {
    match value {
        Some(0) => Err(format!("{field} must be at least 1")),
        _ => Ok(()),
    }
}

fn check_cost(cost: Option<CostCurve>) -> Result<(), String> {
    check_at_least_one("cost.every", cost.map(|curve| curve.every))
}

fn check_finite(field: &str, value: f64) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{field} must be a finite number, found {value}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(kind: DefKind, json: &str) -> DefFile {
        parse_file(kind, json).unwrap_or_else(|e| panic!("expected {json} to parse, got: {e}"))
    }

    fn refusal(kind: DefKind, json: &str) -> String {
        match parse_file(kind, json) {
            Ok(file) => panic!("expected {json} to be refused, got {file:?}"),
            Err(e) => e.reason,
        }
    }

    fn perk_with_effect(effect: &str) -> String {
        format!(
            r#"{{"format": 1, "id": "reskilled:mining_test", "skill": "reskilled:mining",
                "name": "n", "description": "d", "cost": 1, "min_level": 1, "effects": [{effect}]}}"#
        )
    }

    #[test]
    fn skill_colour_and_perk_and_lock_icons_parse() {
        // Given a skill with a colour, a perk with an icon, and a lock entry with an icon
        let skill = r##"{"format": 1, "id": "reskilled:mining", "name": "n", "icon": "minecraft:stone",
            "color": "#8FA6B8"}"##;
        let perk = r#"{"format": 1, "id": "reskilled:p", "skill": "reskilled:mining", "name": "n",
            "description": "d", "cost": 1, "min_level": 1, "effects": [], "icon": "minecraft:raw_iron"}"#;
        let lock = r#"{"format": 1, "entries": [{"target": {"item": {"id": "minecraft:diamond_pickaxe"}},
            "actions": ["break"], "requires": {"reskilled:mining": 16}, "label": "l",
            "icon": "minecraft:diamond_pickaxe"}]}"#;

        // When they are parsed
        let (DefFile::Skill(skill), DefFile::Perk(perk), DefFile::Lock(lock)) = (
            parse_ok(DefKind::Skill, skill),
            parse_ok(DefKind::Perk, perk),
            parse_ok(DefKind::Lock, lock),
        ) else {
            panic!("a skill, a perk and a lock file");
        };

        // Then the colour reads as RGB and the icons are kept
        assert_eq!(skill_rgb(&skill), 0x8FA6B8);
        assert_eq!(perk.icon.as_deref(), Some("minecraft:raw_iron"));
        assert_eq!(
            lock.entries[0].icon.as_deref(),
            Some("minecraft:diamond_pickaxe")
        );
    }

    #[test]
    fn bad_colours_and_icons_are_refused() {
        let skill = |extra: &str| {
            format!(
                r##"{{"format": 1, "id": "a:b", "name": "n", "icon": "minecraft:stone", {extra}}}"##
            )
        };
        for colour in ["8FA6B8", "#8FA6B", "#GGGGGG"] {
            let reason = refusal(DefKind::Skill, &skill(&format!(r#""color": "{colour}""#)));
            assert!(reason.contains("color"), "{colour}: {reason}");
        }
        let perk = r#"{"format": 1, "id": "a:p", "skill": "a:b", "name": "n", "description": "d", "cost": 1,
            "min_level": 1, "effects": [], "icon": "Raw Iron"}"#;
        assert!(refusal(DefKind::Perk, perk).contains("icon"));
    }

    #[test]
    fn a_skill_without_a_colour_gets_a_stable_one() {
        // Given two skills with no colour
        let skill = |id: &str| match parse_ok(
            DefKind::Skill,
            &format!(r#"{{"format": 1, "id": "{id}", "name": "n", "icon": "minecraft:stone"}}"#),
        ) {
            DefFile::Skill(skill) => skill,
            other => panic!("{other:?}"),
        };
        let (a, b) = (skill("othermod:digging"), skill("othermod:cooking"));

        // When their colours are read twice
        // Then each is one of the fallback colours, the same every time
        assert_eq!(skill_rgb(&a), skill_rgb(&a));
        assert!(FALLBACK_RGB.contains(&skill_rgb(&a)));
        assert!(FALLBACK_RGB.contains(&skill_rgb(&b)));
    }

    #[test]
    fn a_skill_says_what_work_grows_it() {
        // Given a skill grown by breaking with pickaxes or a modded drill, by hits taken, and by travel
        let json = r##"{"format": 1, "id": "othermod:digging", "name": "n", "icon": "minecraft:stone",
            "earned_by": [
                {"type": "break", "with": {"any_of": [{"tag": "#minecraft:pickaxes"}, {"id": "othermod:drill"}]}},
                {"type": "hurt", "worth": 2},
                {"type": "travel", "per_block": 0.1},
                {"type": "place"}, {"type": "attack"}, {"type": "projectile_hit"}, {"type": "enchant", "worth": 5}
            ]}"##;

        // When it is parsed
        let DefFile::Skill(skill) = parse_ok(DefKind::Skill, json) else {
            panic!("a skill");
        };

        // Then each kind of work is read, with a worth of one action unless it says otherwise
        assert_eq!(
            skill.earned_by,
            vec![
                EarnRule::Break {
                    with: Matcher::AnyOf(vec![
                        Matcher::Tag("#minecraft:pickaxes".into()),
                        Matcher::Id("othermod:drill".into()),
                    ]),
                    worth: 1.0,
                },
                EarnRule::Hurt { worth: 2.0 },
                EarnRule::Travel { per_block: 0.1 },
                EarnRule::Place { worth: 1.0 },
                EarnRule::Attack { worth: 1.0 },
                EarnRule::ProjectileHit { worth: 1.0 },
                EarnRule::Enchant { worth: 5.0 },
            ]
        );
    }

    #[test]
    fn bad_earning_rules_are_refused() {
        let skill = |rule: &str| {
            format!(
                r#"{{"format": 1, "id": "a:b", "name": "n", "icon": "minecraft:stone", "earned_by": [{rule}]}}"#
            )
        };
        assert!(
            refusal(
                DefKind::Skill,
                &skill(r#"{"type": "break", "with": {"tag": "pickaxes"}}"#)
            )
            .contains("tag")
        );
        assert!(
            refusal(DefKind::Skill, &skill(r#"{"type": "hurt", "worth": -1}"#)).contains("worth")
        );
        assert!(
            refusal(
                DefKind::Skill,
                &skill(r#"{"type": "travel", "per_block": -0.5}"#)
            )
            .contains("per_block")
        );
        assert!(refusal(DefKind::Skill, &skill(r#"{"type": "fishing"}"#)).contains("fishing"));
    }

    #[test]
    fn crop_growth_grows_crops_unless_it_names_other_blocks() {
        // Given a crop growth perk that names no blocks, and one that also grows a modded bush
        let plain = perk_with(
            r#"{"type": "crop_growth", "radius": 5, "interval_seconds": 10, "chance": 0.25}"#,
        );
        let modded = perk_with(
            r##"{"type": "crop_growth", "radius": 5, "interval_seconds": 10, "chance": 0.25,
                "blocks": {"any_of": [{"tag": "#minecraft:crops"}, {"id": "othermod:berry_bush"}]}}"##,
        );

        // When they are parsed
        let blocks = |json: &str| match parse_ok(DefKind::Perk, json) {
            DefFile::Perk(perk) => match &perk.effects[0] {
                Effect::CropGrowth { blocks, .. } => blocks.clone(),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        };

        // Then the first grows #minecraft:crops, and the second also the bush
        assert_eq!(blocks(&plain), Matcher::Tag("#minecraft:crops".into()));
        assert_eq!(
            blocks(&modded),
            Matcher::AnyOf(vec![
                Matcher::Tag("#minecraft:crops".into()),
                Matcher::Id("othermod:berry_bush".into())
            ])
        );
    }

    #[test]
    fn parses_a_skill() {
        // Given a skill file with every field, and one with only the required ones
        let full = r#"{"format": 1, "id": "reskilled:mining", "priority": 5,
            "name": "reskilled.skill.mining", "icon": "minecraft:iron_pickaxe",
            "max_level": 40, "cost": {"base": 3, "every": 5}, "points_every": 5}"#;
        let minimal = r#"{"format": 1, "id": "reskilled:melee",
            "name": "reskilled.skill.melee", "icon": "minecraft:iron_sword"}"#;

        // When both are parsed
        let full = parse_ok(DefKind::Skill, full);
        let minimal = parse_ok(DefKind::Skill, minimal);

        // Then every given field is read, and the missing ones take their defaults
        assert_eq!(
            full,
            DefFile::Skill(SkillDef {
                id: "reskilled:mining".into(),
                priority: 5,
                name: "reskilled.skill.mining".into(),
                icon: "minecraft:iron_pickaxe".into(),
                max_level: Some(40),
                cost: Some(CostCurve { base: 3, every: 5 }),
                points_every: Some(5),
                icon_levels: Default::default(),
                earned_by: Vec::new(),
                color: None,
            })
        );
        assert_eq!(
            minimal,
            DefFile::Skill(SkillDef {
                id: "reskilled:melee".into(),
                priority: 0,
                name: "reskilled.skill.melee".into(),
                icon: "minecraft:iron_sword".into(),
                max_level: None,
                cost: None,
                points_every: None,
                icon_levels: Default::default(),
                earned_by: Vec::new(),
                color: None,
            })
        );
    }

    #[test]
    fn parses_a_perk_with_each_effect_type() {
        // Given a perk using all six effect types
        let json = r##"{"format": 1, "id": "reskilled:mining_deep_veins", "priority": 2,
            "skill": "reskilled:mining", "name": "reskilled.perk.mining_deep_veins",
            "description": "reskilled.perk.mining_deep_veins.desc", "cost": 2, "min_level": 8,
            "requires": ["reskilled:mining_sure_grip"],
            "effects": [
                {"type": "attribute", "attribute": "minecraft:block_break_speed", "amount": 0.1,
                 "operation": "add_multiplied_base"},
                {"type": "drop_bonus", "target": {"tag": "#minecraft:coal_ores"}, "chance": 0.25},
                {"type": "damage", "direction": "taken", "kind": "fall", "multiplier": 0.8},
                {"type": "xp_gain", "multiplier": 1.1},
                {"type": "anvil_cost", "multiplier": 0.9},
                {"type": "mount_attribute", "attribute": "minecraft:movement_speed", "amount": 0.05,
                 "operation": "add_value"}
            ]}"##;

        // When it is parsed
        let file = parse_ok(DefKind::Perk, json);

        // Then each effect keeps its type and values
        assert_eq!(
            file,
            DefFile::Perk(PerkDef {
                id: "reskilled:mining_deep_veins".into(),
                priority: 2,
                skill: "reskilled:mining".into(),
                name: "reskilled.perk.mining_deep_veins".into(),
                description: "reskilled.perk.mining_deep_veins.desc".into(),
                cost: 2,
                min_level: 8,
                requires: vec!["reskilled:mining_sure_grip".into()],
                also_needs: Default::default(),
                icon: None,
                effects: vec![
                    Effect::Attribute {
                        attribute: "minecraft:block_break_speed".into(),
                        amount: 0.1,
                        operation: Operation::AddMultipliedBase,
                    },
                    Effect::DropBonus {
                        target: Matcher::Tag("#minecraft:coal_ores".into()),
                        chance: 0.25
                    },
                    Effect::Damage {
                        direction: Direction::Taken,
                        kind: DamageKind::Fall,
                        multiplier: 0.8
                    },
                    Effect::XpGain { multiplier: 1.1 },
                    Effect::AnvilCost { multiplier: 0.9 },
                    Effect::MountAttribute {
                        attribute: "minecraft:movement_speed".into(),
                        amount: 0.05,
                        operation: Operation::AddValue,
                    },
                ],
            })
        );
    }

    #[test]
    fn parses_a_lock_file_with_nested_matchers() {
        // Given a lock file with an item, a block and an entity entry, and nested matchers
        let json = r##"{"format": 1, "entries": [
            {"target": {"item": {"all_of": [{"tag": "#minecraft:pickaxes"},
                                            {"repairable": "#minecraft:diamond_tool_materials"}]}},
             "actions": ["break", "attack"], "requires": {"reskilled:mining": 16},
             "label": "reskilled.lock.diamond_pickaxe"},
            {"target": {"block": {"any_of": [{"id": "minecraft:anvil"}, {"pattern": "minecraft:*_anvil"}]}},
             "actions": ["open"], "requires": {"reskilled:arcane": 6}, "replace": true},
            {"target": {"entity": {"id": "minecraft:horse"}}, "actions": ["mount"],
             "requires": {"reskilled:taming": 4}}
        ]}"##;

        // When it is parsed
        let file = parse_ok(DefKind::Lock, json);

        // Then the matchers nest as written, and label and replace default to none and false
        assert_eq!(
            file,
            DefFile::Lock(LockFile {
                entries: vec![
                    LockEntry {
                        target: Target::Item(Matcher::AllOf(vec![
                            Matcher::Tag("#minecraft:pickaxes".into()),
                            Matcher::Repairable("#minecraft:diamond_tool_materials".into()),
                        ])),
                        actions: vec![Action::Break, Action::Attack],
                        requires: [("reskilled:mining".to_string(), 16)].into(),
                        label: Some("reskilled.lock.diamond_pickaxe".into()),
                        replace: false,
                        enchantment_level: None,
                        icon: None,
                    },
                    LockEntry {
                        target: Target::Block(Matcher::AnyOf(vec![
                            Matcher::Id("minecraft:anvil".into()),
                            Matcher::Pattern("minecraft:*_anvil".into()),
                        ])),
                        actions: vec![Action::Open],
                        requires: [("reskilled:arcane".to_string(), 6)].into(),
                        label: None,
                        replace: true,
                        enchantment_level: None,
                        icon: None,
                    },
                    LockEntry {
                        target: Target::Entity(Matcher::Id("minecraft:horse".into())),
                        actions: vec![Action::Mount],
                        requires: [("reskilled:taming".to_string(), 4)].into(),
                        label: None,
                        replace: false,
                        enchantment_level: None,
                        icon: None,
                    },
                ],
            })
        );
    }

    #[test]
    fn parses_a_settings_patch() {
        // Given a settings file changing three fields
        let json = r#"{"format": 1, "priority": 10, "max_level": 50, "cost": {"base": 1, "every": 2},
            "creative_ignores_locks": false}"#;

        // When it is parsed
        let file = parse_ok(DefKind::Settings, json);

        // Then only those fields are set
        assert_eq!(
            file,
            DefFile::Settings(SettingsPatch {
                priority: 10,
                max_level: Some(50),
                cost: Some(CostCurve { base: 1, every: 2 }),
                creative_ignores_locks: Some(false),
                ..SettingsPatch::default()
            })
        );
    }

    #[test]
    fn refuses_unknown_fields() {
        // Given a skill with a misspelled field, and an effect with one
        let skill = r#"{"format": 1, "id": "reskilled:mining", "name": "n", "icon": "minecraft:stone",
            "max_levle": 40}"#;
        let effect =
            perk_with_effect(r#"{"type": "xp_gain", "multiplier": 1.1, "multipler": 2.0}"#);

        // When they are parsed
        let skill = refusal(DefKind::Skill, skill);
        let effect = refusal(DefKind::Perk, &effect);

        // Then both are refused and the reason names the field
        assert!(skill.contains("max_levle"), "{skill}");
        assert!(effect.contains("multipler"), "{effect}");
    }

    #[test]
    fn refuses_format_2() {
        // Given a file from a later format, and one with no format
        let later = r#"{"format": 2, "entries": []}"#;
        let missing = r#"{"entries": []}"#;

        // When they are parsed
        let later = refusal(DefKind::Lock, later);
        let missing = refusal(DefKind::Lock, missing);

        // Then both are refused because of the format
        assert!(later.contains("format"), "{later}");
        assert!(missing.contains("format"), "{missing}");
    }

    #[test]
    fn refuses_bad_id() {
        // Given ids without a namespace, with an uppercase letter, and an icon that is not an id
        let no_namespace =
            r#"{"format": 1, "id": "Mining", "name": "n", "icon": "minecraft:stone"}"#;
        let uppercase =
            r#"{"format": 1, "id": "reskilled:Mining", "name": "n", "icon": "minecraft:stone"}"#;
        let bad_icon = r#"{"format": 1, "id": "reskilled:mining", "name": "n", "icon": "stone"}"#;

        // When they are parsed
        let reasons = [no_namespace, uppercase, bad_icon].map(|json| refusal(DefKind::Skill, json));

        // Then each is refused and names the bad value
        assert!(reasons[0].contains("\"Mining\""), "{}", reasons[0]);
        assert!(
            reasons[1].contains("\"reskilled:Mining\""),
            "{}",
            reasons[1]
        );
        assert!(reasons[2].contains("\"stone\""), "{}", reasons[2]);
    }

    #[test]
    fn refuses_bad_ids_inside_perks_focus_and_locks() {
        // Given a bad id in each place a perk, focus area and lock entry names one
        let perk_skill = r#"{"format": 1, "id": "reskilled:p", "skill": "mining", "name": "n",
            "description": "d", "cost": 1, "min_level": 1, "effects": []}"#;
        let focus_skill =
            r#"{"format": 1, "id": "reskilled:working", "name": "n", "skills": ["Mining"]}"#;
        let lock_skill = r#"{"format": 1, "entries": [{"target": {"item": {"id": "minecraft:mace"}},
            "actions": ["attack"], "requires": {"melee": 20}}]}"#;
        let lock_tag = r#"{"format": 1, "entries": [{"target": {"item": {"tag": "minecraft:pickaxes"}},
            "actions": ["break"], "requires": {"reskilled:mining": 4}}]}"#;

        // When they are parsed
        let perk_skill = refusal(DefKind::Perk, perk_skill);
        let focus_skill = refusal(DefKind::Focus, focus_skill);
        let lock_skill = refusal(DefKind::Lock, lock_skill);
        let lock_tag = refusal(DefKind::Lock, lock_tag);

        // Then each is refused and names the bad value
        assert!(perk_skill.contains("\"mining\""), "{perk_skill}");
        assert!(focus_skill.contains("\"Mining\""), "{focus_skill}");
        assert!(lock_skill.contains("\"melee\""), "{lock_skill}");
        assert!(lock_tag.contains("\"minecraft:pickaxes\""), "{lock_tag}");
    }

    #[test]
    fn refuses_negative_or_nan_amounts() {
        // Given a negative chance, a chance above 1, a negative multiplier, and a NaN amount
        let negative_chance = perk_with_effect(
            r#"{"type": "drop_bonus", "target": {"id": "minecraft:stone"}, "chance": -0.1}"#,
        );
        let chance_above_one = perk_with_effect(
            r#"{"type": "drop_bonus", "target": {"id": "minecraft:stone"}, "chance": 1.5}"#,
        );
        let negative_multiplier = perk_with_effect(r#"{"type": "anvil_cost", "multiplier": -1.0}"#);
        let nan_amount = perk_with_effect(
            r#"{"type": "attribute", "attribute": "minecraft:luck", "amount": NaN, "operation": "add_value"}"#,
        );

        // When they are parsed
        let reasons = [
            negative_chance,
            chance_above_one,
            negative_multiplier,
            nan_amount,
        ]
        .map(|j| refusal(DefKind::Perk, &j));

        // Then each is refused; the range checks say which value is wrong
        assert!(reasons[0].contains("chance"), "{}", reasons[0]);
        assert!(reasons[1].contains("chance"), "{}", reasons[1]);
        assert!(reasons[2].contains("multiplier"), "{}", reasons[2]);
        assert!(!reasons[3].is_empty());
    }

    #[test]
    fn negative_attribute_amounts_are_allowed() {
        // Given a trade-off perk that lowers an attribute
        let json = perk_with_effect(
            r#"{"type": "attribute", "attribute": "minecraft:movement_speed", "amount": -0.05,
                "operation": "add_multiplied_base"}"#,
        );

        // When it is parsed
        let file = parse_file(DefKind::Perk, &json);

        // Then it is accepted
        assert!(file.is_ok(), "{file:?}");
    }

    #[test]
    fn refuses_zero_divisors() {
        // Given a zero cost step, a zero perk point step and a zero max level
        let zero_every = r#"{"format": 1, "cost": {"base": 2, "every": 0}}"#;
        let zero_points = r#"{"format": 1, "points_every": 0}"#;
        let zero_max = r#"{"format": 1, "id": "reskilled:mining", "name": "n", "icon": "minecraft:stone",
            "max_level": 0}"#;

        // When they are parsed
        let zero_every = refusal(DefKind::Settings, zero_every);
        let zero_points = refusal(DefKind::Settings, zero_points);
        let zero_max = refusal(DefKind::Skill, zero_max);

        // Then each is refused and names the field
        assert!(zero_every.contains("every"), "{zero_every}");
        assert!(zero_points.contains("points_every"), "{zero_points}");
        assert!(zero_max.contains("max_level"), "{zero_max}");
    }

    #[test]
    fn refuses_actions_that_do_not_fit_the_target() {
        // Given an item lock asking for `mount`, a block lock asking for `attack`, an entity lock asking for
        // `open`, and an entry with no actions
        let entry = |target: &str, action: &str| {
            format!(
                r#"{{"format": 1, "entries": [{{"target": {target}, "actions": [{action}],
                    "requires": {{"reskilled:taming": 4}}}}]}}"#
            )
        };
        let cases = [
            entry(r#"{"item": {"id": "minecraft:saddle"}}"#, r#""mount""#),
            entry(r#"{"block": {"id": "minecraft:anvil"}}"#, r#""attack""#),
            entry(r#"{"entity": {"id": "minecraft:horse"}}"#, r#""open""#),
            entry(r#"{"entity": {"id": "minecraft:horse"}}"#, ""),
        ];

        // When they are parsed
        let reasons = cases.map(|json| refusal(DefKind::Lock, &json));

        // Then each is refused and names the action that does not fit
        assert!(reasons[0].contains("mount"), "{}", reasons[0]);
        assert!(reasons[1].contains("attack"), "{}", reasons[1]);
        assert!(reasons[2].contains("open"), "{}", reasons[2]);
        assert!(reasons[3].contains("actions"), "{}", reasons[3]);
    }

    #[test]
    fn parses_a_focus_area_with_its_rewards() {
        // Given a focus area with one of each reward
        let json = r##"{"format": 1, "id": "reskilled:working", "name": "n",
            "skills": ["reskilled:mining"], "rewards": [
            {"task": "kill", "bonus_percent": 50},
            {"task": "block_xp", "bonus_percent": 25},
            {"task": "break", "blocks": {"tag": "#minecraft:logs"}, "points": 0.25, "label": "l"},
            {"task": "break", "blocks": {"id": "minecraft:wheat"}, "points": 0.5, "state": {"age": "7"}},
            {"task": "smelt", "points": 0.1},
            {"task": "travel", "points": 0.02},
            {"task": "tame", "points": 5},
            {"task": "breed", "points": 2}]}"##;

        // When it is parsed
        let Ok(DefFile::Focus(focus)) = parse_file(DefKind::Focus, json) else {
            panic!("{json} should parse");
        };

        // Then every reward is read, a break's state and label included
        assert_eq!(focus.rewards.len(), 8);
        assert_eq!(
            focus.rewards[3],
            Reward::Break {
                blocks: Matcher::Id("minecraft:wheat".into()),
                points: 0.5,
                state: [("age".to_string(), "7".to_string())].into(),
                label: None,
            }
        );
    }

    #[test]
    fn refuses_rewards_that_make_no_sense() {
        // Given negative points, a block reward matching by repair material, and an unknown task
        let focus = |reward: &str| {
            format!(
                r#"{{"format": 1, "id": "reskilled:working", "name": "n", "skills": [], "rewards": [{reward}]}}"#
            )
        };
        let negative = focus(r#"{"task": "travel", "points": -1}"#);
        let repairable = focus(
            r##"{"task": "break", "blocks": {"repairable": "#minecraft:iron_tool_materials"}, "points": 1}"##,
        );
        let unknown = focus(r#"{"task": "sleep", "points": 1}"#);

        // When they are parsed
        let reasons = [negative, repairable, unknown].map(|json| refusal(DefKind::Focus, &json));

        // Then each is refused for its reason
        assert!(reasons[0].contains("points"), "{}", reasons[0]);
        assert!(reasons[1].contains("repairable"), "{}", reasons[1]);
        assert!(reasons[2].contains("sleep"), "{}", reasons[2]);
    }

    #[test]
    fn a_skills_icon_improves_with_its_level() {
        // Given a skill with a wooden pickaxe icon that becomes iron at 8 and diamond at 16
        let json = r#"{"format": 1, "id": "reskilled:mining", "name": "n", "icon": "minecraft:wooden_pickaxe",
            "icon_levels": {"8": "minecraft:iron_pickaxe", "16": "minecraft:diamond_pickaxe"}}"#;
        let DefFile::Skill(skill) = parse_ok(DefKind::Skill, json) else {
            panic!("a skill");
        };

        // When its icon is read at levels 1, 7, 8, 15, 16 and 32
        let icons = [1, 7, 8, 15, 16, 32].map(|level| skill.icon_at(level).to_string());

        // Then each level shows the best icon it has reached
        assert_eq!(
            icons,
            [
                "minecraft:wooden_pickaxe",
                "minecraft:wooden_pickaxe",
                "minecraft:iron_pickaxe",
                "minecraft:iron_pickaxe",
                "minecraft:diamond_pickaxe",
                "minecraft:diamond_pickaxe"
            ]
        );
        let bad = refusal(
            DefKind::Skill,
            r#"{"format": 1, "id": "reskilled:mining", "name": "n", "icon": "minecraft:stone",
            "icon_levels": {"8": "Not An Id"}}"#,
        );
        assert!(bad.contains("icon_levels"), "{bad}");
    }

    #[test]
    fn settings_default_matches_spec() {
        // Given no settings file at all
        // When the defaults are taken
        let settings = Settings::default();

        // Then they are the spec's defaults
        assert_eq!(
            settings,
            Settings {
                max_level: 32,
                cost: CostCurve { base: 1, every: 1 },
                points_every: 2,
                focus_first_free: true,
                focus_cost: 10,
                death_penalty_levels: 0,
                creative_ignores_locks: true,
                lock_message_cooldown_ticks: 20,
                not_creatures: Vec::new(),
                projectile_damage: Vec::new(),
                equip_slots: Vec::new(),
            }
        );
    }

    fn perk_with(effects: &str) -> String {
        format!(
            r#"{{"format": 1, "id": "reskilled:p", "skill": "reskilled:defense", "name": "n", "description": "d",
                "cost": 1, "min_level": 1, "effects": [{effects}]}}"#
        )
    }

    #[test]
    fn parses_the_mechanic_effects() {
        // Given a perk with a last stand, an effect on kill, crop growth, replanting and an ender pearl rule
        let json = perk_with(
            r#"{"type": "last_stand", "above": 6.0, "cooldown_seconds": 300},
               {"type": "effect_on_kill", "effect": "minecraft:strength", "seconds": 5, "amplifier": 0,
                "mobs": {"any_of": [{"id": "minecraft:zombie"}]}},
               {"type": "crop_growth", "radius": 5, "interval_seconds": 10, "chance": 0.25},
               {"type": "replant", "crops": [{"block": "minecraft:wheat", "seed": "minecraft:wheat_seeds",
                "ripe_age": 7}]},
               {"type": "damage", "direction": "taken", "kind": "ender_pearl", "multiplier": 0.0}"#,
        );

        // When it is parsed
        let DefFile::Perk(perk) = parse_ok(DefKind::Perk, &json) else {
            panic!("a perk");
        };

        // Then each effect is there
        assert_eq!(perk.effects.len(), 5);
        assert!(matches!(
            perk.effects[0],
            Effect::LastStand {
                cooldown_seconds: 300,
                ..
            }
        ));
        assert!(matches!(&perk.effects[3], Effect::Replant { crops } if crops[0].ripe_age == 7));
    }

    #[test]
    fn parses_the_rune_and_home_effects() {
        // Given a perk with every rune and home effect, and a magic damage rule
        let json = perk_with(
            r#"{"type": "enchanted_weapon", "per_enchantment": 0.03, "max": 0.15},
               {"type": "enchanted_armor", "per_enchantment": 0.01, "max": 0.08},
               {"type": "shorter_effects", "effects": ["minecraft:poison"], "multiplier": 0.6},
               {"type": "rune_repair", "interval_seconds": 30, "points": 10},
               {"type": "salvage"},
               {"type": "restful_sleep", "clears": ["minecraft:poison"]},
               {"type": "near_home", "radius": 16, "effect": "minecraft:regeneration", "amplifier": 0},
               {"type": "home_ward", "radius": 24},
               {"type": "safe_haven", "radius": 32},
               {"type": "damage", "direction": "taken", "kind": "magic", "multiplier": 0.7}"#,
        );

        // When it is parsed
        let DefFile::Perk(perk) = parse_ok(DefKind::Perk, &json) else {
            panic!("a perk");
        };

        // Then all ten are there
        assert_eq!(perk.effects.len(), 10);
        assert!(matches!(perk.effects[4], Effect::Salvage));
    }

    #[test]
    fn rune_and_home_effects_that_make_no_sense_are_refused() {
        // Given a weapon bonus capped below its step, a shortening that lengthens, and a ward of 200 blocks
        let cap = perk_with(r#"{"type": "enchanted_weapon", "per_enchantment": 0.5, "max": -1.0}"#);
        let longer = perk_with(
            r#"{"type": "shorter_effects", "effects": ["minecraft:poison"], "multiplier": 1.5}"#,
        );
        let wide = perk_with(r#"{"type": "home_ward", "radius": 200}"#);

        // When they are parsed
        let [cap, longer, wide] = [cap, longer, wide].map(|json| refusal(DefKind::Perk, &json));

        // Then each is refused, saying why
        assert!(cap.contains("max"), "{cap}");
        assert!(longer.contains("multiplier"), "{longer}");
        assert!(wide.contains("radius"), "{wide}");
    }

    #[test]
    fn mechanic_effects_that_make_no_sense_are_refused() {
        // Given a crop growth chance of 2, a radius of 40, and a kill effect on a tag of mobs
        let chance = perk_with(
            r#"{"type": "crop_growth", "radius": 5, "interval_seconds": 10, "chance": 2.0}"#,
        );
        let radius = perk_with(
            r#"{"type": "crop_growth", "radius": 40, "interval_seconds": 10, "chance": 0.5}"#,
        );
        let tag = perk_with(
            r##"{"type": "effect_on_kill", "effect": "minecraft:strength", "seconds": 5, "amplifier": 0,
                "mobs": {"tag": "#minecraft:undead"}}"##,
        );

        // When they are parsed
        let [chance, radius, tag] = [chance, radius, tag].map(|json| refusal(DefKind::Perk, &json));

        // Then each is refused, saying why
        assert!(chance.contains("chance"), "{chance}");
        assert!(radius.contains("radius"), "{radius}");
        assert!(tag.contains("tag"), "{tag}");
    }

    #[test]
    fn parses_an_ability_lock() {
        // Given a lock on another mod's dash
        let json = r#"{"format": 1, "entries": [{"target": {"ability": {"id": "dashmod:dash"}},
            "actions": ["use"], "requires": {"reskilled:agility": 10}}]}"#;

        // When it is parsed
        let DefFile::Lock(file) = parse_ok(DefKind::Lock, json) else {
            panic!("a lock file");
        };

        // Then its target is the ability
        assert_eq!(
            file.entries[0].target,
            Target::Ability(Matcher::Id("dashmod:dash".into()))
        );
    }

    #[test]
    fn ability_locks_allow_only_use_and_no_tags() {
        // Given an ability lock that equips, and one by tag
        let equip = r#"{"format": 1, "entries": [{"target": {"ability": {"id": "dashmod:dash"}},
            "actions": ["equip"], "requires": {"reskilled:agility": 10}}]}"#;
        let tag = r##"{"format": 1, "entries": [{"target": {"ability": {"tag": "#dashmod:moves"}},
            "actions": ["use"], "requires": {"reskilled:agility": 10}}]}"##;

        // When they are parsed
        let (equip, tag) = (refusal(DefKind::Lock, equip), refusal(DefKind::Lock, tag));

        // Then both are refused and say why
        assert!(equip.contains("equip"), "{equip}");
        assert!(tag.contains("tag"), "{tag}");
    }

    #[test]
    fn refuses_enchantment_locks_that_make_no_sense() {
        // Given an enchantment lock by tag, one that opens blocks, and an item lock naming an enchantment level
        let entry = |target: &str, actions: &str, extra: &str| {
            format!(
                r#"{{"format": 1, "entries": [{{"target": {target}, "actions": [{actions}], {extra}
                    "requires": {{"reskilled:melee": 4}}}}]}}"#
            )
        };
        let cases = [
            entry(
                r##"{"enchantment": {"tag": "#minecraft:curse"}}"##,
                r#""attack""#,
                "",
            ),
            entry(
                r#"{"enchantment": {"id": "minecraft:sharpness"}}"#,
                r#""open""#,
                "",
            ),
            entry(
                r#"{"item": {"id": "minecraft:stick"}}"#,
                r#""use""#,
                r#""enchantment_level": 2,"#,
            ),
        ];

        // When they are parsed
        let reasons = cases.map(|json| refusal(DefKind::Lock, &json));

        // Then each is refused and says why
        assert!(reasons[0].contains("tag"), "{}", reasons[0]);
        assert!(reasons[1].contains("open"), "{}", reasons[1]);
        assert!(reasons[2].contains("enchantment_level"), "{}", reasons[2]);
    }

    #[test]
    fn refuses_matchers_the_target_kind_cannot_use() {
        // Given an entity lock by tag, a block lock by repair material nested in any_of, and a drop bonus (which
        // matches broken blocks) by repair material
        let entity_tag = r##"{"format": 1, "entries": [{"target": {"entity": {"tag": "#minecraft:skeletons"}},
            "actions": ["mount"], "requires": {"reskilled:taming": 4}}]}"##;
        let block_repairable = r##"{"format": 1, "entries": [{"target": {"block": {"any_of": [
            {"id": "minecraft:anvil"}, {"repairable": "#minecraft:iron_tool_materials"}]}},
            "actions": ["open"], "requires": {"reskilled:arcane": 6}}]}"##;
        let drop_repairable = perk_with_effect(
            r##"{"type": "drop_bonus", "target": {"repairable": "minecraft:diamond"}, "chance": 0.5}"##,
        );

        // When they are parsed
        let entity_tag = refusal(DefKind::Lock, entity_tag);
        let block_repairable = refusal(DefKind::Lock, block_repairable);
        let drop_repairable = refusal(DefKind::Perk, &drop_repairable);

        // Then each is refused, naming the matcher and the kind it cannot apply to
        assert!(
            entity_tag.contains("tag") && entity_tag.contains("entity"),
            "{entity_tag}"
        );
        assert!(
            block_repairable.contains("repairable") && block_repairable.contains("block"),
            "{block_repairable}"
        );
        assert!(
            drop_repairable.contains("repairable") && drop_repairable.contains("block"),
            "{drop_repairable}"
        );
    }

    #[test]
    fn error_displays_file_then_reason() {
        // Given an error for a file
        let error = DefError {
            file: "reskilled:skills/mining".into(),
            reason: "bad id".into(),
        };

        // When it is displayed
        let shown = error.to_string();

        // Then the file comes first
        assert_eq!(shown, "reskilled:skills/mining: bad id");
    }
}
