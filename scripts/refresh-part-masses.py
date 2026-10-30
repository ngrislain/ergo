#!/usr/bin/env python3
"""Print current catalog masses for review; never silently overwrite calibration data."""
import concurrent.futures
import html.parser
import json
import re
import urllib.request
from pathlib import Path

PARTS = [value['catalogue'] for value in json.loads((Path(__file__).resolve().parents[1]/'crates/world/library/sources.lock.json').read_text())['parts'].values() if value['catalogue']['size'] is not None]


class Text(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.chunks = []

    def handle_data(self, data):
        self.chunks.append(data)


def fetch(part):
    url = part['mass_source']
    request = urllib.request.Request(url, headers={"User-Agent": "Ergo catalog data review/0.1"})
    with urllib.request.urlopen(request, timeout=30) as response:
        parser = Text()
        parser.feed(response.read().decode("utf-8", errors="replace"))
    match = re.search(r"Weight:\s*([0-9.]+)\s*g", " ".join(parser.chunks))
    return {"part": part['id'], "mass_g": float(match[1]) if match else None, "source": url}


if __name__ == "__main__":
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as executor:
        print(json.dumps(list(executor.map(fetch, PARTS)), indent=2))
