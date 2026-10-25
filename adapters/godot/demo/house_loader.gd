class_name HouseLoader
extends RefCounted

# Load exports/houses/manifest.json; spawn a Node3D per house with 3
# LOD MeshInstance3Ds + a StaticBody3D of box colliders.
static func load_houses(manifest_path: String, parent: Node3D) -> Dictionary:
	var out := {"houses": [], "colliders": 0}
	var f := FileAccess.open(manifest_path, FileAccess.READ)
	if f == null:
		return out
	var m: Dictionary = JSON.parse_string(f.get_as_text())
	var base := manifest_path.get_base_dir()
	var part_xf := {}
	var row_x := 0.0
	var row_total := -6.0
	for h in m.get("houses", []):
		if h.get("placement") == null:
			row_total += float(h["bounds"]["max"][0]) - float(h["bounds"]["min"][0]) + 6.0
	var ranges_all := [[0.0, 60.0], [60.0, 250.0]]
	for h in m.get("houses", []):
		var node := Node3D.new()
		node.name = h["id"]
		node.set_meta("mb_house_id", h["id"])
		var pl = h.get("placement")
		if pl != null:
			node.position = Vector3(pl["x"], 0.0, pl["z"])
			node.rotation.y = -PI / 2.0 * float(pl["rot_quarter"])
		else:
			# Unplaced houses: a row along +X centred on the origin, 6 m apart.
			var bmin: Array = h["bounds"]["min"]
			var bmax: Array = h["bounds"]["max"]
			node.position.x = row_x - float(bmin[0]) - row_total / 2.0
			row_x += float(bmax[0]) - float(bmin[0]) + 6.0
		var files: Dictionary = h["files"]
		var ranges := [[0.0, 60.0], [60.0, 250.0], [250.0, 0.0]]
		for li in 3:
			var rel: String = files["lod%d" % li]
			var path := base + "/../" + rel
			var doc := GLTFDocument.new()
			var st := GLTFState.new()
			var err := doc.append_from_file(path, st)
			if err != OK:
				printerr("house_loader: append %s -> %s" % [path, err])
				continue
			var sc := doc.generate_scene(st)
			var mi := MeshInstance3D.new()
			mi.mesh = _first_mesh(sc)
			mi.visibility_range_begin = ranges[li][0]
			mi.visibility_range_end = ranges[li][1]
			mi.visibility_range_end_margin = 5.0
			mi.name = "lod%d" % li
			node.add_child(mi)
		var body := StaticBody3D.new()
		for c in h.get("colliders", []):
			var sh := CollisionShape3D.new()
			var bx := BoxShape3D.new()
			bx.size = Vector3(c["half"][0] * 2.0, c["half"][1] * 2.0, c["half"][2] * 2.0)
			sh.shape = bx
			sh.position = Vector3(c["center"][0], c["center"][1], c["center"][2])
			body.add_child(sh)
			out["colliders"] += 1
		node.add_child(body)
		parent.add_child(node)
		out["houses"].append(node)
		# Gather part instances (house transform x instance transform).
		for inst in h.get("instances", []):
			var t: Array = inst["transform"]
			var local := Transform3D(
				Basis(Vector3(t[0], t[1], t[2]), Vector3(t[3], t[4], t[5]), Vector3(t[6], t[7], t[8])),
				Vector3(t[9], t[10], t[11]))
			for li in 2:
				if int(inst["lods"]) & (1 << li):
					var k := "%s/%d" % [inst["key"], li]
					if not part_xf.has(k):
						part_xf[k] = []
					part_xf[k].append(node.transform * local)
	# One MultiMeshInstance3D per part key x LOD, shared by every house.
	var parts_node := Node3D.new()
	parts_node.name = "Parts"
	parent.add_child(parts_node)
	out["parts"] = parts_node
	out["part_instances"] = {}
	for pt in m.get("parts", []):
		for li in 2:
			var k := "%s/%d" % [pt["key"], li]
			if not part_xf.has(k):
				continue
			var doc := GLTFDocument.new()
			var st := GLTFState.new()
			var path := base + "/../" + String(pt["files"]["lod%d" % li])
			if doc.append_from_file(path, st) != OK:
				printerr("house_loader: append %s failed" % path)
				continue
			var mm := MultiMesh.new()
			mm.transform_format = MultiMesh.TRANSFORM_3D
			mm.mesh = _first_mesh(doc.generate_scene(st))
			var xs: Array = part_xf[k]
			mm.instance_count = xs.size()
			for i in xs.size():
				mm.set_instance_transform(i, xs[i])
			var mmi := MultiMeshInstance3D.new()
			mmi.name = "%s_lod%d" % [pt["key"], li]
			mmi.multimesh = mm
			mmi.visibility_range_begin = ranges_all[li][0]
			mmi.visibility_range_end = ranges_all[li][1]
			mmi.visibility_range_end_margin = 5.0
			parts_node.add_child(mmi)
			out["part_instances"][k] = xs.size()
	return out

static func _first_mesh(n: Node) -> Mesh:
	if n is MeshInstance3D:
		return n.mesh
	for c in n.get_children():
		var m := _first_mesh(c)
		if m:
			return m
	return null
