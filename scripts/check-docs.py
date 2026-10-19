#!/usr/bin/env python3
"""Check local Markdown links, SVG structure, and documented MCP tool names."""
import pathlib
import re
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parents[1]
failures = []
documents = [ROOT / "README.md", ROOT / "TODO.md", ROOT / "AGENTS.md", *sorted((ROOT / "docs").glob("*.md"))]
for document in documents:
    text = document.read_text()
    for link in re.findall(r"!?\[[^\]]*\]\(([^)]+)\)", text):
        if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", link) or link.startswith("#"):
            continue
        path = link.split("#", 1)[0]
        if not (document.parent / path).exists():
            failures.append(f"{document.relative_to(ROOT)}: missing link {link}")

namespace = {"svg": "http://www.w3.org/2000/svg"}
figures = sorted((ROOT / "docs/figures").glob("*.svg"))
for figure in figures:
    try:
        root = ET.parse(figure).getroot()
        assert root.attrib.get("viewBox"), "missing viewBox"
        assert root.find("svg:title", namespace) is not None, "missing title"
        assert root.find("svg:desc", namespace) is not None, "missing description"
    except (ET.ParseError, AssertionError) as error:
        failures.append(f"{figure.relative_to(ROOT)}: {error}")

source = (ROOT / "crates/world/src/mcp.rs").read_text()
tools = set(re.findall(r'tool\(\s*"([a-z_]+)"', source))
documented = (ROOT / "docs/local-construction-mcp.md").read_text()
for tool in tools:
    if f"`{tool}`" not in documented:
        failures.append(f"Undocumented MCP tool: {tool}")

if failures:
    raise SystemExit("\n".join(failures))
print(f"Documentation checks passed: {len(documents)} Markdown files, {len(figures)} SVG figures, {len(tools)} MCP tools.")
