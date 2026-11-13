//! Every line the server shows players, as a translation key with its English, so a player without the mod's
//! language file (a vanilla client on a dedicated server, the console) still reads English.

use horizon_client::mod_api::Text;
use skills_core::defs::PerkDef;
use skills_core::progress::{Refusal, Request};
use skills_core::registry::Defs;

/// A translation key and its English, which is also the template when the key is missing.
pub struct Message {
    pub key: &'static str,
    pub english: &'static str,
}

macro_rules! messages {
    ($($name:ident = $key:literal => $english:literal;)*) => {
        $(pub const $name: Message = Message { key: $key, english: $english };)*
        /// Every message, for the test that keeps them in step with `en_us.json`.
        #[cfg(test)]
        pub const ALL: &[Message] = &[$($name),*];
    };
}

messages! {
    NEEDS = "reskilled.msg.needs" => "Needs %s %s";
    UNKNOWN_SKILL = "reskilled.msg.unknown_skill" => "There is no such skill.";
    UNKNOWN_PERK = "reskilled.msg.unknown_perk" => "There is no such perk.";
    UNKNOWN_FOCUS = "reskilled.msg.unknown_focus" => "There is no such focus.";
    AT_MAX = "reskilled.msg.at_max" => "That skill is already at its highest level.";
    NOT_ENOUGH_LEVELS = "reskilled.msg.not_enough_levels" => "Needs %s XP levels.";
    NOT_ENOUGH_POINTS = "reskilled.msg.not_enough_points" => "Needs %s perk points.";
    BELOW_MIN_LEVEL = "reskilled.msg.below_min_level" => "Needs %s %s first.";
    MISSING_PERK = "reskilled.msg.missing_perk" => "Get %s first.";
    ALREADY_OWNED = "reskilled.msg.already_owned" => "You already have that perk.";
    ALREADY_FOCUSED = "reskilled.msg.already_focused" => "That focus is already active.";
    UNAVAILABLE = "reskilled.msg.unavailable" => "That cannot be done right now.";
    SUMMARY = "reskilled.cmd.summary" => "XP levels: %s. Focus: %s.";
    NO_FOCUS = "reskilled.cmd.no_focus" => "none";
    SKILL_LINE = "reskilled.cmd.skill_line" => "%s %s/%s: raise for %s levels, %s perk points.";
    SKILL_LINE_MAX = "reskilled.cmd.skill_line_max" => "%s %s/%s: at the top, %s perk points.";
    RAISED = "reskilled.cmd.raised" => "%s is now level %s.";
    PERK_BOUGHT = "reskilled.cmd.perk_bought" => "You now have %s.";
    FOCUS_ADDED = "reskilled.cmd.focus_added" => "%s is now one of your focus areas.";
    SET_LEVEL = "reskilled.cmd.set" => "Set %s's %s to level %s.";
    SET_POINTS = "reskilled.cmd.points" => "Set %s's bonus points in %s to %s.";
    RESET_ONE = "reskilled.cmd.reset_one" => "Reset %s's %s.";
    RESET_ALL = "reskilled.cmd.reset_all" => "Reset all of %s's skills.";
    NO_PLAYER = "reskilled.cmd.no_player" => "No player named %s is online.";
    PLAYERS_ONLY = "reskilled.cmd.players_only" => "Only players can do that.";
    NOT_ALLOWED = "reskilled.cmd.not_allowed" => "You may not do that.";
    USAGE = "reskilled.cmd.usage" => "Usage: /skills [raise <skill> | perk <perk> | focus <area> | open]";
    REQUIRES = "reskilled.ui.requires" => "%s: %s %s";
    REQUIREMENTS = "reskilled.ui.requirements" => "Requirements";
    YOU_HAVE = "reskilled.ui.you_have" => "(you: %s)";
    TITLE = "reskilled.ui.title" => "Reskilled";
    XP = "reskilled.ui.xp" => "XP levels: %s";
    LEVEL = "reskilled.ui.level" => "%s/%s";
    LEVEL_OF = "reskilled.ui.level_of" => "Level %s of %s";
    LEVEL_UP = "reskilled.ui.level_up" => "Level up";
    RAISE_TO = "reskilled.ui.raise_to" => "Raise to level %s";
    COST = "reskilled.ui.cost" => "Cost: %s XP levels";
    AT_TOP = "reskilled.ui.at_top" => "Max level";
    PERK_POINTS = "reskilled.ui.perk_points" => "Perk points: %s";
    PERKS = "reskilled.ui.perks" => "Perks";
    PERK_COST = "reskilled.ui.perk_cost" => "Cost: %s perk points";
    OWNED = "reskilled.ui.owned" => "Owned";
    GET = "reskilled.ui.get" => "Unlock";
    NEXT_UNLOCKS = "reskilled.ui.next_unlocks" => "Next unlocks";
    UNLOCK = "reskilled.ui.unlock" => "Level %s: %s";
    ALL_UNLOCKED = "reskilled.ui.all_unlocked" => "Nothing more to unlock here.";
    OPEN_HINT = "reskilled.ui.open_hint" => "Click to see its perks.";
    BACK = "reskilled.ui.back" => "Back";
    FOCUS = "reskilled.ui.focus" => "Focus";
    FOCUS_GIVES = "reskilled.ui.focus_gives" => "Gives XP for:";
    REWARD_KILL = "reskilled.ui.reward.kill" => "Kills: %s more XP";
    REWARD_BLOCK_XP = "reskilled.ui.reward.block_xp" => "Ores: %s more XP";
    REWARD_BREAK = "reskilled.ui.reward.break" => "Breaking blocks";
    REWARD_SMELT = "reskilled.ui.reward.smelt" => "Smelting";
    REWARD_TRAVEL = "reskilled.ui.reward.travel" => "Travelling";
    REWARD_TAME = "reskilled.ui.reward.tame" => "Taming animals";
    REWARD_BREED = "reskilled.ui.reward.breed" => "Breeding animals";
    FOCUS_ADD = "reskilled.ui.focus_add" => "Click to add it for %s XP levels.";
    FOCUS_ADD_FREE = "reskilled.ui.focus_add_free" => "Click to add it: your first is free.";
    FOCUS_ACTIVE = "reskilled.ui.focus_active" => "Unlocked: always on.";
    WAITING = "reskilled.ui.waiting" => "Waiting for the server.";
    MASTERED = "reskilled.ui.mastered" => "Mastered";
    SETTINGS = "reskilled.ui.settings" => "Settings";
    RESET = "reskilled.ui.reset" => "Reset to defaults";
    CLICK_TO_CHANGE = "reskilled.ui.click_to_change" => "Click to change.";
    SECTION_WARNING = "reskilled.ui.section.warning" => "Warning panel";
    SECTION_LOCK = "reskilled.ui.section.lock" => "Lock icon";
    SECTION_TOOLTIPS = "reskilled.ui.section.tooltips" => "Tooltips";
    SECTION_SOUNDS = "reskilled.ui.section.sounds" => "Sounds";
    OPT_PLACE = "reskilled.ui.opt.place" => "Position";
    OPT_TIME = "reskilled.ui.opt.time" => "Shown for";
    OPT_SHOW = "reskilled.ui.opt.show" => "Show";
    OPT_CORNER = "reskilled.ui.opt.corner" => "Corner";
    OPT_DIM = "reskilled.ui.opt.dim" => "Dim item";
    OPT_HIDE_MET = "reskilled.ui.opt.hide_met" => "Hide met";
    OPT_OWN = "reskilled.ui.opt.own" => "Your level";
    OPT_CLICKS = "reskilled.ui.opt.clicks" => "Clicks";
    OPT_CHIMES = "reskilled.ui.opt.chimes" => "Level-ups";
    OPT_REFUSAL = "reskilled.ui.opt.refusal" => "Refusals";
    VAL_ON = "reskilled.ui.val.on" => "On";
    VAL_OFF = "reskilled.ui.val.off" => "Off";
    VAL_ABOVE = "reskilled.ui.val.above" => "Above hotbar";
    VAL_TOP = "reskilled.ui.val.top" => "Top";
    VAL_SECONDS = "reskilled.ui.val.seconds" => "%s s";
    VAL_BOTTOM_LEFT = "reskilled.ui.val.bottom_left" => "Bottom left";
    VAL_TOP_LEFT = "reskilled.ui.val.top_left" => "Top left";
    VAL_TOP_RIGHT = "reskilled.ui.val.top_right" => "Top right";
    VAL_BOTTOM_RIGHT = "reskilled.ui.val.bottom_right" => "Bottom right";
    HELP_PLACE = "reskilled.ui.help.place" => "Where the panel shows when a lock stops you.";
    HELP_TIME = "reskilled.ui.help.time" => "How long the panel stays.";
    HELP_SHOW = "reskilled.ui.help.show" => "A padlock on items you cannot fully use yet.";
    HELP_CORNER = "reskilled.ui.help.corner" => "Which corner of the item the padlock sits in.";
    HELP_DIM = "reskilled.ui.help.dim" => "Darken locked items behind the padlock.";
    HELP_HIDE_MET = "reskilled.ui.help.hide_met" => "Leave out requirements you already meet.";
    HELP_OWN = "reskilled.ui.help.own" => "Show your level next to a requirement you miss.";
    HELP_CLICKS = "reskilled.ui.help.clicks" => "The tick of this screen's buttons.";
    HELP_CHIMES = "reskilled.ui.help.chimes" => "The sounds of raising a skill and unlocking a perk.";
    HELP_REFUSAL = "reskilled.ui.help.refusal" => "The low note when a lock stops you.";
    ACTION_USE = "reskilled.ui.action.use" => "Use";
    ACTION_ATTACK = "reskilled.ui.action.attack" => "Attack";
    ACTION_BREAK = "reskilled.ui.action.break" => "Break";
    ACTION_EQUIP = "reskilled.ui.action.equip" => "Wear";
    ACTION_OPEN = "reskilled.ui.action.open" => "Open";
    ACTION_PLACE = "reskilled.ui.action.place" => "Place";
    ACTION_MOUNT = "reskilled.ui.action.mount" => "Ride";
    ACTION_INTERACT = "reskilled.ui.action.interact" => "Interact";
}

