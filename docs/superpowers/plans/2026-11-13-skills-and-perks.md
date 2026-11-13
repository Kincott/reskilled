# Skills & Perks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Skills & Perks, a Minecraft Horizon mod where players spend XP levels on eight skills, earn perk
points, pick a focus, and need skill levels to use gear, with a skills screen, tooltips and a `/skills` command.

**Architecture:** One mod folder holding a content part (`horizon-mod.toml`, `assets/`, `data/`) and a native trait
mod bundle. The bundle has a pure-logic crate (`skills_core`, no `horizon-client`, holds nearly all rules and most
tests) and a thin trait mod package (`api`) that adapts Horizon's hooks and helpers to the core. Everything ships as
one `.hmod`.

**Tech Stack:** Rust (edition 2024), Horizon trait mod API (`horizon_client::mod_api`), serde and serde_json from
Horizon's `Cargo.lock`, Python 3 (standard library only) for packaging.

**Spec:** `docs/superpowers/specs/2026-11-13-skills-and-perks-design.md`

## Global Constraints

- Platform: Minecraft Horizon on Java Edition 26.3. Pin one Horizon commit (40 hex characters) in Task 1 and use it
  everywhere a commit is named.
- Mod id `skills_and_perks`, display name "Skills & Perks", version `0.1.0`.
- `skills_core` must not depend on `horizon-client`. Both crates may only use crates and versions in Horizon's
  `Cargo.lock` at the pinned commit (at `298c01b`: serde 1.0.229, serde_json 1.0.151, thiserror 2.0.21, log
  0.4.34). Pin them with `=` versions.
- Under the bundle folder, only `.rs` and `.toml` files, `data/` folders, and README/LICENSE files (Horizon's
  `docs/api/mod-files.md`). No `Cargo.lock` inside the bundle.
- Channels: `skills_and_perks:defs` (definition chunks) and `skills_and_perks:msg` (every other `ToClient` message)
  from server to client, `skills_and_perks:act` from client to server. At most 30,000 bytes per message and 32
  messages a tick per player.
- Data folders: `skills_and_perks/skills`, `skills_and_perks/perks`, `skills_and_perks/focus`,
  `skills_and_perks/locks`, `skills_and_perks/settings`, read from every namespace.
- Defaults: max level 32; cost `base + floor((L - 1) / every)` with base 2, every 4; 1 perk point at every 4th
  level; focus discount 25% (discount rounded up, cost at least 1); first focus free, switch 10 levels; death penalty
  0; creative and spectator ignore locks; lock message cooldown 20 ticks.
- Lock ladder: copper 4, iron 8, diamond 16, netherite 24. Specials: mace Melee 20, trident Ranged 16, crossbow
  Ranged 6, elytra Agility 16, shield Defense 4, wolf armor Taming 6, happy ghast mount Taming 12, other mounts
  Taming 4, enchanting table Arcane 8, anvil Arcane 6, brewing stand Arcane 4.
- Originality: no names, perk names, descriptions, icons, layouts or text from Reskillable or Level Up! Reloaded. Do
  not read their code or assets while building.
- All player-facing text is a translation key with English in `assets/skills_and_perks/lang/en_us.json`.
- No em dash (U+2014) in any file or commit message (user convention).
- `--trust-native-code` is the user's consent: only the user runs `horizon-launcher native-build`. Agents never pass
  that flag.
- Commit only with the user's go-ahead for this plan, on a feature branch, never on `main`.

## Review Focus

- A pack raises a requirement with `/reload` while a player wears that armor: it must come off after the reload, not
  only on the next equip. Test: Task 10, `reload_rechecks_equipment`.
- A client spams raise requests with 0 XP levels: every request is refused and levels never go negative. Test:
  Task 9, `raise_without_levels_is_refused_and_takes_nothing`.
- Unequipping locked armor with a full inventory: the item drops at the player's feet, never vanishes. Test:
  Task 10, `full_inventory_drops_the_unequipped_item`.
- Definitions bigger than one message, or chunks arriving after a newer send started: the client keeps only a
  complete, current set. Test: Task 6, `stale_chunks_are_ignored` and Task 12, `defs_reassemble_across_messages`.
- A death penalty pushes a skill below a perk's minimum level: the perk stays bought, stops working, and its
  attribute modifier is removed. Test: Task 5, `perk_below_min_level_is_inactive`, Task 6,
  `inactive_perk_yields_remove_command` and Task 11, `inactive_perk_modifier_is_removed`.

---

## File structure

```
Cargo.toml                                   dev workspace: members core and api (not shipped)
rust-toolchain.toml                          toolchain pin (Task 1)
.gitignore                                   /target, /dist
skills-and-perks/
  HORIZON_PIN.md                             pinned commit, hook API number, capability table (Task 1)
  content/
    horizon-mod.toml                         content manifest
    assets/skills_and_perks/lang/en_us.json  all English text
    data/skills_and_perks/skills_and_perks/{skills,perks,focus,locks,settings}/*.json   defaults
  native/                                    the bundle (becomes mixins/skills_and_perks/ in the .hmod)
    horizon-mixin.toml
    core/Cargo.toml                          package skills_core
    core/src/lib.rs                          module list and re-exports
    core/src/defs.rs                         data file model and parsing
    core/src/registry.rs                     merging files into effective definitions
    core/src/progress.rs                     player state, costs, points, requests
    core/src/locks.rs                        matchers, requirements, checks
    core/src/effects.rs                      active perk effects, attribute command diff
    core/src/wire.rs                         save format and network messages
    core/tests/shipped_defaults.rs           validates content/data defaults
    api/Cargo.toml                           package mod-skills_and_perks
    api/src/lib.rs                           hooks(), the Mod struct, dispatch to modules
    api/src/server/{data.rs,state.rs,actions.rs,commands.rs,enforce.rs,effects.rs,queue.rs}
    api/src/client/{cache.rs,tooltip.rs,screen.rs,layout.rs,keys.rs}
  tools/package.py                           builds dist/skills_and_perks.hmod
  tools/test_package.py                      checks the archive layout
```

