---
name: reskilled-modding
description: Use when creating, retuning or removing Reskilled skills, perks, focus areas, gear locks or settings for a Minecraft Horizon mod or modpack, or when making another mod's items, blocks, mobs, wearables or projectiles work with Reskilled's skill levels.
---

# Customizing Reskilled

Reskilled (Minecraft Horizon) is driven entirely by JSON data files. A mod or pack changes it by **adding its own
files in its own namespace**, never by editing Reskilled's.

- [reference.md](reference.md): every field, default and limit.
- [example-pack/](example-pack/): a complete working pack (a new skill with perks, locks on a mod's items, settings
  entries, a retuned shipped perk and a removed one).
- [integration.md](integration.md): what another mod's code can call (abilities, accessory slots, reading
  progress).

## A correct change

1. **Files** in `data/<your namespace>/reskilled/<skills|perks|focus|locks|settings>/<name>.json`, each with
   `"format": 1`. Your namespace is your mod id or pack name.
2. **New things** get ids in your namespace (`examplemod:digging`).
3. **Changing a shipped thing**: copy Reskilled's file in full into your folder, keep its `id`, add
   `"priority": 1`, edit. The highest priority wins *whole*.
4. **Removing a shipped skill, perk or focus area**: `{"format": 1, "id": "<its id>", "priority": 1, "remove": true}`.
   For a lock: an entry with the same `target`, `"replace": true`, `"requires": {}`.
5. **Locks and the settings lists** (`not_creatures`, `projectile_damage`, `equip_slots`) **add up**: write only
   your entries.
6. **Text**: every `name`, `description` and `label` key has English in `assets/<your namespace>/lang/en_us.json`.
   Descriptions are short and give the real numbers of the perk's effects (`"+10% break speed."`).
7. **Check**: in game, `/reload`, then search `logs/latest.log` for `Reskilled skipped` (each line names the file
   and the reason) and read the count line `Reskilled loaded N skills, N perks, ...`.

## Usual tasks

| Goal | File | Example |
|---|---|---|
| Lock a mod's item, block or mob behind a level | `locks/` | `example-pack/.../locks/examplemod_gear.json` |
| Add a skill that grows by doing (`earned_by`) | `skills/` | `.../skills/digging.json` |
| Add perks to a skill | `perks/` | `.../perks/digging_quick_spade.json` |
| Retune a shipped perk | `perks/`, full copy, priority 1 | `.../perks/override_light_step.json` |
| Remove a shipped perk or skill | its folder, `"remove": true` | `.../perks/remove_keen_eye.json` |
| Put your skill in a shipped focus area | `focus/`, full copy, priority 1 | `.../focus/working.json` |
| Mod's statue/cart earns no Melee XP | `settings/` `not_creatures` | `.../settings/examplemod.json` |
| Mod's darts earn Ranged XP | `settings/` `projectile_damage` | same |
| Shift-click must not put on a locked modded wearable | `settings/` `equip_slots` | same |
| Give a skill its colour, a perk its icon, or put a lock on the skill's road | `color` on the skill, `icon` on the perk or the lock entry (with its `label`) | `.../skills/digging.json`, `.../locks/examplemod_gear.json` |
| Change costs, max level, perk point pace | `settings/`, priority 1, only those fields | reference.md, Settings |
| Make a mod's own accessory slots (rings, amulets) respect equip locks | code: `reskilled::may_equip` / `may_wear` | [integration.md](integration.md), Accessory slots |

Modded gear that joins vanilla tags (`#minecraft:pickaxes`, `#minecraft:chest_armor`...) and is repaired with
diamonds or netherite already falls under the shipped tier locks.

## Common mistakes

| Mistake | Result |
|---|---|
| Editing or shipping files in the `reskilled` namespace | lost on update; another pack's override fights it |
| Overriding with only the changed fields | every left-out field takes its default (a perk loses its effects) |
| Same priority as the shipped file | the later-loaded file wins, which is not guaranteed to be yours |
| `{"tag": ...}` on an entity, enchantment or damage type | file skipped: only `id` and `pattern` work there |
| `"pattern": "*boat"` | file skipped: patterns must look like ids, write `"*:*boat"` |
| A skill in two focus areas | the later area is skipped |
| `"#"` missing on a tag (`"minecraft:pickaxes"`) | file skipped |
| Perk numbers in the description that differ from `effects` | players are told the wrong thing |

Server rules (how raises are paid, perk switches, enchantment locks on or off, shared skills, growing by doing) are set
in game by operators, not in data.