pub fn say(message: &Message, args: &[Text]) -> Text {
    Text::translate_with_fallback(message.key, &english(message.key, message.english), args)
}

/// The English for `key` when the game has no line for it: `fallback`. A test that loaded the shipped English
/// file gets its line instead, as a player with the mod's language file would.
fn english<'a>(key: &str, fallback: &'a str) -> std::borrow::Cow<'a, str> {
    #[cfg(test)]
    if let Some(line) = shipped_english::line(key) {
        return line.into();
    }
    let _ = key;
    fallback.into()
}

/// The shipped English file, for tests that draw what a player with it sees.
#[cfg(test)]
pub(crate) mod shipped_english {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use crate::server::lock;

    static LINES: Mutex<Option<BTreeMap<String, String>>> = Mutex::new(None);

    pub fn load() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../content/assets/reskilled/lang/en_us.json"
        );
        let text = std::fs::read_to_string(path).expect("the shipped English file");
        *lock(&LINES) =
            Some(serde_json::from_str(&text).expect("the shipped English file is JSON"));
    }

    pub fn line(key: &str) -> Option<String> {
        lock(&LINES).as_ref()?.get(key).cloned()
    }

    pub fn reset() {
        *lock(&LINES) = None;
    }
}

pub fn number(n: u32) -> Text {
    Text::literal(n.to_string())
}

