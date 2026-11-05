// Content-addressed binary storage; HTTP authorization/version revalidation is
// owned by the caller. No cached bytes are an offline permission grant.
const NAME = 'ergo-local-data-cell128-v1';
// Cell indexes are deliberately incompatible. Delete old caches rather than
// lazily importing old-layout packets into the new coordinate scheme.
const compatible = (async () => {
  const names = await indexedDB.databases?.().then(values => values.map(v => v.name)).catch(() => []) || [];
  for (const name of new Set(['ergo-local-data-v1', 'ergo-local-data-v2', ...names])) {
    if (name && name !== NAME && name.startsWith('ergo-local-data-')) {
      await new Promise(resolve => { const r = indexedDB.deleteDatabase(name); r.onsuccess = r.onerror = r.onblocked = resolve; });
    }
  }
  await Promise.all((await caches.keys()).filter(name => name.startsWith('ergo-read-packets-')).map(name => caches.delete(name)));
})().catch(() => {});
const MAX_ENTRY = 64 * 1024 * 1024;
const MIN_BUDGET = 128 * 1024 * 1024;
const BUDGET = 2 * 1024 * 1024 * 1024;
const MAX_ENTRIES = 4096;
const budgetFor = quota => Math.min(BUDGET, Math.max(MIN_BUDGET, Math.floor((quota || BUDGET * 5) / 5)));
const writes = {stored: 0, skipped: 0, lastSkipReason: null};
const skipped = reason => { writes.skipped++; writes.lastSkipReason = reason; return false; };
let database, opening, maintenance = Promise.resolve(), enabled = true, epoch = 0, pinTimer;
const versions = new Map(), pinned = new Set(), libraryPinned = new Set();
const activeReads = new Map();
let basicLibrary;
const isPinned = url => pinned.has(url) || libraryPinned.has(url);
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
  if (!opening) opening = compatible.then(() => new Promise((resolve, reject) => {
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
  })).catch(error => { opening = undefined; throw error; });
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
export async function cacheGet(url) {
  try { if (!enabled) return null; url = key(url); return await bounded(stored(url).catch(() => null), url.includes('/view?') || url.includes('/api/library/') ? 4000 : 300); }
  catch { return null; }
}
export async function cachePut(url, etag, bytes, newerOnly = false) {
  try {
    if (!enabled) return skipped('suspended');
    if (bytes.byteLength > MAX_ENTRY) return skipped('entry-too-large');
    url = key(url);
    const started = epoch, version = versions.get(url) || 0;
    const digest = await hash(bytes), data = new Blob([bytes]);
    const quota = await navigator.storage?.estimate?.().then(value => value.quota).catch(() => null);
    const budget = budgetFor(quota);
    return await enqueue(async () => {
      if (!enabled || started !== epoch || version !== (versions.get(url) || 0)) return skipped('superseded-or-suspended');
      const storage = await bounded(db());
      const tx = storage.transaction(['entries', 'blobs'], 'readwrite'), done = completed(tx);
      try {
      const entries = tx.objectStore('entries'), blobs = tx.objectStore('blobs');
      const previous = await request(entries.get(url));
      if (newerOnly && previous && Number(previous.etag.split(':')[0]) > Number(etag.split(':')[0])) {
        tx.abort(); await done.catch(()=>{}); return skipped('superseded-read-descriptor');
      }
      const all = (await request(entries.getAll())).filter(entry => entry.url !== url);
      all.push({url, etag, hash: digest, bytes: data.size, lastUsed: Date.now(), pinnedUntil:Math.max(previous?.pinnedUntil||0,isPinned(url)?Date.now()+75000:0)});
      const refs = new Map();
      for (const entry of all) {
        const record = refs.get(entry.hash) || {count: 0, bytes: entry.bytes};
        record.count++; refs.set(entry.hash, record);
      }
      let total = Array.from(refs.values()).reduce((sum, value) => sum + value.bytes, 0), count = all.length;
      const removed = new Set();
      for (const entry of all.slice().sort((a, b) => a.lastUsed - b.lastUsed)) {
        if (total <= budget && count <= MAX_ENTRIES) break;
        if (entry.url === url || isPinned(entry.url) || entry.pinnedUntil>Date.now()) continue;
        removed.add(entry.url); entries.delete(entry.url); count--;
        const record = refs.get(entry.hash);
        if (--record.count === 0) total -= record.bytes;
      }
      if (total>budget || count>MAX_ENTRIES) {tx.abort();await done.catch(()=>{});return skipped('capacity-protected-by-active-data');}
      // Reclaim only unreferenced blobs in the same atomic transaction.
      const live = new Set(all.filter(entry => !removed.has(entry.url)).map(entry => entry.hash));
      for (const old of await request(blobs.getAllKeys())) if (!live.has(old)) blobs.delete(old);
      if (!await request(blobs.get(digest))) blobs.put({hash: digest, data});
      entries.put(all[all.length - 1]);
      await done;
      writes.stored++;
      return true;
      } catch (error) {
        try { tx.abort(); } catch {}
        await done.catch(() => {});
        throw error;
      }
    });
  } catch (error) { return skipped(error?.name || 'storage-error'); } // Never block display.
}

