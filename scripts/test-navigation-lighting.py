#!/usr/bin/env python3
"""Real WebGL regression: changing navigation must retain orbit shadow coverage."""
import argparse
import json
import math
from playwright.sync_api import expect, sync_playwright


def project(matrix, point):
    clip = [sum(matrix[column * 4 + row] * point[column] for column in range(4))
            for row in range(4)]
    return [value / clip[3] for value in clip[:3]]


def shadow_sample(page):
    return page.evaluate("JSON.parse(JSON.stringify(window.__navigationLight))")


def daylight_projection(before, after):
    # Projection scale/orientation is independent of floating-origin translation.
    for row in range(3):
        a = [before['shadow_matrix'][column * 4 + row] for column in range(3)]
        b = [after['shadow_matrix'][column * 4 + row] for column in range(3)]
        assert math.dist(a, b) < 1e-6, ('Sun shadow coverage changed', before, after)
    # Entering walk may lower the eye, but the light footprint must stay in the
    # same world position. Only whole-cell floating-origin shifts are permitted.
    for axis, pitch in enumerate([128., 153.6, 128.]):
        delta = after['light_position'][axis] - before['light_position'][axis]
        assert abs(delta - round(delta / pitch) * pitch) < .005, ('Sun footprint jumped', delta)


def night_depth(sample):
    point = [eye + direction * 200 for eye, direction in
             zip(sample['eye'], sample['light_forward'])] + [1]
    projected = project(sample['shadow_matrix'], point)
    assert max(abs(projected[0]), abs(projected[1])) < 1e-4, 'Headlamp is not camera-coaxial'
    return projected[2]


