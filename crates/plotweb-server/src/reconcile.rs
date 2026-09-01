//! Reconciling a document the shadow pass reported as diverged (migration phase D→E).
//!
//! The backfill deliberately refuses to touch a document a client owns — re-projecting
//! it from git would fork a second, history-disjoint copy of the same content. That
//! guard is right, and it means the one class of finding the shadow pass exists to
//! produce is also the one class nothing can fix automatically. Someone has to decide
//! which copy is correct, and this is where that decision gets carried out.
//!
//! Two directions, because both happen:
//!
//! - **`Prefer::Git`** — git holds the truth (an edit reached REST but never the CRDT,
//!   which is every edit made in a session without sync). The canonical document is
//!   re-projected from git and its ownership cleared, so it is provisional again.
//! - **`Prefer::Crdt`** — the CRDT holds the truth (an edit reached the CRDT but never
//!   REST). The stored document is materialized back to `DocNode` JSON and written to
//!   git through the same call the REST save uses, so git ends up with content
//!   indistinguishable from the editor having saved it.
//!
//! Both are per-document and both support a dry run, because "which copy wins" is a
//! judgement about someone's writing, not a mechanical merge.
//!
//! Both directions cover **structure** (`book:`) as well as bodies. A structure document
//! used to be skipped here with a note to "clear ownership and re-run the backfill" — a
//! remedy no command implemented, which left a diverged `book:` document with no
//! supported repair at all. Preferring git re-projects the structure and clears
//! ownership (the backfill maintains it again); preferring the CRDT materializes the
//! stored structure into git through the mirror's own write path, which is the deliberate
//! act that pass defers to when it refuses to empty a manuscript by itself.
//!
//! # The part that is not automatic
//!
//! Resolving in git's favour leaves any client still holding the old document with a
//! history disjoint from the new canonical one. Clearing `synced_at` is what stops the
//! two being merged: the document becomes provisional, so the next client to sync
//! claims it afresh rather than merging into it (see the §D8 handshake). A client that
//! already holds a stale copy must therefore be reset — which the sync engine does when
//! the server tells it the histories are unrelated.

use std::path::{Path, PathBuf};

use plotweb_crdt::{
    materialize_body, materialize_book_structure, project_body, project_book_structure, BodyKind,
    BookStructure,
};
use plotweb_git::BookStore;
use plotweb_common::UpdateChapterRequest;

/// Which copy is treated as correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prefer {
    /// Git wins: re-project it into the canonical store.
    Git,
    /// The stored CRDT wins: write it back into git.
    Crdt,
}

impl Prefer {
    pub fn parse(s: &str) -> Option<Prefer> {
        match s {
            "git" => Some(Prefer::Git),
            "crdt" => Some(Prefer::Crdt),
            _ => None,
        }
    }
}

/// What a reconcile did (or would do, under a dry run).
#[derive(Debug, Default)]
pub struct ReconcileSummary {
    pub considered: usize,
    /// `(doc_id, what changed)`.
    pub resolved: Vec<(String, String)>,
    /// Documents that could not be read at all and were rebuilt from git. Separate
    /// from `resolved` because no choice between two copies was made: there was only
    /// ever one readable copy.
    pub rebuilt: Vec<(String, String)>,
    pub skipped: Vec<(String, String)>,
    pub errors: Vec<(String, String)>,
}

/// Reconcile one body document.
///
/// `doc_id` is `chapter:{id}` or `note:{id}`; the book is needed because git addresses
/// content by book. Returns a description of the action for the report.
pub async fn reconcile_body(
    books: &BookStore,
    crdt_dir: &Path,
    book_id: &str,
    doc_id: &str,
    prefer: Prefer,
    dry_run: bool,
) -> Result<String, String> {
    let (kind, id) = if let Some(id) = doc_id.strip_prefix("chapter:") {
        (BodyKind::Chapter, id)
    } else if let Some(id) = doc_id.strip_prefix("note:") {
        (BodyKind::Note, id)
    } else {
        return Err(format!("{doc_id} is not a body document"));
    };

    match prefer {
        Prefer::Git => {
            // Read git's content and project it afresh.
            let content = match kind {
                BodyKind::Chapter => books
                    .get_chapter(book_id, id)
                    .await
                    .map_err(|e| format!("git read failed: {e}"))?
                    .content,
                BodyKind::Note => books
                    .get_note(book_id, id)
                    .await
                    .map_err(|e| format!("git read failed: {e}"))?
                    .content,
            };
            let bytes = project_body(&content, kind)?;
            if dry_run {
                return Ok(format!(
                    "would replace the canonical document from git ({} bytes) and clear \
                     its ownership",
                    bytes.len()
                ));
            }
            crate::sync::replace_canonical_from_git(crdt_dir, doc_id, kind_name(kind), &bytes)
                .map_err(|e| format!("canonical write failed: {e}"))?;
            Ok("canonical document replaced from git; ownership cleared so the next \
                client claims it afresh"
                .to_string())
        }
        Prefer::Crdt => {
            let canonical = crate::sync::canonical_snapshot(&PathBuf::from(crdt_dir), doc_id)
                .map_err(|e| format!("store read failed: {e}"))?
                .ok_or_else(|| "no canonical document to write back".to_string())?;
            let content = materialize_body(&canonical)?;
            if dry_run {
                return Ok(format!(
                    "would write the stored document into git ({} chars of DocNode JSON)",
                    content.len()
                ));
            }
            match kind {
                BodyKind::Chapter => books
                    .update_chapter(
                        book_id,
                        id,
                        &UpdateChapterRequest {
                            title: None,
                            content: Some(content),
                        },
                    )
                    .await
                    .map_err(|e| format!("git write failed: {e}"))?,
                BodyKind::Note => books
                    .update_note(book_id, id, None, Some(&content), None)
                    .await
                    .map_err(|e| format!("git write failed: {e}"))?,
            }
            Ok("stored document written into git".to_string())
        }
    }
}

