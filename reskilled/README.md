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
(`skills`, `perks`, `focus`, `locks`, `settings`). The files this mod ships have priority 0. To change one, ship a
file with the same id and `"priority": 1` or more in your pack's data; lock files add up, and an entry with
`"replace": true` drops earlier locks on the same target. `settings` files merge field by field, highest priority
last, except the lists `not_creatures`, `projectile_damage` and `equip_slots`, which add up. A file with the same id,
a higher priority and `"remove": true` removes a skill, perk or focus area. Each skill's `earned_by` says what work
grows it. `/reload` applies changes, and gear a player may no longer wear comes off. Every field is in the
[data reference](claude-skill/reskilled-modding/reference.md).

Other mods can add their own unlockables, including abilities with no item behind them: see
[integration.md](claude-skill/reskilled-modding/integration.md).

### Customizing with Claude

Reskilled ships a Claude Code skill that knows its data format, the override and removal rules, and the usual
mistakes. Copy the folder `claude-skill/reskilled-modding` into `~/.claude/skills/` (every project) or into your
pack's `.claude/skills/` (that pack only), then ask Claude for the change ("lock my mod's drill behind Mining 12",
"add a Fishing skill with three perks", "remove Keen Eye"). Other agents can read the same folder:
[SKILL.md](claude-skill/reskilled-modding/SKILL.md) and its
[field reference](claude-skill/reskilled-modding/reference.md). The skill's example pack is loaded by the mod's
tests, so it always matches the format the game reads.

## Building

The mod is two parts: `content/` (data and language files, a content mod) and `native/` (the rules, a native trait
mod pinned to one Horizon commit, see [HORIZON_PIN.md](HORIZON_PIN.md)).

- Tests: `cargo test --workspace` at the repository root.
- One file for players and the registry: `python3 -I reskilled/tools/package.py` writes `dist/reskilled.hmod`,
  then `cargo horizon-mod check dist/reskilled.hmod` validates it.
- Playing from source: `horizon-launcher native-build reskilled/native --instance <instance> --trust-native-code`,
  with the content mod built by `cargo horizon-mod build` in `content/`.
- Before a release: the in-game checklist in [TESTING.md](TESTING.md).
