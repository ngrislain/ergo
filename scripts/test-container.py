#!/usr/bin/env python3
"""Smoke-test the actual AMD64 API/web image using Podman (or Docker)."""
import argparse
import json
import pathlib
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--engine", choices=["podman", "docker"], default="podman")
    parser.add_argument("--image", default="localhost/ergo:v0.3.0")
    parser.add_argument("--browser", action="store_true", help="Also run the full host-side Playwright flow")
    args = parser.parse_args()

    def engine(*arguments):
        return subprocess.check_output([args.engine, *arguments], text=True)

    image = json.loads(engine("image", "inspect", args.image))[0]
    assert image["Os"] == "linux" and image["Architecture"] == "amd64", "Scaleway requires Linux/AMD64"
    assert image["Config"]["User"] == "10001:10001", "The API image must run as its non-root user"
    assert image["Config"]["WorkingDir"] == "/app"
    baked_environment = {entry.split("=", 1)[0] for entry in image["Config"].get("Env", [])}
    assert not {"AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "GOOGLE_CLIENT_SECRET", "ERGO_SESSION_SECRET"} & baked_environment

    name = "ergo-container-smoke-" + uuid.uuid4().hex[:12]
    created = False
    try:
        engine("run", "--detach", "--pull=never", "--name", name, "--platform", "linux/amd64",
               "--publish", "127.0.0.1::8080", "--env", "ERGO_GROUND_SOURCE=disabled",
               "--env", "ERGO_ASSET_FETCH=0", *(["--env","ERGO_ISOLATED_FIXTURES=1"] if args.browser else []), args.image)
        created = True
        address = engine("port", name, "8080/tcp").strip().splitlines()[0]
        url = "http://" + address

        def request(path, body=None, content_type="application/json"):
            headers = {"X-Ergo-Request": "1", "Origin": url}
            if body is not None:
                headers["Content-Type"] = content_type
            return urllib.request.urlopen(urllib.request.Request(url + path, data=body, headers=headers), timeout=30)

        for _ in range(100):
            state = json.loads(engine("inspect", name))[0]["State"]
            if not state["Running"]:
                raise RuntimeError(engine("logs", name))
            try:
                with request("/api/config") as response:
                    configuration = json.load(response)
                break
            except (OSError, urllib.error.URLError):
                time.sleep(0.2)
        else:
            raise RuntimeError("Container did not become ready")
        assert configuration["ground_source"] == "disabled"
        assert configuration["storage"] == "Local object store"

        for path in ["/", "/login", "/app", "/mcp.html"]:
            with request(path) as response:
                assert response.status == 200 and b"Ergo" in response.read()
        for path in ["/pkg/ergo_viewer.js", "/mcp-pkg/ergo_client.js"]:
            with request(path) as response:
                assert response.status == 200 and len(response.read()) > 1000
        for path in ["/pkg/ergo_viewer_bg.wasm", "/mcp-pkg/ergo_client_bg.wasm"]:
            with request(path) as response:
                assert response.read(4) == b"\0asm", path

        body = ('--ergo-test\r\nContent-Disposition: form-data; name="file"; filename="container.ldr"\r\n'
                'Content-Type: text/plain\r\n\r\n0 Container smoke\n'
                '1 4 0 0 0 1 0 0 0 1 0 0 0 1 3003.dat\n\r\n--ergo-test--\r\n').encode()
        with request("/api/import", body, "multipart/form-data; boundary=ergo-test") as response:
            scene = json.load(response)["scene"]
        assert len(scene["bricks"]) == 1
        if configuration.get("common_world"):
            world=configuration["common_world"]
            with request("/api/shared-worlds/"+world) as response:head=json.load(response)
            with request("/api/shared-worlds/"+world+"/assemblies",json.dumps({"base":head["revision"],"scene":scene}).encode()) as response:saved=json.load(response)
            with request("/api/shared-worlds/"+world+"/assemblies/"+saved["assembly"]+"/workspace") as response:
                assert len(json.load(response)["scene"]["bricks"])==1
        else:
            with request("/api/worlds", json.dumps(scene).encode()) as response:
                saved = json.load(response)
            with request("/api/worlds/" + saved["id"] + "/export") as response:
                exported = json.load(response)
                assert exported["format"] == "ergo-construction-1" and len(exported["pieces"]) == 1

        if args.browser:
            root = pathlib.Path(__file__).resolve().parents[1]
            subprocess.run([sys.executable, str(root / "scripts/test-web.py"), "--url", url],
                           cwd=root, check=True)

        engine("stop", "--time", "10", name)
        state = json.loads(engine("inspect", name))[0]["State"]
        assert state["ExitCode"] == 0, state
        print(f"Container smoke passed ({args.engine}): Linux/AMD64, non-root startup, HTML/JS/WASM, "
              "file import/save/read-back, and graceful SIGTERM shutdown.")
    except Exception:
        if created:
            print(engine("logs", name), file=sys.stderr)
        raise
    finally:
        if created:
            engine("rm", "--force", name)


if __name__ == "__main__":
    main()
