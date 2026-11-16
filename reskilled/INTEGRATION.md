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

## Accessory slots (rings, amulets, belts)

Reskilled checks `equip` locks on the game's own worn slots. Items in your mod's own slots are out of its sight, so
ask it, the same optional way as for abilities:

```rust
// The player puts a stack into one of your accessory slots: accept it only when this is true.
#[cfg(feature = "reskilled")]
if !reskilled::may_equip(host, player, &stack) {
    return false; // refused; the player was told what they need ("Needs Agility 10")
}
```

`may_equip` counts the item's own `equip` locks and those its enchantments bring ("Protection III needs Agility 6"),
and lets through players whose locks are off. A player's levels can drop later (a reset, a death penalty, a pack's
new data on `/reload`), so check what they already wear now and then, for example when they log in and on your own
slot screen opening, with `may_wear`, which answers without a message:

```rust
#[cfg(feature = "reskilled")]
for (slot, stack) in worn_accessories(player) {
    if !reskilled::may_wear(host, player, &stack) {
        take_off_into_inventory(host, player, slot); // and tell the player as your mod does
    }
}
```

Lock your accessories in data like any item (`"target": {"item": {"id": "ringmod:swift_ring"}}`, `"actions":
["equip"]`), and say in Reskilled's settings which worn slot a shift-click puts a modded wearable in (`equip_slots`)
when it goes into one of the game's own slots.

## Reading a player's progress

```rust
#[cfg(feature = "reskilled")]
{
    let agility = reskilled::skill_level(host, player, "reskilled:agility"); // 1 at first, 0 for an unknown skill
    let has_it = reskilled::has_perk(host, player, "reskilled:agility_wind_runner");
}
```

Call these on the server. Skill and perk ids are the ones in the data, so a pack that renames them changes them.
