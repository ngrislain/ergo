// Only immutable, authenticated model objects are persisted. User lists, sessions,
// manifests and app code always use the network. Logout clears this cache.
const CACHE = 'ergo-model-objects-v1';
const MAX_BYTES = 128 * 1024 * 1024;
const MAX_ITEMS = 512;
let maintenance = Promise.resolve();
self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', event => event.waitUntil(self.clients.claim()));
self.addEventListener('fetch', event => {
  const url = new URL(event.request.url);
  if (event.request.method !== 'GET' || url.origin !== self.location.origin ||
      !/^\/api\/worlds\/[0-9a-f-]+\/objects\/[0-9a-f]{64}$/.test(url.pathname)) return;
  event.respondWith((async () => {
    const cache = await caches.open(CACHE);
    const hit = await cache.match(event.request);
    if (hit) return hit;
    const response = await fetch(event.request);
    if (response.ok && response.headers.get('content-type')?.includes('application/cbor')) {
      const copy = response.clone();
      const request = event.request;
      maintenance = maintenance.catch(() => {}).then(async () => {
        const body = await copy.arrayBuffer();
        if (body.byteLength > MAX_BYTES / 2) return;
        const headers = new Headers(copy.headers);
        headers.delete('Content-Encoding');
        headers.delete('Content-Length');
        headers.set('X-Ergo-Bytes', String(body.byteLength));
        const stored = new Response(body, { status: copy.status, headers });
        await cache.put(request, stored);
        const keys = await cache.keys();
        let total = 0;
        const sizes = await Promise.all(keys.map(async key => Number((await cache.match(key)).headers.get('X-Ergo-Bytes') || 0)));
        total = sizes.reduce((a, b) => a + b, 0);
        let count = keys.length;
        for (let i = 0; i < keys.length && (total > MAX_BYTES || count > MAX_ITEMS); i++) {
          await cache.delete(keys[i]); total -= sizes[i]; count--;
        }
      });
      event.waitUntil(maintenance);
    }
    return response;
  })());
});
self.addEventListener('message', event => {
  if (event.data === 'clear-model-cache') event.waitUntil(caches.delete(CACHE));
});
