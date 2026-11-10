#!/usr/bin/env python3
"""Check plugin instructions, tabs, clipboard and small-screen/no-JS access."""
import argparse
import pathlib

from playwright.sync_api import expect, sync_playwright

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--url", default="http://127.0.0.1:4173")
parser.add_argument("--artifacts", default=".playwright-mcp")
args = parser.parse_args()
output = pathlib.Path(args.artifacts)
output.mkdir(parents=True, exist_ok=True)

with sync_playwright() as p:
    browser = p.chromium.launch(channel="chrome", headless=True)
    errors = []
    for width in [320, 390, 1440]:
        context = browser.new_context(
            viewport={"width": width, "height": 1000},
            permissions=["clipboard-read", "clipboard-write"],
            reduced_motion="reduce",
        )
        # This suite checks the guide independently of WebGL; rendering has its
        # own regression suite. A failed engine load must leave setup usable.
        context.route("**/engine/**", lambda route: route.abort())
        page = context.new_page()
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.goto(args.url + "/#connect")
        page.wait_for_load_state("networkidle")
        plugin = page.get_by_role("tab", name="Plugin", exact=True)
        manual = page.get_by_role("tab", name="Manual MCP", exact=True)
        api = page.get_by_role("tab", name="Direct API", exact=True)
        expect(plugin).to_have_attribute("aria-selected", "true")
        expect(page.locator("#panel-plugin")).to_be_visible()
        expect(page.locator("#panel-mcp")).to_be_hidden()

        # Arrow navigation, wrapping and Home/End must include the new tab.
        plugin.focus()
        plugin.press("ArrowRight")
        expect(manual).to_be_focused()
        expect(page.locator("#panel-mcp")).to_be_visible()
        manual.press("End")
        expect(api).to_be_focused()
        expect(page.locator("#panel-api")).to_be_visible()
        api.press("ArrowRight")
        expect(plugin).to_be_focused()
        plugin.press("ArrowLeft")
        expect(api).to_be_focused()
        api.press("Home")
        expect(plugin).to_be_focused()

        for details in page.locator("#panel-plugin details").all():
            details.locator("summary").click()
            expect(details).to_have_attribute("open", "")
        for client in ["claude", "openai", "opencode", "pi"]:
            expect(page.locator(f'[data-plugin-client="{client}"]')).to_be_visible()
        for button in page.locator("#panel-plugin [data-copy]").all():
            target = page.locator("#" + button.get_attribute("data-copy"))
            expected = target.text_content()
            button.click()
            expect(button).to_have_text("Copied")
            assert page.evaluate("navigator.clipboard.readText()") == expected

        # Check every connection method with all disclosures open.
        for tab in [plugin, manual, api]:
            tab.click()
            panel = page.locator("#" + tab.get_attribute("aria-controls"))
            for details in panel.locator("details:not([open])").all():
                details.locator("summary").click()
            assert page.evaluate("document.documentElement.scrollWidth <= innerWidth"), (
                width, tab.text_content()
            )
        plugin.click()
        page.locator(".step-agent").scroll_into_view_if_needed()
        page.screenshot(path=str(output / f"site-plugin-{width}.png"))
        context.close()

    context = browser.new_context(java_script_enabled=False, viewport={"width": 320, "height": 900})
    page = context.new_page()
    page.goto(args.url + "/#connect")
    expect(page.locator("#plugin-source")).to_be_visible()
    expect(page.locator("#plugin-bridge")).to_be_visible()
    page.locator('[data-plugin-client="claude"] summary').click()
    expect(page.locator("#plugin-claude")).to_be_visible()
    assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
    context.close()
    assert not errors, errors
    browser.close()

print("Site onboarding checks passed: plugin clients, tabs, clipboard, mobile layout and no-JS access.")
