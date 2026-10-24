#!/usr/bin/env python3
"""Exercise real MCP stdio messages, rollback, physical feedback, and PNG content."""
import argparse
import base64
import json
import pathlib
import struct
import subprocess
import tempfile
import os


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="target/debug/ergo-mcp")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="ergo-mcp-test-") as directory:
        workspace = pathlib.Path(directory) / "construction.json"
        library = pathlib.Path(directory) / "ldraw"
        (library / "parts").mkdir(parents=True)
        (library / "parts" / "custom-part.dat").write_text("0 Synthetic custom part\n0 Author: Test fixture\n0 !LICENSE Synthetic test fixture\n4 16 -20 0 -20 20 0 -20 20 0 20 -20 0 20\n4 16 -20 24 -20 -20 24 20 20 24 20 20 24 -20\n")
        process = subprocess.Popen(
            [str(pathlib.Path(args.binary).resolve()), "--workspace", str(workspace), "--headless"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True,
            env={**os.environ,"ERGO_ASSET_FETCH":"0","LDRAW_DIR":str(library)},
        )
        assert process.stdin is not None and process.stdout is not None and process.stderr is not None
        sequence = 0

        def rpc(method, params=None):
            nonlocal sequence
            assert process.stdin is not None and process.stdout is not None and process.stderr is not None
            sequence += 1
            process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": sequence,
                                           "method": method, "params": params or {}}) + "\n")
            process.stdin.flush()
            line = process.stdout.readline()
            assert line, process.stderr.read()
            response = json.loads(line)
            assert response["id"] == sequence, response
            assert "error" not in response, response
            return response["result"]

        def call(tool_name, **arguments):
            return rpc("tools/call", {"name": tool_name, "arguments": arguments})

        try:
            initialized = rpc("initialize", {"protocolVersion": "2025-06-18",
                                            "capabilities": {}, "clientInfo": {"name": "ergo-smoke", "version": "1"}})
            assert initialized["capabilities"]["tools"]["listChanged"] is False
            names = {tool["name"] for tool in rpc("tools/list")["tools"]}
            assert {"test_stability_by_gravity", "test_visual_appearance",
                    "test_stability_by_earthquake", "test_moderate_earthquake_stability"} <= names
            result = call("place_brick", part="3003", color=4, min_corner=[2, 2, 0])
            assert result["structuredContent"]["overlap_free"] is True
            assert call("place_brick", part="3003", color=4, min_corner=[2, 2, 0])["isError"] is True
            assert call("list_bricks")["structuredContent"]["total"] == 1
            result = call("place_brick", part="3003", color=1, min_corner=[8, 2, 8])
            assert result["structuredContent"]["static_under_current_joint_model"] is False
            result = call("test_stability_by_gravity", steps=1)
            assert result["structuredContent"]["report"]["survived_declared_case"] is False
            # A test must not settle/mutate the working copy itself.
            floating = call("list_bricks")["structuredContent"]["bricks"][1]
            assert floating["position"][2] == 9
            visual = call("test_visual_appearance", camera={"position_world": [15, 18, 12],
                          "target_world": [5, 3, 4], "vertical_fov_degrees": 40}, width=320, height=240)
            images = [item for item in visual["content"] if item["type"] == "image"]
            assert len(images) == 1
            png = base64.b64decode(images[0]["data"])
            assert png[:8] == b"\x89PNG\r\n\x1a\n"
            assert struct.unpack(">II", png[16:24]) == (320, 240)
            assert len(png) > 1000
            call("undo")
            result = call("test_stability_by_gravity", steps=1)
            assert result["structuredContent"]["calibrated_stability_verified"] is True
            result = call("test_stability_by_earthquake", peak_ground_acceleration_g=0.1,
                          frequency_hz=4, duration_s=0.5, include_images=False)
            report = result["structuredContent"]
            assert report["acceleration_upper_bound_g"] == 0.1
            assert report["tested_axes"] == ["X", "Y"]
            assert report["working_copy_changed"] is False
            for axis in report["reports"]:
                assert axis["report"]["shake_acceleration_bound_m_s2"] > 0
            result = call("test_moderate_earthquake_stability", include_images=False)
            assert result["structuredContent"]["preset"] == "ergo-moderate-shake-v2"
            assert call("test_stability_by_earthquake", moment_magnitude=6)["isError"] is True
            result = call("test_visual_appearance", views=["north", "top"], width=320, height=240)
            assert len([item for item in result["content"] if item["type"] == "image"]) == 2
            assert call("list_bricks")["structuredContent"]["total"] == 1
            call("place_brick", part="3003", color=1, min_corner=[8, 2, 0])
            child = call("create_group", name="Child", piece_ids=[1])["structuredContent"]["group_id"]
            parent = call("create_group", name="Assembly", piece_ids=[2], group_ids=[child])["structuredContent"]["group_id"]
            before = call("list_bricks")["structuredContent"]["bricks"]
            result = call("transform_group", group_id=parent, translation=[0, 4, 0])
            assert result["isError"] is False, result
            after = call("list_bricks")["structuredContent"]["bricks"]
            for old, new in zip(before, after):
                assert abs(new["position"][1] - old["position"][1] - 4) < 1e-9
            assert call("set_group_members", group_id=child, group_ids=[parent])["isError"] is True
            assert len(call("list_groups")["structuredContent"]["groups"]) == 2
            assert len(json.loads(workspace.read_text())["groups"]) == 2
            assert len(call("get_catalog")["structuredContent"]["catalog"]["colors"]) == 20
            assert len(call("get_catalog", include_extended=True)["structuredContent"]["catalog"]["colors"]) > 300
            custom = call("get_catalog", part="custom-part")
            assert custom["structuredContent"]["requested_part"]["quality"] == "ldraw", custom
            custom = call("place_brick", part="custom-part", color=47, min_corner=[16, 2, 0])
            assert custom["isError"] is False and custom["structuredContent"]["publication_ready"] is False, custom
            assert "sourced" in custom["structuredContent"]["failure"]
            submodel = ("0 FILE pair.ldr\n"
                        "1 16 0 0 0 1 0 0 0 1 0 0 0 1 pillar.ldr\n"
                        "1 16 100 0 0 1 0 0 0 1 0 0 0 1 pillar.ldr\n"
                        "0 FILE pillar.ldr\n"
                        "1 16 0 -24 0 1 0 0 0 1 0 0 0 1 3003.dat\n")
            matrix = [[1, 0, 0, 500], [0, 1, 0, 0], [0, 0, 1, -100], [0, 0, 0, 1]]
            result = call("place_submodel", ldraw=submodel, matrix=matrix, color=14)
            assert result["isError"] is False, result
            stored = json.loads(workspace.read_text())
            assert stored["format"] == "ergo-ldraw-1" and "bricks" not in stored
            assert len(stored["pieces"]) == 5
            assert any(name.endswith("pillar.ldr") for name in stored["model"]["subparts"])
            added = call("list_bricks")["structuredContent"]["bricks"][-2:]
            assert [brick["color"] for brick in added] == [14, 14]
            assert [brick["position"][0] for brick in added] == [25, 30]
            before = workspace.read_bytes()
            assert call("place_submodel", ldraw=submodel, matrix=matrix, color=14)["isError"] is True
            assert workspace.read_bytes() == before, "Failed submodel placement must roll back"
            matrix[3][0] = 1
            assert call("place_submodel", ldraw=submodel, matrix=matrix)["isError"] is True
            assert workspace.exists()
            print("MCP smoke passed: tool discovery, collision rollback, gravity, camera PNGs, earthquakes, reusable LDraw subparts, matrix placement, and canonical storage.")
        finally:
            process.stdin.close()
            process.wait(timeout=10)
            stderr = process.stderr.read()
            assert process.returncode == 0, stderr


if __name__ == "__main__":
    main()
