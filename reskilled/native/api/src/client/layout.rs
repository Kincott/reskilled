//! Where the skills screen puts things, in GUI pixels, and which button a click lands on. No host: pure geometry.
//!
//! A window of 300 by 208 in the middle of the screen (the game's smallest is 320 by 240), with two views:
//!
//! ```text
//!  Reskilled                 XP levels: 23     < Mining                     XP levels: 23
//!  [icon] Mining      12/32  [icon] Melee   8/32    [ icon ] Level 12 of 32         [ Level up ]
//!         =====----  [^ 3]          ===---- [^ 3]   [      ] =========-------    Cost: 3 XP levels
//!         **                        *                        Perk points: 2
//!  ... four rows of two skills ...                  Perks
//!                                                   v Sure Grip *                          Owned
//!                                                   * Rich Seams *                     [ Unlock ]
//!                                                   # Deep Veins **              Needs Mining 16
//!  Focus  [Fighting]  [Roaming]  [Working]          Next unlocks
//!                                                   Level 16: Diamond pickaxes
//! ```

pub const PANEL_W: f32 = 300.0;
pub const PANEL_H: f32 = 208.0;
/// The title band at the top of the window.
pub const HEADER_H: f32 = 22.0;
const PAD: f32 = 8.0;
const INNER_W: f32 = PANEL_W - 2.0 * PAD;

/// The header's small square button (back).
const SMALL_BUTTON: f32 = 14.0;

/// The skill view: five perk rows on show.
pub const PERK_ROWS: usize = 5;
const PERK_TOP: f32 = 84.0;
const PERK_H: f32 = 17.0;
const PERK_STEP: f32 = 18.0;
const BUY_W: f32 = 56.0;
const BIG_SLOT: f32 = 36.0;
const RAISE_W: f32 = 92.0;
/// The scroll bar's track, inside the window's right padding.
const TRACK_W: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq)]
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

    /// A rectangle placed relative to this one's top left.
    fn at(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect {
            x: self.x + x,
            y: self.y + y,
            w,
            h,
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

/// One perk's row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerkRow {
    /// The perk's place in the skill's list.
    pub index: usize,
    pub rect: Rect,
    /// The unlock button (an owned perk's switch), inside the row at its end.
    pub buy: Rect,
    /// An owned perk's area button (its area shown in the world or not), just before the switch.
    pub area: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub panel: Rect,
    pub back: Rect,
    /// The sunken square the skill's big icon sits in.
    pub slot: Rect,
    pub bar: Rect,
    pub raise: Rect,
    /// The perk rows on show.
    pub rows: Vec<PerkRow>,
    /// The heading "Next unlocks" and its lines.
    pub preview: Rect,
    /// The bar from the last unlock reached to the next, beside the heading.
    pub next_bar: Rect,
    pub scroll: Option<Scroll>,
}

/// The first perk that may be on top when the skill has `perks`.
pub fn last_first_perk(perks: usize) -> usize {
    perks.saturating_sub(PERK_ROWS)
}

/// The skill view for a screen of `size`: `perks` rows from row `first`.
pub fn detail(size: [f32; 2], perks: usize, first: usize) -> Detail {
    let panel = panel(size);
    let first = first.min(last_first_perk(perks));
    let rows = (first..perks)
        .take(PERK_ROWS)
        .enumerate()
        .map(|(n, index)| {
            let rect = panel.at(PAD, PERK_TOP + n as f32 * PERK_STEP, INNER_W, PERK_H);
            PerkRow {
                index,
                rect,
                buy: rect.at(INNER_W - BUY_W - 2.0, 2.0, BUY_W, PERK_H - 4.0),
                area: rect.at(INNER_W - 2.0 * BUY_W - 6.0, 2.0, BUY_W, PERK_H - 4.0),
            }
        })
        .collect();
    let list_h = PERK_ROWS as f32 * PERK_STEP - (PERK_STEP - PERK_H);
    let list = panel.at(PAD, PERK_TOP, INNER_W, list_h);
    Detail {
        panel,
        back: panel.at(6.0, 4.0, SMALL_BUTTON, SMALL_BUTTON),
        slot: panel.at(PAD, 28.0, BIG_SLOT, BIG_SLOT),
        bar: panel.at(52.0, 42.0, 138.0, 6.0),
        raise: panel.at(PANEL_W - PAD - RAISE_W, 28.0, RAISE_W, 20.0),
        rows,
        preview: panel.at(PAD, 176.0, INNER_W, 28.0),
        next_bar: panel.at(PANEL_W - PAD - 140.0, 177.0, 100.0, 6.0),
        scroll: scroll(&list, first, PERK_ROWS, perks),
    }
}

/// What a click asks for. Skills and perks are named by their place in the lists the screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// Back to the overview.
    Back,
    /// The raise button of the skill on show.
    RaiseShown,
    /// A perk's unlock button, or an owned perk's switch.
    BuyPerk(usize),
    /// An owned perk's area button: show its area in the world, or stop.
    ToggleArea(usize),
}

