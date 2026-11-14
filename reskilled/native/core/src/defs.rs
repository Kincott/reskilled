//! The data files a pack maker writes: skills, perks, focus areas, locks, settings and perk guides, one JSON object per file.
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
    /// Every `interval_seconds`, each crop within `radius` blocks grows a stage with `chance`.
    CropGrowth {
        radius: u32,
        interval_seconds: u32,
        chance: f64,
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
    /// Waking from a full sleep heals the player and takes these effects off them.
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
    Guide,
}

/// One parsed data file.
#[derive(Debug, Clone, PartialEq)]
pub enum DefFile {
    Skill(SkillDef),
    Perk(PerkDef),
    Focus(FocusDef),
    Lock(LockFile),
    Settings(SettingsPatch),
    Guide(GuideDef),
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
    let file = match kind {
        DefKind::Skill => DefFile::Skill(serde_json::from_value(value).map_err(refused)?),
        DefKind::Perk => DefFile::Perk(serde_json::from_value(value).map_err(refused)?),
        DefKind::Focus => DefFile::Focus(serde_json::from_value(value).map_err(refused)?),
        DefKind::Lock => DefFile::Lock(serde_json::from_value(value).map_err(refused)?),
        DefKind::Settings => DefFile::Settings(serde_json::from_value(value).map_err(refused)?),
        DefKind::Guide => DefFile::Guide(serde_json::from_value(value).map_err(refused)?),
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
            DefFile::Skill(skill) => skill.validate(),
            DefFile::Perk(perk) => perk.validate(),
            DefFile::Focus(focus) => focus.validate(),
            DefFile::Lock(locks) => locks.entries.iter().try_for_each(LockEntry::validate),
            DefFile::Settings(patch) => patch.validate(),
            DefFile::Guide(guide) => guide.validate(),
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
        check_at_least_one("points_every", self.points_every)
    }
}

impl PerkDef {
    fn validate(&self) -> Result<(), String> {
        check_id("id", &self.id)?;
        check_id("skill", &self.skill)?;
        self.requires
            .iter()
            .try_for_each(|perk| check_id("requires", perk))?;
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
            } => {
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
        check_at_least_one("points_every", self.points_every)
    }
}

/// The animated scene that explains a perk in the guide: a timeline of steps over a small diorama of blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuideDef {
    /// The perk it explains.
    pub perk: String,
    #[serde(default)]
    pub priority: i32,
    /// The loop length; at the end the scene starts again from an empty diorama.
    pub seconds: f32,
    pub view: GuideView,
    /// In time order.
    pub steps: Vec<GuideStep>,
}

/// Where the scene is looked at from.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuideView {
    /// The scene point the view is centered on.
    pub center: [f32; 3],
    /// Degrees.
    pub yaw: f32,
    /// Degrees.
    pub pitch: f32,
    /// How large a block draws, in pixels.
    pub scale: f32,
    /// Degrees a second the view turns; 0 holds still.
    #[serde(default)]
    pub spin: f32,
}

/// One thing that happens `at` a second of the scene. No `deny_unknown_fields`: serde cannot combine it with
/// `flatten`, so an unknown action key is refused as "unknown variant" by the action instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuideStep {
    pub at: f32,
    #[serde(flatten)]
    pub action: GuideAction,
}

/// What a step does; the key that names it is the variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuideAction {
    /// Puts blocks in the scene.
    Place(Vec<PlacedBlock>),
    /// Empties these scene positions.
    Remove(Vec<[i32; 3]>),
    /// The line shown under the scene until the next caption.
    Caption(Caption),
    /// An item drawn flat over the scene box, until a step clears icons.
    Icon(Overlay),
    /// A potion effect's icon drawn the same way.
    Effect(Overlay),
    /// A heart bar over the box with this many half hearts.
    Hearts(u32),
    /// Takes overlays away.
    Clear(ClearWhat),
}

