#!/usr/bin/env python3
"""Run against an already-started, isolated development server."""
import argparse
import math
import pathlib
import tempfile
import json
import re
import urllib.request
from playwright.sync_api import sync_playwright, expect


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default="http://127.0.0.1:3000")
    parser.add_argument("--artifacts", default=".playwright-mcp")
    args = parser.parse_args()
    with urllib.request.urlopen(args.url + "/api/config") as response:
        configuration = json.load(response)
    assert configuration.get("ground_source") == "disabled", "Use an isolated server with ERGO_GROUND_SOURCE=disabled; automated tests must not fetch public OSM tiles"
    artifacts = pathlib.Path(args.artifacts)
    artifacts.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="ergo-web-test-") as directory, sync_playwright() as p:
        browser = p.chromium.launch(channel="chrome", headless=True,
                                    args=["--enable-unsafe-swiftshader"])
        page = browser.new_page(viewport={"width": 1440, "height": 950}, device_scale_factor=1)
        errors = []
        cached = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.on("response", lambda response: cached.append(response.url)
                if response.from_service_worker and "x-ergo-bytes" in response.headers else None)
        # The root is a login/setup landing page, not an anonymous workbench.
        page.goto(args.url)
        expect(page.get_by_role("heading", name="A shared world. Built brick by brick.", exact=True)).to_be_visible(timeout=30000)
        expect(page.get_by_role("button", name="Generate access token", exact=True)).to_be_enabled()
        page.screenshot(path=str(artifacts / "landing-desktop.png"), full_page=True)
        page.set_viewport_size({"width":390,"height":844})
        assert page.evaluate("document.documentElement.scrollWidth <= innerWidth"), page.evaluate("() => [...document.querySelectorAll('body *')].filter(e => e.getBoundingClientRect().right > innerWidth+1).map(e=>({tag:e.tagName,classes:e.className,right:e.getBoundingClientRect().right})).slice(0,10)")
        page.screenshot(path=str(artifacts / "landing-mobile.png"), full_page=True)
        page.set_viewport_size({"width":1440,"height":950})
        page.get_by_role("button", name="Generate access token", exact=True).click()
        token_field = page.get_by_role("textbox", name="MCP and API token", exact=True)
        expect(token_field).to_be_visible()
        token = token_field.input_value()
        assert len(token) > 50
        assert "ERGO_PUBLISH_TOKEN_FILE" in page.locator(".connection-code").first.inner_text()
        page.context.grant_permissions(["clipboard-read", "clipboard-write"])
        page.get_by_role("button", name="Copy token", exact=True).click()
        expect(page.get_by_role("button", name="Copied", exact=True)).to_be_visible()
        assert page.evaluate("navigator.clipboard.readText()") == token
        with page.expect_download() as downloaded:
            page.get_by_role("button", name="Download MCP token", exact=True).click()
        assert pathlib.Path(downloaded.value.path()).read_text() == token
        allowance = page.request.get(args.url + "/api/shared-worlds/allowance", headers={"Authorization": "Bearer " + token})
        assert allowance.status == 200 and allowance.json()["actor"] == "development"
        assert page.evaluate("token => !Object.values(localStorage).includes(token)", token)
        page.route("**/api/auth/me", lambda route: route.fulfill(json={"id":None,"name":"Guest","google_enabled":True,"signed_in":False,"development":False,"edit_allowance":None}))
        page.reload()
        expect(page.get_by_role("link", name="Continue with Google", exact=True)).to_have_attribute("href", re.compile(r"^/auth/google\?return_to="))
        expect(page.get_by_role("button", name="Generate access token", exact=True)).to_be_disabled()
        page.unroute("**/api/auth/me")
        page.goto(args.url + "/app")
        page.wait_for_load_state("networkidle")
        expect(page.locator(".status-message")).to_contain_text("Construction loaded", timeout=30000)
        expect(page.locator("#world-canvas")).to_be_visible()
        page.wait_for_function("Number(document.querySelector('#world-canvas').dataset.groundStuds) > 0")
        assert int(page.locator("#world-canvas").get_attribute("data-ground-studs")) <= 97 * 97
        assert page.evaluate("document.querySelector('canvas').getContext('webgl2') !== null")
        page.locator("#world-canvas").click()
        expect(page.locator(".selected-part h3")).to_be_visible()
        page.get_by_role("button", name="Fly", exact=True).click()
        canvas = page.locator("#world-canvas")
        canvas.focus()
        expect(canvas).to_have_attribute("data-navigation", "fly")
        expect(canvas).to_have_attribute("data-navigation-speed", "0")

        def position():
            return json.loads(canvas.get_attribute("data-camera-position"))

        def rotation():
            return json.loads(canvas.get_attribute("data-camera-rotation"))

        def wait_for_travel(old):
            page.wait_for_function("old => { const p = JSON.parse(document.querySelector('#world-canvas').dataset.cameraPosition); return Math.hypot(p[0]-old[0], p[1]-old[1]) > 1; }", arg=old)

        def assert_horizontal_still(old):
            current = position()
            assert sum((current[i] - old[i]) ** 2 for i in (0, 1)) < 1e-8

        # Steering a stopped flight rotates the view without translating it.
        old_position, old_rotation = position(), rotation()
        page.keyboard.down("ArrowRight")
        page.wait_for_function("old => JSON.parse(document.querySelector('#world-canvas').dataset.cameraRotation)[0] < old - 0.2", arg=old_rotation[0])
        page.keyboard.up("ArrowRight")
        assert_horizontal_still(old_position)
        page.keyboard.down("ArrowUp")
        page.wait_for_function("old => JSON.parse(document.querySelector('#world-canvas').dataset.cameraRotation)[1] < old - 0.2", arg=old_rotation[1])
        page.keyboard.up("ArrowUp")
        assert_horizontal_still(old_position)
        page.keyboard.down("a")
        page.wait_for_function("Number(document.querySelector('#world-canvas').dataset.navigationSpeed) > 8")
        page.keyboard.up("a")
        old_position = position()
        wait_for_travel(old_position)  # Flight cruises after A is released.
        page.keyboard.down("z")
        expect(canvas).to_have_attribute("data-navigation-speed", "0")
        page.keyboard.up("z")
        old_position = position()
        page.wait_for_timeout(120)
        assert_horizontal_still(old_position)

        page.get_by_role("button", name="Walk", exact=True).click()
        expect(canvas).to_have_attribute("data-navigation", "walk")
        canvas.focus()
        assert position()[2] >= 4 - 1e-6
        old_position, old_rotation = position(), rotation()
        page.keyboard.down("ArrowLeft")
        page.wait_for_function("old => JSON.parse(document.querySelector('#world-canvas').dataset.cameraRotation)[0] > old + 0.2", arg=old_rotation[0])
        page.keyboard.up("ArrowLeft")
        assert_horizontal_still(old_position)
        page.keyboard.down("ArrowUp")
        wait_for_travel(old_position)
        page.keyboard.up("ArrowUp")
        old_position = position()
        page.keyboard.down("ArrowDown")
        wait_for_travel(old_position)
        page.keyboard.up("ArrowDown")
        current = position()
        # Down moves opposite the horizontal viewing direction, without pitching.
        yaw = rotation()[0]
        assert (current[0] - old_position[0]) * -math.sin(yaw) + (current[1] - old_position[1]) * math.cos(yaw) < 0
        assert abs(rotation()[1]) < 1e-6
        old_position = position()
        speed = float(canvas.get_attribute("data-navigation-speed"))
        page.keyboard.down("a")
        page.wait_for_function("old => Number(document.querySelector('#world-canvas').dataset.navigationSpeed) > old + 3", arg=speed)
        page.keyboard.up("a")
        assert_horizontal_still(old_position)
        page.keyboard.down("z")
        expect(canvas).to_have_attribute("data-navigation-speed", "0")
        page.keyboard.up("z")
        page.keyboard.down("ArrowUp")
        page.wait_for_timeout(120)
        page.keyboard.up("ArrowUp")
        assert_horizontal_still(old_position)

        # Space reaches a 12-unit ballistic peak and lands, even at zero speed.
        floor = position()[2]
        page.evaluate("""floor => {
            const canvas = document.querySelector('#world-canvas');
            window.jumpPeak = floor;
            window.jumpObserver = new MutationObserver(() => {
                window.jumpPeak = Math.max(window.jumpPeak, JSON.parse(canvas.dataset.cameraPosition)[2]);
            });
            window.jumpObserver.observe(canvas, {attributes: true, attributeFilter: ['data-camera-position']});
        }""", floor)
        page.keyboard.down("Space")
        page.wait_for_function("floor => JSON.parse(document.querySelector('#world-canvas').dataset.cameraPosition)[2] > floor + 11.5", arg=floor)
        page.keyboard.up("Space")
        page.wait_for_function("floor => Math.abs(JSON.parse(document.querySelector('#world-canvas').dataset.cameraPosition)[2] - floor) < 1e-6", arg=floor)
        peak = page.evaluate("() => { window.jumpObserver.disconnect(); return window.jumpPeak; }")
        assert 11.5 < peak - floor <= 12.0 + 1e-5, peak - floor
        assert_horizontal_still(old_position)

        # On-screen controls share the same throttle and clear on release.
        faster = page.get_by_role("button", name="Faster (A)", exact=True)
        faster.hover()
        page.mouse.down()
        page.wait_for_function("Number(document.querySelector('#world-canvas').dataset.navigationSpeed) > 3")
        page.mouse.up()
        page.wait_for_timeout(50)
        speed = float(canvas.get_attribute("data-navigation-speed"))
        page.wait_for_timeout(120)
        assert float(canvas.get_attribute("data-navigation-speed")) == speed

        # Eye-height controls reflect the persisted navigation setting on re-entry.
        page.locator("#eye-height").fill("6")
        # Re-entering Fly always resets the cruise throttle.
        page.get_by_role("button", name="Fly", exact=True).click()
        expect(canvas).to_have_attribute("data-navigation-speed", "0")
        expect(page.get_by_role("button", name="Look up", exact=True)).to_be_visible()
        expect(page.get_by_role("button", name="Jump (Space)", exact=True)).to_be_hidden()
        page.get_by_role("button", name="Walk", exact=True).click()
        expect(page.locator("#eye-height")).to_have_value("6")
        page.get_by_role("button", name="Fit construction", exact=True).click()
        expect(canvas).to_have_attribute("data-navigation", "orbit")
        page.screenshot(path=str(artifacts / "smoke-desktop.png"))

        # Full-screen keeps camera/navigation controls and supports both exits.
        viewport = page.locator("#world-viewport")
        page.get_by_role("button", name="Enter full screen", exact=True).click()
        expect(viewport).to_have_class(re.compile(r"is-fullscreen"))
        page.wait_for_function("() => document.fullscreenElement?.id === 'world-viewport'")
        assert page.evaluate("() => {const r=document.querySelector('#world-viewport').getBoundingClientRect();return r.x===0&&r.y===0&&Math.abs(r.width-innerWidth)<1&&Math.abs(r.height-innerHeight)<1;}")
        viewport.get_by_role("button", name="Walk", exact=True).click()
        expect(canvas).to_be_focused()
        expect(canvas).to_have_attribute("data-navigation", "walk")
        page.get_by_role("button", name="Exit full screen", exact=True).click()
        expect(viewport).not_to_have_class(re.compile(r"is-fullscreen"))
        expect(canvas).to_have_attribute("data-navigation", "walk")
        canvas.focus()
        page.keyboard.press("Shift+f")
        expect(viewport).to_have_class(re.compile(r"is-fullscreen"))
        page.keyboard.press("Escape")
        expect(viewport).not_to_have_class(re.compile(r"is-fullscreen"))
        # Browsers/embedded views that deny Fullscreen still get immersive CSS.
        page.evaluate("() => {window.originalFullscreen=Element.prototype.requestFullscreen;Element.prototype.requestFullscreen=()=>Promise.reject(new Error('Fullscreen denied by test'));}")
        try:
            page.get_by_role("button", name="Enter full screen", exact=True).click()
            expect(viewport).to_have_class(re.compile(r"is-fullscreen"))
            page.keyboard.press("Escape")
            expect(viewport).not_to_have_class(re.compile(r"is-fullscreen"))
        finally:
            page.evaluate("() => {Element.prototype.requestFullscreen=window.originalFullscreen;}")
        page.get_by_role("button", name="Fit construction", exact=True).click()

        model = pathlib.Path(directory) / "single.ldr"
        model.write_text("0 Browser smoke construction\n1 4 0 0 0 1 0 0 0 1 0 0 0 1 3003.dat\n")

        def import_file(path, name):
            page.get_by_role("button", name="＋ Import construction", exact=True).click()
            page.locator("#model-file").set_input_files(str(path))
            page.get_by_label("Construction name", exact=True).fill(name)
            page.get_by_role("button", name="Open in workspace", exact=True).click()
            expect(page.locator("h1")).to_have_text(name, timeout=30000)
            expect(page.locator(".snapshot-badge")).to_have_text("Working copy")

        import_file(model, "Browser smoke construction")
        page.locator(".group-button").click()
        page.get_by_label("New group name", exact=True).fill("Assembly")
        page.get_by_label("Piece IDs", exact=True).fill("1")
        page.get_by_role("button", name="Create group", exact=True).click()
        expect(page.get_by_role("combobox", name="Selected group", exact=True)).to_have_value("1")
        page.get_by_role("button", name="Close groups dialog", exact=True).click()
        page.get_by_role("button", name="Validate & save snapshot", exact=True).click()
        expect(page.locator(".snapshot-badge")).to_have_text("Validated snapshot", timeout=30000)
        page.get_by_role("button", name="▷ Run gravity", exact=True).click()
        expect(page.get_by_role("button", name="Ⅱ Pause", exact=True)).to_be_visible()
        page.wait_for_function("parseFloat(document.querySelector('.simulation-stats div:last-child dd').textContent) > 0.2")
        page.get_by_role("button", name="Ⅱ Pause", exact=True).click()
        page.get_by_role("button", name="Reset", exact=True).click()
        expect(page.locator(".simulation-stats div:last-child dd")).to_have_text("0.0s")
        page.get_by_role("button", name="Test solidity…", exact=True).click()
        page.get_by_role("button", name="Run load test", exact=True).click()
        expect(page.locator(".solidity-result")).to_be_visible(timeout=30000)
        expect(page.locator(".solidity-result")).to_contain_text("Peak movement")
        page.get_by_role("button", name="Close solidity dialog", exact=True).click()
        expect(page.locator(".snapshot-badge")).to_have_text("Validated snapshot")

        count = page.locator(".world-item").count()
        overlap = pathlib.Path(directory) / "overlap.ldr"
        overlap.write_text("0 Invalid overlap\n" + "1 4 0 0 0 1 0 0 0 1 0 0 0 1 3003.dat\n" * 2)
        import_file(overlap, "Overlap must not publish")
        page.get_by_role("button", name="Validate & save snapshot", exact=True).click()
        expect(page.locator(".error-toast")).to_contain_text("overlap", timeout=30000)
        assert page.locator(".world-item").count() == count

        page.locator(".world-item").filter(has_text="Paris garden pavilion").click()
        expect(page.locator(".status-message")).to_contain_text("Construction loaded", timeout=30000)
        page.get_by_role("button", name="Nearest construction", exact=True).click()
        expect(page.locator(".status-message")).to_contain_text("Nearest construction", timeout=30000)
        page.set_viewport_size({"width": 390, "height": 844})
        expect(page.locator("#world-canvas")).to_be_visible()
        assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
        page.screenshot(path=str(artifacts / "smoke-mobile.png"), full_page=True)
        assert not errors, errors
        # Reopening the same immutable scene should hit the bounded download cache.
        assert cached, "No persistent model-cache hit was observed"
        # Published shared worlds have a real, bounded navigation view, while
        # editing/simulation never treats this fragment as a complete assembly.
        snapshots = page.request.get(args.url + "/api/worlds").json()
        saved = next(world for world in snapshots if world["name"] == "Browser smoke construction")
        scene = page.request.get(args.url + "/api/worlds/" + saved["id"] + "/export").json()
        auth = {"Authorization":"Bearer " + token,"X-Ergo-Request":"1"}
        response = page.request.post(args.url + "/api/shared-worlds", headers=auth,
                                     data={"name":"Browser shared world","geo":scene["geo"],"authenticated_editing":True})
        assert response.status == 200, response.text()
        head = response.json()
        published = page.request.post(args.url + "/api/shared-worlds/" + head["id"] + "/assemblies",
                                      headers=auth,data={"base":head["revision"],"scene":scene})
        assert published.status == 200, published.text()
        page.set_viewport_size({"width":1440,"height":950})
        page.goto(args.url + "/app?shared_world=" + head["id"])
        expect(page.locator("h1")).to_have_text("Browser shared world",timeout=30000)
        expect(page.locator(".snapshot-badge")).to_have_text("Shared world view")
        expect(page.locator(".status-message")).to_contain_text("Shared-world region loaded",timeout=30000)
        page.get_by_role("button", name="Enter full screen", exact=True).click()
        expect(page.locator("#world-viewport")).to_have_class(re.compile(r"is-fullscreen"))
        page.keyboard.press("Escape")
        expect(page.get_by_role("button", name="▷ Run gravity", exact=True)).to_have_count(0)
        expect(page.get_by_role("link", name="Edit with MCP / API", exact=True)).to_have_attribute("href","/#connect")
        assert page.request.get(args.url + "/api/shared-worlds/" + head["id"],headers=auth).json()["revision"] == published.json()["accepted"]["revision"]
        page.screenshot(path=str(artifacts / "shared-world.png"))
        assert not errors, errors
        browser.close()
        print("Web smoke passed: landing/SSO setup, token generation/copy/download, API access, WebGL/studded ground, nearest construction, fly/walk controls, full-screen entry/exit/fallback, import/save, gravity/reset, solidity report, overlap rejection, cache reuse, and mobile layout.")


if __name__ == "__main__":
    main()
