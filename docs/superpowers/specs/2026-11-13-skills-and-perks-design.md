# Reskilled: design

Renamed on 2026-11-14 at the user's request: the mod is **Reskilled** (id `reskilled`), formerly "Skills & Perks"
(`skills_and_perks`). The user first asked for the name "Reskillable"; Claude declined, since that is another
mod's name, and the user picked Reskilled. Older entries below keep the old name where they quote history.

- Date: 2026-11-13
- Status: approved by the user on 2026-11-13 and built; later changes are recorded in 7.11
- Working name: Reskilled (mod id `reskilled`)
- Platform: Minecraft Horizon (Rust rewrite of Minecraft Java Edition 26.3)

## 1. Intent

The user wants RLCraft-style hardcore progression on Minecraft Horizon, built as a family of standalone, original
mods. A pack maker (RLCraft's creator, the user, anyone) can combine and tune them into a pack. The family is built in
four layers: progression, survival, combat and gear, then creatures and places.

This document covers the first mod, the progression layer: **Reskilled**. In RLCraft, Reskillable and
Level Up! Reloaded fill this role together. Reskilled merges that role into one mod, redesigned for Minecraft
26.3, with its own names, art, text and screens.

Why this mod first: the user asked for it, Horizon supports everything it needs today, and the later layers plug into
its gear locks.

## 2. Platform facts this design relies on

Checked against `horizon-rust/horizon` at commit `298c01b` (2026-11-13), mostly `docs/native-mods.md`,
`docs/mods.md` and `native-hooks.json`. Re-check each at the commit the mod pins.

- Horizon runs Java Edition 26.3, the newest stable release (26.4 is in snapshots). The mods follow Horizon when it
  moves to a newer version.
- Horizon has no Java. A mod is content (`assets/`, `data/`) plus native Rust code, and both can ship as one `.hmod`.
  The launcher compiles native code into the game. A mod pins a Horizon commit and declares the hook API number it
  needs.
- Native code can be a **trait mod**: a Rust package exporting `hooks() -> Box<dyn horizon_client::mod_api::Hooks>`
  and calling helpers on `Host`. It may depend only on crates in Horizon's `Cargo.lock`, plus crates inside the
  bundle.
- Hooks this design needs, all present in `native-hooks.json`:
  - Server, cancellable: `use_item`, `use_item_on`, `attack_entity`, `destroy_block`, `entity_mount`,
    `interact_entity`, `container_opened`, `player_command`.
  - Server, other: `living_equipment_change`, `player_logged_in`, `player_clone`, `player_respawned`,
    `player_xp_change`, `loot_drops`, `living_incoming_damage_v2`, `prepare_anvil`, `register_commands`,
    `net_from_client`, and the data reload listeners (`native_add_server_reload_listeners`, API 400).
  - Client: `tooltip`, `key` / `input_key`, `net_from_server`, and the screen hooks (`screen_render_post`, clicks,
    keys, scroll) over a mod's own screen (`mod_screen_open`, API 111).
- Helpers: `give_levels` (a negative value takes levels), `run_command` (runs a vanilla command such as
  `/attribute`), `living_equipment` and `living_set_equipment`, `give_stack`, `action_bar`, `tell`,
  `play_local_sound`, `net_send_to_player` and `net_send_to_server` (at most 30,000 bytes per message and 32 messages
  a tick each way per player).
- Host helpers outside the catalog (`docs/native-mods.md`, "The typed API from mixin code"): `server_item_in_tag`,
  `block_name_in_tag` and `component`, used to resolve lock targets on the server. Screen drawing helpers: `fill`,
  `quad`, `text`, `text_width`, `item_model`. Ticks: `server_tick` (server) and `tick` (client).
- Not in the trait at this commit, so not used: `player_command` (use `perform_command`), `player_respawned` (use
  `player_clone` and `server_tick`), `server_data_loaded` (use `server_reload_listener_apply`).
- Documented but not yet numbered at this commit (marked `@API:...@` in the docs and absent from
  `native-hooks.json`): `native_break_speed` and the attribute helpers (`native_attribute`,
  `native_set_attribute_base`, `native_set_attributes`). This design does not depend on them: it changes attributes
  with the vanilla `/attribute ... modifier` command through `run_command`. If the pinned commit has the helpers, the
  builder may use them instead.
- Server attachments: typed data on `Holder::Player(uuid)`, saved in the player's file, with `copy_on_death` (kept
  through respawn) and `sync` (sent to Horizon clients).
