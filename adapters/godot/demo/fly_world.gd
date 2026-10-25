extends Node3D

# Fly-camera demo: load a built world and fly around it.
# Run: Godot --path adapters/godot/demo -- res://fly_world.tscn --manifest <path> [--bench <seconds>]

var cam: Camera3D
var speed := 60.0
var yaw := 0.0
var pitch := -0.5
var overlay: Label
var stats := {"chunks": 0, "triangles": 0}
var bench_sec := 0.0
var bench_times: Array = []
var bench_gpu: Array = []
var bench_cpu: Array = []
var bench_t := 0.0
var bench_done := false
var use_t3d := false
var W := 0.0
var D := 0.0

func _ready() -> void:
	var args := OS.get_cmdline_user_args()
	var manifest_path := ""
	for i in args.size():
		if args[i] == "--manifest" and i + 1 < args.size():
			manifest_path = args[i + 1]
		if args[i] == "--bench" and i + 1 < args.size():
			bench_sec = args[i + 1].to_float()
		if args[i] == "--t3d":
			use_t3d = true
	if manifest_path == "":
		printerr("usage: --manifest <path> [--bench <seconds>] [--t3d]")
		get_tree().quit(1)
		return
	if use_t3d:
		# Terrain3D bench: same path, terrain from the r16 layer instead of
		# chunk meshes. layers.json lives next to exports/.
		var layers_path := manifest_path.get_base_dir().path_join("../../layers/layers.json").simplify_path()
		var tr: Dictionary = await Terrain3DImport.import_into(layers_path, self, get_tree())
		if not tr["ok"]:
			printerr("t3d import failed: ", tr["error"])
			get_tree().quit(1)
			return
		var tm: Dictionary = tr["meta"]
		W = (tm["nx"] - 1) * tm["cell_size_m"]
		D = (tm["nz"] - 1) * tm["cell_size_m"]
		stats["chunks"] = 0
		stats["triangles"] = 0 # filled from rendering info below
	else:
		var res := WorldLoader.load_world(manifest_path, self)
		stats["chunks"] = res["chunks"]
		stats["triangles"] = res["triangles"]
		var m: Dictionary = res["manifest"]
		W = m["extent"]["width_m"]
		D = m["extent"]["depth_m"]
	# Light + sky + fog.
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-50, -30, 0)
	sun.shadow_enabled = true
	add_child(sun)
	var env := WorldEnvironment.new()
	var e := Environment.new()
	var sky := Sky.new()
	sky.sky_material = ProceduralSkyMaterial.new()
	e.background_mode = Environment.BG_SKY
	e.sky = sky
	e.fog_enabled = true
	e.fog_density = 0.00015
	# Keep the sky readable through fog: low sky affect + a light-blue fog.
	e.fog_sky_affect = 0.15
	e.fog_light_color = Color(0.6, 0.7, 0.85)
	env.environment = e
	add_child(env)
	# Camera + overlay.
	cam = Camera3D.new()
	cam.far = max(W, D) * 4.0
	add_child(cam)
	cam.position = Vector3(0, max(W, D) * 0.75, max(W, D) * 0.75)
	cam.look_at(Vector3.ZERO, Vector3.UP)
	cam.current = true
	var layer := CanvasLayer.new()
	overlay = Label.new()
	overlay.add_theme_font_size_override("font_size", 16)
	overlay.add_theme_color_override("font_color", Color(0.9, 0.9, 0.9))
	overlay.position = Vector2(10, 10)
	layer.add_child(overlay)
	add_child(layer)
	if bench_sec > 0.0:
		DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
		Engine.max_fps = 0
		DisplayServer.window_set_size(Vector2i(1600, 900))
		RenderingServer.viewport_set_measure_render_time(get_viewport().get_viewport_rid(), true)
		bench_t = -2.0 # warm-up

func _unhandled_input(e: InputEvent) -> void:
	if bench_sec > 0.0:
		return
	if e is InputEventMouseButton:
		if e.button_index == MOUSE_BUTTON_WHEEL_UP and e.pressed:
			speed *= 1.25
		if e.button_index == MOUSE_BUTTON_WHEEL_DOWN and e.pressed:
			speed /= 1.25
	if e is InputEventMouseMotion and Input.is_mouse_button_pressed(MOUSE_BUTTON_RIGHT):
		yaw -= e.relative.x * 0.004
		pitch = clampf(pitch - e.relative.y * 0.004, -1.5, 1.5)

func _process(dt: float) -> void:
	if cam == null:
		return
	if bench_sec > 0.0:
		_bench_frame(dt)
		return
	# Fly controls: WASD/QE + mouse look (right button) + shift speed.
	var boost := 1.0 if not Input.is_key_pressed(KEY_SHIFT) else 4.0
	var dir := Vector3.ZERO
	var fwd := -cam.global_transform.basis.z
	var right := cam.global_transform.basis.x
	if Input.is_key_pressed(KEY_W): dir += fwd
	if Input.is_key_pressed(KEY_S): dir -= fwd
	if Input.is_key_pressed(KEY_D): dir += right
	if Input.is_key_pressed(KEY_A): dir -= right
	if Input.is_key_pressed(KEY_E): dir += Vector3.UP
	if Input.is_key_pressed(KEY_Q): dir -= Vector3.UP
	cam.position += dir.normalized() * speed * boost * dt
	cam.rotation = Vector3(pitch, yaw, 0)
	_overlay(dt)

