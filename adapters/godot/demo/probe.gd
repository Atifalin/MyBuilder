extends SceneTree

# S2 bindings gate: print the same lines as `mybuilder probe`.
func _initialize() -> void:
	print("version ", MyBuilderCore.version())
	print("probe ", MyBuilderCore.probe())
	var seed_hex := MyBuilderCore.derive_seed_hex("0000000000000001", PackedStringArray(["region:r0", "settlement:s0"]))
	print("derive ", seed_hex)
	quit()
