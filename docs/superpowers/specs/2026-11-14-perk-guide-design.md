# Perk guide: design

Date: 2026-11-14. Branch `feat/skills-and-perks`. Mod: Reskilled (`reskilled/`), on Minecraft Horizon (26.3).

## Intent

Players spend scarce perk points blind: a tooltip line like "Every 10 seconds, each crop within 5 blocks has a 1 in 4
chance to grow a stage" is hard to picture. The perk guide shows each perk in action, right where the points are
spent, so the player can judge whether it is worth buying. It is in the spirit of Create's Ponder (short animated
dioramas in a screen) but is not a copy of its look, layout, wording or name.

The user asked for: every perk explained; animated scenes only for the mechanic perks; a card for each number perk;
scenes as data files (approach A below).

## Goals

- From any perk on a skill page, the player opens that perk's guide page in one step and returns in one step.
- A mechanic perk's page plays a short animated 3D diorama with captions, looping, that can be replayed.
- A number perk's page is a card, generated from the perk's own effects, showing what changes (for attributes, the
  player's value now and with the perk).
- Scenes are data files packs and other mods can add or replace; a perk with no scene gets a card automatically.
- Locked and owned perks can both be viewed.

## Non-goals

- Guides for skills, locks, focus areas or the mod as a whole (later, if wanted).
- Mobs, players or other entities in scenes: Horizon's scene drawing takes blocks only.
- A scene editor, or importing scenes from saved structures.
- Sound or narration.

## Anti-goals (a failure even if it works)

- Looking or reading like Create's Ponder: no copied frame, colours, button set, keyword tags, wording or name.
- A guide that hides what the perk really does: every number shown comes from the perk's data, never typed into a
  scene by hand where it could drift.
- Anything that pauses or blocks play on a server; the guide is a client screen like the rest of Reskilled's.

## Constraints

- Horizon's `Host::draw_scene(batch, rect, blocks, view)` draws block states at scene positions into a screen box,
  with a view (`center`, `yaw`, `pitch`, `scale`); blocks only, client only, drawn from the screen's draw hook. It
  gives no projection back, so overlays cannot be pinned to a block's on-screen spot exactly.
- The Reskilled screen is one mod screen with views (`client::cache::View`: Overview, Skill, Settings); the guide is
  another view. It must fit the smallest GUI size, 320 by 240, as the other views do (the preview tests check this).
- The client gets definitions from the server (`ToClient` defs, chunked); data files live under
  `data/<namespace>/reskilled/<folder>/` with priorities (README "For pack makers").
- No em dash anywhere (repository rule). Visual UI is shown to the user as a preview picture before sign-off
  (`reskilled/tools/preview.py`).

## Approaches

- **A. Scenes as data, cards generated (chosen).** A scene is a JSON timeline the mod plays; cards are built from
  the perk's effects. Most up-front work (a timeline player and format), then each scene is only data; packs and
  other mods add scenes by shipping files and get cards for free. Chosen by the user; it matches the rest of
  Reskilled, where everything is data.
- B. Scenes written in Rust, cards generated. Quickest first scenes, but packs cannot add or change scenes and each
  tweak needs a rebuild.
- C. Scenes from saved structures plus a timeline. Nicest authoring, but needs reading vanilla structure files,
  which Horizon does not hand mods; heavy for about twelve scenes.

## Design

### Opening and leaving

- On the skill page, a perk's tooltip ends with a hint line: "Press G to see it in action" (the key is a Horizon mod
  key, `key.reskilled.guide`, default G, rebindable). Pressing it with the mouse over a perk row opens that perk's
  guide page (a new `View::Guide`, remembering the perk).
- The guide page has Back (to the skill page, as the right mouse button does elsewhere), Replay (scenes only), and
  arrows to step to the previous and next perk of the same skill.

### The page

A text sketch at 427 by 240 (the usual GUI size):

```
+----------------------------------------------------------+
| < Back            Replant                    [<]  [>]    |
|----------------------------------------------------------|
|                                                          |
|              [   3D scene, slowly turning   ]           |
|              [   farmland, wheat ripening   ]           |
|                                                          |
|   Breaking a ripe crop plants it again, using one of     |
|   its seeds.                                    (caption)|
|   ======----------  progress            [Replay]         |
|----------------------------------------------------------|
| Gathering 16, Agility 8   4 points   Owned / Locked      |
+----------------------------------------------------------+
```

A card page has the same frame; the scene box holds the card instead:

```
|   Steady Hand                                            |
|   Attack speed     4.0  ->  4.4      (+10%)              |
|   Swing 10% faster.                                      |
```

The footer shows the perk's needs and price as the tooltip does (green met, red not), and whether it is owned.

### Scene files

`data/<namespace>/reskilled/guides/<name>.json`, merged by `perk` with the usual priorities (a higher priority
replaces a lower one for the same perk), sent to clients with the other definitions.

