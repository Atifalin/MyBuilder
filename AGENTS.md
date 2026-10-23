# My Builder — agent notes

## Commands

- Build: `cargo build --workspace`
- Test: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt` (check with `cargo fmt --check`)
- CLI: `cargo run -p mb-cli -- <new|validate|hash|info> ...`

The toolchain is pinned by `rust-toolchain.toml`; rustfmt and clippy are
required components.

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
- No libm transcendental functions (`sin`, `cos`, `exp`, `powf`, `sqrt`,
  `atan`, ...) in generation paths — they differ across OSes. Write needed
  constants as literals. No `mul_add` / fused multiply-add.
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