- Data files: `NativeReloadListener::json(id, folder)` reads `data/<namespace>/<folder>/*.json` from every namespace on
  each data load (world open, `/reload`). Content mods may only write files in their own namespaces.
- Multiplayer: single player and LAN run the mod in the host's game. A dedicated server needs an engine export
  (`horizon-launcher native-build FOLDER --server-output DIR`). Players without the mod can still join a server, and
  the server's rules still apply to them.

## 3. Goals

1. One progression system: XP levels buy skill levels, skill levels earn perk points, and skill levels unlock gear.
2. Locks are visible before they bite: item tooltips, a preview of what the next levels unlock, and a clear message
   when an action is refused.
3. Everything is data, so a pack maker can retune or replace skills, perks, locks, focus areas and costs with no code.
4. Standalone: works with no other mod. Other mods (from this family or anyone's) integrate by shipping data files.
5. Works in single player, on LAN and on a dedicated server. The server decides everything.
6. Modern: covers 26.3 gear and mounts, matches items by tags and components, and uses the game's own attributes.

## 4. Non-goals (version 1)

- The other layers (survival, combat and gear, creatures and places). They integrate like any other mod (7.10).
- A server-only mode or a chest-menu interface (approaches 1 and 3 below were rejected).
- Leaderboards, or classes beyond the focus choice. (Parties and shared skills were later added as a server rule,
  7.11.)
- Importing saves from Reskillable or Level Up! Reloaded.
- Languages other than English. All text must still be translatable.

## 5. Anti-goals (a failure even if everything works)

- Copying the originals: their names, perk names or descriptions, icons, screen layout or text. Build from this
  document, not from their code, assets or text.
- A mod name or icon that could be mistaken for either original.
- Trusting the client. A client only asks; the server checks every request and applies it.
- Losing progress. Skills survive relogs, restarts, `/reload` and death (unless a pack turns on a death penalty).
- Surprise locks: a player hitting a lock without being able to see why or what is needed.
- Requiring any other mod.

## 6. Approaches

The main question was where the code runs.

1. **Server only, like a Spigot plugin.** A chest-menu screen opened with `/skills`; nothing to install to join a
   server. Rejected: no hotkey and no tooltips, which loses two of the agreed fixes.
2. **Full mod on client and server.** A real screen on a hotkey, tooltips and the unlock preview; every player needs
   the mod, which is normal for a modpack. **Picked by the user.**
3. **Both.** The server works alone with chest menus and the mod adds the real interface. Rejected: two interfaces
   to build and keep in step.

Claude's addition within option 2: a plain `/skills` text command, so a player without the mod (or the console) can
still read and spend.

## 7. Player experience

The user approved this design on 2026-11-13. Numbers marked "default" are settings.

### 7.1 Skills

Eight default skills, each from level 1 to a default maximum of 32 (per skill, tunable):

| Skill | Covers by default |
|---|---|
| Melee | attacking with swords, axes, maces and spears |
| Ranged | bows, crossbows, tridents |
| Defense | armor, shields |
| Mining | breaking with pickaxes |
| Gathering | breaking with axes and shovels, hoes, shears, brushes |
| Agility | elytra, movement perks |
| Arcane | enchanting table, anvil, brewing stand |
| Taming | riding mounts, saddles and harnesses, wolf and horse armor |

Requirements are per action, so one item can need different skills for different uses (an axe: Melee to attack,
Gathering to chop).

### 7.2 Raising a skill

- XP levels are the only currency. Raising a skill from level L to L+1 costs `base + floor((L - 1) / every)` XP
  levels, default base 2 and every 4: 2 levels at first, 9 near the top, about 167 levels for one skill from 1 to 32.
- The cost is in levels, not points, so spending at a high XP level is worth more, as in RLCraft. Levels are taken
  with `give_levels(-cost)`.
- A focus discount can apply (7.4). The cost is never below 1.
- Raising happens only when the player asks (screen or command), never on its own.

### 7.3 Perks

Changed by the user on 2026-11-14 (RLCraft's point rate; every perk reachable at the top):

- Every 2nd level of a skill grants 1 perk point for that skill (default `points_every` 2): 16 points at level 32.
- Each skill has 5 perks, priced by the level they unlock at: 2 points at levels 4, 8 and 12, 4 at 16, and 6 for the
  capstone at 24. That is exactly 16, so a maxed skill owns every perk; before then the player picks which to get
  first. Each perk is affordable at its own level.
- Most perks are attribute modifiers on the player, which the game syncs to everyone: for example mining efficiency,
  attack damage, attack speed, sweeping damage, armor, armor toughness, knockback resistance, movement speed, step
  height, safe fall distance, jump strength, block and entity interaction range, luck.
- Other effect types in version 1: extra drop chance (`loot_drops`), damage dealt or taken multipliers by kind
  (melee, projectile, fall, fire, explosion; `living_incoming_damage_v2`), an XP gain multiplier (`player_xp_change`),
  an anvil cost multiplier (`prepare_anvil`), and attribute modifiers on the mount the player is riding.
- Bought perks stay bought. If a pack's death penalty lowers a skill below a perk's minimum level, that perk stops
  working until the level is back; it is not refunded.

### 7.4 Focus

Changed by the user on 2026-11-13: a focus no longer makes raising cheaper; it gives XP for the area's tasks.

- Three default focus areas: Fighting (Melee, Ranged, Defense), Working (Mining, Gathering, Arcane), Roaming
  (Agility, Taming). A player may have several active at once.
- Adding an area costs `focus_cost` (default 10) XP levels for each area that will be active: the first ever is
  free (`focus_first_free`), a second costs 20, a third 30. Dropping one is free.
- Each area lists its rewards in its data file (`rewards`): `kill` and `block_xp` give a percentage more of the XP
  vanilla drops (default +50% for Fighting's kills and Working's ores); `break` (a block matcher, optional
  `state` properties such as a crop's `age`, optional `label`), `smelt`, `travel`, `tame` and `breed` give XP
  points. Defaults: Working mines, digs (0.05 a block), chops logs (0.25), harvests grown crops (0.5) and smelts
  (0.1 an item); Roaming travels (0.02 a block), tames (5) and breeds (2).
- Blocks a player placed give no `break` points (remembered for the session); creative and spectator players and
  creative flight earn nothing; a step over 10 blocks in a tick is a teleport, not travel. Points add up with
  their fractions and whole ones are given on the server tick.
- `/skills focus <area>` adds an area, `/skills unfocus <area>` drops one. Saves from the first format keep their
  one focus as the only active area.

### 7.5 Locks

- A lock gives a requirement (one or more `skill: level` pairs) for some actions on a target:
  - items: `use` (either hand), `attack` (main hand), `break` (main-hand tool), `equip` (armor slots, elytra,
    shield in the off hand)
  - blocks: `open` (enchanting table, anvil, brewing stand, any container), `place`
  - entities: `mount`, `interact`
  - abilities: `use`. An ability is something another mod adds that is not an item, block or entity (a dash, a
    double jump), named by that mod's own id (`dashmod:dash`). See 7.10.
- Targets match by id, by tag (`#minecraft:pickaxes`), by id pattern (`minecraft:*_spear`), or by component (for
  example repairable with a given material tag), and by combinations of these. That is how one entry covers "everything
  netherite" and modded items.
- Default ladder in the matching skill: copper 4, iron 8, diamond 16, netherite 24. Chainmail and golden armor need
  Defense 4 (user, 2026-11-13). Leather, wood, stone and gold tools are free. A tiered pickaxe, shovel or hoe is
  locked for attacking too, at its tool skill's level (user, 2026-11-13). Default specials: mace Melee 20, trident Ranged 16, crossbow Ranged 6, elytra Agility 16,
  shield Defense 4, wolf armor Taming 6, riding a happy ghast Taming 12, riding other mounts Taming 4, enchanting
  table Arcane 8, anvil Arcane 6, brewing stand Arcane 4.
- Each lock entry may carry a `label` (a translation key such as "Diamond tools"), which the screen's unlock preview
  shows. Tags cannot be listed item by item, so the preview shows labels, not every matching item.
- Locks apply to players only, never to mobs.
- Server enforcement, always on: the action is cancelled and the player gets an action-bar line such as "Needs
  Mining 16" (rate limited). Players with the mod also hear a short sound. Locked items already equipped go back to the inventory (dropped at the
  player's feet if it is full); this is checked on equip, on login and after `/reload`.
- While a player holds a tool locked for `break` in the main hand, a `minecraft:block_break_speed` modifier of -100%
  applies, so breaking never starts. The game syncs attributes to every client, so this also works for players
  without the mod. It is removed as soon as they hold something else.
- Players with the mod also see the requirement in the item's tooltip (7.8).
- Creative and spectator players ignore locks (setting, default on).

### 7.6 Death and respawn

- Skill levels, perk points, perks and focus are kept by default (attachment with `copy_on_death`).
- Setting: lose N levels in each skill on death (default 0, never below level 1).
- Attribute perks are re-applied after respawn.

### 7.7 The skills screen (client)

- Opens with a hotkey (default K, rebindable if Horizon allows it) and with `/skills open`. Escape closes it. It does
  not pause single player (Horizon's own-screen behavior).
- Shows the player's XP levels and focus; each skill's level and maximum, raise cost (after discount) and perk
  points; for the selected skill, its perks (bought, buyable, or locked with the reason) and what its next levels
  unlock; and the focus picker.
- Every button sends a request to the server; the screen redraws from the synced state.

```
 Reskilled                         XP levels: 23    Focus: Working
 ----------------------------------------------------------------------
 Mining     12/32   raise: 3 levels   perk points: 2      [selected]
 Melee       8/32   raise: 3 levels   perk points: 1
 Taming      3/32   raise: 2 levels   perk points: 0
 ...
 Mining 16 unlocks: diamond pickaxe
```

### 7.8 Tooltips (client)

- A locked item shows one line per requirement for its actions: red if not met, green if met. Items with no lock show
  nothing extra. A client setting can hide met requirements.

### 7.9 Commands

- Players: `/skills` (text summary), `/skills raise <skill>`, `/skills perk <perk>`, `/skills focus <area>`,
  `/skills open`.
- Operators (permission level 2): `/skills set <player> <skill> <level>`, `/skills points <player> <skill> <n>`,
  `/skills reset <player> [skill]`.

### 7.10 Other mods' unlockables

Added on 2026-11-13 at the user's request: every new mod must be able to add its own unlockables.

- With data only: any mod (or a pack) ships skills, perks and locks for its own items, blocks and entities in its own
  namespace, with no code (section 8).
- Abilities: a mod that adds an ability with no item, block or entity behind it declares a lock entry with an
  `ability` target (`{"ability": {"id": "dashmod:dash"}}`, action `use`, for example Agility 10). It shows in the
  unlock preview like any other lock, and a pack can retune it.
- Integration API: Reskilled' trait mod package publishes a few functions for other trait mods (Horizon's
  "Dependencies between trait mods", as an optional dependency, so neither mod requires the other): a player's
  level in a skill, whether they own a perk, and whether they may use an ability (which also shows the usual
  "Needs Agility 10" line when they may not). The other mod calls it on the server before running its ability.

### 7.11 Options and server rules (added 2026-11-14)

The skills screen's gear opens Reskilled Options, a screen in the game's options style with two tabs. Display holds
each player's own settings (warning panel place and time, padlock on/off, corner and dimming, tooltip options, the
skill strip, each sound), saved to `config/reskilled-client.json`. Server rules holds the rules below, kept on the
world (`Holder::Level`), sent to every player, and changed only by operators (checked in the player's own tick).

- Raise cost: XP levels, or XP points (the level price times "points a level", 10, 20 or 50); points are taken as
  whole levels from the top with the rest given back.
- Perk switches: an owned perk can be switched off (still owned, no effect) and on.
- Perk refunds: off, free, or for 1, 5 or 10 XP levels; a perk another owned perk needs is refunded last.
- Enchantment locks: lock targets may name an enchantment from a level (`enchantment_level`); its locks add to the
  item's when it is used, swung, dug with or worn. The anvil hook does not show its inputs, so applying a book is
  not refused. Defaults lock strong Sharpness, Smite, Bane, Power, Protection, Fortune, Looting, Unbreaking, Mending.
- Shared skills: off, by party (`/skills party`), or everyone. While sharing, each member is shown the highest
  level any member reached by themselves; their own level is kept apart and is theirs again when they leave or
  sharing is turned off (so levels cannot be carried out of a party). Raising, earning, perk points and perks use
  the own level; a death lowers the own level, and the group's shared level with it when that member was best.
- Skills grow by: buying, doing (work noted per skill, level L takes ten actions times L at 100% pace), or both.
- Perks (revised 2026-11-14 after play): a perk may also need levels in other skills (`also_needs`, own levels).
  Besides number effects, perks may carry mechanics: a last stand, a potion effect for kills, crop growth around the
  player, replanting ripe crops, and ender pearl damage rules. Five weak perks became Last Breath, Bloodrush, Pearl
  Step, Green Aura and Replant (same ids, so owners keep their points); XP gain, capstones and taming were retuned.
  Each skill keeps the 2, 2, 2, 4, 6 price ladder, so a maxed skill buys all its perks.
- Building replaces Taming (2026-11-14, after play: mount boosts mattered too little). Building locks placing
  advanced blocks (scaffolding 4; pistons, hoppers, dispensers, brewing stands 8; anvils, enchanting tables,
  observers, comparators 12; beacons, conduits, lodestones, respawn anchors 16), grows by placing blocks (each spot
  once), joins the Roaming focus, and its perks act around the player's home (their respawn point): Salvage, Restful
  Sleep, Hearth, Home Ward, Safe Haven. Taming's mount, lead and animal armor locks moved to Agility at the same
  levels. Arcane became magic and enchanted gear: Spellward, Runeblade, Warding Runes, Antidote, Living Runes; no
  shipped perk raises XP gain any more. The table and mock-ups above still show Taming as first designed.
- Skill strip: a display setting showing the player's skills and levels beside the inventory and crafting table.
- Enchantment locks (2026-11-16, after play): enchantments need Arcane only; the gear's own skill already locks the
  gear. Level I is free and each level above it needs 3 more Arcane (Sharpness, Smite, Bane and Power up to V: 12;
  Protection IV: 9; Unbreaking III: 6); Fortune and Looting climb 6 a level (III: 12); Mending needs 12. A refusal or
  tooltip line brought by an enchantment names it ("Sharpness III: Arcane 6").
- Restful Sleep (2026-11-16, after play): damage wakes a sleeping player, so poison kept the player from sleeping.
  Getting into bed now lifts the effects it clears; a full night ends them and heals; leaving early (or the game)
  brings them back as they were; dying drops them.
- Perk guide removed (2026-11-16, after play): the G page with animated scenes and value cards was taken out at
  the user's request; some perks had no scene. A perk's tooltip (name, description, cost, needs) is its only
  explanation, and every perk description is to be rewritten.

## 8. Data

All definitions are JSON files read by reload listeners from every namespace:

- `data/<ns>/reskilled/skills/*.json`
- `data/<ns>/reskilled/perks/*.json`
- `data/<ns>/reskilled/focus/*.json`
- `data/<ns>/reskilled/locks/*.json`
- `data/<ns>/reskilled/settings/*.json`

Rules:

- Each entry names what it defines with an `id` (for example `reskilled:mining`). When several files define
  the same id, the highest `priority` wins (default 0; a tie goes to the later file in load order). A pack maker's own
  content mod or a world data pack can therefore override anything without writing into this mod's namespace.
- Lock files add entries. An entry with `"replace": true` drops earlier entries for the same target.
- Settings merge field by field, highest priority first.
- A bad file is logged with its path and reason and skipped. Bad data never crashes the game.
- Each file carries a `format` number so later versions can read old files.
- Player-facing text is translation keys, with English in `assets/reskilled/lang/en_us.json`.
- The mod's defaults ship in its own content (`data/reskilled/...`, `assets/reskilled/...`).
- Client-only preferences (tooltip options) go in `config/reskilled-client.json`.

## 9. Architecture

- The repo holds the whole family, one folder per mod. This mod lives in its own folder.
- **Core crate** (no `horizon-client`): data model, parsing and validation, merge rules, cost math, lock matching,
  perk resolution, request validation, the save format and its migrations. Most tests live here.
- **Trait mod package**: implements `Hooks` and adapts the game to the core. Server side: enforcement, perk effects,
  commands, networking, attachments. Client side: screen, tooltips, hotkey, dig speed.
- Ships as one `.hmod` (content and code), pinned to one Horizon commit, with that commit and the hook API number
  recorded in the repo.
- Lock requirements are resolved per item, block or entity id on first use and cached until the next data load, so
  checks on every attack or block break stay cheap. (The server cannot list every item in a tag, so nothing is
  precomputed.)
- **Player state**: one attachment type on `Holder::Player(uuid)` holding a format version, each skill's level and
  perk points, bought perks and the focus. It has a codec, `copy_on_death: true` and `sync: true`.
- **Definitions to clients**: on login and after `/reload`, the server sends the effective skills, perks, focus
  areas, settings and lock labels on channel `reskilled:defs`, split into chunks within the message limits.
  Requests go the other way on `reskilled:act`; the server treats them as untrusted input.
- **Item requirements for tooltips**: only the server can test tags, so it resolves each item's requirements (cached
  per data load). A client asks for the item ids it shows (inventory contents and hovered items) on
  `reskilled:act` and caches the answers from `reskilled:msg` until the next data load. A tooltip for
  an item not answered yet shows nothing extra until the answer arrives (a tick or two).
- **Attribute perks**: modifiers with a stable id per perk (and one for the dig lock), applied with the vanilla
  `/attribute <player> <attribute> modifier add|remove <id> ...` command through `run_command`. They are re-applied on
  login, respawn and data reload, and removed when a perk stops applying.
- **Saved state for skills a pack removed**: kept in the save but ignored, so adding the skill back restores it. A
  level above a lowered maximum is capped when read.

## 10. Testing

- Work is test-first.
- Core crate unit tests: cost math, focus discount, perk point grants, perk prerequisites, lock matching (id, tag,
  pattern, component, combinations), priority and replace merging, settings merge, request validation (not enough
  XP, already at max, unknown ids), save round trips and migrations.
- Hook tests with `Host` test doubles through `dispatch_<hook>` (the method Horizon documents): locked attack, break,
  use, mount and open are cancelled; locked armor is sent back; a raise request takes the cost and adds the level; bad
  requests are refused.
- In game: a single-player checklist run, then a dedicated-server export joined by one client with the mod and one
  without.

## 11. Build and environment needs

- A Rust toolchain at the version the native mod template pins (not installed on this machine yet), Python 3.12 or
  newer (3.14 is installed), `cargo-horizon-mod`, and a Horizon checkout at the pinned commit.
- In-game tests need Horizon's prepared game resources, made from the user's own copy of Java Edition 26.3. Without
  them, only the unit and hook tests can run.

## 12. Publishing (later)

- Before release, check that the name is free on Horizon's registry, Modrinth and CurseForge.
- The description says: "Fills the role of Reskillable and Level Up! Reloaded on Horizon (not affiliated)", with
  tags for skills, RPG and progression.
- Horizon's registry needs a public GitHub repository under a license that lets players run the mod. The license is
  the user's choice at release time.

## 13. Left to the builder

- Exact hook versions and the API number at the pinned commit; whether to use Horizon's attribute helpers instead of
  `/attribute` if the pinned commit has them.
- Which equipment hook or tick check keeps the dig-lock modifier in step with the held item.
- The perk list and its numbers, within 7.3; the final default lock table, within the ladder in 7.5.
- Screen layout details and original icon art, within 7.7; another default hotkey if K is taken.
- The chunking format for definitions, save format details, and logging.

## 14. Calls Claude made

The user handed these to Claude; each can be changed.

- The eight default skills, first with Taming (new) and no Building skill; Building replaced Taming on 2026-11-14
  (7.11).
- The cost curve (2 levels, +1 every 4 levels) and 1 perk point every 4 levels.
- Three focus areas with a 25% discount and a 10-level switching cost.
- The lock ladder (copper 4, iron 8, diamond 16, netherite 24) and the special items listed in 7.5.
- Skills kept on death by default.
- Creative and spectator players ignore locks.
- Locks enforced by cancelling the action, plus a zero dig-speed modifier while holding a locked tool.
- Data overrides by `priority` so pack makers never write into this mod's namespace.
- A `/skills` text command beside the screen.
- One repository for the whole family.