```json
{
  "format": 1,
  "perk": "reskilled:gathering_sapling_keeper",
  "priority": 0,
  "seconds": 8,
  "view": {"center": [1.5, 0.5, 1.5], "yaw": 35, "pitch": 30, "scale": 22, "spin": 8},
  "steps": [
    {"at": 0, "place": [{"pos": [0, 0, 0], "block": "minecraft:farmland"},
                        {"pos": [0, 1, 0], "block": "minecraft:wheat[age=7]"}]},
    {"at": 0, "caption": {"key": "reskilled.guide.replant.1", "text": "A ripe crop is broken."}},
    {"at": 3, "remove": [[0, 1, 0]]},
    {"at": 3, "icon": {"item": "minecraft:wheat_seeds", "at": [0.55, 0.4]}},
    {"at": 4, "place": [{"pos": [0, 1, 0], "block": "minecraft:wheat[age=0]"}]},
    {"at": 4, "caption": {"key": "reskilled.guide.replant.2",
                          "text": "It is planted again with one of its own seeds."}}
  ]
}
```

- `seconds`: the loop length; at the end the scene starts again from an empty diorama.
- `view.spin`: degrees a second the view turns; 0 holds still.
- Steps, in time order, each with `at` in seconds:
  - `place`: blocks by state string (`minecraft:wheat[age=7]`, the game's own form), at scene positions;
  - `remove`: positions emptied;
  - `caption`: a translation `key` with English fallback `text` (a server pack's scene reads well on clients that
    lack its language file), and optional `args` filling the caption's `%s` in order, each naming a value of the
    perk's effects and how to show it: `{"effect": "crop_growth", "field": "chance", "as": "percent"}`, with `as`
    one of `number`, `percent` (0.25 reads 25%), `seconds`, `blocks` or `hearts` (health points halved). Shown under
    the scene until the next caption;
  - `icon`: an item drawn flat over the scene box at a point given as fractions of the box (0 to 1), until the
    next step that clears it (`"clear": "icons"`);
  - `effect`: a potion effect's icon, the same way (Bloodrush's Strength, Antidote's Poison shrinking);
  - `hearts`: a heart bar over the box, with a number of half hearts (Last Breath draining to 1).
- Captions never state a perk's numbers in their text; a caption that needs one uses `%s` and `args`, so packs that
  retune a perk keep a true guide. An argument naming an effect or field the perk does not have makes the file bad.
- The server checks a file when it loads the data, including that each block state string names a real state
  (it has `Host::default_state` and `Host::block_states()` too), and drops a bad file whole with an error, as other
  data files are. Clients trust what they are sent.
- There is no scrubbing: the timeline plays, loops, and Replay starts it again from an empty diorama.

### Cards

Built on the client from the perk's effects, one line each:

- `attribute`: the attribute's name, the player's value now (`Host::local_attribute`) and with this perk's modifier
  applied (taking the perk out first if it is owned), and the change in percent or points.
- `damage`: "Projectile damage dealt x1.08", and the same for taken.
- `drop_bonus`: "25% chance of an extra drop from logs".
- `mount_attribute`: as `attribute`, labelled "your mount", with no value now.
- `enchanted_weapon` and `enchanted_armor`: "+3% damage per enchantment on your weapon, up to 15%" and the armor
  equivalent. `shorter_effects`, `rune_repair`, `last_stand` and the other mechanic effects get a line of their own
  numbers too (for example "Harmful effects last 60% as long").
- A scene page shows the perk's card lines too, under the caption, as many as fit; the scene is the picture and the
  lines are the exact values.
- Any effect with no card line (a mechanic perk with no scene file, or an effect type a pack added): the perk's
  description, wrapped.

### The scenes to ship

One scene each for Last Breath, Bloodrush, Pearl Step, Green Aura, Replant, Antidote, Living Runes, Salvage,
Restful Sleep, Hearth, Home Ward and Safe Haven, with English captions. Scenes without mobs lean on overlays:
Last Breath shows a heart bar falling to half a heart beside a cliff of blocks; Bloodrush shows a sword icon then
the Strength effect icon; Home Ward shows a house of blocks, a bed, a blast's blocks coming back untouched inside
the ring and gone outside it.

## Left to the builder

- Resolving a state string such as `minecraft:wheat[age=7]` to a `BlockStateId` (server to check, client to draw):
  `Host::default_state` gives a block's default; the property lookup may need `Host::block_states()` searched by
  block and properties. Check before writing the format's parser, and say if it cannot be done.
- The exact layout numbers, sizes at 320 by 240, and the colours, keeping to Reskilled's existing palette.
- How the timeline is driven (client ticks since the page opened) and how a step at a time is rebuilt when the
  scene loops or the player steps back.
- The wording of captions and card lines, under the repository's plain-English and no em dash rules.

## Calls made in this design (not asked of the user)

- Opening by a key while hovering (G), not a click, because clicks on perk rows already buy, switch and refund.
- Scenes are server data sent with the definitions, not client assets, so a server's pack can change them and every
  player sees the same guide.
- Captions fill numbers from the perk's data rather than typing them, chosen by `args`.
- Captions carry English fallback text; the server checks scene files, block states included, and drops bad ones
  whole; scene pages also show the card lines; no scrubbing (all settled after the builder check).
