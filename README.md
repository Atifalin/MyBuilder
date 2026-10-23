# My Builder

A suite of procedural, rule-based builders — worlds, villages, cities and
houses with interiors — that generates a populated game world from recipes
and a seed. No AI is involved in generation: you dictate the rules and the
result is mathematical and deterministic. There is no built-in 3D viewport;
the toolchain generates and exports, and you view results in Godot or
Blender.

## Status

Phase 1 through S10 are done: deterministic core crates and CLI, the
three-way bindings gate (CLI / Godot 4.7 / Blender 5.2 all returning one
probe hash), terrain generation to chunked glTF imported by both
engines, seamless higher-detail regions, and the persistent rebuild
cache with per-chunk invalidation. Next up: the Tauri app shell.

Planning docs: [my-builder-plan.md](my-builder-plan.md),
[my-builder-goals-and-packs.md](my-builder-goals-and-packs.md),
[my-builder-algorithms-and-gaps.md](my-builder-algorithms-and-gaps.md).

The engine gates are verified on **macOS arm64** only; Windows/Linux
adapter builds are compile-checked in CI.

## Prerequisites

- [rustup](https://rustup.rs/) — the exact toolchain (currently Rust
  1.99.1 + rustfmt + clippy) is auto-pinned by `rust-toolchain.toml`.
- Godot 4.7.x — only for the Godot adapter.
- Blender 5.2.x — only for the Blender adapter (its bundled Python 3.13 is
  used; a recent CPython 3.11+ on PATH is needed to build the PyO3 crate).
- Tested on macOS arm64; CI covers Linux, macOS and Windows.

## Quick start

```sh
git clone https://github.com/Atifalin/MyBuilder.git && cd MyBuilder
cargo build --release -p mb-cli
cargo run --release -p mb-cli -- new my-world --seed 0000000000000001
```
```
created project "My World" at my-world
seed: 0000000000000001
```
```sh
cargo run --release -p mb-cli -- validate my-world
cargo run --release -p mb-cli -- info my-world
cargo run --release -p mb-cli -- hash my-world
cargo run --release -p mb-cli -- probe
```
```
ok: no diagnostics
name: My World
seed: 0000000000000001
extent: 2048 x 2048 m
terrain: feel=Hilly octaves=5 gain=0.5 lacunarity=2 relief_m=120 ridged_mix=0.15
chunk grid: 8 x 8 chunks of 256 m
vertices per chunk side: 257
project dde70d4eb73e82c6fec5da06d078868d5a1c900cd04b3f6b2bf18253d8ad6bed
world   1c233b7481c12a3335f87803ab9010b81bd968bdeb8217f96043dc778e7f3575
version 0.1.0
probe 2ea5ca993b84f1be29e078663e8f2b0edff151170f71fdd346ade3ffce37f3b7
```

## Project folder format

`mybuilder new` creates:

```
my-world/
  project.json        schema version, seed, units, world scale
  recipes/world.json  world recipe (extent, chunks, terrain, shapes)
  layers/  overrides/  cache/  styles/  previews/  exports/
```

`project.json` (defaults):

```json
{
  "schema_version": 1,
  "name": "My World",
  "seed": "0000000000000001",
  "generator_version": 1,
  "units": "meters",
  "up_axis": "y",
  "handedness": "right",
  "world_scale": { "meters_per_unit": 1.0, "cell_size_m": 1.0 },
  "recipes": { "world": "recipes/world.json" }
}
```

`recipes/world.json` (defaults, shapes trimmed for brevity):

