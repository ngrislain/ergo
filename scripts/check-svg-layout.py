#!/usr/bin/env python3
"""Render the architecture SVG and check text fits its smallest containing panel."""
import pathlib
from playwright.sync_api import sync_playwright

root = pathlib.Path(__file__).resolve().parents[1]
with sync_playwright() as p:
    browser = p.chromium.launch(channel="chrome", headless=True)
    page = browser.new_page(viewport={"width": 1200, "height": 900})
    page.set_content((root / "docs/figures/architecture.svg").read_text())
    violations = page.evaluate("""() => {
      const rectangles = [...document.querySelectorAll('svg rect')].map(e => ({
        x: +e.getAttribute('x') || 0, y: +e.getAttribute('y') || 0,
        w: +e.getAttribute('width'), h: +e.getAttribute('height')
      }));
      return [...document.querySelectorAll('svg text')].flatMap(e => {
        const x = +e.getAttribute('x'), y = +e.getAttribute('y'), b = e.getBBox();
        const panel = rectangles.filter(r => x >= r.x && x <= r.x+r.w && y >= r.y && y <= r.y+r.h)
          .sort((a,b) => a.w*a.h-b.w*b.h)[0];
        return panel && (b.x < panel.x+6 || b.x+b.width > panel.x+panel.w-6)
          ? [{text:e.textContent, width:b.width, panel}] : [];
      });
    }""")
    artifacts = root / ".playwright-mcp"
    artifacts.mkdir(exist_ok=True)
    page.screenshot(path=str(artifacts / "architecture.png"), full_page=True)
    browser.close()
    assert not violations, violations
print("Architecture SVG rendered; panel text fits.")
