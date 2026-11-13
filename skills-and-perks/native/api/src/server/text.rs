//! Every line the server shows players, as a translation key with its English, so a player without the mod's
//! language file (a vanilla client on a dedicated server, the console) still reads English.

use horizon_client::mod_api::Text;
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
    NEEDS = "skills_and_perks.msg.needs" => "Needs %s %s";
    UNKNOWN_SKILL = "skills_and_perks.msg.unknown_skill" => "There is no such skill.";
    UNKNOWN_PERK = "skills_and_perks.msg.unknown_perk" => "There is no such perk.";
    UNKNOWN_FOCUS = "skills_and_perks.msg.unknown_focus" => "There is no such focus.";
    AT_MAX = "skills_and_perks.msg.at_max" => "That skill is already at its highest level.";
    NOT_ENOUGH_LEVELS = "skills_and_perks.msg.not_enough_levels" => "Needs %s XP levels.";
    NOT_ENOUGH_POINTS = "skills_and_perks.msg.not_enough_points" => "Needs %s perk points.";
    BELOW_MIN_LEVEL = "skills_and_perks.msg.below_min_level" => "Needs %s %s first.";
    MISSING_PERK = "skills_and_perks.msg.missing_perk" => "Get %s first.";
    ALREADY_OWNED = "skills_and_perks.msg.already_owned" => "You already have that perk.";
    ALREADY_FOCUSED = "skills_and_perks.msg.already_focused" => "That is already your focus.";
    UNAVAILABLE = "skills_and_perks.msg.unavailable" => "That cannot be done right now.";
    SUMMARY = "skills_and_perks.cmd.summary" => "XP levels: %s. Focus: %s.";
    NO_FOCUS = "skills_and_perks.cmd.no_focus" => "none";
    SKILL_LINE = "skills_and_perks.cmd.skill_line" => "%s %s/%s: raise for %s levels, %s perk points.";
    SKILL_LINE_MAX = "skills_and_perks.cmd.skill_line_max" => "%s %s/%s: at the top, %s perk points.";
    RAISED = "skills_and_perks.cmd.raised" => "%s is now level %s.";
    PERK_BOUGHT = "skills_and_perks.cmd.perk_bought" => "You now have %s.";
    FOCUS_SET = "skills_and_perks.cmd.focus_set" => "Your focus is now %s.";
    SET_LEVEL = "skills_and_perks.cmd.set" => "Set %s's %s to level %s.";
    SET_POINTS = "skills_and_perks.cmd.points" => "Set %s's bonus points in %s to %s.";
    RESET_ONE = "skills_and_perks.cmd.reset_one" => "Reset %s's %s.";
    RESET_ALL = "skills_and_perks.cmd.reset_all" => "Reset all of %s's skills.";
    NO_PLAYER = "skills_and_perks.cmd.no_player" => "No player named %s is online.";
    PLAYERS_ONLY = "skills_and_perks.cmd.players_only" => "Only players can do that.";
    NOT_ALLOWED = "skills_and_perks.cmd.not_allowed" => "You may not do that.";
    USAGE = "skills_and_perks.cmd.usage" => "Usage: /skills [raise <skill> | perk <perk> | focus <area> | open]";
}

pub fn say(message: &Message, args: &[Text]) -> Text {
    Text::translate_with_fallback(message.key, message.english, args)
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
    Text::translate_with_fallback(key, &fallback_name(id), &[])
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

pub fn focus_name(defs: &Defs, focus: &str) -> Text {
    defs.focus
        .get(focus)
        .map_or_else(|| Text::literal(focus), |f| name(&f.name, &f.id))
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
            "/../../content/assets/skills_and_perks/lang/en_us.json"
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
        let names = ["skills_and_perks:mining", "pack:deep_sea_diving", "weird"].map(fallback_name);

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
