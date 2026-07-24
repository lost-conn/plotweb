//! Read-only migration dry-run audit (Phase 2 · migration phase B).
//!
//! Walks every book's git storage (all users), round-trips every document through
//! the canonical [`plotweb_crdt`] projection, and reports which migrate losslessly
//! (`clean`) vs. which must stay on git (`flagged`, with a reason). It is a
//! **read-only** operation: it opens the same stores `main.rs` uses but issues only
//! reads — no Automerge blobs are written, no git repo is mutated, no DB row is
//! touched. Safe to run against production.
//!
//! Enumeration: books come from the rhypedb `Book` index (a bare `Book` scan returns
//! every row, across all users); each row carries the PlotWeb `uuid` and its owner
//! `user_id`. Content is read through [`plotweb_git::BookStore`]'s read APIs
//! (`get_book` / `list_chapters` / `list_notes`) over `DATA_DIR`. The per-user
//! `user:` index is round-tripped once per owner, built from the books we walked.

use std::collections::BTreeMap;
use std::path::PathBuf;

use plotweb_crdt::{
    BookStructureInput, RoundTrip, UserIndexInput, roundtrip_body, roundtrip_book_structure,
    roundtrip_user_index,
};
use plotweb_git::BookStore;

use crate::rhype::RhypeStore;

/// One document's round-trip outcome (structure / chapter / note / user index).
struct DocResult {
    /// Owning book (empty for a `user:` index).
    book_id: String,
    /// `"book:{id}"` / `"chapter:{id}"` / `"note:{id}"` / `"user:{id}"`.
    doc_id: String,
    doc_type: &'static str,
    result: RoundTrip,
}

/// Per-book walk: its docs (structure + every chapter + every note) and any git error.
struct BookReport {
    book_id: String,
    title: String,
    user_id: String,
    read_error: Option<String>,
    chapters_scanned: usize,
    notes_scanned: usize,
    docs: Vec<DocResult>,
}

/// A `user:` index round-trip.
struct UserReport {
    user_id: String,
    book_count: usize,
    result: RoundTrip,
}

/// A cached dashboard entry gathered while walking books (for the `user:` index).
type UserEntry = (String, String, Option<String>, String); // (book_id, title, cover, updated_at)

