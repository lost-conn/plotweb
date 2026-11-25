//! Guards the jkbase frontend deploy against lockfile drift.
//!
//! jkbase's trunk buildpack builds `plotweb-web` with `trunk build --release
//! --offline`, after downloading the exact `wasm-bindgen` CLI in its fetch phase.
//! It reads that version from the Cargo.lock at the *build context root* (this
//! repo's root lock, because `jkbase.toml` sets `context = "."`), not from
//! `plotweb-web/Cargo.lock`, which is the lock trunk actually builds with
//! (`plotweb-web` is excluded from the root workspace). When the two disagree,
//! trunk finds the wrong wasm-bindgen on PATH, refuses it offline, and the deploy
//! fails — silently, since jkbase's build log truncates trunk's error away. That is
//! how every deploy from the rinch 5ff256a repin onward failed (2026-11).
//!
//! So the root lock must carry the same `wasm-bindgen` as the frontend lock. Fix a
//! failure here from the repo root with:
//!
//!     cargo update -p wasm-bindgen-futures --precise <plotweb-web's version>
//!
//! (that moves the whole wasm-bindgen / js-sys / web-sys family together), or
//! `cargo update -p wasm-bindgen --precise <version>` if nothing pins js-sys.

use std::path::Path;

/// Every version of `package` in the Cargo.lock at `path` (lockfile v3/v4 text).
fn locked_versions(path: &Path, package: &str) -> Vec<String> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let name_line = format!("name = \"{package}\"");
    let mut lines = text.lines();
    let mut versions = Vec::new();
    while let Some(line) = lines.next() {
        if line.trim() == name_line {
            if let Some(v) = lines
                .next()
                .and_then(|l| l.trim().strip_prefix("version = \""))
                .and_then(|l| l.strip_suffix('"'))
            {
                versions.push(v.to_string());
            }
        }
    }
    versions
}

#[test]
fn root_lock_wasm_bindgen_matches_the_frontend_lock() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let frontend = locked_versions(&repo.join("plotweb-web/Cargo.lock"), "wasm-bindgen");
    let root = locked_versions(&repo.join("Cargo.lock"), "wasm-bindgen");

    assert_eq!(
        frontend.len(),
        1,
        "plotweb-web/Cargo.lock should resolve exactly one wasm-bindgen, found {frontend:?}"
    );
    // jkbase takes the *first* wasm-bindgen entry in the root lock, so it must be the
    // only one — or at least the first must be the frontend's.
    assert_eq!(
        root.first(),
        frontend.first(),
        "root Cargo.lock has wasm-bindgen {root:?} but plotweb-web/Cargo.lock has {frontend:?}: \
         the jkbase frontend deploy provisions the root lock's version and will fail. \
         See this file's header for the fix."
    );
}
