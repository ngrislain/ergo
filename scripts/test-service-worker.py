#!/usr/bin/env python3
"""Exercise real service-worker activation and app reload on a secure localhost origin."""
import argparse
import http.server
import pathlib
import threading

from playwright.sync_api import sync_playwright

ROOT = pathlib.Path(__file__).resolve().parents[1]
OLD_WORKER = "self.addEventListener('install',()=>self.skipWaiting());self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()));"
HTML = b'''<!doctype html><title>Cell upgrade test</title><main>App ready</main>
<script>
sessionStorage.appLoads=String(Number(sessionStorage.appLoads||0)+1);
setInterval(()=>navigator.serviceWorker.getRegistration().then(reg=>{
  window.workerState=reg?.active?.state||'';
}),25);
</script>'''


class Handler(http.server.BaseHTTPRequestHandler):
    worker = ''

    def log_message(self, format, *args):
        pass

    def do_GET(self):
        body = self.worker.encode() if self.path == '/sw.js' else HTML
        self.send_response(200)
        self.send_header('Content-Type', 'application/javascript' if self.path == '/sw.js' else 'text/html')
        self.send_header('Cache-Control', 'no-store')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--sw-file', type=pathlib.Path, default=ROOT/'web/sw.js')
    args = parser.parse_args()
    source = args.sw_file.read_text()
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    url = f'http://127.0.0.1:{server.server_port}'
    try:
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch(channel='chrome', headless=True)
            try:
                for upgrade in [False, True]:
                    context = browser.new_context()
                    try:
                        Handler.worker = OLD_WORKER if upgrade else source
                        page = context.new_page()
                        errors = []
                        page.on('pageerror', lambda error: errors.append(str(error)))
                        page.goto(url+'/app', wait_until='networkidle')
                        page.evaluate("async()=>{await caches.open('ergo-model-objects-v3');await caches.open('ergo-read-packets-v1');}")
                        page.evaluate("()=>{void navigator.serviceWorker.register('/sw.js');}")
                        if upgrade:
                            page.wait_for_function('navigator.serviceWorker.controller !== null')
                            Handler.worker = source
                            page.evaluate("()=>{void navigator.serviceWorker.getRegistration().then(reg=>reg.update());}")
                        page.wait_for_function("window.workerState==='activated' && navigator.serviceWorker.controller && Number(sessionStorage.appLoads)===2", timeout=15000)
                        state = page.evaluate("async()=>({loads:Number(sessionStorage.appLoads),caches:await caches.keys(),active:(await navigator.serviceWorker.getRegistration()).active.state})")
                        assert state['loads'] == 2 and state['active'] == 'activated', state
                        assert not any(name in state['caches'] for name in ['ergo-model-objects-v3', 'ergo-read-packets-v1']), state
                        assert not errors, errors
                        print('Service-worker activation and one completed app reload passed:', 'upgrade' if upgrade else 'first install', flush=True)
                    finally:
                        context.close()
            finally:
                browser.close()
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == '__main__':
    main()
