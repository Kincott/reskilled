//! Steve, drawn by the mod in pixel art: Horizon keeps the inventory's player preview and the players' skins from
//! mods, so the screen shows the default player instead.

use horizon_client::mod_api::{Host, HudBatch};

use super::paint::pixel_art;

/// Steve's head, front on, 8 by 8 pixels.
pub const HEAD: [&str; 8] = [
    "hhhhhhhh", "hhhhhhhh", "hHsssssh", "ssssssss", "sWBssBWs", "sssnnsss", "ssmmmmss", "ssssssss",
];

/// The colours of Steve's pixels.
pub fn colour(pixel: u8) -> Option<u32> {
    Some(match pixel {
        b'h' => 0xFF3B_2A1A,
        b'H' => 0xFF4A_3420,
        b's' => 0xFFC6_9473,
        b'S' => 0xFFB0_7F60,
        b'W' => 0xFFFF_FFFF,
        b'B' => 0xFF3D_5AA8,
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

/// Draws Steve's head with its top left at `at`, each pixel `scale` GUI pixels square.
pub fn draw_head(host: &dyn Host, batch: &mut HudBatch, at: [f32; 2], scale: f32) {
    draw_art(host, batch, &HEAD, at, scale);
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