/// Entry point for `plotweb-server audit-migration [--json <path>]`.
///
/// Opens `DATA_DIR` (git) + `RHYPEDB_DATA_DIR` (metadata index) read-only, walks
/// every book, prints a human report to stdout (plus optional JSON), and returns.
/// Flags are data, not errors — the caller exits 0 regardless of what flags.
pub async fn run(json_path: Option<String>) {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "data/books".into());
    let rhype_dir = std::env::var("RHYPEDB_DATA_DIR").unwrap_or_else(|_| "data/rhypedb".into());

    let rhype = match RhypeStore::open(&rhype_dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("audit-migration: failed to open rhypedb at {rhype_dir}: {e}");
            return;
        }
    };
    let books = BookStore::new(PathBuf::from(&data_dir));

    // A bare `Book` scan returns every book row (all users) — rhypedb `Source::All`.
    let mut rows = rhype.find("Book").await.unwrap_or_default();
    rows.sort_by_key(|a| a.string("uuid"));

    let mut book_reports: Vec<BookReport> = Vec::new();
    let mut user_entries: BTreeMap<String, Vec<UserEntry>> = BTreeMap::new();

    for row in &rows {
        let book_id = row.string("uuid").unwrap_or_default();
        let user_id = row.string("user_id").unwrap_or_default();
        if book_id.is_empty() {
            continue;
        }

        // ── Read everything for this book (async → owned, Send data). ──
        let (book_data, read_error) = match books.get_book(&book_id).await {
            Ok(d) => (Some(d), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let chapters = books.list_chapters(&book_id).await.unwrap_or_default();
        let (notes_list, notes_tree) = books.list_notes(&book_id).await.unwrap_or_default();

        // Feed the user: index (mirror books::list's fallback on a missing repo).
        let (u_title, u_cover, u_updated) = match &book_data {
            Some(d) => (d.title.clone(), d.cover_image.clone(), d.updated_at.clone()),
            None => (
                row.string("title").unwrap_or_default(),
                None,
                row.string("created_at").unwrap_or_default(),
            ),
        };
        user_entries.entry(user_id.clone()).or_default().push((
            book_id.clone(),
            u_title,
            u_cover,
            u_updated,
        ));

        // ── Round-trips (sync, !Send). Created + dropped between awaits. ──
        let mut docs: Vec<DocResult> = Vec::new();

        // book: structure
        let structure = match &book_data {
            Some(d) => {
                let input = BookStructureInput {
                    title: d.title.clone(),
                    description: d.description.clone(),
                    font_settings: d.font_settings.clone(),
                    cover_ref: d.cover_image.clone(),
                    created_at: d.created_at.clone(),
                    chapters: chapters
                        .iter()
                        .map(|c| (c.id.clone(), c.title.clone()))
                        .collect(),
                    root_order: notes_tree.root_order.clone(),
                    children: notes_tree.children.clone(),
                    collapsed: notes_tree.collapsed.clone(),
                    notes: notes_list
                        .iter()
                        .map(|n| (n.id.clone(), n.title.clone(), n.color.clone()))
                        .collect(),
                };
                roundtrip_book_structure(&input)
            }
            None => RoundTrip::flag(format!(
                "git read failed: {}",
                read_error.clone().unwrap_or_default()
            )),
        };
        docs.push(DocResult {
            book_id: book_id.clone(),
            doc_id: format!("book:{book_id}"),
            doc_type: "book",
            result: structure,
        });

        // chapter: bodies
        for c in &chapters {
            docs.push(DocResult {
                book_id: book_id.clone(),
                doc_id: format!("chapter:{}", c.id),
                doc_type: "chapter",
                result: roundtrip_body(&c.content),
            });
        }

        // note: bodies
        for n in &notes_list {
            docs.push(DocResult {
                book_id: book_id.clone(),
                doc_id: format!("note:{}", n.id),
                doc_type: "note",
                result: roundtrip_body(&n.content),
            });
        }

        book_reports.push(BookReport {
            title: book_data
                .as_ref()
                .map(|d| d.title.clone())
                .unwrap_or_else(|| row.string("title").unwrap_or_default()),
            book_id,
            user_id,
            read_error,
            chapters_scanned: chapters.len(),
            notes_scanned: notes_list.len(),
            docs,
        });
    }

    // ── User indices: one round-trip per owner. ──
    let mut user_reports: Vec<UserReport> = Vec::new();
    for (user_id, entries) in &user_entries {
        let result = roundtrip_user_index(&UserIndexInput {
            books: entries.clone(),
        });
        user_reports.push(UserReport {
            user_id: user_id.clone(),
            book_count: entries.len(),
            result,
        });
    }

    print_report(&data_dir, &rhype_dir, &book_reports, &user_reports);

    if let Some(path) = json_path {
        let json = build_json(&book_reports, &user_reports);
        match std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap()) {
            Ok(()) => println!("\nMachine-readable report written to {path}"),
            Err(e) => eprintln!("audit-migration: failed to write JSON to {path}: {e}"),
        }
    }
}

