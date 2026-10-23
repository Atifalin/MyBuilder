#!/usr/bin/env bash
# S2 bindings gate: build the CLI + Godot + Blender adapters in release
# and prove that all three print the SAME `probe <hex>` and `version X`.
#
# Env overrides:
#   GODOT_BIN   default /Applications/Godot.app/Contents/MacOS/Godot
#   BLENDER_BIN default /Applications/Blender.app/Contents/MacOS/Blender
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

GODOT_BIN="${GODOT_BIN:-/Applications/Godot.app/Contents/MacOS/Godot}"
BLENDER_BIN="${BLENDER_BIN:-/Applications/Blender.app/Contents/MacOS/Blender}"
BLENDER_PY="$(dirname "$BLENDER_BIN")/../Resources/5.2/python/bin/python3.13"
DEMO="adapters/godot/demo"
SITE="$ROOT/target/blender-site"
WHEEL_DIR="$ROOT/target/wheels"

echo "== building workspace (release) =="
cargo build --release -p mb-cli -p mb-godot 2>&1 | tail -1

echo "== CLI probe =="
CLI_OUT="$(cargo run -q --release -p mb-cli -- probe)"
echo "$CLI_OUT"

echo "== Godot probe =="
mkdir -p "$DEMO/bin"
cp target/release/libmb_godot.dylib "$DEMO/bin/" 2>/dev/null \
  || cp target/release/mb_godot.dll "$DEMO/bin/" 2>/dev/null \
  || cp target/release/libmb_godot.so "$DEMO/bin/"
# First run in a fresh checkout needs an editor import pass so the
# GDExtension is registered in .godot/global_script_class_cache.cfg.
# That pass may crash on teardown under --headless (engine teardown bug,
# not ours), so its exit code is ignored; the class cache is still written.
if [ ! -d "$DEMO/.godot" ]; then
  "$GODOT_BIN" --headless --path "$DEMO" --import >/dev/null 2>&1 || true
fi
GODOT_OUT="$("$GODOT_BIN" --headless --path "$DEMO" --script res://probe.gd 2>&1 || true)"
echo "$GODOT_OUT" | grep -E '^(version|probe|derive) '

echo "== Blender probe (maturin wheel) =="
if [ ! -x "$ROOT/target/maturin-venv/bin/maturin" ]; then
  python3 -m venv "$ROOT/target/maturin-venv"
  "$ROOT/target/maturin-venv/bin/pip" install -q maturin
fi
(cd adapters/blender && "$ROOT/target/maturin-venv/bin/maturin" build --release -o "$WHEEL_DIR" -q)
WHEEL="$(ls -t "$WHEEL_DIR"/mybuilder_core-*.whl | head -1)"
echo "wheel: $(basename "$WHEEL")"
"$BLENDER_PY" -m pip install -q --upgrade --target "$SITE" "$WHEEL"
BLENDER_OUT="$("$BLENDER_BIN" --background --factory-startup --python-expr \
  "import sys; sys.path.insert(0, '$SITE'); import mybuilder_core as m; print('version', m.version()); print('probe', m.probe()); print('derive', m.derive_seed_hex('0000000000000001', [('region','r0'),('settlement','s0')]))" 2>&1)"
echo "$BLENDER_OUT" | grep -E '^(version|probe|derive) '

echo "== comparison =="
get() { echo "$1" | awk -v k="$2" '$1 == k {print $2}'; }
CLI_V="$(get "$CLI_OUT" version)"; CLI_P="$(get "$CLI_OUT" probe)"
GOD_V="$(get "$GODOT_OUT" version)"; GOD_P="$(get "$GODOT_OUT" probe)"
BLE_V="$(get "$BLENDER_OUT" version)"; BLE_P="$(get "$BLENDER_OUT" probe)"
printf '%-10s %-10s %s\n' adapter version probe
printf '%-10s %-10s %s\n' cli "$CLI_V" "$CLI_P"
printf '%-10s %-10s %s\n' godot "$GOD_V" "$GOD_P"
printf '%-10s %-10s %s\n' blender "$BLE_V" "$BLE_P"

if [ -z "$CLI_P" ] || [ -z "$GOD_P" ] || [ -z "$BLE_P" ]; then
  echo "FAIL: an adapter produced no probe line" >&2
  exit 1
fi
if [ "$CLI_P" = "$GOD_P" ] && [ "$GOD_P" = "$BLE_P" ] \
   && [ "$CLI_V" = "$GOD_V" ] && [ "$GOD_V" = "$BLE_V" ]; then
  echo "PASS: all three adapters agree"
  exit 0
fi
echo "FAIL: probe mismatch" >&2
exit 1
