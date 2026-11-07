#!/usr/bin/env bash
# Phase 3 step 6: houses placed on hilly terrain get site heights and
# foundations; Godot loads terrain + houses together and checks them.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/target/gates/house_terrain"
P="$OUT/project"
rm -rf "$OUT"
mkdir -p "$OUT"
MB() { cargo run --release -q -p mb-cli -- "$@" >/dev/null; }
MB new "$P" --size 1024 --feel hilly --start land-first --seed 00000000000000b2
MB house add "$P" --id h1 --footprint rect --w 10 --d 8 --floors 2 --x 0 --z 0
MB house add "$P" --id h2 --footprint rect --w 8 --d 7 --floors 1 --complexity 0 --x 18 --z 4 --rot 1
MB house add "$P" --id h3 --footprint l --w 14 --d 12 --cut-w 6 --cut-d 5 --floors 2 --x -22 --z 6
MB house add "$P" --id h4 --footprint rect --w 10 --d 8 --floors 3 --complexity 2 --x 4 --z -22 --rot 2
MB house add "$P" --id h5 --footprint courtyard --w 20 --d 20 --court-w 8 --court-d 8 --floors 2 --complexity 2 --x 40 --z -30
MB house add "$P" --id h6 --footprint rect --w 9 --d 8 --floors 2 --x -30 --z -28 --rot 3
MB build "$P"
cd "$ROOT/adapters/godot/demo"
LOG=$(/Applications/Godot.app/Contents/MacOS/Godot --path . --script res://houses_terrain_view.gd -- --project="$P" --out="$OUT" 2>&1 || true)
echo "$LOG" | grep -E "HOUSE TERRAIN|SCRIPT ERROR|Parse Error" || true
echo "$LOG" | grep -q "HOUSE TERRAIN PASS"