---

### Task 1: Toolchain, Horizon pin and a greeting trait mod that runs in game

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `skills-and-perks/HORIZON_PIN.md`,
  `skills-and-perks/native/horizon-mixin.toml`, `skills-and-perks/native/core/Cargo.toml`,
  `skills-and-perks/native/core/src/lib.rs`, `skills-and-perks/native/api/Cargo.toml`,
  `skills-and-perks/native/api/src/lib.rs`, `skills-and-perks/content/horizon-mod.toml`,
  `setup/install-rustup.sh`

**Interfaces:**
- Produces: `api` exports `pub fn hooks() -> Box<dyn horizon_client::mod_api::Hooks>`; `skills_core` crate name;
  `HORIZON_PIN.md` with the commit later tasks use.

- [ ] **Step 1: Write `setup/install-rustup.sh`** (`apt-get update` then `apt-get install -y rustup`, nothing else)
  and ask the user to run `! sudo bash setup/install-rustup.sh`. Then, without sudo, run `rustup default stable` and
  `cargo install --git https://github.com/horizon-rust/horizon --rev <pin from Step 3> cargo-horizon-mod` (after
  Step 3).
- [ ] **Step 2: Ask the user to install the Horizon launcher** from the tarball on GitHub releases into
  `~/Applications/horizon/`, sign in, run Prepare (or `horizon-launcher prepare --download`), install the newest
  version, and create an instance named `skills-dev` (`horizon-launcher instance create skills-dev`).
- [ ] **Step 3: Record the pin.** Find the commit of the version the instance plays; write it, the matching
  `NATIVE_HOOKS_API` number and a capability table to `skills-and-perks/HORIZON_PIN.md`. The table lists every hook
  and helper named in spec section 2 with "present" or "missing" at that commit (check `native-hooks.json` and the
  generated `horizon_client::mod_api` docs). A missing item uses the fallback the spec names, or stop and report.
- [ ] **Step 4: Write the bundle skeleton** from Horizon's native template: `horizon-mixin.toml` with `[mod] id =
  "skills_and_perks"`, `version = "0.1.0"`, `[compatibility] horizon_revision = "<pin>"`, `[api] path = "api"`, and
  `[[crate]] id = "skills_core"`, `path = "core"`. `api/Cargo.toml`: package `mod-skills_and_perks`, edition 2024,
  `horizon-client = { git = "https://github.com/horizon-rust/horizon", rev = "<pin>" }`, `skills_core = { path =
  "../core" }`. `api/src/lib.rs`: the template's greeting in `player_logged_in`, text "Skills & Perks loaded".
  `rust-toolchain.toml`: `channel = "1.95.0"` (the version Horizon's native mod template validates), or newer if the
  pinned commit's README asks for it.
- [ ] **Step 5: Write `content/horizon-mod.toml`** from Horizon's content template: id `skills_and_perks`, name
  "Skills & Perks", version `0.1.0`, description "Skills, perks and gear locks for Horizon. Fills the role of
  Reskillable and Level Up! Reloaded on Horizon (not affiliated).", api `"0.2"`, side `"both"`, capabilities
  `["registries"]`.
- [ ] **Step 6: Compile.** Run `cargo build -p skills_core -p mod-skills_and_perks`. Expected: builds (the first build
  of `horizon-client` is slow).
- [ ] **Step 7: In-game smoke test (user runs).** Ask the user to run `horizon-launcher native-build
  skills-and-perks/native --instance skills-dev --trust-native-code --profile dev`, start the instance and join a
  world. Expected: chat shows "Skills & Perks loaded".
- [ ] **Step 8: Commit** `chore: scaffold skills and perks mod pinned to horizon <short pin>`.

### Task 2: Data file model and parsing

**Files:**
- Create: `skills-and-perks/native/core/src/defs.rs`
- Modify: `skills-and-perks/native/core/src/lib.rs`

**Interfaces:**
- Produces (all derive `Debug, Clone, PartialEq, Serialize, Deserialize` with `#[serde(deny_unknown_fields)]`;
  `Action` also derives `Copy, Eq, Ord, Hash`; ids are `String` in `namespace:path` form):
  - `pub struct SkillDef { id, priority: i32 (default 0), name: String, icon: String, max_level: Option<u32>, cost: Option<CostCurve>, points_every: Option<u32> }`
  - `pub struct CostCurve { base: u32, every: u32 }`
  - `pub struct PerkDef { id, priority, skill: String, name: String, description: String, cost: u32, min_level: u32, requires: Vec<String>, effects: Vec<Effect> }`
  - `pub enum Effect` (tag `type`, snake case): `Attribute { attribute: String, amount: f64, operation: Operation }`, `DropBonus { target: Matcher, chance: f64 }`, `Damage { direction: Direction, kind: DamageKind, multiplier: f64 }`, `XpGain { multiplier: f64 }`, `AnvilCost { multiplier: f64 }`, `MountAttribute { attribute: String, amount: f64, operation: Operation }`
  - `pub enum Operation { AddValue, AddMultipliedBase, AddMultipliedTotal }` (serialized `add_value` etc., the names `/attribute` uses)
  - `pub enum Direction { Dealt, Taken }`, `pub enum DamageKind { Melee, Projectile, Fall, Fire, Explosion, Any }`
  - `pub struct FocusDef { id, priority, name: String, skills: Vec<String> }`
  - `pub struct LockFile { entries: Vec<LockEntry> }`, `pub struct LockEntry { target: Target, actions: Vec<Action>, requires: BTreeMap<String, u32>, label: Option<String>, replace: bool (default false) }`
  - `pub enum Target { Item(Matcher), Block(Matcher), Entity(Matcher) }` (externally tagged: `{"item": ...}`)
  - `pub enum Matcher { Id(String), Tag(String), Pattern(String), Repairable(String), AllOf(Vec<Matcher>), AnyOf(Vec<Matcher>) }`
  - `pub enum Action { Use, Attack, Break, Equip, Open, Place, Mount, Interact }`
  - `pub struct SettingsPatch` with `priority: i32` and every `Settings` field as `Option`
  - `pub struct Settings { max_level: u32, cost: CostCurve, points_every: u32, focus_discount_percent: u32, focus_first_free: bool, focus_switch_cost: u32, death_penalty_levels: u32, creative_ignores_locks: bool, lock_message_cooldown_ticks: u32 }` with `Default` = the Global Constraints values
  - `pub enum DefKind { Skill, Perk, Focus, Lock, Settings }`, `pub enum DefFile { Skill(SkillDef), Perk(PerkDef), Focus(FocusDef), Lock(LockFile), Settings(SettingsPatch) }`
  - `pub fn parse_file(kind: DefKind, json: &str) -> Result<DefFile, DefError>`; every file has `"format": 1` (other values refused)
  - `pub struct DefError { pub file: String, pub reason: String }` (thiserror, `Display` "<file>: <reason>")