/// The skill view's button under the point, if any.
pub fn hit_detail(d: &Detail, x: f32, y: f32) -> Option<Button> {
    if d.back.contains(x, y) {
        return Some(Button::Back);
    }
    if d.raise.contains(x, y) {
        return Some(Button::RaiseShown);
    }
    if let Some(row) = d.rows.iter().find(|r| r.area.contains(x, y)) {
        return Some(Button::ToggleArea(row.index));
    }
    let row = d.rows.iter().find(|r| r.buy.contains(x, y));
    row.map(|r| Button::BuyPerk(r.index))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALLEST: [f32; 2] = [320.0, 240.0];
    const USUAL: [f32; 2] = [427.0, 240.0];

    fn inside(outer: &Rect, inner: &Rect) -> bool {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.x + inner.w <= outer.x + outer.w
            && inner.y + inner.h <= outer.y + outer.h
    }

    fn apart(a: &Rect, b: &Rect) -> bool {
        a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y
    }

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

    #[test]
    fn the_skill_view_fits_five_perks_inside_the_window() {
        // Given a skill with five perks
        // When its view is laid out on the smallest screen
        let d = detail(SMALLEST, 5, 0);

        // Then everything is inside the window, the rows one under another, with no scrolling
        let parts = [&d.back, &d.slot, &d.bar, &d.raise, &d.preview];
        for part in parts.into_iter().chain(d.rows.iter().map(|r| &r.rect)) {
            assert!(inside(&d.panel.shrunk(3.0), part), "{part:?}");
        }
        assert_eq!(d.rows.len(), 5);
        for pair in d.rows.windows(2) {
            assert_eq!(pair[1].rect.y, pair[0].rect.y + 18.0);
            assert!(inside(&pair[0].rect, &pair[0].buy));
        }
        assert!(apart(&d.rows[4].rect, &d.preview) && apart(&d.slot, &d.raise));
        assert_eq!(d.scroll, None);
    }

    #[test]
    fn more_perks_than_fit_scroll() {
        // Given a skill with seven perks
        // When its view is laid out from its third perk, and from past the end
        let d = detail(USUAL, 7, 2);
        let past = detail(USUAL, 7, 6);

        // Then perks 2 to 6 show, past the end is the end, and a bar says where
        assert_eq!(
            d.rows.iter().map(|r| r.index).collect::<Vec<_>>(),
            (2..7).collect::<Vec<_>>()
        );
        assert_eq!(past, d);
        assert!(d.scroll.is_some());
    }

    #[test]
    fn skill_view_clicks_land_on_its_buttons() {
        // Given a skill view with three perks
        let d = detail(USUAL, 3, 0);
        let second = d.rows[1];

        // When its back button, raise button, a perk's unlock button and that perk's name are clicked
        let back = hit_detail(&d, d.back.x + 1.0, d.back.y + 1.0);
        let raise = hit_detail(&d, d.raise.x + 1.0, d.raise.y + 1.0);
        let buy = hit_detail(&d, second.buy.x + 1.0, second.buy.y + 1.0);
        let name = hit_detail(&d, second.rect.x + 1.0, second.rect.y + 1.0);

        // Then the buttons answer, and the name is not one
        assert_eq!(back, Some(Button::Back));
        assert_eq!(raise, Some(Button::RaiseShown));
        assert_eq!(buy, Some(Button::BuyPerk(1)));
        assert_eq!(name, None);
    }
}
