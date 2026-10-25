#!/usr/bin/env bash
# Build a project (release) and run the windowed fly-bench.
# Usage: scripts/fly-bench.sh <project dir> [bench seconds]
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GODOT_BIN="${GODOT_BIN:-/Applications/Godot.app/Contents/MacOS/Godot}"
DIR="$1"
SEC="${2:-25}"
"$ROOT/target/release/mybuilder" build "$DIR" >/dev/null 2>&1 || cargo run --release -p mb-cli -- build "$DIR" >/dev/null
export MB_FLY_PNG="${MB_FLY_PNG:-$ROOT/target/gates/fly/fly.png}"
export MB_FLY_TOP_PNG="${MB_FLY_TOP_PNG:-$ROOT/target/gates/fly/fly_top.png}"
"$GODOT_BIN" --disable-vsync --max-fps 0 --path "$ROOT/adapters/godot/demo" res://fly_world.tscn -- --manifest "$DIR/exports/terrain/manifest.json" --bench "$SEC" 2>&1 | grep -E "BENCH|saved|WARNING|error|ERROR" || true
