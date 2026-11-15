// Keeps a copy of the scanner app on the phone, so the installed app still opens when the address it
// was installed from no longer answers (the free internet address changes when the tunnel restarts).
// The app then asks for the computer's QR code and connects to the new address from here.
// Fresh files are used whenever the computer answers; the saved copy only when it does not.
const CACHE = 's2c-shell-v1';
const SHELL = ['/scanner.html', '/scan-worker.js', '/vendor/barcode-detector.js', '/vendor/zxing_reader.wasm',
  '/logo.svg', '/icon-180.png', '/icon-192.png', '/icon-512.png'];

self.addEventListener('install', e => {
  e.waitUntil(caches.open(CACHE).then(c => c.addAll(SHELL)).then(() => self.skipWaiting()));
});
self.addEventListener('activate', e => e.waitUntil(self.clients.claim()));

self.addEventListener('fetch', e => {
  const u = new URL(e.request.url);
  if (e.request.method !== 'GET' || u.origin !== self.location.origin || !SHELL.includes(u.pathname)) return;
  e.respondWith((async () => {
    const cache = await caches.open(CACHE);
    const saved = await cache.match(u.pathname); // saved without the ?t=... part
    const stop = new AbortController();
    const timer = saved && setTimeout(() => stop.abort(), 3000); // a dead address must not hang the app
    try {
      const fresh = await fetch(e.request, { signal: stop.signal });
      // A stopped tunnel answers with an error page, not with our files.
      if (!fresh.ok) return saved || fresh;
      cache.put(u.pathname, fresh.clone());
      return fresh;
    } catch {
      return saved || Response.error();
    } finally {
      clearTimeout(timer);
    }
  })());
});
