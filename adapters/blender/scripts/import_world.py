"""S3 check: import chunk GLBs and verify world coverage at scale.

Run: Blender --background --factory-startup --python import_world.py -- \
    <abs manifest path> [--blend <out>]
"""

import json
import sys

import bpy
from mathutils import Vector


def wire_color_attribute(obj):
    """glTF COLOR_0 should feed base color; wire a Color Attribute node
    if the importer left it unconnected."""
    if obj.type != 'MESH' or not obj.material_slots:
        return
    for slot in obj.material_slots:
        mat = slot.material
        if not mat or not mat.use_nodes:
            continue
        bsdf = next((n for n in mat.node_tree.nodes if n.type == 'BSDF_PRINCIPLED'), None)
        if bsdf is None or bsdf.inputs['Base Color'].is_linked:
            continue
        attr = mat.node_tree.nodes.new('ShaderNodeVertexColor')
        mat.node_tree.links.new(attr.outputs['Color'], bsdf.inputs['Base Color'])


def main() -> int:
    argv = sys.argv[sys.argv.index("--") + 1:]
    manifest_path = argv[argv.index("--manifest") + 1] if "--manifest" in argv else argv[0]
    blend_out = argv[argv.index("--blend") + 1] if "--blend" in argv else None

    with open(manifest_path) as f:
        manifest = json.load(f)

    import os.path

    base = os.path.dirname(manifest_path)
    chunks = manifest["chunks"]
    h_min = manifest["h_min"]
    h_max = manifest["h_max"]
    step = (h_max - h_min) / 65535.0
    w = manifest["extent"]["width_m"]
    d = manifest["extent"]["depth_m"]

    failures = []
    imported = []
    for c in chunks:
        path = os.path.join(base, c["file"])
        before = set(bpy.data.objects)
        try:
            bpy.ops.import_scene.gltf(filepath=path)
        except Exception as e:  # noqa: BLE001
            failures.append(f"gltf import failed for {c['file']}: {e}")
            continue
        new_objs = [o for o in bpy.data.objects if o not in before]
        for obj in new_objs:
            wire_color_attribute(obj)
        imported.extend(new_objs)

    if len(imported) != len(chunks):
        failures.append(f"imported {len(imported)} objects, manifest lists {len(chunks)}")

    # Combined world-space bounding box. glTF Y-up -> Blender Z-up, so
    # glTF z maps to Blender -y.
    eps = 0.01
    if imported:
        corners = [
            obj.matrix_world @ Vector(corner)
            for obj in imported
            for corner in obj.bound_box
        ]
        xs = [c.x for c in corners]
        ys = [c.y for c in corners]
        zs = [c.z for c in corners]
        if abs(min(xs) - (-w / 2.0)) > eps or abs(max(xs) - w / 2.0) > eps:
            failures.append(f"x span {min(xs)}..{max(xs)} != extent +/-{w/2.0}")
        # glTF z in [-d/2, d/2] maps to Blender y in [-d/2, d/2] (sign flip
        # is symmetric around the origin).
        if abs(min(ys) - (-d / 2.0)) > eps or abs(max(ys) - d / 2.0) > eps:
            failures.append(f"y span {min(ys)}..{max(ys)} != extent +/-{d/2.0}")
        if min(zs) < h_min - step - eps or max(zs) > h_max + step + eps:
            failures.append(f"z range {min(zs)}..{max(zs)} outside {h_min}..{h_max}")

    if blend_out:
        bpy.ops.wm.save_as_mainfile(filepath=blend_out)
        print("saved", blend_out)

    if failures:
        for f_ in failures:
            print("S3 BLENDER FAIL:", f_)
        return 1
    print("S3 BLENDER PASS")
    return 0


sys.exit(main())
