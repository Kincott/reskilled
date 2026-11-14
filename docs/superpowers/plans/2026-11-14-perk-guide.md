# Perk Guide Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pressing G over a perk on Reskilled's skill page opens a guide page: an animated 3D scene for mechanic perks, a
generated card for number perks.

**Architecture:** Scenes are data files (`reskilled/guides/`) parsed and checked in the `skills_core` crate, merged by
perk like other definitions and sent to clients inside `Defs`. The core also plays a scene's timeline into a frame
(blocks, caption, overlays) and builds card lines from a perk's effects, so both are tested without the game. The
client adds a `Guide` view to the Reskilled screen that draws the frame with `Host::draw_scene`, the overlays and the
card lines.

**Tech Stack:** Rust 1.99.1 (Horizon pin), `skills_core` (no Horizon dependency) and `mod-reskilled` (trait mod),
serde_json; `reskilled/tools/preview.py` for the sign-off picture.

**Spec:** `docs/superpowers/specs/2026-11-14-perk-guide-design.md`

## Global Constraints

- No em dash (U+2014) anywhere: code, data, text, commits. Sweep: `grep -rlP '\x{2014}' reskilled/ docs/ --exclude-dir=target`.
- No AI attribution in commits. Conventional commits on `feat/skills-and-perks`; do not push.
- Nothing may copy Create's Ponder: no "Ponder" name, no copied frame, colours, buttons, keyword tags or wording.
- Every page fits the smallest GUI size, 320 by 240, and the usual 427 by 240 (`client::preview` checks).
- Captions never type a perk's numbers; they fill `%s` from `args`.
- Gate after each task: `cargo fmt --all`, `cargo clippy -p skills_core -p mod-reskilled --all-targets` (no warnings),
  `cargo test --workspace`.
