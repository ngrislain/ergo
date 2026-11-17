#!/usr/bin/env python3
"""Check the HTTPS-to-loopback MCP login handoff after cargo xtask build-web."""
import argparse
from pathlib import Path
from urllib.parse import parse_qs, urlencode, urlsplit

from playwright.sync_api import expect, sync_playwright

argparse.ArgumentParser(description=__doc__).parse_args()
WEB = Path(__file__).resolve().parents[1] / 'web'
ORIGIN = 'https://ergo.test'
LOOPBACK = 'http://127.0.0.1:32145/callback/' + 'a' * 64
LOGIN = ORIGIN + '/login?' + urlencode({'mcp_callback': LOOPBACK}) + '#connect'
session = {'google_enabled': True, 'name': 'Alice Builder', 'signed_in': False, 'development': False}
receipt = []


def serve_viewer(route):
    path = urlsplit(route.request.url).path
    if path == '/api/auth/me':
        route.fulfill(json=session)
    elif path == '/api/auth/mcp-token':
        route.fulfill(json={'token': 'test-token', 'expires_at': 2000000000})
    else:
        route.fulfill(path=WEB / ('index.html' if path == '/login' else path.lstrip('/')))


def capture_callback(route):
    receipt.append((route.request.all_headers(), route.request.post_data))
    route.fulfill(content_type='text/html', body='<h1>Authorization received</h1>')


with sync_playwright() as playwright:
    browser = playwright.chromium.launch(channel='chrome')
    page = browser.new_context(service_workers='block').new_page()
    page.route(ORIGIN + '/**', serve_viewer)
    page.route(LOOPBACK, capture_callback)
    page.goto(LOGIN)
    google = page.get_by_role('link', name='Continue with Google')
    expect(google).to_be_visible()
    return_to = parse_qs(urlsplit(google.get_attribute('href')).query)['return_to'][0]
    assert ORIGIN + return_to == LOGIN
    expect(page.get_by_role('button', name='Authorize MCP server')).to_have_count(0)
    session['signed_in'] = True
    page.reload()
    page.get_by_role('button', name='Authorize MCP server').click()
    expect(page.get_by_role('heading', name='Authorization received')).to_be_visible()
    [(headers, body)] = receipt
    assert headers['origin'] == ORIGIN
    assert headers['referer'] == ORIGIN + '/'
    assert body == 'token=test-token'
    browser.close()

print('MCP browser approval sends the HTTPS origin, not the approval URL, on the loopback POST.')
