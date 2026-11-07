extends SceneTree

# House preview + check: loads exports/houses/manifest.json via HouseLoader,
# saves screenshots and prints HOUSE GODOT PASS/FAIL.
# Run: Godot --path adapters/godot/demo --script res://houses_view.gd -- --manifest=<path> [--out=<dir>]

var frame := 0
var houses: Array = []
var parts: Node3D
var part_counts: Dictionary = {}
var cam: Camera3D
var out_dir := "target/gates/house"
var manifest := ""
var draw_calls := 0
var textured := 0

func _init():
	for a in OS.get_cmdline_user_args():
		if a.begins_with("--manifest="):
			manifest = a.substr(11)
		elif a.begins_with("--out="):
			out_dir = a.substr(6)

func _house(id: String) -> Node3D:
	for h in houses:
		if h.name == id:
			return h
	return houses[0]

# Force one LOD everywhere (houses and parts) and disable distance ranges.
func _force_lod(k: int) -> void:
	for h in houses:
		for i in 3:
			var mi: MeshInstance3D = h.get_node("lod%d" % i)
			mi.visible = (i == k)
			mi.visibility_range_begin = 0.0
			mi.visibility_range_end = 0.0
		var ii: MeshInstance3D = h.get_node_or_null("interior")
		if ii:
			ii.visible = (k == 0)
			ii.visibility_range_end = 0.0
	for mmi in parts.get_children():
		mmi.visible = mmi.name.ends_with("_lod%d" % k)
		mmi.visibility_range_begin = 0.0
		mmi.visibility_range_end = 0.0

func _shot(name: String) -> void:
	get_root().get_viewport().get_texture().get_image().save_png(out_dir + "/" + name)

func _look(h: Node3D, eye: Vector3, at: Vector3) -> void:
	_clear_sections()
	cam.projection = Camera3D.PROJECTION_PERSPECTIVE
	cam.near = 0.05
	cam.look_at_from_position(h.global_position + eye, h.global_position + at, Vector3.UP)

# Cut-away: everything above `cut_y` is discarded and back faces seen
# through the cut are drawn dark, so cut walls read as solid (a 3D plan).
const CUT_SHADER := """
shader_type spatial;
render_mode cull_disabled;
uniform float cut_y;
varying vec3 wpos;
void vertex() { wpos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz; }
void fragment() {
	if (wpos.y > cut_y) { discard; }
	ALBEDO = FRONT_FACING ? COLOR.rgb : vec3(0.16, 0.13, 0.11);
	ROUGHNESS = 0.9;
}
"""

func _section(h: Node3D, cut_y: float, dist: float) -> void:
	_clear_sections()
	var sh := Shader.new()
	sh.code = CUT_SHADER
	var mat := ShaderMaterial.new()
	mat.shader = sh
	mat.set_shader_parameter("cut_y", h.global_position.y + cut_y)
	for n in ["lod0", "interior"]:
		var mi: MeshInstance3D = h.get_node_or_null(n)
		if mi:
			mi.material_override = mat
	parts.visible = false
	cam.projection = Camera3D.PROJECTION_PERSPECTIVE
	cam.near = 0.05
	var at := h.global_position + Vector3(0, cut_y - 1.0, 0)
	cam.look_at_from_position(at + Vector3(-0.35, 1.0, -0.6).normalized() * dist, at, Vector3.UP)

func _clear_sections() -> void:
	for h in houses:
		for n in ["lod0", "interior"]:
			var mi: MeshInstance3D = h.get_node_or_null(n)
			if mi:
				mi.material_override = null
	parts.visible = true

