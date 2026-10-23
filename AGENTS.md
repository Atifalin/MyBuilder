# My Builder — agent notes

## Commands

- Build: `cargo build --workspace`
- Test: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt` (check with `cargo fmt --check`)
- CLI: `cargo run -p mb-cli -- <new|validate|hash|info> ...`

The toolchain is pinned by `rust-toolchain.toml`; rustfmt and clippy are
required components.

## Build engine (S10)

- `mybuilder build` uses a persistent cache in the OS cache dir
  (`<os cache>/MyBuilder/projects/<name>-<16hex>/`, CACHEDIR.TAG at the
  root). Resolve with `mybuilder cache-path <dir>`; override via
  `--cache-dir` / `MB_CACHE_DIR` (env read by the CLI only). Layout:
  `index.json` (stage id -> {key, artifact hash}), `blobs/<hash>.bin`.
  Blobs are blake3-verified on load; corrupt/missing -> re-run.
  Concurrency is the project-level `.mybuilder.lock` (OS advisory
  lock, pid inside; `.gitignore`d by `new`). DiskCache itself does
  not lock — callers must hold the project lock.
- Deletes never follow symlinks: every stale-file delete walks each
  component with symlink_metadata and canonicalizes the parent; writes
  still follow symlinks (exports/ may point into a Godot project).
- Sliced stages (`world.chunk.*`, `world.region.*.chunk.*`) may ONLY read
  their slice — the run fn receives the slice bytes, so the cache key
  provably covers every input byte. Chunks use a 1-sample edge apron
  (no corners: diagonals stay uncoupled); region chunks use a
  mesh_step+1 coarse window.
- `mybuilder build --clean --yes` wipes cache + files listed in the
  previous manifest/layers.json (never anything else); refuses without
  --yes.
- Bumping `mb_seed::CURRENT_GENERATOR_VERSION` rekeys every stage.
- Editing helpers that write the recipe don't bump generator_version;
  generation algorithms do.
- Gen 2 (texturing): chunk artifacts carry a `u32 len + RGBA8 PNG`
  splat tail after the GLB; the GLB's own length header gives the
  boundary. Godot: `import_world.gd` switches to `terrain_splat.gdshader`
  when the manifest has `splat_channels` (UV2 + `splat` uniform; PNGs
  load via `Image.load_from_file`). Blender: `import_world.py` wires an
  Image Texture on UV layer 2 (`ShaderNodeUVMap`, Non-Color) through
  Separate Color into weighted palette MixRGBs — its SeparateColor node
  has no Alpha output; use the texture's Alpha output directly.
- Control app: `cd apps/desktop/src-tauri && cargo run` (run from that dir so its
  `.cargo/config.toml` puts output in the root `target/app`; `--manifest-path` from the
  repo root ignores it and creates a duplicate multi-GB target dir)
  (Tauri 2, plain-cargo run, assets from `ui/` via custom-protocol;
  build output in `target/app/`). Headless check:
  `MB_APP_AUTOTEST=<project> target/app/debug/mb-app`.
- UI-facing API lives in mb-bridge: `build(dir, req, &CancelToken, sink)`
  emits `BuildEvent`s (`--json-events` prints them as NDJSON; write
  `cancel` to stdin to abort), `Session` for recipe edits with
  undo/redo snapshots, `clear_all` for wipe+reseed under the project
  lock, and `PreviewSource` for map tiles (zoom z -> mpp 8·2^-z).

## Generator versions

`project.json` carries `generator_version` (default 1 when absent — never
silently upgraded). Any intentional change to generated output (any
generation golden re-bless) must bump `mb_seed::CURRENT_GENERATOR_VERSION`
once released; old projects keep old output until `mybuilder upgrade`.
Every bless requires `MB_BLESS_REASON` and appends to GOLDEN_CHANGELOG.md.

## Golden tests (determinism)

Golden files are BLAKE3 hex digests at `crates/*/tests/golden/<name>.blake3`.
They are checked in and are the cross-OS determinism gate (CI runs the same
tests on Linux, macOS and Windows).

To create or update goldens after an intentional output change:

    MB_BLESS=1 cargo test --workspace

Then review the diffs under `tests/golden/` before committing — a changed
golden means a changed output.

## Determinism rules

- No `HashMap` iteration anywhere that affects output; use `BTreeMap` or
  `Vec`.
- No libm transcendental functions (`sin`, `cos`, `exp`, `powf`,
  `atan`, ...) in generation paths — they differ across OSes. Write needed
  constants as literals. No `mul_add` / fused multiply-add.
  `sqrt` IS allowed: IEEE 754 requires it to be correctly rounded, so it
  is bit-identical on every platform (used by the EDT and normal
  normalization).
- All randomness comes from `mb_seed::Rng` (counter-based); derive child
  seeds with `mb_seed::derive` / `derive_path`, never from iteration order.
- Seeds serialize as 16-char lowercase hex strings in JSON.
- World coordinates are meters, right-handed, Y-up; 2D plans live in the
  XZ plane with the origin at the world center. All noise is evaluated in
  world coordinates, never tile indices.
- Canonical hashes are BLAKE3 of compact serde_json bytes of *typed*
  structs; typed structs must never contain maps.

## Adapters and the S2 bindings gate

`crates/bridge` (mb-bridge) is the only facade adapters may call; adapters
must never depend on the other core crates directly.

- `adapters/godot` (mb-godot, cdylib, godot 0.5 + `api-4-7`): build with
  `cargo build -p mb-godot --release`, copy `target/release/libmb_godot.dylib`
  into `adapters/godot/demo/bin/` (both are gitignored), then run
  `Godot --headless --path adapters/godot/demo --script res://probe.gd`.
  Gotchas: on a fresh checkout the demo needs one
  `--headless --import` pass so the extension lands in the class cache;
  that pass may crash on teardown under headless (engine bug, harmless —
  the cache is written first). GDScript calls static funcs as
  `MyBuilderCore.probe()`; godot-rust needs `GString::from(&String)`
  (there is no `From<String>`, only `From<&String>`/`From<&str>`).
- `adapters/blender` (mb-blender, cdylib, pyo3 0.30 + `extension-module`
  + `abi3-py311`): build the wheel with maturin (`pyproject.toml` is in
  the crate dir), install with Blender's own pip
  (`Blender.app/Contents/Resources/5.2/python/bin/python3.13 -m pip install
  --target <site> <wheel>`), then `import mybuilder_core`. Gotchas: on
  macOS, building the cdylib with plain cargo needs
  `-undefined dynamic_lookup` — emitted per-cdylib by
  `pyo3_build_config::add_extension_module_link_args()` in the crate's
  `build.rs` (NOT global rustflags: that would mask real link errors in
  every other artifact).
  PyO3's build script needs a Python >= 3.11 interpreter on PATH (CI uses
  `actions/setup-python`). Both adapter crates set `[lib] test = false,
  doctest = false` so `cargo test` never links test binaries for cdylibs.
- One-shot verification: `bash scripts/s2-bindings-gate.sh` builds release,
  runs all three probes and exits 0 only if the `version` and `probe`
  lines match. Override binaries via `GODOT_BIN` / `BLENDER_BIN`.

## Terrain pipeline (S3)

- `mybuilder build <dir>` runs masks -> distance -> height -> export via
  mb-pipeline and writes layers/, previews/hillshade.png,
  exports/terrain/chunk_*.glb and manifest.json. Files are only written
  when their bytes change (blake3 compare); the manifest is written last,
  atomically.
- Gotchas hit during S3: glTF extras DO survive Godot's runtime
  `GLTFDocument.append_from_file`/`generate_scene` (node meta "extras").
  GDScript `:=` type inference fails on `max()`/transform products —
  annotate `var x: float`/`var aabb: AABB`. `Camera3D.look_at` requires
  the node to be inside the tree; use `look_at_from_position` instead.
  Godot AABB checks: apply each node's transform exactly once (the scene
  root's own transform is not included in `Node3D.transform` of children,
  apply it at the top level).
- `scripts/s3-terrain-gate.sh` runs new+build+Godot+Blender checks and
  exits 0 only if both print PASS.
- Vertex colors: the glTF spec expects COLOR_0 to feed base color, but
  neither importer does it automatically — `import_world.gd` sets
  `vertex_color_use_as_albedo = true` per StandardMaterial3D, and
  `import_world.py` wires a Color Attribute node into Base Color.
- Noise lattice artifacts: axis-aligned gradients plus same-orientation
  octave lattices draw visible grid lines. mb-geom uses 22.5-degree-rotated
  gradients and a rational rotation R=[[0.6,-0.8],[0.8,0.6]] plus an
  i*(37.719,-91.137) offset per octave — do not "simplify" these.
