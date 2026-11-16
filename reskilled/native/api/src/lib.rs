//! The trait mod: the launcher links this crate into the game and calls `hooks()`. Each method of `Hooks` is one of
//! Horizon's hook points (Horizon docs/native-mods.md, "Trait mods"); each hands its work to the module that owns it.
use horizon_client::mod_api::{
    AddServerReloadListenersEvent, AttackEntityEvent, BabyEntitySpawnEvent, BlockExpEvent,
    ClientDisconnectingEvent, ContainerOpenedEvent, DestroyBlockEvent, EnchantItemEvent,
    EntityMountEvent, EntityTameEvent, ExplosionDetonateEvent, Flow, FurnaceExtractEvent,
    GuiRenderPostEvent, Hooks, Host, InteractEntityEvent, InventoryClickEvent,
    ItemDecorationsEvent, ItemStack, KeyEvent, LevelHost, LivingDamagePreEvent,
    LivingEntityUseItemStartEvent, LivingExperienceDropEvent, LivingIncomingDamageEvent,
    LivingIncomingDamageV2Event, LootDropsEvent, ModifyDefaultComponentsEvent, NetFromClientEvent,
    NetFromServerEvent, OnEquipEvent, PerformCommandEvent, PerformConsoleCommandEvent,
    PlaceBlockEvent, PlayerBedEnterEvent, PlayerBedLeaveEvent, PlayerCloneEvent,
    PlayerLoggedInEvent, PlayerLoggedOutEvent, PlayerSpawnPointSetEvent, PlayerSwapHandsEvent,
    PlayerTickEvent, PlayerXpChangeEvent, PotentialSpawnsEvent, PrepareAnvilEvent,
    RegisterCommandsEvent, ScreenMouseButtonPressedPreEvent, ScreenMouseScrolledPreEvent,
    ScreenRenderPostEvent, ServerDataLoadedEvent, ServerReloadListenerApplyEvent,
    ServerStoppedEvent, TickEvent, TimeSkipEvent, TooltipColorEvent, TooltipEvent, UseItemEvent,
    UseItemOnEvent, WorldSubmitEvent,
};
use skills_core::defs::Action;

/// A player's eye height asleep is 0.2 (1.62 standing, 0.4 swimming): below this they lie in bed.
const ASLEEP_EYE_HEIGHT: f32 = 0.3;

mod client;
pub mod integration;
pub use integration::{has_perk, may_equip, may_use_ability, may_wear, skill_level};
mod server;
#[cfg(test)]
mod test_host;
mod text;

use server::commands::{Source, run_command};
use server::enforce::{self, Actor, acting};
use server::queue::{Job, queue};