func _process(_d) -> bool:
	frame += 1
	if frame == 1:
		if manifest == "":
			printerr("houses_view: --manifest required")
			quit(1)
			return true
		var root := Node3D.new()
		get_root().add_child(root)
		var gnd := MeshInstance3D.new()
		var pm := PlaneMesh.new()
		pm.size = Vector2(300, 300)
		gnd.mesh = pm
		var gm := StandardMaterial3D.new()
		gm.albedo_color = Color(0.4, 0.5, 0.35)
		gnd.material_override = gm
		root.add_child(gnd)
		var sun := DirectionalLight3D.new()
		sun.rotation_degrees = Vector3(-50, 150, 0)
		sun.shadow_enabled = true
		root.add_child(sun)
		var env := WorldEnvironment.new()
		var e := Environment.new()
		e.background_mode = Environment.BG_SKY
		var sk := Sky.new()
		sk.sky_material = ProceduralSkyMaterial.new()
		e.sky = sk
		e.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
		env.environment = e
		root.add_child(env)
		var res: Dictionary = HouseLoader.load_houses(manifest, root)
		houses = res["houses"]
		parts = res["parts"]
		part_counts = res["part_instances"]
		textured = int(res.get("textured", 0))
		if houses.is_empty():
			printerr("houses_view: no houses loaded")
			quit(1)
			return true
		cam = Camera3D.new()
		root.add_child(cam)
		cam.look_at_from_position(Vector3(0, 16, 60), Vector3(0, 4, 0), Vector3.UP)
		cam.current = true
		return false
	if frame == 3:
		DirAccess.make_dir_recursive_absolute(out_dir)
		draw_calls = RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_DRAW_CALLS_IN_FRAME)
		_shot("houses.png")
		_force_lod(1)
		return false
	if frame == 5:
		_shot("houses_lod1.png")
		_force_lod(2)
		return false
	if frame == 7:
		_shot("houses_lod2.png")
		_force_lod(0)
		_look(_house("rect"), Vector3(0, 2.5, -11), Vector3(0, 2.0, 0))
		return false
	if frame == 9:
		_shot("house_door.png")
		_look(_house("haveli"), Vector3(-3, 5.5, -18), Vector3(0, 4.5, 0))
		return false
	if frame == 11:
		_shot("house_jharokha.png")
		_look(_house("haveli"), Vector3(-14, 22, -14), Vector3(0, 6, 0))
		return false
	# Default house: plinth 0.45 m, storeys 3.2 m; cut 1.4 m above a floor.
	if frame == 13:
		_shot("house_roof.png")
		_section(_house("haveli"), 0.45 + 1.4, 30.0)
		return false
	if frame == 15:
		_shot("house_section_haveli_s0.png")
		_section(_house("haveli"), 0.45 + 3.2 + 1.4, 30.0)
		return false
	if frame == 17:
		_shot("house_section_haveli_s1.png")
		_section(_house("ell"), 0.45 + 1.4, 22.0)
		return false
	if frame == 19:
		_shot("house_section_ell_s0.png")
		_section(_house("ell"), 0.45 + 3.2 + 1.4, 22.0)
		return false
	if frame == 21:
		_shot("house_section_ell_s1.png")
		_look(_house("haveli"), Vector3(-6.5, 2.0, -7.5), Vector3(0, 1.6, 0))
		return false
	if frame == 23:
		_shot("house_interior_haveli.png")
		_look(_house("rect"), Vector3(2.5, 1.3, -6.5), Vector3(0.5, 0.8, 0))
		return false
	if frame == 25:
		_shot("house_closeup.png")
		_look(_house("haveli"), Vector3(-6.0, 2.6, -6.0), Vector3(-4.5, 0.45, -4.0))
		return false
	if frame == 27:
		_shot("house_closeup_floor.png")
		var ok := true
		if textured == 0:
			printerr("no textured surfaces")
			ok = false
		for h in houses:
			if h.get_node_or_null("interior") == null:
				printerr("no interior node on %s" % h.name)
				ok = false
		var m2: Dictionary = JSON.parse_string(FileAccess.open(manifest, FileAccess.READ).get_as_text())
		if houses.size() != m2["houses"].size():
			printerr("house count mismatch")
			ok = false
		for h in houses:
			for k in 3:
				var mi: MeshInstance3D = h.get_node_or_null("lod%d" % k)
				if mi == null or mi.mesh == null:
					printerr("missing lod%d on %s" % [k, h.name])
					ok = false
				else:
					var mat := mi.get_active_material(0)
					if mat != null and mat.albedo_color == Color.WHITE:
						printerr("white material on %s lod%d" % [h.name, k])
						ok = false
		# Part MultiMesh instance counts must match the manifest.
		var want := {}
		for h in m2["houses"]:
			for inst in h.get("instances", []):
				for li in 2:
					if int(inst["lods"]) & (1 << li):
						var key := "%s/%d" % [inst["key"], li]
						want[key] = int(want.get(key, 0)) + 1
		if want.size() != part_counts.size():
			printerr("part multimesh count %d != %d" % [part_counts.size(), want.size()])
			ok = false
		for key in want:
			if int(part_counts.get(key, -1)) != int(want[key]):
				printerr("part %s instances %s != %d" % [key, part_counts.get(key, -1), want[key]])
				ok = false
		if ok:
			print("HOUSE GODOT PASS houses=%d part_multimeshes=%d draw_calls=%d textured=%d" % [houses.size(), part_counts.size(), draw_calls, textured])
			quit(0)
		else:
			printerr("HOUSE GODOT FAIL")
			quit(1)
		return true
	if frame > 600:
		printerr("houses_view: timeout")
		quit(1)
		return true
	return false
