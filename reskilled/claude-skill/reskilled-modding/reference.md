# Reskilled data reference

Every field of every Reskilled data file, with its default and limits. The game checks all of this when it loads
data (world open and `/reload`) and skips a bad file with a line in `logs/latest.log`:
`Reskilled skipped <file>: <reason>`.

## Contents

- [Where files go](#where-files-go)
- [How files combine](#how-files-combine)
- [Matchers](#matchers)
- [Skills](#skills) and [what grows them](#earned_by)
- [Perks](#perks) and [their effects](#effects)
- [Focus areas](#focus-areas)
- [Locks](#locks)
- [Settings](#settings)
- [Removing a skill, perk or focus area](#removing)
- [Text](#text)

## Where files go

`data/<namespace>/reskilled/<folder>/<name>.json`, where `<folder>` is `skills`, `perks`, `focus`, `locks` or
`settings`. Use **your own namespace** (your mod id or pack name); never edit or ship files in the `reskilled`
namespace. The files can ship in a content mod (`horizon-mod.toml` with `assets/` and `data/`) or in a world's data
pack (`saves/<world>/datapacks/<pack>/data/...`).

Every file is one JSON object with `"format": 1`.

Ids are `namespace:path`, lowercase (`examplemod:digging`). Translation keys are any string; put their English in
`assets/<namespace>/lang/en_us.json`.

## How files combine

| Folder | Rule |
|---|---|
| `skills`, `perks`, `focus` | One definition per `id`. The file with the highest `priority` (default 0) wins whole; a tie goes to the file loaded later. To change a shipped one, copy it **in full**, give it `"priority": 1` and edit; fields you leave out take their defaults, they do not fall back to the shipped file. |
| `locks` | Entries **add up** across files. An entry with `"replace": true` first drops earlier entries for an equal `target`. |
| `settings` | Each field: highest `priority` wins. The lists `not_creatures`, `projectile_damage` and `equip_slots` **add up** across files instead. |

Shipped files are all priority 0.

## Matchers

A matcher picks ids. It is one of:

| Form | Picks | Works on |
|---|---|---|
| `{"id": "minecraft:diamond_sword"}` | that id | everything |
| `{"tag": "#minecraft:pickaxes"}` | ids in the tag (leading `#`) | items, blocks |
| `{"pattern": "minecraft:*_sword"}` | ids where `*` is any run of characters; must look like an id, so "any namespace" is `*:` (`*:*boat`) | everything |
| `{"repairable": "#minecraft:diamond_tool_materials"}` | items repaired with that tag or item | items |
| `{"any_of": [m, ...]}` | ids any of them picks | as its parts |
| `{"all_of": [m, ...]}` | ids all of them pick | as its parts |

Entities, enchantments, damage types and abilities have no tags the game lets Reskilled test: use `id` and
`pattern` for them. Modded items join tag-based locks and rules by joining the vanilla tags
(`#minecraft:pickaxes`, `#minecraft:head_armor`...).

## Skills

`skills/<name>.json`

| Field | Type | Default | Notes |
|---|---|---|---|
| `id` | id | required | |
| `priority` | integer | 0 | |
| `name` | translation key | required | |
| `icon` | item id | required | |
| `icon_levels` | object, level → item id | `{}` | from that level on, the icon becomes that item |
| `max_level` | integer ≥ 1 | settings' `max_level` | |
| `cost` | `{"base": n, "every": n ≥ 1}` | settings' `cost` | raising from level L costs `base + floor((L - 1) / every)` XP levels |
| `points_every` | integer ≥ 1 | settings' `points_every` | one perk point at every multiple of this level |
| `earned_by` | list of rules | `[]` | see below; a skill with none grows only by buying |
| `color` | `"#RRGGBB"` | one of eight, chosen by the id | the skill's stripe and accents on the screens |

### earned_by

How the skill grows when the server's rule lets skills grow by doing. Each rule is worth `worth` actions (default
1); a level L takes ten actions times L at the rule's usual pace. A rule's worth is at least 0.

| `type` | Counts | Fields |
|---|---|---|
| `break` | a block broken with an item in the main hand that `with` picks | `with` (item matcher), `worth` |
| `place` | a block placed, once for each spot | `worth` |
| `attack` | a blow on an entity the settings' `not_creatures` do not pick | `worth` |
| `projectile_hit` | a projectile the player shot hitting, by a damage type the settings' `projectile_damage` pick | `worth` |
| `hurt` | a hit the player takes from something | `worth` |
| `travel` | each block travelled | `per_block` (≥ 0) |
| `enchant` | enchanting an item | `worth` |

```json
{"format": 1, "id": "examplemod:digging", "name": "examplemod.skill.digging", "icon": "minecraft:wooden_shovel",
 "earned_by": [{"type": "break", "with": {"any_of": [{"tag": "#minecraft:shovels"}, {"id": "examplemod:drill"}]}}]}
```

## Perks

`perks/<name>.json`

| Field | Type | Default | Notes |
|---|---|---|---|
| `id` | id | required | |
| `priority` | integer | 0 | |
| `skill` | skill id | required | its points pay for the perk |
| `name`, `description` | translation keys | required | the tooltip shows each sentence of the description as a bullet; keep numbers in it true to `effects` |
| `cost` | integer | required | perk points |
| `min_level` | integer | required | level in `skill` to buy it and keep it working |
| `requires` | list of perk ids | `[]` | bought first |
| `also_needs` | object, skill id → level | `{}` | levels in other skills, to buy it and keep it working |
| `effects` | list | required | see below |
| `icon` | item id | the skill's icon | drawn for the perk on the skill's road and in its panel |

A perk whose `skill`, `also_needs` skill or `requires` perk does not exist is skipped with an error.

### Effects

Each has a `type`. Damage kinds: `melee`, `projectile`, `fall`, `fire`, `explosion`, `ender_pearl`, `magic`, `any`.
Attribute operations: `add_value`, `add_multiplied_base`, `add_multiplied_total` (vanilla's `/attribute`).

| `type` | Fields | Limits |
|---|---|---|
| `attribute` | `attribute` (vanilla attribute id), `amount`, `operation` | |
| `mount_attribute` | as `attribute`, on the mount ridden | |
| `drop_bonus` | `target` (block matcher), `chance` | chance 0–1 |
| `damage` | `direction` (`dealt`/`taken`), `kind`, `multiplier` | multiplier ≥ 0 |
| `xp_gain` | `multiplier` | ≥ 0 |
| `anvil_cost` | `multiplier` | ≥ 0 |
| `last_stand` | `cooldown_seconds`, optional `above` (least health before the blow, half hearts; default 0: any) | above ≥ 0; `/kill` is never survived |
| `effect_on_kill` | `effect` (effect id), `seconds`, `amplifier`, `mobs` (entity matcher) | |
| `crop_growth` | `radius`, `interval_seconds`, `chance`, `blocks` (block matcher, default `#minecraft:crops`) | radius ≤ 16, interval ≥ 1, chance 0–1; blocks grow by their `age` property |
| `replant` | `crops`: list of `{"block", "seed", "ripe_age"}` | |
| `enchanted_weapon` | `per_enchantment`, `max` | both 0–1 |
| `enchanted_armor` | `per_enchantment`, `max` | both 0–1 |
| `shorter_effects` | `effects` (effect ids), `multiplier` | multiplier 0–1 |
| `rune_repair` | `interval_seconds`, `points` | interval ≥ 1 |
| `salvage` | none | |
| `restful_sleep` | `clears` (effect ids) | |
| `near_home` | `radius`, `effect`, `amplifier` | radius ≤ 64 |
| `home_ward` | `radius` | ≤ 64 |
| `safe_haven` | `radius` | ≤ 64 |

"Home" is the player's respawn point (bed or respawn anchor). Perks with a `near_home`, `home_ward`, `safe_haven` or `crop_growth` effect get an
Area button on the skills screen that draws the covered ground in the world.

## Focus areas

`focus/<name>.json`: a group of skills that gives extra XP for its tasks. A skill may be in one area only.

| Field | Type | Default |
|---|---|---|
| `id` | id | required |
| `priority` | integer | 0 |
| `name` | translation key | required |
| `skills` | list of skill ids | required |
| `rewards` | list | `[]` |

Rewards, by `task`: `kill` (`bonus_percent` more mob XP), `block_xp` (`bonus_percent` more block XP), `break`
(`blocks` matcher, `points`, optional `state` like `{"age": "7"}`, optional `label` key; blocks a player placed give
nothing), `smelt`, `travel`, `tame`, `breed` (each `points`).

To add your skill to a shipped area, override the area in full (copy it, `"priority": 1`, add your skill).

## Locks

`locks/<name>.json`: `{"format": 1, "entries": [...]}`

| Field | Type | Default | Notes |
|---|---|---|---|
| `target` | `{"item": m}`, `{"block": m}`, `{"entity": m}`, `{"enchantment": m}` or `{"ability": m}` | required | |
| `actions` | list | required | items and enchantments: `use`, `attack`, `break`, `equip`; blocks: `open`, `place`; entities: `mount`, `interact`; abilities: `use` |
| `requires` | object, skill id → level | required | all must be met |
| `label` | translation key | none | shown in the skill page's "Next unlocks" |
| `replace` | boolean | false | drops earlier entries for an equal target first; with `"requires": {}` it only removes them |
| `enchantment_level` | integer | 1 | enchantment targets only: the lock applies from this level up |
| `icon` | item id | none | with a `label`, the entry shows on its skill's road at its level with this item; without one it is not on the road |

An enchantment lock adds to the lock of any item carrying that enchantment, and the player is told which
enchantment brings it. `equip` locks are checked on the game's own worn slots (armor, off hand); another mod's
accessory slots ask Reskilled through `reskilled::may_equip` ([integration.md](integration.md)). Ability locks are for other mods' code; see [integration.md](integration.md).

```json
{"format": 1, "entries": [
  {"target": {"item": {"id": "examplemod:drill"}}, "actions": ["break", "attack"],
   "requires": {"examplemod:digging": 12, "reskilled:mining": 8}, "label": "examplemod.lock.drill"}]}
```

## Settings

`settings/<name>.json`

| Field | Type | Shipped |
|---|---|---|
| `priority` | integer | 0 |
| `max_level` | integer ≥ 1 | 32 |
| `cost` | `{"base", "every" ≥ 1}` | `{"base": 1, "every": 1}` |
| `points_every` | integer ≥ 1 | 2 |
| `focus_first_free` | boolean | true |
| `focus_cost` | integer, XP levels per active area | 10 |
| `death_penalty_levels` | integer, levels lost per skill on death | 0 |
| `creative_ignores_locks` | boolean | true |
| `lock_message_cooldown_ticks` | integer | 20 |
| `not_creatures` | list of entity matchers (**adds up**) | decorations, boats, rafts, minecarts |
| `projectile_damage` | list of damage-type matchers (**adds up**) | `*:*arrow*`, `*:*trident*`, `*:*thrown*`, `*:*fireball*`, `*:*wind_charge*` |
| `equip_slots` | list of `{"items": item matcher, "slot": "head"/"chest"/"legs"/"feet"/"offhand"}` (**adds up**, first match wins) | the four armor tags, elytra, shield |

How raises are paid, perk switches, enchantment locks on or off, shared skills and growing by doing are **server rules**,
set in game (Reskilled Options, Server rules tab, operators only) and kept with the world; they are not data.

## Removing

A skill, perk or focus area is removed by a file with the same `id`, a higher `priority` and `"remove": true`, in
its own folder. It takes no other field.

```json
{"format": 1, "id": "reskilled:ranged_keen_eye", "priority": 1, "remove": true}
```

What only pointed at it goes quietly: a removed skill's perks (and perks requiring those), its place in focus
areas (an area left empty goes too), and lock needs on it (an entry needing nothing else is dropped). Players'
saved progress in a removed skill is kept, so adding it back restores it.

A lock is removed with an entry for the same target, `"replace": true` and `"requires": {}`.

## Text

Every `name`, `description` and `label` is a translation key. Give each one English in
`assets/<your namespace>/lang/en_us.json`. Without it, a skill's or perk's name falls back to its id as words
(`examplemod:quick_spade` reads "Quick spade") and a description shows nothing. Reskilled's own style is short
and numeric: `"+10% break speed."`, `"20% chance of extra clay and gravel drops."`.
