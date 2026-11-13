//! The trait mod: the launcher links this crate into the game and calls `hooks()`. Each method of `Hooks` is one of
//! Horizon's hook points (Horizon docs/native-mods.md, "Trait mods").
use horizon_client::mod_api::{Hooks, Host, PlayerLoggedInEvent, Text};

/// Smoke-test greeting until the login hook gets real work (Task 8).
const LOADED_GREETING: &str = "Skills & Perks loaded";

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Mod)
}

struct Mod;

impl Hooks for Mod {
    /// A player joined the server: tell them the mod is running.
    fn player_logged_in(&self, host: &mut dyn Host, event: &mut PlayerLoggedInEvent<'_>) {
        host.tell(event.player, Text::literal(LOADED_GREETING));
    }
}
