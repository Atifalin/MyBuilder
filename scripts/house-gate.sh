#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
P="$ROOT/target/gates/house/project"
rm -rf "$ROOT/target/gates/house"
mkdir -p "$P"
cargo run --release -p mb-cli -- new "$P" --size 1024 >/dev/null
cargo run --release -p mb-cli -- house add "$P" --id rect --footprint rect --w 10 --d 8 --floors 2 >/dev/null
cargo run --release -p mb-cli -- house add "$P" --id ell --footprint l --w 14 --d 12 --cut-w 6 --cut-d 5 --floors 2 --complexity 2 --front e >/dev/null
cargo run --release -p mb-cli -- house add "$P" --id court --footprint courtyard --w 18 --d 18 --court-w 8 --court-d 8 --floors 1 >/dev/null
cargo run --release -p mb-cli -- build "$P" >/dev/null
MANIFEST=$(cd "$P/exports/houses" && pwd)/manifest.json
cd "$ROOT/adapters/godot/demo"
/Applications/Godot.app/Contents/MacOS/Godot --path . --script res://houses_view.gd -- --manifest="$MANIFEST" --out="$ROOT/target/gates/house" 2>&1 | grep -E "HOUSE|error" | tail -5
