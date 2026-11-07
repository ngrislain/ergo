#!/usr/bin/env python3
"""Real WebGL checks for the unlisted part-library/colour reference page."""
import argparse
import pathlib
import re

from playwright.sync_api import sync_playwright

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--url', required=True)
parser.add_argument('--artifacts', default='target/catalogue-browser')
args = parser.parse_args()
output = pathlib.Path(args.artifacts)
output.mkdir(parents=True, exist_ok=True)

with sync_playwright() as playwright:
    browser = playwright.chromium.launch(channel='chrome', headless=True,
                                         args=['--enable-unsafe-swiftshader'])
    try:
        page = browser.new_page(viewport={'width': 1440, 'height': 1000})
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        response = page.request.get(args.url+'/api/catalog')
        assert response.ok
        catalogue = response.json()
        library = catalogue['library']
        assert len(catalogue['parts']) >= 138 and library['parts'] > len(catalogue['parts'])
        for part in catalogue['parts']:
            assert 'recommended' not in part and 'derivation' not in part
            assert part['mass_kg'] > 0 and all(v > 0 for v in part['nominal_dimensions_mm'])
            assert 'https://www.bricklink.com/' in part['mass_source']
            assert len(part['library_revision']) == 64
        tree = next(part for part in catalogue['parts'] if part['id'] == '3470.dat')
        assert tree['receiving_grids'][0]['first'][:2] == [-.5, -.5]
        horse = next(part for part in catalogue['parts'] if part['id'] == '4493c01.dat')
        assert {tuple(grid['first'][:2]) for grid in horse['receiving_grids']} == {(-.5, -1.25), (.5, -1.25), (-.5, 1.75), (.5, 1.75)}
        pieces = page.request.get(args.url+'/api/catalog/parts?limit=128').json()
        assert pieces['total'] == library['parts']
        estimated = [r for r in pieces['results'] if r.get('derivation')]
        assert estimated and all(r['mass_source'].startswith('estimated:voxel-v1') for r in estimated)
        assert page.request.get(args.url+'/api/catalog/parts?list=bricklink').json()['total'] == library['listed']
        assert page.request.get(args.url+'/catalogue/').ok
        page.goto(args.url+'/catalogue', wait_until='domcontentloaded')
        page.wait_for_function("document.body.dataset.catalogueReady==='true'")
        page.wait_for_load_state('networkidle')
        # The default view is every supported piece, ordered by ID.
        everything = library['parts']
        first_page = min(48, everything)
        assert page.locator('#part-results > li').count() == first_page
        assert f"of {everything:,} pieces" in page.locator('#part-count').inner_text()
        assert 'noindex' in page.locator('meta[name=robots]').get_attribute('content')
        assert '/catalogue' not in page.request.get(args.url+'/app').text()
        if everything > 48:
            page.get_by_role('button', name='Show more pieces').click()
            page.wait_for_function(f"document.querySelectorAll('#part-results > li').length==={min(96, everything)}")
        page.locator('#search').fill('tree')
        page.wait_for_function("document.querySelectorAll('#part-results > li').length===2")
        page.get_by_role('button', name=re.compile(r'3470\.dat')).click()
        page.wait_for_function("document.querySelector('#part-id').textContent==='3470.dat' && document.querySelector('#preview-status').textContent.startsWith('Source mesh')")
        page.get_by_role('button', name='Green (2)', exact=True).click()
        green = page.locator('#part-preview').evaluate('(canvas)=>canvas.toDataURL()')
        page.get_by_role('button', name='Red (4)', exact=True).click()
        red = page.locator('#part-preview').evaluate('(canvas)=>canvas.toDataURL()')
        assert green != red, 'Choosing a palette colour did not change the real geometry preview'
        page.locator('#part-preview').focus()
        page.keyboard.press('ArrowRight')
        assert red != page.locator('#part-preview').evaluate('(canvas)=>canvas.toDataURL()')
        page.locator('#search').fill('3308a')
        page.wait_for_function("document.querySelectorAll('#part-results > li').length===1")
        page.get_by_role('button', name=re.compile(r'3308a\.dat')).click()
        page.wait_for_function("document.querySelector('#part-id').textContent==='3308a.dat' && document.querySelector('#preview-status').textContent.startsWith('Source mesh')")
        assert page.locator('#interface-details li').count() == 2
        plain = page.locator('#part-preview').evaluate('(canvas)=>canvas.toDataURL()')
        page.locator('#show-interfaces').check()
        assert plain != page.locator('#part-preview').evaluate('(canvas)=>canvas.toDataURL()')
        assert page.locator('#part-preview').evaluate("canvas=>canvas.getContext('webgl2').getError()") == 0
        # Every supported piece looks alike; its facts state an estimated mass.
        assert page.locator('#part-results .tier-badge').count() == 0
        page.locator('#search').fill('2412b')
        page.wait_for_function("document.querySelectorAll('#part-results > li').length===1")
        page.get_by_role('button', name=re.compile(r'2412b\.dat')).click()
        page.wait_for_function("document.querySelector('#part-id').textContent==='2412b.dat' && document.querySelector('#preview-status').textContent.startsWith('Source mesh')")
        assert 'Estimated mass' in page.locator('#part-facts').inner_text()
        # BrickLink/official alias numbers find their canonical geometry.
        page.locator('#search').fill('15561')
        page.wait_for_function("document.querySelectorAll('#part-results > li').length===1")
        assert page.get_by_role('button', name=re.compile(r'2412b\.dat')).count() == 1
        page.locator('#search').fill('not-a-real-catalogue-piece')
        page.wait_for_function("!document.querySelector('#empty').hidden")
        page.get_by_role('button', name='Clear filters', exact=True).click()
        page.wait_for_function(f"document.querySelectorAll('#part-results > li').length==={first_page}")
        page.wait_for_function("[...document.querySelectorAll('#part-results img')].slice(0,10).every(image=>image.hasAttribute('src'))")
        page.screenshot(path=str(output/'catalogue-desktop.png'), full_page=False)
        page.set_viewport_size({'width': 390, 'height': 844})
        page.locator('#search').fill('3471')
        page.wait_for_function("document.querySelectorAll('#part-results > li').length===1")
        page.get_by_role('button', name=re.compile(r'3471\.dat')).click()
        page.wait_for_function("document.querySelector('#part-id').textContent==='3471.dat' && document.querySelector('#preview-status').textContent.startsWith('Source mesh')")
        page.locator('summary').click()
        page.get_by_role('link', name='Back to pieces', exact=True).click()
        page.evaluate('window.scrollTo(0,0)')
        page.screenshot(path=str(output/'catalogue-mobile.png'), full_page=False)
        assert page.evaluate('document.documentElement.scrollWidth <= innerWidth + 1'), 'Mobile page overflows horizontally'
        assert not errors, errors
        print(f'Catalogue passed: {library["parts"]} supported pieces, paged ID-ordered search, separate BrickLink list, aliases, genuine geometry, colours, keyboard rotation, interfaces, mobile and unlisted route.')
    finally:
        browser.close()