/// Reconcile one `book:` structure document.
///
/// The structure counterpart of [`reconcile_body`], and the reason this module can now
/// answer for every document the shadow pass reports.
///
/// `Prefer::Git` re-projects `book.json` (chapter order, titles, notes tree, metadata)
/// and clears ownership, so the document is provisional again and the backfill maintains
/// it. `Prefer::Crdt` replays the stored structure into git through
/// [`crate::mirror::mirror_structure`] — the same writes the mirror makes, so git ends up
/// with a book indistinguishable from one edited through REST.
///
/// The mirror refuses to empty a manuscript it is only shadowing; here the emptying is
/// permitted, because someone chose it. That is what its "reconcile this deliberately"
/// message has always pointed at.
pub async fn reconcile_structure(
    books: &BookStore,
    crdt_dir: &Path,
    book_id: &str,
    prefer: Prefer,
    dry_run: bool,
) -> Result<String, String> {
    let doc_id = format!("book:{book_id}");

    match prefer {
        Prefer::Git => {
            let input = crate::structure::read_structure_input(books, book_id)
                .await
                .ok_or_else(|| "no readable structure in git".to_string())?;
            let bytes = project_book_structure(&input)?;
            if dry_run {
                return Ok(format!(
                    "would replace the canonical structure from git ({} chapters, {} notes) \
                     and clear its ownership",
                    input.chapters.len(),
                    input.notes.len()
                ));
            }
            crate::sync::replace_canonical_from_git(crdt_dir, &doc_id, "book", &bytes)
                .map_err(|e| format!("canonical write failed: {e}"))?;
            Ok("canonical structure replaced from git; ownership cleared so the next \
                client claims it afresh"
                .to_string())
        }
        Prefer::Crdt => {
            let canonical = crate::sync::canonical_snapshot(&PathBuf::from(crdt_dir), &doc_id)
                .map_err(|e| format!("store read failed: {e}"))?
                .ok_or_else(|| "no canonical structure to write back".to_string())?;
            let want = materialize_book_structure(&canonical)?;
            let input = crate::structure::read_structure_input(books, book_id)
                .await
                .ok_or_else(|| "no readable structure in git".to_string())?;
            let have = input.structure();
            if have == want {
                return Ok("git already matches the stored structure".to_string());
            }
            if dry_run {
                return Ok(format!(
                    "would write the stored structure into git ({})",
                    describe_structure_change(&have, &want)
                ));
            }
            if crate::mirror::mirror_structure(books, book_id, &canonical, true).await {
                Ok(format!(
                    "stored structure written into git ({})",
                    describe_structure_change(&have, &want)
                ))
            } else {
                Err("the structure write reported nothing written — see the log".to_string())
            }
        }
    }
}

