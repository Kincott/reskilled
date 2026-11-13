//! The trait mod: the launcher links this crate into the game and calls `hooks()`. Each method of `Hooks` is one of
//! Horizon's hook points (Horizon docs/native-mods.md, "Trait mods"); each hands its work to the module that owns it.
use horizon_client::mod_api::{
    AddServerReloadListenersEvent, AttackEntityEvent, ClientDisconnectingEvent,
    ContainerOpenedEvent, DestroyBlockEvent, EntityMountEvent, Flow, Hooks, Host,
    InteractEntityEvent, ItemStack, LivingIncomingDamageEvent, LootDropsEvent,
    ModifyDefaultComponentsEvent, NetFromClientEvent, NetFromServerEvent, OnEquipEvent,
    PerformCommandEvent, PerformConsoleCommandEvent, PlayerCloneEvent, PlayerLoggedInEvent,
    PlayerLoggedOutEvent, PlayerXpChangeEvent, PrepareAnvilEvent, RegisterCommandsEvent,
    ServerDataLoadedEvent, ServerReloadListenerApplyEvent, ServerTickEvent, ServerTickPhase,
    TickEvent, TooltipEvent, UseItemEvent, UseItemOnEvent,
};

mod client;
mod server;
#[cfg(test)]
mod test_host;
mod text;

use server::commands::{Source, run_command};
use server::enforce::{self, Actor, acting};
use server::queue::{Job, queue};

/// The mod's id: its namespace, and the id of its screen.
pub(crate) const MOD_ID: &str = "skills_and_perks";
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

    /// The new player is not placed yet, so the death penalty waits for the tick. Either way (a death or the End's
    /// exit) the new player's modifiers are resynced in full.
    fn player_clone(&self, _host: &mut dyn Host, event: &mut PlayerCloneEvent<'_>) {
        server::effects::mark_full(event.player);
        queue(if event.was_death {
            Job::ApplyDeath(event.player)
        } else {
            Job::SyncAttributes(event.player)
        });
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

    fn use_item(&self, host: &mut dyn Host, event: &mut UseItemEvent<'_>) -> Flow {
        acted(host, |host, actor| {
            enforce::on_use_item(host, actor, &event.stack.item)
        })
    }

    fn use_item_on(&self, host: &mut dyn Host, event: &mut UseItemOnEvent<'_>) -> Flow {
        acted(host, |host, actor| {
            enforce::on_use_item_on(host, actor, &event.stack.item)
        })
    }

    fn attack_entity(&self, host: &mut dyn Host, _event: &mut AttackEntityEvent<'_>) -> Flow {
        let main_hand = host.held().map(|(item, _, _)| item);
        acted(host, |host, actor| {
            enforce::on_attack(host, actor, main_hand.as_deref())
        })
    }

    fn destroy_block(&self, host: &mut dyn Host, _event: &mut DestroyBlockEvent) -> Flow {
        let main_hand = host.held().map(|(item, _, _)| item);
        acted(host, |host, actor| {
            enforce::on_destroy(host, actor, main_hand.as_deref())
        })
    }

    fn entity_mount(&self, host: &mut dyn Host, event: &mut EntityMountEvent<'_>) -> Flow {
        let Some(actor) = enforce::rider(host, event.entity, event.kind) else {
            return Flow::Continue;
        };
        let flow = enforce::on_mount(host, &actor, event.vehicle_kind, event.mounting);
        if flow == Flow::Continue {
            queue(Job::Mount {
                player: actor.player,
                vehicle: event.vehicle,
                mounting: event.mounting,
            });
        }
        flow
    }

    fn living_incoming_damage(&self, event: &mut LivingIncomingDamageEvent<'_>) -> Flow {
        server::effects::on_damage(
            event.entity,
            event.damage_type,
            event.causing,
            &mut event.amount,
        );
        Flow::Continue
    }

    fn player_xp_change(&self, _host: &mut dyn Host, event: &mut PlayerXpChangeEvent<'_>) -> Flow {
        server::effects::on_xp(event.player, &mut event.amount);
        Flow::Continue
    }

    fn prepare_anvil(
        &self,
        _host: &mut dyn Host,
        event: &mut PrepareAnvilEvent<'_>,
    ) -> Option<ItemStack> {
        server::effects::on_anvil(event.player, &mut event.cost);
        None
    }

    fn loot_drops(&self, host: &mut dyn Host, event: &mut LootDropsEvent<'_>) {
        let context = event.context;
        if let (Some(player), Some(_)) = (context.player, context.block) {
            server::effects::bonus_drops(
                host,
                player,
                context.dimension,
                context.table,
                event.drops,
            );
        }
    }

    fn interact_entity(&self, host: &mut dyn Host, event: &mut InteractEntityEvent<'_>) -> Flow {
        let held = event.stack.map(|stack| stack.item.clone());
        acted(host, |host, actor| {
            enforce::on_interact(host, actor, event.kind, held.as_deref())
        })
    }

    fn container_opened(&self, host: &mut dyn Host, event: &mut ContainerOpenedEvent<'_>) -> Flow {
        let block = event.pos.map(|pos| host.block(pos));
        acted(host, |host, actor| {
            enforce::on_container(host, actor, block.as_deref())
        })
    }

    fn on_equip(&self, host: &mut dyn Host, event: &mut OnEquipEvent<'_>) {
        if let Some(actor) = acting(host) {
            enforce::on_equip(
                host,
                &actor,
                event.slot,
                event.new.map(|stack| &*stack.item),
            );
        }
    }

    fn net_from_server(&self, host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        client::cache::on_message(host, event.channel, event.data);
    }

    fn tick(&self, host: &mut dyn Host, _event: &mut TickEvent) {
        client::cache::on_tick(host);
    }

    fn tooltip(&self, _host: &dyn Host, event: &mut TooltipEvent<'_>) {
        event
            .lines
            .extend(client::tooltip::requirement_lines(&event.stack.item));
    }

    fn client_disconnecting(&self, _host: &dyn Host, _event: &mut ClientDisconnectingEvent) {
        client::cache::on_disconnect();
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

/// Runs an enforcement check for the acting player; with no acting player there is nothing to check.
fn acted(host: &mut dyn Host, decide: impl FnOnce(&mut dyn Host, &Actor) -> Flow) -> Flow {
    match acting(host) {
        Some(actor) => decide(host, &actor),
        None => Flow::Continue,
    }
}
