#!/usr/bin/env python3
"""Real MCP/WASM build → authenticated HTTP publication → persistent read-back.

Uses a disposable local object store and synthetic signed test sessions. No live
Google/S3 credentials or production connector measurements are needed.
"""
import argparse
import base64
import hashlib
import hmac
import json
import os
import pathlib
import select
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


SECRET = b"synthetic-mcp-publish-test-session-secret-only"


def signed_session(actor):
    encode = lambda value: base64.urlsafe_b64encode(value).rstrip(b"=")
    payload = encode(json.dumps({"user_id": actor, "expires": int(time.time()) + 3600}).encode())
    signature = encode(hmac.new(SECRET, payload, hashlib.sha256).digest())
    return b".".join([payload, signature]).decode()


def http(url, *, body=None, token=None, session=None):
    headers = {"X-Ergo-Request": "1"}
    if token:
        headers["Authorization"] = "Bearer " + token
    if session:
        headers["Cookie"] = "ergo_session=" + session
    if body is not None:
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(url, headers=headers,
                                     data=None if body is None else json.dumps(body).encode())
    try:
        response = urllib.request.urlopen(request, timeout=30)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status, json.load(response)


class Mcp:
    def __init__(self, binary, workspace, url, token, directory, environment):
        token_file = directory / "delegation-token"
        token_file.write_text(token)
        token_file.chmod(0o600)
        self.process = subprocess.Popen(
            [str(binary), "--workspace", str(workspace), "--headless", "--publish-url", url,
             "--publish-token-file", str(token_file)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, cwd=directory, env=environment)
        self.sequence = 0
        self.rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                               "clientInfo": {"name": "ergo-publication-e2e", "version": "1"}})

    def rpc(self, method, params=None):
        assert self.process.stdin is not None and self.process.stdout is not None and self.process.stderr is not None
        self.sequence += 1
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.sequence,
                                             "method": method, "params": params or {}}) + "\n")
        self.process.stdin.flush()
        assert select.select([self.process.stdout], [], [], 135)[0], "MCP tool timed out"
        line = self.process.stdout.readline()
        assert line, self.process.stderr.read()
        response = json.loads(line)
        assert response["id"] == self.sequence and "error" not in response, response
        return response["result"]

    def call(self, tool, **arguments):
        return self.rpc("tools/call", {"name": tool, "arguments": arguments})

    def close(self):
        assert self.process.stdin is not None and self.process.stderr is not None
        self.process.stdin.close()
        self.process.wait(timeout=10)
        assert self.process.returncode == 0, self.process.stderr.read()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--mcp-binary", default="target/debug/ergo-mcp")
    parser.add_argument("--server-binary", default="target/debug/ergo-server")
    args = parser.parse_args()
    mcp_binary = pathlib.Path(args.mcp_binary).resolve()
    server_binary = pathlib.Path(args.server_binary).resolve()
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    url = f"http://127.0.0.1:{port}"
    environment = {k: v for k, v in os.environ.items()
                   if not k.startswith(("ERGO_", "GOOGLE_", "S3_"))}
    environment.update(ERGO_ASSET_FETCH="0", ERGO_GROUND_SOURCE="disabled",ERGO_LEGACY_FIXTURES="1")
    with tempfile.TemporaryDirectory(prefix="ergo-publish-test-") as temporary:
        directory = pathlib.Path(temporary)
        server_environment = {**environment, "ERGO_BIND": f"127.0.0.1:{port}",
                              "ERGO_PUBLIC_URL": url, "ERGO_DATA_DIR": str(directory / "store"),
                              "GOOGLE_CLIENT_ID": "synthetic-test-client",
                              "GOOGLE_CLIENT_SECRET": "synthetic-test-secret",
                              "ERGO_SESSION_SECRET": SECRET.decode(),
                              "ERGO_EDIT_BRICKS_PER_PERIOD": "2"}
        server = None
        mcp = None
        log = tempfile.TemporaryFile(mode="w+t")

        def start_server():
            process = subprocess.Popen([str(server_binary)], env=server_environment,
                                       cwd=directory, stdout=log, stderr=log)
            for _ in range(100):
                if process.poll() is not None:
                    log.seek(0)
                    raise AssertionError(log.read())
                try:
                    if http(url + "/api/config")[0] == 200:
                        return process
                except OSError:
                    time.sleep(0.1)
            raise AssertionError("Publication test server did not start")

        def stop_server(process):
            process.send_signal(signal.SIGTERM)
            process.wait(timeout=10)
            assert process.returncode == 0, "Server must drain requests and shut down cleanly on SIGTERM"

        try:
            server = start_server()
            assert http(url + "/api/auth/mcp-token", body={})[0] != 200
            tokens = {}
            for actor in ("alice", "bob", "charlie"):
                status, grant = http(url + "/api/auth/mcp-token", body={"expires_in_seconds": 600},
                                     session=signed_session(actor))
                assert status == 200, grant
                assert grant["audience"] == url
                tokens[actor] = grant["token"]
            workspace = directory / "construction.ergo.json"
            mcp = Mcp(mcp_binary, workspace, url, tokens["alice"], directory, environment)
            assert "publish_construction" in {t["name"] for t in mcp.rpc("tools/list")["tools"]}
            assert mcp.call("get_edit_allowance")["structuredContent"]["capacity"] == 2
            placed = mcp.call("place_brick", part="3003", color=378, min_corner=[0, 0, 0])
            assert not placed["isError"]
            assert placed["structuredContent"]["global_commit_available"] is True
            assert json.loads(placed["content"][-1]["text"])["global_commit_available"] is True
            published = mcp.call("publish_construction", name="Agent publication", authenticated_editing=False)
            assert not published["isError"], published
            receipt = published["structuredContent"]
            assert receipt["published"] and receipt["publication_state"] == "confirmed", receipt
            assert receipt["audit"]["actor"] == "alice"
            assert receipt["audit"]["validation"]["checked_bricks"] == 1
            world = receipt["world_id"]
            world_url = url + "/api/shared-worlds/" + world
            first_piece = receipt["pieces"]["1"]
            status, remote = http(world_url + "/pieces/" + first_piece +
                                  "?revision=" + receipt["accepted_revision"], token=tokens["alice"])
            assert status == 200 and remote["geometry"]["color"] == 378, remote
            assert "/app?shared_world=" in receipt["view_url"]
            assert http(receipt["assembly_url"], token=tokens["alice"])[0] == 200
            assert http(receipt["assembly_url"])[0] == 401
            assert http(receipt["assembly_url"], token=tokens["bob"])[0] == 403

            # An existing-world publication adds a distinct assembly, not an in-place update.
            assert not mcp.call("move_brick", id=1, delta=[10, 0, 0])["isError"]
            second = mcp.call("publish_construction", world_id=world,
                              base_revision=receipt["accepted_revision"])
            assert not second["isError"], second
            second = second["structuredContent"]
            assert second["assembly_id"] != receipt["assembly_id"]
            assert second["accepted_revision"] != receipt["accepted_revision"]
            accepted = second["accepted_revision"]

            # Floating geometry and uncalibrated stacked connections cannot change the head.
            geo = http(world_url, token=tokens["alice"])[1]["metadata"]["geo"]
            status, validation_world = http(url + "/api/shared-worlds", token=tokens["alice"],
                                             body={"name": "Validation rejection test", "geo": geo})
            assert status == 200, validation_world
            validation_url = url + "/api/shared-worlds/" + validation_world["id"]
            for corner in ([30, 0, 8], [10, 0, 1]):
                assert not mcp.call("place_brick", part="3003", color=4,
                                    min_corner=corner)["isError"]
                rejected = mcp.call("publish_construction", world_id=validation_world["id"])
                assert rejected["isError"] and rejected["structuredContent"]["http_status"] == 422, rejected
                assert rejected["structuredContent"]["publication_state"] == "rejected"
                reason = rejected["structuredContent"]["server_response"]["error"]
                assert ("ground support" in reason if corner[2] == 8 else "measured" in reason), reason
                assert http(validation_url, token=tokens["alice"])[1]["revision"] == validation_world["revision"]
                assert http(world_url, token=tokens["alice"])[1]["revision"] == accepted
                assert not mcp.call("remove_bricks", ids=[2])["isError"]
            mcp.close()
            mcp = None

            for token, expected in [(tokens["bob"], 403), ("tampered-token", 401)]:
                mcp = Mcp(mcp_binary, workspace, url, token, directory, environment)
                denied = mcp.call("publish_construction", world_id=world)
                assert denied["isError"] and denied["structuredContent"]["http_status"] == expected, denied
                mcp.close()
                mcp = None

            # MCP-created worlds default to authenticated collaboration. Another
            # signed-in actor can edit, is paced, and can receive a paid entitlement.
            mcp = Mcp(mcp_binary, workspace, url, tokens["bob"], directory, environment)
            collaboration = mcp.call("publish_construction", name="Shared collaboration")
            assert not collaboration["isError"], collaboration
            collaboration = collaboration["structuredContent"]
            mcp.close()
            mcp = None
            shared_url = url + "/api/shared-worlds/" + collaboration["world_id"]
            assert http(shared_url, token=tokens["charlie"])[1]["policy"]["authenticated_editing"] is True
            piece_id = collaboration["pieces"]["1"]
            def next_edit():
                head = http(shared_url, token=tokens["charlie"])[1]
                geometry = http(shared_url + "/pieces/" + piece_id, token=tokens["charlie"])[1]["geometry"]
                geometry["position"][0] += 1
                return {"base": head["revision"], "pieces": [{"id": piece_id, "geometry": geometry, "mesh": None}]}
            for _ in range(2):
                status, result = http(shared_url + "/commits", body=next_edit(), token=tokens["charlie"])
                assert status == 200, result
            status, limited = http(shared_url + "/commits", body=next_edit(), token=tokens["charlie"])
            assert status == 429 and limited["retry_after_seconds"] > 0, limited
            subprocess.run([str(server_binary), "user-plan", "charlie", "paid"], cwd=directory,
                           env=server_environment, check=True, stdout=subprocess.DEVNULL)
            status, paid_commit = http(shared_url + "/commits", body=next_edit(), token=tokens["charlie"])
            assert status == 200, paid_commit
            assert http(url + "/api/shared-worlds/allowance", token=tokens["charlie"])[1]["unlimited"] is True

            # Read back after a process restart, proving publication persisted remotely.
            stop_server(server)
            server = None
            server = start_server()
            assert http(second["assembly_url"], token=tokens["alice"])[0] == 200
            assert http(receipt["assembly_url"], token=tokens["alice"])[0] == 200
            assert http(world_url, token=tokens["alice"])[1]["revision"] == accepted
            assert http(shared_url, token=tokens["charlie"])[1]["revision"] == paid_commit["revision"]
            assert http(url + "/api/shared-worlds/allowance", token=tokens["charlie"])[1]["unlimited"] is True
            print("MCP publication passed: real WASM build, signed user delegation, new/existing-world "
                  "publication, accepted-revision geometry read-back, restart persistence, and "
                  "rejection of unauthorized, floating, and uncalibrated constructions; shared editing, "
                  "persistent per-user pacing, and paid-plan bypass.")
        finally:
            if mcp is not None:
                mcp.close()
            if server is not None:
                stop_server(server)
            log.close()


if __name__ == "__main__":
    main()
