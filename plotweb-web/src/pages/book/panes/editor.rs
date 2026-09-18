//! Editor pane: the chapter prose surface, its topbar (title, word count, save
//! status), the save-alert banner, and the feedback sidebar. Also home to
//! `scroll_to_text_in_editor` and its text-matching helpers, shared with the
//! feedback cards (`super::super::feedback`) that jump into this pane.

use rinch::prelude::*;
use wasm_bindgen::JsCast;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};

use crate::pages::editor_utils;
use crate::rinch_backend::Editor;

use super::super::state::BookState;
use super::super::BookPane;
use super::super::feedback::editor_feedback_item;

/// Char-safe truncation for log messages (byte-slicing panics on multibyte UTF-8).
fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect::<String>()
}

/// Scroll to and highlight a piece of text within the editor.
/// Walks all text nodes in `#editor-main`, finds a match for `selected_text`
/// (disambiguating with `context_block` if there are multiple matches),
/// creates a Selection over it, scrolls it into view, and applies a flash highlight.
pub(in crate::pages::book) fn scroll_to_text_in_editor(selected_text: &str, context_block: &str) {
    log::info!("scroll_to_text_in_editor called, text='{}'", truncate_chars(selected_text, 40));
    if selected_text.is_empty() {
        return;
    }

    let doc = match crate::platform::window().and_then(|w| w.document()) {
        Some(d) => d,
        None => { log::warn!("scroll_to_text: no document"); return; }
    };
    let editor = match doc.query_selector("#editor-main").ok().flatten() {
        Some(el) => el,
        None => { log::warn!("scroll_to_text: no #editor-main"); return; }
    };

    // Collect all text content with node boundaries.
    // Insert a single space at block-element boundaries so that cross-paragraph
    // selections (which include \n between blocks) can still be matched after
    // we normalize whitespace in both the needle and the haystack.
    let mut text_nodes: Vec<web_sys::Node> = Vec::new();
    let mut full_text = String::new();
    let mut node_offsets: Vec<(usize, usize)> = Vec::new(); // (start_byte, end_byte) in full_text

    collect_text_nodes_spaced(&editor.into(), &mut text_nodes, &mut full_text, &mut node_offsets);

    // Normalize whitespace: collapse runs of whitespace into a single space.
    // Build a mapping from normalized byte offset back to original byte offset.
    let (norm_text, norm_to_orig) = normalize_whitespace_map(&full_text);

    // Normalize the needle the same way
    let norm_needle = normalize_whitespace(selected_text);
    if norm_needle.is_empty() {
        return;
    }

    // Find all matches of the normalized needle in the normalized text
    let mut matches: Vec<usize> = Vec::new();
    let mut search_start = 0;
    while let Some(pos) = norm_text[search_start..].find(&norm_needle) {
        matches.push(search_start + pos);
        search_start += pos + 1;
    }

    // Fuzzy fallback: if no exact match, try progressively shorter prefixes
    // (the author may have edited the text after feedback was left)
    if matches.is_empty() && norm_needle.len() > 20 {
        let min_len = norm_needle.len() / 2;
        let mut try_len = norm_needle.len() - 1;
        while try_len >= min_len {
            // Ensure we don't cut in the middle of a UTF-8 char
            while try_len > 0 && !norm_needle.is_char_boundary(try_len) {
                try_len -= 1;
            }
            if try_len == 0 { break; }
            let prefix = &norm_needle[..try_len];
            search_start = 0;
            while let Some(pos) = norm_text[search_start..].find(prefix) {
                matches.push(search_start + pos);
                search_start += pos + 1;
            }
            if !matches.is_empty() {
                log::info!("scroll_to_text: fuzzy match with {}/{} chars", try_len, norm_needle.len());
                break;
            }
            try_len -= 1;
        }
    }

    if matches.is_empty() {
        log::warn!("scroll_to_text: '{}' not found in editor content ({} chars)", truncate_chars(selected_text, 40), full_text.len());
        return;
    }
    log::info!("scroll_to_text: found {} match(es)", matches.len());

    // Determine the effective needle length used (may be shorter if fuzzy matched)
    let effective_needle_len = if matches.is_empty() { norm_needle.len() } else {
        // Check if first match is an exact-length match
        let first = matches[0];
        if first + norm_needle.len() <= norm_text.len() && &norm_text[first..first + norm_needle.len()] == norm_needle.as_str() {
            norm_needle.len()
        } else {
            // Fuzzy: find the prefix length that matched
            let mut l = norm_needle.len() - 1;
            while l > 0 {
                if norm_needle.is_char_boundary(l) && norm_text[first..].starts_with(&norm_needle[..l]) {
                    break;
                }
                l -= 1;
            }
            l
        }
    };

    // Pick the best match using context_block for disambiguation
    let norm_context = normalize_whitespace(context_block);
    let norm_match_start = if matches.len() == 1 || norm_context.is_empty() {
        matches[0]
    } else {
        // Find the match whose surrounding text best matches context_block
        // by looking for the longest common substring between context and surrounding text
        *matches.iter().min_by_key(|&&pos| {
            let ctx_start = pos.saturating_sub(norm_context.len());
            let ctx_end = (pos + effective_needle_len + norm_context.len()).min(norm_text.len());
            let surrounding = &norm_text[ctx_start..ctx_end];
            // Score: length of longest contiguous overlap with context
            let score = longest_common_substring(surrounding, &norm_context);
            norm_context.len().saturating_sub(score)
        }).unwrap()
    };

    // Map normalized offsets back to original byte offsets
    let match_start = norm_to_orig[norm_match_start];
    let match_end = if norm_match_start + effective_needle_len < norm_to_orig.len() {
        norm_to_orig[norm_match_start + effective_needle_len]
    } else {
        full_text.len()
    };

    // Map byte offsets back to text node + byte offset within that node,
    // then convert to UTF-16 code unit offsets for the DOM Range/Text APIs.
    let (start_node_idx, start_byte_off) = byte_offset_to_node(&node_offsets, match_start);
    let (end_node_idx, end_byte_off) = byte_offset_to_node(&node_offsets, match_end);

    // Convert UTF-8 byte offsets to UTF-16 code unit offsets
    let start_text = text_nodes[start_node_idx].text_content().unwrap_or_default();
    let end_text = text_nodes[end_node_idx].text_content().unwrap_or_default();
    let start_u16 = utf8_byte_to_utf16(&start_text, start_byte_off);
    let end_u16 = utf8_byte_to_utf16(&end_text, end_byte_off);

    log::info!("scroll_to_text: node {}[{}] -> node {}[{}] (u16: {} -> {})",
        start_node_idx, start_byte_off, end_node_idx, end_byte_off, start_u16, end_u16);

    // Create a Range over the matched text
    let range = match doc.create_range() {
        Ok(r) => r,
        Err(e) => { log::warn!("scroll_to_text: create_range failed: {:?}", e); return; }
    };
    if let Err(e) = range.set_start(&text_nodes[start_node_idx], start_u16) {
        log::warn!("scroll_to_text: set_start failed: {:?}", e);
        return;
    }
    if let Err(e) = range.set_end(&text_nodes[end_node_idx], end_u16) {
        log::warn!("scroll_to_text: set_end failed: {:?}", e);
        return;
    }

    // Scroll the start of the match into view within .editor-scroll
    let start_parent = match text_nodes[start_node_idx].parent_element() {
        Some(el) => el,
        None => return,
    };
    if let Ok(Some(scroll_container)) = doc.query_selector(".editor-scroll") {
        let el_rect = start_parent.get_bounding_client_rect();
        let container_rect = scroll_container.get_bounding_client_rect();
        let scroll_top = scroll_container.scroll_top() as f64;
        // Scroll so the target is roughly 1/3 from the top of the container
        let target = scroll_top + el_rect.top() - container_rect.top()
            - container_rect.height() / 3.0;
        scroll_container.set_scroll_top(target.max(0.0) as i32);
    }

    // Select the range so it's visually highlighted with the browser's native selection
    if let Some(selection) = doc.get_selection().ok().flatten() {
        selection.remove_all_ranges().ok();
        selection.add_range(&range).ok();
    }

    // Also apply a temporary flash highlight to each text node in the range.
    // We wrap individual text nodes (not the whole range) to avoid the
    // cross-element-boundary error that surround_contents throws.
    let mut marks: Vec<web_sys::Element> = Vec::new();
    for idx in start_node_idx..=end_node_idx {
        if idx >= text_nodes.len() { break; }
        let node = &text_nodes[idx];
        let text = node.text_content().unwrap_or_default();
        if text.is_empty() { continue; }

        // Figure out the slice of this text node that's part of the match (in UTF-16 units)
        let node_u16_len: u32 = text.encode_utf16().count() as u32;
        let slice_start_u16 = if idx == start_node_idx { start_u16 } else { 0 };
        let slice_end_u16 = if idx == end_node_idx { end_u16 } else { node_u16_len };

        if slice_start_u16 == 0 && slice_end_u16 == node_u16_len {
            // Whole text node is in the match — wrap it directly
            if let Ok(mark) = doc.create_element("mark") {
                mark.set_class_name("feedback-highlight");
                if let Some(parent) = node.parent_node() {
                    parent.insert_before(&mark, Some(node)).ok();
                    mark.append_child(node).ok();
                    marks.push(mark);
                }
            }
        } else {
            // Partial text node — split and wrap the matched portion
            if let Ok(text_node) = node.clone().dyn_into::<web_sys::Text>() {
                // Split at slice_end first (so offsets stay valid), then at slice_start
                let _after = text_node.split_text(slice_end_u16).ok();
                let matched = text_node.split_text(slice_start_u16).ok();
                if let Some(matched_node) = matched {
                    if let Ok(mark) = doc.create_element("mark") {
                        mark.set_class_name("feedback-highlight");
                        if let Some(parent) = matched_node.parent_node() {
                            parent.insert_before(&mark, Some(&matched_node)).ok();
                            mark.append_child(&matched_node).ok();
                            marks.push(mark);
                        }
                    }
                }
            }
        }
    }

    // Remove marks after animation (2s)
    // Web-only: `marks` are DOM nodes this function found by querying the
    // document, so on native it is always empty and there is nothing to unwind.
    if !marks.is_empty() {
        crate::web_only! {
        let closure = wasm_bindgen::closure::Closure::once(move || {
            for mark in &marks {
                if let Some(parent) = mark.parent_node() {
                    while let Some(child) = mark.first_child() {
                        parent.insert_before(&child, Some(mark)).ok();
                    }
                    parent.remove_child(mark).ok();
                }
            }
            // Normalize to merge split text nodes back together
            if let Some(doc) = crate::platform::window().and_then(|w| w.document()) {
                if let Ok(Some(editor)) = doc.query_selector("#editor-main") {
                    editor.normalize();
                }
            }
        });
        if let Some(window) = crate::platform::window() {
            window.set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                2000,
            ).ok();
        }
        closure.forget();
        }
    }
}

