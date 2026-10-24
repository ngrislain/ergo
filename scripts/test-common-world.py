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
env.update(ERGO_ASSET_FETCH="0",ERGO_GROUND_SOURCE="disabled",ERGO_LEGACY_FIXTURES="1")
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
        status,redirected=helpers.http(url+"/api/shared-worlds",token=tokens["alice"],body={"name":"No private fork","geo":head["metadata"]["geo"]})
        assert status==409 and "World creation is disabled" in redirected["error"]
        with sync_playwright() as p:
            print("MCP shared commits/conflicts passed; checking public browser and handoff",flush=True)
            browser=p.chromium.launch(channel="chrome",headless=True,args=["--enable-unsafe-swiftshader"])
            guest=browser.new_page(viewport={"width":1440,"height":950})
            guest.goto(url)
            expect(guest.locator("h1")).to_have_text("Ergo Sandbox",timeout=30000)
            expect(guest.get_by_role("link",name="Sign in with Google")).to_be_visible()
            guest.close()
            context=browser.new_context(viewport={"width":1440,"height":950})
            context.add_cookies([{"name":"ergo_session","value":helpers.signed_session("alice"),"url":url,"httpOnly":True}])
            page=context.new_page();page.goto(url)
            page.get_by_role("button",name=re.compile("Agent construction.*Edit shared construction")).click(timeout=30000)
            page.get_by_role("button",name=re.compile("Groups…")).click(timeout=30000)
            page.get_by_label("Selected group").select_option(str(group))
            page.get_by_role("button",name="+X",exact=True).click()
            expect(page.locator(".status-message")).to_contain_text("Committed to the common world in S3",timeout=30000)
            page.keyboard.press("Escape")
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
        cleanup=subprocess.run([str(ROOT/"target/debug/ergo-server"),"shared-cleanup-legacy",head["id"]],
            cwd=directory,env=server_env,capture_output=True,text=True)
        assert cleanup.returncode==0,cleanup.stdout+cleanup.stderr
        assert helpers.http(url+f"/api/shared-worlds/{head['id']}")[0]==200
    finally:
        for process in processes:process.close()
        server.send_signal(signal.SIGTERM);server.wait(timeout=10)
        log.close()
print("Common-world integration passed: public single sandbox, immediate attributed MCP commits, stable IDs, position-based opening, conflict/invalid-edit rollback, live browser commit and context handoff.")
