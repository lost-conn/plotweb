use std::path::PathBuf;

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

use plotweb_server::email::EmailService;
use plotweb_server::{api_router, build_state, session_layer};

#[tokio::main]
async fn main() {
    // Subcommand dispatch. With no args (or an unknown first arg) we run the server,
    // exactly as before. `audit-migration` runs the read-only migration dry-run and
    // exits without ever starting the server or writing anything.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("audit-migration") {
        // Optional `--json <path>` for a machine-readable report.
        let json_path = args
            .iter()
            .position(|a| a == "--json")
            .and_then(|i| args.get(i + 1))
            .cloned();
        plotweb_server::audit::run(json_path).await;
        return;
    }
    // `backfill-migration` runs the lock-free canonical Automerge backfill (Phase C):
    // it WRITES a snapshot blob per clean document into `PLOTWEB_CRDT_DIR` and exits
    // without starting the server. Additive + reversible (only the CRDT store is
    // written); git and rhypedb are read-only.
    if args.get(1).map(String::as_str) == Some("backfill-migration") {
        plotweb_server::backfill::run().await;
        return;
    }
    // `shadow-report` (migration phase D) compares the canonical store against git and
    // exits. Read-only and lock-free, like the audit — safe against production.
    if args.get(1).map(String::as_str) == Some("shadow-report") {
        plotweb_server::shadow::run().await;
        return;
    }
    // `reconcile --prefer git|crdt [--dry-run]` resolves documents the shadow pass
    // reports as diverged. Writes — to the canonical store, to git, or (dry run)
    // neither — so it takes an explicit direction rather than guessing.
    if args.get(1).map(String::as_str) == Some("reconcile") {
        let prefer = args
            .iter()
            .position(|a| a == "--prefer")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| plotweb_server::reconcile::Prefer::parse(s));
        let Some(prefer) = prefer else {
            eprintln!("reconcile: --prefer git|crdt is required (which copy is correct?)");
            return;
        };
        let dry_run = args.iter().any(|a| a == "--dry-run");
        plotweb_server::reconcile::run(prefer, dry_run).await;
        return;
    }

    // `quarantine list` / `quarantine show <doc_id> <epoch>` reach the copies a rebuild
    // set aside. Read-only. Without this the bytes are kept but unreachable, which is
    // only marginally better than not keeping them.
    if args.get(1).map(String::as_str) == Some("quarantine") {
        plotweb_server::quarantine::run(args.get(2).map(String::as_str), &args[3..]).await;
        return;
    }

    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:plotweb.db".into());
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "data/books".into());
    let rhype_dir = std::env::var("RHYPEDB_DATA_DIR").unwrap_or_else(|_| "data/rhypedb".into());
    let crdt_dir = std::env::var("PLOTWEB_CRDT_DIR")
        .unwrap_or_else(|_| plotweb_server::sync::DEFAULT_CRDT_DIR.into());

    let state = build_state(
        &db_url,
        PathBuf::from(&data_dir),
        &rhype_dir,
        PathBuf::from(&crdt_dir),
        EmailService::from_env(),
    )
    .await;

    // Handles for the boot-time migration hooks below. Cloned before `api_router`
    // consumes the state — the `user:` backfill pass reuses the server's own rhypedb
    // handle (rhypedb is single-writer, so an in-process share is the only way to run
    // it without stopping the server).
    let boot_rhype = state.rhype.clone();
    let boot_books = state.books.clone();
    let state_cutover = state.cutover.clone();
    let mirror_state = state.clone();

    // Static files — serve the built frontend, with SPA fallback to index.html
    let dist_path = std::env::var("DIST_DIR").unwrap_or_else(|_| "../plotweb-web/dist".into());
    let index_path = format!("{}/index.html", dist_path);
    let serve_dir = ServeDir::new(&dist_path).not_found_service(ServeFile::new(&index_path));

    let session = session_layer(state.db.clone()).await;
    let app = Router::new()
        .merge(api_router(state))
        .fallback_service(serve_dir)
        .layer(session);

    // Say which books read from the canonical store. Without this the only way to
    // confirm a cutover took effect is to read a chapter and infer it.
    if state_cutover.is_empty() {
        println!("[cutover] no books cut over (PLOTWEB_CUTOVER_BOOKS unset)");
    } else if state_cutover.is_all() {
        println!("[cutover] EVERY book reads from the canonical store (PLOTWEB_CUTOVER_BOOKS=*)");
    } else {
        for book_id in state_cutover.book_ids() {
            println!("[cutover] book {book_id} reads from the canonical store");
        }
    }

    // Sync writes to a cut-over book move the canonical copy without touching git.
    // This pass carries them across, debounced, so version history, export and the
    // beta-reader views keep seeing current content — and so the cutover flag stays a
    // real rollback. Idle books cost nothing; see `mirror`'s module docs.
    tokio::spawn(plotweb_server::mirror::run(mirror_state));

    let addr = "0.0.0.0:3000";
    println!("PlotWeb server running on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    // Opt-in boot-time migration content audit (env `PLOTWEB_AUDIT_ON_BOOT`).
    // Runs the lock-free, read-only content audit in the background — alongside
    // serving, no rhypedb lock, no writes — and logs the report to stdout so the
    // migration fidelity of production data can be reviewed via the platform logs
    // without stopping the server or copying its volume. Unset the flag afterward.
    if std::env::var("PLOTWEB_AUDIT_ON_BOOT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        let dd = data_dir.clone();
        tokio::spawn(async move {
            plotweb_server::audit::run_boot_audit(dd).await;
        });
    }

    // Opt-in boot-time migration passes. The backfill (env `PLOTWEB_BACKFILL_ON_BOOT`)
    // refreshes canonical documents from git; the shadow pass
    // (env `PLOTWEB_SHADOW_ON_BOOT`, phase D) compares them to git and reports.
    //
    // They run in ONE task, in that order, deliberately. Spawned separately they race:
    // the shadow pass reads documents the backfill is still rewriting, and the report
    // comes back a mix of refreshed and stale — not wrong exactly, but not evidence of
    // anything either. Sequencing them makes "refresh, then measure" a single boot
    // rather than two restarts.
    let want_backfill = std::env::var("PLOTWEB_BACKFILL_ON_BOOT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let want_shadow = std::env::var("PLOTWEB_SHADOW_ON_BOOT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    // `PLOTWEB_RECONCILE_ON_BOOT` used to run a reconcile here. It no longer does, and
    // the variable is refused loudly rather than ignored quietly.
    //
    // The `*_ON_BOOT` hooks exist so a read-only pass can run lock-free beside live
    // traffic — the audit and the shadow report are read-only, and that is why they are
    // safe to leave on. Reconcile is not: it rebuilds canonical documents and orphans
    // the history every connected device holds. On 2026-08-28 it did exactly that,
    // unattended, while a browser was open, and a writing session was lost the next day
    // as a consequence. A rebuild is a decision someone makes, watching the output —
    // `plotweb-server reconcile --prefer git|crdt`, with `--dry-run` first.
    if std::env::var("PLOTWEB_RECONCILE_ON_BOOT")
        .ok()
        .is_some_and(|v| !v.is_empty() && v != "0")
    {
        eprintln!(
            "[boot] PLOTWEB_RECONCILE_ON_BOOT is no longer honoured: a reconcile rebuilds \
             canonical documents and orphans connected devices, so it must be run \
             deliberately — `plotweb-server reconcile --prefer git|crdt [--dry-run]`. \
             Unset the variable to silence this."
        );
    }

    if want_backfill || want_shadow {
        let dd = data_dir.clone();
        let cd = crdt_dir.clone();
        // What the shadow verdict means depends on whether the flag it talks about is
        // already on.
        let boot_cutover = plotweb_server::cutover::Cutover::from_env();
        tokio::spawn(async move {
            if want_backfill {
                plotweb_server::backfill::run_boot_backfill(
                    dd.clone(),
                    cd.clone(),
                    boot_rhype,
                    boot_books,
                )
                .await;
            }
            if want_shadow {
                plotweb_server::shadow::run_on_boot(dd, cd, &boot_cutover).await;
            }
        });
    }

    axum::serve(listener, app).await.unwrap();
}
