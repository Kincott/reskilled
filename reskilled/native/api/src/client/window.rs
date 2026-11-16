//! The window every page of the screen sits in: a brass frame, and tabs above it for Home, each skill and Options.
//! The open tab joins the frame.

use horizon_client::mod_api::{Host, HudBatch};
use skills_core::progress::{PlayerState, points_available};
use skills_core::wire::ClientDefs;

use super::cache::Tab;
use super::layout::Rect;

/// The window's frame and body colours.
const BLACK: u32 = 0xFF07_080B;
const BODY: u32 = 0xFF1A_1D26;
const SPECK: u32 = 0xFF1E_222C;
const BRASS_LIGHT: u32 = 0xFFE6_C36A;
const BRASS: u32 = 0xFFAD_8436;
const BRASS_DARK: u32 = 0xFF5A_4116;
const TAB_BACK: u32 = 0xFF14_161D;
const TAB_DIM: u32 = 0x5509_0A0D;
/// Tabs: the top of their row, their height, their width and how far apart they sit, at most and at least.
const TAB_TOP: f32 = 2.0;
const TAB_H: f32 = 28.0;
const TAB_W: f32 = 28.0;
const PITCH: f32 = 31.0;
const LEAST_PITCH: f32 = 25.0;
/// The frame's top, its greatest width, the margin under it and to the GUI's sides.
const FRAME_TOP: f32 = 30.0;
const FRAME_MOST_W: f32 = 383.0;
const MARGIN: f32 = 8.0;
const BOTTOM: f32 = 6.0;

/// The window: its frame, the body inside it for a page, its tabs, and the arrows that scroll the skill tabs when
/// they do not all fit.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowLayout {
    pub frame: Rect,
    pub body: Rect,
    pub tabs: Vec<(Tab, Rect)>,
    pub scroll_left: Option<Rect>,
    pub scroll_right: Option<Rect>,
}

/// The window on a GUI of `size` with `skills` skill tabs, the first shown being `first_tab`.
pub fn window(size: [f32; 2], skills: usize, first_tab: usize) -> WindowLayout {
    let w = (size[0] - 2.0 * MARGIN).min(FRAME_MOST_W).floor();
    let frame = Rect {
        x: ((size[0] - w) / 2.0).floor(),
        y: FRAME_TOP,
        w,
        h: (size[1] - FRAME_TOP - BOTTOM).floor(),
    };
    let body = Rect {
        x: frame.x + 8.0,
        y: frame.y + 9.0,
        w: frame.w - 16.0,
        h: frame.h - 17.0,
    };
    let tab = |x: f32, w: f32| Rect {
        x,
        y: TAB_TOP,
        w,
        h: TAB_H,
    };
    let home_x = frame.x + 6.0;
    let options_x = frame.x + frame.w - 34.0;
    let start = home_x + PITCH;
    let room = options_x - 3.0 - start;
    let mut tabs = vec![(Tab::Home, tab(home_x, TAB_W))];
    let (mut scroll_left, mut scroll_right) = (None, None);
    let fitting = if skills == 0 {
        PITCH
    } else {
        (room / skills as f32).floor().min(PITCH)
    };
    if skills == 0 || fitting >= LEAST_PITCH {
        for i in 0..skills {
            tabs.push((
                Tab::Skill(i),
                tab(start + i as f32 * fitting, fitting - 3.0),
            ));
        }
    } else {
        let slots = (room / LEAST_PITCH).floor() as usize;
        let first = first_tab.min(skills.saturating_sub(1));
        let left = first > 0;
        let right = first + slots - usize::from(left) < skills;
        let shown = slots - usize::from(left) - usize::from(right);
        let mut x = start;
        if left {
            scroll_left = Some(tab(x, LEAST_PITCH - 3.0));
            x += LEAST_PITCH;
        }
        for i in first..(first + shown).min(skills) {
            tabs.push((Tab::Skill(i), tab(x, LEAST_PITCH - 3.0)));
            x += LEAST_PITCH;
        }
        if right {
            scroll_right = Some(tab(
                start + (slots - 1) as f32 * LEAST_PITCH,
                LEAST_PITCH - 3.0,
            ));
        }
    }
    tabs.push((Tab::Options, tab(options_x, TAB_W)));
    WindowLayout {
        frame,
        body,
        tabs,
        scroll_left,
        scroll_right,
    }
}

