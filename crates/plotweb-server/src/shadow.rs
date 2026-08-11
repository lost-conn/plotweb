//! Shadow validation (Phase 2 · migration phase D).
//!
//! Git is still authoritative and still serves every read. This pass answers the one
//! question that decides whether cutover is safe: **does what the canonical store is
//! actually holding still say the same thing as git?**
//!
//! That is a different question from the audit's. [`crate::audit`] asks whether git
//! content *projects* losslessly — a property of the projection, answered by
//! projecting fresh each time, and it was already answered (92/92 clean). This pass
//! reads the **stored** document, the one clients now write to, and compares it to
//! git. A body that projects perfectly can still diverge here: a device that edits
//! offline moves the CRDT, and if its REST dual-write never lands, git keeps the older
//! text. Cutover means promoting the stored copy to authoritative, so a soak that
//! finds no divergence is exactly the evidence needed — and a soak that finds some has
//! found a bug worth having before the flip, not after.
//!
//! Read-only and lock-free, like the content audit: it opens the same stores the live
//! server uses but issues only reads, so it is safe to run against production
//! alongside serving traffic.

use std::path::PathBuf;

use plotweb_crdt::{compare_body, compare_book_structure, BodyKind, BookStructureInput, Shadow};
use plotweb_git::BookStore;
use rinch_storage::FsStore;

/// What the pass found, across every document it looked at.
#[derive(Debug, Default)]
pub struct ShadowSummary {
    pub books: usize,
    pub compared: usize,
    pub matched: usize,
    /// Documents where the stored copy and git disagree — the finding that matters.
    pub diverged: Vec<(String, String)>,
    /// Documents the server holds no canonical copy of. Expected for anything no
    /// client has synced and no backfill has emitted; not a divergence.
    pub absent: usize,
    /// Stored copies that could not be read as documents at all (a blob from before a
    /// CRDT change, corruption). No signal rather than bad signal.
    pub unreadable: Vec<(String, String)>,
}

impl ShadowSummary {
    /// Whether the soak is clean — the precondition for phase E.
    pub fn is_clean(&self) -> bool {
        self.diverged.is_empty() && self.unreadable.is_empty()
    }
}

fn record(summary: &mut ShadowSummary, doc_id: &str, outcome: Shadow) {
    summary.compared += 1;
    match outcome {
        Shadow::Match => summary.matched += 1,
        Shadow::Diverged { detail } => summary.diverged.push((doc_id.to_string(), detail)),
        Shadow::Unreadable { reason } => summary.unreadable.push((doc_id.to_string(), reason)),
    }
}

/// Compare every document in `data_dir` against the canonical copy in `crdt_dir`.
///
/// Enumerates books straight from the filesystem, exactly like
/// [`crate::audit::run_content_audit`] and [`crate::backfill::run_content_backfill`],
/// so it needs no rhypedb lock and can run beside the live server. `user:` indices are
/// out of scope here: they cache dashboard fields rather than authored content, so
/// they carry nothing git would disagree with.
pub async fn run_shadow_pass(data_dir: &str, crdt_dir: &str) -> Result<ShadowSummary, String> {
    let store = FsStore::open(PathBuf::from(crdt_dir))
        .map_err(|e| format!("failed to open CRDT store at {crdt_dir}: {e}"))?;
    let books = BookStore::new(PathBuf::from(data_dir));

    let mut book_ids: Vec<String> = match std::fs::read_dir(data_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.join("manuscript").join("book.json").is_file())
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect(),
        Err(e) => return Err(format!("could not read {data_dir}: {e}")),
    };
    book_ids.sort();

    let mut summary = ShadowSummary::default();

    for book_id in &book_ids {
        summary.books += 1;
        let book_data = books.get_book(book_id).await;
        let chapters = books.list_chapters(book_id).await.unwrap_or_default();
        let (notes, notes_tree) = books.list_notes(book_id).await.unwrap_or_default();

        // book: structure
        let book_doc_id = format!("book:{book_id}");
        if let Ok(d) = &book_data {
            let input = BookStructureInput {
                title: d.title.clone(),
                description: d.description.clone(),
                font_settings: d.font_settings.clone(),
                cover_ref: d.cover_image.clone(),
                created_at: d.created_at.clone(),
                chapters: chapters.iter().map(|c| (c.id.clone(), c.title.clone())).collect(),
                root_order: notes_tree.root_order.clone(),
                children: notes_tree.children.clone(),
                collapsed: notes_tree.collapsed.clone(),
                notes: notes
                    .iter()
                    .map(|n| (n.id.clone(), n.title.clone(), n.color.clone()))
                    .collect(),
            };
            match crate::sync::canonical_snapshot(&PathBuf::from(crdt_dir), &book_doc_id) {
                Ok(Some(bytes)) => {
                    record(&mut summary, &book_doc_id, compare_book_structure(&input, &bytes))
                }
                Ok(None) => summary.absent += 1,
                Err(e) => summary
                    .unreadable
                    .push((book_doc_id.clone(), format!("store read failed: {e}"))),
            }
        }

        for (doc_id, content, kind) in chapters
            .iter()
            .map(|c| (format!("chapter:{}", c.id), c.content.clone(), BodyKind::Chapter))
            .chain(
                notes
                    .iter()
                    .map(|n| (format!("note:{}", n.id), n.content.clone(), BodyKind::Note)),
            )
        {
            match crate::sync::canonical_snapshot(&PathBuf::from(crdt_dir), &doc_id) {
                Ok(Some(bytes)) => record(&mut summary, &doc_id, compare_body(&content, &bytes, kind)),
                Ok(None) => summary.absent += 1,
                Err(e) => summary
                    .unreadable
                    .push((doc_id.clone(), format!("store read failed: {e}"))),
            }
        }

        let _ = &store;
    }

    Ok(summary)
}