```json
{
  "schema_version": 1,
  "extent": {
    "width_m": 2048.0,
    "depth_m": 2048.0
  },
  "chunk_size_m": 256.0,
  "sea_level_m": 0.0,
  "mode": "land_first",
  "terrain": {
    "feel": "hilly",
    "overrides": {}
  },
  "profile": {
    "shelf_depth_m": 40.0,
    "shelf_width_m": 300.0,
    "coast_height_m": 2.0,
    "inland_height_m": 30.0,
    "inland_width_m": 400.0,
    "mountain_height_m": 600.0,
    "mountain_falloff_m": 250.0,
    "feature_size_m": 512.0,
    "snow_line_m": 300.0
  },
  "export": {
    "mesh_step_cells": 4
  },
  "detail_regions": [],
  "shapes": [
    {
      "id": "coast-1",
      "kind": "land",
      "points": [[700.0, 0.0], [610.0, 350.0], "..."]
    }
  ]
}
```

- The default world carries one centered 12-point land blob (~700 m radius)
  so there is always land to build on. Shape `kind` is one of
  `land | water | mountain | forest`.
- `profile` drives the coast-to-inland height curve (algorithms doc 3.4)
  plus mountain weighting and the base noise wavelength
  (`frequency = 1 / feature_size_m`).
- `export.mesh_step_cells` is the vertex stride of exported chunk meshes
  (data stays at 1 m cells).
- `terrain.overrides` may set `octaves`, `gain`, `lacunarity`, `relief_m`,
  `ridged_mix`; each replaces the corresponding preset field.

Terrain feel presets:

| feel | octaves | gain | lacunarity | relief_m | ridged_mix |
|---|---|---|---|---|---|
| bumpy | 4 | 0.45 | 2.0 | 20 | 0.0 |
| hilly | 5 | 0.50 | 2.0 | 120 | 0.15 |
| mountainous | 7 | 0.55 | 2.0 | 800 | 0.6 |

Conventions: meters, right-handed, Y-up, origin at the world centre, 2D
plans in the XZ plane, seeds as 16-char lowercase hex strings.

## Build a world

`mybuilder build <dir>` validates, runs the stage DAG against the
persistent cache, and writes all outputs atomically:

```
global stages: 5 ran, 0 reused (0.671 s)
chunks: 64 ran of 64 (0 cached, 0.021 s)
cache: 0 hits, 69 misses, 0 corrupt, 0 missing, 69 blobs written, 0 deleted
files written: 72
files unchanged: 0
h_min -40.0 m, h_max 67.00906494119648 m
64 chunks, 270400 vertices, 524288 triangles
total wall time: 1.326 s
```

Generated files: `layers/height.r16` + `height_terrain3d.r16` + mask
u8s + `layers.json`; `previews/hillshade.png`;
`exports/terrain/chunk_{cx}_{cz}.glb` (positions, normals, colors, uvs);
`exports/terrain/regions/<id>/chunk_{cx}_{cz}.glb` for detail-region
chunks (coarse entries get `"superseded_by": "<id>"`); and
`exports/terrain/manifest.json` with per-chunk ids, origins, counts and
blake3s.

`detail_regions` in world.json declares higher-detail regions:

```json
"detail_regions": [
  { "id": "village-1", "chunk_min": [2, 2], "chunk_max": [3, 3],
    "cell_divisor": 2, "mesh_step_cells": 1, "extra_octaves": 2,
    "blend_band_m": 16.0 }
]
```

`cell_divisor` splits the grid cell (fine spacing = cell / divisor);
`mesh_step_cells` is the fine vertex stride; `extra_octaves` continues
the base fBm series (max 6); `blend_band_m` is the border band where the
fine surface blends onto the coarse mesh so shared border vertices are
bit-identical and in-between ones sit exactly on the coarse edge.

`generator_version` in project.json pins the algorithm set; old projects
keep old output until `mybuilder upgrade <dir>` bumps it. Files listed
in the previous manifest but not the new one are deleted; every written
or deleted path must pass a confinement check (no `..`, no absolute or
`:` paths) so a tampered manifest can't touch files outside the project.

## Repository layout

