class_name Terrain3DImport
extends SceneTree
# Terrain3D import trial (v1.0.2-stable, Godot 4.4+; verified on 4.7.1).
#
# Standalone verify: Godot --headless --path demo --script terrain3d_import.gd -- --layers <layers.json>
# From fly_world.gd: await Terrain3DImport.import_into(layers_json, self)
#
# Conventions found (v1.0.2):
# - Terrain3DUtil.load_image(path, cache_mode, r16_range, r16_size) decodes a
#   raw little-endian u16 .r16 into an RF image scaled to r16_range.
# - terrain.data is READ-ONLY and only exists one frame after the node enters
#   the tree (data/assets are created on the first idle frame).
# - data.import_images([height, control, color], Vector3 pos, offset, scale):
#   pos is the world-space min corner of the image, SNAPPED DOWN to the
#   region grid (256 m at the default region size). Verified: a pos of -128
#   placed pixel (0,0) at world -256. Sample (i,j) lands at
#   snapped_pos + (i,j)*vertex_spacing.
# - update_maps() must be called after import before get_height works;
#   get_height at a position with no region returns NaN.
# - Terrain3D._physics_process wants an active camera (set_camera or any
#   Camera3D.current); harmless headless, needed for windowed runs.

static func import_into(layers_json: String, parent: Node, tree: SceneTree) -> Dictionary:
	var res := {"ok": false, "terrain": null, "error": "", "meta": {}}
	var meta = JSON.parse_string(FileAccess.get_file_as_string(layers_json))
	if meta == null:
		res["error"] = "layers.json parse failed"
		return res
	res["meta"] = meta
	var base := layers_json.get_base_dir()
	var r16 := base.path_join(meta["files"]["height_terrain3d"])
	var hmin: float = meta["h_min"]
	var hmax: float = meta["h_max"]
	var nx: int = meta["nx"]
	var nz: int = meta["nz"]
	# height_terrain3d.r16 is the CELL grid: (nx-1) x (nz-1) u16 samples.
	var r16_size := Vector2i(nx - 1, nz - 1)
	var cell: float = meta["cell_size_m"]
	var ox: float = meta["origin"][0]
	var oz: float = meta["origin"][1]
	if not ClassDB.can_instantiate("Terrain3D"):
		res["error"] = "Terrain3D class missing (extension not loaded)"
		return res
	var terrain: Node3D = ClassDB.instantiate("Terrain3D")
	terrain.vertex_spacing = cell
	parent.add_child(terrain)
	await tree.process_frame # data is created on the first frame
	if terrain.data == null:
		res["error"] = "terrain.data still null after a frame"
		return res
	var util = ClassDB.instantiate("Terrain3DUtil")
	var img: Image = util.load_image(r16, ResourceLoader.CACHE_MODE_IGNORE, Vector2(hmin, hmax), r16_size)
	if img == null or img.is_empty():
		res["error"] = "r16 load failed: " + r16
		return res
	var imgs: Array[Image] = []
	imgs.resize(3) # TYPE_HEIGHT, TYPE_CONTROL, TYPE_COLOR
	imgs[0] = img
	# Snap the import corner to the 256 m region grid; import_images floors
	# the position anyway, so doing it here keeps res["origin"] honest.
	var rs := 256.0 # default Terrain3D region size in meters
	var sx := floorf(ox / rs) * rs
	var sz := floorf(oz / rs) * rs
	terrain.data.import_images(imgs, Vector3(sx, 0, sz), 0.0, 1.0)
	res["import_origin"] = Vector2(sx, sz)
	terrain.data.update_maps(3, true, false) # MapType.TYPE_MAX
	await tree.process_frame
	res["ok"] = true
	res["terrain"] = terrain
	return res

static func dequantized(meta: Dictionary, r16_bytes: PackedByteArray, gi: int, gj: int) -> float:
	var nx: int = meta["nx"] - 1 # r16 is the cell grid
	var u := r16_bytes.decode_u16((gj * nx + gi) * 2)
	return meta["h_min"] + u * meta["step"]

static func verify(meta: Dictionary, terrain: Node3D, base: String, import_origin: Vector2) -> Array:
	# Known cell-grid indices -> expected dequantized height within one step.
	# Sample (i,j) sits at import_origin + (i,j)*cell (region-grid snapped).
	var r16 := FileAccess.get_file_as_bytes(base.path_join(meta["files"]["height_terrain3d"]))
	var nx: int = meta["nx"] - 1
	var nz: int = meta["nz"] - 1
	var ox: float = import_origin.x
	var oz: float = import_origin.y
	var cell: float = meta["cell_size_m"]
	var step: float = meta["step"]
	var pts := [
		Vector2i(0, 0), Vector2i(nx / 2, nz / 2),
		Vector2i(nx * 3 / 4, nz / 4), Vector2i(nx - 1, nz - 1),
	]
	var out := []
	for p in pts:
		var exp := dequantized(meta, r16, p.x, p.y)
		var got: float = terrain.data.get_height(Vector3(ox + p.x * cell, 0, oz + p.y * cell))
		out.append({"p": p, "expected": exp, "got": got, "ok": absf(got - exp) <= step + 1e-4})
	return out

func _init() -> void:
	var args := OS.get_cmdline_user_args()
	var layers := ""
	for i in args.size():
		if args[i] == "--layers" and i + 1 < args.size():
			layers = args[i + 1]
	if layers == "":
		printerr("usage: --script terrain3d_import.gd -- --layers <layers.json>")
		quit(1)
		return
	var r: Dictionary = await import_into(layers, root, self)
	if not r["ok"]:
		printerr("T3D_IMPORT " + r["error"])
		quit(1)
		return
	var checks: Array = verify(r["meta"], r["terrain"], layers.get_base_dir(), r["import_origin"])
	var ok := true
	for c in checks:
		ok = ok and c["ok"]
		print("T3D_CHECK node(%d,%d) expected %.4f got %.4f %s" % [c["p"].x, c["p"].y, c["expected"], c["got"], "OK" if c["ok"] else "FAIL"])
	print("T3D_IMPORT ", "PASS" if ok else "FAIL")
	quit(0 if ok else 1)
