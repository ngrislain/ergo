#!/usr/bin/env python3
"""Real MCP + browser integration for the one public, attributed shared sandbox."""
import importlib.util
import json
import os
import pathlib
import re
import signal
import socket
import subprocess
import tempfile
import time
from playwright.sync_api import sync_playwright, expect

ROOT=pathlib.Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location("publish_test",ROOT/"scripts/test-mcp-publish.py")
assert spec is not None and spec.loader is not None
helpers=importlib.util.module_from_spec(spec)
spec.loader.exec_module(helpers)
with socket.socket() as sock:
    sock.bind(("127.0.0.1",0));port=sock.getsockname()[1]
url=f"http://127.0.0.1:{port}"
env={k:v for k,v in os.environ.items() if not k.startswith(("ERGO_","GOOGLE_","S3_","AWS_"))}
env.update(ERGO_ASSET_FETCH="0",ERGO_GROUND_SOURCE="disabled",ERGO_ISOLATED_FIXTURES="1")
with tempfile.TemporaryDirectory(prefix="ergo-common-") as directory:
    directory=pathlib.Path(directory)
    (directory/"web").symlink_to(ROOT/"web",target_is_directory=True)
    (directory/"store/users").mkdir(parents=True)
    for actor in ["alice","bob"]:
        (directory/f"store/users/{actor}.json").write_text(json.dumps({"id":actor,"name":actor.title()+" Builder",
            "email":actor+"@example.test","picture":None,"created_at":1,"last_login":1}))
    server_env={**env,"ERGO_BIND":f"127.0.0.1:{port}","ERGO_PUBLIC_URL":url,"ERGO_DATA_DIR":str(directory/"store"),
        "GOOGLE_CLIENT_ID":"test","GOOGLE_CLIENT_SECRET":"test","ERGO_SESSION_SECRET":helpers.SECRET.decode()}
    log=tempfile.TemporaryFile(mode="w+t")
    processes=[]
    def start():
        server=subprocess.Popen([str(ROOT/"target/debug/ergo-server")],cwd=directory,env=server_env,stdout=log,stderr=log)
        for _ in range(100):
            if server.poll() is not None:
                log.seek(0);raise AssertionError(log.read())
            try:
                if helpers.http(url+"/api/config")[0]==200:return server
            except OSError:time.sleep(.1)
        raise AssertionError("Server did not start")
    server=start()
    try:
        status,head=helpers.http(url+"/api/shared-worlds",session=helpers.signed_session("alice"),
            body={"name":"Ergo Sandbox","geo":{"latitude":48.8566,"longitude":2.3522,"arcseconds_per_block":1},"authenticated_editing":True})
        assert status==200,head
        # A completed HTTP response can precede asynchronous snapshot-pin cleanup.
        # Quiesce the disposable source before stopping its runtime for migration.
        for _ in range(100):
            settled=subprocess.run([str(ROOT/'target/debug/ergo-server'),'shared-maintenance-status',head['id']],
                cwd=directory,env=server_env,capture_output=True,text=True,check=True)
            if not json.loads(settled.stdout)['pins']:break
            time.sleep(.02)
        else:raise AssertionError('Source readers did not quiesce before migration')
        server.terminate();server.wait(timeout=10)
        migration=subprocess.run([str(ROOT/"target/debug/ergo-server"),"shared-migrate",head["id"],"world"],
            cwd=directory,env=server_env,capture_output=True,text=True)
        assert migration.returncode==0,migration.stdout+ migration.stderr
        server_env["ERGO_COMMON_WORLD_ID"]=head["id"]
        server_env["ERGO_WORLD_PREFIX"]="world"
        server=start()
        assert helpers.http(url+"/api/shared-worlds")[0]==200,"The one sandbox must be publicly readable"
        assert helpers.http(url+"/api/shared-worlds/allowance")[0]==401,"Account ledgers are not public sandbox geometry"
        tokens={actor:helpers.http(url+"/api/auth/mcp-token",session=helpers.signed_session(actor),body={"expires_in_seconds":600})[1]["token"] for actor in ["alice","bob"]}
        mcp=helpers.Mcp(ROOT/"target/debug/ergo-mcp",directory/"alice.ergo.json",url,tokens["alice"],directory,env)
        print("Connected common-world MCP; testing immediate commits",flush=True)
        processes.append(mcp)
        schemas={tool["name"]:tool for tool in mcp.rpc("tools/list")["tools"]}
        assert "world_id" not in schemas["publish_construction"]["inputSchema"]["properties"]
        assert "authenticated_editing" not in schemas["publish_construction"]["inputSchema"]["properties"]
        assert "largest suitable" in mcp.call("get_catalog")["structuredContent"]["building_guidance"]
        first=mcp.call("place_brick",part="3003",color=4,min_corner=[8,8,0])
        assert not first.get("isError"),first
        receipt=first["structuredContent"]["shared_commit"]
        assert receipt["published"] and receipt["world_id"]==head["id"]
        assert receipt["audit"]["actor_name"]=="Alice Builder"
        assembly=receipt["assembly_id"];birth=receipt["pieces"]["1"]
        second=mcp.call("place_brick",part="3003",color=1,min_corner=[12,8,0])
        receipt=second["structuredContent"]["shared_commit"]
        assert receipt["assembly_id"]==assembly and receipt["pieces"]["1"]==birth
        revision=receipt["accepted_revision"]
        assert mcp.call("place_brick",part="3003",color=4,min_corner=[8,8,0])["isError"]
        assert mcp.call("place_brick",part="3003",color=4,min_corner=[20,8,10])["isError"],"Floating edits must roll back instead of becoming shared"
        assert helpers.http(url+f"/api/shared-worlds/{head['id']}")[1]["revision"]==revision
        assert mcp.call("list_bricks")["structuredContent"]["total"]==2
        group=mcp.call("create_group",name="Pair",piece_ids=[1,2])["structuredContent"]["group_id"]
        moved=mcp.call("transform_group",group_id=group,translation=[0,4,0])
        assert moved["structuredContent"]["shared_commit"]["published"]
        bob=helpers.Mcp(ROOT/"target/debug/ergo-mcp",directory/"bob.ergo.json",url,tokens["bob"],directory,env)
        processes.append(bob)
        opened=bob.call("open_shared_construction",position_world=[10,12,0])
        assert opened["structuredContent"]["assembly_id"]==assembly,opened
        assert bob.call("list_bricks")["structuredContent"]["total"]==2,"Opening shared state must restore the existing browser runtime"
        added=bob.call("place_brick",part="3003",color=14,min_corner=[30,30,0])
        assert added["structuredContent"]["shared_commit"]["audit"]["actor_name"]=="Bob Builder"
        assert mcp.call("transform_group",group_id=group,translation=[1,0,0])["isError"],"A stale assembly replacement must not erase another user's edit"
        refreshed=mcp.call("open_shared_construction",assembly_id=assembly)
        assert refreshed["structuredContent"]["brick_count"]==3
        assert mcp.call("list_bricks")["structuredContent"]["total"]==3
        neighbor=helpers.Mcp(ROOT/"target/debug/ergo-mcp",directory/"neighbor.ergo.json",url,tokens["alice"],directory,env)
        processes.append(neighbor)
        assert neighbor.call("place_brick",part="3003",color=2,min_corner=[70,70,0])["structuredContent"]["shared_commit"]["published"]
        status,location=helpers.http(url+f"/api/shared-worlds/{head['id']}/assemblies/{assembly}/location")
        assert status==200 and location["assembly_id"]==assembly,location
        assert location["position_world"]==[9,13,1],location
        assert "scene" not in location,"Navigation must use a bounded hint, not an isolated authoring export"
        status,redirected=helpers.http(url+"/api/shared-worlds",token=tokens["alice"],body={"name":"No private fork","geo":head["metadata"]["geo"]})
        assert status==409 and "World creation is disabled" in redirected["error"]
        with sync_playwright() as p:
            print("MCP shared commits/conflicts passed; checking public browser and handoff",flush=True)
            browser=p.chromium.launch(channel="chrome",headless=True,args=["--enable-unsafe-swiftshader"])
            guest=browser.new_page(viewport={"width":1440,"height":950})
            guest.goto(url)
            expect(guest.locator("h1")).to_have_text("Ergo Sandbox",timeout=30000)
            expect(guest.get_by_role("link",name="Sign in with Google")).to_be_visible()
            guest.locator(f'[data-assembly-id="{assembly}"]').click(timeout=30000)
            expect(guest.locator(".status-message")).to_contain_text("Shared-world region loaded",timeout=30000)
            expect(guest.locator("h1")).to_have_text("Ergo Sandbox")
            expect(guest.locator(".status-bar")).to_contain_text("4 bricks")
            selected=guest.locator(f'[data-assembly-id="{assembly}"]')
            expect(selected.locator('.construction-author')).to_have_text('Alice Builder')
            expect(selected.locator('.construction-date')).to_have_text(re.compile(r'\d{4}-\d{2}-\d{2}'))
            expect(selected.locator('.construction-size')).to_have_text('3')
            for key in ['name','author','date','size']:
                guest.get_by_label('Sort constructions',exact=True).select_option(key)
                expect(guest.get_by_label('Sort constructions',exact=True)).to_have_value(key)
            sizes=[int(s.replace(',','')) for s in guest.locator('.construction-size').all_text_contents()]
            assert sizes==sorted(sizes,reverse=True)
            with guest.expect_download() as captured:
                guest.get_by_role('button',name='Download construction',exact=True).click()
            download=captured.value;text=pathlib.Path(download.path()).read_text()
            assert download.suggested_filename.endswith('.mpd') and text.startswith('0 FILE construction.ldr')
            assert sum(line.startswith('1 ') for line in text.splitlines())==3
            assert '0 FILE 3003.dat' in text
            guest.get_by_label('Construction export format',exact=True).select_option('ergo')
            with guest.expect_download() as captured:
                guest.get_by_role('button',name='Download construction',exact=True).click()
            native=json.loads(pathlib.Path(captured.value.path()).read_text())
            assert native['format']=='ergo-construction-1' and len(native['pieces'])==3
            short_list_height=guest.locator(".construction-list").evaluate("el=>el.clientHeight")
            assert guest.locator(".world-coordinates").bounding_box()['y']<guest.locator(".browser").bounding_box()['y']
            guest.close()
            context=browser.new_context(viewport={"width":1440,"height":950})
            context.add_cookies([{"name":"ergo_session","value":helpers.signed_session("alice"),"url":url,"httpOnly":True}])
            page=context.new_page()
            # Exercise scrolling pagination without creating fake accepted objects.
            actual=helpers.http(url+f"/api/shared-worlds/{head['id']}/assemblies?limit=32")[1]
            copies=[{"id":f"{1000+i:064x}","name":f"Navigation fixture {i}"} for i in range(40)]
            def list_page(route):
                following="after=" in route.request.url
                value={"items":copies[30:] if following else actual["items"]+copies[:30],
                       "next":None if following else "7/0","revision":actual["revision"]}
                route.fulfill(status=200,content_type="application/json",body=json.dumps(value))
            page.route("**/assemblies?*",list_page)
            requested=[];page.on("request",lambda request:requested.append(request.url))
            page.goto(url)
            expect(page.locator(".construction-list .world-item")).to_have_count(32,timeout=30000)
            panel=page.locator(".construction-list")
            assert panel.evaluate("el=>el.scrollHeight>el.clientHeight && getComputedStyle(el).overflowY==='auto'")
            assert panel.evaluate("el=>el.clientHeight")==short_list_height,"List height must not grow with its item count"
            panel.evaluate("el=>el.scrollTop=el.scrollHeight")
            expect(page.locator(".construction-list .world-item")).to_have_count(42,timeout=15000)
            page.get_by_label('Sort constructions',exact=True).select_option('name')
            names=page.locator('.construction-list .world-item strong').all_text_contents()
            assert names==sorted(names,key=str.lower)
            assert panel.evaluate("el=>el.clientHeight")==short_list_height,"Pagination must stay inside the fixed-height zone"
            panel.evaluate("el=>el.scrollTop=0")
            page.locator(f'[data-assembly-id="{assembly}"]').click()
            expect(page.locator(".status-message")).to_contain_text("Shared-world region loaded",timeout=30000)
            expect(page.locator("h1")).to_have_text("Ergo Sandbox")
            expect(page.locator(".status-bar")).to_contain_text("4 bricks")
            assert any(request.endswith("/location") for request in requested)
            assert not any("/workspace" in request for request in requested),"Browsing must not open an isolated editable scene"
            gps=page.locator(".camera-gps strong")
            expect(gps).to_have_attribute("data-latitude",re.compile(r".+"))
            origin=page.locator(".reference-origin strong").inner_text()
            before=(gps.get_attribute("data-latitude"),gps.get_attribute("data-longitude"))
            page.get_by_role("button",name="Walk",exact=True).click()
            canvas=page.locator("#world-canvas");canvas.focus()
            page.keyboard.down("ArrowUp")
            page.wait_for_function("old=>{const e=document.querySelector('.camera-gps strong');return e.dataset.latitude!==old[0]||e.dataset.longitude!==old[1]}",arg=before)
            page.keyboard.up("ArrowUp")
            assert page.locator(".reference-origin strong").inner_text()==origin,"The coordinate reference must not follow the camera"
            page.wait_for_function("()=>{const p=JSON.parse(document.querySelector('#world-canvas').dataset.cameraPosition);const e=document.querySelector('.camera-gps strong');return Math.abs(Number(e.dataset.latitude)-(48.8566+p[1]/(3600*1024)))<1e-10&&Math.abs(Number(e.dataset.longitude)-(2.3522+p[0]/(3600*1024)))<1e-10}")
            page.evaluate("dispatchEvent(new PageTransitionEvent('pagehide'))")
            for _ in range(100):
                status,saved=helpers.http(url+"/api/user-position",token=tokens["alice"])
                if status==200 and saved["position"] and saved["position"]["context"]["navigation"]=="walk":break
                time.sleep(.05)
            else:raise AssertionError("Signed-in camera position was not saved")
            position=mcp.call("get_user_position")["structuredContent"]
            assert position["position"]["context"]["world_id"]==head["id"]
            assert position["ground_position_world"][2]==0 and position["model_transform"][2][3]==0
            assert bob.call("get_user_position")["structuredContent"]["position"] is None,"Other users' positions must remain private"
            page.get_by_role("button",name="Edit here with an agent").click()
            page.wait_for_url(re.compile(r"/login\?x=.*#connect"))
            configuration=json.loads(page.locator(".connection-code").first.inner_text())
            assert configuration["mcpServers"]["ergo"]["env"]["ERGO_SHARED_ASSEMBLY_ID"]==assembly
            assert "ERGO_POSITION_WORLD" in configuration["mcpServers"]["ergo"]["env"]
            browser.close()
        opened=mcp.call("open_shared_construction",assembly_id=assembly)["structuredContent"]
        renamed=mcp.call("publish_construction",name="Shared façade",expected_local_revision=opened["local_revision"])
        assert renamed["structuredContent"]["published"],renamed
        assert renamed["structuredContent"]["pieces"]["1"]==birth
        assert helpers.http(url+f"/api/shared-worlds/{head['id']}")[0]==200
    finally:
        for process in processes:process.close()
        server.send_signal(signal.SIGTERM);server.wait(timeout=10)
        log.close()
print("Common-world integration passed: public single sandbox, attributed MCP commits, stable IDs, invalid/conflict rollback, scrolling navigation at actual positions with neighbors, live camera GPS and context handoff.")