| path | crate | purpose |
|---|---|---|
| `crates/seed` | mb-seed | hashing, seed derivation, counter-based RNG, stable IDs, golden-test helper |
| `crates/schema` | mb-schema | project/world recipe types, defaults, versioning, validation, canonical hashing |
| `crates/geom` | mb-geom | integer grid snapping, deterministic Perlin/fBm/ridged noise |
| `crates/pipeline` | mb-pipeline | stage DAG runner with memo cache and early cutoff |
| `crates/bridge` | mb-bridge | engine-agnostic facade; the only crate adapters may call |
| `crates/terrain` | mb-terrain | masks, signed distance (EDT), height, quantized layers, chunk meshes, GLB writer, output bundle |
| `crates/cli` | mb-cli | the `mybuilder` binary |
| `adapters/godot` | mb-godot | Godot GDExtension (`MyBuilderCore` class) + headless demo project |
| `adapters/blender` | mb-blender | PyO3 module `mybuilder_core`, built as an abi3 wheel |
| `scripts/` | — | `s2-bindings-gate.sh`, `s3-terrain-gate.sh` |

## Testing

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Golden tests compare outputs to `crates/*/tests/golden/*.blake3`.
Re-bless with `MB_BLESS=1 MB_BLESS_REASON="why" cargo test` — every
bless appends a row to `GOLDEN_CHANGELOG.md`, and a test enforces that
the last row per golden matches the file, so a forgotten log fails CI.

Gates: `scripts/s2-bindings-gate.sh` (CLI = Godot = Blender probe),
`scripts/s3-terrain-gate.sh` (build + both engine imports),
`scripts/s4-seams-gate.sh` (detail-region seams). Determinism rules in
AGENTS.md.

## Using the core from Godot

```bash
cargo build -p mb-godot --release
mkdir -p adapters/godot/demo/bin
cp target/release/libmb_godot.dylib adapters/godot/demo/bin/
/Applications/Godot.app/Contents/MacOS/Godot --headless \
    --path adapters/godot/demo --import          # once, registers the extension
/Applications/Godot.app/Contents/MacOS/Godot --headless \
    --path adapters/godot/demo --script res://probe.gd
```

prints `version 0.1.0`, `probe 7b70ecba…`, `derive b4c73de3bc9dbdd4`.
In GDScript the facade is the `MyBuilderCore` class with static methods
`version()`, `probe()`, `sample_fbm(...)`, `derive_seed_hex(root, path)`
where path entries are `"kind:key"` strings.

## Using the core from Blender

Build the abi3 wheel with maturin and install it with Blender's own
Python:

```sh
python3 -m venv target/maturin-venv
target/maturin-venv/bin/pip install maturin==1.15.0
target/maturin-venv/bin/maturin build --release -m adapters/blender/Cargo.toml
/Applications/Blender.app/Contents/Resources/5.2/python/bin/python3.13 \
    -m pip install --upgrade --target target/blender-site \
    target/wheels/mybuilder_core-0.1.0-cp311-abi3-macosx_11_0_arm64.whl
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
    --python-expr "import sys; sys.path.insert(0,'$PWD/target/blender-site'); import mybuilder_core as m; print('probe', m.probe())"
# probe 2ea5ca993b84f1be29e078663e8f2b0edff151170f71fdd346ade3ffce37f3b7
```

Import a built world (checks object count, world bounds, height range):

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
    --python adapters/blender/scripts/import_world.py -- \
    --manifest /abs/path/to/exports/terrain/manifest.json