// On-demand diagnostics expose counts/capacity, never request URLs or geometry.
// Raising this disk budget does not allocate a buffer of that size in WASM.
export async function cacheInfo() {
  const estimate = await navigator.storage?.estimate?.().catch(() => null);
  const base = {capacityBytes: budgetFor(estimate?.quota), maxEntryBytes: MAX_ENTRY, maxEntries: MAX_ENTRIES,
    browserQuotaBytes: estimate?.quota ?? null, browserUsageBytes: estimate?.usage ?? null,
    persistent: await navigator.storage?.persisted?.().catch(() => false) || false,
    writes: {...writes}};
  try {
    const storage = await bounded(db()), tx = storage.transaction('entries', 'readonly'), done = completed(tx);
    const entries = await request(tx.objectStore('entries').getAll()); await done;
    const blobs = new Map(entries.map(entry => [entry.hash, entry.bytes]));
    const protectedBlobs = new Set(entries.filter(entry => isPinned(entry.url) || entry.pinnedUntil > Date.now()).map(entry => entry.hash));
    return {...base, available: true, entries: entries.length, uniqueBlobs: blobs.size,
      storedBytes: [...blobs.values()].reduce((sum, bytes) => sum + bytes, 0),
      protectedBytes: [...protectedBlobs].reduce((sum, hash) => sum + blobs.get(hash), 0),
      regionEntries: entries.filter(entry => new URL(entry.url).pathname.endsWith('/view')).length,
      groundEntries: entries.filter(entry => new URL(entry.url).pathname.startsWith('/api/ground/')).length,
      libraryEntries: entries.filter(entry => new URL(entry.url).pathname.startsWith('/api/library/')).length};
  } catch { return {...base, available: false}; }
}
globalThis.ergoCacheInfo = cacheInfo;

// Tiny pending-read descriptors retain the exact accepted snapshot across reloads.
// They are never construction packets and never grant offline permission.
function pendingKey(url) {
  const parsed = new URL(key(url)); parsed.searchParams.delete('revision'); parsed.searchParams.sort();
  return key('/__ergo_pending_view?view='+encodeURIComponent(parsed.href));
}
export async function pendingReadGet(url) {
  try {
    const saved = await cacheGet(pendingKey(url)); if (!saved) return null;
    const value = JSON.parse(new TextDecoder().decode(saved.bytes));
    return value.pending && /^[0-9a-f]{64}$/.test(value.revision)
      && value.started <= Date.now()+300000 && Date.now()-value.started < 7*86400000
      ? {revision:value.revision} : null;
  } catch { return null; }
}
export async function pendingReadBegin(url, revision) {
  if (!/^[0-9a-f]{64}$/.test(revision)) return null;
  const target=pendingKey(url), started=Date.now(), token=started+':'+crypto.randomUUID();
  activeReads.set(target,token);versions.set(target,(versions.get(target)||0)+1);
  const bytes=new TextEncoder().encode(JSON.stringify({pending:true,revision,started}));
  return await cachePut(target,token,bytes,true) ? token : null;
}
export async function pendingReadFinish(url, token) {
  if (!token) return false;
  try {
    const target=pendingKey(url), started=epoch;
    const bytes=new TextEncoder().encode(JSON.stringify({pending:false}));
    const digest=await hash(bytes), data=new Blob([bytes]);
    return await enqueue(async()=>{
      if(!enabled||started!==epoch||(activeReads.has(target)&&activeReads.get(target)!==token))return false;
      const storage=await bounded(db()),tx=storage.transaction(['entries','blobs'],'readwrite'),done=completed(tx);
      const entries=tx.objectStore('entries'),blobs=tx.objectStore('blobs'),entry=await request(entries.get(target));
      if(entry?.etag!==token){await done;return false;}
      // A completion tombstone prevents a late older tab from restoring a stale
      // pending descriptor after a newer request has completed.
      if(!await request(blobs.get(digest)))blobs.put({hash:digest,data});
      entries.put({...entry,etag:Date.now()+':complete',hash:digest,bytes:data.size,lastUsed:Date.now()});
      const live=new Set((await request(entries.getAll())).map(value=>value.hash));
      for(const old of await request(blobs.getAllKeys()))if(!live.has(old))blobs.delete(old);
      await done;activeReads.delete(target);return true;
    });
  } catch {return false;}
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
  } catch {}
}
function refreshPins() {
  clearInterval(pinTimer);
  if (!enabled || !pinned.size && !libraryPinned.size) return;
  const heartbeat=()=>enqueue(async()=>{
    if (!enabled)return;
    const storage=await bounded(db()),tx=storage.transaction('entries','readwrite'),done=completed(tx);
    const entries=tx.objectStore('entries');
    for (const url of new Set([...pinned,...libraryPinned])) {
      const entry=await request(entries.get(url));
      if(entry)entries.put({...entry,lastUsed:Date.now(),pinnedUntil:Date.now()+75000});
    }
    await done;
  }).catch(()=>{});
  heartbeat();pinTimer=setInterval(heartbeat,30000);
}
export function cachePin(url) { pinned.clear(); if(url)pinned.add(key(url)); refreshPins(); }
export function cacheSuspend() { enabled = false; epoch++; clearInterval(pinTimer);if (database) database.close(); database = opening = undefined; }
export function cacheResume() { enabled = true;refreshPins(); }
export async function requestPersistence() { try { return await navigator.storage?.persist?.(); } catch { return false; } }