/// The tab under the point.
pub fn hit_tab(w: &WindowLayout, x: f32, y: f32) -> Option<Tab> {
    w.tabs
        .iter()
        .find(|(_, r)| r.contains(x, y))
        .map(|(tab, _)| *tab)
}

fn bevel(host: &dyn Host, batch: &mut HudBatch, r: &Rect, light: u32, dark: u32) {
    host.fill(batch, [r.x, r.y, r.w, 1.0], light);
    host.fill(batch, [r.x, r.y + 1.0, 1.0, r.h - 1.0], light);
    host.fill(batch, [r.x + 1.0, r.y + r.h - 1.0, r.w - 1.0, 1.0], dark);
    host.fill(batch, [r.x + r.w - 1.0, r.y + 1.0, 1.0, r.h - 2.0], dark);
}

/// The frame: a black outline, a bevelled brass border, the body with faint specks, studs in the corners.
fn draw_frame(host: &dyn Host, batch: &mut HudBatch, f: &Rect) {
    host.fill(batch, f.array(), BLACK);
    let brass = f.shrunk(1.0);
    host.fill(batch, brass.array(), BRASS);
    bevel(host, batch, &brass, BRASS_LIGHT, BRASS_DARK);
    host.fill(batch, f.shrunk(3.0).array(), BRASS_DARK);
    let body = f.shrunk(4.0);
    host.fill(batch, body.array(), BODY);
    let (x0, y0) = (body.x as i32, body.y as i32);
    for y in 0..body.h as i32 {
        for x in 0..body.w as i32 {
            if ((x0 + x) * 7 + (y0 + y) * 13) % 23 == 0 {
                host.fill(batch, [(x0 + x) as f32, (y0 + y) as f32, 1.0, 1.0], SPECK);
            }
        }
    }
    for (x, y) in [
        (f.x + 1.0, f.y + 1.0),
        (f.x + f.w - 4.0, f.y + 1.0),
        (f.x + 1.0, f.y + f.h - 4.0),
        (f.x + f.w - 4.0, f.y + f.h - 4.0),
    ] {
        host.fill(batch, [x, y, 3.0, 3.0], BRASS_LIGHT);
        host.fill(batch, [x + 1.0, y + 1.0, 1.0, 1.0], BRASS_DARK);
    }
}

/// A tab's frame: the open tab is taller and brass-lit, the others dark and set back.
fn draw_tab_frame(host: &dyn Host, batch: &mut HudBatch, r: &Rect, open: bool) {
    let top = if open { r.y } else { r.y + 4.0 };
    let h = r.y + r.h + 2.0 - top;
    host.fill(batch, [r.x, top, r.w, h], BLACK);
    host.fill(
        batch,
        [r.x + 1.0, top + 1.0, r.w - 2.0, h - 1.0],
        if open { BRASS } else { BRASS_DARK },
    );
    if open {
        host.fill(batch, [r.x + 1.0, top + 1.0, r.w - 2.0, 1.0], BRASS_LIGHT);
        host.fill(batch, [r.x + 1.0, top + 2.0, 1.0, h - 2.0], BRASS_LIGHT);
    }
    host.fill(
        batch,
        [r.x + 3.0, top + 3.0, r.w - 6.0, h - 3.0],
        if open { BODY } else { TAB_BACK },
    );
}

/// What a tab shows: Home Steve's head, a skill its icon (and a pip while it has points to spend), Options a gear.
fn draw_tab_face(
    host: &dyn Host,
    batch: &mut HudBatch,
    r: &Rect,
    tab: Tab,
    defs: Option<&ClientDefs>,
    state: &PlayerState,
    open: bool,
) {
    let top = if open { r.y } else { r.y + 4.0 };
    let icon_at = [(r.x + (r.w - 16.0) / 2.0).floor(), top + 6.0];
    match tab {
        Tab::Home => super::steve::draw_head(host, batch, icon_at, 2.0),
        Tab::Options => super::paint::gear(
            host,
            batch,
            [icon_at[0] + 3.0, icon_at[1] + 3.0],
            0xFFC9_CED8,
        ),
        Tab::Skill(i) => {
            let Some(defs) = defs else { return };
            let Some((id, skill)) = defs.defs.skills.iter().nth(i) else {
                return;
            };
            let level = skills_core::progress::level(state, &defs.defs, id);
            host.item(batch, skill.icon_at(level), icon_at, 16.0);
            if points_available(&defs.defs, state, id) > 0 {
                host.fill(batch, [r.x + r.w - 9.0, top + 4.0, 5.0, 5.0], BLACK);
                host.fill(
                    batch,
                    [r.x + r.w - 8.0, top + 5.0, 3.0, 3.0],
                    super::paint::PIP,
                );
            }
        }
    }
    if !open {
        host.fill(
            batch,
            [r.x + 3.0, top + 3.0, r.w - 6.0, r.y + r.h - top - 1.0],
            TAB_DIM,
        );
    }
}

