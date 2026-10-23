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
