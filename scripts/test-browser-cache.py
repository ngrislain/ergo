#!/usr/bin/env python3
"""Actual IndexedDB checks: deduplication, LRU, corruption, denial and logout fencing."""
import argparse,re
from playwright.sync_api import expect,sync_playwright

parser=argparse.ArgumentParser();parser.add_argument('--url',default='http://127.0.0.1:3000');parser.add_argument('--artifacts');args=parser.parse_args()
with sync_playwright() as playwright:
    browser=playwright.chromium.launch(channel='chrome',headless=True,args=['--enable-unsafe-swiftshader'])
    try:
        page=browser.new_page(viewport={'width':1200,'height':900});errors=[]
        page.on('pageerror',lambda error:errors.append(str(error)))
        page.goto(args.url+'/app',wait_until='domcontentloaded')
        expect(page.locator('.status-message')).to_contain_text('Construction loaded',timeout=60000)
        bindings=page.request.get(args.url+'/pkg/ergo_viewer.js').text()
        module=re.search(r"from ['\"](\./snippets/[^'\"]+browser_cache\.js)['\"]",bindings).group(1)
        report=page.evaluate('''async module => {
          const cache=await import(module);
          const req=request=>new Promise((resolve,reject)=>{request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);});
          const done=tx=>new Promise((resolve,reject)=>{tx.oncomplete=resolve;tx.onerror=tx.onabort=()=>reject(tx.error);});
          const inspect=async()=>{const db=await req(indexedDB.open('ergo-local-data-v2',1));try{const tx=db.transaction(['entries','blobs'],'readonly'),complete=done(tx);const entries=await req(tx.objectStore('entries').getAll());const blobs=await req(tx.objectStore('blobs').getAllKeys());await complete;return {entries,blobs};}finally{db.close();}};
          const originalOpen=indexedDB.open.bind(indexedDB);
          indexedDB.open=()=>{throw new Error('Simulated blocked storage');};
          if(await cache.cacheGet('/api/cache-test/missing')!==null)throw Error('Blocked storage must be a miss');
          indexedDB.open=originalOpen;
          const bytes=new Uint8Array([1,2,3,4]);
          if(!await cache.cachePut('/api/cache-test/a','one',bytes))throw Error('Initial cache write failed');
          await cache.cachePut('/api/cache-test/b','two',bytes);
          let state=await inspect();if(state.entries.length!==2||state.blobs.length!==1)throw Error('Identical content was duplicated');
          const old=await caches.open('ergo-read-packets-v1');
          const checksum=Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes)),b=>b.toString(16).padStart(2,'0')).join('');
          for(const x of [7,8,9])await old.put('/api/cache-test/view?x='+x+'&format=cbor&revision=fixture',new Response(bytes,{headers:{etag:'legacy-'+x,'x-ergo-local-bytes':'4','x-ergo-local-sha256':checksum}}));
          for(const x of [7,8,9])if(!(await cache.cacheGet('/api/cache-test/view?x='+x+'&format=cbor&revision=fixture')))throw Error('Legacy cache was lost');
          for(let attempt=0;attempt<100&&(await old.keys()).length;attempt++)await new Promise(resolve=>setTimeout(resolve,20));
          if((await old.keys()).length)throw Error('Verified migration did not finish');
          state=await inspect();if(state.blobs.length!==1)throw Error('Migrated packets were not deduplicated');
          const hit=await cache.cacheGet('/api/cache-test/a');if(hit.etag!=='one'||hit.bytes[2]!==3)throw Error('Cache round trip failed');
          const db=await req(indexedDB.open('ergo-local-data-v2',1));const tx=db.transaction('blobs','readwrite'),complete=done(tx);
          tx.objectStore('blobs').put({hash:state.blobs[0],data:new Blob([new Uint8Array([9,9,9,9])])});await complete;db.close();
          if(await cache.cacheGet('/api/cache-test/a')!==null)throw Error('Corrupted bytes were reused');
          await cache.cachePut('/api/cache-test/a','repaired',bytes);
          if((await cache.cacheGet('/api/cache-test/a')).etag!=='repaired')throw Error('Corruption did not heal');
          await cache.cacheForget('/api/cache-test/a');
          if(await cache.cacheGet('/api/cache-test/a')!==null)throw Error('Read-denied entry survived eviction');
          const originalEstimate=navigator.storage.estimate.bind(navigator.storage);
          navigator.storage.estimate=async()=>({quota:640*1024*1024,usage:0});
          const big=new Uint8Array(32*1024*1024);
          big.fill(1);await cache.cachePut('/api/cache-test/hot','hot',big);
          big.fill(2);await cache.cachePut('/api/cache-test/cold','cold',big);
          if(!await cache.cacheGet('/api/cache-test/hot'))throw Error('Hot entry missing');
          cache.cachePin('/api/cache-test/hot');
          big.fill(3);await cache.cachePut('/api/cache-test/new','new',big);
          if(await cache.cacheGet('/api/cache-test/cold')!==null)throw Error('LRU did not evict cold data');
          if(!await cache.cacheGet('/api/cache-test/hot'))throw Error('Active data was evicted');
          const originalPut=IDBObjectStore.prototype.put;
          IDBObjectStore.prototype.put=function(value,...args){if(this.name==='blobs')throw new DOMException('Simulated quota failure','QuotaExceededError');return originalPut.call(this,value,...args);};
          big.fill(4);if(await cache.cachePut('/api/cache-test/quota','quota',big))throw Error('Quota failure was reported as a cache hit');
          IDBObjectStore.prototype.put=originalPut;
          if(!await cache.cacheGet('/api/cache-test/new'))throw Error('Failed write discarded unrelated retained data');
          navigator.storage.estimate=originalEstimate;
          const pending=cache.cachePut('/api/cache-test/late','late',big);cache.cacheSuspend();
          if(await pending)throw Error('A suspended/logged-out write committed');
          cache.cacheResume();
          if(await cache.cacheGet('/api/cache-test/late')!==null)throw Error('Logged-out bytes reappeared');
          state=await inspect();return {deduplicated:true,legacy_migrated:true,corruption_healed:true,lru_and_active_pin:true,logout_fenced:true,entries:state.entries.length,blobs:state.blobs.length};
        }''',args.url+'/pkg/'+module.removeprefix('./'))
        assert not errors,errors
        print('Browser cache checks passed:',report)
    finally:browser.close()