parser = argparse.ArgumentParser()
parser.add_argument('--url', default='http://127.0.0.1:3000')
parser.add_argument('--artifacts', default='.playwright-mcp')
args = parser.parse_args()
results = []
with sync_playwright() as playwright:
    browser = playwright.chromium.launch(channel='chrome', headless=True,
                                          args=['--enable-unsafe-swiftshader'])
    try:
        for hour, expected in [(12, 'sunlight'), (0, 'flashlight')]:
            page = browser.new_page(viewport={'width': 1440, 'height': 950})
            errors = []
            page.on('pageerror', lambda error: errors.append(str(error)))
            page.add_init_script('''const started=performance.now();
              Date.now=()=>Date.UTC(2026,5,21,HOUR)+performance.now()-started;
              window.__navigationLight={casters:[]};
              const proto=WebGL2RenderingContext.prototype,names=new WeakMap();
              const buffers=new WeakMap(),upload=proto.bufferData;
              proto.bufferData=function(target,value,...rest){
                if(target===this.ARRAY_BUFFER&&ArrayBuffer.isView(value)){
                  let hash=2166136261;for(const byte of new Uint8Array(value.buffer,value.byteOffset,value.byteLength))hash=Math.imul(hash^byte,16777619);
                  buffers.set(this.getParameter(this.ARRAY_BUFFER_BINDING),hash>>>0);
                }return upload.call(this,target,value,...rest);
              };
              const bind=proto.bindFramebuffer;
              proto.bindFramebuffer=function(target,fb){if(target===this.FRAMEBUFFER&&fb)window.__navigationLight.casters=[];return bind.call(this,target,fb);};
              const draw=proto.drawArraysInstanced;
              proto.drawArraysInstanced=function(...args){
                if(this.getParameter(this.DRAW_FRAMEBUFFER_BINDING))window.__navigationLight.casters.push([
                  args[2],args[3],buffers.get(this.getVertexAttrib(0,this.VERTEX_ATTRIB_ARRAY_BUFFER_BINDING)),
                  buffers.get(this.getVertexAttrib(3,this.VERTEX_ATTRIB_ARRAY_BUFFER_BINDING)),
                  this.getUniform(this.getParameter(this.CURRENT_PROGRAM),this.getUniformLocation(this.getParameter(this.CURRENT_PROGRAM),'stud_mode'))
                ].join(':'));
                return draw.apply(this,args);
              };
              const get=proto.getUniformLocation;
              proto.getUniformLocation=function(program,name){const value=get.call(this,program,name);if(value)names.set(value,name);return value;};
              const vector=proto.uniform3f;
              proto.uniform3f=function(location,x,y,z){const name=names.get(location);if(['eye','light_position','light_forward'].includes(name))window.__navigationLight[name]=[x,y,z];return vector.call(this,location,x,y,z);};
              const matrix=proto.uniformMatrix4fv;
              proto.uniformMatrix4fv=function(location,transpose,value){const name=names.get(location);if(name==='shadow_matrix')window.__navigationLight[name]=Array.from(value);return matrix.call(this,location,transpose,value);};
            '''.replace('HOUR', str(hour)))
            page.goto(args.url + '/app', wait_until='networkidle')
            expect(page.locator('.status-message')).to_contain_text('Construction loaded', timeout=30000)
            canvas = page.locator('#world-canvas')
            expect(canvas).to_have_attribute('data-lighting', expected)
            if hour == 0:
                # Ensure the orbit light reaches beyond the old free-camera
                # 100-unit far plane, even with a very small default fixture.
                old = canvas.get_attribute('data-rendered-frames')
                canvas.dispatch_event('wheel', {'deltaY': 2700, 'cancelable': True})
                page.wait_for_function("old=>document.querySelector('#world-canvas').dataset.renderedFrames!==old", arg=old)
            orbit = shadow_sample(page)
            if hour == 12:
                assert orbit['casters'], 'Expected actual resident shadow geometry'
                for quality in ['performance','high','auto']:
                    old=canvas.get_attribute('data-rendered-frames')
                    page.get_by_label('Render detail',exact=True).select_option(quality)
                    page.wait_for_function("old=>document.querySelector('#world-canvas').dataset.renderedFrames!==old",arg=old)
                    current=shadow_sample(page)
                    daylight_projection(orbit,current)
                    assert sorted(current['casters'])==sorted(orbit['casters']), 'Colour quality changed actual shadow geometry'
                for zoom in [900,-900]:
                    old=canvas.get_attribute('data-rendered-frames')
                    canvas.dispatch_event('wheel',{'deltaY':zoom,'cancelable':True})
                    page.wait_for_function("old=>document.querySelector('#world-canvas').dataset.renderedFrames!==old",arg=old)
                    current=shadow_sample(page)
                    daylight_projection(orbit,current)
                    assert sorted(current['casters'])==sorted(orbit['casters']), 'Camera zoom/ground LOD changed the shadow casters'
            modes = []
            for mode in ['Fly', 'Walk', 'Fly']:
                old = canvas.get_attribute('data-rendered-frames')
                page.get_by_role('button', name=mode, exact=True).click()
                expect(canvas).to_have_attribute('data-navigation', mode.lower())
                page.wait_for_function("old=>document.querySelector('#world-canvas').dataset.renderedFrames!==old", arg=old)
                current = shadow_sample(page)
                if hour == 12:
                    daylight_projection(orbit, current)
                    if mode == 'Fly' and not modes:
                        for offset in [[0, -5, 0], [10, -20, -40], [-8, 6, 15]]:
                            a = project(orbit['shadow_matrix'], [x+y for x,y in zip(orbit['eye'],offset)]+[1])
                            b = project(current['shadow_matrix'], [x+y for x,y in zip(current['eye'],offset)]+[1])
                            assert math.dist(a,b) < 1e-4, 'Fly clipped/recentered the orbit shadow volume'
                else:
                    assert abs(night_depth(orbit)-night_depth(current)) < 2e-5, 'Night shadow reach shrank on a mode change'
                canvas.focus()
                old = canvas.get_attribute('data-rendered-frames')
                page.keyboard.down('ArrowRight')
                page.wait_for_function("old=>document.querySelector('#world-canvas').dataset.renderedFrames!==old", arg=old)
                page.keyboard.up('ArrowRight')
                turned = shadow_sample(page)
                if hour == 12:
                    daylight_projection(orbit, turned)
                else:
                    assert abs(night_depth(orbit)-night_depth(turned)) < 2e-5
                modes.append(mode.lower())
            assert not errors, errors
            results.append({'lighting': expected, 'modes': modes, 'orbit_coverage_preserved': True})
            page.close()
    finally:
        browser.close()
print('Navigation lighting checks passed: stable sunlight volume/casters across colour quality, zoom and navigation;', json.dumps(results))
