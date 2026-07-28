//! Canonical `git-DocNode → Automerge` projection + per-doc round-trip validators.
//!
//! This crate is the **single source of truth** for how PlotWeb documents project
//! onto the locked Automerge schema (`docs/offline-first-rinch-plan.md`, "Locked
//! Automerge schema (v1)"). Both the server (the migration audit/backfill, now) and
//! the client (a later refactor) are meant to drive the *same* projection code, so a
//! server-migrated document is byte-compatible with a client-written one.
//!
//! Four document types, four projections:
//!
//! - **Body** (`chapter:` / `note:`) — [`roundtrip_body`]. The rich-text body is a
//!   [`rinch_editor_collab`] CRDT (blocks · per-block `Text` · marks · lists). We do
//!   **not** hand-roll this projection; the editor owns it. The validator drives the
//!   editor-collab seam and compares the materialized `DocNode` back to the input.
//! - **Book structure** (`book:`) — [`roundtrip_book_structure`]. A hand-projected
//!   `automerge::AutoCommit` mirroring [`plotweb-web`'s `local_book`] shape (schema
//!   §2): `meta`, `chapters` order + titles, `notes` tree + titles/colors.
//! - **User index** (`user:`) — [`roundtrip_user_index`]. A hand-projected doc
//!   mirroring `local_user` (schema §1): `books` map of cached dashboard entries.
//!
//! # Nothing here writes
//! Every function is pure: it builds an in-memory Automerge doc, `save()`/`load()`s
//! it, materializes it back, and compares. No blob store, no git, no disk, no DB.
//!
//! # The equality definition (load-bearing)
//! A too-loose equality yields false "Clean"; a too-strict one yields false
//! "Flagged". Equality here is **semantic**, never byte-for-byte:
//!
//! - **Bodies:** the original DocNode JSON is first *canonicalized* by running it
//!   through the editor model (`node_from_doc → to_doc`). The round-tripped side goes
//!   through the same model (`projected_doc → to_doc`). Both endpoints are a
//!   [`DocNode`](rinch_editor_core::serialize::DocNode), whose `attrs` is a
//!   `BTreeMap` (attr order normalized) and whose `marks` come out of the schema in a
//!   deterministic order. Both sides are then **coalesced** — adjacent same-mark
//!   inline text runs merged, empty runs dropped — so a difference in inline
//!   *segmentation* is not read as a difference in *content*. (This is essential for
//!   real data: the markdown importer emits a paragraph as many sentence-segmented
//!   text nodes, whereas the CRDT stores one `Text` per block and materializes it
//!   back as one span per mark boundary — same characters, same per-character marks,
//!   fewer nodes.) Comparing the coalesced `DocNode`s catches any real difference in
//!   structure, text, or per-character marks while ignoring segmentation and key
//!   order. See [`body`].
//! - **Structure:** compared as a normalized value ([`book`]/[`user`] `*Norm`):
//!   `List`s (order-bearing: chapter order, note root/child order) keep their order;
//!   `Map`s (titles, colors, collapse, cached book entries) compare as maps
//!   (`BTreeMap`/`BTreeSet`), so Automerge key reordering is a non-difference. The
//!   `user:` index additionally applies the projection's newest-first sort before
//!   comparison, exactly as the dashboard renders it.

pub mod body;
pub mod book;
pub mod user;

pub use body::{project_body, roundtrip_body, BodyKind};
pub use book::{project_book_structure, roundtrip_book_structure, BookStructureInput};
pub use user::{project_user_index, roundtrip_user_index, UserIndexInput};

/// The outcome of round-tripping one document through its canonical projection.
///
/// [`Flagged`](RoundTrip::Flagged) is *data, not an error*: it means "this document
/// does not migrate losslessly and must be left on git," and it carries a
/// human-readable reason (an unsupported block type, a parse failure, or a
/// high-level description of what differs). The projection **never panics** and
/// **never silently drops** content — an unfaithful projection is always a `Flagged`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundTrip {
    /// The document round-trips losslessly (semantic equality holds).
    Clean,
    /// The document does not round-trip losslessly; `reason` says why.
    Flagged { reason: String },
}

impl RoundTrip {
    /// Construct a [`RoundTrip::Flagged`] from any message.
    pub fn flag(reason: impl Into<String>) -> Self {
        RoundTrip::Flagged {
            reason: reason.into(),
        }
    }

    /// True if this is [`RoundTrip::Clean`].
    pub fn is_clean(&self) -> bool {
        matches!(self, RoundTrip::Clean)
    }

    /// The flag reason, if flagged.
    pub fn reason(&self) -> Option<&str> {
        match self {
            RoundTrip::Flagged { reason } => Some(reason),
            RoundTrip::Clean => None,
        }
    }
}
