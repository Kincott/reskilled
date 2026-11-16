//! The mod's look, shared by its screen, its lock warning and its item badge: colours, panels, bars, buttons and
//! small pixel pictures. Everything is drawn with the host's fills and text, so no texture has to load.

use horizon_client::mod_api::{Host, HudBatch};

use super::layout::Rect;

/// ARGB fills.
pub const SHADE: u32 = 0xA000_0000;
pub const OUTLINE: u32 = 0xFF08_090C;
pub const FRAME_LIGHT: u32 = 0xFFD9_B45A;
pub const FRAME: u32 = 0xFFA8_7F32;
pub const FRAME_DARK: u32 = 0xFF6B_4E1C;
pub const BODY: u32 = 0xFF1C_1F27;
pub const HEADER: u32 = 0xFF27_2B36;
pub const CARD: u32 = 0xFF2A_2F3B;
pub const CARD_HOVER: u32 = 0xFF37_3E4F;
pub const CARD_EDGE: u32 = 0xFF41_485A;
pub const INSET: u32 = 0xFF12_1419;
pub const BAR_FILL: u32 = 0xFF5F_C63A;
pub const BAR_FULL: u32 = 0xFFF2_C14E;
pub const ALERT: u32 = 0xFFD9_453A;
pub const WHITE_ARGB: u32 = 0xFFE8_ECF2;
/// RGB text colours.
pub const WHITE: u32 = 0xFF_FFFF;
pub const GRAY: u32 = 0xA8_ADB8;
pub const DIM: u32 = 0x6C_7280;
pub const GOLD: u32 = 0xF2_C14E;
/// A skill tab's mark while the skill has perk points to spend.
pub const PIP: u32 = 0xFFF5_C451;
pub const GREEN: u32 = 0x7F_DB5A;
pub const RED: u32 = 0xFF_6B5E;
/// A line of the game's font is 8 pixels tall.
pub const GLYPH: f32 = 8.0;
/// Vanilla's font has these: a filled diamond for a perk point, a tick for owned.
pub const POINT: &str = "\u{25C6}";
pub const TICK: &str = "\u{2714}";

/// How a button looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Plain,
    /// The thing to press next: a raise the player can pay for, a perk they can get.
    Go,
    /// Cannot be pressed now.
    Off,
}

/// A button's face, lit edge and dark edge.
fn button_colours(look: Look, hover: bool) -> (u32, u32, u32) {
    match (look, hover) {
        (Look::Off, _) => (0xFF26_2931, 0xFF30_343E, 0xFF1B_1D23),
        (Look::Go, false) => (0xFF3C_7A2A, 0xFF62_B045, 0xFF24_4F19),
        (Look::Go, true) => (0xFF4A_9434, 0xFF86_D862, 0xFF2B_5C1E),
        (Look::Plain, false) => (0xFF45_4D61, 0xFF6A_7690, 0xFF2A_303E),
        (Look::Plain, true) => (0xFF56_6180, 0xFF93_A2C4, 0xFF32_3A4C),
    }
}

pub fn fill(host: &dyn Host, batch: &mut HudBatch, rect: &Rect, argb: u32) {
    host.fill(batch, rect.array(), argb);
}

/// A one-pixel edge just inside the rectangle.
pub fn edge(host: &dyn Host, batch: &mut HudBatch, r: &Rect, argb: u32) {
    host.fill(batch, [r.x, r.y, r.w, 1.0], argb);
    host.fill(batch, [r.x, r.y + r.h - 1.0, r.w, 1.0], argb);
    host.fill(batch, [r.x, r.y + 1.0, 1.0, r.h - 2.0], argb);
    host.fill(batch, [r.x + r.w - 1.0, r.y + 1.0, 1.0, r.h - 2.0], argb);
}

/// A lit top and left edge and a dark bottom and right edge just inside the rectangle, so it looks raised.
fn bevel(host: &dyn Host, batch: &mut HudBatch, r: &Rect, light: u32, dark: u32) {
    host.fill(batch, [r.x, r.y, r.w, 1.0], light);
    host.fill(batch, [r.x, r.y + 1.0, 1.0, r.h - 1.0], light);
    host.fill(batch, [r.x + 1.0, r.y + r.h - 1.0, r.w - 1.0, 1.0], dark);
    host.fill(batch, [r.x + r.w - 1.0, r.y + 1.0, 1.0, r.h - 2.0], dark);
}

