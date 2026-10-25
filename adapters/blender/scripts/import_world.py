"""S3 check: import chunk GLBs and verify world coverage at scale.

Run: Blender --background --factory-startup --python import_world.py -- \
    <abs manifest path> [--blend <out>]
"""

import json
import sys

import bpy
from mathutils import Vector


PALETTE = {
    'grass': (0.35, 0.55, 0.28, 1.0),
    'sand': (0.87, 0.79, 0.56, 1.0),
    'rock': (0.45, 0.43, 0.40, 1.0),
    'snow': (0.92, 0.93, 0.95, 1.0),
}


def wire_splat_material(obj, splat_path):
    """Gen 2: build a material that samples the splat PNG on UV layer 2
    and blends the 4 palette colors by RGBA weights."""
    if obj.type != 'MESH' or not obj.material_slots:
        return
    img = bpy.data.images.load(splat_path)
    img.colorspace_settings.name = 'Non-Color'
    img.alpha_mode = 'CHANNEL_PACKED'
    for slot in obj.material_slots:
        mat = slot.material
        if not mat:
            continue
        mat.use_nodes = True
        nt = mat.node_tree
        bsdf = next((n for n in nt.nodes if n.type == 'BSDF_PRINCIPLED'), None)
        if bsdf is None:
            continue
        for n in list(nt.nodes):
            if n.type == 'BSDF_PRINCIPLED' or n.type == 'OUTPUT_MATERIAL':
                continue
            nt.nodes.remove(n)
        tex = nt.nodes.new('ShaderNodeTexImage')
        tex.image = img
        uvmap = nt.nodes.new('ShaderNodeUVMap')
        # Second UV layer = TEXCOORD_1 (splat).
        uvmap.uv_map = obj.data.uv_layers[1].name if len(obj.data.uv_layers) > 1 else obj.data.uv_layers[0].name
        nt.links.new(uvmap.outputs['UV'], tex.inputs['Vector'])
        sep = nt.nodes.new('ShaderNodeSeparateColor')
        nt.links.new(tex.outputs['Color'], sep.inputs['Color'])
        # weighted palette blend: col = grass*R + sand*G + rock*B + snow*A
        # Each MIX(black, palette, fac=w) yields w*palette; the ADDs sum them.
        add1 = nt.nodes.new('ShaderNodeMixRGB'); add1.blend_type='MIX'; add1.inputs[0].default_value=1.0
        add1.inputs[1].default_value = (0.0, 0.0, 0.0, 1.0)
        add1.inputs[2].default_value = PALETTE['grass']
        add2 = nt.nodes.new('ShaderNodeMixRGB'); add2.blend_type='MIX'; add2.inputs[0].default_value=1.0
        add2.inputs[1].default_value = (0.0, 0.0, 0.0, 1.0)
        add2.inputs[2].default_value = PALETTE['sand']
        add3 = nt.nodes.new('ShaderNodeMixRGB'); add3.blend_type='MIX'; add3.inputs[0].default_value=1.0
        add3.inputs[1].default_value = (0.0, 0.0, 0.0, 1.0)
        add3.inputs[2].default_value = PALETTE['rock']
        add4 = nt.nodes.new('ShaderNodeMixRGB'); add4.blend_type='MIX'; add4.inputs[0].default_value=1.0
        add4.inputs[1].default_value = (0.0, 0.0, 0.0, 1.0)
        add4.inputs[2].default_value = PALETTE['snow']
        nt.links.new(sep.outputs['Red'], add1.inputs[0])
        nt.links.new(sep.outputs['Green'], add2.inputs[0])
        nt.links.new(sep.outputs['Blue'], add3.inputs[0])
        nt.links.new(tex.outputs['Alpha'], add4.inputs[0])
        s1 = nt.nodes.new('ShaderNodeMixRGB'); s1.blend_type='ADD'; s1.inputs[0].default_value=1.0
        s2 = nt.nodes.new('ShaderNodeMixRGB'); s2.blend_type='ADD'; s2.inputs[0].default_value=1.0
        s3 = nt.nodes.new('ShaderNodeMixRGB'); s3.blend_type='ADD'; s3.inputs[0].default_value=1.0
        nt.links.new(add1.outputs[0], s1.inputs[1]); nt.links.new(add2.outputs[0], s1.inputs[2])
        nt.links.new(add3.outputs[0], s2.inputs[1]); nt.links.new(add4.outputs[0], s2.inputs[2])
        nt.links.new(s1.outputs[0], s3.inputs[1]); nt.links.new(s2.outputs[0], s3.inputs[2])
        nt.links.new(s3.outputs[0], bsdf.inputs['Base Color'])


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


