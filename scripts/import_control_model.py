#!/usr/bin/env python3
"""Convert credited GLB/glTF assets to the compact terminal mesh format.
AirPods: python3 scripts/import_control_model.py source.glb output.mesh
Others: uv run --with pillow scripts/import_control_model.py source.glb output.mesh sony
        (also accepts a Sketchfab glTF zip; use 'mouse' for the MX Master scan)
Source credits and modifications: assets/control/SOURCES.md.
"""
import json
import math
import struct
import sys
import io
import zipfile
from collections import defaultdict
from pathlib import Path

source = Path(sys.argv[1])
kind = sys.argv[3] if len(sys.argv) > 3 else "airpods"
if source.suffix == ".zip":
    archive = zipfile.ZipFile(source)
    doc = json.loads(archive.read("scene.gltf"))
    blobs = [archive.read(b["uri"]) for b in doc["buffers"]]
    read_image = lambda i: archive.read(doc["images"][i]["uri"])
else:
    raw = source.read_bytes()
    assert raw[:4] == b"glTF"
    size = struct.unpack_from("<I", raw, 12)[0]
    doc = json.loads(raw[20:20 + size])
    blobs = [raw[28 + size:]]
    def read_image(i):
        v = doc["bufferViews"][doc["images"][i]["bufferView"]]
        return blobs[v.get("buffer", 0)][v.get("byteOffset", 0):v.get("byteOffset", 0) + v["byteLength"]]

def accessor(index):
    a = doc["accessors"][index]
    v = doc["bufferViews"][a["bufferView"]]
    blob = blobs[v.get("buffer", 0)]
    fmt = {5126: "f", 5125: "I", 5123: "H", 5121: "B"}[a["componentType"]]
    count = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[a["type"]]
    stride = v.get("byteStride", struct.calcsize(fmt) * count)
    offset = v.get("byteOffset", 0) + a.get("byteOffset", 0)
    return [struct.unpack_from("<" + fmt * count, blob, offset + i * stride) for i in range(a["count"])]

def unit(v):
    length = max(1e-12, math.sqrt(sum(x * x for x in v)))
    return [x / length for x in v]

IDENTITY = [1.,0,0,0, 0,1.,0,0, 0,0,1.,0, 0,0,0,1.]
def multiply(a, b):
    return [sum(a[k * 4 + row] * b[col * 4 + k] for k in range(4)) for col in range(4) for row in range(4)]
def matrix(node):
    if "matrix" in node: return node["matrix"]
    x,y,z,w = node.get("rotation", [0,0,0,1])
    sx,sy,sz = node.get("scale", [1,1,1])
    tx,ty,tz = node.get("translation", [0,0,0])
    return [(1-2*y*y-2*z*z)*sx, (2*x*y+2*z*w)*sx, (2*x*z-2*y*w)*sx,0,
            (2*x*y-2*z*w)*sy, (1-2*x*x-2*z*z)*sy, (2*y*z+2*x*w)*sy,0,
            (2*x*z+2*y*w)*sz, (2*y*z-2*x*w)*sz, (1-2*x*x-2*y*y)*sz,0, tx,ty,tz,1]
def point(m, p):
    return [sum(m[k*4+j]*p[k] for k in range(3)) + m[12+j] for j in range(3)]
def normal(m, n):
    # Inverse-transpose, including nonuniform scales in glTF node hierarchies.
    a,b,c,d,e,f,g,h,i = [m[j] for j in [0,4,8,1,5,9,2,6,10]]
    cofactor = [e*i-f*h, f*g-d*i, d*h-e*g, c*h-b*i, a*i-c*g, b*g-a*h, b*f-c*e, c*d-a*f, a*e-b*d]
    det = a*cofactor[0] + b*cofactor[1] + c*cofactor[2]
    return unit([sum(cofactor[row*3+k]*n[k] for k in range(3)) / (det or 1) for row in range(3)])

vertices, triangles, lookup = [], [], {}
sums = defaultdict(lambda: [0.] * 8)
def add_primitive(positions, normals, indices, albedos, part, material, step):
    remap = []
    for p,n,albedo in zip(positions, normals, albedos):
        key = (part, material, *(round(x / step) for x in p))
        if key not in lookup:
            lookup[key] = len(vertices)
            vertices.append((part, key))
        values = sums[key]
        for i in range(3): values[i] += p[i]; values[i+3] += n[i]
        values[6] += albedo
        values[7] += 1
        remap.append(lookup[key])
    for i in range(0, len(indices), 3):
        face = tuple(remap[j] for j in indices[i:i+3])
        if len(set(face)) == 3: triangles.append(face)

