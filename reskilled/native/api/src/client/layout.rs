//! Geometry the screen's pages share, in GUI pixels: rectangles, the small centred panel shown while the server's
//! definitions are on their way, and scroll bars. No host: pure geometry.

pub const PANEL_W: f32 = 300.0;
pub const PANEL_H: f32 = 208.0;
/// The title band at the top of the window.
pub const HEADER_H: f32 = 22.0;
/// The scroll bar's track, inside the window's right padding.
const TRACK_W: f32 = 3.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// As `[x, y, width, height]`, the form the drawing helpers take.
    pub fn array(&self) -> [f32; 4] {
        [self.x, self.y, self.w, self.h]
    }

    /// The rectangle `by` pixels in from every side.
    pub fn shrunk(&self, by: f32) -> Rect {
        Rect {
            x: self.x + by,
            y: self.y + by,
            w: self.w - 2.0 * by,
            h: self.h - 2.0 * by,
        }
    }
}

/// The window, centred on whole pixels and never off the top left of a screen smaller than it.
pub fn panel(size: [f32; 2]) -> Rect {
    Rect {
        x: ((size[0] - PANEL_W) / 2.0).floor().max(0.0),
        y: ((size[1] - PANEL_H) / 2.0).floor().max(0.0),
        w: PANEL_W,
        h: PANEL_H,
    }
}

/// A scroll bar: the track and the thumb in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scroll {
    pub track: Rect,
    pub thumb: Rect,
}

/// The bar for `shown` of `total` rows from row `first`, beside `area`; none when everything fits.
pub(super) fn scroll(area: &Rect, first: usize, shown: usize, total: usize) -> Option<Scroll> {
    if total <= shown {
        return None;
    }
    let track = Rect {
        x: area.x + area.w + 2.0,
        w: TRACK_W,
        ..*area
    };
    let thumb = Rect {
        y: track.y + (track.h * first as f32 / total as f32).round(),
        h: (track.h * shown as f32 / total as f32).round(),
        ..track
    };
    Some(Scroll { track, thumb })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALLEST: [f32; 2] = [320.0, 240.0];
    const USUAL: [f32; 2] = [427.0, 240.0];

    #[test]
    fn the_window_is_centred_and_fits_the_smallest_screen() {
        // Given the game's smallest screen and a usual one
        // When the window is placed on each
        let small = panel(SMALLEST);
        let usual = panel(USUAL);

        // Then it is whole on both, in the middle
        assert_eq!(
            (small.x, small.y, small.w, small.h),
            (10.0, 16.0, 300.0, 208.0)
        );
        assert_eq!((usual.x, usual.y), (63.0, 16.0));
        assert!(small.x + small.w <= SMALLEST[0] && small.y + small.h <= SMALLEST[1]);
    }
}
