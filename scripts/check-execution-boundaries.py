#!/usr/bin/env python3
"""Ensure isolated server/bridge builds do not pull in the dynamics engine."""
import subprocess

for package in ("ergo-server", "ergo-mcp"):
    tree = subprocess.check_output(["cargo", "tree", "-p", package, "--prefix", "none", "--format", "{p}"], text=True)
    assert "rapier3d" not in tree, f"{package} unexpectedly enables client simulation"
    print(f"{package}: no native Rapier dependency")
