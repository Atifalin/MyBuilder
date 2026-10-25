class_name WorldLoader
# Reusable world importer: loads chunk GLBs (+ region chunks + water)
# from a manifest and returns the assembled Node3D plus stats.
# `parent` is the node the world is attached under.
static func load_world(manifest_path: String, parent: Node) -> Dictionary:
	var res := {
		"root": null, "chunks": 0, "vertices": 0, "triangles": 0,
		"water": 0, "failures": [], "manifest": {},
	}
	var manifest = JSON.parse_string(FileAccess.get_file_as_string(manifest_path))
	if manifest == null:
		res["failures"].append("manifest is not valid JSON")
		return res
	res["manifest"] = manifest
	var base_dir := manifest_path.get_base_dir()
	var gen2: bool = manifest.has("splat_channels")
	var shader_path := "res://terrain_splat.gdshader"
	var world_root := Node3D.new()
	world_root.name = "LoadedWorld"
	parent.add_child(world_root)
	var work: Array = []
	for c in manifest["chunks"]:
		if not c.has("superseded_by"):
			work.append(c)
	for reg in manifest.get("regions", []):
		for c in reg["chunks"]:
			work.append(c)
	for c in work:
		var glb_path := base_dir.path_join(c["file"])
		var doc := GLTFDocument.new()
		var state := GLTFState.new()
		var err := doc.append_from_file(glb_path, state)
		if err != OK:
			res["failures"].append("append_from_file failed for %s: %s" % [c["file"], error_string(err)])
			continue
		var scene := doc.generate_scene(state)
		if gen2 and c.has("splat"):
			_apply_splat(scene, base_dir.path_join(c["splat"]["file"]), shader_path, c["file"], res)
		else:
			_enable_vertex_colors(scene)
		world_root.add_child(scene)
		res["chunks"] += 1
		res["vertices"] += int(c.get("vertex_count", 0))
		res["triangles"] += int(c.get("triangle_count", 0))
		# gen 3: water surface mesh for this chunk.
		if c.has("water"):
			var wpath := base_dir.path_join(c["water"]["file"])
			var wdoc := GLTFDocument.new()
			var wstate := GLTFState.new()
			if wdoc.append_from_file(wpath, wstate) == OK:
				var wscene := wdoc.generate_scene(wstate)
				var wmat := StandardMaterial3D.new()
				wmat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
				wmat.albedo_color = Color(0.1, 0.35, 0.65, 0.6)
				wmat.roughness = 0.1
				wscene.set_meta("mb_water", true)
				_apply_override(wscene, wmat)
				world_root.add_child(wscene)
				res["water"] += 1
	# Translucent sea plane at manifest sea level, extended past the world
	# extent so the seafloor terrain never shows through the horizon.
	var sea := MeshInstance3D.new()
	var plane := PlaneMesh.new()
	var w: float = manifest["extent"]["width_m"]
	var d: float = manifest["extent"]["depth_m"]
	plane.size = Vector2(w, d) * 2.0
	sea.mesh = plane
	var sea_mat := StandardMaterial3D.new()
	sea_mat.albedo_color = Color(0.05, 0.25, 0.5, 0.7)
	sea_mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	sea_mat.roughness = 0.15
	sea.material_override = sea_mat
	sea.position = Vector3(0, manifest["sea_level_m"], 0)
	sea.set_meta("mb_sea", true)
	world_root.add_child(sea)
	res["root"] = world_root
	return res

static func _apply_override(node: Node, mat: Material) -> void:
	if node is MeshInstance3D:
		node.material_override = mat
	for ch in node.get_children():
		_apply_override(ch, mat)

static func _enable_vertex_colors(node: Node) -> void:
	if node is MeshInstance3D:
		var mi: MeshInstance3D = node
		for si in mi.mesh.get_surface_count():
			var mat := mi.get_active_material(si)
			if mat is StandardMaterial3D:
				mat.vertex_color_use_as_albedo = true
	for child in node.get_children():
		_enable_vertex_colors(child)

static func _apply_splat(node: Node, png_path: String, shader_path: String, file: String, res: Dictionary) -> void:
	var img := Image.new()
	if img.load(png_path) != OK:
		res["failures"].append("splat png failed: " + png_path)
		return
	var tex := ImageTexture.create_from_image(img)
	var mat := ShaderMaterial.new()
	mat.shader = load(shader_path)
	mat.set_shader_parameter("splat", tex)
	if node is MeshInstance3D:
		node.material_override = mat
	for ch in node.get_children():
		_apply_splat(ch, png_path, shader_path, file, res)
