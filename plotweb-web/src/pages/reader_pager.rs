//! Native (desktop) pagination for the reader.
//!
//! The web reader paginates with CSS multi-column layout: `#reader-content` gets
//! a `column-width` of one page, the browser flows the chapter into columns,
//! `scroll_width` counts them and a `translateX` shows one. rinch's native
//! renderer (rinch-dom) has no multi-column layout, so on desktop those styles
//! do nothing and the chapter would run on past the bottom of the frame as one
//! tall column.
//!
//! Desktop therefore pages that single column **vertically**. Once rinch has
//! laid the chapter out, [`flow_blocks`] reads where every block landed
//! (`NodeHandle::get_layout_bounds`), [`page_ranges`] cuts the column into
//! page-tall slices — only ever between lines or between blocks, never through
//! a line — and [`show_page`] clips `#reader-content` to one slice and scrolls
//! it there. Nothing here is compiled for wasm32.

use rinch::prelude::NodeHandle;

/// The reader's body text size and line height (`.reader-content` in
/// `READER_CSS`). Only a first guess at a paragraph's line pitch: [`line_pitch`]
/// snaps it to the paragraph's measured height.
const BODY_FONT_PX: f64 = 17.0;
const BODY_LINE_HEIGHT: f64 = 1.85;

/// Slack for layout rounding when comparing positions (px).
const EPS: f64 = 0.5;

/// One block of the laid-out chapter, in `#reader-content`'s coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlowBlock {
    pub top: f64,
    pub bottom: f64,
    /// Line pitch for a block of text, which may be cut between lines;
    /// `None` for one that moves as a whole (a heading, an image, a rule).
    pub line: Option<f64>,
}

/// The visible slice `[start, end)` of the column for each page, in order.
///
/// A page ends where the next one starts, except that a page break landing in
/// the gap between two blocks starts the next page at the next block, so a
/// page never opens on a paragraph's bottom margin (CSS fragmentation drops it
/// the same way at a column break). Always at least one page.
pub fn page_ranges(blocks: &[FlowBlock], page_h: f64) -> Vec<(f64, f64)> {
    let end = blocks.iter().fold(0.0_f64, |m, b| m.max(b.bottom));
    if page_h <= 1.0 || end <= page_h + EPS {
        return vec![(0.0, end.max(0.0))];
    }
    let mut blocks = blocks.to_vec();
    blocks.sort_by(|a, b| a.top.total_cmp(&b.top));

    let mut pages = Vec::new();
    let mut start = 0.0_f64;
    // Every pass advances `start` by at least a line or a whole page, so this
    // only bounds a pathological input (a NaN, a zero line pitch).
    for _ in 0..100_000 {
        if end - start <= page_h + EPS {
            break;
        }
        let limit = start + page_h;
        let crossing = blocks
            .iter()
            .find(|b| b.top < limit - EPS && b.bottom > limit + EPS);
        let (page_end, next_start) = match crossing {
            // The limit falls between blocks: cut there, and start the next
            // page at the next block's top.
            None => {
                let next = blocks
                    .iter()
                    .map(|b| b.top)
                    .filter(|&t| t >= limit - EPS)
                    .fold(f64::INFINITY, f64::min);
                (limit, if next.is_finite() { next.max(limit) } else { end })
            }
            Some(b) => {
                let cut = match b.line {
                    // After the last whole line that fits.
                    Some(lh) if lh > 0.0 => {
                        let n = ((limit - b.top) / lh + 1e-6).floor().max(0.0);
                        b.top + n * lh
                    }
                    // Before the block, so it moves whole to the next page.
                    _ => b.top,
                };
                // Nothing fit — a line or an image taller than the remaining
                // page at its very top. Cut through it rather than loop.
                let cut = if cut <= start + EPS { limit } else { cut };
                (cut, cut)
            }
        };
        pages.push((start, page_end));
        if next_start <= start + EPS {
            break;
        }
        start = next_start;
    }
    pages.push((start, end));
    pages
}

/// Snap a guessed line pitch to a text block's measured height: a block of
/// text is a whole number of lines tall, so `height / round(height / guess)`
/// is its real pitch whenever the guess is within half a line per line.
pub fn line_pitch(height: f64, guess: f64) -> f64 {
    if height <= 0.0 || guess <= 0.0 {
        return guess;
    }
    let lines = (height / guess).round().max(1.0);
    height / lines
}

/// Inline elements, which flow inside their block's lines rather than
/// stacking as blocks of their own.
fn is_inline(tag: &str) -> bool {
    matches!(
        tag,
        "a" | "abbr" | "b" | "bdi" | "bdo" | "br" | "cite" | "code" | "del" | "dfn" | "em" | "i"
            | "ins" | "kbd" | "mark" | "q" | "s" | "samp" | "small" | "span" | "strong" | "sub"
            | "sup" | "time" | "u" | "var" | "wbr" | "font"
    )
}

/// Blocks that move to the next page whole rather than split between lines.
fn is_atomic(tag: &str) -> bool {
    matches!(
        tag,
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "img" | "hr" | "table" | "figure" | "svg"
            | "video" | "iframe"
    )
}

/// Every leaf block under `el`, in `el`'s coordinates: walks into containers
/// (a blockquote, a list) and stops at a block holding only inline content.
pub fn flow_blocks(el: &NodeHandle) -> Vec<FlowBlock> {
    let mut out = Vec::new();
    collect(el, 0.0, 0, &mut out);
    if out.is_empty() {
        // Bare text straight in the column: one block of body lines.
        let h = el.scroll_height();
        if h > 0.0 {
            out.push(FlowBlock {
                top: 0.0,
                bottom: h,
                line: Some(line_pitch(h, BODY_FONT_PX * BODY_LINE_HEIGHT)),
            });
        }
    }
    out
}

