#!/usr/bin/env python3
"""Cache every current canonical part/construct, then resolve offline after reload."""
import argparse
import hashlib
import re
from playwright.sync_api import sync_playwright

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--url', required=True)
parser.add_argument('--artifacts')
args = parser.parse_args()
with sync_playwright() as playwright:
    browser = playwright.chromium.launch(channel='chrome', headless=True)
    try:
        page = browser.new_page()
        config = page.request.get(args.url + '/api/config').json()
        revision = config['basic_library']
        root = args.url + '/api/library/' + revision + '/'
        response = page.request.get(root + 'manifest.json')
        assert response.ok and 'immutable' in response.headers['cache-control']
        manifest = response.json()
        assert manifest['revision'] == revision
        pack = page.request.get(root + 'parts.pack').body()
        assert len(pack) == manifest['pack_bytes']
        assert hashlib.sha256(pack).hexdigest() == manifest['pack_hash']
        assert page.request.get(root + 'manifest.json', headers={'If-None-Match': response.headers['etag']}).status == 304
        assert page.request.get(args.url + '/api/library/' + '0' * 64 + '/parts.pack').status == 404
        bindings = page.request.get(args.url + '/pkg/ergo_viewer.js').text()
        found = re.search(r"from ['\"](\./snippets/[^'\"]+browser_cache\.js)['\"]", bindings)
        assert found, 'The built viewer must export the browser cache module'
        module = found.group(1)
        module = args.url + '/pkg/' + module.removeprefix('./')
        page.route(args.url + '/app', lambda route: route.fulfill(body='<main>Basic library regression</main>', content_type='text/html'))
        page.goto(args.url + '/app', wait_until='domcontentloaded')
        assert page.evaluate('''async ({module, revision}) => {
          const cache=await import(module);
          if(!await cache.warmBasicLibrary(revision))return false;
          const value=await cache.cacheGet(`/api/library/${revision}/manifest.json`);
          const manifest=JSON.parse(new TextDecoder().decode(value.bytes));
          const keys=Object.keys(manifest.parts.parts);
          const ids=manifest.constructs.map(entry=>entry.id).sort();
          if(keys.length!==138 || JSON.stringify(ids)!==JSON.stringify(['castle-standard-v1','classic-car-v2','medieval-archer-v1','medieval-guard-v3','minifigure-v2']))return false;
          cache.cachePin('/api/unrelated-active-region');
          return true;
        }''', {'module': module, 'revision': revision})
        # A new JS context must hydrate from persistent bytes, with all library
        # network reads blocked. No previous in-memory library survives reload.
        network = []
        def block(route):
            network.append(route.request.url)
            route.abort()
        page.route('**/api/library/**', block)
        page.reload(wait_until='domcontentloaded')
        result = page.evaluate('''async ({module, revision}) => {
          const cache=await import(module);
          if(!await cache.warmBasicLibrary(revision))throw Error('Offline complete library cache miss');
          const value=await cache.cacheGet(`/api/library/${revision}/manifest.json`);
          const manifest=JSON.parse(new TextDecoder().decode(value.bytes));
          let count=0,bytes=0;
          for(const entry of Object.values(manifest.parts.parts)) {
            const mesh=await cache.basicPartBytes(entry.catalogue.id);
            if(!mesh?.length)throw Error('Missing offline part '+entry.catalogue.id);
            count++;bytes+=mesh.length;
          }
          if(await cache.basicPartBytes('not-a-part.dat')!==null)throw Error('Invented cached part');
          return {parts:count,constructs:manifest.constructs.length,decoded_bytes:bytes,offline_after_reload:true};
        }''', {'module': module, 'revision': revision})
        assert not network, network
        print('Complete basic library cache passed:', result)
    finally:
        browser.close()
