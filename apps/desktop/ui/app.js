/* global window */
const T = window.__TAURI__;
// Autotest/debug: forward console + errors to Rust stderr.
const fwd = (lvl, args) => {
  try { T.core.invoke("js_log", { level: lvl, msg: args.map(String).join(" ") }).catch(()=>{}); } catch {}
};
for (const m of ["log","warn","error","info"]) {
  const orig = console[m].bind(console);
  console[m] = (...a) => { orig(...a); fwd(m, a); };
}
window.onerror = (msg, src, line, col, err) => { fwd("error", [`${msg} @${src}:${line}:${col}`, err&&err.stack]); };
window.onunhandledrejection = (e) => { fwd("error", ["unhandledrejection", e.reason && (e.reason.stack||e.reason)]); };
const { invoke } = T.core;
const { listen } = T.event;
const $ = (id) => document.getElementById(id);
const log = (txt, cls) => {
  const el = $("log"); const line = document.createElement("span");
  if (cls) line.className = cls;
  line.textContent = txt + "\n"; el.appendChild(line);
  el.scrollTop = el.scrollHeight;
};
let st = null;            // server state
let z = 0, panX = 0, panY = 0, fit = false;
const tiles = new Map(); // key "z/tx/ty" -> HTMLImageElement
let pending = new Set();
const ORIGIN = { x: 0, z: 0 }; // world min corner after fit
let world = { w: 2048, d: 2048, ox: 0, oz: 0 };