/// Draws the window: the closed tabs, the frame, then the open tab joined to it, and the scroll arrows.
pub fn draw_window(
    host: &dyn Host,
    batch: &mut HudBatch,
    w: &WindowLayout,
    open: Tab,
    defs: Option<&ClientDefs>,
    state: &PlayerState,
) {
    for (tab, r) in w.tabs.iter().filter(|(tab, _)| *tab != open) {
        draw_tab_frame(host, batch, r, false);
        draw_tab_face(host, batch, r, *tab, defs, state, false);
    }
    draw_frame(host, batch, &w.frame);
    if let Some((tab, r)) = w.tabs.iter().find(|(tab, _)| *tab == open) {
        draw_tab_frame(host, batch, r, true);
        host.fill(batch, [r.x + 3.0, w.frame.y, r.w - 6.0, 4.0], BODY);
        draw_tab_face(host, batch, r, *tab, defs, state, true);
    }
    for (arrow, r) in [("<", w.scroll_left), (">", w.scroll_right)] {
        if let Some(r) = r {
            draw_tab_frame(host, batch, &r, false);
            super::paint::text_centred(
                host,
                batch,
                arrow,
                &Rect { y: r.y + 4.0, ..r },
                super::paint::WHITE,
            );
        }
    }
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

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    #[test]
    fn the_window_and_tabs_fit_both_sizes() {
        for size in [SMALLEST, USUAL] {
            // Given eight skills
            // When the window is laid out
            let w = window(size, 8, 0);

            // Then the frame is inside the GUI, the body inside the frame, every tab inside the GUI's width, no two
            // tabs overlap, and there are Home, eight skills and Options with no scroll arrows
            let gui = Rect {
                x: 0.0,
                y: 0.0,
                w: size[0],
                h: size[1],
            };
            assert!(inside(&gui, &w.frame), "{size:?}: {:?}", w.frame);
            assert!(inside(&w.frame, &w.body), "{size:?}");
            assert_eq!(w.tabs.len(), 10, "{size:?}");
            for (i, (_, a)) in w.tabs.iter().enumerate() {
                assert!(
                    a.x >= 0.0 && a.x + a.w <= size[0],
                    "{size:?}: tab {i} {a:?}"
                );
                for (_, b) in &w.tabs[i + 1..] {
                    assert!(!overlap(a, b), "{size:?}: {a:?} and {b:?}");
                }
            }
            assert_eq!(w.tabs[0].0, Tab::Home);
            assert_eq!(w.tabs[9].0, Tab::Options);
            assert!(w.scroll_left.is_none() && w.scroll_right.is_none());
        }
    }

    #[test]
    fn many_skills_scroll_the_tabs() {
        // Given fourteen skills at the smallest size
        // When the tabs are laid out from the start, and from the fourth skill
        let start = window(SMALLEST, 14, 0);
        let later = window(SMALLEST, 14, 3);

        // Then they scroll: a right arrow at the start, both arrows later, and the first skill tab is skill 3
        assert!(start.scroll_right.is_some() && start.scroll_left.is_none());
        assert!(later.scroll_left.is_some());
        let first_skill = later.tabs.iter().find_map(|(tab, _)| match tab {
            Tab::Skill(i) => Some(*i),
            _ => None,
        });
        assert_eq!(first_skill, Some(3));
    }

    #[test]
    fn a_click_finds_its_tab() {
        let w = window(USUAL, 8, 0);
        let (tab, rect) = w.tabs[3];
        assert_eq!(hit_tab(&w, rect.x + 4.0, rect.y + 4.0), Some(tab));
        assert_eq!(hit_tab(&w, w.body.x + 4.0, w.body.y + 4.0), None);
    }
}