fn print_report(data_dir: &str, rhype_dir: &str, books: &[BookReport], users: &[UserReport]) {
    println!("PlotWeb migration dry-run audit");
    println!("  Modifies no book content and no metadata. Reads git storage and");
    println!("  rhypedb only; the sole write is rhypedb's own open LOCK file, so the");
    println!("  server (or any other tool) must be stopped while this runs.");
    println!("  DATA_DIR         : {data_dir}");
    println!("  RHYPEDB_DATA_DIR : {rhype_dir}");
    println!();

    let mut total_docs = 0usize;
    let mut total_clean = 0usize;
    let mut total_flagged = 0usize;
    let mut flagged_lines: Vec<String> = Vec::new();

    for b in books {
        println!("Book {} \"{}\"  (owner {})", b.book_id, b.title, b.user_id);
        if let Some(err) = &b.read_error {
            println!("  ! git read failed: {err}");
        }

        let structure = &b.docs[0].result; // structure is always pushed first
        let chapters_clean = b
            .docs
            .iter()
            .filter(|d| d.doc_type == "chapter" && d.result.is_clean())
            .count();
        let notes_clean = b
            .docs
            .iter()
            .filter(|d| d.doc_type == "note" && d.result.is_clean())
            .count();

        println!("  structure : {}", status_str(structure));
        println!(
            "  chapters  : {} scanned, {} clean, {} flagged",
            b.chapters_scanned,
            chapters_clean,
            b.chapters_scanned - chapters_clean
        );
        println!(
            "  notes     : {} scanned, {} clean, {} flagged",
            b.notes_scanned,
            notes_clean,
            b.notes_scanned - notes_clean
        );

        for d in &b.docs {
            total_docs += 1;
            if d.result.is_clean() {
                total_clean += 1;
            } else {
                total_flagged += 1;
                let reason = d.result.reason().unwrap_or("");
                println!("  [flagged] {} ({}) — {}", d.doc_id, d.doc_type, reason);
                flagged_lines.push(format!(
                    "  {} ({}) [book {}] — {}",
                    d.doc_id, d.doc_type, d.book_id, reason
                ));
            }
        }
        println!();
    }

    println!("User indices");
    for u in users {
        println!(
            "  user:{}  : {}  ({} book(s))",
            u.user_id,
            status_str(&u.result),
            u.book_count
        );
        total_docs += 1;
        if u.result.is_clean() {
            total_clean += 1;
        } else {
            total_flagged += 1;
            flagged_lines.push(format!(
                "  user:{} (user) — {}",
                u.user_id,
                u.result.reason().unwrap_or("")
            ));
        }
    }
    println!();

    println!("────────────────────────────────────────");
    println!("Grand totals");
    println!("  books scanned : {}", books.len());
    println!("  user indices  : {}", users.len());
    println!("  docs scanned  : {total_docs}");
    println!("  clean         : {total_clean}");
    println!("  flagged       : {total_flagged}");
    println!();
    if flagged_lines.is_empty() {
        println!("Flagged docs  : (none — every document round-trips losslessly)");
    } else {
        println!("Flagged docs:");
        for line in &flagged_lines {
            println!("{line}");
        }
    }
}

fn status_str(rt: &RoundTrip) -> String {
    match rt {
        RoundTrip::Clean => "clean".to_string(),
        RoundTrip::Flagged { reason } => format!("FLAGGED — {reason}"),
    }
}

/// Machine-readable report: every document with its status + reason, plus totals.
/// Phase C consumes this to know exactly which docs are clean (to backfill) vs
/// flagged (to skip).
fn build_json(books: &[BookReport], users: &[UserReport]) -> serde_json::Value {
    use serde_json::json;

    let mut documents: Vec<serde_json::Value> = Vec::new();
    let mut clean = 0usize;
    let mut flagged = 0usize;

    for b in books {
        for d in &b.docs {
            if d.result.is_clean() {
                clean += 1;
            } else {
                flagged += 1;
            }
            documents.push(json!({
                "book_id": d.book_id,
                "doc_id": d.doc_id,
                "type": d.doc_type,
                "status": if d.result.is_clean() { "clean" } else { "flagged" },
                "reason": d.result.reason(),
            }));
        }
    }
    for u in users {
        if u.result.is_clean() {
            clean += 1;
        } else {
            flagged += 1;
        }
        documents.push(json!({
            "book_id": "",
            "doc_id": format!("user:{}", u.user_id),
            "type": "user",
            "status": if u.result.is_clean() { "clean" } else { "flagged" },
            "reason": u.result.reason(),
        }));
    }

    json!({
        "read_only": true,
        "books_scanned": books.len(),
        "user_indices": users.len(),
        "docs_scanned": clean + flagged,
        "clean": clean,
        "flagged": flagged,
        "documents": documents,
    })
}