/// The mod's id: its namespace, and the id of its screen.
pub(crate) const MOD_ID: &str = "reskilled";
/// Server to client: definition chunks.
pub(crate) const DEFS_CHANNEL: &str = "reskilled:defs";
/// Server to client: every other message (`ToClient`).
pub(crate) const MSG_CHANNEL: &str = "reskilled:msg";
/// Client to server: requests (`ToServer`).
pub(crate) const ACT_CHANNEL: &str = "reskilled:act";
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

    fn player_logged_out(&self, host: &mut dyn Host, event: &mut PlayerLoggedOutEvent<'_>) {
        server::enforce::on_logged_out(host, event.player);
        server::state::logged_out(event.player);
    }

    /// The new player is not placed yet, so the death penalty waits for the tick. Either way (a death or the End's
    /// exit) the new player's modifiers are resynced in full.
    fn player_clone(&self, _host: &mut dyn Host, event: &mut PlayerCloneEvent<'_>) {
        server::effects::mark_full(event.player);
        if event.was_death {
            server::mechanics::respawned(event.player);
        }
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

    fn attack_entity(&self, host: &mut dyn Host, event: &mut AttackEntityEvent<'_>) -> Flow {
        let main_hand = host.held().map(|(item, _, _)| item);
        let flow = acted(host, |host, actor| {
            enforce::on_attack(host, actor, main_hand.as_deref())
        });
        if flow == Flow::Continue
            && let Some(player) = host.acting_player()
            && server::focus::rewarded(player.game_mode)
        {
            server::earning::on_attack(player.uuid, event.kind);
        }
        flow
    }

    fn enchant_item(&self, _host: &mut dyn Host, event: &mut EnchantItemEvent<'_>) {
        server::earning::on_enchant(event.player);
    }

    fn destroy_block(&self, host: &mut dyn Host, event: &mut DestroyBlockEvent) -> Flow {
        let main_hand = host.held().map(|(item, _, _)| item);
        let flow = acted(host, |host, actor| {
            enforce::on_destroy(host, actor, main_hand.as_deref())
        });
        if flow == Flow::Continue
            && let Some(player) = host.acting_player()
        {
            let rewarded = server::focus::rewarded(player.game_mode);
            let placed =
                server::focus::on_break(host, player.uuid, rewarded, &player.dimension, event.pos);
            server::mechanics::on_break(host, player.uuid, &player.dimension, event.pos, placed);
            if rewarded && !placed {
                server::earning::on_break(host, player.uuid, main_hand.as_deref());
            }
        }
        flow
    }

    fn place_block(&self, host: &mut dyn Host, event: &mut PlaceBlockEvent<'_>) {
        if let Some(player) = host.acting_player() {
            server::focus::on_place(&player.dimension, event.pos);
            server::mechanics::on_place(host, &player.dimension, event.pos);
            if server::focus::rewarded(player.game_mode) {
                server::earning::on_place(player.uuid, &player.dimension, event.pos);
            }
        }
    }

    fn block_exp(&self, _level: &dyn LevelHost, event: &mut BlockExpEvent<'_>) {
        let pos = [event.pos.x, event.pos.y, event.pos.z];
        server::focus::block_xp(pos, event.experience);
    }

    fn living_experience_drop(
        &self,
        host: &mut dyn Host,
        event: &mut LivingExperienceDropEvent<'_>,
    ) {
        server::mechanics::on_kill(host, event.entity, event.kind);
        server::focus::kill_xp(event.entity, event.xp);
    }

    fn furnace_extract(&self, host: &mut dyn Host, event: &mut FurnaceExtractEvent<'_>) {
        server::focus::on_smelt(host, event.player, event.count);
    }

    fn entity_tame(&self, host: &mut dyn Host, event: &mut EntityTameEvent<'_>) {
        if let Some(owner) = event.owner {
            server::focus::on_tame(host, owner);
        }
    }

    fn baby_entity_spawn(&self, host: &mut dyn Host, event: &mut BabyEntitySpawnEvent<'_>) -> Flow {
        if event.player != 0 {
            server::focus::on_breed(host, event.player);
        }
        Flow::Continue
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
        server::focus::hurt(event.entity, event.causing);
        server::earning::hurt(event.entity, event.causing, event.damage_type);
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
        if let (Some(player), Some((pos, _))) = (context.player, context.block) {
            server::mechanics::on_drops(host, player, context.dimension, pos, event.drops);
            if server::mechanics::drop_bonus_allowed(player, context.dimension, pos) {
                server::effects::bonus_drops(
                    host,
                    player,
                    context.dimension,
                    context.table,
                    event.drops,
                );
            }
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

    fn inventory_click(&self, host: &mut dyn Host, event: &mut InventoryClickEvent<'_>) -> Flow {
        match enforce::listed(host, |p| p.uuid == event.player) {
            Some(actor) => enforce::on_inventory_click(
                host,
                &actor,
                event.window,
                event.slot,
                event.button,
                event.mode,
            ),
            None => Flow::Continue,
        }
    }

    fn player_swap_hands(&self, host: &mut dyn Host, event: &mut PlayerSwapHandsEvent<'_>) -> Flow {
        match enforce::listed(host, |p| p.uuid == event.player) {
            Some(actor) => enforce::on_swap_hands(host, &actor),
            None => Flow::Continue,
        }
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

    /// Once a tick in each game: the window's runs the client's part, the server's own (a single-player world's
    /// server thread, a dedicated server) the server's. The server's work is done here, not in `server_tick`:
    /// Horizon runs commands and reaches every player only from this tick.
    /// Each player's own tick, where Horizon reaches them: their waiting requests and the focus XP they are owed.
    fn player_tick(&self, host: &mut dyn Host, _event: &mut PlayerTickEvent) {
        if let Some(player) = host.acting_player() {
            server::actions::run_pending_for(host, player.uuid);
            server::rules::run_for(host, player.uuid);
            server::earning::run_for(host, player.uuid);
            server::focus::pay(host, player.uuid);
            server::mechanics::run_for(
                host,
                player.uuid,
                &player.dimension,
                player.position,
                player.health,
                usize::from(player.selected_slot),
                player.eye_height < ASLEEP_EYE_HEIGHT,
            );
        }
    }

    fn living_incoming_damage_v2(
        &self,
        _host: &mut dyn Host,
        event: &mut LivingIncomingDamageV2Event<'_>,
    ) -> Flow {
        server::mechanics::on_weapon_hit(event.causing, event.weapon, &mut event.amount);
        Flow::Continue
    }

    fn player_spawn_point_set(
        &self,
        host: &mut dyn Host,
        event: &mut PlayerSpawnPointSetEvent<'_>,
    ) -> Flow {
        server::homes::set(host, event.player, event.dimension, event.pos, event.cause);
        Flow::Continue
    }

    fn world_submit(&self, host: &dyn Host, event: &mut WorldSubmitEvent<'_>) {
        client::areas::draw(host, event.draw);
    }

    fn time_skip(&self, _host: &mut dyn Host, event: &mut TimeSkipEvent<'_>) -> Flow {
        server::mechanics::on_time_skip(event.cause);
        Flow::Continue
    }

    fn player_bed_enter(&self, _host: &mut dyn Host, event: &mut PlayerBedEnterEvent<'_>) -> Flow {
        server::mechanics::on_bed_enter(event.player);
        Flow::Continue
    }

    fn player_bed_leave(&self, _host: &mut dyn Host, event: &mut PlayerBedLeaveEvent<'_>) {
        server::mechanics::on_bed_leave(event.player);
    }

    fn explosion_detonate(&self, host: &mut dyn Host, event: &mut ExplosionDetonateEvent<'_>) {
        server::homes::ensure_loaded(host);
        server::mechanics::on_explosion(event.dimension, event.blocks);
    }

    fn potential_spawns(&self, _host: &dyn Host, event: &mut PotentialSpawnsEvent<'_>) -> Flow {
        if server::mechanics::may_spawn(event.dimension, event.category, event.pos) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    fn living_damage_pre(&self, event: &mut LivingDamagePreEvent<'_>) {
        server::mechanics::on_damage(
            event.entity,
            event.kind,
            event.damage_type,
            &mut event.amount,
        );
    }

    fn server_stopped(&self, _host: &mut dyn Host, _event: &mut ServerStoppedEvent) {
        server::forget_world();
    }

    fn tick(&self, host: &mut dyn Host, _event: &mut TickEvent) {
        if host.window() {
            client::cache::on_tick(host);
            client::anvil::on_tick(host);
        } else {
            server_tick(host);
        }
    }

    /// The window's own player never starts a use the cache knows is locked, so no shield is raised and no bow
    /// drawn for show; the server refuses the use itself in `use_item` and says why.
    fn living_entity_use_item_start(
        &self,
        host: &mut dyn Host,
        event: &mut LivingEntityUseItemStartEvent<'_>,
    ) -> Flow {
        let own = event.entity.is_none() && host.window();
        if own
            && client::cache::with(|cache| cache.lacks(event.stack, |action| action == Action::Use))
        {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    fn item_decorations(&self, host: &dyn Host, event: &mut ItemDecorationsEvent<'_>) {
        client::badge::draw_lock_badge(host, event.batch, event.stack, event.x, event.y);
    }

    fn tooltip(&self, _host: &dyn Host, event: &mut TooltipEvent<'_>) {
        event
            .lines
            .extend(client::tooltip::requirement_lines(event.stack));
    }

    fn client_disconnecting(&self, _host: &dyn Host, _event: &mut ClientDisconnectingEvent) {
        client::cache::on_disconnect();
    }

    fn tooltip_color(&self, _host: &dyn Host, event: &mut TooltipColorEvent<'_>) {
        let locked = event.stack.and_then(client::tooltip::locked_border);
        if let Some([top, bottom]) = locked {
            event.border_top = top;
            event.border_bottom = bottom;
        }
    }

    fn gui_render_post(&self, host: &dyn Host, event: &mut GuiRenderPostEvent<'_>) {
        client::warning::draw(host, event.batch, event.gui_hidden);
    }

    fn screen_render_post(&self, host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>) {
        let size = [event.batch.width, event.batch.height];
        client::screen::draw(host, event.batch, size, event.screen.mouse);
        let screen = event.screen;
        client::anvil::note_screen(screen.menu);
        client::anvil::draw(host, event.batch, screen.menu, screen.rect, screen.mouse);
        client::strip::draw_beside(
            host,
            event.batch,
            screen.id,
            screen.menu,
            screen.rect,
            screen.mouse,
        );
    }

    fn screen_mouse_button_pressed_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenMouseButtonPressedPreEvent<'_>,
    ) -> Flow {
        let screen = event.screen;
        if client::anvil::click(host, screen.menu, screen.rect, screen.mouse, event.button)
            == Flow::Cancel
        {
            return Flow::Cancel;
        }
        let Some(size) = host.client_state().map(|state| state.gui_size) else {
            return Flow::Continue;
        };
        client::screen::click(host, size, screen.mouse, event.button)
    }

    fn screen_mouse_scrolled_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenMouseScrolledPreEvent<'_>,
    ) -> Flow {
        client::screen::scroll(host, event.scroll[1])
    }

    fn key(&self, host: &mut dyn Host, event: &mut KeyEvent<'_>) {
        client::keys::on_key(host, event.key, event.pressed);
    }
}

/// The server's part of a tick: tell clients their status, count travel, and run the queued jobs.
fn server_tick(host: &mut dyn Host) {
    server::state::watch_players(host);
    server::focus::watch(host);
    server::queue::run(host, server::queue::take_queued());
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