/// A one-line account of what preferring the CRDT would do to git, for the report.
///
/// Counts rather than a diff: the report is read to decide whether to run the thing for
/// real, and "3 chapters removed" is the number that decision turns on.
fn describe_structure_change(have: &BookStructure, want: &BookStructure) -> String {
    let have_ids: Vec<&String> = have.chapters.iter().map(|(id, _)| id).collect();
    let want_ids: Vec<&String> = want.chapters.iter().map(|(id, _)| id).collect();

    let added = want_ids.iter().filter(|id| !have_ids.contains(id)).count();
    let removed = have_ids.iter().filter(|id| !want_ids.contains(id)).count();
    let renamed = want
        .chapters
        .iter()
        .filter(|(id, title)| {
            have.chapters
                .iter()
                .any(|(hid, htitle)| hid == id && htitle != title)
        })
        .count();
    let reordered = added == 0 && removed == 0 && have_ids != want_ids;

    let notes_added = want
        .note_titles
        .keys()
        .filter(|id| !have.note_titles.contains_key(*id))
        .count();
    let notes_removed = have
        .note_titles
        .keys()
        .filter(|id| !want.note_titles.contains_key(*id))
        .count();
    let meta = have.title != want.title
        || have.description != want.description
        || have.font_settings_json != want.font_settings_json
        || have.cover_ref != want.cover_ref;

    let mut parts = Vec::new();
    if added > 0 {
        parts.push(format!("{added} chapters added"));
    }
    if removed > 0 {
        parts.push(format!("{removed} chapters removed"));
    }
    if renamed > 0 {
        parts.push(format!("{renamed} renamed"));
    }
    if reordered {
        parts.push("reordered".to_string());
    }
    if notes_added > 0 {
        parts.push(format!("{notes_added} notes added"));
    }
    if notes_removed > 0 {
        parts.push(format!("{notes_removed} notes removed"));
    }
    if meta {
        parts.push("metadata".to_string());
    }
    if parts.is_empty() {
        "no visible difference".to_string()
    } else {
        parts.join(", ")
    }
}

fn kind_name(kind: BodyKind) -> &'static str {
    match kind {
        BodyKind::Chapter => "chapter",
        BodyKind::Note => "note",
    }
}

/// Reconcile every document the shadow pass reports as **diverged** (client-owned and
/// disagreeing with git). Staleness is left alone: a backfill run is its remedy, and
/// treating it here would rewrite documents nobody is in conflict over.
pub async fn run_all(
    data_dir: &str,
    crdt_dir: &str,
    prefer: Prefer,
    dry_run: bool,
) -> Result<ReconcileSummary, String> {
    let books = BookStore::new(PathBuf::from(data_dir));
    let findings = crate::shadow::run_shadow_pass(data_dir, crdt_dir).await?;

    let mut summary = ReconcileSummary::default();
    for (doc_id, detail) in &findings.diverged {
        summary.considered += 1;
        let Some(book_id) = owning_book(&books, data_dir, doc_id).await else {
            summary.skipped.push((
                doc_id.clone(),
                "could not determine the owning book".to_string(),
            ));
            continue;
        };
        if doc_id.starts_with("book:") {
            match reconcile_structure(&books, Path::new(crdt_dir), &book_id, prefer, dry_run).await
            {
                Ok(action) => summary
                    .resolved
                    .push((doc_id.clone(), format!("{action} [was: {detail}]"))),
                Err(e) => summary.errors.push((doc_id.clone(), e)),
            }
            continue;
        }
        match reconcile_body(&books, Path::new(crdt_dir), &book_id, doc_id, prefer, dry_run).await {
            Ok(action) => summary
                .resolved
                .push((doc_id.clone(), format!("{action} [was: {detail}]"))),
            Err(e) => summary.errors.push((doc_id.clone(), e)),
        }
    }

    // Documents that could not be read as documents at all. The shadow pass has always
    // reported these — "a blob from before a CRDT change" — and nothing has ever fixed
    // them, so a projection change left every body permanently unreadable while reads
    // fell back to git and writes were dropped in its favour.
    //
    // Direction is not a judgement here the way it is for a divergence: there is no
    // readable stored copy to prefer, so git is the only side with content and these
    // are rebuilt from it whatever `prefer` says. Recorded distinctly so the report
    // never implies someone's CRDT edits were weighed and lost.
    for (doc_id, reason) in &findings.unreadable {
        summary.considered += 1;
        let Some(book_id) = owning_book(&books, data_dir, doc_id).await else {
            summary.skipped.push((
                doc_id.clone(),
                "could not determine the owning book".to_string(),
            ));
            continue;
        };
        let outcome = if doc_id.starts_with("book:") {
            reconcile_structure(&books, Path::new(crdt_dir), &book_id, Prefer::Git, dry_run).await
        } else {
            reconcile_body(
                &books,
                Path::new(crdt_dir),
                &book_id,
                doc_id,
                Prefer::Git,
                dry_run,
            )
            .await
        };
        match outcome {
            Ok(action) => summary.rebuilt.push((
                doc_id.clone(),
                format!("{action} [unreadable: {reason}]"),
            )),
            Err(e) => summary.errors.push((doc_id.clone(), e)),
        }
    }
    Ok(summary)
}