- Server checks (block exists, property keys exist) use `Host::default_state` and `Host::block_state_property`;
  exact state matching (`age=7`) is the client's, through `Host::block_states()` (client-only). A block the client
  cannot match is left out of the scene with a `log::warn!`, the rest plays. (A narrowing of the spec's "the server
  checks block states", found while planning: Horizon's state table is client-only.)

## Review Focus

- A guide for a perk the data no longer has (a pack removed it): dropped with an error, no crash. Test in Task 1.
- A scene block the client cannot match: left out, scene still plays. Test in Task 6.
- A perk with no effects and no scene: the card shows its description. Test in Task 3.
- G pressed with no perk under the mouse, or outside the skill page: nothing happens. Test in Task 7.
- Stepping to another perk or pressing Replay restarts the timeline from an empty scene. Test in Task 7.

---

### Task 1: Guide definitions in the core

**Files:**
- Modify: `reskilled/native/core/src/defs.rs` (types, `DefKind::Guide`, `DefFile::Guide`, parse, validate)
- Modify: `reskilled/native/core/src/registry.rs` (`Defs.guides`, merge by perk, drop guides of unknown perks)
- Test: in those files' test modules

**Interfaces:**
- Produces:
  - `pub struct GuideDef { pub perk: String, pub priority: i32, pub seconds: f32, pub view: GuideView, pub steps: Vec<GuideStep> }`
  - `pub struct GuideView { pub center: [f32; 3], pub yaw: f32, pub pitch: f32, pub scale: f32, pub spin: f32 }` (`spin` default 0)
  - `pub struct GuideStep { pub at: f32, #[serde(flatten)] pub action: GuideAction }`
  - `pub enum GuideAction` (serde, `rename_all = "snake_case"`, externally tagged by key):
    `Place(Vec<PlacedBlock>)`, `Remove(Vec<[i32; 3]>)`, `Caption(Caption)`, `Icon(Overlay)`, `Effect(Overlay)`,
    `Hearts(u32)`, `Clear(ClearWhat)`
  - `pub struct PlacedBlock { pub pos: [i32; 3], pub block: String }` (a state string, `minecraft:wheat[age=7]`)
  - `pub struct Caption { pub key: String, pub text: String, #[serde(default)] pub args: Vec<CaptionArg> }`
  - `pub struct CaptionArg { pub effect: String, pub field: String, #[serde(rename = "as")] pub shown_as: ShownAs }`
  - `pub enum ShownAs { Number, Percent, Seconds, Blocks, Hearts }`
  - `pub struct Overlay { pub item: Option<String>, pub effect: Option<String>, pub at: [f32; 2] }` (`at` fractions of the box)
  - `pub enum ClearWhat { Icons, Hearts }`
  - `pub fn parse_state(state: &str) -> Option<(String, Vec<(String, String)>)>` (block id and properties)
  - `pub fn effect_field(perk: &PerkDef, effect: &str, field: &str) -> Option<f64>`: the numeric field of the
    perk's first effect whose `type` is `effect` (serialize the effect with serde_json and read the field)
  - `Defs { ..., pub guides: BTreeMap<String, GuideDef> }` keyed by perk id; `DefKind::Guide`, folder `guides`.

- [ ] **Step 1: Write the failing tests**

`parses_a_guide` (the spec's Replant example parses; 6 steps; step 0 is `Place` with `wheat[age=7]` at `[0, 1, 0]`),
`guides_that_make_no_sense_are_refused` (each refused, the reason naming the field: `seconds` 0; a step `at` past
`seconds`; steps out of time order; `scale` 0; an overlay `at` outside 0 to 1; a state string `minecraft:wheat[age`
that `parse_state` rejects), `parse_state_reads_block_and_properties`
(`"minecraft:wheat[age=7,x=y]"` gives `("minecraft:wheat", [("age","7"),("x","y")])`, `"minecraft:stone"` gives no
properties, `"minecraft:wheat[age]"` is None), `caption_args_must_name_a_real_value` (in `registry.rs`: a guide whose
arg names `crop_growth.radius` on a perk with crop growth is kept, one naming `crop_growth.speed` is dropped with an
error naming `speed`), `a_guide_for_an_unknown_perk_is_dropped_with_error`, `a_higher_priority_guide_replaces_a_lower_one`.

- [ ] **Step 2: Run** `cargo test -p skills_core`; expect compile failures for the missing types.
- [ ] **Step 3: Implement** the types, parsing in `parse_file`, `GuideDef::validate`, `parse_state`, `effect_field`,
  and the registry merge (guides after perks; drop unknown perks and unknown caption args, each with a `DefError`).
- [ ] **Step 4: Run** `cargo test -p skills_core`; expect PASS.
- [ ] **Step 5: Commit** `feat: guide definitions for perks`.

### Task 2: Playing a timeline

**Files:**
- Create: `reskilled/native/core/src/guide.rs` (add `pub mod guide;` to `lib.rs`)

**Interfaces:**
- Consumes: Task 1 types.
- Produces:
  - `pub struct Frame { pub blocks: BTreeMap<[i32; 3], String>, pub caption: Option<Caption>, pub icons: Vec<Overlay>, pub hearts: Option<u32>, pub progress: f32 }`
  - `pub fn frame_at(guide: &GuideDef, seconds_since_start: f32) -> Frame`: loops at `guide.seconds`; applies every
    step with `at <= t` in order to an empty frame; `progress` is `t / seconds`, 0 to 1. `Effect` overlays join
    `icons` (an overlay with `effect` set).
  - `pub fn yaw_at(guide: &GuideDef, seconds_since_start: f32) -> f32`: `view.yaw + view.spin * t` (t not looped,
    so the turn is smooth across loops).

- [ ] **Step 1: Write the failing tests** `a_frame_holds_what_the_steps_placed_so_far` (Replant example at 2.0s: two
  blocks, caption 1; at 3.5s: `[0,1,0]` gone, one icon; at 4.5s: wheat age 0 back, caption 2),
  `the_timeline_loops_from_an_empty_scene` (at `seconds + 0.5` equals the frame at 0.5), `clear_takes_the_icons_off`,
  `the_view_turns_by_its_spin`.
- [ ] **Step 2: Run** `cargo test -p skills_core guide`; expect FAIL.
- [ ] **Step 3: Implement** `frame_at` and `yaw_at`.
- [ ] **Step 4: Run**; expect PASS.
- [ ] **Step 5: Commit** `feat: play a perk guide's timeline`.

### Task 3: Card lines

**Files:**
- Create: `reskilled/native/core/src/card.rs` (`pub mod card;`)

**Interfaces:**
- Consumes: `PerkDef`, `Effect`, Task 1 `effect_field`.
- Produces:
  - `pub enum CardLine { Attribute { attribute: String, amount: f64, operation: Operation, mount: bool }, Damage { direction: Direction, kind: DamageKind, multiplier: f64 }, Chance { what: CardChance, chance: f64 }, Value { key: &'static str, args: Vec<f64> }, Description }`
  - `pub enum CardChance { ExtraDrop, CropGrowth }`
  - `pub fn card_lines(perk: &PerkDef) -> Vec<CardLine>`: one line per effect, in order; `Value` keys (translation
    keys under `reskilled.card.`) for `enchanted_weapon` (per, max as percents), `enchanted_armor`,
    `shorter_effects` (multiplier), `rune_repair` (points, seconds), `last_stand` (above, cooldown), `crop_growth`
    (radius, chance, interval), `home_ward`, `safe_haven`, `near_home` (radius), `xp_gain`, `anvil_cost`; a perk with
    no effects, or only effects with no line (`salvage`, `restful_sleep`, `replant`, `effect_on_kill`), gives
    `[Description]`.
  - `pub fn with_modifier(now: f64, base: f64, amount: f64, operation: Operation) -> f64`: the value once the
    modifier is added (`add_value`: now + amount; `add_multiplied_base`: now + base * amount;
    `add_multiplied_total`: now * (1 + amount)).

- [ ] **Step 1: Write the failing tests** `number_perks_get_a_line_per_effect` (Wind Runner gives an `Attribute`
  movement_speed line and a `Damage` taken fall x0.7 line), `rune_perks_get_their_values`
  (Runeblade: `Value { key: "reskilled.card.enchanted_weapon", args: [3.0, 15.0] }`),
  `a_perk_with_nothing_to_show_gets_its_description` (Salvage and a perk with no effects: `[Description]`),
  `modifiers_apply_as_the_game_does` (`with_modifier(4.0, 4.0, 0.1, AddMultipliedBase) == 4.4`, `AddValue` 20+4=24,
  `AddMultipliedTotal` 10 * 1.5 = 15).
- [ ] **Step 2: Run**; expect FAIL. **Step 3: Implement.** **Step 4: Run**; expect PASS.
- [ ] **Step 5: Commit** `feat: card lines for number perks`.

### Task 4: Captions with values

**Files:**
- Modify: `reskilled/native/core/src/guide.rs`

**Interfaces:**
- Produces: `pub fn caption_values(perk: &PerkDef, caption: &Caption) -> Vec<String>`: each arg's value from
  `effect_field`, shown as: `Number` plain (no trailing `.0`), `Percent` x100 with `%`, `Seconds` with ` s`,
  `Blocks` with ` blocks`, `Hearts` halved with ` hearts`; a missing value gives `?`.

- [ ] **Step 1: Write the failing test** `caption_values_fill_from_the_perk` (Green Aura: radius as `Blocks` gives
  `"5 blocks"`, chance as `Percent` gives `"25%"`, interval as `Seconds` gives `"10 s"`; Last Breath `above` as
  `Hearts` gives `"3 hearts"`).
- [ ] **Step 2: Run**; FAIL. **Step 3: Implement.** **Step 4: Run**; PASS.
- [ ] **Step 5: Commit** `feat: fill guide captions from the perk's values`.

### Task 5: Loading and checking guides on the server

**Files:**
- Modify: `reskilled/native/api/src/server/data.rs` (`FOLDERS` gains `("guides", DefKind::Guide)`, length 6; after
  building, drop guides naming a block `Host::default_state` does not know or a property key its default state
  lacks per `Host::block_state_property`, logging each as other data errors are)
- Modify: `reskilled/native/api/src/client/preview.rs` (its `FOLDERS` gains guides)
- Modify: `reskilled/native/api/src/test_host.rs` (`block_state_property` from a `HashMap<(u32, String), String>`
  field `state_properties`, next to `default_states`)

**Interfaces:**
- Consumes: Task 1.
- Produces: guides inside `Defs` on the server and, unchanged, in `ClientDefs.defs` on clients.

- [ ] **Step 1: Write the failing tests** (in `server/data.rs`) `guides_load_and_reach_clients` (a guide file loads
  into `current().defs.guides` and appears in the decoded client defs), `a_guide_with_an_unknown_block_is_dropped`
  (`minecraft:not_a_block` unknown to `default_state`: guide gone, an error logged), `a_guide_with_an_unknown_property_is_dropped`
  (`minecraft:wheat[colour=red]`).
- [ ] **Step 2: Run** `cargo test -p mod-reskilled data`; FAIL. **Step 3: Implement.** **Step 4: Run**; PASS.
- [ ] **Step 5: Commit** `feat: load and check perk guides on the server`.

### Task 6: Resolving scene blocks on the client

**Files:**
- Create: `reskilled/native/api/src/client/scene.rs` (`mod scene;` in `client/mod.rs`)

**Interfaces:**
- Consumes: Task 1 `parse_state`, Task 2 `Frame`.
- Produces:
  - `pub fn resolve(state: &str, candidates: impl Fn(&str) -> Vec<BlockStateId>, property: impl Fn(BlockStateId, &str) -> Option<String>) -> Option<BlockStateId>`:
    the candidate (all states of the block) whose properties match every given one.
  - `pub fn scene_blocks(host: &dyn Host, frame: &Frame) -> Vec<(([i32; 3]), BlockStateId)>`: resolves each block
    with `host.block_states()` (states named the block) and `host.block_state_property`, caching resolutions in a
    `Mutex<BTreeMap<String, Option<BlockStateId>>>` cleared when `host.block_sprites_version()` changes; an
    unresolved block is left out with one `log::warn!` per state string.

- [ ] **Step 1: Write the failing tests** `resolve_picks_the_state_with_matching_properties` (candidates 10, 11, 12
  with age 0, 1, 7: `minecraft:wheat[age=7]` gives 12; `minecraft:wheat` gives the first candidate),
  `an_unmatched_block_is_left_out` (`resolve("minecraft:wheat[age=9]", ...)` is None).
- [ ] **Step 2: Run**; FAIL. **Step 3: Implement.** **Step 4: Run**; PASS.
- [ ] **Step 5: Commit** `feat: resolve guide scene blocks on the client`.

### Task 7: The guide page

**Files:**
- Modify: `reskilled/native/api/src/client/cache.rs` (`View::Guide`; `guide_perk: usize` (index in the shown
  skill's perks), `guide_started: u64` (client tick), `hovered_perk: Option<usize>` set by the skill page's draw)
- Modify: `reskilled/native/api/src/client/keys.rs` (`GUIDE_KEY = "guide"`, default `key.keyboard.g`: on the
  screen, in `View::Skill` with `hovered_perk` set, open the guide; otherwise nothing)
- Create: `reskilled/native/api/src/client/guide.rs` (layout, drawing, clicks for the page)
- Modify: `reskilled/native/api/src/client/screen.rs` (route draw and click in `View::Guide` to `guide.rs`; the
  perk tooltip's last line becomes the hint `reskilled.ui.guide_hint` "Press %s to see it in action", the key's
  display name)
- Modify: `reskilled/native/api/src/test_host.rs` (`draw_scene` records `Paint::Scene { rect, blocks: Vec<([i32; 3], u32)>, yaw }`)
- Modify: `reskilled/native/api/src/text.rs` and `content/assets/reskilled/lang/en_us.json` (hint, Back, Replay,
  card line texts `reskilled.card.*`, the attribute and damage kind names the card uses)

**Interfaces:**
- Consumes: Tasks 2, 3, 4, 6.
- Produces: `pub fn draw(host: &dyn Host, batch: &mut HudBatch, size: [f32; 2], mouse: [f32; 2])` and
  `pub fn click(host: &dyn Host, size: [f32; 2], at: [f32; 2], button: i32) -> Flow` in `client/guide.rs`;
  `pub struct GuidePage { pub window: Rect, pub scene: Rect, pub caption: Rect, pub lines: Rect, pub back: Rect, pub replay: Rect, pub prev: Rect, pub next: Rect }`
  and `pub fn page(size: [f32; 2]) -> GuidePage` in `client/layout.rs`.

Layout follows the spec's sketch: header (Back, perk name, prev and next), scene box, caption and progress bar,
card lines, footer with needs and price (reuse `perk_tip`'s need lines). A perk with a guide draws the scene, its
overlays (items via `host.item`, effect icons via the GUI atlas `gui/sprites/mob_effect/<path>`, a heart bar with
`gui/sprites/hud/heart/full` and `half`), caption and progress, then its card lines under the caption as fit; a
perk without one draws the card lines in the scene box. Attribute lines use `host.local_attribute` for now and
`with_modifier` for with (for an owned perk, `now` is shown as "with" and "without" is `now` less the modifier).

- [ ] **Step 1: Write the failing tests** `g_over_a_perk_opens_its_guide`,
  `g_with_no_perk_under_the_mouse_does_nothing` (also on the overview), `a_mechanic_perk_plays_its_scene`
  (`Paint::Scene` with the frame's blocks at 0.5s), `a_number_perk_shows_its_card`
  (Steady Hand: a line containing `4.4` given `local_attribute` 4.0), `back_returns_to_the_skill_page`,
  `stepping_or_replaying_restarts_the_scene` (after next, and after Replay, `guide_started` is the current tick and the
  first frame is empty), `the_tooltip_says_how_to_open_the_guide` (last line `Press G to see it in action`).
- [ ] **Step 2: Run** `cargo test -p mod-reskilled guide`; FAIL. **Step 3: Implement.** **Step 4: Run**; PASS.
- [ ] **Step 5: Commit** `feat: the perk guide page`.

### Task 8: The shipped scenes and the preview

**Files:**
- Create: `reskilled/content/data/reskilled/reskilled/guides/<perk path>.json` for the 12 mechanic perks:
  `defense_braced_stance` (Last Breath), `melee_planted_feet` (Bloodrush), `agility_soft_landing` (Pearl Step),
  `gathering_foragers_pace` (Green Aura), `gathering_sapling_keeper` (Replant), `arcane_deep_lore` (Antidote),
  `arcane_quiet_mind` (Living Runes), `building_salvage`, `building_restful_sleep`, `building_hearth`,
  `building_home_ward`, `building_safe_haven`
- Modify: `reskilled/content/assets/reskilled/lang/en_us.json` (captions `reskilled.guide.<perk path>.<n>`, the
  same text as each file's fallback)
- Modify: `reskilled/native/core/tests/shipped_defaults.rs`, `reskilled/native/api/src/client/preview.rs`
- Modify: `reskilled/README.md` (pack makers: the `guides` folder, with the Replant file as the example)

Each scene runs 6 to 10 seconds, is at most 6 by 6 by 6 blocks, uses `args` wherever its caption states a number,
and follows the spec's notes (Last Breath: heart bar to 1; Bloodrush: sword icon then Strength icon; Home Ward: a
house and bed, blocks inside the ring kept).

- [ ] **Step 1: Write the failing tests** `every_mechanic_perk_has_a_guide` (the 12 ids above all in
  `defaults().guides`), `shipped_guides_fit_their_box` (every placed block within 6 of the view center),
  `every_caption_key_has_english` (each key in `en_us.json` with the file's fallback text), and in `preview.rs`
  `the_guide_fits_the_window` (Replant and Steady Hand pages at 320 and 427 wide: all text and items inside the
  window, no text overlapping; scenes saved for `preview.py`).
- [ ] **Step 2: Run**; FAIL. **Step 3: Write the 12 files, the text and the README section.** **Step 4: Run**; PASS.
- [ ] **Step 5: Render the preview** (`SKILLS_PREVIEW_DIR=<dir> cargo test -p mod-reskilled preview`, then
  `python3 -I reskilled/tools/preview.py <dir> <assets>`; teach `preview.py` to draw `Paint::Scene` as a flat
  iso sketch of its blocks' positions) and show the user the Replant and Steady Hand pages for sign-off.
- [ ] **Step 6: Commit** `feat: twelve perk scenes and the guide preview`.