# S3 BLENDER PASS
```

## Rebuilds and the persistent cache (S10)

`mybuilder build` keeps a content-addressed cache in the OS cache dir —
`<os cache>/MyBuilder/projects/<name>-<16-hex>/` (macOS
`~/Library/Caches`, Linux `$XDG_CACHE_HOME`/`~/.cache`, Windows
`%LOCALAPPDATA%`) so cloud-sync tools never touch it. Print it with
`mybuilder cache-path <dir>`; override with `--cache-dir` or
`MB_CACHE_DIR`. `.mybuilder.lock` in the project is an OS advisory lock
held for the whole build — a concurrent build fails fast with
"another build is running on this project". Output deletes never follow
symlinks; writes still do (exports/ may be symlinked into a Godot
project). Chunk
stages are sliced, so edits re-run only the affected chunks plus their
edge neighbours (shared borders need neighbouring normals).

Real runs on the default 2048 m world (release build):

```
# fresh build (above): 1.326 s, 72 files written
# no-op rebuild:
global stages: 0 ran, 5 reused (0.000 s)
chunks: 0 ran of 64 (64 cached, 0.000 s)
cache: 69 hits, 0 misses, 0 corrupt, 0 missing, 0 blobs written, 0 deleted
files written: 0
total wall time: 0.646 s
# one shape point moved 15 m:
global stages: 5 ran, 0 reused (0.657 s)
chunks: 14 ran of 64 (50 cached, 0.005 s)
files written: 19
total wall time: 1.299 s
# full wipe + rebuild:
mybuilder build --clean --yes my-world   # deletes cache + listed outputs
# machine-readable event stream for UIs:
mybuilder build --json-events my-world   # NDJSON on stdout, text -> stderr
```

`--json-events` emits one `BuildEvent` JSON object per line; the first
is `started` and the last is `finished`:

```
{"type":"started","mode":"incremental","stages_total":69,"chunks_total":64}
{"type":"stage_started","stage":"world.masks","group":"global"}
{"type":"stage_finished","stage":"world.masks","group":"global","executed":true,"ms":11.587}
{"type":"finished","status":"ok","error":null,"ms":1340.2}
```

Write `cancel` to stdin to abort (exit code 2). `--full` recomputes
everything (cache reads are skipped but still written).
`mybuilder clear my-world --seed 42 [--drop-overrides] --yes` wipes the
cache + listed outputs and reseeds without building.

The UI also gets `mb_bridge::Session` (validated recipe edits with
undo/redo) and `mb_bridge::PreviewSource` (RGBA PNG heightfield tiles;
zoom z -> 8·2^-z m/px, 256-px tiles aligned to the world min corner).

The cache is ~186 MB for the default world (mostly the f64 height and
distance planes) — fine to delete anytime; it is rebuilt on demand.

## Control app (desktop)

`apps/desktop/` is a Tauri 2 control app over the mb-bridge API
(kept out of the root workspace; its own Cargo.lock, output under
`target/app/`). Run it with plain cargo — no tauri-cli needed, since
`frontendDist` serves the static `ui/` folder via the `custom-protocol`
feature:

```sh
cd apps/desktop/src-tauri && cargo run
```

Run it from that folder: its `.cargo/config.toml` sends build output to the
root `target/app/`. Using `--manifest-path` from the repo root skips that
config and creates a second multi-GB `target/` inside `apps/desktop/src-tauri/`.

Controls: toolbar drawing tools (Pan, Land, Water, Mountain, Forest,
Select+Delete — freehand strokes are clamped, Douglas–Peucker
simplified, Chaikin-smoothed and mm-snapped before they are stored in
the recipe, so shapes are undoable and deterministic), Open project…
(folder picker), feel dropdown, octaves slider
with a "use preset" checkbox, Undo/Redo (Cmd/Ctrl+Z, Shift+Cmd/Ctrl+Z),
Build (full rebuild), Rebuild (incremental), Clear all & new seed
(modal with "keep overrides"), Cancel, a tiled hillshade map (drag to
pan, wheel to zoom z∈[-2,6] at 8·2^-z m/px), and a live log with
progress and per-stage timings.

Limits today: terrain editing is feel/octaves plus shape draw/remove;
no edit-by-drag or vertex editing yet, and the app is macOS-verified
only.

## Disk usage

`target/` holds all build artifacts and gate projects (several GB;
`cargo clean` frees it). Each project's `cache/` is safe to delete
anytime — it is rebuilt on the next build, or use
`mybuilder build --clean --yes`.

## License

TBD (open decision).
