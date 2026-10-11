// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License
// Browser lifecycle/cache plumbing only; all London Rura logic remains Rust.
//
// This is the one app in the family that cannot work offline, and the whole
// design of this file follows from that. It caches the app *shell* — the eight
// files in ASSETS — so the page still loads with no connection and can say
// plainly that it cannot reach TfL. It does NOT cache, and does not intercept,
// anything from api.tfl.gov.uk: a cached arrivals list is not a departures
// board, it is a wrong one, and TfL's feed holds only a few minutes of arrivals
// anyway. `ASSETS.includes` in the fetch handler is the enforcement — every URL
// outside the eight returns early, so an API request is never answered from the
// cache, never stored, and never even inspected.
//
// No waiting call here: an update must not swap the wasm under a live tab, or a
// board mid-fetch would read bindings from one version and logic from another.
// An update takes over once the old tabs close.
//
// IS_OWN is checked on every request rather than inferred from the scope.
// Scope is a registration's claim, not a promise, and this app shares an origin
// with pages that are not this board; IS_OWN keeps holding if the scope is ever
// wrong. The manifest is the one own-directory URL the handler never answers
// from the cache at all — it must always be a fresh read, because it decides
// what an install is.
const ROOT = new URL('./', self.location.href);
const CACHE = 'london-rura-' + ROOT.pathname + '-__VERSION__';
const ASSETS = ['./', 'app.js', 'app_bg.wasm', 'manifest.webmanifest', 'icon-192.png', 'icon-512.png', 'icon.svg', 'index.html'].map(p => new URL(p, ROOT).href);
const IS_OWN = url => url.startsWith(ROOT.href);
// The manifest's URL on its own. It stays in ASSETS — a manifest that 404s
// should still fail the install, loudly — but it is the one precached URL the
// fetch handler will never answer out of the cache (see there).
const MANIFEST = new URL('manifest.webmanifest', ROOT).href;
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
  // Everything else — unrelated pages, API requests, files, blobs — returns
  // early. api.tfl.gov.uk is not in ASSETS, so this one line keeps the live TfL
  // API out of the cache and lets the app's own error message appear. The
  // directory test is the other half of that rule: a URL outside this app's own
  // directory is never this worker's to answer, whatever the registration's
  // scope says, so a mis-scoped worker cannot serve the pages it shares this
  // origin with.
  //
  // The manifest is the one own-directory GET that always goes to the network.
  // It is what the browser reads to decide what app an install *is*: while a
  // cached copy answered, a fresh install of this app could read a manifest
  // from before any change to it, and derive the same stale identity the
  // family once shared — the very bug the identity fix was meant to retire.
  // The manifest is tiny; no offline entry point needs it cached.
  const url = event.request.url;
  if (url === MANIFEST) return;
  if (event.request.method !== 'GET' || !IS_OWN(url) || !ASSETS.includes(url)) return;
  event.respondWith((async () => {
    const cache = await caches.open(CACHE);
    const cached = await cache.match(event.request);
    return cached || fetch(event.request);
  })());
});
