# Native Builds Assessment

_Feasibility of shipping PlotWeb as a native desktop and/or mobile application._

Status: assessment only (no code changes). Date: 2026-07-14.

## TL;DR

- **PlotWeb today** = an Axum server (SQLite + git-backed storage, sessions,
  email, WebSockets) plus a Rust/WASM single-page frontend built on **rinch-web**
  (the browser-DOM backend of the rinch UI framework).
- **rinch itself is genuinely cross-platform**: a mature native **desktop**
  backend (winit + wgpu/Vello, with a CPU fallback), an early **Android** backend,
  and backend-agnostic `rsx!` / components / signals. **No iOS backend exists.**
- **The blocker is not rinch — it's how PlotWeb uses the web.** The frontend makes
  **163 direct `web_sys` calls across 13 files**, including **69 imperative
  DOM-manipulation sites** (`set_inner_html`, `query_selector`, `create_element`),
  plus `fetch`, WebSockets, History-API routing, and Google-Fonts loading. rinch's
  native backends have **no DOM and no cross-platform HTTP/WS/router/storage
  abstractions**, so a "true native" port is a real rewrite, not a recompile.
- **Recommended path (phased):**
  1. **Now, ~free:** lean on the **existing PWA** (manifest + service worker +
     the corrected icons) for installability on desktop and Android.
  2. **Low effort, high payoff:** a **Tauri desktop wrapper** that bundles the
     built SPA and runs the existing Axum server as a sidecar → a real installable
     desktop app reusing ~100% of the current code.
  3. **High effort, only if warranted:** a **true rinch-native** desktop/Android
     build (rewrite the 69 DOM sites + fetch/WS/router behind a platform
     abstraction, and solve the server-bundling story). Justified only if you need
     native rendering/perf/offline-first, not just "an app you can install."
  4. **iOS:** greenfield in rinch; reachable today only via a webview wrapper
     (Tauri mobile / Capacitor) or the PWA.

---

## 1. What PlotWeb is (and why it matters here)

PlotWeb is a **client–server** app, not a self-contained UI:

- **Server** (`crates/plotweb-server`, `-git`, `-import`, `-export`, `-common`):
  Axum REST + WebSockets, SQLite via sqlx, **git-backed** book/chapter/note
  storage (`git2`), tower-sessions (now SQLite-backed — persists across
  restarts), rhypedb metadata store, email (password reset), DOCX/Markdown import,
  multipart uploads.
- **Frontend** (`plotweb-web`): Rust → WASM via Trunk, rinch-web backend, talks to
  the server over `/api`.

Any "native build" must answer **two independent questions**:

1. **UI portability** — can the frontend render without a browser DOM?
2. **Server story** — does the app bundle the server locally, or call a remote API?

These are separable: a webview wrapper solves #1 trivially (it *is* a browser) and
lets you pick either server story; a true-native rinch port solves #1 the hard way
but is the only path to a "no-browser" binary.

---

## 2. rinch's native capabilities (what the framework gives us)

Findings from `/home/notyou/projects/fiction/rinch`:

| Backend | Maturity | Notes |
|---|---|---|
| **Desktop** (winit + wgpu/Vello) | **Strong, proven** | `rinch::shell::run(...)` entrypoints; real window/menu (`muda`)/tray (`tray-icon`)/devtools; CPU (tiny-skia) fallback for no-GPU. Two nontrivial apps (`paint`, `ui-zoo`) run from the *same* component code as their web builds. Default feature is `desktop`. |
| **Android** (tiny-skia + softbuffer, JNI) | **Early PoC** | `rinch-android` JNI bridge + Java companion classes; one example (`hello-android`) with a hand-rolled `build-apk.sh` (cargo-ndk → d8 → aapt2 → apksigner). CPU-only, no Gradle, no CI. |
| **iOS** | **None** | No UIKit/objc/Apple code, target, or example. Greenfield. |

- **Portable by construction:** the `rsx!` macro, `rinch-core` signals/effects, and
  60+ `rinch-components` build against a `DomDocument` trait — desktop uses
  `RinchDocument`, web uses `WebDocument`. "Same `rsx!`, same `Signal`, same
  `Button {…}`, different backend." PlotWeb's component *structure* would largely
  carry over.
- **`rinch-platform`** abstracts the **shell** (window, renderer, event loop,
  input, menus, time) — **not app services**.
- **Cross-platform app services rinch does _not_ provide** (each is a porting cost):
  HTTP/fetch, WebSockets (a WS pair exists but is buried in the WebRTC signaling
  crate), **client-side routing** (apps roll their own), fonts, storage/filesystem.
  **Clipboard is the one** genuinely cross-platform app service (`rinch-clipboard`).
- **Caveats:** desktop builds depend on a **wgpu fork** (`joeleaver/wgpu-fork`),
  not crates.io wgpu; the rich-text **Editor** view is `desktop`-gated. rinch is a
  fast-moving, pinned-commit dependency (we just landed a fix in it this session).

---

## 3. The porting cost on PlotWeb's side

### 3a. Frontend web-coupling (163 `web_sys` sites / 13 files)

| Concern | ~Sites | Native replacement | Difficulty |
|---|---|---|---|
| **Imperative DOM** (`set_inner_html`, `query_selector`, `create_element`, `append_child`) | **69** | Rewrite as declarative rinch nodes/components; `set_inner_html` of chapter/note HTML has **no native equivalent** — must parse markdown/HTML into rinch nodes or use the rinch editor view | **Hard** (architectural) |
| **Fonts** (`fonts.rs`, Google Fonts + `FontFace`) | 22 | Bundle fonts; load via Parley/HarfBuzz | Medium |
| **WebSockets** (`ws.rs`, live feedback) | 21 | `tokio-tungstenite` behind a `#[cfg]` seam | Medium |
| **Fetch/HTTP** (`api.rs`) | 11 | `reqwest`/`ureq` behind a `#[cfg]` seam | Low–Medium |
| **Editor/selection/touch** (contenteditable, `Selection`, `Range`, `TouchEvent`) | 12 | rinch editor view (desktop-gated) + input abstraction | Hard |
| **Routing** (`router.rs`, History/PopState) | 3 | Signal + `match` (rinch has no router) | Low |

