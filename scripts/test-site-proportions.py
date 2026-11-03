#!/usr/bin/env python3
"""Real-browser image and WebGL proportion checks across responsive layouts."""
import argparse
import json
import math
import pathlib

from playwright.sync_api import sync_playwright


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--url', default='http://127.0.0.1:4173')
parser.add_argument('--artifacts', type=pathlib.Path)
args = parser.parse_args()

# Capture the actual main-pass camera matrix, not an inferred CSS-only ratio.
PROJECTION = """(() => {
  const names = new WeakMap(), proto = WebGL2RenderingContext.prototype;
  const location = proto.getUniformLocation, matrix = proto.uniformMatrix4fv;
  proto.getUniformLocation = function(program, name) {
    const result = location.call(this, program, name);
    if (result) names.set(result, name);
    return result;
  };
  proto.uniformMatrix4fv = function(location, transpose, values) {
    if (names.get(location) === 'view_projection') window.__imageProjection = Array.from(values);
    return matrix.call(this, location, transpose, values);
  };
})();"""


def check_images(page):
    page.evaluate("() => Promise.all([...document.images].map(img => img.decode()))")
    images = page.evaluate("""() => [...document.images].filter(img => img.clientWidth && img.clientHeight).map(img => ({
      src: img.getAttribute('src'), width: img.clientWidth, height: img.clientHeight,
      naturalWidth: img.naturalWidth, naturalHeight: img.naturalHeight,
      transform: getComputedStyle(img).transform,
    }))""")
    assert images, 'Expected visible website illustrations'
    for image in images:
        expected_height = image['width'] * image['naturalHeight'] / image['naturalWidth']
        # clientWidth/Height and SVG intrinsic dimensions are integer-rounded.
        assert abs(image['height'] - expected_height) <= 1, f'Non-proportional image: {image}'
        if image['transform'] != 'none':
            matrix = list(map(float, image['transform'].removeprefix('matrix(').removesuffix(')').split(',')))
            a, b, c, d, _, _ = matrix
            assert math.isclose(math.hypot(a, b), math.hypot(c, d), rel_tol=1e-5), image
            assert abs(a*c+b*d) < 1e-5, f'Sheared image: {image}'
    assert page.evaluate('document.documentElement.scrollWidth <= innerWidth'), 'Horizontal overflow'
    return len(images)


def check_canvas(page):
    page.locator('[data-brick-scene]').scroll_into_view_if_needed()
    page.wait_for_function("""() => {
      const canvas = document.querySelector('[data-brick-scene] canvas');
      const dpr = Math.min(devicePixelRatio, 2);
      return canvas && window.__imageProjection &&
        canvas.width === Math.floor(canvas.clientWidth*dpr) &&
        canvas.height === Math.floor(canvas.clientHeight*dpr) &&
        document.querySelector('[data-brick-scene]').dataset.motion === 'still';
    }""")
    dimensions = page.evaluate("""() => {
      const canvas = document.querySelector('[data-brick-scene] canvas');
      return {width: canvas.clientWidth, height: canvas.clientHeight, projection: window.__imageProjection};
    }""")
    m = dimensions['projection']
    # The lengths of the two camera rows, converted to CSS pixels, must agree:
    # equal physical lengths cannot acquire unequal horizontal/vertical scale.
    horizontal = math.sqrt(sum(m[i]**2 for i in [0, 4, 8])) * dimensions['width']
    vertical = math.sqrt(sum(m[i]**2 for i in [1, 5, 9])) * dimensions['height']
    assert math.isclose(horizontal, vertical, rel_tol=1e-5), f'Stretched WebGL projection: {dimensions}'


with sync_playwright() as p:
    browser = p.chromium.launch(channel='chrome', headless=True,
                                args=['--enable-unsafe-swiftshader', '--use-angle=swiftshader'])
    errors = []
    checks = []
    for device_scale in [1, 2, 3]:
        context = browser.new_context(viewport={'width': 390, 'height': 844},
                                      device_scale_factor=device_scale, is_mobile=True,
                                      has_touch=True, reduced_motion='reduce')
        context.add_init_script(PROJECTION)
        page = context.new_page()
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.goto(args.url)
        page.wait_for_load_state('networkidle')
        for width, height in [(320, 740), (390, 844), (430, 932), (540, 800),
                              (768, 1024), (800, 1024), (844, 390), (1440, 1000)]:
            page.set_viewport_size({'width': width, 'height': height})
            check_canvas(page)
            count = check_images(page)
            checks.append({'width': width, 'height': height, 'dpr': device_scale, 'images': count})
            if args.artifacts and device_scale == 2 and width in [390, 844, 1440]:
                args.artifacts.mkdir(parents=True, exist_ok=True)
                page.screenshot(path=str(args.artifacts/f'site-proportions-{width}.png'), full_page=True, scale='css')
        # Header and footer are also shared with the legal/error pages.
        for route in ['/privacy/', '/terms/', '/404.html']:
            page.set_viewport_size({'width': 390, 'height': 844})
            page.goto(args.url.rstrip('/')+route)
            page.wait_for_load_state('networkidle')
            check_images(page)
        context.close()
    # Static/fallback illustrations must also retain their proportions.
    context = browser.new_context(viewport={'width': 390, 'height': 844}, java_script_enabled=False)
    page = context.new_page()
    page.goto(args.url)
    page.wait_for_load_state('networkidle')
    check_images(page)
    context.close()
    assert not errors, errors
    browser.close()
print(json.dumps({'passed': True, 'responsive_projection_checks': len(checks),
                  'device_pixel_ratios': [1, 2, 3], 'static_and_shared_page_images_checked': True}))
