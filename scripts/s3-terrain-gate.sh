#!/usr/bin/env bash
# S3 terrain gate: new project -> build -> Godot check -> Blender check.
# Env: GODOT_BIN, BLENDER_BIN (defaults under /Applications).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

GODOT_BIN="${GODOT_BIN:-/Applications/Godot.app/Contents/MacOS/Godot}"
BLENDER_BIN="${BLENDER_BIN:-/Applications/Blender.app/Contents/MacOS/Blender}"
GATE_DIR="$ROOT/target/gates/s3"; rm -rf "$GATE_DIR"; PROJ="$GATE_DIR/project"
DEMO="adapters/godot/demo"

echo "== new project (seed 1) =="
rm -rf "$PROJ"
cargo run -q --release -p mb-cli -- new "$PROJ" --seed 0000000000000001

echo "== build =="
cargo run -q --release -p mb-cli -- build "$PROJ"

MANIFEST="$PROJ/exports/terrain/manifest.json"

echo "== Godot check =="
if [ ! -d "$DEMO/.godot" ]; then
  "$GODOT_BIN" --headless --path "$DEMO" --import >/dev/null 2>&1 || true
fi
GODOT_OUT="$("$GODOT_BIN" --headless --path "$DEMO" \
  --script res://import_world.gd -- --manifest "$MANIFEST" 2>&1 || true)"
echo "$GODOT_OUT" | grep -E 'S3|FAIL' || true

echo "== Blender check =="
BLENDER_OUT="$("$BLENDER_BIN" --background --factory-startup \
  --python adapters/blender/scripts/import_world.py -- \
  --manifest "$MANIFEST" --blend "$ROOT/target/gates/s3/world.blend" 2>&1 || true)"
echo "$BLENDER_OUT" | grep -E 'S3|FAIL|Error' | tail -5 || true

ok=1
echo "$GODOT_OUT" | grep -q "S3 GODOT PASS" || ok=0
echo "$BLENDER_OUT" | grep -q "S3 BLENDER PASS" || ok=0
if [ "$ok" = 1 ]; then
  echo "S3 GATE PASS"
  exit 0
fi
echo "S3 GATE FAIL" >&2
exit 1