/// A block at a scene position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedBlock {
    pub pos: [i32; 3],
    /// A state string in the game's own form, `minecraft:wheat[age=7]`; see [`parse_state`].
    pub block: String,
}

/// A line of the scene's text. It never states a perk's numbers: it has a `%s` for each of `args`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caption {
    /// Translation key.
    pub key: String,
    /// English fallback for clients that lack the key.
    pub text: String,
    /// Values of the perk's effects filling the `%s` of the text in order.
    #[serde(default)]
    pub args: Vec<CaptionArg>,
}

/// A value of one of the perk's effects, and how a caption shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptionArg {
    /// The effect's `type`, such as `crop_growth`.
    pub effect: String,
    /// A numeric field of that effect, such as `chance`.
    pub field: String,
    #[serde(rename = "as")]
    pub shown_as: ShownAs,
}

/// How a caption shows a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShownAs {
    Number,
    /// 0.25 reads 25%.
    Percent,
    Seconds,
    Blocks,
    /// Health points halved.
    Hearts,
}

/// An item or potion effect icon over the scene box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    pub item: Option<String>,
    pub effect: Option<String>,
    /// Fractions of the box, 0 to 1 from its top left.
    pub at: [f32; 2],
}

/// What a clear step takes away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClearWhat {
    Icons,
    Hearts,
}

impl GuideDef {
    fn validate(&self) -> Result<(), String> {
        check_id("perk", &self.perk)?;
        check_positive("seconds", self.seconds)?;
        let view = &self.view;
        for (field, value) in [
            ("view.yaw", view.yaw),
            ("view.pitch", view.pitch),
            ("view.spin", view.spin),
        ] {
            check_finite(field, value.into())?;
        }
        for value in view.center {
            check_finite("view.center", value.into())?;
        }
        check_positive("view.scale", view.scale)?;
        let mut last = 0.0;
        for (index, step) in self.steps.iter().enumerate() {
            let at = step.at;
            check_finite("at", at.into())?;
            if at < 0.0 || at > self.seconds {
                return Err(format!(
                    "step {index}: at {at} is outside the scene's 0 to {} seconds",
                    self.seconds
                ));
            }
            if at < last {
                return Err(format!(
                    "step {index}: at {at} is before the step above it ({last}); steps must be in time order"
                ));
            }
            last = at;
            step.action
                .validate()
                .map_err(|reason| format!("step {index}: {reason}"))?;
        }
        Ok(())
    }
}

impl GuideAction {
    fn validate(&self) -> Result<(), String> {
        match self {
            GuideAction::Place(blocks) => blocks.iter().try_for_each(PlacedBlock::validate),
            GuideAction::Caption(caption) => caption.validate(),
            GuideAction::Icon(overlay) => overlay.validate("item"),
            GuideAction::Effect(overlay) => overlay.validate("effect"),
            GuideAction::Remove(_) | GuideAction::Hearts(_) | GuideAction::Clear(_) => Ok(()),
        }
    }
}

impl PlacedBlock {
    fn validate(&self) -> Result<(), String> {
        parse_state(&self.block).map(|_| ()).ok_or_else(|| {
            format!(
                "block {:?} is not a block state (minecraft:wheat[age=7], lowercase)",
                self.block
            )
        })
    }
}

impl Caption {
    fn validate(&self) -> Result<(), String> {
        if self.key.is_empty() {
            return Err("caption key must not be empty".into());
        }
        if self.text.is_empty() {
            return Err("caption text must not be empty".into());
        }
        self.args.iter().try_for_each(|arg| {
            if arg.effect.is_empty() || arg.field.is_empty() {
                Err("a caption arg needs an effect and a field".to_string())
            } else {
                Ok(())
            }
        })
    }
}

