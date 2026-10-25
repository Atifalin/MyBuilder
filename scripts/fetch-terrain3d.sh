#!/usr/bin/env bash
# Download Terrain3D (TokisanGames) into the Godot demo project's addons/.
# The directory is gitignored — binaries are never committed.
# Usage: scripts/fetch-terrain3d.sh [version]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${1:-v1.0.2-stable}"
URL="https://github.com/TokisanGames/Terrain3D/releases/download/${VER}/Terrain3D_${VER}.zip"
DEST="$ROOT/adapters/godot/demo/addons"
TMP="$ROOT/target/terrain3d"
mkdir -p "$TMP" "$DEST"
curl -L --retry 3 -C - -m 300 -o "$TMP/terrain3d.zip" "$URL"
rm -rf "$TMP/x" "$DEST/terrain_3d"
unzip -q -o "$TMP/terrain3d.zip" "addons/terrain_3d/*" -d "$TMP/x"
cp -R "$TMP/x/addons/terrain_3d" "$DEST/"
# macOS Gatekeeper quarantines downloaded frameworks; strip it or the
# extension fails to load with a generic dlopen error.
if [[ "$(uname)" == "Darwin" ]]; then
  xattr -dr com.apple.quarantine "$DEST/terrain_3d" 2>/dev/null || true
fi
echo "Terrain3D $VER installed to $DEST/terrain_3d"
