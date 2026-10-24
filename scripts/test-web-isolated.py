#!/usr/bin/env python3
"""Run the browser smoke against a disposable, network-disabled development store."""
import argparse
import os
import pathlib
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--server-binary", default=str(ROOT / "target/debug/ergo-server"))
parser.add_argument("--artifacts", default=str(ROOT / ".playwright-mcp"))
parser.add_argument("--script", default=str(ROOT / "scripts/test-web.py"))
args = parser.parse_args()
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
with tempfile.TemporaryDirectory(prefix="ergo-isolated-web-") as temporary:
    directory = pathlib.Path(temporary)
    (directory / "web").symlink_to(ROOT / "web", target_is_directory=True)
    environment = {k:v for k,v in os.environ.items() if not k.startswith(("ERGO_","GOOGLE_","S3_"))}
    environment.update(ERGO_BIND=f"127.0.0.1:{port}", ERGO_ASSET_FETCH="0", ERGO_GROUND_SOURCE="disabled",ERGO_LEGACY_FIXTURES="1")
    with tempfile.TemporaryFile(mode="w+t") as log:
        server = subprocess.Popen([str(pathlib.Path(args.server_binary).resolve())], cwd=directory,
                                  env=environment, stdout=log, stderr=log)
        try:
            for _ in range(100):
                if server.poll() is not None:
                    log.seek(0)
                    raise RuntimeError(log.read())
                try:
                    urllib.request.urlopen(f"http://127.0.0.1:{port}/api/config", timeout=1).close()
                    break
                except OSError:
                    time.sleep(0.1)
            else:
                raise RuntimeError("Isolated web server did not start")
            subprocess.run([sys.executable,str(pathlib.Path(args.script).resolve()),"--url",f"http://127.0.0.1:{port}","--artifacts",args.artifacts], cwd=ROOT, check=True)
        finally:
            server.send_signal(signal.SIGTERM)
            server.wait(timeout=10)
            assert server.returncode == 0, "Server did not shut down cleanly on SIGTERM"
