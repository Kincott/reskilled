# Reskilled: the road interface

- Date: 2026-11-16
- Status: direction approved by the user from the mockups below; this document waits for the user's review
- Replaces: the skills screen's overview and skill view, and the options screen (spec 7.7, 7.11 display parts)

The user asked for an overhaul, not a cosmetic pass: after a refined version of the current cards, a skill wheel
(rejected) and a character sheet, they approved the "road" design. Mockups, drawn with the game's own font, icons and
GUI sprites at the mod's window size (427 x 240 GUI pixels) by `reskilled/tools/mockups/road.py`:

- [A skill's road](2026-11-16-road-ui/road_skill.png)
- [Home](2026-11-16-road-ui/road_home.png)
- [Options](2026-11-16-road-ui/road_options.png)

## 1. Idea

Each skill is a road from level 1 to its top level. What the skill gives is laid out where it is earned: the gear a
level unlocks above the road, the perks below it. The player sees where they are, what the next levels bring, and
what to spend points on, without reading a list. The look stays the mod's own (dark slate body, brass frame, gold
text) and borrows vanilla's advancement screen: its frames for perks, its stone backdrop for the road, tabs along the
top.

## 2. The window

- One window for everything, with tabs above it: **Home** (the player's face), then one tab per skill (its icon, in
  id order), and **Options** (a gear) at the right end. The open tab joins the window's frame.
- A skill tab carries a small gold pip while the skill has perk points to spend.
- K opens the window on Home; `/skills open` too. Escape closes it from any tab.
- The window is centred and fills the GUI height less a margin; its width follows the GUI width up to 383 pixels.
  At the smallest GUI (320 x 240) everything still fits: the road compresses, tabs narrow (they may drop to
  icon-only 24-pixel tabs), and no text overlaps or leaves the window.
- Packs with more than eight skills: tabs that do not fit scroll with arrow tabs at both ends.

## 3. Home

Left column, "you":

- The player's head in a gold goal frame. It is drawn as a `minecraft:player_head` item carrying the player's
  profile; if Horizon does not draw the skin in a screen, the default head shows instead (to be checked in game).
- Name, then "Total level N" (the sum of all skill levels).
- **Gear**: the player's real head, chest, legs, feet, main-hand and off-hand items in slots. An item the player may
  not fully use gets a padlock and, under the slots, the first unmet need of the hovered or first locked item
  ("✘ Pickaxe: Mining 16"). Hovering a slot shows the item's own tooltip (with its requirement lines).
Right, one row per skill, on one line:

- Accent stripe, icon, name, "Lv N".
- A mini road: a line from 1 to the top level, gold up to the player's level, with a stop per perk (gold owned, green
  buyable, dark otherwise).
- Unspent points (◆N), and a raise button showing its price with the arrow; Shift and Ctrl raise 5 or all, with the
  same hints as today. Mastered skills show ★ Mastered in place of the button and an amber row.
- Clicking a row opens its skill's tab. Hover brass-outlines the row.
- Under the rule that skills grow by doing, the raise button gives way to the earning progress ("37/120").

Along the bottom, the full width of the window (moved there from under the gear at the user's request: it was
cramped):

- **Focus**: a gold FOCUS label, then the focus areas as buttons sharing the width, active ones green with a tick;
  the hover tooltip stays as today (what it gives XP for, what adding it costs).

## 4. A skill's road

Header:

- The skill's icon in a goal frame, its name, "Level N of M", unspent perk points, the player's XP (chip with an XP
  bottle), and **Level up · N XP** (the price in the rule's unit; greyed when it cannot be paid; Shift and Ctrl as
  above; "Mastered" when at the top). A shared level shows as today ("Shared level. Yours: N").

The road, on vanilla's stone advancement backdrop darkened:

- A line from level 1 to the top level, gold up to the player's level, with level plates every 4 levels (and at 1
  and the top), the player's level highlighted with a marker above it.
- **Above**: each lock tier labelled for this skill, as a slot with its icon at its level, joined to the road; tiers
  not reached are dimmed with a padlock. Hovering one names what it unlocks (the lock's label) and at what level.
- **Below**: each perk at its `min_level`, in an advancement frame (task frame; the skill's last perk in the
  challenge frame): gold "obtained" frame with a tick when owned, a green outline when it can be bought now, dimmed
  with a padlock otherwise, and its price in ◆ under it. Two perks at one level sit side by side. Perks requiring
  another perk are joined to it by a thin line under the road.
- **At the end**: mastery, in a challenge frame (the skill's icon, gold when reached).
- Wider than the window (a top level above 32, many tiers): the road scrolls sideways with the wheel.

The dock, under the road, describes what is hovered (else the next thing to unlock):

- A perk: frame and icon, name, "Mining perk · level 8", the description as bullets with numbers picked out, its
  needs ticked or crossed (level, other skills, required perks, points), and its action: **Unlock** when buyable;
  **Area** (show its ground in the world) and **On/Off** (when switches are allowed) when owned.
- A gear tier: its label, the level and the skill it needs, and the player's progress toward it.
- Clicking a perk node selects it in the dock so the buttons can be reached without holding the hover.

## 5. Options

Same window, Options tab. Two columns:

- **Display** (every player): sections Lock warning (position, time), Lock icon (padlock, corner, dim), Tooltips
  (hide met, your level), Skill strip, Sounds (clicks, level-up, refusal). Each option is a row with its value as a
  small button at the right, green when on.
- **Server rules** (operators): raise cost, points a level, skills grow by, earning pace, perk switches, enchantment
  locks, shared skills. For other players the rows are greyed and a padlock line says "Only operators can change
  these."
- A help box shows the hovered option's explanation. **Defaults** resets the display options; **Done** returns to
  the tab the player came from.
- At the smallest GUI the columns stack and scroll.

## 6. Data

- Perks get an optional `icon` (an item id) for their node and dock; without one, the skill's icon is used. The
  shipped perks each get one.
- Lock entries get an optional `icon` (an item id) shown above the road with their label; entries without an icon
  stay off the road (they still show in item tooltips and refusals). The shipped tier, block and special locks get
  icons (the golden, copper, iron, diamond and netherite tool of the skill; the block; the item).
- Skills get an optional `color` (hex RGB) for their stripe and accents; the shipped skills get one each, and a skill
  without one takes a colour from its id.
- Clients receive these with the definitions as today. The format number stays 1: the fields are optional.
- The modding skill's reference and example pack document and use the new fields.

## 7. Kept as they are

The skill strip beside the inventory, the lock warning panel, item tooltips and the locked-item padlock, the in-world
perk areas, the sounds, and every request, rule and check behind the buttons. Only how the screens are drawn and
laid out changes; the click handling keeps sending the same requests.

## 8. Testing

- The preview tests draw Home, every shipped skill's road and Options at 320 x 240 and 427 x 240, and check that
  every text and slot stays inside the window and no two texts overlap.
- Behaviour tests (raising, buying, switching, focus, areas, options, rules) are carried over to the new layout's hit
  areas.
- In game: the player head with the player's skin, the stone backdrop, tab switching, scrolling a long road, and an
  operator and a player on the rules column.
