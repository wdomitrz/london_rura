// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License
// Browser lifecycle/cache plumbing only; all London Rura logic remains Rust.
//
// This is the one app in the family that cannot work offline, and the whole
// design of this file follows from that. It caches the app *shell* — the eight
// files below — so that the page still loads with no connection and can say
// plainly that it cannot reach TfL. It does NOT cache, and does not intercept,
// anything from api.tfl.gov.uk: a cached arrivals list is not a departures
// board, it is a wrong one, and TfL's own feed only holds a few minutes of
// arrivals anyway. The `ASSETS.includes` allowlist below is the enforcement —
// the fetch handler returns early for every URL that is not one of the eight,
// so an API request is never answered from the cache, never stored, and never
// even looked at.
//
// No waiting call here: an update must not swap the wasm under a live tab, or
// a board in the middle of a fetch would be reading bindings from one version
// and logic from another. An update takes over once the old tabs close.
const ROOT = new URL('./', self.location.href);
const CACHE = 'london-rura-' + ROOT.pathname + '-__VERSION__';
const ASSETS = ['./', 'app.js', 'app_bg.wasm', 'manifest.webmanifest', 'icon-192.png', 'icon-512.png', 'icon.svg', 'index.html'].map(p => new URL(p, ROOT).href);
self.addEventListener('install', event => {
  event.waitUntil(caches.open(CACHE).then(cache => cache.addAll(ASSETS)));
});
self.addEventListener('activate', event => {
  event.waitUntil((async () => {
    const prefix = 'london-rura-' + ROOT.pathname + '-';
    for (const key of await caches.keys()) {
      if (key.startsWith(prefix) && key !== CACHE) await caches.delete(key);
    }
    await self.clients.claim();
  })());
});
self.addEventListener('fetch', event => {
  // Deliberately leave unrelated pages, API requests, files and blobs alone.
  // api.tfl.gov.uk is not in ASSETS, so this one line is what keeps the live
  // TfL API out of the cache and lets the app's own error message appear.
  if (event.request.method !== 'GET' || !ASSETS.includes(event.request.url)) return;
  event.respondWith((async () => {
    const cache = await caches.open(CACHE);
    const cached = await cache.match(event.request);
    return cached || fetch(event.request);
  })());
});
