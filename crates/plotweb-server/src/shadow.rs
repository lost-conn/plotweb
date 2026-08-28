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

use plotweb_crdt::{compare_body, compare_book_structure, BodyKind, Shadow};
use plotweb_git::BookStore;
use rinch_storage::FsStore;

/// What the pass found, across every document it looked at.
///
/// Divergence is split by **who owns the document**, because the two cases mean
/// entirely different things and only one of them should ever hold a cutover:
///
/// - A **client-owned** document (its manifest carries `synced_at`) that disagrees
///   with git means a client and the server have genuinely fallen out of step. That is
///   the finding phase D exists to produce.
/// - A document no client has synced is just the backfill's snapshot, frozen when it
///   was taken. Any editing since moves git and leaves the snapshot behind. That is
///   staleness, not divergence: git is complete, and a backfill re-run resolves it.
///
/// Without the split, ordinary authoring drives the report red within a day and the
/// real signal drowns in it.
#[derive(Debug, Default)]
pub struct ShadowSummary {
    pub books: usize,
    pub compared: usize,
    pub matched: usize,
    /// Client-owned documents that disagree with git — the finding that matters.
    pub diverged: Vec<(String, String)>,
    /// Never-synced snapshots that git has moved past. Informational.
    pub stale: Vec<(String, String)>,
    /// Documents the server holds no canonical copy of. Expected for anything no
    /// client has synced and no backfill has emitted; not a divergence.
    pub absent: usize,
    /// Stored copies that could not be read as documents at all (a blob from before a
    /// CRDT change, corruption). No signal rather than bad signal.
    pub unreadable: Vec<(String, String)>,
}

impl ShadowSummary {
    /// Whether the soak is clean — the precondition for phase E.
    ///
    /// Staleness deliberately does not count: it says the backfill has not run lately,
    /// not that anything is wrong.
    pub fn is_clean(&self) -> bool {
        self.diverged.is_empty() && self.unreadable.is_empty()
    }
}

/// Whether a client has taken ownership of this document (manifest `synced_at`).
fn client_owned(store: &FsStore, doc_id: &str) -> bool {
    matches!(
        crate::sync::read_manifest(store, doc_id),
        Ok(Some(m)) if m.synced_at.is_some()
    )
}

fn record(summary: &mut ShadowSummary, store: &FsStore, doc_id: &str, outcome: Shadow) {
    summary.compared += 1;
    match outcome {
        Shadow::Match => summary.matched += 1,
        Shadow::Diverged { detail } => {
            let bucket = if client_owned(store, doc_id) {
                &mut summary.diverged
            } else {
                &mut summary.stale
            };
            bucket.push((doc_id.to_string(), detail));
        }
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
            let input = crate::structure::structure_input(d, &chapters, &notes, &notes_tree);
            match crate::sync::canonical_snapshot(&PathBuf::from(crdt_dir), &book_doc_id) {
                Ok(Some(bytes)) => record(
                    &mut summary,
                    &store,
                    &book_doc_id,
                    compare_book_structure(&input, &bytes),
                ),
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
                Ok(Some(bytes)) => record(
                    &mut summary,
                    &store,
                    &doc_id,
                    compare_body(&content, &bytes, kind),
                ),
                Ok(None) => summary.absent += 1,
                Err(e) => summary
                    .unreadable
                    .push((doc_id.clone(), format!("store read failed: {e}"))),
            }
        }
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
    println!("  DIVERGED (synced)  : {}", summary.diverged.len());
    println!(
        "  stale (never synced): {} (backfill snapshot older than git — re-run the backfill)",
        summary.stale.len()
    );
    println!("  unreadable         : {}", summary.unreadable.len());
    println!();

    if summary.diverged.is_empty() {
        println!("Diverged client-owned documents: (none)");
    } else {
        println!("DIVERGED — a client and git disagree on a document the client owns:");
        for (doc_id, detail) in &summary.diverged {
            println!("  {doc_id} — {detail}");
        }
    }
    if !summary.stale.is_empty() {
        println!();
        println!("Stale snapshots (git has moved on; not a divergence):");
        for (doc_id, detail) in &summary.stale {
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
        println!(
            "Verdict: clean. Every client-owned document agrees with git{}.",
            if summary.stale.is_empty() {
                String::new()
            } else {
                format!(
                    " ({} stale snapshot(s) noted above — refresh with a backfill run)",
                    summary.stale.len()
                )
            }
        );
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
pub async fn run_on_boot(data_dir: String, crdt_dir: String, cutover: &crate::cutover::Cutover) {
    println!("[boot-shadow] comparing the canonical store against git");
    match run_shadow_pass(&data_dir, &crdt_dir).await {
        Ok(summary) => {
            print_summary(&data_dir, &crdt_dir, &summary);
            report_against_cutover(&summary, cutover);
        }
        Err(e) => eprintln!("[boot-shadow] {e}"),
    }
    println!("[boot-shadow] shadow pass complete");
}

/// Say what an unclean store means for the books that are *already* cut over.
///
/// The verdict on its own reads as advice about a future decision — "resolve the above
/// before considering phase E" — and stays exactly as calm when the flag has been on
/// for days and the documents it is talking about are the ones authors are writing
/// into. This says which state the running server is actually in, and names the remedy.
fn report_against_cutover(summary: &ShadowSummary, cutover: &crate::cutover::Cutover) {
    if cutover.is_empty() || summary.is_clean() {
        return;
    }
    println!();
    println!(
        "[boot-shadow] CUTOVER IS ON while {} document(s) are unreadable and {} diverge.",
        summary.unreadable.len(),
        summary.diverged.len()
    );
    println!(
        "[boot-shadow] Those documents are being read from git, and writes to them are \
         redirected there rather than being dropped — degraded, not lost."
    );
    println!(
        "[boot-shadow] Rebuild them with `plotweb-server reconcile --prefer git` (or set \
         PLOTWEB_RECONCILE_ON_BOOT=git and restart)."
    );
}
