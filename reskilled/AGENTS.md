# Reskilled: notes for coding agents

Reskilled is a skills, perks and gear-lock mod for Minecraft Horizon (Java Edition 26.3, rewritten in Rust).

## Customizing it (a pack or another mod)

Use the `reskilled-modding` skill in [claude-skill/reskilled-modding/](claude-skill/reskilled-modding/SKILL.md):
the rules, the usual tasks, a field-by-field [reference](claude-skill/reskilled-modding/reference.md) and a
complete [example pack](claude-skill/reskilled-modding/example-pack/). Changes go in the pack's own namespace,
never in `content/data/reskilled/`.

## Working on the mod itself

- `content/`: the shipped data and English (`assets/reskilled/lang/en_us.json`). `native/core`: data model,
  parsing, merging, rules (no game code; most tests). `native/api`: the game hooks, server and client.
- Every behavior that names content (skills, items, entities, damage types) belongs in data, not code: other mods
  must be able to reach it. Settings lists add up across files; definitions override by priority.
- Tests: `cargo test --workspace` and `cargo clippy --workspace --all-targets` at the repository root, both clean.
  `native/core/tests/shipped_defaults.rs` holds the shipped data to its intended values;
  `native/core/tests/example_pack.rs` keeps the skill's example pack loading cleanly, so change both with the data
  format.
- English strings the server sends live in `native/api/src/text.rs` with their keys; a test keeps them equal to
  `en_us.json`.
- The mod pins one Horizon commit ([HORIZON_PIN.md](HORIZON_PIN.md)). Building it into the game
  (`horizon-launcher native-build ... --trust-native-code`) runs its code with the user's rights: leave that command
  to the user.
