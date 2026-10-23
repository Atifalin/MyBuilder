extends SceneTree

# S3 check: load chunk GLBs at runtime (no editor import) and verify
# they cover the world extent at the right scale.
# Usage: --script res://import_world.gd -- --manifest <abs path> [--png <out>]
#
# With --png it also spawns a camera + light, waits a few frames, saves a
# screenshot and quits (needs a windowed run, not --headless).

var failures: Array = []

func _initialize() -> void:
	var args := OS.get_cmdline_user_args()
	var manifest_path := ""
	var png_path := ""
	for i in args.size():
		if args[i] == "--manifest" and i + 1 < args.size():
			manifest_path = args[i + 1]
		if args[i] == "--png" and i + 1 < args.size():
			png_path = args[i + 1]
	if manifest_path == "":
		printerr("usage: --manifest <abs path> [--png <out>]")
		quit(1)
		return

	var manifest = JSON.parse_string(FileAccess.get_file_as_string(manifest_path))
	if manifest == null:
		failures.append("manifest is not valid JSON")
		finish(png_path, manifest)
		return

	var base_dir := manifest_path.get_base_dir()
	var chunks: Array = manifest["chunks"]
	var h_min: float = manifest["h_min"]
	var h_max: float = manifest["h_max"]
	var step: float = (h_max - h_min) / 65535.0
	var w: float = manifest["extent"]["width_m"]
	var d: float = manifest["extent"]["depth_m"]

	var world_root := Node3D.new()
	world_root.name = "S3Terrain"
	root.add_child(world_root)

	var have_union := false
	var union := AABB()
	var loaded := 0
	var extras_seen := 0
	for c in chunks:
		var glb_path := base_dir.path_join(c["file"])
		var doc := GLTFDocument.new()
		var state := GLTFState.new()
		var err := doc.append_from_file(glb_path, state)
		if err != OK:
			failures.append("append_from_file failed for %s: %s" % [c["file"], error_string(err)])
			continue
		var scene := doc.generate_scene(state)
		enable_vertex_colors(scene)
		world_root.add_child(scene)
		loaded += 1
		var aabb: AABB = scene.transform * subtree_aabb(scene)
		if not have_union:
			union = aabb
			have_union = true
		else:
			union = union.merge(aabb)
		# Chunk AABB must lie inside the world extent and the height range.
		var e := 0.01
		if aabb.position.x < -w / 2.0 - e or aabb.end.x > w / 2.0 + e \
			or aabb.position.z < -d / 2.0 - e or aabb.end.z > d / 2.0 + e:
			failures.append("%s out of extent: %s" % [c["file"], aabb])
		if aabb.position.y < h_min - step - e or aabb.end.y > h_max + step + e:
			failures.append("%s out of height range: %s" % [c["file"], aabb])
		# Extras: node meta "extras" if the runtime importer keeps them.
		if find_extras(scene):
			extras_seen += 1

	if loaded != chunks.size():
		failures.append("loaded %d chunks, manifest lists %d" % [loaded, chunks.size()])
	if have_union:
		var e2 := 0.01
		if abs(union.position.x - (-w / 2.0)) > e2 or abs(union.end.x - w / 2.0) > e2 \
			or abs(union.position.z - (-d / 2.0)) > e2 or abs(union.end.z - d / 2.0) > e2:
			failures.append("union AABB does not span the world: %s" % union)
	else:
		failures.append("no chunks loaded")
	print("S3: extras present on %d/%d chunks" % [extras_seen, loaded])
	finish(png_path, {"w": w, "d": d, "h_max": h_max, "sea": manifest["sea_level_m"]})

func enable_vertex_colors(node: Node) -> void:
	# glTF COLOR_0 is supposed to feed base color, but Godot's runtime
	# importer leaves it off; turn it on per surface material.
	if node is MeshInstance3D:
		var mi: MeshInstance3D = node
		for si in mi.mesh.get_surface_count():
			var mat := mi.get_active_material(si)
			if mat is StandardMaterial3D:
				mat.vertex_color_use_as_albedo = true
	for child in node.get_children():
		enable_vertex_colors(child)

func subtree_aabb(node: Node) -> AABB:
	# Returns the AABB in `node`'s local space. Each child's own transform
	# is applied HERE (once) by the parent loop, not inside the child call.
	var aabb := AABB()
	var have := false
	if node is MeshInstance3D:
		aabb = node.get_aabb()
		have = true
	for child in node.get_children():
		var c := subtree_aabb(child)
		if child is Node3D:
			c = child.transform * c
		if not have:
			aabb = c
			have = true
		else:
			aabb = aabb.merge(c)
	return aabb

func find_extras(node: Node) -> bool:
	if node.has_meta("extras") or node.get("extras") != null:
		return true
	for child in node.get_children():
		if find_extras(child):
			return true
	return false

func finish(png_path: String, ctx) -> void:
	if failures.is_empty():
		print("S3 GODOT PASS")
	else:
		for f in failures:
			print("S3 GODOT FAIL: ", f)
	if png_path != "" and failures.is_empty():
		await render_png(png_path, ctx)
	quit(0 if failures.is_empty() else 1)

func render_png(png_path: String, ctx) -> void:
	var w: float = ctx["w"]
	var d: float = ctx["d"]
	var h_max: float = ctx["h_max"]
	var sea: float = ctx["sea"]
	# Flat translucent sea plane at sea level.
	var sea_mesh := MeshInstance3D.new()
	var plane := PlaneMesh.new()
	plane.size = Vector2(w, d)
	sea_mesh.mesh = plane
	var sea_mat := StandardMaterial3D.new()
	sea_mat.albedo_color = Color(0.05, 0.25, 0.5, 0.7)
	sea_mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	sea_mat.roughness = 0.15
	sea_mesh.material_override = sea_mat
	sea_mesh.position = Vector3(0, sea, 0)
	root.add_child(sea_mesh)
	var cam := Camera3D.new()
	# 45 degrees above the world centre, far enough to see it all.
	var dist: float = max(w, d) * 1.1
	cam.position = Vector3(0, dist * 0.75 + h_max, dist * 0.75)
	cam.far = dist * 4.0
	root.add_child(cam)
	# look_at_from_position works even before the node is in the tree.
	cam.look_at_from_position(cam.position, Vector3(0, 0, 0), Vector3.UP)
	cam.current = true
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50, -30, 0)
	root.add_child(light)
	var env := WorldEnvironment.new()
	var e := Environment.new()
	var sky := Sky.new()
	sky.sky_material = ProceduralSkyMaterial.new()
	e.background_mode = Environment.BG_SKY
	e.sky = sky
	env.environment = e
	root.add_child(env)
	for i in 10:
		await process_frame
	var img := get_root().get_texture().get_image()
	DirAccess.make_dir_recursive_absolute(png_path.get_base_dir())
	img.save_png(png_path)
	print("saved ", png_path)
