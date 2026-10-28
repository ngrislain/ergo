#!/usr/bin/env python3
"""Profile real WebGL rendering of a local construction preview; never publish it."""
import argparse,json,os,socket,subprocess,tempfile,time,urllib.request
from pathlib import Path
from playwright.sync_api import expect,sync_playwright

ROOT=Path(__file__).resolve().parents[1]

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('construction',type=Path)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--qualities',nargs='+',choices=['auto','performance','high'],default=['auto','performance'])
    args=parser.parse_args();args.construction=args.construction.resolve()
    if not args.construction.is_file() or not args.output.parent.is_dir():parser.error('Construction and output parent must exist')
    scene=subprocess.check_output(['cargo','run','--release','-p','ergo-client','--example','profile_scene','--','--scene-json',str(args.construction)],cwd=ROOT)
    preview=b'{"scene":'+scene+b',"notes":[]}'
    with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    base=f'http://127.0.0.1:{port}'
    environment={key:value for key,value in os.environ.items() if not key.startswith(('ERGO_','S3_','AWS_','GOOGLE_'))}
    with tempfile.TemporaryDirectory(prefix='ergo-render-profile-') as temporary,tempfile.TemporaryFile(mode='w+t') as log:
        directory=Path(temporary);(directory/'web').symlink_to(ROOT/'web',target_is_directory=True)
        environment.update(ERGO_BIND=f'127.0.0.1:{port}',ERGO_PUBLIC_URL=base,ERGO_DATA_DIR=str(directory/'store'),
                           ERGO_ISOLATED_FIXTURES='1',ERGO_ASSET_FETCH='0',ERGO_GROUND_SOURCE='disabled')
        server=subprocess.Popen([str(ROOT/'target/debug/ergo-server')],cwd=directory,env=environment,stdout=log,stderr=log)
        try:
            for _ in range(150):
                try:
                    with urllib.request.urlopen(base+'/api/config',timeout=2):break
                except OSError:
                    if server.poll() is not None:raise RuntimeError('Disposable render server stopped')
                    time.sleep(.1)
            else:raise RuntimeError('Disposable render server did not start')
            with sync_playwright() as playwright:
                browser=playwright.chromium.launch(channel='chrome',headless=True,args=['--enable-unsafe-swiftshader'])
                try:
                    page=browser.new_page(viewport={'width':1440,'height':1000},device_scale_factor=1)
                    page.add_init_script('const clockStart=performance.now();Date.now=()=>Date.UTC(2026,5,21,12)+performance.now()-clockStart')
                    errors=[];page.on('pageerror',lambda error:errors.append(str(error)))
                    # The readback may contain aliases that are intentionally not
                    # valid authoring-import IDs. Only the test transport is
                    # intercepted; the real WASM planner and WebGL are unchanged.
                    page.route('**/api/import',lambda route:route.fulfill(status=200,content_type='application/json',body=preview))
                    page.goto(base+'/app',wait_until='domcontentloaded')
                    expect(page.get_by_role('button',name='＋ Import construction',exact=True)).to_be_enabled(timeout=60000)
                    expect(page.locator('.status-message')).to_contain_text('Construction loaded',timeout=60000)
                    page.get_by_role('button',name='＋ Import construction',exact=True).click()
                    page.locator('#model-file').set_input_files({'name':'read-only-profile.json','mimeType':'application/json','buffer':b'{}'})
                    invalid=page.locator('#import-dialog form').evaluate('form=>Array.from(form.querySelectorAll(":invalid")).map(input=>({type:input.type,message:input.validationMessage}))')
                    assert not invalid,invalid
                    with page.expect_response(lambda response:response.url==base+'/api/import',timeout=60000) as importing:
                        page.locator('#import-dialog form').evaluate('(form)=>form.requestSubmit()')
                    assert importing.value.ok,importing.value.text()
                    expect(page.locator('.status-message')).to_contain_text('Imported',timeout=120000)
                    canvas=page.locator('canvas').first
                    results=[]
                    # Prime every policy before the measured round so lazy GPU
                    # compilation/upload from the first transition is not a win
                    # attributed to whichever policy happens to run last.
                    for round_index,quality in enumerate(args.qualities*2):
                        page.get_by_role('combobox',name='Render detail',exact=True).select_option(quality)
                        page.wait_for_timeout(1000)
                        canvas.evaluate("canvas=>canvas.getContext('webgl2').finish()")
                        # Warm continuous UI-driven orbit; do not measure idle
                        # callbacks or confuse presentation intervals with GPU time.
                        frames=canvas.evaluate('''canvas => new Promise(resolve=>{
                          const intervals=[];let previous=null;const start=performance.now();
                          const rect=canvas.getBoundingClientRect(),x=rect.x+rect.width/2,y=rect.y+rect.height/2;
                          function pointer(type,dx){canvas.dispatchEvent(new PointerEvent(type,{bubbles:true,clientX:x+dx,clientY:y,button:0,pointerId:1}));}
                          pointer('pointerdown',0);
                          function frame(now){const elapsed=now-start;
                            if(elapsed>=2000&&previous!==null)intervals.push(now-previous);previous=now;
                            if(elapsed<6000){pointer('pointermove',Math.sin(elapsed*Math.PI/3000)*24);requestAnimationFrame(frame);}
                            else {pointer('pointermove',10);pointer('pointerup',10);
                              pointer('pointerdown',10);pointer('pointermove',0);pointer('pointerup',0);resolve(intervals);}}
                          requestAnimationFrame(frame);
                        })''')
                        if round_index<len(args.qualities):continue
                        metrics=canvas.evaluate('''canvas=>Object.fromEntries([
                          'data-color-triangles','data-brick-shadow-triangles-estimate','data-color-instances',
                          'data-detail-scale','data-lod-counts','data-shadow-lod-counts',
                          'data-color-budget-estimate-exceeded','data-shadow-draw-calls',
                          'data-instance-upload-bytes'].map(name=>[name,canvas.getAttribute(name)]))''')
                        frames.sort();metrics.update(quality=quality,presentation_samples=len(frames),
                            median_presentation_ms=frames[len(frames)//2] if frames else None,
                            p95_presentation_ms=frames[min(len(frames)-1,int(len(frames)*.95))] if frames else None)
                        results.append(metrics)
                        page.screenshot(path=str(args.output.with_name(args.output.stem+'-'+quality+'.png')),timeout=60000)
                    assert not errors,errors
                    driver=canvas.evaluate("canvas=>{const gl=canvas.getContext('webgl2'),ext=gl.getExtension('WEBGL_debug_renderer_info');return {renderer:ext?gl.getParameter(ext.UNMASKED_RENDERER_WEBGL):gl.getParameter(gl.RENDERER),error:gl.getError()};}")
                    assert driver['error']==0,driver
                    args.output.write_text(json.dumps({'construction':str(args.construction),'backend':'actual headless Chrome WebGL2, read-only fixture transport; hardware dependent; no world publication','driver':driver,'motion':'One complete warm-up round; UI pointer orbit, then 2s warm-up and 4s presentation sampling per mode','results':results,'page_errors':errors},indent=2)+'\n')
                    print(json.dumps({'output':str(args.output),'results':results}))
                finally:browser.close()
        finally:server.terminate();server.wait(timeout=15)

if __name__=='__main__':main()
