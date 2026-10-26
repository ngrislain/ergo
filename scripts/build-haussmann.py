#!/usr/bin/env python3
"""Build and inspect a Haussmann-style proposal through real MCP tools.

This is an isolated, non-destructive construction test, not another hosted world.
It records physical-validation limitations and never bypasses shared acceptance.
"""
import argparse
import base64
from collections import Counter
import importlib.util
import json
import pathlib
import shutil

ROOT=pathlib.Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location("demo",ROOT/"scripts/demo-mcp.py")
assert spec and spec.loader
demo=importlib.util.module_from_spec(spec);spec.loader.exec_module(demo)
PARTS={"3007":(8,2,1),"2456":(6,2,1),"3001":(4,2,1),"3002":(3,2,1),"3003":(2,2,1),"3005":(1,1,1),
    "41539":(8,8,1/3),"3035":(8,4,1/3),"3034":(8,2,1/3),"3031":(4,4,1/3),"3022":(2,2,1/3),"3020":(4,2,1/3),
    "3666":(6,1,1/3),"3023":(2,1,1/3),"3710":(4,1,1/3),"3024":(1,1,1/3),"2431":(4,1,1/3)}
IDENTITY=(1,0,0,0,1,0,0,0,1)
QUARTER=(0,0,-1,0,1,0,1,0,0)
WINDOW=(1,0,0,0,0,1,0,-1,0)
TALL_WINDOW=(0,0,1,-1,0,0,0,-1,0)


def number(value):
    return format(value,".12g")


def reference(file,color,position,matrix=IDENTITY):
    return "1 "+str(color)+" "+" ".join(map(number,(*position,*matrix)))+" "+file


def place(lines,part,color,x,north,height,turn=False):
    width,depth,tall=PARTS[part]
    if turn:width,depth=depth,width
    lines.append(reference(part+".dat",color,((x+width/2)*20,-(height+tall)*24,(north+depth/2)*20),QUARTER if turn else IDENTITY))


def strip(lines,length,color,x,north,height,turn=False,rail=False):
    # Largest admissible catalog pieces, preserving the requested footprint.
    choices=[("3666",6),("3710",4),("3023",2)] if rail else [("3007",8),("2456",6),("3001",4),("3003",2)]
    offset=0
    for part,size in choices:
        while length>=size:
            place(lines,part,color,x+(0 if turn else offset),north+(offset if turn else 0),height,turn)
            offset+=size;length-=size
    assert length==0


def floor_plate(lines,height,color=15,balcony=False):
    rows=[(-2,8),(6,8),(14,2)] if balcony else [(0,8),(8,8)]
    for north,depth in rows:
        for x in range(0,32,8):place(lines,"41539" if depth==8 else "3034",color,x,north,height)


