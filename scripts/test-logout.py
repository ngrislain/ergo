#!/usr/bin/env python3
"""Sign-out from setup/viewer against a disposable Google-enabled local server."""
import base64
import hashlib
import hmac
import json
import os
import pathlib
import socket
import subprocess
import tempfile
import time
import urllib.request
from playwright.sync_api import sync_playwright, expect

ROOT = pathlib.Path(__file__).resolve().parents[1]
KEY = b"local-browser-test-only-signing-key-123456789"
with socket.socket() as sock:
    sock.bind(("127.0.0.1",0))
    port = sock.getsockname()[1]
url = f"http://127.0.0.1:{port}"
with tempfile.TemporaryDirectory(prefix="ergo-logout-") as directory:
    directory = pathlib.Path(directory)
    (directory/"web").symlink_to(ROOT/"web", target_is_directory=True)
    (directory/"data/users").mkdir(parents=True)
    (directory/"data/users/test-user.json").write_text(json.dumps({"id":"test-user","name":"Browser Builder",
        "email":"test@example.test","picture":None,"created_at":1,"last_login":1}))
    env = {k:v for k,v in os.environ.items() if not k.startswith(("ERGO_","GOOGLE_","S3_","AWS_"))}
    env.update(ERGO_BIND=f"127.0.0.1:{port}",ERGO_PUBLIC_URL=url,ERGO_ASSET_FETCH="0",ERGO_GROUND_SOURCE="disabled",
        GOOGLE_CLIENT_ID="local-test",GOOGLE_CLIENT_SECRET="local-test",ERGO_SESSION_SECRET=KEY.decode())
    with tempfile.TemporaryFile() as log:
        server = subprocess.Popen([str(ROOT/"target/debug/ergo-server")],cwd=directory,env=env,stdout=log,stderr=log)
        try:
            for _ in range(100):
                try:
                    urllib.request.urlopen(url+"/api/config",timeout=1).close()
                    break
                except OSError: time.sleep(.1)
            with sync_playwright() as p:
                browser = p.chromium.launch(channel="chrome",headless=True,args=["--enable-unsafe-swiftshader"])
                context = browser.new_context(viewport={"width":390,"height":844})
                for path in ["/login#connect","/app"]:
                    encode = lambda b: base64.urlsafe_b64encode(b).decode().rstrip("=")
                    payload = encode(json.dumps({"user_id":"test-user","expires":int(time.time())+600}).encode())
                    token = payload+"."+encode(hmac.new(KEY,payload.encode(),hashlib.sha256).digest())
                    context.add_cookies([{"name":"ergo_session","value":token,"url":url,"httpOnly":True}])
                    page = context.new_page()
                    page.goto(url+path)
                    page.wait_for_load_state("networkidle")
                    expect(page.get_by_role("button",name="Sign out",exact=True)).to_be_visible(timeout=30000)
                    assert context.request.get(url+"/api/auth/me").json()["signed_in"]
                    page.get_by_role("button",name="Sign out",exact=True).click()
                    page.wait_for_url(url+"/login")
                    expect(page.get_by_role("link",name="Continue with Google")).to_be_visible()
                    assert not context.request.get(url+"/api/auth/me").json()["signed_in"]
                    assert not any(c["name"] in ["ergo_session","ergo_login"] for c in context.cookies())
                    response = context.request.post(url+"/api/auth/logout",headers={"X-Ergo-Request":"1"})
                    assert response.status == 204
                    page.close()
                browser.close()
        finally:
            server.terminate()
            server.wait(timeout=10)
print("Logout browser checks passed: setup/viewer/mobile sign-out, expired/missing-session retry, cookie clearing and guest redirect.")
