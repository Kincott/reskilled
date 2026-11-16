//! Geometry the screen's pages share, in GUI pixels: rectangles and scroll bars. No host: pure geometry.

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
