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
	for h in m.get("houses", []):
		var node := Node3D.new()
		node.name = h["id"]
		node.set_meta("mb_house_id", h["id"])
		var pl = h.get("placement")
		if pl != null:
			node.position = Vector3(pl["x"], 0.0, pl["z"])
			node.rotation.y = -PI / 2.0 * float(pl["rot_quarter"])
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
	return out

static func _first_mesh(n: Node) -> Mesh:
	if n is MeshInstance3D:
		return n.mesh
	for c in n.get_children():
		var m := _first_mesh(c)
		if m:
			return m
	return null