// Only public, content-versioned library data may be used without a world GET.
// Construction packets still require fresh authorization, including HTTP 304.
export async function warmBasicLibrary(revision) {
  if (!/^[0-9a-f]{64}$/.test(revision) || !enabled) return false;
  const started=epoch, root=`/api/library/${revision}/`;
  const controller=new AbortController(), timer=setTimeout(()=>controller.abort(),3600000);
  try {
    const load=async file=>{
      const url=root+file, cached=await cacheGet(url);
      if(cached)return {url,bytes:cached.bytes,etag:cached.etag,stored:true};
      const response=await fetch(url,{signal:controller.signal});
      if(!response.ok)throw Error('Basic library unavailable');
      const length=Number(response.headers.get('Content-Length'));
      if(length>MAX_ENTRY)throw Error('Basic library too large');
      const bytes=new Uint8Array(await response.arrayBuffer());
      if(bytes.byteLength>MAX_ENTRY)throw Error('Basic library too large');
      return {url,bytes,etag:response.headers.get('ETag')||revision,stored:false};
    };
    const manifest=await load('manifest.json'), value=JSON.parse(new TextDecoder().decode(manifest.bytes));
    if(value.format!=='ergo-basic-library-1'||value.revision!==revision||value.schema!==2 || !Array.isArray(value.constructs))return false;
    const pack=await load('parts.pack');
    if(pack.bytes.length!==value.pack_bytes||await hash(pack.bytes)!==value.pack_hash) {
      await cacheForget(pack.url);return false;
    }
    if(!enabled||epoch!==started)return false;
    libraryPinned.clear();libraryPinned.add(key(manifest.url));libraryPinned.add(key(pack.url));
    basicLibrary={manifest:value,pack:pack.bytes};
    const savedManifest=manifest.stored || await cachePut(manifest.url,manifest.etag,manifest.bytes);
    const savedPack=pack.stored || await cachePut(pack.url,pack.etag,pack.bytes);
    refreshPins();
    return savedManifest && savedPack;
  } catch { return false; } finally { clearTimeout(timer); }
}
export async function basicPartBytes(part) {
  try {
    if(!enabled||!basicLibrary||!globalThis.DecompressionStream)return null;
    const entry=Object.values(basicLibrary.manifest.parts.parts).find(entry=>entry.catalogue.id===part || entry.catalogue.aliases.includes(part));
    const asset=entry?.asset;
    if(!asset||asset.offset+asset.bytes>basicLibrary.pack.length)return null;
    const compressed=basicLibrary.pack.slice(asset.offset,asset.offset+asset.bytes);
    const stream=new Blob([compressed]).stream().pipeThrough(new DecompressionStream('gzip'));
    const bytes=new Uint8Array(await new Response(stream).arrayBuffer());
    if(bytes.length>32*1024*1024||await hash(bytes)!==asset.mesh_hash)return null;
    return bytes;
  } catch { return null; }
}
