#!/usr/bin/env python3
"""Cache the immutable part-library document, then load it offline after reload."""
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
        revision = config['part_library']
        assert config['part_library_format'] == 'ergo-part-library-2'
        root = args.url + '/api/library/' + revision + '/'
        response = page.request.get(root + 'library.json')
        assert response.ok and 'immutable' in response.headers['cache-control']
        library = response.json()
        # Rust verifies the content revision before installing (unit-tested there).
        assert library['revision'] == revision
        assert page.request.get(root + 'library.json', headers={'If-None-Match': response.headers['etag']}).status == 304
        reviewed = page.request.get(root + 'reviewed.json').json()
        assert all('derivation' not in entry['catalogue'] for entry in reviewed['parts'].values())
        assert len(reviewed['revision']) == 64 and reviewed['revision'] != revision
        assert page.request.get(args.url + '/api/library/' + '0' * 64 + '/library.json').status == 404
        assert page.request.get(root + 'parts.pack').status == 404
        # Geometry objects are content-addressed and public only when the
        # installed library names them.
        entry = library['parts']['3001.dat']
        mesh = page.request.get(args.url + '/api/part-objects/' + entry['mesh_hash'])
        assert mesh.ok and 'immutable' in mesh.headers['cache-control']
        assert hashlib.sha256(mesh.body()).hexdigest() == entry['mesh_hash']
        assert page.request.get(args.url + '/api/part-objects/' + 'a' * 64).status == 404
        bindings = page.request.get(args.url + '/pkg/ergo_viewer.js').text()
        found = re.search(r"from ['\"](\./snippets/[^'\"]+browser_cache\.js)['\"]", bindings)
        assert found, 'The built viewer must export the browser cache module'
        module = args.url + '/pkg/' + found.group(1).removeprefix('./')
        page.route(args.url + '/app', lambda route: route.fulfill(body='<main>Part library regression</main>', content_type='text/html'))
        page.goto(args.url + '/app', wait_until='domcontentloaded')
        assert page.evaluate('''async ({module, revision}) => {
          const cache=await import(module);
          const bytes=await cache.partLibrary(revision);
          if(!bytes?.length)return false;
          const library=JSON.parse(new TextDecoder().decode(bytes));
          cache.cachePin('/api/unrelated-active-region');
          return library.revision===revision && Object.keys(library.parts).length>=138;
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
          const bytes=await cache.partLibrary(revision);
          if(!bytes?.length)throw Error('Offline part library cache miss');
          const library=JSON.parse(new TextDecoder().decode(bytes));
          if(await cache.partLibrary('not-a-revision')!==null)throw Error('Invented library revision');
          return {parts:Object.keys(library.parts).length,bytes:bytes.length,offline_after_reload:true};
        }''', {'module': module, 'revision': revision})
        assert not network, network
        print('Part library cache passed:', result)
    finally:
        browser.close()
