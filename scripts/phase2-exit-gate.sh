#!/usr/bin/env bash
# Phase 2 exit gate: wizard autotest + fly bench on the created project.
# PASS only if the autotest passed AND the bench meets the steady bars.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$ROOT/target/gates/exit"
APP="$ROOT/target/app/release/mb-app"
mkdir -p "$GATE"

echo "== build app (release: debug terrain gen is too slow for the gate) =="
(cd "$ROOT/apps/desktop/src-tauri" && cargo build --release) || { echo "EXIT GATE FAIL: app build"; exit 1; }

rm -rf "$GATE/Wizard World"
echo "== wizard autotest =="
MB_APP_AUTOTEST_NEW="$GATE" "$APP" 2>&1 | grep -E "AUTOTEST_RESULT|autotestNew failed|timeout" | tee "$GATE/autotest.log" | tail -5
# autotest exit code is the verdict; grep pipeline hides it, so re-check the JSON.
RESULT_LINE=$(grep "AUTOTEST_RESULT" "$GATE/autotest.log" | tail -1)
APP_RC=$?

echo "== fly bench =="
PROJ="$GATE/Wizard World"
if [ ! -f "$PROJ/exports/terrain/manifest.json" ]; then
  echo "EXIT GATE FAIL: no manifest at $PROJ"; exit 1
fi
export MB_FLY_PNG="$GATE/fly.png" MB_FLY_TOP_PNG="$GATE/fly_top.png"
bash "$ROOT/scripts/fly-bench.sh" "$PROJ" 25 | tee "$GATE/bench.log" | tail -5

BENCH=$(grep -o 'BENCH {.*}' "$GATE/bench.log" | tail -1)
echo "$BENCH"

# Extract the fields with python for robust JSON parsing.
read P1 MAXF <<<"$(python3 - "$BENCH" <<'EOF'
import json,sys
try:
  b=json.loads(sys.argv[1][6:]); print(b["p1_low_fps"], b["max_frame_ms"])
except Exception: print(0, 999)
EOF
)"
AUTOTEST_OK=0
echo "$RESULT_LINE" | python3 -c "
import json,sys
line=sys.stdin.read().split('AUTOTEST_RESULT ',1)[-1].strip()
try:
  r=json.loads(line)
  ok=(r.get('wizard') and r.get('created') and r.get('status_build')=='ok'
      and r.get('status_rebuild')=='ok' and r.get('manifest')
      and r.get('land_shape') and r.get('water_shape')
      and r.get('undo_ok') and r.get('redo_ok')
      and (r.get('tiles_alpha') or 0)>0 and r.get('unhandled',9)==0)
  sys.exit(0 if ok else 1)
except Exception: sys.exit(1)" && AUTOTEST_OK=1

STEADY=$(python3 -c "print(1 if float('$P1')>=60 and float('$MAXF')<33 else 0)")
echo "autotest_ok=$AUTOTEST_OK p1_low_fps=$P1 max_frame_ms=$MAXF"
echo "screenshots: $GATE/fly.png $GATE/fly_top.png"
if [ "$AUTOTEST_OK" = 1 ] && [ "$STEADY" = 1 ]; then
  echo "PHASE 2 EXIT GATE PASS"
  exit 0
fi
echo "PHASE 2 EXIT GATE FAIL"
exit 1