async function refresh() {
  try {
    st = JSON.parse(await invoke("state"));
  } catch { return; }
  $("projName").textContent = st.name;
  $("seed").textContent = "seed " + st.seed;
  $("feel").value = st.feel;
  $("oct").value = st.octaves;
  $("octVal").textContent = st.octaves;
  $("octPreset").checked = !st.octaves_is_override;
  $("oct").disabled = !st.octaves_is_override || st.building;
  $("dirty").classList.toggle("hidden", !st.dirty);
  $("undoBtn").disabled = !st.can_undo || st.building;
  $("redoBtn").disabled = !st.can_redo || st.building;
  for (const id of ["feel","oct","octPreset","buildBtn","rebuildBtn","clearBtn"])
    $(id).disabled = st.building;
  $("cancelBtn").disabled = !st.building;
  $("emptyHint").classList.toggle("hidden", !!st.has_preview);
  if (!fit && st.has_preview) fitWorld();
  draw();
}
function fitWorld() {
  const c = $("map");
  const zf = Math.log2(Math.max(c.width,1) / Math.max(world.w,1) * 8);
  z = Math.max(-2, Math.min(6, Math.floor(zf)));
  panX = 0; panY = 0; fit = true;
}
const mpp = () => 8 * Math.pow(2, -z);
const tileSize = 256;
function worldToPx(x, zz) { const c=$("map"); return (x - world.ox)/mpp() + panX + c.width/2 - world.w/mpp()/2, 0; }
function draw() {
  const c = $("map"), ctx = c.getContext("2d");
  if (c.width !== c.clientWidth) { c.width = c.clientWidth; c.height = c.clientHeight; }
  ctx.clearRect(0,0,c.width,c.height);
  if (!st || !st.has_preview) return;
  // visible tile range at zoom z; tiles aligned to world min corner
  const ts = tileSize * mpp(); // world meters per tile
  const x0 = world.ox - panX * mpp() / tileSize * tileSize - c.width/2 * mpp() + world.w/2;
  const z0 = world.oz - panY * mpp() / tileSize * tileSize - c.height/2 * mpp() + world.d/2;
  const nx = Math.ceil(c.width / tileSize) + 1, nz = Math.ceil(c.height / tileSize) + 1;
  const tx0 = Math.floor(x0 / ts), tz0 = Math.floor(z0 / ts);
  for (let tz = tz0; tz < tz0 + nz; tz++) for (let tx = tx0; tx < tx0 + nx; tx++) {
    const key = `${z}/${tx}/${tz}`;
    let img = tiles.get(key);
    if (img === undefined && !pending.has(key)) {
      pending.add(key);
      invoke("preview_tile", { x0: tx*ts, z0: tz*ts, x1: tx*ts+ts, z1: tz*ts+ts, mpp: mpp() })
        .then((bytes) => {
          const blob = new Blob([new Uint8Array(bytes)], {type:"image/png"});
          const im = new Image();
          im.onload = () => { tiles.set(key, im); pending.delete(key); draw(); };
          im.src = URL.createObjectURL(blob);
        }).catch(() => { tiles.set(key, null); pending.delete(key); });
    }
    const sx = tx*tileSize + c.width/2 - world.w/mpp()/2 + world.w/ts*0 - x0/ts*tileSize;
    const sy = tz*tileSize + c.height/2 - world.d/mpp()/2 - z0/ts*tileSize;
    if (img) ctx.drawImage(img, sx, sy, tileSize, tileSize);
  }
}
// pan + zoom
let drag = null;
const cv = $("map");
cv.addEventListener("mousedown", (e) => { drag = {x:e.clientX, y:e.clientY}; cv.style.cursor="grabbing"; });
window.addEventListener("mouseup", () => { drag = null; cv.style.cursor="grab"; });
window.addEventListener("mousemove", (e) => {
  if (!drag) return;
  panX += e.clientX - drag.x; panY += e.clientY - drag.y;
  drag = {x:e.clientX, y:e.clientY}; draw();
});
cv.addEventListener("wheel", (e) => {
  e.preventDefault();
  const oz = z; z = Math.max(-2, Math.min(6, z + (e.deltaY < 0 ? 1 : -1)));
  if (z !== oz) { const s = mpp()/ (8*Math.pow(2,-oz)); panX = e.offsetX - (e.offsetX-panX)*(1/s); panY = e.offsetY - (e.offsetY-panY)*(1/s); draw(); }
});
// controls
$("openBtn").onclick = async () => {
  const sel = await T.dialog.open({ directory: true });
  if (sel) { await invoke("open_project", { path: sel }); tiles.clear(); fit=false; refresh(); }
};
$("feel").onchange = async (e) => { await invoke("set_feel", { feel: e.target.value }); refresh(); };
$("octPreset").onchange = async (e) => {
  await invoke("set_octaves", { octaves: e.target.checked ? null : parseInt($("oct").value) });
  refresh();
};
$("oct").onchange = async (e) => { if (!$("octPreset").checked) { await invoke("set_octaves", { octaves: parseInt(e.target.value) }); refresh(); } };
$("oct").oninput = (e) => $("octVal").textContent = e.target.value;
$("undoBtn").onclick = async () => { await invoke("undo"); refresh(); };
$("redoBtn").onclick = async () => { await invoke("redo"); refresh(); };
window.addEventListener("keydown", (e) => {
  if ((e.metaKey||e.ctrlKey) && e.key === "z") { e.preventDefault(); invoke(e.shiftKey?"redo":"undo").then(refresh); }
});
$("buildBtn").onclick = () => invoke("build", { mode: "full" }).then(refresh);
$("rebuildBtn").onclick = () => invoke("build", { mode: "incremental" }).then(refresh);
$("cancelBtn").onclick = () => invoke("cancel_build");
$("clearBtn").onclick = () => $("modal").classList.remove("hidden");
$("mCancel").onclick = () => $("modal").classList.add("hidden");
$("mConfirm").onclick = async () => {
  $("modal").classList.add("hidden");
  await invoke("clear_all", { keep_overrides: $("keepOv").checked });
  tiles.clear(); refresh();
};
$("clearLog").onclick = () => $("log").textContent = "";
// build events
let reused = 0;
listen("build-event", (ev) => {
  const e = ev.payload;
  switch (e.type) {
    case "started": reused=0; log(`build ${e.mode}: ${e.stages_total} stages, ${e.chunks_total} chunks`); break;
    case "stage_started": $("stageLine").textContent = `${e.stage}`; break;
    case "stage_finished":
      if (e.executed) log(`  ${e.stage} — ${e.ms.toFixed(1)} ms`); else reused++;
      break;
    case "progress":
      $("pbar").style.width = e.percent + "%";
      $("stageLine").textContent = `${e.stage} · chunks ${e.chunks_done}/${e.chunks_total}`;
      break;
    case "diagnostic": {
      const cls = e.severity === "error" ? "log-err" : e.severity === "warning" ? "log-warn" : "log-info";
      log(`${e.severity} ${e.code}${e.entity_id?` [${e.entity_id}]`:""}: ${e.message}${e.hint?` (hint: ${e.hint})`:""}`, cls);
      break;
    }
    case "files_written": log(`files: ${e.written} written, ${e.unchanged} unchanged, ${e.deleted} deleted${e.skipped_unsafe?`, ${e.skipped_unsafe} skipped unsafe`:""}`); break;
    case "finished":
      if (reused) log(`  … + ${reused} reused`);
      $("pbar").style.width = "0"; $("stageLine").textContent = "";
      log(`build ${e.status} in ${e.ms.toFixed(0)} ms${e.error?` — ${e.error}`:""}`, e.status==="ok"?"log-ok":"log-err");
      break;
  }
});
listen("preview-updated", () => { tiles.clear(); refresh(); });
// autotest: frontend-driven script when MB_APP_AUTOTEST is set
async function autotest() {
  const rep = { events: {}, tiles_alpha: 0, can_undo: false, can_redo: false, status1: "", status2: "" };
  const done = (status) => new Promise((res) => {
    const un = listen("build-event", (ev) => {
      const e = ev.payload;
      rep.events[e.type] = (rep.events[e.type]||0)+1;
      if (e.type === "finished") { un.then(u=>u()); res(e.status); }
    });
  });
  await new Promise(r => setTimeout(r, 300));
  fwd("info", ["autotest: set_feel"]);
  await invoke("set_feel", { feel: "mountainous" });
  fwd("info", ["autotest: build start"]);
  const p1 = done(); invoke("build", { mode: "incremental" }); rep.status1 = await p1;
  fwd("info", ["autotest: build1 done", rep.status1]);
  // count drawn tiles with alpha
  const ts = tileSize * mpp();
  const img = tiles.get(`${z}/0/0`);
  if (img) {
    const c2 = document.createElement("canvas"); c2.width=c2.height=tileSize;
    const cx = c2.getContext("2d"); cx.drawImage(img,0,0);
    rep.tiles_alpha = cx.getImageData(0,0,tileSize,tileSize).data.filter((_,i)=>i%4===3&&_).length;
  } else rep.tiles_alpha = -1;
  fwd("info", ["autotest: undo"]);
  await invoke("undo");
  const p2 = done(); invoke("build", { mode: "incremental" }); rep.status2 = await p2;
  fwd("info", ["autotest: build2 done", rep.status2]);
  const st2 = JSON.parse(await invoke("state"));
  rep.can_undo = st2.can_undo; rep.can_redo = st2.can_redo;
  await invoke("autotest_report", { report: JSON.stringify(rep) });
}
async function boot() {
  try {
    fwd("info", ["app.js boot, __TAURI__ keys:", Object.keys(T||{})]);
    if (await invoke("autotest_mode")) autotest();
  } catch (e) { fwd("error", ["boot failed", e]); }
}
if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot);
else boot();
refresh();