def model():
    main=[];floor_plate(main,0,71)
    files={"main.ldr":main}
    for extended in [False,True]:
        lines=[]
        for z in range(6):
            # A ground-connected internal bearing spine supports the interior
            # floor panels; a hollow shell would leave those panels floating.
            strip(lines,28,19,2,7,z)
            strip(lines,32,19,0,14,z)
            for x in [0,30]:strip(lines,12,19,x,2,z,turn=True)
            if z in [0,5]:strip(lines,32,19,0,0,z)
            else:
                for x,part in [(0,"3002"),(5,"3001"),(11,"3001"),(17,"3001"),(23,"3001"),(29,"3002")]:place(lines,part,19,x,0,z)
        for x in range(3,28,6):
            place(lines,"3020",19,x,-1,1,turn=True)
            lines.append(reference("3020.dat",1,((x+1)*20,-3*24,2.4*20),TALL_WINDOW))
        floor_plate(lines,6,15,extended)
        files["storey-balcony-floor.ldr" if extended else "storey.ldr"]=lines
    balcony=[]
    for x in [0,6,12,18,24,31]:place(balcony,"3005",0,x,-2,0)
    strip(balcony,32,0,0,-2,1,rail=True)
    files["balcony.ldr"]=balcony
    for level in range(6):
        z=1/3+level*(6+1/3)
        main.append(reference("storey-balcony-floor.ldr" if level in [1,4,5] else "storey.ldr",16,(0,-z*24,0)))
        if level in [2,5]:main.append(reference("balcony.ldr",0,(0,-z*24,0)))
    roof=1/3+6*(6+1/3)
    for inset in range(4):
        z=roof+inset
        strip(main,32-2*inset,72,inset,inset,z)
        strip(main,32-2*inset,72,inset,14-inset,z)
        for x in [inset,30-inset]:strip(main,12-2*inset,72,x,inset+2,z,turn=True)
    for x in range(4,28,8):
        place(main,"41539",72,x,4,roof+4)
    dormer=[]
    place(dormer,"3710",19,0,-1,0)
    for x in [0,3]:
        place(dormer,"3005",19,x,-1,1/3)
        for z in [4/3,5/3]:place(dormer,"3024",19,x,-1,z)
    dormer.append(reference("3022.dat",1,(2*20,-(7/6)*24,0),WINDOW))
    place(dormer,"2431",19,0,-1,2)
    files["dormer.ldr"]=dormer
    for x in [2,8,14,20,26]:main.append(reference("dormer.ldr",19,(x*20,-roof*24,0)))
    return "\r\n".join("0 FILE "+name+"\r\n"+"\r\n".join(lines) for name,lines in files.items())+"\r\n"


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary",default="ergo-mcp")
    parser.add_argument("--output",type=pathlib.Path,default=pathlib.Path("target/haussmann-mcp"))
    parser.add_argument("--documentation-assets",type=pathlib.Path)
    args=parser.parse_args()
    binary=shutil.which(args.binary)
    if not binary:parser.error("Install ergo-mcp or pass --binary target/debug/ergo-mcp")
    output=args.output.resolve();output.mkdir(parents=True,exist_ok=True)
    workspace=output/"haussmann.ergo.json"
    if workspace.exists():parser.error("Choose a fresh output directory to avoid overwriting a proposal")
    text=model();(output/"haussmann.mpd").write_text(text)
    if args.documentation_assets:
        args.documentation_assets.mkdir(parents=True,exist_ok=True)
        (args.documentation_assets/"haussmann.mpd").write_text(text)
    mcp=demo.Mcp(binary,workspace,"https://ergosandbox.com",None)
    try:
        placed=mcp.call("place_submodel",document=text,color=19)
        validation=mcp.call("validate_construction")["structuredContent"]
        report={"brick_count":validation["brick_count"],"validation":validation,"shared_committed":False,
            "interpretation":"Non-destructive architectural proposal. No alternate hosted world and no guessed connector capacities."}
        listing=mcp.call("list_bricks",limit=200)["structuredContent"]
        counts=Counter(brick["part"] for brick in listing["bricks"])
        offset=200
        while offset<listing["total"]:
            counts.update(brick["part"] for brick in mcp.call("list_bricks",offset=offset,limit=200)["structuredContent"]["bricks"])
            offset+=200
        report["parts"]=dict(sorted(counts.items()))
        for name,camera in [
            ("street",{"position_world":[65,-95,65],"target_world":[16,5,21],"vertical_fov_degrees":38}),
            ("front",{"position_world":[16,-105,24],"target_world":[16,2,22],"vertical_fov_degrees":32}),
        ]:
            image=mcp.call("test_visual_appearance",camera=camera,width=768,height=768,lighting="studio")
            content=next(item for item in image["content"] if item["type"]=="image")
            png=base64.b64decode(content["data"])
            (output/f"haussmann-{name}.png").write_bytes(png)
            if args.documentation_assets:(args.documentation_assets/f"haussmann-{name}.png").write_bytes(png)
        report["gravity"]=mcp.call("test_stability_by_gravity",steps=1)["structuredContent"]
        (output/"report.json").write_text(json.dumps(report,indent=2)+"\n")
        print(json.dumps({"brick_count":report["brick_count"],"overlap_free":validation["overlap_free"],
            "publication_ready":validation["publication_ready"],"failure":validation.get("failure"),
            "calibration_issue":validation.get("calibration_issue"),"output":str(output)},indent=2))
    finally:mcp.close()


if __name__=="__main__":main()
