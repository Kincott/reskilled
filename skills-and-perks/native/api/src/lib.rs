//! The trait mod: the launcher links this crate into the game and calls `hooks()`. Each method of `Hooks` is one of
//! Horizon's hook points (Horizon docs/native-mods.md, "Trait mods"); each hands its work to the module that owns it.
use horizon_client::mod_api::{
    AddServerReloadListenersEvent, Hooks, Host, ModifyDefaultComponentsEvent, PlayerCloneEvent,
    PlayerLoggedInEvent, PlayerLoggedOutEvent, ServerDataLoadedEvent,
    ServerReloadListenerApplyEvent, ServerTickEvent, ServerTickPhase,
};

mod server;

use server::queue::{Job, queue};

/// Server to client: definition chunks.
pub(crate) const DEFS_CHANNEL: &str = "skills_and_perks:defs";

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Mod)
}

pub(crate) struct Mod;

impl Hooks for Mod {
    fn modify_default_components(&self, event: &mut ModifyDefaultComponentsEvent<'_>) {
        server::data::note_repair_material(event.item, event.components);
    }

    fn add_server_reload_listeners(
        &self,
        _host: &mut dyn Host,
        event: &mut AddServerReloadListenersEvent<'_>,
    ) {
        server::data::add_listeners(event.listeners);
    }

    fn server_reload_listener_apply(
        &self,
        _host: &mut dyn Host,
        event: &mut ServerReloadListenerApplyEvent<'_>,
    ) {
        server::data::collect(event.listener, event.files);
    }

    fn server_data_loaded(&self, _host: &mut dyn Host, _event: &mut ServerDataLoadedEvent) {
        server::data::load();
    }

    fn player_logged_in(&self, _host: &mut dyn Host, event: &mut PlayerLoggedInEvent<'_>) {
        server::state::logged_in(event.player);
    }

    fn player_logged_out(&self, _host: &mut dyn Host, event: &mut PlayerLoggedOutEvent<'_>) {
        server::state::logged_out(event.player);
    }

    /// The new player is not placed yet, so the death penalty waits for the tick.
    fn player_clone(&self, _host: &mut dyn Host, event: &mut PlayerCloneEvent<'_>) {
        if event.was_death {
            queue(Job::ApplyDeath(event.player));
        }
    }

    fn server_tick(&self, host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {
        if matches!(event.phase, ServerTickPhase::ServerTickEnd { .. }) {
            server::queue::run(host, server::queue::take_queued());
        }
    }
}