func _overlay(dt: float) -> void:
	var ri := RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_DRAW_CALLS_IN_FRAME)
	overlay.text = "fps %.0f | %.1f ms | draws %d | tris %d | chunks %d | pos %.0f,%.0f,%.0f" % [
		1.0 / max(dt, 1e-6), dt * 1000.0, ri, stats["triangles"], stats["chunks"],
		cam.position.x, cam.position.y, cam.position.z,
	]

func _bench_frame(dt: float) -> void:
	if bench_done:
		return
	bench_t += dt
	# Deterministic path: circle the island at ~60 m above ground, then a
	# low coast pass.
	var t := bench_t
	cam.position = _path_pos(t)
	cam.look_at(Vector3(0, 10, 0), Vector3.UP)
	_overlay(dt)
	if t > 0.0:
		bench_times.append(dt)
		var rid := get_viewport().get_viewport_rid()
		bench_gpu.append(RenderingServer.viewport_get_measured_render_time_gpu(rid))
		bench_cpu.append(RenderingServer.viewport_get_measured_render_time_cpu(rid))
	if t >= bench_sec:
		bench_done = true
		_finish_bench()

func _path_pos(t: float) -> Vector3:
	var r: float = maxf(W, D) * 0.4
	if t > 20.0:
		# low pass over the coast: circle r*0.85 at 20 m.
		var a2 := (t - 20.0) * 0.25
		return Vector3(cos(a2) * r * 0.85, 20.0, sin(a2) * r * 0.85)
	var a := t * 0.15
	return Vector3(cos(a) * r, 60.0, sin(a) * r)

# After the timing window ends, render one extra frame at the mid-path pose
# and take the screenshot there — the GPU readback hitch stays out of the
# recorded frame times.
func _finish_bench() -> void:
	cam.position = _path_pos(bench_sec * 0.5)
	cam.look_at(Vector3(0, 10, 0), Vector3.UP)
	await RenderingServer.frame_post_draw
	var out := OS.get_environment("MB_T3D_PNG" if use_t3d else "MB_FLY_PNG")
	if out != "":
		var img := get_viewport().get_texture().get_image()
		DirAccess.make_dir_recursive_absolute(out.get_base_dir())
		img.save_png(out)
		print("saved ", out)
	# One more frame at a high ~45° pose with the whole island in frame.
	var top := OS.get_environment("MB_FLY_TOP_PNG")
	if top != "":
		var dist: float = maxf(W, D) * 1.1
		cam.position = Vector3(0, dist * 0.75 + 100.0, dist * 0.75)
		cam.look_at(Vector3.ZERO, Vector3.UP)
		await RenderingServer.frame_post_draw
		var img2 := get_viewport().get_texture().get_image()
		DirAccess.make_dir_recursive_absolute(top.get_base_dir())
		img2.save_png(top)
		print("saved ", top)
	# Extra posed captures: MB_FLY_POSES="png|px,py,pz|tx,ty,tz;png|..."
	var poses := OS.get_environment("MB_FLY_POSES")
	if poses != "":
		for spec in poses.split(";", false):
			var parts := spec.split("|")
			if parts.size() != 3:
				continue
			var pv := parts[1].split_floats(",")
			var tv := parts[2].split_floats(",")
			cam.position = Vector3(pv[0], pv[1], pv[2])
			cam.look_at(Vector3(tv[0], tv[1], tv[2]), Vector3.UP)
			await RenderingServer.frame_post_draw
			var im := get_viewport().get_texture().get_image()
			DirAccess.make_dir_recursive_absolute(parts[0].get_base_dir())
			im.save_png(parts[0])
			print("saved ", parts[0])
	_bench_report()

func _avg(a: Array) -> float:
	var s := 0.0
	for v in a:
		s += v
	return s / max(a.size(), 1)

func _bench_report() -> void:
	if bench_times.is_empty():
		get_tree().quit(1)
		return
	var worst := 0.0
	var lows := bench_times.duplicate()
	lows.sort()
	for v in bench_times:
		worst = max(worst, v)
	var n := lows.size()
	var p1_ms: float = lows[int(n * 0.99)]
	var avg_fps := 1.0 / _avg(bench_times)
	var p1_low_fps := 1.0 / p1_ms
	if p1_low_fps > avg_fps * 1.001:
		print("WARNING: p1_low_fps %.1f > avg_fps %.1f" % [p1_low_fps, avg_fps])
	var gpu_sorted := bench_gpu.duplicate()
	gpu_sorted.sort()
	var p99_gpu: float = gpu_sorted[int(gpu_sorted.size() * 0.99)]
	var capped := false
	for rate in [60.0, 120.0, 144.0]:
		if absf(avg_fps - rate) / rate < 0.02:
			capped = true
	var ri := RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_DRAW_CALLS_IN_FRAME)
	var tris: int = stats["triangles"]
	if tris == 0:
		tris = RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_PRIMITIVES_IN_FRAME)
	var tag := "BENCH_T3D" if stats["chunks"] == 0 else "BENCH"
	print("%s {\"avg_fps\":%.1f,\"p1_low_fps\":%.1f,\"max_frame_ms\":%.2f,\"avg_gpu_ms\":%.2f,\"p99_gpu_ms\":%.2f,\"avg_cpu_ms\":%.2f,\"capped\":%s,\"frames\":%d,\"draw_calls\":%d,\"triangles\":%d,\"chunks\":%d}" % [
		tag, avg_fps, p1_low_fps, worst * 1000.0,
		_avg(bench_gpu), p99_gpu, _avg(bench_cpu),
		"true" if capped else "false",
		n, ri, tris, stats["chunks"],
	])
	get_tree().quit(0)
