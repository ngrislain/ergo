#!/usr/bin/env python3
"""Real WebGL checks for studio lighting, camera orbit, zoom and catalog art."""
import argparse
import io
import pathlib
from PIL import Image
from playwright.sync_api import sync_playwright, expect

parser = argparse.ArgumentParser()
parser.add_argument("--url", default="http://127.0.0.1:4173")
parser.add_argument("--artifacts", default=".playwright-mcp")
args = parser.parse_args()
output = pathlib.Path(args.artifacts)
output.mkdir(parents=True, exist_ok=True)

with sync_playwright() as p:
    browser = p.chromium.launch(channel="chrome", headless=True,
        args=["--enable-unsafe-swiftshader", "--use-angle=swiftshader"])
    errors = []
    pictures = []
    for hour in [0, 12]:
        context = browser.new_context(viewport={"width":1440,"height":1000}, reduced_motion="reduce")
        context.add_init_script("""Date.now=()=>Date.UTC(2026,9,24,HOUR);
          window.__gpu={shadow:0,main:0,errors:[],eye:[],scale:[]};
          const names=new WeakMap(), proto=WebGL2RenderingContext.prototype;
          const location=proto.getUniformLocation;
          proto.getUniformLocation=function(p,n){const v=location.call(this,p,n);if(v)names.set(v,n);return v;};
          const vector=proto.uniform3f;
          proto.uniform3f=function(l,x,y,z){if(names.get(l)==='eye')window.__gpu.eye=[x,y,z];return vector.call(this,l,x,y,z);};
          const matrix=proto.uniformMatrix4fv;
          proto.uniformMatrix4fv=function(l,t,m){if(names.get(l)==='display_scale')window.__gpu.scale=Array.from(m);if(names.get(l)==='shadow_matrix')window.__gpu.shadowMatrix=Array.from(m);return matrix.call(this,l,t,m);};
          const draw=proto.drawArraysInstanced;
          proto.drawArraysInstanced=function(...args){window.__gpu[this.getParameter(this.DRAW_FRAMEBUFFER_BINDING)?'shadow':'main']++;draw.apply(this,args);const e=this.getError();if(e)window.__gpu.errors.push(e);};
        """.replace("HOUR",str(hour)))
        page = context.new_page()
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto(args.url)
        page.wait_for_load_state("networkidle")
        page.wait_for_function("document.querySelector('[data-brick-scene]').dataset.motion==='still'")
        mount = page.locator("[data-brick-scene]")
        canvas = mount.locator("canvas")
        expect(canvas).to_have_attribute("data-lighting","studio")
        gpu = page.evaluate("window.__gpu")
        assert gpu["shadow"] and gpu["main"] and not gpu["errors"], gpu
        assert abs(gpu["scale"][5]-1.2) < 1e-6, "Physical height must not use flattened construction proportions"
        initial = canvas.screenshot()
        pictures.append(initial)
        if hour == 0:
            before_eye = gpu["eye"]
            light = canvas.get_attribute("data-light-position")
            box = canvas.bounding_box()
            x, y = box["x"]+box["width"]*.5, box["y"]+box["height"]*.5
            page.mouse.move(x,y)
            page.mouse.down()
            page.mouse.move(x+160,y+60,steps=12)
            page.mouse.up()
            page.wait_for_function("Number(document.querySelector('[data-brick-scene]').dataset.viewPitch)>1")
            assert page.evaluate("window.__gpu.eye") != before_eye, "Orbit must move the actual eye uniform"
            assert canvas.get_attribute("data-light-position") == light
            assert page.evaluate("window.__gpu.shadowMatrix") == gpu["shadowMatrix"], "Studio shadow projection must stay scene-fixed while orbiting"
            assert canvas.screenshot() != initial
            canvas.focus()
            canvas.press("Home")
            canvas.press("-")
            page.wait_for_function("Number(document.querySelector('[data-brick-scene]').dataset.viewZoom)>1.1")
            canvas.press("Home")
            page.mouse.move(x,y)
            page.mouse.wheel(0,200)
            page.wait_for_function("Number(document.querySelector('[data-brick-scene]').dataset.viewZoom)>1.1")
            for _ in range(4):
                page.get_by_role("button", name="Orbit around the brick model").click()
            page.wait_for_function("document.querySelector('[data-brick-scene]').dataset.motion==='still'")
            rear = canvas.screenshot(path=str(output/"site-studio-rear.png"))
            pixels = Image.open(io.BytesIO(rear)).convert("RGB").getdata()
            colored = [rgb for rgb in pixels if isinstance(rgb,tuple) and max(rgb)-min(rgb)>35 and min(rgb)<180]
            assert len(colored)>500 and sum(max(rgb) for rgb in colored)/len(colored)>90
            page.get_by_role("button", name="Reset view").click()
            image = page.locator(".intro-line .tiny-brick")
            svg = page.request.get(args.url+"/brick-outline.svg").text()
            assert 'data-part="3003.dat"' in svg and 'data-body-mm="16,9.6,16"' in svg
            expect(image).to_be_visible()
            page.screenshot(path=str(output/"site-studio-desktop.png"))
        context.close()
    assert pictures[0] == pictures[1], "Studio rendering should be independent of UTC"
    mobile = browser.new_context(viewport={"width":390,"height":844}, is_mobile=True, has_touch=True, reduced_motion="reduce")
    page = mobile.new_page()
    page.goto(args.url)
    page.wait_for_load_state("networkidle")
    canvas = page.locator("[data-brick-scene] canvas")
    expect(canvas).to_be_visible()
    assert page.evaluate("document.documentElement.scrollWidth<=innerWidth")
    page.screenshot(path=str(output/"site-studio-mobile.png"))
    assert not errors, errors
    browser.close()
print("Site browser checks passed: real camera orbit, zoom, fixed studio key, day/night equality, lit rear views, physical proportions, GL passes and mobile layout.")
