# Adding your mod's unlockables to Reskilled

Reskilled locks gear, blocks, mounts and abilities behind skill levels. Your mod can join in two ways, and neither
makes Reskilled required: without it installed, your mod plays as it always did.

## 1. With data only (items, blocks, entities)

Ship a lock file in your mod's or pack's data, in your own namespace:
`data/<your namespace>/reskilled/locks/<name>.json`.

```json
{
  "format": 1,
  "entries": [
    {
      "target": { "item": { "id": "yourmod:obsidian_sword" } },
      "actions": ["attack"],
      "requires": { "reskilled:melee": 20 },
      "label": "yourmod.lock.obsidian_sword"
    }
  ]
}
```

- Targets: `item`, `block`, `entity`, `enchantment` and `ability`. Items and blocks match by `id`, `tag`
  (`#yourmod:swords`), `pattern` (`yourmod:*_sword`), `repairable` (items only), `all_of` and `any_of`.
- Actions: items `use`, `attack`, `break`, `equip`; blocks `open`, `place`; entities `mount`, `interact`;
  enchantments the item actions; abilities `use`.
- `label` is a translation key; it shows in the skill's "Next unlocks" list. Put its English in your language file.
- You can also add skills, perks and focus areas the same way (`reskilled/skills/`, `reskilled/perks/`,
  `reskilled/focus/`). A pack overrides a shipped file with the same id and a higher `priority`.

## 2. Abilities, with a few lines of code (a dash, a double jump)

An ability has no item, block or entity behind it, so the lock names it by your mod's id.

**Lock it in data**, as above:

```json
{
  "format": 1,
  "entries": [
    {
      "target": { "ability": { "id": "dashmod:dash" } },
      "actions": ["use"],
      "requires": { "reskilled:agility": 10 },
      "label": "dashmod.lock.dash"
    }
  ]
}
```

**Ask Reskilled before running it.** Reskilled is a native trait mod with an `[api]` package. Declare it as an
optional dependency in your bundle's `horizon-mixin.toml` (Horizon's "Dependencies between trait mods"):

```toml
[dependencies]
reskilled = { version = "^0.1", optional = true }
```

The launcher adds the crate and a Cargo feature named `reskilled`, enabled only when Reskilled is installed. Do not
add them to your `Cargo.toml` yourself. Then, on the server, before your ability runs:

```rust
fn try_dash(host: &mut dyn Host, player: u128) {
    #[cfg(feature = "reskilled")]
    if !reskilled::may_use_ability(host, player, "dashmod:dash") {
        return; // the player was told what they need ("Needs Agility 10")
    }
    // ... dash ...
}
```

`may_use_ability` is true when no lock names the ability, when the player has the levels, or when locks are off
for them (creative or spectator, by Reskilled's settings). When it is false, the player gets the same warning as
for any other lock. A pack can retune or remove your lock in its own data.

## Reading a player's progress

```rust
#[cfg(feature = "reskilled")]
{
    let agility = reskilled::skill_level(host, player, "reskilled:agility"); // 1 at first, 0 for an unknown skill
    let has_it = reskilled::has_perk(host, player, "reskilled:agility_wind_runner");
}
```

Call these on the server. Skill and perk ids are the ones in the data, so a pack that renames them changes them.
