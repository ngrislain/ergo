#!/usr/bin/env python3
"""Print current catalog masses for review; never silently overwrite calibration data."""
import concurrent.futures
import html.parser
import json
import re
import urllib.request

PARTS = "3005 3004 3010 3009 3008 3003 3002 3001 2456 3007 3024 3023 3710 3666 3022 3021 3020 3795 3034 3031 3032 3035 41539 3070b 3069b 3068b 2431".split()


class Text(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.chunks = []

    def handle_data(self, data):
        self.chunks.append(data)


def fetch(part):
    catalog_part = part[:-1] if part in {"3070b", "3069b", "3068b"} else part
    url = f"https://www.bricklink.com/v2/catalog/catalogitem.page?P={catalog_part}"
    request = urllib.request.Request(url, headers={"User-Agent": "Ergo catalog data review/0.1"})
    with urllib.request.urlopen(request, timeout=30) as response:
        parser = Text()
        parser.feed(response.read().decode("utf-8", errors="replace"))
    match = re.search(r"Weight:\s*([0-9.]+)\s*g", " ".join(parser.chunks))
    return {"part": f"{part}.dat", "mass_g": float(match[1]) if match else None, "source": url}


if __name__ == "__main__":
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as executor:
        print(json.dumps(list(executor.map(fetch, PARTS)), indent=2))
