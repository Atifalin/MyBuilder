#!/usr/bin/env bash
# S4 gate: fixture world with a 2x2-chunk detail region near the coast,
# then the S3 engine checks (which load region chunks for superseded
# coarse chunks). Env: GODOT_BIN, BLENDER_BIN.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"
GODOT_BIN="${GODOT_BIN:-/Applications/Godot.app/Contents/MacOS/Godot}"
BLENDER_BIN="${BLENDER_BIN:-/Applications/Blender.app/Contents/MacOS/Blender}"
GATE_DIR="$ROOT/target/gates/s4"; rm -rf "$GATE_DIR"; PROJ="$GATE_DIR/project"; DEMO="adapters/godot/demo"
rm -rf "$PROJ"
cargo run -q --release -p mb-cli -- new "$PROJ" --seed 0000000000000001 >/dev/null
cp scripts/fixtures/s4-world.json "$PROJ/recipes/world.json"
echo "== build =="
cargo run -q --release -p mb-cli -- build "$PROJ"
MANIFEST="$PROJ/exports/terrain/manifest.json"
[ -d "$DEMO/.godot" ] || "$GODOT_BIN" --headless --path "$DEMO" --import >/dev/null 2>&1 || true
GODOT_OUT="$("$GODOT_BIN" --headless --path "$DEMO" --script res://import_world.gd -- --manifest "$MANIFEST" 2>&1 || true)"
echo "$GODOT_OUT" | grep -E 'S3|FAIL' || true
BLENDER_OUT="$("$BLENDER_BIN" --background --factory-startup --python adapters/blender/scripts/import_world.py -- --manifest "$MANIFEST" --blend "$ROOT/target/gates/s4/world.blend" 2>&1 || true)"
echo "$BLENDER_OUT" | grep -E 'S3|FAIL|Error' | tail -3 || true
ok=1
echo "$GODOT_OUT" | grep -q "S3 GODOT PASS" || ok=0
echo "$BLENDER_OUT" | grep -q "S3 BLENDER PASS" || ok=0
# screenshots at a region border, normal + wireframe
"$GODOT_BIN" --path "$DEMO" --script res://import_world.gd -- --manifest "$MANIFEST" --png "$ROOT/target/gates/s4/godot_border.png" --border 512,256 >/dev/null 2>&1 || true
MB_WIRE=1 "$GODOT_BIN" --path "$DEMO" --script res://import_world.gd -- --manifest "$MANIFEST" --png "$ROOT/target/gates/s4/godot_border_wire.png" --border 512,256 >/dev/null 2>&1 || true
[ "$ok" = 1 ] && { echo "S4 GATE PASS"; exit 0; }
echo "S4 GATE FAIL" >&2; exit 1
