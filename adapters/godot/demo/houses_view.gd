extends SceneTree

var frame := 0
var houses: Array = []
var cam: Camera3D
var out_dir := "target/gates/house"
var manifest := ""

func _init():
	for a in OS.get_cmdline_user_args():
		if a.begins_with("--manifest="):
			manifest = a.substr(11)
		elif a.begins_with("--out="):
			out_dir = a.substr(6)

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
		var pm := PlaneMesh.new(); pm.size = Vector2(200, 200)
		gnd.mesh = pm
		var gm := StandardMaterial3D.new(); gm.albedo_color = Color(0.4, 0.5, 0.35)
		gnd.material_override = gm
		root.add_child(gnd)
		var sun := DirectionalLight3D.new()
		sun.rotation_degrees = Vector3(-50, 30, 0); sun.shadow_enabled = true
		root.add_child(sun)
		var env := WorldEnvironment.new()
		var e := Environment.new()
		e.background_mode = Environment.BG_SKY
		var sk := Sky.new(); var psm := ProceduralSkyMaterial.new(); sk.sky_material = psm
		e.sky = sk
		env.environment = e
		root.add_child(env)
		var res = HouseLoader.load_houses(manifest, root)
		houses = res["houses"]
		if houses.is_empty():
			printerr("houses_view: no houses loaded")
			quit(1)
			return true
		var total := -6.0
		for h in houses:
			total += 14.0 + 6.0
		var off := -total / 2.0
		for h in houses:
			h.position.x = off + 7.0
			off += 20.0
		cam = Camera3D.new()
		root.add_child(cam)
		cam.look_at_from_position(Vector3(0, 12, 42), Vector3(0, 4, 0), Vector3.UP)
		cam.current = true
		return false
	if frame == 3:
		DirAccess.make_dir_recursive_absolute(out_dir)
		get_root().get_viewport().get_texture().get_image().save_png(out_dir + "/houses.png")
		for h in houses:
			for k in 3:
				var mi: MeshInstance3D = h.get_node("lod%d" % k)
				mi.visible = (k == 1)
				mi.visibility_range_begin = 0.0
				mi.visibility_range_end = 0.0
		return false
	if frame == 5:
		get_root().get_viewport().get_texture().get_image().save_png(out_dir + "/houses_lod1.png")
		for h in houses:
			for k in 3:
				var mi: MeshInstance3D = h.get_node("lod%d" % k)
				mi.visible = (k == 2)
				mi.visibility_range_begin = 0.0
				mi.visibility_range_end = 0.0
		return false
	if frame == 7:
		get_root().get_viewport().get_texture().get_image().save_png(out_dir + "/houses_lod2.png")
		for h in houses:
			for k in 3:
				var mi: MeshInstance3D = h.get_node("lod%d" % k)
				mi.visible = (k == 0)
				mi.visibility_range_begin = 0.0
				mi.visibility_range_end = 0.0
		var r: Node3D = houses[0]
		for h in houses:
			if h.name == "rect":
				r = h
		cam.look_at_from_position(r.global_position + Vector3(0, 2.5, -11), r.global_position + Vector3(0, 2.0, 0), Vector3.UP)
		return false
	if frame == 9:
		get_root().get_viewport().get_texture().get_image().save_png(out_dir + "/house_door.png")
		# checks
		var ok := true
		var m2: Dictionary = JSON.parse_string(FileAccess.open(manifest, FileAccess.READ).get_as_text())
		if houses.size() != m2["houses"].size(): ok = false
		for h in houses:
			for k in 3:
				var mi: MeshInstance3D = h.get_node_or_null("lod%d" % k)
				if mi == null or mi.mesh == null:
					ok = false
				else:
					var m3 := mi.get_active_material(0)
					if m3 != null and m3.albedo_color == Color.WHITE:
						ok = false
		if ok:
			print("HOUSE GODOT PASS houses=%d" % houses.size())
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