impl Overlay {
    /// `needed` is the field this kind of overlay draws: `item` for an icon, `effect` for an effect.
    fn validate(&self, needed: &str) -> Result<(), String> {
        let (found, other) = match needed {
            "item" => (&self.item, &self.effect),
            _ => (&self.effect, &self.item),
        };
        match (found, other) {
            (Some(id), None) => check_id(needed, id)?,
            (None, _) => return Err(format!("{needed} is missing")),
            (Some(_), Some(_)) => return Err("give an item or an effect, not both".into()),
        }
        for value in self.at {
            if !(0.0..=1.0).contains(&value) {
                return Err(format!(
                    "at must be fractions of the box between 0 and 1, found {value}"
                ));
            }
        }
        Ok(())
    }
}

/// Splits a block state string such as `minecraft:wheat[age=7,x=y]` into the block id and its properties, in
/// order. `None` when it is not one: a bad id, an unclosed bracket, or a property without a name or a value.
pub fn parse_state(state: &str) -> Option<(String, Vec<(String, String)>)> {
    let (block, properties) = match state.split_once('[') {
        None => (state, Vec::new()),
        Some((block, rest)) => {
            let inside = rest.strip_suffix(']')?;
            let properties = inside
                .split(',')
                .map(|pair| {
                    let (name, value) = pair.split_once('=')?;
                    let word = |s: &str| {
                        !s.is_empty()
                            && s.bytes()
                                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
                    };
                    (word(name) && word(value)).then(|| (name.to_string(), value.to_string()))
                })
                .collect::<Option<Vec<_>>>()?;
            (block, properties)
        }
    };
    is_id(block).then(|| (block.to_string(), properties))
}

