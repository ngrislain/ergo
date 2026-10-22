#!/usr/bin/env python3
"""Run against an already-started, isolated development server."""
import argparse
import math
import pathlib
import tempfile
import json
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
        page.goto(args.url)
        page.wait_for_load_state("networkidle")
        expect(page.locator(".status-message")).to_contain_text("Construction loaded", timeout=30000)
        expect(page.locator("#world-canvas")).to_be_visible()
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
        page.set_viewport_size({"width": 390, "height": 844})
        expect(page.locator("#world-canvas")).to_be_visible()
        assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
        page.screenshot(path=str(artifacts / "smoke-mobile.png"), full_page=True)
        assert not errors, errors
        # Reopening the same immutable scene should hit the bounded download cache.
        assert cached, "No persistent model-cache hit was observed"
        browser.close()
        print("Web smoke passed: WebGL, picking, fly steering/throttle, walk movement/turning/speed/jump, on-screen throttle, import/save, gravity/reset, solidity report, overlap rejection, cache reuse, and mobile layout.")


if __name__ == "__main__":
    main()