/// Human report. Divergences are listed in full: this pass exists to surface them, and
/// a count alone would not tell you whether to hold the cutover.
pub fn print_summary(data_dir: &str, crdt_dir: &str, summary: &ShadowSummary) {
    println!();
    println!("────────────────────────────────────────");
    println!("PlotWeb shadow validation (Phase D)");
    println!("  Read-only: git serves every read and is unchanged by this pass.");
    println!("  DATA_DIR        : {data_dir}");
    println!("  PLOTWEB_CRDT_DIR: {crdt_dir}");
    println!();
    println!("  books scanned      : {}", summary.books);
    println!("  documents compared : {}", summary.compared);
    println!("  matching git       : {}", summary.matched);
    println!("  no canonical copy  : {} (never synced or backfilled)", summary.absent);
    println!("  diverged           : {}", summary.diverged.len());
    println!("  unreadable         : {}", summary.unreadable.len());
    println!();

    if summary.diverged.is_empty() {
        println!("Diverged documents: (none)");
    } else {
        println!("Diverged documents — the stored copy disagrees with git:");
        for (doc_id, detail) in &summary.diverged {
            println!("  {doc_id} — {detail}");
        }
    }
    if !summary.unreadable.is_empty() {
        println!();
        println!("Unreadable stored copies (no signal, not a divergence):");
        for (doc_id, reason) in &summary.unreadable {
            println!("  {doc_id} — {reason}");
        }
    }
    println!();
    if summary.is_clean() {
        println!("Verdict: clean. Every canonical copy the server holds agrees with git.");
    } else {
        println!("Verdict: NOT clean — resolve the above before considering phase E (cutover).");
    }
}

/// Entry point for `plotweb-server shadow-report`.
pub async fn run() {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "data/books".into());
    let crdt_dir = std::env::var("PLOTWEB_CRDT_DIR")
        .unwrap_or_else(|_| crate::sync::DEFAULT_CRDT_DIR.into());

    println!("[shadow] comparing the canonical store against git — read-only, lock-free");
    match run_shadow_pass(&data_dir, &crdt_dir).await {
        Ok(summary) => print_summary(&data_dir, &crdt_dir, &summary),
        Err(e) => eprintln!("shadow-report: {e}"),
    }
}

/// Boot-time hook (env `PLOTWEB_SHADOW_ON_BOOT`): run the comparison beside the live
/// server and log the report, so a soak needs no downtime and no volume copy.
pub async fn run_on_boot(data_dir: String, crdt_dir: String) {
    println!("[boot-shadow] comparing the canonical store against git");
    match run_shadow_pass(&data_dir, &crdt_dir).await {
        Ok(summary) => print_summary(&data_dir, &crdt_dir, &summary),
        Err(e) => eprintln!("[boot-shadow] {e}"),
    }
    println!("[boot-shadow] shadow pass complete");
}
