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

    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:plotweb.db".into());
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "data/books".into());
    let rhype_dir = std::env::var("RHYPEDB_DATA_DIR").unwrap_or_else(|_| "data/rhypedb".into());

    let state = build_state(
        &db_url,
        PathBuf::from(&data_dir),
        &rhype_dir,
        EmailService::from_env(),
    )
    .await;

    // Static files — serve the built frontend, with SPA fallback to index.html
    let dist_path = std::env::var("DIST_DIR").unwrap_or_else(|_| "../plotweb-web/dist".into());
    let index_path = format!("{}/index.html", dist_path);
    let serve_dir = ServeDir::new(&dist_path).not_found_service(ServeFile::new(&index_path));

    let session = session_layer(state.db.clone()).await;
    let app = Router::new()
        .merge(api_router(state))
        .fallback_service(serve_dir)
        .layer(session);

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

    // Opt-in boot-time canonical Automerge backfill (env `PLOTWEB_BACKFILL_ON_BOOT`).
    // Runs the lock-free, additive content backfill in the background — alongside
    // serving, no rhypedb lock — writing a snapshot blob per clean document into
    // `PLOTWEB_CRDT_DIR`. Reversible: deleting that directory reverts to git-only.
    if std::env::var("PLOTWEB_BACKFILL_ON_BOOT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        let dd = data_dir.clone();
        let cd = std::env::var("PLOTWEB_CRDT_DIR").unwrap_or_else(|_| "data/crdt".into());
        tokio::spawn(async move {
            plotweb_server::backfill::run_boot_backfill(dd, cd).await;
        });
    }

    axum::serve(listener, app).await.unwrap();
}
