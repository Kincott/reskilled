# Reskilled: in-game checklist

The unit and hook tests cover the rules; this list covers what only the game shows. Run it on the commit you mean to
release, in a fresh world, then on a dedicated server.

## Build

```sh
cd reskilled/content && cargo horizon-mod build && cd ../..
~/Applications/horizon/horizon-launcher native-build reskilled/native --instance skills-dev --trust-native-code
```

Start the game with the content mod (without `--dev-mod` the log says "Reskilled loaded 0 skills" and the screen
is empty):

```sh
~/Applications/horizon/horizon-launcher play --instance skills-dev --world reskilled-test --creative \
  --arg=--dev-mod --arg="$PWD/reskilled/content/target/horizon-mod/reskilled.hmod"
```

In the new world, `/gamemode survival` and `/xp add @s 200 levels`.

## Single player

Screen and raising
- [ ] K opens the skills screen; `/skills open` does too. All eight skills show, Building among them.
- [ ] A raise takes the shown price in XP levels. Shift-click raises 5, Ctrl-click raises as many as you can pay.
- [ ] Holding Shift or Ctrl shows where the raise leads and its price.
- [ ] Clicking raise many times fast never takes more levels than the skill went up by (the same-tick fix).
- [ ] A focus area can be added; the first is free.

Perks
- [ ] Buying a perk spends its points; switching off and refunding work as the server rules say.
- [ ] Hovering a perk shows its description.

Locks
- [ ] An iron pickaxe's tooltip shows the padlock and "Mining 8"; breaking with it below 8 is refused with that line.
- [ ] Locked armor: right-click, shift-click, number key and dragging onto the slot each end with it off and back
      where it came from.
- [ ] Full inventory, locked chestplate worn, `/skills reset @s`: it drops at your feet, and only one drops.
- [ ] Locked mount and locked menu (e.g. an enchanting table) are refused.
- [ ] `/gamemode creative` ignores locks; going back to survival takes off locked gear worn meanwhile.

Mechanic perks
- [ ] Salvage: a placed glass block drops glass. A placed shulker box with items in it drops with its items.
      A placed bed and a placed door each drop one, whichever half is broken.
- [ ] Replant: a ripe wheat crop broken comes back as a seedling, one seed fewer in the drops.
- [ ] Last Stand: a lethal blow at high health leaves you at half a heart, once per cooldown.
- [ ] Home perks: set a bed, then check Hearth near it, Home Ward against a creeper, Safe Haven at night.
- [ ] Restful Sleep: a full night's sleep heals you and clears the listed effects.

Server rules and persistence
- [ ] The gear on the screen opens Reskilled Options; Server rules change in game and survive a reload of the world.
- [ ] Skills survive quitting and reopening the world, `/reload`, and death.
- [ ] Under "doing" earning, mining with a pickaxe climbs Mining; placing and breaking one block repeatedly does not
      climb Building.

## Dedicated server

```sh
~/Applications/horizon/horizon-launcher native-build reskilled/native --server-output <dir> --trust-native-code
```

- [ ] A client with the mod joins: the screen, tooltips and raises work as in single player.
- [ ] A second client without the mod joins: `/skills` lists its skills, `/skills raise mining` works, and a locked
      action is refused with a red "Needs" line on the action bar.
- [ ] Parties: `/skills party create`, `invite`, `join`; with sharing on "parties", the lower player sees the higher
      level, and gets their own back on `leave`.
- [ ] A non-operator cannot change the server rules; an operator can.
- [ ] The server log has no Reskilled warnings.