/// Convert a UTF-8 byte offset within a string to a UTF-16 code unit offset.
/// DOM APIs (Range.setStart, Text.splitText) use UTF-16 offsets,
/// but Rust's str::find returns UTF-8 byte positions.
fn utf8_byte_to_utf16(text: &str, byte_offset: usize) -> u32 {
    let clamped = byte_offset.min(text.len());
    text[..clamped].encode_utf16().count() as u32
}

/// Recursively collect text nodes under `node`, inserting a space at block-element
/// boundaries so that cross-paragraph selections can be matched.
fn collect_text_nodes_spaced(
    node: &web_sys::Node,
    text_nodes: &mut Vec<web_sys::Node>,
    full_text: &mut String,
    node_offsets: &mut Vec<(usize, usize)>,
) {
    if node.node_type() == web_sys::Node::TEXT_NODE {
        let start = full_text.len();
        let content = node.text_content().unwrap_or_default();
        full_text.push_str(&content);
        node_offsets.push((start, full_text.len()));
        text_nodes.push(node.clone());
        return;
    }
    // Insert a space before block elements to separate their text
    let is_block = if let Ok(el) = node.clone().dyn_into::<web_sys::Element>() {
        let tag = el.tag_name().to_lowercase();
        matches!(tag.as_str(), "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "li" | "br")
    } else {
        false
    };
    if is_block && !full_text.is_empty() && !full_text.ends_with(' ') {
        full_text.push(' ');
    }
    let children = node.child_nodes();
    for i in 0..children.length() {
        if let Some(child) = children.item(i) {
            collect_text_nodes_spaced(&child, text_nodes, full_text, node_offsets);
        }
    }
}

/// Collapse runs of whitespace into a single space and trim.
fn normalize_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut last_was_space = true; // true to trim leading
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(c);
            last_was_space = false;
        }
    }
    if result.ends_with(' ') {
        result.pop();
    }
    result
}