- [ ] **Step 1: Write failing tests** in `defs.rs` (`#[cfg(test)]`): `parses_a_skill`, `parses_a_perk_with_each_effect_type`, `parses_a_lock_file_with_nested_matchers`, `parses_a_settings_patch`, `refuses_unknown_fields`, `refuses_format_2`, `refuses_bad_id` (`"Mining"` without a namespace, uppercase), `refuses_negative_or_nan_amounts`, `settings_default_matches_spec` (asserts every default listed in Global Constraints).
- [ ] **Step 2: Run** `cargo test -p skills_core defs::` and expect failures (missing items).
- [ ] **Step 3: Implement** the types and `parse_file`. Id rule: `^[a-z0-9_.-]+:[a-z0-9_./-]+$`, checked by hand (no regex crate needed).
- [ ] **Step 4: Run** `cargo test -p skills_core defs::`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): parse skills, perks, focus, locks and settings files`.

### Task 3: Merging files into effective definitions

**Files:**
- Create: `skills-and-perks/native/core/src/registry.rs`

**Interfaces:**
- Consumes: Task 2 types.
- Produces:
  - `pub struct SourceFile { pub path: String, pub order: usize, pub kind: DefKind, pub json: String }` (`path` is `namespace:folder/name`, `order` is load order)
  - `pub struct Defs { pub skills: BTreeMap<String, SkillDef>, pub perks: BTreeMap<String, PerkDef>, pub focus: BTreeMap<String, FocusDef>, pub locks: Vec<LockEntry>, pub settings: Settings }` (derives `Debug, Clone, Default, Serialize, Deserialize`)
  - `pub fn build(files: Vec<SourceFile>) -> (Defs, Vec<DefError>)`
  - `impl Defs { pub fn skill(&self, id: &str) -> Option<&SkillDef>; pub fn max_level(&self, skill: &str) -> u32; pub fn cost_curve(&self, skill: &str) -> CostCurve; pub fn points_every(&self, skill: &str) -> u32; pub fn focus_of(&self, skill: &str) -> Option<&str> }`

- [ ] **Step 1: Write failing tests:** `higher_priority_wins`, `tie_goes_to_later_file`, `replace_drops_earlier_entries_for_the_same_target`, `settings_merge_field_by_field_by_priority`, `perk_with_unknown_skill_is_dropped_with_error`, `perk_requiring_unknown_perk_is_dropped_with_error`, `focus_with_unknown_skill_is_dropped_with_error`, `lock_requiring_unknown_skill_is_dropped_with_error`, `skill_in_two_focus_areas_is_an_error` (the later one is dropped), `bad_file_is_reported_and_others_still_load`.
- [ ] **Step 2: Run** `cargo test -p skills_core registry::`; expect failures.
- [ ] **Step 3: Implement** `build`: parse each file (errors collected), resolve each id to the highest `priority` then the highest `order`, apply lock files in order (`replace: true` removes earlier entries with an equal `target`), fold settings patches lowest priority first, then drop entries whose references don't resolve (one `DefError` each).
- [ ] **Step 4: Run** the tests; expect PASS.
- [ ] **Step 5: Commit** `feat(core): merge data files by priority with reference checks`.

### Task 4: Lock matching and requirement checks

**Files:**
- Create: `skills-and-perks/native/core/src/locks.rs`

**Interfaces:**
- Consumes: `Matcher`, `Target`, `Action`, `LockEntry`, `Defs`.
- Produces:
  - `pub trait Facts { fn item_in_tag(&self, item: &str, tag: &str) -> bool; fn block_in_tag(&self, block: &str, tag: &str) -> bool; fn item_repairable(&self, item: &str) -> Option<String>; }` (`item_repairable` is the default `minecraft:repairable` component's `items` value, a `#tag` or an item id)
  - `pub enum TargetKind { Item, Block, Entity }`
  - `pub struct Requirements { pub by_action: BTreeMap<Action, BTreeMap<String, u32>> }` (empty means unlocked;
    derives `Debug, Clone, Default, PartialEq, Serialize, Deserialize`)
  - `pub fn matches(m: &Matcher, kind: TargetKind, id: &str, facts: &dyn Facts) -> bool`
  - `pub fn requirements_for(defs: &Defs, kind: TargetKind, id: &str, facts: &dyn Facts) -> Requirements` (all matching entries; for each action and skill keep the highest level)
  - `pub struct LockCache` with `pub fn get(&mut self, defs: &Defs, kind: TargetKind, id: &str, facts: &dyn Facts) -> &Requirements` and `pub fn clear(&mut self)`
  - `pub struct Unmet { pub skill: String, pub level: u32 }`
  - `pub fn check(reqs: &Requirements, action: Action, levels: &dyn Fn(&str) -> u32) -> Result<(), Unmet>` (the first unmet in skill id order)
  - `pub fn labels_by_level(defs: &Defs, skill: &str) -> BTreeMap<u32, Vec<String>>` (lock labels per required level, for the unlock preview)

