// PlotWeb service worker — minimal PWA support: installability + offline app shell.
//
// Strategy:
//   * App shell (navigations / index.html): network-first, fall back to the
//     cached shell when offline. This lets a fresh deploy (which points at new
//     hashed asset names) propagate as soon as the user is online again.
//   * Same-origin static assets (hashed JS/WASM/CSS, icons, fonts): stale-
//     while-revalidate. Hashed filenames make this safe — a new build fetches
//     new names and the old entries are simply evicted with the cache version.
//   * /api/ and any non-GET request: passthrough to the network, never cached
//     (dynamic + session-authenticated).
//
// Bump CACHE_VERSION to force old caches to be dropped on the next activate.
const CACHE_VERSION = 'plotweb-v1';
const SHELL_URL = '/index.html';
const PRECACHE = [SHELL_URL, '/', '/manifest.webmanifest', '/favicon.png', '/assets/logo.png'];

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(CACHE_VERSION)
      .then((cache) => cache.addAll(PRECACHE))
      .then(() => self.skipWaiting())
      .catch(() => self.skipWaiting())
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE_VERSION).map((k) => caches.delete(k))))
      .then(() => self.clients.claim())
  );
});

self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);

  // Only handle GET; let everything else (POST feedback, etc.) hit the network.
  if (request.method !== 'GET') return;

  // Never cache the API — it is dynamic and session-authenticated.
  if (url.origin === self.location.origin && url.pathname.startsWith('/api/')) return;

  // App-shell navigations: network-first so new deploys are picked up, with an
  // offline fallback to the cached shell (the SPA renders client-side routes).
  if (request.mode === 'navigate') {
    event.respondWith(
      fetch(request)
        .then((resp) => {
          const copy = resp.clone();
          caches.open(CACHE_VERSION).then((cache) => cache.put(SHELL_URL, copy));
          return resp;
        })
        .catch(() => caches.match(SHELL_URL).then((cached) => cached || caches.match('/')))
    );
    return;
  }

  // Same-origin static assets: stale-while-revalidate.
  if (url.origin === self.location.origin) {
    event.respondWith(
      caches.open(CACHE_VERSION).then((cache) =>
        cache.match(request).then((cached) => {
          const network = fetch(request)
            .then((resp) => {
              if (resp && resp.status === 200) cache.put(request, resp.clone());
              return resp;
            })
            .catch(() => cached);
          return cached || network;
        })
      )
    );
  }
});
