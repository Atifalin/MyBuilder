# My Builder

A suite of procedural, rule-based builders — worlds, villages, cities and
houses with interiors — that generates a populated game world from recipes
and a seed. No AI is involved in generation: you dictate the rules and the
result is mathematical and deterministic. There is no built-in 3D viewport;
the toolchain generates and exports, and you view results in Godot or
Blender.

## Status

Phase 1 foundation is done: the deterministic core (seeds, schema, noise,
incremental pipeline), the `mybuilder` CLI, and the S2 bindings gate, which
proves the same Rust function returns bit-identical results from the CLI,
from Godot 4.7.1 (GDExtension) and from Blender 5.2.3 (PyO3 module). The
gate itself is verified on macOS arm64 only; Windows and Linux adapter
builds are compile-checked in CI but not yet run there.

Planning documents: [my-builder-plan.md](my-builder-plan.md),
[my-builder-goals-and-packs.md](my-builder-goals-and-packs.md),
[my-builder-algorithms-and-gaps.md](my-builder-algorithms-and-gaps.md).
Contributor rules: [AGENTS.md](AGENTS.md).

## Prerequisites

- [rustup](https://rustup.rs/) — the exact toolchain (currently Rust
  1.99.1 + rustfmt + clippy) is auto-pinned by `rust-toolchain.toml`.
- Godot 4.7.x — only for the Godot adapter.
- Blender 5.2.x — only for the Blender adapter (its bundled Python 3.13 is
  used; a recent CPython 3.11+ on PATH is needed to build the PyO3 crate).
- Tested on macOS arm64; CI covers Linux, macOS and Windows.

## Quick start

```bash
git clone https://github.com/Atifalin/MyBuilder.git && cd MyBuilder
cargo build --workspace
```

Create and inspect a project:

```bash
$ cargo run -p mb-cli -- new my-world --seed 0000000000000001
created project "My World" at my-world
seed: 0000000000000001

$ cargo run -p mb-cli -- validate my-world
ok: no diagnostics

$ cargo run -p mb-cli -- info my-world
name: My World
seed: 0000000000000001
extent: 2048 x 2048 m
terrain: feel=Hilly octaves=5 gain=0.5 lacunarity=2 relief_m=120 ridged_mix=0.15
chunk grid: 8 x 8 chunks of 256 m
vertices per chunk side: 257

$ cargo run -p mb-cli -- hash my-world
project 35ef84f2312f2539a52f1c76752c848a533e818a1d7f5b2f327709f0d15c5594
world   250c175afda026153f85ee35b32938e9eb8405a6d54a9d26bfe0a243c91afb3f

$ cargo run -p mb-cli -- probe
version 0.1.0
probe 2ea5ca993b84f1be29e078663e8f2b0edff151170f71fdd346ade3ffce37f3b7
```

`validate` prints one line per finding as `severity code path: message
(hint: …)`, supports `--json`, and exits 1 when errors exist.

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

```bash
$ cargo run --release -p mb-cli -- build my-world
stage world.masks: ran in 0.008 s
stage world.distance: ran in 0.114 s
stage world.height: ran in 0.301 s
stage world.export: ran in 0.148 s
files written: 72
files unchanged: 0
h_min -40.000 m, h_max 76.419 m
64 chunks, 270400 vertices, 524288 triangles
```

It writes, into the project folder:

```
layers/height.r16            full-res u16 height (2049x2049 for 2048 m)
layers/height_terrain3d.r16  cropped 2048x2048 variant for Terrain3D
layers/land.u8  mountain.u8  forest.u8
layers/layers.json           dims, origin, h_min/h_max, step, file list
previews/hillshade.png       shaded-relief preview, 1 px per 2 m
exports/terrain/chunk_*.glb  one glTF mesh per 256 m chunk (mesh_step 4)
exports/terrain/manifest.json  chunk index + totals (written last, atomic)
```

Only changed files are rewritten (blake3 compare), so a rebuild with no
edits reports `files written: 0`.

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

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Golden tests pin BLAKE3 digests of generator output at
`crates/*/tests/golden/*.blake3` and are the cross-OS determinism gate (CI
runs them on all three OSes). After an intentional output change, run
`MB_BLESS=1 cargo test --workspace` and review the golden diffs before
committing.

Determinism rules in short: no `HashMap` iteration in outputs, no libm
transcendental functions or `mul_add` in generation paths, all randomness
from `mb_seed`, noise evaluated in world coordinates. See
[AGENTS.md](AGENTS.md) for the full list.

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

```bash
python3 -m venv target/maturin-venv
target/maturin-venv/bin/pip install maturin
(cd adapters/blender && ../../target/maturin-venv/bin/maturin build --release -o ../../target/wheels)
/Applications/Blender.app/Contents/Resources/5.2/python/bin/python3.13 \
    -m pip install --target target/blender-site \
    target/wheels/mybuilder_core-0.1.0-cp311-abi3-macosx_11_0_arm64.whl
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
    --python-expr "import sys; sys.path.insert(0,'$PWD/target/blender-site'); \
                   import mybuilder_core as m; print(m.version(), m.probe())"
```

The module exposes `version()`, `probe()`, `sample_fbm(...)` and
`derive_seed_hex(root, [(kind, key), ...])`. The wheel is abi3
(`cp311-abi3`), so it loads in Blender's bundled Python 3.13.

Run the whole gate in one step (uses `GODOT_BIN`/`BLENDER_BIN` env vars or
the /Applications defaults):

```bash
bash scripts/s2-bindings-gate.sh
# prints a table and exits 0 only if CLI, Godot and Blender agree
```

## License

TBD (open decision).