if kind == "airpods":
    # Source-specific assemblies: left/right earbuds, hinge lid, case body.
    for node, part in [(4, 1), (11, 2), (18, 3), (23, 0)]:
        children = doc["nodes"][node]["children"]
        primitives = [p for c in children for p in doc["meshes"][doc["nodes"][c]["mesh"]]["primitives"]]
        points = [v for p in primitives for v in accessor(p["attributes"]["POSITION"])]
        center = [(min(p[i] for p in points)+max(p[i] for p in points))/2 for i in range(3)]
        for p in primitives:
            positions = accessor(p["attributes"]["POSITION"])
            normals = accessor(p["attributes"]["NORMAL"])
            material = p.get("material", 2)
            albedo = .13 if material in (0,3,4) else .95 if material == 2 else .55
            def transform(v):
                if part in (1,2):
                    return [(v[0]-center[0])*.34+(-1.35 if part==1 else 1.35), -(v[2]-center[2])*.34+.8, (v[1]-center[1])*.34+.15]
                return [v[0]*.36, -v[2]*.36-.3, v[1]*.36]
            add_primitive([transform(v) for v in positions], [[n[0],-n[2],n[1]] for n in normals],
                          [v[0] for v in accessor(p["indices"])], [albedo]*len(positions), part, material, .018)
else:
    from PIL import Image
    textures = {}
    parts = []
    def visit(index, parent):
        node = doc["nodes"][index]
        m = multiply(parent, matrix(node))
        if "mesh" in node:
            for p in doc["meshes"][node["mesh"]]["primitives"]:
                assert p.get("mode", 4) == 4, "Triangles required"
                positions = [point(m, v) for v in accessor(p["attributes"]["POSITION"])]
                normals = [normal(m, v) for v in accessor(p["attributes"]["NORMAL"])]
                material = p.get("material", 0)
                props = doc.get("materials", [{}])[material].get("pbrMetallicRoughness", {})
                color = props.get("baseColorFactor", [1,1,1,1])
                albedos = [.45 + .5*math.sqrt(sum(color[:3])/3)] * len(positions)
                if "baseColorTexture" in props and "TEXCOORD_0" in p["attributes"]:
                    tex = props["baseColorTexture"]["index"]
                    if tex not in textures:
                        textures[tex] = Image.open(io.BytesIO(read_image(doc["textures"][tex]["source"]))).convert("RGB")
                    image = textures[tex]
                    albedos = []
                    for uv in accessor(p["attributes"]["TEXCOORD_0"]):
                        pixel = image.getpixel((min(image.width-1,max(0,int(uv[0]*image.width))),min(image.height-1,max(0,int(uv[1]*image.height)))))
                        albedos.append(.18 + .77*(sum(pixel)/765)**.7)
                indices = [v[0] for v in accessor(p["indices"])] if "indices" in p else list(range(len(positions)))
                parts.append((positions, normals, indices, albedos, material))
        for child in node.get("children", []): visit(child,m)
    for root in doc["scenes"][doc.get("scene",0)]["nodes"]: visit(root,IDENTITY)
    points = [p for part in parts for p in part[0]]
    minimum = [min(p[i] for p in points) for i in range(3)]
    maximum = [max(p[i] for p in points) for i in range(3)]
    center = [(a+b)/2 for a,b in zip(minimum,maximum)]
    scale = 3 / max(b-a for a,b in zip(minimum,maximum))
    print("source bounds", minimum,maximum)
    for positions,normals,indices,albedos,material in parts:
        positions = [[(v-c)*scale for v,c in zip(p,center)] for p in positions]
        if kind == "sony":
            positions = [[p[2],p[1],-p[0]] for p in positions]
            normals = [[n[2],n[1],-n[0]] for n in normals]
        add_primitive(positions,normals,indices,albedos,0,material,.03 if kind=="mouse" else .012)

triangles = list(dict.fromkeys(triangles))
with open(sys.argv[2], "wb") as out:
    out.write(struct.pack("<4sII", b"OSM1", len(vertices), len(triangles)))
    for part, key in vertices:
        s = sums[key]
        out.write(struct.pack("<7fI", *(v/s[7] for v in s[:3]), *unit(s[3:6]), s[6]/s[7], part))
    for triangle in triangles: out.write(struct.pack("<3I", *triangle))
print(f"{len(vertices)} vertices, {len(triangles)} triangles → {sys.argv[2]}")
