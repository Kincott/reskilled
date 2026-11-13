//! The trait mod: the launcher links this crate into the game and calls `hooks()`. Each method of `Hooks` is one of
//! Horizon's hook points (Horizon docs/native-mods.md, "Trait mods"); each hands its work to the module that owns it.
use horizon_client::mod_api::{
    AddServerReloadListenersEvent, Flow, Hooks, Host, ModifyDefaultComponentsEvent,
    NetFromClientEvent, PerformCommandEvent, PerformConsoleCommandEvent, PlayerCloneEvent,
    PlayerLoggedInEvent, PlayerLoggedOutEvent, RegisterCommandsEvent, ServerDataLoadedEvent,
    ServerReloadListenerApplyEvent, ServerTickEvent, ServerTickPhase,
};

mod server;

use server::commands::{Source, run_command};
use server::queue::{Job, queue};

/// Server to client: definition chunks.
pub(crate) const DEFS_CHANNEL: &str = "skills_and_perks:defs";
/// Server to client: every other message (`ToClient`).
pub(crate) const MSG_CHANNEL: &str = "skills_and_perks:msg";
/// Client to server: requests (`ToServer`).
pub(crate) const ACT_CHANNEL: &str = "skills_and_perks:act";
/// Horizon's limit for one mod message, in bytes.
pub(crate) const MAX_MESSAGE_BYTES: usize = 30_000;

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
        server::state::logged_in(event.player, event.name);
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

    fn net_from_client(&self, host: &mut dyn Host, event: &mut NetFromClientEvent<'_>) {
        if let Some(message) = server::actions::decode(event.channel, event.data) {
            server::actions::handle(host, event.player, message);
        }
    }

    fn register_commands(&self, _host: &mut dyn Host, event: &mut RegisterCommandsEvent<'_>) {
        server::commands::register(event.commands);
    }

    fn perform_command(&self, host: &mut dyn Host, event: &mut PerformCommandEvent<'_>) -> Flow {
        took(run_command(
            host,
            &Source::player(event.player),
            event.command,
        ))
    }

    fn perform_console_command(
        &self,
        host: &mut dyn Host,
        event: &mut PerformConsoleCommandEvent<'_>,
    ) -> Flow {
        took(run_command(host, &Source::CONSOLE, event.command))
    }

    fn server_tick(&self, host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {
        if matches!(event.phase, ServerTickPhase::ServerTickEnd { .. }) {
            server::queue::run(host, server::queue::take_queued());
        }
    }
}

/// `Cancel` tells the game this mod ran the command, so no other mod and not vanilla answers it.
fn took(ran: bool) -> Flow {
    if ran { Flow::Cancel } else { Flow::Continue }
}