/// The numeric `field` of the perk's first effect whose `type` is `effect` (`crop_growth`, `chance`), read as the
/// effect is written in a data file. `None` when the perk has no such effect, or the field is missing or not a
/// number.
pub fn effect_field(perk: &PerkDef, effect: &str, field: &str) -> Option<f64> {
    perk.effects.iter().find_map(|candidate| {
        let value = serde_json::to_value(candidate).ok()?;
        (value.get("type")?.as_str()? == effect)
            .then(|| value.get(field)?.as_f64())
            .flatten()
    })
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

fn check_positive(field: &str, value: f32) -> Result<(), String> {
    check_finite(field, value.into())?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(format!("{field} must be more than 0, found {value}"))
    }
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
                    },
                    LockEntry {
                        target: Target::Entity(Matcher::Id("minecraft:horse".into())),
                        actions: vec![Action::Mount],
                        requires: [("reskilled:taming".to_string(), 4)].into(),
                        label: None,
                        replace: false,
                        enchantment_level: None,
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

    const REPLANT_GUIDE: &str = r#"{
      "format": 1,
      "perk": "reskilled:gathering_sapling_keeper",
      "priority": 0,
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

    #[test]
    fn parses_a_guide() {
        // Given the replant scene
        // When it is parsed
        let DefFile::Guide(guide) = parse_ok(DefKind::Guide, REPLANT_GUIDE) else {
            panic!("expected a guide");
        };

        // Then it has its six steps, the first placing ripe wheat
        assert_eq!(guide.perk, "reskilled:gathering_sapling_keeper");
        assert_eq!(guide.seconds, 8.0);
        assert_eq!(guide.view.spin, 8.0);
        assert_eq!(guide.steps.len(), 6);
        let GuideAction::Place(blocks) = &guide.steps[0].action else {
            panic!("expected a place step, got {:?}", guide.steps[0]);
        };
        assert_eq!(blocks[1].pos, [0, 1, 0]);
        assert_eq!(blocks[1].block, "minecraft:wheat[age=7]");
    }

    #[test]
    fn a_guides_spin_defaults_to_zero() {
        // Given a guide whose view names no spin
        let json = REPLANT_GUIDE.replace(", \"spin\": 8", "");

        // When it is parsed
        let DefFile::Guide(guide) = parse_ok(DefKind::Guide, &json) else {
            panic!("expected a guide");
        };

        // Then it holds still
        assert_eq!(guide.view.spin, 0.0);
    }

    #[test]
    fn guides_that_make_no_sense_are_refused() {
        // Given scenes with a zero length, a step past the end, steps out of order, a zero scale, an overlay
        // outside the box and a broken state string
        let cases = [
            (
                REPLANT_GUIDE.replace("\"seconds\": 8", "\"seconds\": 0"),
                "seconds",
            ),
            (
                REPLANT_GUIDE.replace("{\"at\": 4, \"place\"", "{\"at\": 9, \"place\""),
                "at",
            ),
            (
                REPLANT_GUIDE.replace("{\"at\": 3, \"remove\"", "{\"at\": 5, \"remove\""),
                "order",
            ),
            (
                REPLANT_GUIDE.replace("\"scale\": 22", "\"scale\": 0"),
                "scale",
            ),
            (REPLANT_GUIDE.replace("[0.55, 0.4]", "[1.5, 0.4]"), "at"),
            (REPLANT_GUIDE.replace("wheat[age=7]", "wheat[age"), "block"),
        ];

        // When each is parsed
        let reasons = cases
            .clone()
            .map(|(json, _)| refusal(DefKind::Guide, &json));

        // Then each is refused, the reason naming the field
        for (reason, (_, field)) in reasons.iter().zip(&cases) {
            assert!(reason.contains(field), "{field}: {reason}");
        }
    }

    #[test]
    fn an_icon_needs_an_item_and_an_effect_needs_an_effect() {
        // Given an icon step with only an effect, and an effect step with only an item
        let icon = REPLANT_GUIDE.replace(
            "\"item\": \"minecraft:wheat_seeds\"",
            "\"effect\": \"minecraft:strength\"",
        );
        let effect = REPLANT_GUIDE.replace("\"icon\": {\"item\"", "\"effect\": {\"item\"");

        // When they are parsed
        let icon = refusal(DefKind::Guide, &icon);
        let effect = refusal(DefKind::Guide, &effect);

        // Then each is refused naming the missing field
        assert!(icon.contains("item"), "{icon}");
        assert!(effect.contains("effect"), "{effect}");
    }

    #[test]
    fn parse_state_reads_block_and_properties() {
        // Given states with properties, with none, and with a property lacking a value
        // When they are read
        let wheat = parse_state("minecraft:wheat[age=7,x=y]");
        let stone = parse_state("minecraft:stone");
        let bare = parse_state("minecraft:wheat[age]");

        // Then the block and properties come out, or nothing for the broken one
        assert_eq!(
            wheat,
            Some((
                "minecraft:wheat".to_string(),
                vec![
                    ("age".to_string(), "7".to_string()),
                    ("x".to_string(), "y".to_string())
                ]
            ))
        );
        assert_eq!(stone, Some(("minecraft:stone".to_string(), vec![])));
        assert_eq!(bare, None);
        assert_eq!(parse_state("Wheat"), None);
        assert_eq!(parse_state("minecraft:wheat[]"), None);
    }

    #[test]
    fn effect_field_reads_a_number_of_the_perks_first_matching_effect() {
        // Given a perk with two crop growths and a last stand
        let json = perk_with(
            r#"{"type": "last_stand", "above": 6.0, "cooldown_seconds": 300},
               {"type": "crop_growth", "radius": 5, "interval_seconds": 10, "chance": 0.25},
               {"type": "crop_growth", "radius": 9, "interval_seconds": 10, "chance": 0.5}"#,
        );
        let DefFile::Perk(perk) = parse_ok(DefKind::Perk, &json) else {
            panic!("expected a perk");
        };

        // When fields are read
        // Then the first matching effect answers, and a missing effect or field gives nothing
        assert_eq!(effect_field(&perk, "crop_growth", "radius"), Some(5.0));
        assert_eq!(effect_field(&perk, "crop_growth", "chance"), Some(0.25));
        assert_eq!(effect_field(&perk, "last_stand", "above"), Some(6.0));
        assert_eq!(effect_field(&perk, "crop_growth", "speed"), None);
        assert_eq!(effect_field(&perk, "replant", "radius"), None);
        assert_eq!(effect_field(&perk, "crop_growth", "type"), None);
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