def water_material():
    """Gen-3 water: Principled transmission + COLOR_0-driven colour ramp
    (depth01 in R: shallow turquoise -> deep blue; kind tints in G) and
    a Noise->Bump ripple. Blender 5.2 EEVEE transparency uses
    surface_render_method."""
    m = bpy.data.materials.get("MBWater")
    if m:
        return m
    m = bpy.data.materials.new("MBWater")
    m.use_nodes = True
    try:
        m.surface_render_method = 'DITHERED'
    except Exception:  # noqa: BLE001
        try:
            m.blend_method = 'BLEND'
        except Exception:  # noqa: BLE001
            pass
    nt = m.node_tree
    bsdf = next(n for n in nt.nodes if n.type == 'BSDF_PRINCIPLED')
    bsdf.inputs["Transmission Weight"].default_value = 1.0
    bsdf.inputs["IOR"].default_value = 1.33
    bsdf.inputs["Roughness"].default_value = 0.05
    # COLOR_0 -> Separate Color: R drives a shallow->deep ramp.
    attr = nt.nodes.new('ShaderNodeVertexColor')
    attr.layer_name = "Color"
    sep = nt.nodes.new('ShaderNodeSeparateColor')
    ramp = nt.nodes.new('ShaderNodeValToRGB')
    ramp.color_ramp.elements[0].color = (0.25, 0.75, 0.80, 1.0)
    ramp.color_ramp.elements[1].color = (0.02, 0.12, 0.35, 1.0)
    nt.links.new(attr.outputs["Color"], sep.inputs["Color"])
    nt.links.new(sep.outputs["Red"], ramp.inputs["Fac"])
    # kind tint: G < 0.25 sea (bluer), ~0.5 lake (greener), ~1 river (teal)
    kind = nt.nodes.new('ShaderNodeValToRGB')
    kind.color_ramp.elements[0].color = (0.7, 0.85, 1.2, 1.0)
    kind.color_ramp.elements[0].position = 0.0
    kind.color_ramp.elements[1].color = (0.3, 0.85, 0.8, 1.0)
    kind.color_ramp.elements[1].position = 1.0
    mid = kind.color_ramp.elements.new(0.5)
    mid.color = (0.9, 1.1, 0.95, 1.0)
    nt.links.new(sep.outputs["Green"], kind.inputs["Fac"])
    tint = nt.nodes.new('ShaderNodeMixRGB')
    tint.blend_type = 'MULTIPLY'
    tint.inputs[0].default_value = 1.0
    nt.links.new(ramp.outputs["Color"], tint.inputs[1])
    nt.links.new(kind.outputs["Color"], tint.inputs[2])
    nt.links.new(tint.outputs["Color"], bsdf.inputs["Base Color"])
    # ripple bump
    noise = nt.nodes.new('ShaderNodeTexNoise')
    noise.inputs["Scale"].default_value = 0.4
    noise.inputs["Detail"].default_value = 3.0
    bump = nt.nodes.new('ShaderNodeBump')
    bump.inputs["Strength"].default_value = 0.25
    bump.inputs["Distance"].default_value = 0.5
    nt.links.new(noise.outputs["Fac"], bump.inputs["Height"])
    nt.links.new(bump.outputs["Normal"], bsdf.inputs["Normal"])
    return m



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
    water_objs = []
    work = [c for c in chunks if "superseded_by" not in c]
    for reg in manifest.get("regions", []):
        work.extend(reg["chunks"])
    for c in work:
        path = os.path.join(base, c["file"])
        before = set(bpy.data.objects)
        try:
            bpy.ops.import_scene.gltf(filepath=path)
        except Exception as e:  # noqa: BLE001
            failures.append(f"gltf import failed for {c['file']}: {e}")
            continue
        new_objs = [o for o in bpy.data.objects if o not in before]
        for obj in new_objs:
            if "splat_channels" in manifest and "splat" in c:
                wire_splat_material(obj, os.path.join(base, c["splat"]["file"]))
            else:
                wire_color_attribute(obj)
        # gen 3: water surface chunk
        if "water" in c:
            wpath = os.path.join(base, c["water"]["file"])
            before2 = set(bpy.data.objects)
            try:
                bpy.ops.import_scene.gltf(filepath=wpath)
            except Exception as e:  # noqa: BLE001
                failures.append(f"water gltf import failed for {c['file']}: {e}")
            for obj in [o for o in bpy.data.objects if o not in before2]:
                if obj.type == 'MESH':
                    obj.data.materials.clear()
                    obj.data.materials.append(water_material())
                    water_objs.append(obj)
        imported.extend(new_objs)
        if "splat_channels" in manifest and "splat" in c:
            for obj in new_objs:
                if obj.type == 'MESH' and obj.material_slots:
                    m = obj.material_slots[0].material
                    if m is None or not any(
                        n.type == 'TEX_IMAGE' for n in m.node_tree.nodes
                    ):
                        failures.append(f"{c['file']}: splat material missing")
                    img = bpy.data.images.get(os.path.basename(c["splat"]["file"]))
                    if img is None or img.size[0] == 0:
                        failures.append(f"{c['file']}: splat image not loaded")

    # gen 3: sea ring
    if "sea" in manifest:
        spath = os.path.join(base, manifest["sea"]["file"])
        before3 = set(bpy.data.objects)
        try:
            bpy.ops.import_scene.gltf(filepath=spath)
        except Exception as e:  # noqa: BLE001
            failures.append(f"sea ring import failed: {e}")
        for obj in [o for o in bpy.data.objects if o not in before3]:
            if obj.type == 'MESH':
                obj.data.materials.clear()
                obj.data.materials.append(water_material())
                water_objs.append(obj)

    if len(imported) != len(work):
        failures.append(f"imported {len(imported)} objects, manifest lists {len(work)}")

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

    # gen 3: every water object must use the MBWater material.
    if "sea" in manifest or any(c.get("water") for c in work):
        for obj in water_objs:
            ok = any(
                s.material and s.material.name == "MBWater"
                for s in obj.material_slots
            )
            if not ok:
                failures.append(f"{obj.name}: water material missing")

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