The **69 imperative-DOM sites are the crux.** PlotWeb deliberately manipulates the
DOM directly in places (e.g. injecting rendered chapter HTML via `set_inner_html` —
see the project memory on rinch's rsx not supporting `dangerous_inner_html`). That
pattern is browser-only; on a native backend the content has to become real rinch
nodes. This is design work, not a mechanical swap.

### 3b. Server story

The server is a full backend (SQLite + **git working copies on disk** + rhypedb +
email + WS). Options:

- **Remote API** (app is a thin client to a hosted server): easiest; keeps the
  current deploy. But the "native app" is then online-only — arguably no better
  than the PWA except for packaging.
- **Bundled/embedded server** (sidecar process or in-proc): enables offline/local
  data. Feasible on **desktop** (ship the `plotweb-server` binary as a Tauri
  sidecar or embed Axum in-process). **Heavy on mobile** — git working copies +
  SQLite + an HTTP server on a phone is awkward (storage, background limits); would
  likely require rethinking storage (e.g. libgit2 to app-private dirs, or dropping
  git-on-device).

---

## 4. Options compared

| # | Option | UI approach | Server story | Effort | Reuse | Platforms | Verdict |
|---|---|---|---|---|---|---|---|
| **A** | **Enhance the PWA** | Existing web app | Remote (as today) | **Very low** | ~100% | Desktop + Android installable, iOS "Add to Home Screen" | **Do now.** Free, already 90% there. |
| **B** | **Tauri desktop wrapper** | Webview of built SPA | Axum as bundled **sidecar** (offline-capable) or remote | **Low–Med** | ~100% | macOS/Win/Linux | **Best near-term native.** Real installer, local data, minimal new code. |
| **C** | **True rinch-native desktop** | Rewrite to rinch desktop backend | Embed/sidecar Axum | **High** | UI structure only; ~69 DOM sites + net rewritten | macOS/Win/Linux | Only for native rendering/perf/feel. Depends on wgpu fork. |
| **D** | **rinch-native Android** | rinch Android backend (CPU) | Hard (server on device) | **High+** | UI structure; net rewrite; server rethink | Android | Framework immature (1 example); premature. |
| **E** | **Tauri Mobile / Capacitor** | Webview of SPA | Remote API | **Medium** | ~100% frontend | iOS + Android | Pragmatic mobile path; app-store presence without a rinch rewrite. |
| **F** | **rinch-native iOS** | — | — | **Very high** | — | iOS | Not supported by rinch; greenfield. Rule out. |

---

## 5. Recommendation

A phased path that maximizes reuse and defers the expensive rewrite until a
concrete need justifies it:

1. **Phase 0 — PWA (now):** verify installability end-to-end (manifest, service
   worker, offline shell, the corrected icons). Ships desktop + Android "install"
   today at ~zero cost. iOS gets Add-to-Home-Screen.
2. **Phase 1 — Tauri desktop (when a desktop app is wanted):** wrap the built SPA;
   ship `plotweb-server` as a sidecar for local/offline data (or point at the
   hosted API). Reuses ~all current code; yields real `.dmg`/`.msi`/AppImage
   installers. **This is the recommended first "native build."**
3. **Phase 2 — Mobile via webview (if app-store presence matters):** Tauri Mobile
   or Capacitor around the same SPA, talking to the hosted API. Gets iOS **and**
   Android without touching rinch.
4. **Phase 3 — True rinch-native (only if justified):** budget for rewriting the
   ~69 imperative-DOM sites + fetch/WS/router/fonts behind a `#[cfg(web)]` /
   `#[cfg(native)]` platform seam, and solving server bundling. Pursue only if you
   need genuine native rendering/perf/offline-first that a webview can't give.
   Track rinch's Android maturity and the wgpu-fork situation before committing.

### Rule out for now
- **rinch-native iOS** (unsupported).
- **rinch-native Android as a first step** (single hello-world example; too
  immature to build a real app on yet).

### Open questions to settle before Phase 1+
- **Why native?** (offline-first? app-store distribution? native feel/perf?
  desktop-only convenience?) — this decides B vs C and remote vs bundled server.
- **Data locality:** must a user's books live on-device (offline), or is a hosted
  server acceptable? This is the single biggest driver of effort.
- **Target platforms priority:** desktop-first vs mobile-first.

---

## Appendix — evidence

- Frontend web coupling: `plotweb-web/src` — 163 `web_sys::` sites / 13 files;
  breakdown: DOM 69, fonts 22, WS 21 (`ws.rs`), fetch 11 (`api.rs`), editor/touch
  12, routing 3 (`router.rs`).
- Server: `crates/plotweb-server` (Axum, sqlx/SQLite, tower-sessions +
  sqlx-store), `crates/plotweb-git` (git2 working copies), rhypedb, email, imports.
- rinch: `crates/rinch` (`desktop` default feature; `shell::run*`, `windows.rs`,
  `menu/`, `tray.rs`), `crates/rinch-android` (+ `examples/hello-android`),
  `crates/rinch-platform`, `crates/rinch-clipboard`; examples `paint*` / `ui-zoo*`
  prove web+desktop parity. No iOS code. Desktop needs the `joeleaver/wgpu-fork`.