/// The window: a dark body in a brass frame with a black outline, and its header band.
pub fn panel(host: &dyn Host, batch: &mut HudBatch, r: &Rect, header: f32) {
    fill(host, batch, r, OUTLINE);
    let frame = r.shrunk(1.0);
    fill(host, batch, &frame, FRAME);
    bevel(host, batch, &frame, FRAME_LIGHT, FRAME_DARK);
    let body = r.shrunk(3.0);
    fill(host, batch, &body, BODY);
    host.fill(batch, [body.x, body.y, body.w, header - 3.0], HEADER);
    host.fill(
        batch,
        [body.x, body.y + header - 3.0, body.w, 1.0],
        FRAME_DARK,
    );
}

/// A sunken square for an icon.
pub fn slot(host: &dyn Host, batch: &mut HudBatch, r: &Rect) {
    fill(host, batch, r, INSET);
    bevel(host, batch, r, OUTLINE, CARD_EDGE);
}

/// The game's container windows (the inventory, a chest): their face, bevel and outline.
pub const CONTAINER: u32 = 0xFFC6_C6C6;
const CONTAINER_LIGHT: u32 = 0xFFFF_FFFF;
const CONTAINER_SHADOW: u32 = 0xFF55_5555;
const CONTAINER_OUTLINE: u32 = 0xFF00_0000;
/// The game's item slot: dark top and left, light bottom and right, mid grey inside.
const SLOT_DARK: u32 = 0xFF37_3737;
const SLOT_FACE: u32 = 0xFF8B_8B8B;
/// The game's hovered slot: white at half strength over the slot.
const SLOT_HOVER: u32 = 0x80FF_FFFF;
/// A container slot's side, its border included.
pub const CONTAINER_SLOT: f32 = 18.0;

/// A panel drawn as the game's container windows are: light grey, a black outline with cut corners, a white bevel
/// at the top and left and a dark one at the bottom and right.
pub fn container_panel(host: &dyn Host, batch: &mut HudBatch, r: &Rect) {
    let Rect { x, y, w, h } = *r;
    for (rect, argb) in [
        ([x + 1.0, y, w - 2.0, 1.0], CONTAINER_OUTLINE),
        ([x + 1.0, y + h - 1.0, w - 2.0, 1.0], CONTAINER_OUTLINE),
        ([x, y + 1.0, 1.0, h - 2.0], CONTAINER_OUTLINE),
        ([x + w - 1.0, y + 1.0, 1.0, h - 2.0], CONTAINER_OUTLINE),
        ([x + 1.0, y + 1.0, w - 2.0, h - 2.0], CONTAINER),
        ([x + 1.0, y + 1.0, w - 4.0, 2.0], CONTAINER_LIGHT),
        ([x + 1.0, y + 1.0, 2.0, h - 4.0], CONTAINER_LIGHT),
        ([x + 3.0, y + h - 3.0, w - 4.0, 2.0], CONTAINER_SHADOW),
        ([x + w - 3.0, y + 3.0, 2.0, h - 4.0], CONTAINER_SHADOW),
    ] {
        host.fill(batch, rect, argb);
    }
}

/// An 18 by 18 item slot as the game draws one at `at`.
pub fn container_slot(host: &dyn Host, batch: &mut HudBatch, at: [f32; 2]) {
    let [x, y] = at;
    let side = CONTAINER_SLOT;
    host.fill(batch, [x, y, side - 1.0, 1.0], SLOT_DARK);
    host.fill(batch, [x, y, 1.0, side - 1.0], SLOT_DARK);
    host.fill(
        batch,
        [x + 1.0, y + side - 1.0, side - 1.0, 1.0],
        CONTAINER_LIGHT,
    );
    host.fill(
        batch,
        [x + side - 1.0, y + 1.0, 1.0, side - 1.0],
        CONTAINER_LIGHT,
    );
    host.fill(batch, [x + 1.0, y + 1.0, side - 2.0, side - 2.0], SLOT_FACE);
}

/// The game's light over a hovered slot at `at`, drawn over its item.
pub fn container_slot_highlight(host: &dyn Host, batch: &mut HudBatch, at: [f32; 2]) {
    let side = CONTAINER_SLOT;
    host.fill(
        batch,
        [at[0] + 1.0, at[1] + 1.0, side - 2.0, side - 2.0],
        SLOT_HOVER,
    );
}

