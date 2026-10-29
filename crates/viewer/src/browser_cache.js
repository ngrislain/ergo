// Content-addressed binary storage; HTTP authorization/version revalidation is
// owned by the caller. No cached bytes are an offline permission grant.
const NAME = 'ergo-local-data-v2';
const LEGACY = 'ergo-read-packets-v1';
const MAX_ENTRY = 64 * 1024 * 1024;
const BUDGET = 512 * 1024 * 1024;
const MAX_ENTRIES = 1024;
let database, opening, maintenance = Promise.resolve(), enabled = true, epoch = 0, pinTimer;
const versions = new Map(), pinned = new Set();
const request = value => new Promise((resolve, reject) => {
  value.onsuccess = () => resolve(value.result);
  value.onerror = () => reject(value.error);
});
const completed = tx => {
  const promise = new Promise((resolve, reject) => {
    const timer = setTimeout(() => { try { tx.abort(); } catch {} reject(new Error('Local cache transaction timed out')); }, 3000);
    tx.oncomplete = () => { clearTimeout(timer); resolve(); };
    tx.onabort = tx.onerror = () => { clearTimeout(timer); reject(tx.error || new Error('Local cache transaction failed')); };
  });
  promise.catch(() => {});
  return promise;
};
const bounded = (promise, ms = 1500) => {
  let timer;
  return Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('Local cache unavailable')), ms); })]).finally(() => clearTimeout(timer));
};
function key(url) {
  const parsed = new URL(url, location.href);
  if (parsed.origin !== location.origin) throw new Error('Cross-origin cache key');
  for (const name of ['from_x', 'from_y', 'from_z', '']) parsed.searchParams.delete(name);
  parsed.searchParams.sort();
  return parsed.href;
}
function db() {
  if (!enabled) return Promise.reject(new Error('Local cache suspended'));
  if (database) return Promise.resolve(database);
  if (!opening) opening = new Promise((resolve, reject) => {
    const open = indexedDB.open(NAME, 1);
    open.onupgradeneeded = () => {
      open.result.createObjectStore('entries', {keyPath: 'url'});
      open.result.createObjectStore('blobs', {keyPath: 'hash'});
    };
    open.onerror = () => reject(open.error);
    open.onblocked = () => reject(new Error('Local cache blocked'));
    open.onsuccess = () => {
      if (!enabled) { open.result.close(); reject(new Error('Local cache suspended')); return; }
      database = open.result;
      database.onversionchange = () => { database.close(); database = undefined; opening = undefined; };
      resolve(database);
    };
  }).catch(error => { opening = undefined; throw error; });
  return opening;
}
async function hash(bytes) {
  return Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)), b => b.toString(16).padStart(2, '0')).join('');
}
function enqueue(work) {
  const next = maintenance.catch(() => {}).then(work);
  maintenance = next.catch(() => {});
  return next;
}
async function stored(url) {
  const storage = await bounded(db());
  const tx = storage.transaction(['entries', 'blobs'], 'readonly'), done = completed(tx);
  const entry = await request(tx.objectStore('entries').get(url));
  const blob = entry && await request(tx.objectStore('blobs').get(entry.hash));
  await done;
  if (!entry || !blob || entry.bytes > MAX_ENTRY || blob.data.size !== entry.bytes) return null;
  const bytes = new Uint8Array(await bounded(blob.data.arrayBuffer(), 3000));
  if (await hash(bytes) !== entry.hash) {
    await enqueue(async () => {
      const tx = storage.transaction(['entries', 'blobs'], 'readwrite'), done = completed(tx);
      const entries = tx.objectStore('entries');
      for (const candidate of await request(entries.getAll())) if (candidate.hash === entry.hash) entries.delete(candidate.url);
      tx.objectStore('blobs').delete(entry.hash);
      await done;
    });
    return null;
  }
  // Touch metadata only; reading a 35 MB region must not rewrite that blob.
  enqueue(async () => {
    if (!enabled) return;
    const tx = storage.transaction('entries', 'readwrite'), done = completed(tx);
    const current = await request(tx.objectStore('entries').get(url));
    if (current?.hash === entry.hash) tx.objectStore('entries').put({...current, lastUsed: Date.now()});
    await done;
  }).catch(() => {});
  return {etag: entry.etag, bytes};
}
async function legacy(url) {
  if (!url.includes('/view?') || !(await caches.keys()).includes(LEGACY)) return null;
  const cache = await caches.open(LEGACY);
  const candidate = (await cache.keys()).find(candidate => key(candidate.url) === url);
  if (!candidate) return null;
  const response = await cache.match(candidate);
  const amount = Number(response.headers.get('x-ergo-local-bytes'));
  if (!Number.isFinite(amount) || amount > MAX_ENTRY) return null;
  const bytes = new Uint8Array(await response.arrayBuffer());
  const etag = response.headers.get('etag');
  if (!etag || bytes.byteLength !== amount || await hash(bytes) !== response.headers.get('x-ergo-local-sha256')) return null;
  // Lazy migration preserves the old entry until its verified replacement has
  // committed. Several region URLs can then share one immutable blob.
  cachePut(url, etag, bytes).then(saved => saved && cache.delete(candidate)).catch(() => {});
  return {etag, bytes};
}
export async function cacheGet(url) {
  try { if (!enabled) return null; url = key(url); return await bounded(stored(url).catch(() => null), url.includes('/view?') ? 4000 : 300) || await bounded(legacy(url), 4000); }
  catch { return null; }
}
export async function cachePut(url, etag, bytes) {
  try {
    if (!enabled || bytes.byteLength > MAX_ENTRY) return false;
    url = key(url);
    const started = epoch, version = versions.get(url) || 0;
    const digest = await hash(bytes), data = new Blob([bytes]);
    const quota = await navigator.storage?.estimate?.().then(value => value.quota).catch(() => null);
    const budget = Math.min(BUDGET, Math.max(MAX_ENTRY, Math.floor((quota || BUDGET * 10) / 10)));
    return await enqueue(async () => {
      if (!enabled || started !== epoch || version !== (versions.get(url) || 0)) return false;
      const storage = await bounded(db());
      const tx = storage.transaction(['entries', 'blobs'], 'readwrite'), done = completed(tx);
      try {
      const entries = tx.objectStore('entries'), blobs = tx.objectStore('blobs');
      const previous = await request(entries.get(url));
      const all = (await request(entries.getAll())).filter(entry => entry.url !== url);
      all.push({url, etag, hash: digest, bytes: data.size, lastUsed: Date.now(), pinnedUntil:Math.max(previous?.pinnedUntil||0,pinned.has(url)?Date.now()+75000:0)});
      const refs = new Map();
      for (const entry of all) {
        const record = refs.get(entry.hash) || {count: 0, bytes: entry.bytes};
        record.count++; refs.set(entry.hash, record);
      }
      let total = Array.from(refs.values()).reduce((sum, value) => sum + value.bytes, 0), count = all.length;
      const removed = new Set();
      for (const entry of all.slice().sort((a, b) => a.lastUsed - b.lastUsed)) {
        if (total <= budget && count <= MAX_ENTRIES) break;
        if (entry.url === url || pinned.has(entry.url) || entry.pinnedUntil>Date.now()) continue;
        removed.add(entry.url); entries.delete(entry.url); count--;
        const record = refs.get(entry.hash);
        if (--record.count === 0) total -= record.bytes;
      }
      if (total>budget || count>MAX_ENTRIES) {tx.abort();await done.catch(()=>{});return false;}
      // Reclaim only unreferenced blobs in the same atomic transaction.
      const live = new Set(all.filter(entry => !removed.has(entry.url)).map(entry => entry.hash));
      for (const old of await request(blobs.getAllKeys())) if (!live.has(old)) blobs.delete(old);
      if (!await request(blobs.get(digest))) blobs.put({hash: digest, data});
      entries.put(all[all.length - 1]);
      await done;
      return true;
      } catch (error) {
        try { tx.abort(); } catch {}
        await done.catch(() => {});
        throw error;
      }
    });
  } catch { return false; } // Quota/private-mode failures must not block display.
}
export async function cacheForget(url) {
  try {
    url = key(url); versions.set(url, (versions.get(url) || 0) + 1);
    await enqueue(async () => {
      const storage = await bounded(db()), tx = storage.transaction(['entries', 'blobs'], 'readwrite'), done = completed(tx);
      const entries = tx.objectStore('entries');
      entries.delete(url);
      const live = new Set((await request(entries.getAll())).map(entry => entry.hash));
      for (const old of await request(tx.objectStore('blobs').getAllKeys())) if (!live.has(old)) tx.objectStore('blobs').delete(old);
      await done;
    });
    if ((await caches.keys()).includes(LEGACY)) {
      const cache = await caches.open(LEGACY);
      for (const candidate of await cache.keys()) if (key(candidate.url) === url) await cache.delete(candidate);
    }
  } catch {}
}
export function cachePin(url) {
  pinned.clear();clearInterval(pinTimer);
  if (!url) return;
  url=key(url);pinned.add(url);
  const heartbeat=()=>enqueue(async()=>{
    if (!enabled||!pinned.has(url))return;
    const storage=await bounded(db()),tx=storage.transaction('entries','readwrite'),done=completed(tx);
    const entries=tx.objectStore('entries'),entry=await request(entries.get(url));
    if(entry)entries.put({...entry,lastUsed:Date.now(),pinnedUntil:Date.now()+75000});
    await done;
  }).catch(()=>{});
  heartbeat();pinTimer=setInterval(heartbeat,30000);
}
export function cacheSuspend() { enabled = false; epoch++; clearInterval(pinTimer);if (database) database.close(); database = opening = undefined; }
export function cacheResume() { enabled = true;if(pinned.size)cachePin([...pinned][0]); }
export async function requestPersistence() { try { return await navigator.storage?.persist?.(); } catch { return false; } }
