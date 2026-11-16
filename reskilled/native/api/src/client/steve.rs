//! Steve, drawn by the mod in pixel art: Horizon keeps the inventory's player preview and the players' skins from
//! mods, so the screen shows the default player instead.

use horizon_client::mod_api::{Host, HudBatch};

use super::paint::pixel_art;

/// Steve, front on, 16 by 32 pixels: head 8 by 8, body and arms 12 high, legs 12 high.
pub const STEVE: [&str; 32] = [
    "....hhhhhhhh....",
    "....hhhhhhhh....",
    "....hHsssssh....",
    "....ssssssss....",
    "....sWBssBWs....",
    "....sssnnsss....",
    "....ssmmmmss....",
    "....ssssssss....",
    "ttttTTsssTTTtttt",
    "ttttTTTssTTTtttt",
    "ttttTTTTTTTTtttt",
    "ttttTTTTTTTTtttt",
    "ssssTTTTTTTTssss",
    "ssssTTTTTTTTssss",
    "ssssTTTTTTTTssss",
    "ssssTTTTTTTTssss",
    "ssssTTTTTTTTssss",
    "ssssTTTTTTTTssss",
    "ssssTTTTTTTTssss",
    "SSSSTTTTTTTTSSSS",
    "....ppppPPPP....",
    "....ppppPPPP....",
    "....ppppPPPP....",
    "....ppppPPPP....",
    "....ppppPPPP....",
    "....ppppPPPP....",
    "....ppppPPPP....",
    "....ppppPPPP....",
    "....ppppPPPP....",
    "....ppppPPPP....",
    "....ggggGGGG....",
    "....ggggGGGG....",
];

/// Steve's head: the first eight rows of his figure, columns 4 to 11.
const HEAD_ROWS: usize = 8;
const HEAD_LEFT: usize = 4;
/// His eyes' row, the columns of their whites and pupils looking ahead, and his face's middle column.
const EYE_ROW: usize = 4;
const EYES: [[usize; 2]; 2] = [[5, 6], [9, 10]];
const AHEAD: [usize; 2] = [6, 9];
const FACE_MIDDLE: f32 = 8.0;
/// How far the mouse must be from his face, in GUI pixels, before his eyes follow it.
const LOOK_DEAD_ZONE: f32 = 8.0;
pub const PUPIL: u32 = 0xFF3D_5AA8;

/// The colours of Steve's pixels.
pub fn colour(pixel: u8) -> Option<u32> {
    Some(match pixel {
        b'h' => 0xFF3B_2A1A,
        b'H' => 0xFF4A_3420,
        b's' => 0xFFC6_9473,
        b'S' => 0xFFB0_7F60,
        b'W' => 0xFFFF_FFFF,
        b'B' => PUPIL,
        b'n' => 0xFF9A_6B52,
        b'm' => 0xFF7A_4B3B,
        b't' => 0xFF2B_A3A3,
        b'T' => 0xFF1F_8C8C,
        b'p' => 0xFF3F_3FA8,
        b'P' => 0xFF35_358F,
        b'g' => 0xFF6B_6B6B,
        b'G' => 0xFF5A_5A5A,
        _ => return None,
    })
}

/// Draws Steve's head with its top left at `at`, each pixel `scale` GUI pixels square, looking ahead.
pub fn draw_head(host: &dyn Host, batch: &mut HudBatch, at: [f32; 2], scale: f32) {
    let rows: Vec<&str> = STEVE[..HEAD_ROWS]
        .iter()
        .map(|row| &row[HEAD_LEFT..HEAD_LEFT + 8])
        .collect();
    draw_art(host, batch, &rows, at, scale);
}

/// Draws Steve with his top left at `at`, each pixel `scale` GUI pixels square, his eyes turned toward `look` (the
/// mouse): a pixel left or right, and a GUI pixel up or down, once it is more than a few pixels from his face.
pub fn draw_steve(host: &dyn Host, batch: &mut HudBatch, at: [f32; 2], scale: f32, look: [f32; 2]) {
    let face = [
        at[0] + FACE_MIDDLE * scale,
        at[1] + (EYE_ROW as f32 + 0.5) * scale,
    ];
    let toward = |from: f32, to: f32| {
        if to < from - LOOK_DEAD_ZONE {
            -1
        } else if to > from + LOOK_DEAD_ZONE {
            1
        } else {
            0
        }
    };
    let (across, down) = (toward(face[0], look[0]), toward(face[1], look[1]));
    for (r, line) in STEVE.iter().enumerate() {
        for (c, pixel) in line.bytes().enumerate() {
            // The eyes are drawn white here, their pupils after
            let pixel = if pixel == b'B' { b'W' } else { pixel };
            if let Some(argb) = colour(pixel) {
                let x = at[0] + c as f32 * scale;
                let y = at[1] + r as f32 * scale;
                host.fill(batch, [x, y, scale, scale], argb);
            }
        }
    }
    let lift = down as f32;
    for (eye, ahead) in EYES.iter().zip(AHEAD) {
        let column = match across {
            -1 => eye[0],
            1 => eye[1],
            _ => ahead,
        };
        let x = at[0] + column as f32 * scale;
        let y = at[1] + EYE_ROW as f32 * scale;
        // Looking up or down, the pupil keeps to the top or bottom of the eye, a GUI pixel shorter
        let (top, h) = if lift == 0.0 || scale <= 1.0 {
            (y, scale)
        } else if lift < 0.0 {
            (y, scale - 1.0)
        } else {
            (y + 1.0, scale - 1.0)
        };
        host.fill(batch, [x, top, scale, h], PUPIL);
    }
}

/// Draws rows of Steve's pixels, each `scale` GUI pixels square.
fn draw_art(host: &dyn Host, batch: &mut HudBatch, rows: &[&str], at: [f32; 2], scale: f32) {
    if scale == 1.0 {
        pixel_art(host, batch, at, rows, colour);
        return;
    }
    for (r, line) in rows.iter().enumerate() {
        for (c, pixel) in line.bytes().enumerate() {
            if let Some(argb) = colour(pixel) {
                let x = at[0] + c as f32 * scale;
                let y = at[1] + r as f32 * scale;
                host.fill(batch, [x, y, scale, scale], argb);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use horizon_client::mod_api::HudBatch;

    use super::*;
    use crate::test_host::{Paint, TestHost};

    #[test]
    fn steve_looks_toward_the_mouse() {
        // Given Steve at (100, 50), three GUI pixels to his pixel: his face's middle is at (124, 63)
        let host = TestHost::default();
        let pupils = |look: [f32; 2]| -> Vec<f32> {
            host.painted.borrow_mut().clear();
            draw_steve(
                &host,
                &mut HudBatch::new([427, 240]),
                [100.0, 50.0],
                3.0,
                look,
            );
            host.painted
                .borrow()
                .iter()
                .filter_map(|p| match p {
                    Paint::Fill { rect, argb } if *argb == PUPIL => Some(rect[0]),
                    _ => None,
                })
                .collect()
        };

        // When the mouse is far to his left, far to his right, and on his face
        let left = pupils([0.0, 63.0]);
        let right = pupils([400.0, 63.0]);
        let ahead = pupils([124.0, 63.0]);

        // Then his two pupils sit one pixel further left looking left than looking right, and inward looking ahead
        assert_eq!(left.len(), 2);
        assert_eq!(left, right.iter().map(|x| x - 3.0).collect::<Vec<_>>());
        assert_eq!(ahead, vec![118.0, 127.0]);
    }
}