/// A progress bar filled to `part` of `whole`, gold once full.
pub fn bar(host: &dyn Host, batch: &mut HudBatch, r: &Rect, part: u32, whole: u32) {
    fill(host, batch, r, OUTLINE);
    let inner = r.shrunk(1.0);
    fill(host, batch, &inner, INSET);
    let done = part.min(whole) as f32 / whole.max(1) as f32;
    let colour = if part >= whole { BAR_FULL } else { BAR_FILL };
    host.fill(
        batch,
        [inner.x, inner.y, (inner.w * done).round(), inner.h],
        colour,
    );
}

/// A raised button with its label centred.
pub fn button(
    host: &dyn Host,
    batch: &mut HudBatch,
    r: &Rect,
    label: &str,
    look: Look,
    hover: bool,
) {
    let hover = hover && look != Look::Off;
    let (face, light, dark) = button_colours(look, hover);
    fill(host, batch, r, OUTLINE);
    let inner = r.shrunk(1.0);
    fill(host, batch, &inner, face);
    bevel(host, batch, &inner, light, dark);
    let colour = if look == Look::Off { DIM } else { WHITE };
    text_centred(host, batch, label, r, colour);
}

/// Text centred in the rectangle, both ways.
pub fn text_centred(host: &dyn Host, batch: &mut HudBatch, text: &str, r: &Rect, rgb: u32) {
    let x = r.x + ((r.w - host.text_width(text)) / 2.0).round();
    let y = r.y + ((r.h - GLYPH) / 2.0).round();
    host.text(batch, text, [x, y], rgb, true);
}

/// Text ending at `right`.
pub fn text_right(host: &dyn Host, batch: &mut HudBatch, text: &str, right: f32, y: f32, rgb: u32) {
    host.text(batch, text, [right - host.text_width(text), y], rgb, true);
}

/// The text, cut with an ellipsis when it is wider than `width`.
pub fn fitted(host: &dyn Host, text: &str, width: f32) -> String {
    if host.text_width(text) <= width {
        return text.to_string();
    }
    let mut kept: String = text.to_string();
    while !kept.is_empty() && host.text_width(&format!("{kept}...")) > width {
        kept.pop();
    }
    format!("{}...", kept.trim_end())
}