/// Which book owns `doc_id`, by asking git.
async fn owning_book(books: &BookStore, data_dir: &str, doc_id: &str) -> Option<String> {
    let book_ids: Vec<String> = std::fs::read_dir(data_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join("manuscript").join("book.json").is_file())
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();

    for book_id in book_ids {
        if let Some(id) = doc_id.strip_prefix("chapter:") {
            if books
                .list_chapters(&book_id)
                .await
                .unwrap_or_default()
                .iter()
                .any(|c| c.id == id)
            {
                return Some(book_id);
            }
        } else if let Some(id) = doc_id.strip_prefix("note:") {
            if books
                .list_notes(&book_id)
                .await
                .unwrap_or_default()
                .0
                .iter()
                .any(|n| n.id == id)
            {
                return Some(book_id);
            }
        } else if doc_id == format!("book:{book_id}") {
            return Some(book_id);
        }
    }
    None
}

/// Entry point for `plotweb-server reconcile --prefer git|crdt [--dry-run]`.
pub async fn run(prefer: Prefer, dry_run: bool) {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "data/books".into());
    let crdt_dir =
        std::env::var("PLOTWEB_CRDT_DIR").unwrap_or_else(|_| crate::sync::DEFAULT_CRDT_DIR.into());

    println!(
        "[reconcile] resolving diverged client-owned documents in favour of {}{}",
        match prefer {
            Prefer::Git => "GIT",
            Prefer::Crdt => "the stored CRDT",
        },
        if dry_run { " (dry run)" } else { "" }
    );

    match run_all(&data_dir, &crdt_dir, prefer, dry_run).await {
        Ok(summary) => {
            println!();
            println!("────────────────────────────────────────");
            println!("PlotWeb reconcile{}", if dry_run { " (dry run)" } else { "" });
            println!("  documents considered : {}", summary.considered);
            println!("  resolved             : {}", summary.resolved.len());
            println!("  rebuilt (unreadable) : {}", summary.rebuilt.len());
            println!("  skipped              : {}", summary.skipped.len());
            println!("  errors               : {}", summary.errors.len());
            println!();
            for (doc_id, action) in &summary.resolved {
                println!("  {doc_id} — {action}");
            }
            for (doc_id, action) in &summary.rebuilt {
                println!("  REBUILT {doc_id} — {action}");
            }
            for (doc_id, why) in &summary.skipped {
                println!("  SKIPPED {doc_id} — {why}");
            }
            for (doc_id, e) in &summary.errors {
                println!("  ERROR   {doc_id} — {e}");
            }
            if summary.considered == 0 {
                println!(
                    "Nothing to do: no client-owned document disagrees with git, and none \
                     is unreadable."
                );
            }
        }
        Err(e) => eprintln!("reconcile: {e}"),
    }
}

/// Boot-time hook (env `PLOTWEB_RECONCILE_ON_BOOT`): `dry-run`, `git`, or `crdt`.
///
/// Exists because a subcommand is unreachable where it is needed. The platform gives
/// logs, secrets, restart and deploy — no shell — so a tool that only runs as
/// `plotweb-server reconcile` can resolve nothing on the deployment that actually has
/// divergences. Anything unrecognised is treated as a dry run: a typo in an environment
/// variable must not rewrite prose.
pub async fn run_on_boot(setting: &str, data_dir: String, crdt_dir: String) {
    let (prefer, dry_run) = match setting {
        "git" => (Prefer::Git, false),
        "crdt" => (Prefer::Crdt, false),
        other => {
            if other != "dry-run" && other != "1" && other != "true" {
                println!(
                    "[boot-reconcile] unrecognised setting {other:?} — running as a dry run; \
                     use `git` or `crdt` to actually resolve"
                );
            }
            (Prefer::Git, true)
        }
    };

    println!(
        "[boot-reconcile] resolving diverged client-owned documents in favour of {}{}",
        match prefer {
            Prefer::Git => "GIT",
            Prefer::Crdt => "the stored CRDT",
        },
        if dry_run { " (dry run)" } else { "" }
    );
    match run_all(&data_dir, &crdt_dir, prefer, dry_run).await {
        Ok(summary) => {
            println!("[boot-reconcile] considered {}", summary.considered);
            for (doc_id, action) in &summary.resolved {
                println!("[boot-reconcile]   {doc_id} — {action}");
            }
            for (doc_id, action) in &summary.rebuilt {
                println!("[boot-reconcile]   REBUILT {doc_id} — {action}");
            }
            for (doc_id, why) in &summary.skipped {
                println!("[boot-reconcile]   SKIPPED {doc_id} — {why}");
            }
            for (doc_id, e) in &summary.errors {
                println!("[boot-reconcile]   ERROR   {doc_id} — {e}");
            }
        }
        Err(e) => eprintln!("[boot-reconcile] {e}"),
    }
    println!("[boot-reconcile] complete");
}
