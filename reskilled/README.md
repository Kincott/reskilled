# Reskilled

Skills, perks and gear locks for Minecraft Horizon (26.3). Fills the role of Reskillable and Level Up! Reloaded on
Horizon (not affiliated).

Players raise eight skills (Melee, Ranged, Defense, Mining, Gathering, Agility, Arcane, Building) with XP, unlock
perks with the points the levels earn, and need levels to use gear: iron at 8, diamond at 16, netherite at 24, and
more. Everything is data, and the server's rules change in game.

## For players

- **K** opens the skills screen (`/skills open` too). Click a skill to see its perks and what its next levels
  unlock; the gear opens Reskilled Options.
- Shift-click a raise button to raise 5 levels at once, Ctrl-click to raise as many as your XP pays for.
- Locked items show a padlock and their requirements in the tooltip. A refused action shows what it needs.
- Focus areas (Fighting, Working, Roaming) give extra XP for their tasks; the first is free.
- `/skills` lists your skills; `/skills raise <skill>`, `perk <perk>`, `focus <area>` and `party ...` do the
  rest from chat.

## For server operators

Reskilled Options' **Server rules** tab (operators only) sets how raises are paid (XP levels or points), perk
switches and refunds, enchantment locks, shared skills (off, parties or everyone) and how skills grow (buying, doing
or both). The rules are kept with the world.

Operator commands: `/skills set <player> <skill> <level>`, `/skills points <player> <skill> <n>`,
`/skills reset <player> [skill]`.

## For pack makers

Every skill, perk, focus area, lock and setting is a JSON file under `data/<namespace>/reskilled/<folder>/`
(`skills`, `perks`, `focus`, `locks`, `settings`, `guides`). The files this mod ships have priority 0. To change one, ship a
file with the same id and `"priority": 1` or more in your pack's data; lock files add up, and an entry with
`"replace": true` drops earlier locks on the same target. `settings` files merge field by field, highest priority
last. `/reload` applies changes, and gear a player may no longer wear comes off.

Perks that work by a mechanic rather than a number also have a guide: a short scene of blocks the skills screen
plays when a player presses G over the perk. Guides are files in the `guides` folder, one per perk, merged by `perk`
with the same priorities. A caption never types a perk's numbers: it has a `%s` for each of its `args`, filled from
the perk's effects, so a pack that retunes a perk keeps a true guide. Replant's, shortened:

```json
{
  "format": 1,
  "perk": "reskilled:gathering_sapling_keeper",
  "priority": 0,
  "seconds": 8,
  "view": {"center": [1.5, 1.0, 0.5], "yaw": 35, "pitch": 30, "scale": 24, "spin": 4},
  "steps": [
    {"at": 0, "place": [{"pos": [1, 0, 0], "block": "minecraft:farmland[moisture=7]"},
                        {"pos": [1, 1, 0], "block": "minecraft:wheat[age=7]"}]},
    {"at": 0, "caption": {"key": "reskilled.guide.gathering_sapling_keeper.1", "text": "A ripe crop is broken."}},
    {"at": 3, "remove": [[1, 1, 0]]},
    {"at": 3, "icon": {"item": "minecraft:wheat_seeds", "at": [0.62, 0.3]}},
    {"at": 4, "place": [{"pos": [1, 1, 0], "block": "minecraft:wheat[age=0]"}]},
    {"at": 4, "caption": {"key": "reskilled.guide.gathering_sapling_keeper.2",
                          "text": "It is planted again with one of its own seeds."}}
  ]
}
```

`view` sets where the scene is seen from: `center` (the scene point in the middle), `yaw` and `pitch` in degrees,
`scale` in pixels per block, and `spin` in degrees a second (0 holds still). Steps run in time order, each starting
before `seconds`, and the scene loops at `seconds`; a step does one thing. Besides `place`, `remove` and `caption`
there are `icon` (an item, `{"item": "minecraft:wheat_seeds", "at": [x, y]}`), `effect` (a potion effect's icon,
`{"effect": "minecraft:strength", "at": [x, y]}`), `hearts` (a heart bar of so many half hearts, at most 40) and
`clear` (`"icons"` or `"hearts"`). An overlay's `at` is fractions of the scene box, 0 to 1 from its top left. An arg
reads `{"effect": "crop_growth", "field": "chance", "as": "percent"}`, with `as` one of `number`, `percent`,
`seconds`, `blocks` or `hearts`. When it loads the data the server checks that each placed block and its property
keys exist and that each arg names a real value of the perk, and drops a bad file with an error. A property value
the block does not have (`age=9` on wheat) is only warned about in the client log, and that block is left out of the
scene. A perk without a guide shows its exact values instead, with its description under them.

Other mods can add their own unlockables, including abilities with no item behind them: see
[INTEGRATION.md](INTEGRATION.md).

## Building

The mod is two parts: `content/` (data and language files, a content mod) and `native/` (the rules, a native trait
mod pinned to one Horizon commit, see [HORIZON_PIN.md](HORIZON_PIN.md)).

- Tests: `cargo test --workspace` at the repository root.
- One file for players and the registry: `python3 -I reskilled/tools/package.py` writes `dist/reskilled.hmod`,
  then `cargo horizon-mod check dist/reskilled.hmod` validates it.
- Playing from source: `horizon-launcher native-build reskilled/native --instance <instance> --trust-native-code`,
  with the content mod built by `cargo horizon-mod build` in `content/`.
