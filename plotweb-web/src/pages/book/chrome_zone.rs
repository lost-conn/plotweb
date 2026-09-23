//! Where a pointer move counts as "reaching for the chrome".
//!
//! The chrome-collapse (see `editor_writing` in `book/mod.rs`) used to return on
//! *any* mousemove — reasonable for "the author put the mouse down", wrong for
//! "the author is reading and their hand drifted an inch over the prose". Every
//! pixel of cursor travel over the manuscript itself was clearing the collapse
//! before it had a chance to do anything.
//!
//! [`in_reveal_zone`] narrows that to pointer positions near where the collapsed
//! chrome actually lives at rest: a strip down the left edge (the sidebar), a band
//! across the top (topbar + toolbar + the "Notes here" row), and a strip down the
//! right edge (the feedback rail, when one is open). Moving into prose in the
//! middle of the screen is a no-op; moving toward an edge means the author is
//! reaching for the thing that used to be there, so the chrome comes back. On a
//! phone-width viewport every move (i.e. every tap) still reveals. Escape
//! always works regardless of pointer position — it doesn't go through this.
//!
//! Pure function over plain coordinates, host-tested like [`super::notes_filter`]
//! and [`super::sigils`], deliberately taking no `web_sys` types so the test below
//! runs under plain `cargo test` (no wasm target, no browser).

/// Width of the left/right edge strips, in CSS pixels.
///
/// Deliberately much narrower than the resting sidebar (250px) or feedback rail
/// (300px) — this is a *reach for the edge* gesture, not "the mouse happens to be
/// somewhere over where the sidebar used to be". 56px is comfortably wider than a
/// scrollbar and narrow enough that crossing the midline of a normal-width window
/// doesn't accidentally cross it too.
const EDGE_ZONE_PX: f64 = 56.0;

/// Height of the top band, in CSS pixels, measured from the top of the viewport
/// (the editor layout is flush with it — there is no app header above it).
///
/// Sized to comfortably clear the resting topbar (~56px) + toolbar (~46px) +
/// "Notes here" row (~34px when present) with some slack, so a pointer aiming at
/// any of the three rows — not just the very top pixel — counts as a reach.
const TOP_ZONE_PX: f64 = 160.0;

/// At or below this width (the ≤768px mobile breakpoint shared with the CSS) every
/// move reveals, as before zones existed. There is no hovering pointer there — a
/// "mousemove" is the compatibility event a *tap* fires — so a tap anywhere is the
/// author asking for the chrome back, not a hand drifting over the prose.
const MOBILE_MAX_PX: f64 = 768.0;

/// Does a pointer at `(x, y)` on a viewport `viewport_w` wide count as reaching for
/// the collapsed chrome?
///
/// `x`/`y` are viewport-relative coordinates (`MouseEvent::client_x/y`), matching
/// the strips they're tested against — the left/top/right chrome all sits flush
/// against the viewport edges (see `BOOK_WORKSPACE_CSS`: `.book-workspace` is
/// `height: 100dvh` with no offset). `viewport_h` isn't needed: the top band is
/// measured from `y = 0` regardless of how tall the viewport is, and there is no
/// bottom zone (the footer is never faded — see `EDITOR_CSS`).
pub fn in_reveal_zone(x: f64, y: f64, viewport_w: f64) -> bool {
    if viewport_w <= MOBILE_MAX_PX {
        return true;
    }
    let left = x <= EDGE_ZONE_PX;
    let top = y <= TOP_ZONE_PX;
    let right = x >= viewport_w - EDGE_ZONE_PX;
    left || top || right
}

#[cfg(test)]
mod tests {
    use super::*;

    const VW: f64 = 1440.0;

    #[test]
    fn left_edge_reveals() {
        assert!(in_reveal_zone(0.0, 500.0, VW));
        assert!(in_reveal_zone(EDGE_ZONE_PX, 500.0, VW));
        assert!(!in_reveal_zone(EDGE_ZONE_PX + 1.0, 500.0, VW));
    }

    #[test]
    fn top_band_reveals() {
        assert!(in_reveal_zone(700.0, 0.0, VW));
        assert!(in_reveal_zone(700.0, TOP_ZONE_PX, VW));
        assert!(!in_reveal_zone(700.0, TOP_ZONE_PX + 1.0, VW));
    }

    #[test]
    fn right_edge_reveals() {
        assert!(in_reveal_zone(VW, 500.0, VW));
        assert!(in_reveal_zone(VW - EDGE_ZONE_PX, 500.0, VW));
        assert!(!in_reveal_zone(VW - EDGE_ZONE_PX - 1.0, 500.0, VW));
    }

    #[test]
    fn prose_in_the_middle_does_not_reveal() {
        // A point comfortably inside all three margins on a typical desktop width.
        assert!(!in_reveal_zone(700.0, 400.0, VW));
    }

    #[test]
    fn a_narrow_desktop_viewport_still_has_a_working_right_edge() {
        // Regression guard: on the narrowest desktop viewport the right zone must
        // not swallow the whole screen.
        let narrow = MOBILE_MAX_PX + 1.0;
        assert!(in_reveal_zone(narrow, 500.0, narrow));
        assert!(!in_reveal_zone(narrow / 2.0, 500.0, narrow));
    }

    #[test]
    fn mobile_reveals_on_any_move() {
        // A tap on the prose is a deliberate "give me the chrome back" on a phone.
        assert!(in_reveal_zone(187.0, 400.0, 375.0));
        assert!(in_reveal_zone(384.0, 400.0, MOBILE_MAX_PX));
    }
}