fn collect(parent: &NodeHandle, origin_y: f64, depth: usize, out: &mut Vec<FlowBlock>) {
    if depth > 32 {
        return;
    }
    for child in parent.children() {
        let Some(tag) = child.tag_name().map(|t| t.to_ascii_lowercase()) else {
            continue; // text / comment nodes between blocks
        };
        if is_inline(&tag) || tag == "style" || tag == "script" {
            continue;
        }
        let Some((_, y, _, h)) = child.get_layout_bounds() else { continue };
        let (top, h) = (origin_y + y as f64, h as f64);
        if h <= 0.0 {
            continue;
        }
        let has_block_child = !is_atomic(&tag)
            && child.children().iter().any(|c| {
                c.tag_name()
                    .map(|t| t.to_ascii_lowercase())
                    .is_some_and(|t| !is_inline(&t))
            });
        if has_block_child {
            collect(&child, top, depth + 1, out);
            continue;
        }
        let atomic = is_atomic(&tag) || child.query_selector("img").is_some();
        out.push(FlowBlock {
            top,
            bottom: top + h,
            line: (!atomic).then(|| line_pitch(h, BODY_FONT_PX * BODY_LINE_HEIGHT)),
        });
    }
}

/// Show one page: clip `#reader-content` to the page's slice of the column and
/// scroll it to the slice's top. `overflow: hidden` still scrolls when told to
/// (as in a browser), and keeps the next page's first line out of view.
pub fn show_page(el: &NodeHandle, range: (f64, f64)) {
    let (start, end) = range;
    el.set_styles(&[
        ("height", &format!("{}px", (end - start).max(0.0))),
        ("overflow", "hidden"),
    ]);
    el.set_scroll_top(start);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(top: f64, lines: usize, lh: f64) -> FlowBlock {
        FlowBlock { top, bottom: top + lines as f64 * lh, line: Some(lh) }
    }

    #[test]
    fn short_chapter_is_one_page() {
        let blocks = [text(0.0, 3, 30.0), text(106.0, 2, 30.0)];
        assert_eq!(page_ranges(&blocks, 500.0), vec![(0.0, 166.0)]);
    }

    #[test]
    fn empty_chapter_is_one_page() {
        assert_eq!(page_ranges(&[], 500.0), vec![(0.0, 0.0)]);
    }

    #[test]
    fn cuts_between_lines_never_through_one() {
        // One long paragraph of 30px lines; a 100px page holds 3 whole lines.
        let blocks = [text(0.0, 10, 30.0)];
        let pages = page_ranges(&blocks, 100.0);
        assert_eq!(pages, vec![(0.0, 90.0), (90.0, 180.0), (180.0, 270.0), (270.0, 300.0)]);
        for (s, e) in &pages {
            assert!(e - s <= 100.0);
            assert_eq!(s % 30.0, 0.0, "page starts on a line");
        }
    }

    #[test]
    fn break_in_a_margin_starts_next_page_at_next_block() {
        // Paragraph ends at 90, next starts at 106; the page limit (100) falls
        // in the margin.
        let blocks = [text(0.0, 3, 30.0), text(106.0, 3, 30.0)];
        assert_eq!(page_ranges(&blocks, 100.0), vec![(0.0, 100.0), (106.0, 196.0)]);
    }

    #[test]
    fn heading_moves_whole_to_next_page() {
        let blocks = [
            text(0.0, 2, 30.0),
            FlowBlock { top: 70.0, bottom: 120.0, line: None },
            text(140.0, 2, 30.0),
        ];
        let pages = page_ranges(&blocks, 100.0);
        assert_eq!(pages[0], (0.0, 70.0));
        assert_eq!(pages[1].0, 70.0);
    }

    #[test]
    fn image_taller_than_a_page_is_cut_rather_than_looping() {
        let blocks = [FlowBlock { top: 0.0, bottom: 250.0, line: None }];
        let pages = page_ranges(&blocks, 100.0);
        assert_eq!(pages, vec![(0.0, 100.0), (100.0, 200.0), (200.0, 250.0)]);
    }

    #[test]
    fn every_page_fits_and_pages_cover_the_text_in_order() {
        let lh = 17.0 * 1.85;
        let mut blocks = Vec::new();
        let mut y = 0.0;
        for i in 0..40 {
            let b = text(y, 1 + (i * 7) % 9, lh);
            y = b.bottom + 8.0;
            blocks.push(b);
        }
        let page_h = 611.0;
        let pages = page_ranges(&blocks, page_h);
        assert!(pages.len() > 1);
        for w in pages.windows(2) {
            assert!(w[0].1 <= w[1].0 + 1e-9, "pages overlap: {w:?}");
        }
        for (s, e) in &pages {
            assert!(e - s <= page_h + EPS, "page too tall: {s}..{e}");
            // A page edge never falls strictly inside a line.
            for b in &blocks {
                for edge in [*s, *e] {
                    if edge > b.top + EPS && edge < b.bottom - EPS {
                        let into = (edge - b.top) / lh;
                        assert!((into - into.round()).abs() < 1e-6, "cut through a line at {edge}");
                    }
                }
            }
        }
        assert_eq!(pages.last().unwrap().1, blocks.last().unwrap().bottom);
    }

    #[test]
    fn line_pitch_snaps_guess_to_measured_height() {
        assert!((line_pitch(94.35, 31.0) - 31.45).abs() < 1e-9);
        assert!((line_pitch(31.45, 30.0) - 31.45).abs() < 1e-9);
        assert_eq!(line_pitch(0.0, 31.45), 31.45);
    }
}
