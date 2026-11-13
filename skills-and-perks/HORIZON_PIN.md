# Horizon pin

Skills & Perks builds on one exact Horizon commit. Every file that names a commit uses these values.

| What | Value |
|---|---|
| Horizon commit | `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` |
| Release | Nightly `nightly-2026-11-13-9f42064`, kept for good as tag `nightly-source/2026-11-13-9f42064` |
| Minecraft version | 26.3 |
| `NATIVE_HOOKS_API` | 415 (`crates/horizon-client/src/game/native_hooks.rs`) |
| horizon-mixins commit | `5a64af938ec86b00d53d16a408aabc0038fb3e57` (`launcher/src/native_mods.rs`, `MIXINS_REV`) |
| Rust | 1.99.1, the toolchain the launcher compiles native mods with (`docs/native-mods.md`, "What the launcher does") |
| Pinned crate versions | serde 1.0.229, serde_json 1.0.151, thiserror 2.0.21, log 0.4.34 (Horizon's `Cargo.lock`) |

Why a Nightly: the newest stable release at the time (v0.2.97, 2026-10-28) has no trait mod API and no
`native-hooks.json`. Native mods pin a Nightly's commit (`docs/native-mods.md`, "Pinned to a Nightly").

## Capability table

Checked on 2026-11-13 against the `Hooks` and `Host` traits Cargo generated for this commit
(`mod_api_generated.rs` and `mod_api_generated_host.rs` in `horizon-client`'s build output), not only against
the docs: the docs' list of hooks "not in the trait yet" is older than the trait at this commit.

### Hooks (trait methods)

| Hook | Side | At this commit |
|---|---|---|
| `use_item`, `use_item_on` | server | present, return `Flow` |
| `attack_entity`, `destroy_block`, `entity_mount`, `interact_entity`, `container_opened` | server | present, return `Flow` |
| `player_command` | server | present, returns `Flow` |
| `perform_command` | server | present, returns `Flow` |
| `living_equipment_change` | server | present |
| `player_logged_in`, `player_clone` | server | present |
| `player_respawned` | server | present |
| `player_xp_change` | server | present, returns `Flow` |
| `loot_drops` | server | present |
| `living_incoming_damage_v2` | server | present, returns `Flow` |
| `prepare_anvil` | server | present, returns `Option<ItemStack>` |
| `register_commands` | server | present |
| `net_from_client` | server | present |
| `add_server_reload_listeners`, `server_reload_listener_apply` | server | present |
| `server_data_loaded` | server | present |
| `server_tick` | server | present |
| `tooltip` | client | present, takes `&dyn Host` |
| `key`, `input_key` | client | present |
| `net_from_server` | client | present |
| `screen_render_post` | client | present, takes `&dyn Host` |
| `screen_click` | client | present, takes `&dyn Host`, returns `Option<i32>` |
| `screen_key_pressed_pre`, `screen_mouse_scrolled_pre` | client | present, return `Flow` |
| `tick` | client | present |
| `break_speed` | both | present, takes `&dyn Host` |

### Host helpers

| Helper | At this commit |
|---|---|
| `give_levels(uuid, levels) -> bool` | present |
| `run_command(line) -> bool` | present |
| `living_equipment(living, slot)`, `living_set_equipment(living, slot, stack)` | present |
| `give_stack(uuid, stack) -> Option<ItemStack>` (what did not fit) | present |
| `action_bar(uuid, Text)`, `tell(uuid, Text)` | present |
| `play_local_sound(sound, at, volume, pitch, delay)` | present |
| `net_send_to_player(player, channel, data)`, `net_send_to_server(channel, data)` | present |
| `server_item_in_tag(&ItemStack, tag)`, `block_name_in_tag(block, tag)` | present |
| `component(&ItemStack, id) -> Option<&Tag>` | present |
| `fill`, `quad`, `text`, `text_width`, `item_model` | present |
| `mod_screen_open(mod_id)`, `mod_screen()` | present |
| `attachments(holder)` | present |
| `server_players()`, `acting_player()` (`NativeServerPlayer` with `xp_level`, `game_mode`, `permission`) | present |
| `attribute`, `set_attribute_base`, `set_attributes` | present |

### Notes for later tasks

- The spec's fallbacks for `player_command`, `player_respawned` and `server_data_loaded` are not needed at this
  commit, and `break_speed` and the attribute helpers exist. The plan keeps its approach (vanilla `/attribute`
  through `run_command`, and the `minecraft:block_break_speed` modifier); the spec allows either.
- `NativeServerPlayer` is `#[non_exhaustive]` with no public constructor, so a test double cannot return one.
  Hook tests read the XP level and game mode through code that a double can bypass (Horizon's docs, "Testing a
  trait mod": test the logic beneath the hook).
- `Text::translate_with_fallback(key, fallback, args)` lets a player without the mod's language file read English.
