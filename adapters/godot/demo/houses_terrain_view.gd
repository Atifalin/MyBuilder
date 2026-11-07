extends SceneTree

# Placed houses on built terrain: loads the terrain manifest via WorldLoader
# and the houses manifest via HouseLoader, checks every placed house stands
# at its site height on a foundation, saves screenshots and prints
# HOUSE TERRAIN PASS/FAIL.
# Run: Godot --path adapters/godot/demo --script res://houses_terrain_view.gd -- --project=<dir> [--out=<dir>]

var frame := 0
var project := ""
var out_dir := "target/gates/house_terrain"
var cam: Camera3D
var houses: Array = []
var hm: Dictionary = {}
var steep: Node3D
var steep_depth := 0.0

func _init():
	for a in OS.get_cmdline_user_args():
		if a.begins_with("--project="):
			project = a.substr(10)
		elif a.begins_with("--out="):
			out_dir = a.substr(6)

func _shot(name: String) -> void:
	get_root().get_viewport().get_texture().get_image().save_png(out_dir + "/" + name)

func _fail(msg: String) -> void:
	printerr(msg)
	printerr("HOUSE TERRAIN FAIL")
	quit(1)

func _process(_d) -> bool:
	frame += 1
	if frame == 1:
		var root := Node3D.new()
		get_root().add_child(root)
		WorldLoader.load_world(project + "/exports/terrain/manifest.json", root)
		var res: Dictionary = HouseLoader.load_houses(project + "/exports/houses/manifest.json", root)
		houses = res["houses"]
		hm = JSON.parse_string(FileAccess.get_file_as_string(project + "/exports/houses/manifest.json"))
		var sun := DirectionalLight3D.new()
		sun.rotation_degrees = Vector3(-40, 150, 0)
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
		cam = Camera3D.new()
		cam.far = 4000.0
		root.add_child(cam)
		cam.current = true
		# Checks: every placed house at its site height, with a foundation.
		var placed := 0
		for h in hm["houses"]:
			if h.get("placement") == null:
				continue
			placed += 1
			var node: Node3D = null
			for n in houses:
				if n.name == h["id"]:
					node = n
			if node == null:
				_fail("house %s not loaded" % h["id"])
				return true
			var site: Dictionary = h["site"]
			if absf(node.position.y - float(site["y"])) > 0.001:
				_fail("house %s y %f != site %f" % [h["id"], node.position.y, site["y"]])
				return true
			var f: MeshInstance3D = node.get_node_or_null("foundation")
			if f == null or f.mesh == null:
				_fail("house %s has no foundation" % h["id"])
				return true
			var aabb: AABB = f.mesh.get_aabb()
			var want: float = -float(site["foundation_depth"])
			if absf(aabb.position.y - want) > 0.01:
				_fail("house %s foundation bottom %f != %f" % [h["id"], aabb.position.y, want])
				return true
			if float(site["foundation_depth"]) > steep_depth:
				steep_depth = float(site["foundation_depth"])
				steep = node
		if placed == 0 or steep == null:
			_fail("no placed houses")
			return true
		DirAccess.make_dir_recursive_absolute(out_dir)
		# Overview of the whole cluster.
		var c := Vector3.ZERO
		for n in houses:
			c += n.position
		c /= float(houses.size())
		cam.look_at_from_position(c + Vector3(-70, 55, -70), c, Vector3.UP)
		return false
	if frame == 4:
		_shot("terrain_houses_overview.png")
		var p := steep.position
		cam.look_at_from_position(p + Vector3(-16, 5, -20), p + Vector3(0, 1.5, 0), Vector3.UP)
		return false
	if frame == 7:
		_shot("terrain_house_steep_a.png")
		var p := steep.position
		cam.look_at_from_position(p + Vector3(20, 4, 16), p + Vector3(0, 1.0, 0), Vector3.UP)
		return false
	if frame == 10:
		_shot("terrain_house_steep_b.png")
		var p := steep.position
		cam.look_at_from_position(p + Vector3(-20, 4, 16), p + Vector3(0, 1.0, 0), Vector3.UP)
		return false
	if frame == 13:
		_shot("terrain_house_steep_c.png")
		print("HOUSE TERRAIN PASS houses=%d max_foundation_m=%.2f steepest=%s" % [houses.size(), steep_depth, steep.name])
		quit(0)
		return true
	if frame > 600:
		_fail("timeout")
		return true
	return false
