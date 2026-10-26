#!/usr/bin/env python3
"""Build three local examples with real MCP calls, then explicitly publish them.

Uses the installed prebuilt ergo-mcp executable; Python itself needs only stdlib.
No direct S3 writes, browser cookies, or API shortcuts. Publication is never retried.
"""
import argparse
import base64
import hashlib
import json
import os
import pathlib
import select
import shutil
import subprocess
import tempfile


def designs():
    rocket = ["....R....", "...RWR...", "...RWR...", "...RWR...", "..RRWRR..",
              ".RRWWWRR.", "...YYY...", "...Y.Y...", "..Y...Y.."]
    colors = {"R":4,"W":15,"Y":14}
    yield "Pixel rocket", [("3005",colors[c],[12+x,12+len(rocket)-y,0],0)
        for y,row in enumerate(rocket) for x,c in enumerate(row) if c in colors]
    compass = [("3005",14,[20,20,0],0)]
    for direction, color in [((0,1),4),((1,0),1),((0,-1),15),((-1,0),28)]:
        dx,dy = direction
        for n in range(1,6):
            compass.append(("3005",color,[20+dx*n,20+dy*n,0],0))
        for side in [-1,1]:
            compass.append(("3005",color,[20+dx*3+dy*side,20+dy*3-dx*side,0],0))
    yield "Compass plaza", compass
    courtyard = [("3001",28,[10+4*i,y,0],0) for y in [10,24] for i in range(4)]
    courtyard += [("3001",28,[x,12+4*i,0],1) for x in [10,24] for i in range(3)]
    courtyard += [("3024",71,[17,y,0],0) for y in range(12,24)]
    courtyard += [("3003",2,[x,y,0],0) for x in [13,20] for y in [14,20]]
    yield "Garden courtyard", courtyard


class Mcp:
    def __init__(self, binary, workspace, url, token_file):
        command = [binary,"--workspace",str(workspace),"--headless"]
        environment = os.environ.copy()
        if token_file:
            command += ["--publish-url",url,"--publish-token-file",str(token_file)]
        else:
            for key in ["ERGO_PUBLISH_URL","ERGO_PUBLISH_TOKEN","ERGO_PUBLISH_TOKEN_FILE"]:
                environment.pop(key,None)
        self.log = tempfile.TemporaryFile(mode="w+t")
        self.process = subprocess.Popen(command,stdin=subprocess.PIPE,stdout=subprocess.PIPE,
            stderr=self.log,text=True,env=environment)
        self.sequence = 0
        self.rpc("initialize",{"protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"ergo-production-demo","version":"1"}})

    def rpc(self, method, params):
        assert self.process.stdin is not None and self.process.stdout is not None
        self.sequence += 1
        self.process.stdin.write(json.dumps({"jsonrpc":"2.0","id":self.sequence,
            "method":method,"params":params})+"\n")
        self.process.stdin.flush()
        if not select.select([self.process.stdout],[],[],900)[0]:
            raise RuntimeError("MCP timed out; no request has been automatically retried")
        line = self.process.stdout.readline()
        if not line:
            self.log.seek(0)
            raise RuntimeError("MCP exited: "+self.log.read())
        response = json.loads(line)
        if "error" in response:
            raise RuntimeError(str(response["error"]))
        return response["result"]

    def call(self, tool_name, **arguments):
        result = self.rpc("tools/call",{"name":tool_name,"arguments":arguments})
        if result.get("isError"):
            raise RuntimeError(json.dumps(result.get("structuredContent",result)))
        return result

    def close(self):
        assert self.process.stdin is not None
        self.process.stdin.close()
        self.process.wait(timeout=15)
        self.log.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode",choices=["build","publish"])
    parser.add_argument("--binary",default="ergo-mcp")
    parser.add_argument("--output",type=pathlib.Path,default=pathlib.Path("target/mcp-prod-demo"))
    parser.add_argument("--url",default="https://ergosandbox.com")
    parser.add_argument("--token-file",type=pathlib.Path)
    args = parser.parse_args()
    binary = shutil.which(args.binary)
    if not binary: parser.error("Install ergo-mcp first, or pass --binary /absolute/path/ergo-mcp")
    output = args.output.resolve()
    manifest = output/"results.json"
    if args.mode == "publish" and not args.token_file:
        parser.error("publish requires --token-file from /login#connect")
    if args.mode == "build":
        if manifest.exists(): parser.error("Output already contains a run; choose a new --output")
        output.mkdir(parents=True,exist_ok=True)
        results = {"origin":args.url,"models":[]}
        for name, placements in designs():
            slug = name.lower().replace(" ","-")
            workspace = output/(slug+".ergo.json")
            mcp = Mcp(binary,workspace,args.url,args.token_file)
            try:
                for part in sorted({part for part,_,_,_ in placements}):
                    mcp.call("get_catalog",part=part)
                for part,color,corner,turns in placements:
                    mcp.call("place_brick",part=part,color=color,min_corner=corner,quarter_turns=turns)
                feedback = mcp.call("validate_construction")["structuredContent"]
                if not feedback["publication_ready"]: raise RuntimeError(json.dumps(feedback))
                gravity = mcp.call("test_stability_by_gravity",steps=1)["structuredContent"]
                images = mcp.call("view_construction",views=["isometric","top"],width=640,height=480)
                for index,image in enumerate(c for c in images["content"] if c["type"] == "image"):
                    (output/f"{slug}-{index}.png").write_bytes(base64.b64decode(image["data"]))
                results["models"].append({"name":name,"workspace":workspace.name,
                    "sha256":hashlib.sha256(workspace.read_bytes()).hexdigest(),
                    "brick_count":feedback["brick_count"],"validation":feedback,"gravity":gravity})
                manifest.write_text(json.dumps(results,indent=2)+"\n")
                print(f"Built {name}: {len(placements)} bricks; images and local working copy in {output}")
            finally: mcp.close()
    else:
        results = json.loads(manifest.read_text())
        if results["origin"] != args.url: parser.error("Run origin differs from --url")
        for model in results["models"]:
            if model.get("publication_attempted"):
                if model.get("publication",{}).get("published") is True:
                    print("Already confirmed: "+model["publication"]["view_url"])
                    continue
                parser.error("Prior publication has no confirmed result; resolve it before any new attempt")
            workspace = output/model["workspace"]
            if hashlib.sha256(workspace.read_bytes()).hexdigest() != model["sha256"]:
                parser.error("Working copy changed since the recorded visual review")
            mcp = Mcp(binary,workspace,args.url,args.token_file.resolve())
            try:
                feedback = mcp.call("validate_construction")["structuredContent"]
                if not feedback["publication_ready"]: raise RuntimeError(json.dumps(feedback))
                model["publication_attempted"] = True
                manifest.write_text(json.dumps(results,indent=2)+"\n")
                result = mcp.call("publish_construction",name="MCP demo — "+model["name"],
                    expected_local_revision=feedback["revision"])
                publication = result["structuredContent"]
                model["publication"] = publication
                manifest.write_text(json.dumps(results,indent=2)+"\n")
                if publication.get("published") is not True: raise RuntimeError("Publication not confirmed")
                print(model["name"]+": "+publication["view_url"])
                print("Accepted author: "+str(publication.get("audit",{}).get("actor_name")))
            finally: mcp.close()


if __name__ == "__main__": main()