/// English for a name whose translation is missing: the id's path as a phrase (`deep_sea_diving`, "Deep sea
/// diving").
pub fn fallback_name(id: &str) -> String {
    let path = id
        .split_once(':')
        .map_or(id, |(_, path)| path)
        .replace(['_', '/'], " ");
    let mut chars = path.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// A data name (translation key) with the id's English as its fallback.
pub fn name(key: &str, id: &str) -> Text {
    Text::translate_with_fallback(key, &english(key, &fallback_name(id)), &[])
}

pub fn skill_name(defs: &Defs, skill: &str) -> Text {
    defs.skill(skill)
        .map_or_else(|| Text::literal(skill), |s| name(&s.name, &s.id))
}

pub fn perk_name(defs: &Defs, perk: &str) -> Text {
    defs.perks
        .get(perk)
        .map_or_else(|| Text::literal(perk), |p| name(&p.name, &p.id))
}

/// What a perk does, in words; empty when its translation is missing.
pub fn perk_description(perk: &PerkDef) -> Text {
    Text::translate_with_fallback(&perk.description, &english(&perk.description, ""), &[])
}

pub fn focus_name(defs: &Defs, focus: &str) -> Text {
    defs.focus
        .get(focus)
        .map_or_else(|| Text::literal(focus), |f| name(&f.name, &f.id))
}

/// A lock label (`reskilled.lock.diamond_pickaxes`) with its last part as the English fallback.
pub fn label_name(key: &str) -> Text {
    let last = key.rsplit('.').next().unwrap_or(key);
    Text::translate_with_fallback(key, &english(key, &fallback_name(last)), &[])
}

/// Why a request was refused, in words.
pub fn refusal(defs: &Defs, refusal: &Refusal, request: &Request) -> Text {
    match refusal {
        Refusal::UnknownSkill => say(&UNKNOWN_SKILL, &[]),
        Refusal::UnknownPerk => say(&UNKNOWN_PERK, &[]),
        Refusal::UnknownFocus => say(&UNKNOWN_FOCUS, &[]),
        Refusal::AtMax => say(&AT_MAX, &[]),
        Refusal::NotEnoughLevels { needed } => say(&NOT_ENOUGH_LEVELS, &[number(*needed)]),
        Refusal::NotEnoughPoints { needed } => say(&NOT_ENOUGH_POINTS, &[number(*needed)]),
        Refusal::BelowMinLevel { needed } => {
            let skill = match request {
                Request::BuyPerk { perk } => {
                    defs.perks.get(perk).map(|p| skill_name(defs, &p.skill))
                }
                _ => None,
            };
            say(
                &BELOW_MIN_LEVEL,
                &[skill.unwrap_or_else(Text::empty), number(*needed)],
            )
        }
        Refusal::MissingPerk { perk } => say(&MISSING_PERK, &[perk_name(defs, perk)]),
        Refusal::AlreadyOwned => say(&ALREADY_OWNED, &[]),
        Refusal::AlreadyFocused => say(&ALREADY_FOCUSED, &[]),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn english_fallbacks_match_en_us() {
        // Given the mod's English language file
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../content/assets/reskilled/lang/en_us.json"
        );
        let english: BTreeMap<String, String> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();

        // When every server message is looked up in it
        // Then each key is there with the same English
        assert!(!ALL.is_empty());
        for message in ALL {
            assert_eq!(
                english.get(message.key).map(String::as_str),
                Some(message.english),
                "{}",
                message.key
            );
        }
    }

    #[test]
    fn fallback_names_come_from_the_id_path() {
        // Given ids from this mod and from a pack
        // When their fallback names are read
        let names = ["reskilled:mining", "pack:deep_sea_diving", "weird"].map(fallback_name);

        // Then the path is turned into a capitalised phrase
        assert_eq!(
            names,
            [
                "Mining".to_string(),
                "Deep sea diving".to_string(),
                "Weird".to_string()
            ]
        );
    }
}