/// Normalize whitespace and return a mapping from each normalized byte offset
/// to the corresponding original byte offset.
fn normalize_whitespace_map(s: &str) -> (String, Vec<usize>) {
    let mut result = String::with_capacity(s.len());
    let mut mapping = Vec::with_capacity(s.len());
    let mut last_was_space = true;
    for (byte_idx, c) in s.char_indices() {
        if c.is_whitespace() {
            if !last_was_space {
                mapping.push(byte_idx);
                result.push(' ');
                last_was_space = true;
            }
        } else {
            // For multi-byte chars, push the byte_idx for each byte of the output char
            let start = result.len();
            result.push(c);
            let added = result.len() - start;
            for i in 0..added {
                mapping.push(byte_idx + i);
            }
            last_was_space = false;
        }
    }
    if result.ends_with(' ') {
        result.pop();
        mapping.pop();
    }
    // Sentinel: map one past the end
    mapping.push(s.len());
    (result, mapping)
}

/// Find the length of the longest common substring between two strings.
fn longest_common_substring(a: &str, b: &str) -> usize {
    // Simple O(n*m) approach, fine for short context blocks (<= 200 chars)
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let mut max_len = 0;
    // Use a single-row DP to save memory
    let mut prev = vec![0usize; b_bytes.len() + 1];
    let mut curr = vec![0usize; b_bytes.len() + 1];
    for i in 1..=a_bytes.len() {
        for j in 1..=b_bytes.len() {
            if a_bytes[i - 1] == b_bytes[j - 1] {
                curr[j] = prev[j - 1] + 1;
                if curr[j] > max_len {
                    max_len = curr[j];
                }
            } else {
                curr[j] = 0;
            }
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.iter_mut().for_each(|v| *v = 0);
    }
    max_len
}

/// Map a byte offset in the concatenated full text back to a (node_index, offset_within_node).
fn byte_offset_to_node(node_offsets: &[(usize, usize)], byte_offset: usize) -> (usize, usize) {
    for (i, &(start, end)) in node_offsets.iter().enumerate() {
        if byte_offset >= start && byte_offset <= end {
            return (i, byte_offset - start);
        }
    }
    // Fallback: last node, end
    let last = node_offsets.len().saturating_sub(1);
    (last, node_offsets.get(last).map(|o| o.1 - o.0).unwrap_or(0))
}

/// Render the Editor pane (CSS toggle, always in DOM — preserves undo history).
#[allow(clippy::too_many_arguments)]
pub(in crate::pages::book) fn render<GB, ST, SC, SA, AR, RF, DF, ARO, RFO, DFO>(
    __scope: &mut RenderScope,
    state: BookState,
    book_id: String,
    saved_here_only: impl Fn() -> bool + Copy + 'static,
    go_back_to_chapters: GB,
    save_chapter_title_inline: ST,
    schedule_chapter_autosave: SC,
    save_content: SA,
    author_reply: AR,
    resolve_feedback: RF,
    delete_feedback: DF,
) -> NodeHandle
where
    GB: Fn() + 'static + Copy,
    ST: Fn(String) + 'static + Copy,
    SC: Fn() + 'static + Copy,
    SA: Fn(String) + 'static + Copy,
    AR: Fn(String) -> ARO + 'static + Copy,
    RF: Fn(String) -> RFO + 'static + Copy,
    DF: Fn(String) -> DFO + 'static + Copy,
    ARO: Fn() + 'static,
    RFO: Fn() + 'static,
    DFO: Fn() + 'static,
{
    let BookState {
        active_pane,
        chapter_title,
        editor_word_count,
        save_status,
        save_alert,
        beta_feedback,
        show_feedback_sidebar,
        chapter_handle,
        pending_feedback_scroll,
        reply_drafts,
        editor_writing,
        ..
    } = state;

    // This chapter's feedback — read wherever the rail, its header count, and
    // the mobile toolbar badge each need "does this chapter have feedback".
    let current_chapter_feedback = move || {
        let cid = match active_pane.get() {
            BookPane::Editor(ref cid) => cid.clone(),
            _ => return Vec::new(),
        };
        beta_feedback.get().into_iter().filter(|f| f.chapter_id == cid).collect::<Vec<_>>()
    };

    rsx! {
        div {
            class: {move || if editor_writing.get() { "editor-layout is-writing" } else { "editor-layout" }},
            style: {move || if matches!(active_pane.get(), BookPane::Editor(_)) { "" } else { "display:none;" }},

            div { class: "editor-topbar",
                div { class: "editor-topbar-left",
                    ActionIcon {
                        variant: "subtle",
                        onclick: go_back_to_chapters,
                        {render_tabler_icon(__scope, TablerIcon::ArrowLeft, TablerIconStyle::Outline)}
                    }
                    TextInput {
                        class: "editor-title-input",
                        value_fn: move || chapter_title.get(),
                        oninput: move |v: String| save_chapter_title_inline(v),
                    }
                }
                if !current_chapter_feedback().is_empty() {
                    ActionIcon {
                        variant: {move || if show_feedback_sidebar.get() { "filled".to_string() } else { "subtle".to_string() }},
                        size: "sm",
                        onclick: move || show_feedback_sidebar.update(|v| *v = !*v),
                        {render_tabler_icon(__scope, TablerIcon::MessageCircle, TablerIconStyle::Outline)}
                    }
                }
            }

            // A save that didn't land says so here rather than only in the
            // four-word status indicator in the footer. `if` (not a reactive
            // text node) so the whole block swaps in and out. Deliberately NOT
            // faded by the typing collapse (task 4 covers ambient chrome, not
            // an active data-loss warning the author needs to see).
            if save_alert.get().is_some() {
                div {
                    style: "padding: 8px 16px 0;",
                    Alert {
                        color: "orange",
                        title: "Your writing isn't reaching the server",
                        {move || save_alert.get().unwrap_or_default()}
                        Space { h: "xs" }
                        div {
                            style: "display: flex; gap: 8px;",
                            Button {
                                variant: "light",
                                size: "xs",
                                onclick: move || {
                                    if let BookPane::Editor(ref cid) = active_pane.get() {
                                        save_content(cid.clone());
                                    }
                                },
                                "Retry save"
                            }
                            Button {
                                variant: "subtle",
                                size: "xs",
                                onclick: move || save_alert.set(None),
                                "Dismiss"
                            }
                        }
                    }
                }
            }

            // Formatting toolbar: no selection-driven popover (see the doc
            // comment on `editor_toolbar` in editor_utils.rs — rinch's
            // `EditorHandle` exposes no selection-change signal or screen-space
            // selection rect to anchor one against, and a guessed popover
            // position is worse than a toolbar). It fades with the rest of the
            // chrome instead via the shared `is-writing` class.
            {editor_utils::editor_toolbar(__scope, chapter_handle.get(), book_id.clone(), schedule_chapter_autosave)}

            div {
                style: "display: flex; flex: 1; overflow: hidden; position: relative;",
                div { class: "editor-scroll",
                    // Wrapper keeps the `#editor-main` id the feedback
                    // scroll-to-text feature queries; the model-first
                    // editor renders its text nodes inside it.
                    div {
                        class: "editor-content",
                        id: "editor-main",
                        Editor {
                            editor: chapter_handle.get(),
                            content: String::new(),
                        }
                    }
                }

                // Editor feedback backdrop (mobile)
                if !current_chapter_feedback().is_empty() {
                    div {
                        class: {move || if show_feedback_sidebar.get() { "editor-feedback-backdrop open" } else { "editor-feedback-backdrop" }},
                        onclick: move || show_feedback_sidebar.set(false),
                    }
                }

                // Feedback rail — mounted only when this chapter has feedback
                // (task 3). A chapter with none used to hold 300px open just to
                // say so; the prose column gets that width back instead. The
                // toggle (topbar icon above) and the unresolved-count badge
                // only appear in the same condition, so there's nothing to
                // "open" when the rail isn't mounted.
                if !current_chapter_feedback().is_empty() {
                    div {
                        class: {move || if show_feedback_sidebar.get() { "editor-feedback-sidebar visible" } else { "editor-feedback-sidebar hidden" }},

                        div { class: "editor-feedback-header",
                            "Feedback"
                            if current_chapter_feedback().iter().filter(|f| !f.resolved).count() > 0 {
                                span {
                                    class: "editor-feedback-count",
                                    {move || format!("{} unresolved", current_chapter_feedback().iter().filter(|f| !f.resolved).count())}
                                }
                            }
                            ActionIcon {
                                variant: "subtle",
                                size: "xs",
                                onclick: move || show_feedback_sidebar.set(false),
                                {render_tabler_icon(__scope, TablerIcon::X, TablerIconStyle::Outline)}
                            }
                        }
                        div { class: "editor-feedback-list",
                            for fb in current_chapter_feedback() {
                                {editor_feedback_item(__scope, fb, pending_feedback_scroll, reply_drafts, author_reply, resolve_feedback, delete_feedback)}
                            }
                        }
                    }
                }
            }

            // Footer: word count + save state. Moved out of the topbar (task 5)
            // into a quiet strip that fades with the rest of the chrome —
            // visible when you look, invisible when you write. The "Saved" /
            // "Saved on this device" distinction is load-bearing for cut-over
            // books (`saved_here_only`) and keeps its exact wording and dot.
            //
            // It sits in the layout column *after* the content row, not inside
            // the scroller: placed within `.editor-scroll` it followed the prose
            // and was pushed below the fold, so the word count and save state
            // were only reachable by scrolling to the end of the chapter —
            // strictly worse than the topbar it replaced.
            div { class: "editor-footer",
                div {
                    class: "editor-word-count",
                    {move || format!("{} words", editor_word_count.get())}
                }
                div {
                    class: {|| format!("save-indicator {}", save_status.get())},
                    {move || match save_status.get() {
                        "saving" => "Saving...".to_string(),
                        // "Saved" has to mean the same thing everywhere. For a
                        // cut-over book sync is how an edit reaches the server,
                        // so with sync off this save reached this device and
                        // nothing else — say so.
                        "saved" if saved_here_only() => {
                            "Saved on this device".to_string()
                        }
                        "saved" => "Saved".to_string(),
                        "error" => "Save failed — retry".to_string(),
                        _ => "Unsaved".to_string(),
                    }}
                }
            }
        }
    }
}