- [ ] **Step 1: Write failing tests:** `id_matches_exactly`, `pattern_star_matches_any_run` (`minecraft:*_spear` matches `minecraft:iron_spear`, not `minecraft:iron_spears`), `tag_uses_facts`, `repairable_uses_facts`, `all_of_and_any_of_combine`, `matchers_respect_target_kind` (an item matcher never matches a block), `highest_level_wins_across_entries`, `check_reports_first_unmet_in_id_order`, `cache_clears`, `labels_group_by_level`.
- [ ] **Step 2: Run** `cargo test -p skills_core locks::`; expect failures.
- [ ] **Step 3: Implement.** Patterns support `*` only; tags are written with a leading `#` in data and passed to `Facts` without it.
- [ ] **Step 4: Run**; expect PASS.
- [ ] **Step 5: Commit** `feat(core): match lock targets and check requirements`.

### Task 5: Player state, costs, points and requests

**Files:**
- Create: `skills-and-perks/native/core/src/progress.rs`

**Interfaces:**
- Consumes: `Defs`, `Settings`.
- Produces:
  - `pub struct PlayerState { pub format: u32, pub skills: BTreeMap<String, SkillProgress>, pub focus: Option<String>, pub focus_chosen_before: bool }` and `pub struct SkillProgress { pub level: u32, pub points_spent: u32, pub perks: BTreeSet<String> }` (unknown skill ids are kept)
  - `pub fn level(state: &PlayerState, defs: &Defs, skill: &str) -> u32` (1 when absent, capped at the skill's max)
  - `pub fn raise_cost(defs: &Defs, state: &PlayerState, skill: &str) -> Option<u32>` (None at max)
  - `pub fn points_available(defs: &Defs, state: &PlayerState, skill: &str) -> u32`
  - `pub enum Request { Raise { skill: String }, BuyPerk { perk: String }, SetFocus { focus: String } }`
  - `pub enum Refusal { UnknownSkill, UnknownPerk, UnknownFocus, AtMax, NotEnoughLevels { needed: u32 }, NotEnoughPoints { needed: u32 }, BelowMinLevel { needed: u32 }, MissingPerk { perk: String }, AlreadyOwned, AlreadyFocused }`
  - `pub struct Outcome { pub state: PlayerState, pub levels_spent: u32 }`
  - `pub fn apply(defs: &Defs, state: &PlayerState, xp_levels: u32, req: &Request) -> Result<Outcome, Refusal>`
  - `pub fn on_death(defs: &Defs, state: &PlayerState) -> PlayerState` (death penalty, never below 1)
  - `pub fn active_perks(defs: &Defs, state: &PlayerState) -> Vec<&PerkDef>` (bought and min level still met)

- [ ] **Step 1: Write failing tests** with the spec's values: `cost_follows_default_curve` (L1 to L4 cost 2, L5 cost 3, L29 to L31 cost 9), `total_to_max_is_167`, `focus_discount_rounds_the_discount_up` (2 to 1, 3 to 2, 4 to 3, 5 to 3, 9 to 6), `points_every_fourth_level` (level 3: 0, level 4: 1, level 32: 8), `raise_spends_levels_and_adds_a_level`, `raise_without_levels_is_refused_and_takes_nothing`, `raise_at_max_is_refused`, `perk_needs_points_min_level_and_required_perks`, `first_focus_is_free_then_switching_costs_10`, `death_penalty_never_goes_below_1`, `perk_below_min_level_is_inactive`, `unknown_saved_skill_is_kept_but_ignored`.
- [ ] **Step 2: Run** `cargo test -p skills_core progress::`; expect failures.
- [ ] **Step 3: Implement.** Discounted cost: `max(1, cost - ceil(cost * percent / 100))`.
- [ ] **Step 4: Run**; expect PASS.
- [ ] **Step 5: Commit** `feat(core): skill costs, perk points, focus and requests`.

### Task 6: Perk effects, attribute command diff, save and wire formats

**Files:**
- Create: `skills-and-perks/native/core/src/effects.rs`, `skills-and-perks/native/core/src/wire.rs`

**Interfaces:**
- Consumes: `active_perks`, `Effect`, `PlayerState`, `Requirements`.
- Produces:
  - `pub struct AttributeMod { pub id: String, pub attribute: String, pub amount: f64, pub operation: Operation }` (id `skills_and_perks:<perk path>/<effect index>`)
  - `pub struct ActiveEffects { pub attributes: Vec<AttributeMod>, pub mount_attributes: Vec<AttributeMod>, pub xp_gain: f64, pub anvil_cost: f64, pub damage: Vec<(Direction, DamageKind, f64)>, pub drops: Vec<(Matcher, f64)> }` (multipliers combine by product, 1.0 when none)
  - `pub fn active_effects(defs: &Defs, state: &PlayerState) -> ActiveEffects`
  - `pub fn damage_multiplier(effects: &ActiveEffects, direction: Direction, kind: DamageKind) -> f64` (`Any` rules apply to every kind)
  - `pub const DIG_LOCK: &str = "skills_and_perks:dig_lock"` (attribute `minecraft:block_break_speed`, amount -1.0, `add_multiplied_total`)
  - `pub fn attribute_commands(target: &str, before: &[AttributeMod], after: &[AttributeMod]) -> Vec<String>` (removals first, then additions, each sorted by id; format `attribute <target> <attribute> modifier remove <id>` and `attribute <target> <attribute> modifier add <id> <amount> <operation>`)
  - `pub fn encode_state(s: &PlayerState) -> String` and `pub fn decode_state(json: &str) -> Result<PlayerState, DefError>` (JSON, `format: 1`)
  - `pub enum ToServer { Act(Request), Need { items: Vec<String> } }`, `pub enum ToClient { Reqs { items: Vec<(String, Requirements)> }, Open, Refused { skill: String, level: u32 } }` (serde JSON; `Request` derives `Serialize, Deserialize`)
  - `pub struct ClientDefs { pub defs: Defs, pub labels: BTreeMap<String, BTreeMap<u32, Vec<String>>> }` (`defs` with `locks` emptied; `labels` from `labels_by_level` per skill)
  - `pub struct DefsChunk { pub send_id: u32, pub index: u32, pub total: u32, pub data: String }`
  - `pub fn chunk_defs(defs_json: &str, send_id: u32, max_bytes: usize) -> Vec<DefsChunk>` (each encoded chunk at most `max_bytes`)
  - `pub struct Reassembler` with `pub fn push(&mut self, c: DefsChunk) -> Option<String>` (a newer `send_id` discards older parts; returns the full JSON when complete)
  - `pub fn client_defs_json(defs: &Defs) -> String` (the `ClientDefs` for `defs`, as JSON; no matchers)

- [ ] **Step 1: Write failing tests:** `multipliers_multiply`, `any_damage_rule_applies_to_every_kind`, `attribute_commands_remove_then_add_sorted`, `no_change_gives_no_commands`, `inactive_perk_yields_remove_command` (a perk dropping below min level yields a remove command), `dig_lock_constants_match_spec`, `state_round_trips`, `decode_refuses_format_2`, `wire_round_trips`, `client_defs_have_no_locks_but_have_labels`, `chunks_fit_the_limit` (30,000 bytes), `defs_reassemble_in_any_order`, `stale_chunks_are_ignored`.
- [ ] **Step 2: Run** `cargo test -p skills_core effects:: wire::`; expect failures.
- [ ] **Step 3: Implement.** Amounts print with Rust's default `f64` formatting.
- [ ] **Step 4: Run**; expect PASS.
- [ ] **Step 5: Commit** `feat(core): perk effects, attribute diff, save and network formats`.

### Task 7: Default content and its validation test

**Files:**
- Create: `skills-and-perks/content/data/skills_and_perks/skills_and_perks/skills/{melee,ranged,defense,mining,gathering,agility,arcane,taming}.json`, `.../perks/<skill>_<perk>.json` (5 per skill), `.../focus/{fighting,working,roaming}.json`, `.../locks/{tiers,specials,blocks,mounts}.json`, `.../settings/defaults.json`, `skills-and-perks/content/assets/skills_and_perks/lang/en_us.json`, `skills-and-perks/native/core/tests/shipped_defaults.rs`

**Interfaces:**
- Consumes: `registry::build`, `locks::requirements_for` with a test `Facts` that knows vanilla tags used by the files.
- Produces: the default data set; translation keys `skills_and_perks.skill.<id path>`, `skills_and_perks.perk.<id path>`, `skills_and_perks.perk.<id path>.desc`, `skills_and_perks.focus.<id path>`, `skills_and_perks.lock.<label>`, `skills_and_perks.ui.*`, `skills_and_perks.msg.*`.

- [ ] **Step 1: Write the failing test** `shipped_defaults.rs` reading every file under `content/data` (path from `CARGO_MANIFEST_DIR`): `defaults_load_without_errors`, `eight_skills_with_spec_ids`, `each_skill_has_5_perks_costing_more_than_8_points`, `each_skill_has_one_capstone_at_level_24_or_more`, `three_focus_areas_cover_all_skills_once`, `tier_ladder_matches_spec` (copper, iron, diamond and netherite pickaxe, sword, chestplate and spear at 4, 8, 16, 24), `specials_match_spec` (every special in Global Constraints), `free_tiers_are_unlocked` (wood, stone, leather, gold, chainmail), `every_text_key_exists_in_en_us`.
- [ ] **Step 2: Run** `cargo test -p skills_core --test shipped_defaults`; expect failures.
- [ ] **Step 3: Write the data and English text.** Skill icons are vanilla items: Melee `minecraft:iron_sword`, Ranged `minecraft:bow`, Defense `minecraft:shield`, Mining `minecraft:iron_pickaxe`, Gathering `minecraft:iron_axe`, Agility `minecraft:feather`, Arcane `minecraft:enchanted_book`, Taming `minecraft:saddle`. Perks use mostly attribute effects (spec 7.3); names and wording are original.
- [ ] **Step 4: Run**; expect PASS.
- [ ] **Step 5: Commit** `feat(content): default skills, perks, focus areas, locks and English text`.

### Task 8: Server data loading and player state

**Files:**
- Create: `skills-and-perks/native/api/src/server/data.rs`, `skills-and-perks/native/api/src/server/state.rs`, `skills-and-perks/native/api/src/server/queue.rs`
- Modify: `skills-and-perks/native/api/src/lib.rs`

**Interfaces:**
- Consumes: `registry::build`, `wire::{encode_state, decode_state, chunk_defs, client_defs_json}`, `LockCache`,
  `locks::{Facts, TargetKind, Requirements}`.
- Produces (`Uuid` and `EquipSlot` below mean the player id and equipment slot types Horizon's events and helpers
  use, such as `event.player`):
  - `pub struct ServerData { pub defs: Arc<Defs>, pub generation: u64, pub locks: Mutex<LockCache> }` in a `static SERVER: RwLock<Option<Arc<ServerData>>>`
  - `pub struct HostFacts<'a>(pub &'a dyn Host)` implementing `Facts` with `server_item_in_tag`,
    `block_name_in_tag` and the `component` helper (`minecraft:repairable` of a fresh `ItemStack::new(id, 1)`)
  - `pub fn requirements(host: &dyn Host, kind: TargetKind, id: &str) -> Requirements` (through `SERVER`'s cache;
    empty before the first data load)
  - `pub static STATE: AttachmentType<PlayerState>` (namespace `skills_and_perks`, name `state`, codec storing `encode_state` as one string tag `json`, `copy_on_death: true`, `sync: true`)
  - `pub fn state_of(host: &mut dyn Host, player: Uuid) -> PlayerState` and `pub fn set_state(host: &mut dyn Host, player: Uuid, s: PlayerState)`
  - `pub enum Job { SendDefs(Uuid), SyncAttributes(Uuid), CheckEquipment(Uuid), Unequip { player: Uuid, slot: EquipSlot } }` and `pub fn queue(job: Job)`, drained in `server_tick` (helpers that refuse inside a player's hook run from there). This task runs `SendDefs`; `CheckEquipment` and `Unequip` go to `enforce::run_job` (Task 10) and `SyncAttributes` to `effects::run_job` (Task 11). Until those tasks land, the drain logs and drops them.
  - Hooks: `add_server_reload_listeners` (one JSON listener per data folder), `server_reload_listener_apply` (collect files, `build`, log each `DefError` with `log::warn!`, swap `SERVER`, bump `generation`, queue `SendDefs`, `SyncAttributes` and `CheckEquipment` for every online player), `player_logged_in` (queue the same three), `player_clone` (on death: `on_death`, then queue `SyncAttributes`)

- [ ] **Step 1: Write failing tests** with `Host` doubles and `dispatch_*` (Horizon's documented method): `reload_builds_defs_and_logs_bad_files`, `reload_queues_jobs_for_online_players`, `login_queues_defs_attributes_and_equipment`, `state_round_trips_through_the_attachment_codec`, `death_applies_the_penalty_setting`.
- [ ] **Step 2: Run** `cargo test -p mod-skills_and_perks server::`; expect failures.
- [ ] **Step 3: Implement.** `SendDefs` sends `chunk_defs(client_defs_json(..), generation as u32, 30_000)` on `skills_and_perks:defs`, at most 32 messages per player per tick (the rest wait for the next tick).
- [ ] **Step 4: Run**; expect PASS.
- [ ] **Step 5: Commit** `feat(server): load data files, keep player state, queue work`.

### Task 9: Requests and the `/skills` command

**Files:**
- Create: `skills-and-perks/native/api/src/server/actions.rs`, `skills-and-perks/native/api/src/server/commands.rs`

**Interfaces:**
- Consumes: `progress::apply`, `state_of`, `set_state`, `queue`, `requirements` (Task 8), `wire::{ToServer, ToClient}`.
- Produces:
  - `pub fn handle(host: &mut dyn Host, player: Uuid, msg: ToServer)`: `Act` runs `apply` with the player's current XP level; success calls `give_levels(player, -(levels_spent as i32))`, `set_state`, queues `SyncAttributes`; a refusal sends an action-bar line (`skills_and_perks.msg.<refusal>`). `Need` answers with `ToClient::Reqs` for up to 64 ids per message on `skills_and_perks:msg`.
  - Hooks: `net_from_client` (channel `skills_and_perks:act` only; malformed messages ignored), `register_commands` (`/skills`, `/skills raise <skill>`, `/skills perk <perk>`, `/skills focus <area>`, `/skills open`, and for permission level 2: `/skills set <player> <skill> <level>`, `/skills points <player> <skill> <n>`, `/skills reset <player> [skill]`), `perform_command` (runs them; `open` sends `ToClient::Open` on `skills_and_perks:msg`; skill, perk and area arguments accept the id path, so `mining` means `skills_and_perks:mining`)

- [ ] **Step 1: Write failing tests:** `raise_takes_levels_and_updates_state`, `raise_without_levels_is_refused_and_takes_nothing`, `malformed_message_is_ignored`, `need_answers_in_batches_of_64`, `skills_command_lists_every_skill`, `raise_command_matches_the_request_path`, `admin_commands_need_level_2`, `reset_clears_one_skill_or_all`.
- [ ] **Step 2: Run** `cargo test -p mod-skills_and_perks actions:: commands::`; expect failures.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run**; expect PASS.
- [ ] **Step 5: Commit** `feat(server): skill requests and the skills command`.

### Task 10: Lock enforcement

**Files:**
- Create: `skills-and-perks/native/api/src/server/enforce.rs`

**Interfaces:**
- Consumes: `locks::check`, `requirements` and `HostFacts` (Task 8), `state_of`, `queue`, `effects::DIG_LOCK`.
- Produces:
  - `pub fn run_job(host: &mut dyn Host, job: Job)` for `CheckEquipment` and `Unequip`
  - `fn refuse(host, player, unmet)`: action bar `skills_and_perks.msg.needs` ("Needs %s %s") and `ToClient::Refused` on `skills_and_perks:msg` (the client plays the sound), at most once per `lock_message_cooldown_ticks` per player
  - Hooks returning `Flow::Cancel` when `check` fails: `use_item` and `use_item_on` (`Use` on the stack; `use_item_on` also `Place` for block items and `Open` for the clicked block), `attack_entity` (`Attack`, main hand), `destroy_block` (`Break`, main hand), `entity_mount` (`Mount`, mounting only), `interact_entity` (`Interact`, the target's type), `container_opened` (`Open`, the block)
  - `living_equipment_change`: a player's armor slot, or off-hand shield or elytra, holding an item that fails `Equip` queues `Unequip`; a main-hand change queues `SyncAttributes` (which adds or removes `DIG_LOCK`)
  - `Unequip` job: clear the slot with `living_set_equipment`, give it back with `give_stack`; what doesn't fit drops at the player's feet
  - `CheckEquipment` job: runs the equip check over all of a player's equipment
  - Creative and spectator players pass every check when `creative_ignores_locks` is true

- [ ] **Step 1: Write failing tests:** one `locked_<action>_is_cancelled` and one `unlocked_<action>_passes` per hook above, `creative_bypasses_locks`, `message_is_rate_limited`, `locked_armor_is_unequipped`, `full_inventory_drops_the_unequipped_item`, `reload_rechecks_equipment`, `main_hand_change_queues_attribute_sync`.
- [ ] **Step 2: Run** `cargo test -p mod-skills_and_perks enforce::`; expect failures.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run**; expect PASS.
- [ ] **Step 5: Commit** `feat(server): enforce skill locks`.

### Task 11: Perk effects in game

**Files:**
- Create: `skills-and-perks/native/api/src/server/effects.rs`

**Interfaces:**
- Consumes: `effects::{active_effects, attribute_commands, damage_multiplier, DIG_LOCK}`, `locks::matches`.
- Produces:
  - `pub fn run_job(host: &mut dyn Host, job: Job)` for `SyncAttributes`
  - `SyncAttributes` job: computes the wanted modifiers (perks, plus `DIG_LOCK` while the main hand fails `Break`), diffs against the last applied set kept in memory per player (empty after login, respawn or reload), and runs each line with `run_command`
  - Hooks: `living_incoming_damage_v2` (dealt multiplier when the attacker is a player, taken multiplier when the victim is one, kind from the damage type), `player_xp_change` (multiply positive amounts by `xp_gain`, rounding down), `prepare_anvil` (multiply the level cost by `anvil_cost`, rounding up, at least 1), `loot_drops` (for each drop rule whose matcher matches the broken block, with its chance, duplicate one random stack), `entity_mount` (mounting adds `mount_attributes` to the vehicle with `run_command`, dismounting removes them)

- [ ] **Step 1: Write failing tests:** `attribute_sync_runs_the_diff`, `inactive_perk_modifier_is_removed`, `dig_lock_added_for_locked_main_hand_and_removed_after`, `dealt_and_taken_multipliers_apply_by_kind`, `xp_gain_rounds_down`, `anvil_cost_never_below_1`, `drop_bonus_duplicates_with_its_chance` (seeded random), `mount_modifiers_follow_the_rider`.
- [ ] **Step 2: Run** `cargo test -p mod-skills_and_perks server::effects::`; expect failures.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run**; expect PASS.
- [ ] **Step 5: Commit** `feat(server): apply perk effects`.

### Task 12: Client cache and tooltips

**Files:**
- Create: `skills-and-perks/native/api/src/client/cache.rs`, `skills-and-perks/native/api/src/client/tooltip.rs`

**Interfaces:**
- Consumes: `wire::{Reassembler, ToClient, ToServer, ClientDefs}`, `STATE` (client copy through `host.attachments()`).
- Produces:
  - `pub struct ClientCache { pub defs: Option<ClientDefs>, pub reqs: HashMap<String, Requirements>, pending: BTreeSet<String> }` in a `static`; `reqs` cleared when a new `send_id` completes
  - Hooks: `net_from_server` (`skills_and_perks:defs` through `Reassembler`; on `skills_and_perks:msg`, `Reqs` fills the cache, `Open` opens the screen, `Refused` plays `minecraft:block.note_block.bass` with `play_local_sound`), `tick` (sends one `ToServer::Need` with up to 64 pending ids, plus ids from the player's inventory not cached yet), `tooltip` (for a cached item, one line per action and skill: `skills_and_perks.ui.requires`, red when unmet and green when met, hiding met lines when the client setting says so; a miss adds the id to `pending`)
  - Client setting file `config/skills_and_perks-client.json` with `hide_met_requirements: bool` (default false)

- [ ] **Step 1: Write failing tests:** `defs_reassemble_across_messages`, `reqs_fill_the_cache`, `new_defs_clear_the_reqs_cache`, `refused_plays_the_sound`, `tooltip_shows_unmet_in_red_and_met_in_green`, `tooltip_hides_met_when_set`, `miss_is_requested_once_on_next_tick`.
- [ ] **Step 2: Run** `cargo test -p mod-skills_and_perks client::`; expect failures.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run**; expect PASS.
- [ ] **Step 5: Commit** `feat(client): definition cache and requirement tooltips`.

### Task 13: Skills screen and hotkey

**Files:**
- Create: `skills-and-perks/native/api/src/client/layout.rs`, `skills-and-perks/native/api/src/client/screen.rs`, `skills-and-perks/native/api/src/client/keys.rs`

**Interfaces:**
- Consumes: `ClientCache` (its `defs.defs` is a `Defs` without locks, so the `progress` functions work on it),
  `progress::{level, raise_cost, points_available}`, `wire::ToServer`.
- Produces:
  - `pub fn layout(width: i32, height: i32, skills: usize, perks: usize) -> Layout` and `pub fn hit(l: &Layout, x: f64, y: f64) -> Option<Button>` with `pub enum Button { SelectSkill(usize), Raise(usize), BuyPerk(usize), SetFocus(usize) }` (pure, no host)
  - `screen.rs`: draws with `fill`, `text`, `text_width` and `item_model` in `screen_render_post` when `mod_screen()` is `skills_and_perks`: header (XP levels, focus), one row per skill (icon, name, `level/max`, raise cost after discount, perk points), the selected skill's perks (bought, buyable, or locked with the reason) and its unlock preview (`labels_by_level` for the next levels), and the focus picker. `screen_click` maps `hit` to `ToServer::Act` messages.
  - `keys.rs`: `key` / `input_key` open the screen with `mod_screen_open("skills_and_perks")` on K (key code from Horizon's GLFW encoding) when no other screen is open

- [ ] **Step 1: Write failing tests** for the pure parts: `rows_fit_eight_skills_at_minimum_size` (427 by 240 GUI pixels), `hit_finds_each_button`, `hit_outside_returns_none`, `raise_button_sends_raise_request`.
- [ ] **Step 2: Run** `cargo test -p mod-skills_and_perks client::layout`; expect failures.
- [ ] **Step 3: Implement** layout, drawing and keys (the sketch in spec 7.7 is the target).
- [ ] **Step 4: Run**; expect PASS.
- [ ] **Step 5: In-game check (user runs the native build).** Expected: K opens the screen, raising Mining with XP works, a perk can be bought, Escape closes it.
- [ ] **Step 6: Commit** `feat(client): skills screen and hotkey`.

### Task 14: Abilities and the integration API for other mods

Added on 2026-11-13 at the user's request (spec 7.10). Runs after Task 13, before packaging.

**Files:**
- Modify: `skills-and-perks/native/core/src/defs.rs`, `skills-and-perks/native/core/src/locks.rs`
- Create: `skills-and-perks/native/api/src/integration.rs`, `skills-and-perks/INTEGRATION.md`

**Interfaces:**
- Consumes: `Target`, `TargetKind`, `Matcher`, `check`, `requirements`, `state_of`, `level`, refusal messaging (Task 10).
- Produces:
  - `Target::Ability(Matcher)` (`{"ability": {"id": "dashmod:dash"}}`) and `TargetKind::Ability`; abilities allow
    only `use`; `id` and `pattern` matchers only (no tags or repair materials)
  - In the `api` crate, `pub mod integration` re-exported at the crate root: `pub fn skill_level(host: &mut dyn Host,
    player: u128, skill: &str) -> u32`, `pub fn has_perk(host: &mut dyn Host, player: u128, perk: &str) -> bool`,
    `pub fn may_use_ability(host: &mut dyn Host, player: u128, ability: &str) -> bool` (checks the `ability` locks,
    honours `creative_ignores_locks`, and on refusal sends the same action-bar line and sound as other locks)
  - `INTEGRATION.md`: how another mod declares an ability lock in data and calls the API behind
    `#[cfg(feature = "skills_and_perks")]`

- [ ] **Step 1: Write failing tests:** `parses_an_ability_lock`, `ability_allows_only_use`, `ability_refuses_tag_matchers`, `ability_requirements_are_matched_by_id_and_pattern`, `ability_labels_join_the_preview`, `may_use_ability_checks_the_level`, `may_use_ability_refusal_sends_the_needs_line`, `creative_may_use_locked_abilities`, `skill_level_and_has_perk_read_the_saved_state`.
- [ ] **Step 2: Run** `cargo test --workspace`; expect failures.
- [ ] **Step 3: Implement**, then write `INTEGRATION.md` with a worked example (a `dashmod:dash` ability at Agility 10).
- [ ] **Step 4: Run**; expect PASS.
- [ ] **Step 5: Commit** `feat: ability locks and an integration api for other mods`.

### Task 15: Packaging and in-game acceptance

**Files:**
- Create: `skills-and-perks/tools/package.py`, `skills-and-perks/tools/test_package.py`

**Interfaces:**
- Consumes: the content folder and the bundle folder; `HORIZON_PIN.md` values.
- Produces: `dist/skills_and_perks.hmod`: `horizon-mod.toml`, `assets/`, `data/` from `content/`; `horizon-native.json` (`schema` 1, `id`, `version`, `game` `"26.3"`, `horizon` pin, `mixins` the horizon-mixins commit the pin uses, `hooks` the API number); `mixins/skills_and_perks/` from `native/` (only allowed files). Deflate entries, sorted paths, fixed timestamps.

- [ ] **Step 1: Write the failing test** `test_package.py` (`python3 -I -m unittest`): `archive_has_only_allowed_entries`, `manifests_name_the_same_id_and_version`, `native_json_fields_match_the_pin`, `no_cargo_lock_or_target_inside`, `build_is_reproducible` (two builds, same bytes).
- [ ] **Step 2: Run** `python3 -I -m unittest skills-and-perks/tools/test_package.py`; expect failures.
- [ ] **Step 3: Implement `package.py`** (standard library `zipfile` only).
- [ ] **Step 4: Run**; expect PASS. Then run `cargo horizon-mod check dist/skills_and_perks.hmod`; expect no errors.
- [ ] **Step 5: Single-player acceptance (user runs the build and plays; agent records results in `skills-and-perks/ACCEPTANCE.md`).** Checklist: screen opens on K and with `/skills open`; raising costs the shown levels; a perk changes the stat (for example mining speed); a diamond pickaxe is refused below Mining 16 with the message, its tooltip in red, and no breaking progress; a locked chestplate comes off; riding a horse needs Taming 4; dying keeps skills; raising a lock with a data pack and `/reload` takes the armor off.
- [ ] **Step 6: Dedicated server acceptance (user runs).** `horizon-launcher native-build skills-and-perks/native --server-output <dir> --version <tag> --trust-native-code`, then one client with the mod and one without: both are bound by locks, `/skills` works for both, only the first has the screen and tooltips.
- [ ] **Step 7: Commit** `build: package skills and perks as one hmod` and the acceptance notes.
