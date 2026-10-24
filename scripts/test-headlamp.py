#!/usr/bin/env python3
"""Check headlamp alignment against the actual WebGL view-projection matrix."""
import argparse
import math
from playwright.sync_api import sync_playwright, expect


def ray(matrix):
    rows = [[matrix[c*4+r] for c in range(4)]+[float(r==c) for c in range(4)] for r in range(4)]
    for c in range(4):
        pivot = max(range(c,4),key=lambda r:abs(rows[r][c]))
        rows[c],rows[pivot] = rows[pivot],rows[c]
        scale = rows[c][c]
        rows[c] = [x/scale for x in rows[c]]
        for r in range(4):
            if r != c:
                scale = rows[r][c]
                rows[r] = [a-scale*b for a,b in zip(rows[r],rows[c])]
    inverse = [row[4:] for row in rows]
    def point(z):
        v = [row[2]*z+row[3] for row in inverse]
        return [x/v[3] for x in v[:3]]
    delta = [a-b for a,b in zip(point(1),point(-1))]
    length = math.sqrt(sum(x*x for x in delta))
    return [x/length for x in delta]


def check(page):
    uniforms = page.evaluate("window.__lamp")
    assert math.dist(uniforms["eye"],uniforms["light_position"]) < 1e-4, uniforms
    forward = ray(uniforms["view_projection"])
    assert sum(a*b for a,b in zip(forward,uniforms["light_forward"])) > 1-1e-6, uniforms
    return uniforms


parser = argparse.ArgumentParser()
parser.add_argument("--url",default="http://127.0.0.1:3000")
parser.add_argument("--artifacts",default=".playwright-mcp")
args = parser.parse_args()
with sync_playwright() as p:
    browser = p.chromium.launch(channel="chrome",headless=True,args=["--enable-unsafe-swiftshader"])
    page = browser.new_page(viewport={"width":1440,"height":950})
    page.add_init_script("""const start=performance.now();Date.now=()=>Date.UTC(2026,5,21,0)+performance.now()-start;
      window.__lamp={};const proto=WebGL2RenderingContext.prototype,names=new WeakMap();
      const location=proto.getUniformLocation;proto.getUniformLocation=function(p,n){const l=location.call(this,p,n);if(l)names.set(l,n);return l;};
      const vector=proto.uniform3f;proto.uniform3f=function(l,x,y,z){const n=names.get(l);if(['eye','light_position','light_forward'].includes(n))window.__lamp[n]=[x,y,z];return vector.call(this,l,x,y,z);};
      const matrix=proto.uniformMatrix4fv;proto.uniformMatrix4fv=function(l,t,m){if(names.get(l)==='view_projection')window.__lamp.view_projection=Array.from(m);return matrix.call(this,l,t,m);};
    """)
    page.goto(args.url+"/app")
    page.wait_for_load_state("networkidle")
    expect(page.locator(".status-message")).to_contain_text("Construction loaded",timeout=30000)
    canvas = page.locator("#world-canvas")
    expect(canvas).to_have_attribute("data-lighting","flashlight")
    for mode in ["Fly","Walk"]:
        page.get_by_role("button",name=mode,exact=True).click()
        expect(canvas).to_have_attribute("data-navigation",mode.lower())
        canvas.focus()
        before = check(page)
        page.keyboard.down("ArrowRight")
        page.wait_for_function("old=>JSON.stringify(window.__lamp.light_forward)!==JSON.stringify(old)",arg=before["light_forward"])
        page.keyboard.up("ArrowRight")
        check(page)
    browser.close()
print("Headlamp browser checks passed: fly/walk beam origin is the camera eye and axis matches the real center view ray before/after steering.")
