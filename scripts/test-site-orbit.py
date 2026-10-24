#!/usr/bin/env python3
"""Real WebGL checks for cursor lighting, drag orbit, zoom and catalog art."""
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
        context = browser.new_context(viewport={"width":1440,"height":1000}, reduced_motion="no-preference")
        context.add_init_script("""Date.now=()=>Date.UTC(2026,9,24,HOUR);
          window.__gpu={shadow:0,main:0,errors:[],eye:[],scale:[]};
          const names=new WeakMap(), buffers=new WeakMap(), proto=WebGL2RenderingContext.prototype;
          const upload=proto.bufferData;
          proto.bufferData=function(target,value,...args){
            if(target===this.ARRAY_BUFFER&&ArrayBuffer.isView(value)){
              let hash=2166136261;for(const n of new Uint8Array(value.buffer,value.byteOffset,value.byteLength))hash=Math.imul(hash^n,16777619);
              buffers.set(this.getParameter(this.ARRAY_BUFFER_BINDING),hash>>>0);
            }return upload.call(this,target,value,...args);
          };
          const bind=proto.bindFramebuffer;
          proto.bindFramebuffer=function(target,fb){if(target===this.FRAMEBUFFER&&fb)window.__gpu.shadowCasters=[];return bind.call(this,target,fb);};
          const location=proto.getUniformLocation;
          proto.getUniformLocation=function(p,n){const v=location.call(this,p,n);if(v)names.set(v,n);return v;};
          const vector=proto.uniform3f;
          proto.uniform3f=function(l,x,y,z){if(names.get(l)==='eye')window.__gpu.eye=[x,y,z];return vector.call(this,l,x,y,z);};
          const matrix=proto.uniformMatrix4fv;
          proto.uniformMatrix4fv=function(l,t,m){if(names.get(l)==='display_scale')window.__gpu.scale=Array.from(m);if(names.get(l)==='shadow_matrix')window.__gpu.shadowMatrix=Array.from(m);return matrix.call(this,l,t,m);};
          const draw=proto.drawArraysInstanced;
          proto.drawArraysInstanced=function(...args){const shadow=!!this.getParameter(this.DRAW_FRAMEBUFFER_BINDING);window.__gpu[shadow?'shadow':'main']++;if(shadow)window.__gpu.shadowCasters.push([args[2],args[3],buffers.get(this.getVertexAttrib(0,this.VERTEX_ATTRIB_ARRAY_BUFFER_BINDING)),buffers.get(this.getVertexAttrib(3,this.VERTEX_ATTRIB_ARRAY_BUFFER_BINDING)),this.getUniform(this.getParameter(this.CURRENT_PROGRAM),this.getUniformLocation(this.getParameter(this.CURRENT_PROGRAM),'stud_mode'))].join(':'));draw.apply(this,args);const e=this.getError();if(e)window.__gpu.errors.push(e);};
        """.replace("HOUR",str(hour)))
        page = context.new_page()
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto(args.url)
        page.wait_for_load_state("networkidle")
        page.wait_for_function("document.querySelector('[data-brick-scene]').dataset.motion==='still'")
        mount = page.locator("[data-brick-scene]")
        canvas = mount.locator("canvas")
        expect(canvas).to_have_attribute("data-lighting","point")
        gpu = page.evaluate("window.__gpu")
        assert gpu["shadow"] and gpu["main"] and not gpu["errors"], gpu
        assert abs(gpu["scale"][5]-1.2) < 1e-6, "Physical height must not use flattened construction proportions"
        initial = canvas.screenshot()
        initial_pixels = Image.open(io.BytesIO(initial)).convert("RGB").getdata()
        assert sum(1 for rgb in initial_pixels if isinstance(rgb,tuple) and rgb[1]>rgb[0]*1.25 and rgb[1]>rgb[2]*1.25 and rgb[1]>50)>100, "Tree foliage must render green"
        pictures.append(initial)
        if hour == 0:
            before_eye = gpu["eye"]
            light = canvas.get_attribute("data-light-position")
            box = canvas.bounding_box()
            x, y = box["x"]+box["width"]*.5, box["y"]+box["height"]*.5
            pose = [mount.get_attribute("data-view-"+key) for key in ["yaw","pitch","zoom"]]
            page.mouse.move(box["x"]+box["width"]*.85,box["y"]+box["height"]*.2)
            page.wait_for_function("old=>document.querySelector('[data-brick-scene] canvas').dataset.lightPosition!==old",arg=light)
            assert page.evaluate("window.__gpu.eye")==before_eye, "Hover must position the light, not orbit the camera"
            assert [mount.get_attribute("data-view-"+key) for key in ["yaw","pitch","zoom"]]==pose
            assert canvas.screenshot()!=initial, "Pointer-controlled light must change the actual rendered illumination"
            page.mouse.move(x,y)
            page.evaluate("() => new Promise(requestAnimationFrame)")
            light = canvas.get_attribute("data-light-position")
            gpu = page.evaluate("window.__gpu")
            page.mouse.down()
            page.mouse.move(x+160,y+60,steps=12)
            page.mouse.up()
            page.wait_for_function("Number(document.querySelector('[data-brick-scene]').dataset.viewPitch)>1")
            assert page.evaluate("window.__gpu.eye") != before_eye, "Orbit must move the actual eye uniform"
            assert canvas.get_attribute("data-light-position") == light
            assert page.evaluate("window.__gpu.shadowMatrix") == gpu["shadowMatrix"], "Studio shadow projection must stay scene-fixed while orbiting"
            assert sorted(page.evaluate("window.__gpu.shadowCasters"))==sorted(gpu["shadowCasters"]), "Orbit must not change caster geometry, instances or stud LOD"
            assert canvas.screenshot() != initial
            canvas.focus()
            canvas.press("Home")
            canvas.press("-")
            page.wait_for_function("Number(document.querySelector('[data-brick-scene]').dataset.viewZoom)>1.1")
            canvas.press("Home")
            page.wait_for_function("document.querySelector('[data-brick-scene]').dataset.motion==='still'")
            page.mouse.move(x,y)
            page.mouse.wheel(0,200)
            page.wait_for_function("window.scrollY>0")
            assert [mount.get_attribute("data-view-"+key) for key in ["yaw","pitch","zoom"]]==["0.73000","0.65000","1.00000"], "Scrolling must not orbit or zoom the camera"
            page.evaluate("window.scrollTo({top:0,behavior:'instant'})")
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
    assert pictures[0] == pictures[1], "Point/studio rendering should be independent of UTC"
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
print("Site browser checks passed: green foliage, hover-driven point light, drag-only orbit, normal page scroll, keyboard zoom, unchanged light/casters during drag, GL passes and mobile layout.")