/// The text broken into lines no wider than `width`, at spaces.
pub fn wrapped(host: &dyn Host, text: &str, width: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if host.text_width(&format!("{line} {word}")) <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// The padlock, 8 by 10: `O` outline, `S`/`s` steel, `Y`/`y`/`d` brass from light to dark, `K` the keyhole.
pub const PADLOCK: [&str; 10] = [
    "..OOOO..", ".OSSSsO.", ".OS..sO.", ".OS..sO.", "OOOOOOOO", "OYYYYYyO", "OYyKKydO", "OYyKKydO",
    "OydddddO", "OOOOOOOO",
];
pub const PADLOCK_SIZE: [f32; 2] = [8.0, 10.0];

fn padlock_colour(pixel: u8) -> Option<u32> {
    match pixel {
        b'O' => Some(0xFF1A_1206),
        b'S' => Some(0xFFE6_EBF2),
        b's' => Some(0xFF98_A3B3),
        b'Y' => Some(0xFFFF_DD66),
        b'y' => Some(0xFFEF_B23C),
        b'd' => Some(0xFFB5_781E),
        b'K' => Some(0xFF3A_2408),
        _ => None,
    }
}

/// Draws the padlock with its top left at `at`.
pub fn padlock(host: &dyn Host, batch: &mut HudBatch, at: [f32; 2]) {
    pixel_art(host, batch, at, &PADLOCK, padlock_colour);
}

/// Draws pixel art from rows of letters with its top left at `at`, one fill for each run of a colour; `colour`
/// names each letter's colour, none for a clear pixel.
pub(crate) fn pixel_art(
    host: &dyn Host,
    batch: &mut HudBatch,
    at: [f32; 2],
    rows: &[&str],
    colour: impl Fn(u8) -> Option<u32>,
) {
    for (row, pixels) in rows.iter().enumerate() {
        let pixels = pixels.as_bytes();
        let mut start = 0;
        while start < pixels.len() {
            let run = pixels[start..]
                .iter()
                .take_while(|p| **p == pixels[start])
                .count();
            if let Some(argb) = colour(pixels[start]) {
                let rect = [at[0] + start as f32, at[1] + row as f32, run as f32, 1.0];
                host.fill(batch, rect, argb);
            }
            start += run;
        }
    }
}

/// The level-up arrow, 9 by 8: `O` outline, `A` its face, `a` its shaded right side.
const ARROW_ART: [&str; 8] = [
    "....O....",
    "...OAO...",
    "..OAAaO..",
    ".OAAAaaO.",
    "OOOAAaOOO",
    "..OAAaO..",
    "..OAAaO..",
    "..OOOOO..",
];
pub const ARROW_SIZE: [f32; 2] = [9.0, 8.0];
/// The arrow's face when the raise can be paid for, and when it cannot.
pub const ARROW: u32 = 0xFFF0_FFE0;
const ARROW_SHADE: u32 = 0xFFB4_E39A;
const ARROW_EDGE: u32 = 0xFF17_3A0F;
const ARROW_OFF: u32 = 0xFF7A_8090;
const ARROW_OFF_SHADE: u32 = 0xFF5A_606C;
const ARROW_OFF_EDGE: u32 = 0xFF15_171C;

/// Draws the level-up arrow with its top left at `at`, bright when `lit`.
pub fn arrow(host: &dyn Host, batch: &mut HudBatch, at: [f32; 2], lit: bool) {
    let colours = if lit {
        [ARROW, ARROW_SHADE, ARROW_EDGE]
    } else {
        [ARROW_OFF, ARROW_OFF_SHADE, ARROW_OFF_EDGE]
    };
    pixel_art(host, batch, at, &ARROW_ART, |pixel| match pixel {
        b'A' => Some(colours[0]),
        b'a' => Some(colours[1]),
        b'O' => Some(colours[2]),
        _ => None,
    });
}

/// A raised button showing the level-up arrow before its label, both centred.
pub fn arrow_button(
    host: &dyn Host,
    batch: &mut HudBatch,
    r: &Rect,
    label: &str,
    look: Look,
    hover: bool,
) {
    button(host, batch, r, "", look, hover);
    let gap = 3.0;
    let width = ARROW_SIZE[0] + gap + host.text_width(label);
    let x = r.x + ((r.w - width) / 2.0).round();
    let y = r.y + ((r.h - ARROW_SIZE[1]) / 2.0).round();
    arrow(host, batch, [x, y], look != Look::Off);
    let colour = if look == Look::Off { DIM } else { WHITE };
    host.text(
        batch,
        label,
        [x + ARROW_SIZE[0] + gap, r.y + ((r.h - GLYPH) / 2.0).round()],
        colour,
        true,
    );
}

/// The settings gear, 8 by 8.
const GEAR: [&str; 8] = [
    "..O..O..", ".OOOOOO.", "OOO..OOO", ".O....O.", ".O....O.", "OOO..OOO", ".OOOOOO.", "..O..O..",
];

/// Draws the settings gear with its top left at `at`.
pub fn gear(host: &dyn Host, batch: &mut HudBatch, at: [f32; 2], argb: u32) {
    for (row, pixels) in GEAR.iter().enumerate() {
        for (col, pixel) in pixels.bytes().enumerate() {
            if pixel == b'O' {
                host.fill(
                    batch,
                    [at[0] + col as f32, at[1] + row as f32, 1.0, 1.0],
                    argb,
                );
            }
        }
    }
}

/// The mastery crown, 9 by 7: `O` outline, `Y` gold, `y` its shade, `R` red gems.
const CROWN_ART: [&str; 7] = [
    "O...O...O",
    "OO.OYO.OO",
    "OYOYYYOYO",
    "OYYYYYYYO",
    "OYRYRYRyO",
    "OyyyyyyyO",
    "OOOOOOOOO",
];
pub const CROWN_SIZE: [f32; 2] = [9.0, 7.0];
/// The crown's gold.
pub const CROWN: u32 = 0xFFFF_D23C;

/// Draws the mastery crown with its top left at `at`.
pub fn crown(host: &dyn Host, batch: &mut HudBatch, at: [f32; 2]) {
    pixel_art(host, batch, at, &CROWN_ART, |pixel| match pixel {
        b'O' => Some(0xFF4A_3206),
        b'Y' => Some(CROWN),
        b'y' => Some(0xFFC9_8E1C),
        b'R' => Some(0xFFE0_3A3A),
        _ => None,
    });
}

/// A mastered card's warm body.
pub const MASTER_CARD: u32 = 0xFF33_2B1D;
/// The bright core of the glint that runs along a mastered bar, and its softer edges.
pub const GLINT: u32 = 0xFFFF_FBEA;
const GLINT_HALO: u32 = 0x99FF_F2C4;
const BAR_GOLD_LIGHT: u32 = 0xFFFF_E79A;
const BAR_GOLD_DARK: u32 = 0xFFB3_7F22;
const PLAQUE: u32 = 0xFF4A_3A1B;
const SPARK: u32 = 0x00FF_F6D2;

/// A full bar in bevelled gold, with a glint `along` of the way (0 to 1) through its run. The glint enters from the
/// left, crosses, and leaves on the right, so for part of the run none shows.
pub fn mastered_bar(host: &dyn Host, batch: &mut HudBatch, r: &Rect, along: f32) {
    fill(host, batch, r, OUTLINE);
    let inner = r.shrunk(1.0);
    fill(host, batch, &inner, BAR_FULL);
    host.fill(batch, [inner.x, inner.y, inner.w, 1.0], BAR_GOLD_LIGHT);
    if inner.h > 2.0 {
        host.fill(
            batch,
            [inner.x, inner.y + inner.h - 1.0, inner.w, 1.0],
            BAR_GOLD_DARK,
        );
    }
    // The glint takes the first 60% of the run and rests for the rest.
    let travel = (along / 0.6).min(2.0);
    if travel >= 1.0 {
        return;
    }
    let centre = (inner.x - 2.0 + travel * (inner.w + 4.0)).round();
    for (offset, width, colour) in [
        (-2.0, 1.0, GLINT_HALO),
        (-1.0, 2.0, GLINT),
        (1.0, 1.0, GLINT_HALO),
    ] {
        let left = (centre + offset).max(inner.x);
        let right = (centre + offset + width).min(inner.x + inner.w);
        if right > left {
            host.fill(batch, [left, inner.y, right - left, inner.h], colour);
        }
    }
}

/// Small gold brackets in the four corners just inside `r`, each with a bright tip.
pub fn corner_brackets(host: &dyn Host, batch: &mut HudBatch, r: &Rect) {
    const ARM: f32 = 4.0;
    let (left, top) = (r.x + 2.0, r.y + 2.0);
    let (right, bottom) = (r.x + r.w - 3.0, r.y + r.h - 3.0);
    for (x, y, dx, dy) in [
        (left, top, 1.0, 1.0),
        (right, top, -1.0, 1.0),
        (left, bottom, 1.0, -1.0),
        (right, bottom, -1.0, -1.0),
    ] {
        let along_x = if dx > 0.0 { x } else { x - ARM + 1.0 };
        let along_y = if dy > 0.0 { y } else { y - ARM + 1.0 };
        host.fill(batch, [along_x, y, ARM, 1.0], FRAME);
        host.fill(batch, [x, along_y, 1.0, ARM], FRAME);
        host.fill(batch, [x, y, 1.0, 1.0], BAR_GOLD_LIGHT);
    }
}

/// A raised gold plaque, for a label of honour.
pub fn plaque(host: &dyn Host, batch: &mut HudBatch, r: &Rect) {
    fill(host, batch, r, OUTLINE);
    let inner = r.shrunk(1.0);
    fill(host, batch, &inner, PLAQUE);
    bevel(host, batch, &inner, FRAME_LIGHT, FRAME_DARK);
}

/// A four-pointed twinkle centred on `at`, `strength` (0 to 1) bright.
pub fn sparkle(host: &dyn Host, batch: &mut HudBatch, at: [f32; 2], strength: f32) {
    let alpha = |a: f32| ((a * strength.clamp(0.0, 1.0) * 255.0) as u32) << 24 | SPARK;
    if strength <= 0.05 {
        return;
    }
    let [x, y] = at;
    host.fill(batch, [x, y, 1.0, 1.0], alpha(1.0));
    host.fill(batch, [x - 1.0, y, 1.0, 1.0], alpha(0.7));
    host.fill(batch, [x + 1.0, y, 1.0, 1.0], alpha(0.7));
    host.fill(batch, [x, y - 1.0, 1.0, 1.0], alpha(0.7));
    host.fill(batch, [x, y + 1.0, 1.0, 1.0], alpha(0.7));
    if strength > 0.6 {
        host.fill(batch, [x - 2.0, y, 1.0, 1.0], alpha(0.3));
        host.fill(batch, [x + 2.0, y, 1.0, 1.0], alpha(0.3));
        host.fill(batch, [x, y - 2.0, 1.0, 1.0], alpha(0.3));
        host.fill(batch, [x, y + 2.0, 1.0, 1.0], alpha(0.3));
    }
}
