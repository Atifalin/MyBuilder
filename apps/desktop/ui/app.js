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
const safe = (f) => async (...a) => { try { await f(...a); } catch (e) { (typeof log==="function"?log:console.error)(String(e), "log-err"); } };
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
const cv = $("map");
const ORIGIN = { x: 0, z: 0 }; // world min corner after fit
let world = { w: 2048, d: 2048, ox: 0, oz: 0 };

async function refresh() {
  try {
    st = JSON.parse(await invoke("state"));
  } catch (e) { if (st) log("refresh: " + e, "log-err"); return; }
  $("projName").textContent = st.name;
  document.body.classList.add("open");
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
  refreshShapes();
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
  // world extent rectangle (always, even with no preview)
  const [ex0,ez0] = w2c(world.ox, world.oz);
  const [ex1,ez1] = w2c(world.ox+world.w, world.oz+world.d);
  ctx.strokeStyle = "#666"; ctx.lineWidth = 1;
  ctx.strokeRect(ex0, ez0, ex1-ex0, ez1-ez0);
  // shape outlines
  for (const s of shapeList) {
    ctx.strokeStyle = KIND_COLOR[s.kind] || "#ccc";
    ctx.lineWidth = s.id === selId ? 3 : 1.5;
    ctx.beginPath();
    s.points.forEach(([x,z],i) => { const [cx,cy]=w2c(x,z); i?ctx.lineTo(cx,cy):ctx.moveTo(cx,cy); });
    ctx.closePath(); ctx.stroke();
  }
  // live stroke
  if (stroke && stroke.length) {
    ctx.strokeStyle = KIND_COLOR[tool] || "#fff"; ctx.lineWidth = 2;
    ctx.beginPath();
    stroke.forEach(([x,z],i)=>{ const [cx,cy]=w2c(x,z); i?ctx.lineTo(cx,cy):ctx.moveTo(cx,cy); });
    ctx.stroke();
  }
}
// ---- shape tools ----
let tool = "pan";
let stroke = null;      // [{x,z}] world meters while drawing
let selId = null;
let shapeList = [];     // cached shapes for overlay+select
const KIND_COLOR = { land:"#4a9e4a", water:"#3a7bd5", mountain:"#8a6a3a", forest:"#2e6b34" };
document.querySelectorAll(".tool").forEach(b => b.onclick = () => {
  document.querySelectorAll(".tool").forEach(x=>x.classList.remove("on"));
  b.classList.add("on"); tool = b.dataset.t;
});
$("delBtn").onclick = safe(async () => {
  if (selId) { await invoke("remove_shape", { id: selId }); selId=null; await refreshShapes(); refresh(); }
});
window.addEventListener("keydown", (e) => {
  if ((e.key==="Delete"||e.key==="Backspace") && selId)
    invoke("remove_shape",{id:selId}).then(()=>{selId=null;refreshShapes();refresh();}).catch(e=>log(String(e),"log-err"));
  if (e.key==="Escape") stroke=null;
});
async function refreshShapes() {
  try { shapeList = JSON.parse(await invoke("shapes")); } catch { shapeList=[]; }
  draw();
}
// world meters <-> canvas px (world extent fills height-ish view via fit)
function w2c(x, z) {
  const c = $("map");
  return [
    (x - world.ox)/mpp() + panX + c.width/2 - world.w/mpp()/2,
    (z - world.oz)/mpp() + panY + c.height/2 - world.d/mpp()/2,
  ];
}
function c2w(px, py) {
  const c = $("map");
  return [
    (px - panX - c.width/2 + world.w/mpp()/2)*mpp() + world.ox,
    (py - panY - c.height/2 + world.d/mpp()/2)*mpp() + world.oz,
  ];
}
function pip(px, pz, pts) { // point in polygon
  let inside = false;
  for (let i=0,j=pts.length-1;i<pts.length;j=i++) {
    const [xi,zi]=pts[i],[xj,zj]=pts[j];
    if ((zi>pz)!==(zj>pz) && px < (xj-xi)*(pz-zi)/(zj-zi)+xi) inside=!inside;
  }
  return inside;
}
// pointer: draw/select vs pan
cv.addEventListener("pointerdown", (e) => {
  if (e.button !== 0) return;
  if (tool === "pan") return; // pan handled below
  const [wx,wz] = c2w(e.offsetX, e.offsetY);
  if (tool === "select") {
    let best=null, bestArea=1e30;
    for (const s of shapeList)
      if (pip(wx,wz,s.points)) {
        let a=0; for (let i=0,j=s.points.length-1;i<s.points.length;j=i++)
          a += s.points[j][0]*s.points[i][1]-s.points[i][0]*s.points[j][1];
        a=Math.abs(a/2); if (a<bestArea){bestArea=a;best=s.id;}
      }
    selId=best; $("delBtn").disabled=!best; draw();
  } else {
    stroke = [[wx,wz]];
  }
});
window.addEventListener("pointermove", (e) => {
  if (!stroke) return;
  const rect = cv.getBoundingClientRect();
  const [wx,wz] = c2w(e.clientX-rect.left, e.clientY-rect.top);
  const last = stroke[stroke.length-1];
  const [lx,ly] = w2c(last[0],last[1]);
  const [cx,cy] = w2c(wx,wz);
  if (Math.hypot(cx-lx,cy-ly) >= 1.5) stroke.push([wx,wz]);
  draw();
});
window.addEventListener("pointerup", async (e) => {
  if (!stroke) return;
  const s = stroke; stroke = null;
  if (s.length >= 3) {
    try {
      const id = await invoke("add_shape", { kind: tool, stroke: s, toleranceM: 1.5*mpp() });
      log(`added ${tool} ${id} (${s.length} pts)`, "log-ok");
      await refreshShapes(); refresh();
    } catch (e) { log(String(e), "log-err"); }
  }
  draw();
});
// pan + zoom
let drag = null;
cv.addEventListener("mousedown", (e) => { if (tool==="pan") { drag = {x:e.clientX, y:e.clientY}; cv.style.cursor="grabbing"; } });
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
$("openBtn").onclick = safe(async () => {
  const sel = await T.dialog.open({ directory: true });
  if (sel) { await invoke("open_project", { path: sel }); tiles.clear(); fit=false; refresh(); }
});
$("feel").onchange = safe(async (e) => { await invoke("set_feel", { feel: e.target.value }); refresh(); });
$("octPreset").onchange = safe(async (e) => {
  await invoke("set_octaves", { octaves: e.target.checked ? null : parseInt($("oct").value) });
  refresh();
});
$("oct").onchange = safe(async (e) => { if (!$("octPreset").checked) { await invoke("set_octaves", { octaves: parseInt(e.target.value) }); refresh(); } });
$("oct").oninput = (e) => $("octVal").textContent = e.target.value;
$("undoBtn").onclick = safe(async () => { await invoke("undo"); refresh(); });
$("redoBtn").onclick = safe(async () => { await invoke("redo"); refresh(); });
window.addEventListener("keydown", (e) => {
  if ((e.metaKey||e.ctrlKey) && e.key === "z") { e.preventDefault(); invoke(e.shiftKey?"redo":"undo").then(refresh).catch(e=>log(String(e),"log-err")); }
});
$("buildBtn").onclick = () => invoke("build", { mode: "full" }).then(refresh).catch(e=>log(String(e),"log-err"));
$("rebuildBtn").onclick = () => invoke("build", { mode: "incremental" }).then(refresh).catch(e=>log(String(e),"log-err"));
$("cancelBtn").onclick = () => invoke("cancel_build").catch(e=>log(String(e),"log-err"));
$("clearBtn").onclick = () => $("modal").classList.remove("hidden");
$("mCancel").onclick = () => $("modal").classList.add("hidden");
$("mConfirm").onclick = safe(async () => {
  $("modal").classList.add("hidden");
  await invoke("clear_all", { keepOverrides: $("keepOv").checked });
  tiles.clear(); refresh();
});
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
// ---- New project wizard ----
const wz = { step: 0, size: 2048, feel: "hilly", start: "water-first" };
function wzShow() {
  document.querySelectorAll(".wz-step").forEach((s) =>
    s.classList.toggle("hidden", +s.dataset.step !== wz.step));
  $("wzBack").disabled = wz.step === 0;
  $("wzNext").classList.toggle("hidden", wz.step === 3);
  $("wzCreate").classList.toggle("hidden", wz.step !== 3);
  $("wzErr").textContent = "";
}
document.querySelectorAll("#wizard .card").forEach((c) => c.onclick = () => {
  c.parentElement.querySelectorAll(".card").forEach((x) => x.classList.remove("on"));
  c.classList.add("on");
  const v = c.dataset.v;
  if (["1024","2048","4096"].includes(v)) wz.size = +v;
  else if (["bumpy","hilly","mountainous"].includes(v)) wz.feel = v;
  else wz.start = v;
});
$("newBtn").onclick = () => {
  document.querySelectorAll('.wz-step[data-step="1"] .card')[1].classList.add("on");
  document.querySelectorAll('.wz-step[data-step="2"] .card')[1].classList.add("on");
  document.querySelectorAll('.wz-step[data-step="3"] .card')[0].classList.add("on");
  wz.step = 0; wzShow();
  $("wizard").classList.remove("hidden");
};
$("wzPick").onclick = safe(async () => {
  const sel = await T.dialog.open({ directory: true });
  if (sel) $("wzParent").value = sel;
});
$("wzBack").onclick = () => { wz.step = Math.max(0, wz.step - 1); wzShow(); };
$("wzNext").onclick = () => {
  if (wz.step === 0) {
    if (!$("wzName").value.trim()) { $("wzErr").textContent = "name required"; return; }
    if (!$("wzParent").value.trim()) { $("wzErr").textContent = "pick a parent folder"; return; }
  }
  wz.step = Math.min(3, wz.step + 1); wzShow();
};
$("wzCancel").onclick = () => $("wizard").classList.add("hidden");
const sanitizeName = (s) => s.replace(/[^A-Za-z0-9 _-]/g, "_").trim();
async function wizardCreate(opts) {
  const name = sanitizeName(opts?.name ?? $("wzName").value);
  const parent = opts?.parent ?? $("wzParent").value;
  const st2 = await invoke("new_project", {
    parent, name, sizeM: opts?.size ?? wz.size,
    feel: opts?.feel ?? wz.feel, start: opts?.start ?? wz.start,
  });
  return st2;
}
$("wzCreate").onclick = safe(async () => {
  try {
    await wizardCreate();
    $("wizard").classList.add("hidden");
    tiles.clear(); fit = false; refresh();
    log("project created; full build started", "log-ok");
  } catch (e) { $("wzErr").textContent = String(e); }
});
// autotest: frontend-driven script when MB_APP_AUTOTEST is set
async function autotest() {
  const rep = { events: {}, tiles_alpha: 0, can_undo: false, can_redo: false, status1: "", status2: "", shapes_after: 0, tile_changed: false, unhandled: 0 };
  let unhandled = 0;
  const origOnRej = window.onunhandledrejection;
  window.onunhandledrejection = (e) => { unhandled++; origOnRej && origOnRej(e); };
  // startBuild: listener must be REGISTERED before invoke("build"),
  // otherwise a fast build's finished event is missed and we hang.
  const startBuild = (mode) => new Promise((res, rej) => {
    let un = () => {};
    listen("build-event", (ev) => {
      const e = ev.payload;
      rep.events[e.type] = (rep.events[e.type]||0)+1;
      if (e.type === "finished") { un(); res(e.status); }
    }).then((u) => {
      un = u;
      invoke("build", { mode }).catch((e) => { un(); rej(e); });
    });
  });
  const waitFinished = () => new Promise((res) => {
    let un = () => {};
    listen("build-event", (ev) => {
      const e = ev.payload;
      rep.events[e.type] = (rep.events[e.type]||0)+1;
      if (e.type === "finished") { un(); res(e.status); }
    }).then((u) => { un = u; });
  });
  await new Promise(r => setTimeout(r, 300));
  fwd("info", ["autotest: set_feel"]);
  await invoke("set_feel", { feel: "mountainous" });
  fwd("info", ["autotest: build start"]);
  rep.status1 = await startBuild("incremental");
  fwd("info", ["autotest: build1 done", rep.status1]);
  // synthesized mountain stroke through the same add path
  const n0 = shapeList.length;
  const pts = [];
  for (let i=0;i<24;i++){const t=i/24*2*Math.PI;pts.push([30*Math.cos(t),30*Math.sin(t)]);}
  // tile hash = length of the drawn canvas PNG before the rebuild
  const before = $("map").toDataURL().length;
  fwd("info",["autotest: add_shape invoking"]);
  await invoke("add_shape", { kind:"mountain", stroke:pts, toleranceM:1.5*mpp() });
  fwd("info",["autotest: add_shape returned"]);
  await refreshShapes();
  fwd("info",["autotest: shapes refreshed"]);
  rep.shapes_after = shapeList.length - n0;
  rep.status1b = await startBuild("incremental");
  await refreshShapes();
  rep.tile_changed = $("map").toDataURL().length !== before;
  // count alpha>0 canvas pixels (sampled)
  {
    const cx = $("map").getContext("2d");
    const d2 = cx.getImageData(0,0,$("map").width,$("map").height).data;
    let a=0; for (let i=3;i<d2.length;i+=400) if (d2[i]) a++;
    rep.tiles_alpha = a;
  }

  await invoke("undo");
  await refreshShapes();
  rep.shapes_after_undo = shapeList.length - n0; // expect 0
  // capture undo/redo state BEFORE clear_all (it reopens the session)
  const st3 = JSON.parse(await invoke("state"));
  rep.can_undo = st3.can_undo; rep.can_redo = st3.can_redo;
  // clear_all + full build (bug regression: keepOverrides camelCase)
  // clear_all starts a full build itself; the listener registers
  // BEFORE we invoke clear_all.
  const wait3 = waitFinished();
  await invoke("clear_all", { keepOverrides: true });
  rep.status3 = await wait3;
  rep.unhandled = unhandled;
  // count drawn tiles with alpha
  const ts = tileSize * mpp();
  const img = tiles.get(`${z}/0/0`);
  if (img) {
    const c2 = document.createElement("canvas"); c2.width=c2.height=tileSize;
    const cx = c2.getContext("2d"); cx.drawImage(img,0,0);
    rep.tiles_alpha = cx.getImageData(0,0,tileSize,tileSize).data.filter((_,i)=>i%4===3&&_).length;
  } else rep.tiles_alpha = -1;
  rep.status2 = rep.status1b; // incremental rebuild after shape add

  await invoke("autotest_report", { report: JSON.stringify(rep) });
}
// Wizard autotest (MB_APP_AUTOTEST_NEW=<parent dir>): drives the real
// wizard path — create, wait for build, draw land + water strokes through
// the canvas add path, undo/redo, rebuild, verify outputs.
async function autotestNew() {
  const rep = { wizard: true, created: false, events: {}, tiles_alpha: 0,
    land_shape: false, water_shape: false, undo_ok: false, redo_ok: false,
    manifest: false, status_build: "", status_rebuild: "", unhandled: 0 };
  let unhandled = 0;
  const origOnRej = window.onunhandledrejection;
  window.onunhandledrejection = (e) => { unhandled++; origOnRej && origOnRej(e); };
  const waitFinished = () => new Promise((res) => {
    let un = () => {};
    listen("build-event", (ev) => {
      const e = ev.payload;
      rep.events[e.type] = (rep.events[e.type]||0)+1;
      if (e.type === "finished") { un(); res(e.status); }
    }).then((u) => { un = u; });
  });
  try {
    const parent = await invoke("autotest_wizard_parent");
    const wait1 = waitFinished(); // register BEFORE create kicks the build
    // Drive the real wizard UI: fill fields, click through the steps.
    $("newBtn").click();
    $("wzName").value = "Wizard World";
    $("wzParent").value = parent;
    $("wzNext").click(); // -> size
    document.querySelectorAll('.wz-step[data-step="1"] .card').forEach(c =>
      c.classList.toggle("on", c.dataset.v === "2048"));
    wz.size = 2048;
    $("wzNext").click(); // -> feel
    document.querySelectorAll('.wz-step[data-step="2"] .card').forEach(c =>
      c.classList.toggle("on", c.dataset.v === "mountainous"));
    wz.feel = "mountainous";
    $("wzNext").click(); // -> start
    document.querySelectorAll('.wz-step[data-step="3"] .card').forEach(c =>
      c.classList.toggle("on", c.dataset.v === "water-first"));
    wz.start = "water-first";
    await wizardCreate();           // same call the Create button makes
    $("wizard").classList.add("hidden");
    rep.created = true;
    rep.status_build = await wait1;
    await refresh();
    rep.manifest = await invoke("file_exists",
      { path: parent + "/Wizard World/exports/terrain/manifest.json" });
    // Land stroke extending the island + Water stroke inland lake —
    // through the same invoke path the canvas tool uses.
    const land = [];
    for (let i=0;i<16;i++){const t=i/16*2*Math.PI;land.push([200+80*Math.cos(t),80*Math.sin(t)]);}
    await invoke("add_shape", { kind:"land", stroke:land, toleranceM:1.5*mpp() });
    const water = [];
    for (let i=0;i<16;i++){const t=i/16*2*Math.PI;water.push([-100+40*Math.cos(t),-100+40*Math.sin(t)]);}
    await invoke("add_shape", { kind:"water", stroke:water, toleranceM:1.5*mpp() });
    await refreshShapes();
    rep.land_shape = shapeList.some(s => s.kind === "land" && s.id !== "coast-1");
    rep.water_shape = shapeList.some(s => s.kind === "water");
    // Undo once (removes water shape), Redo (restores it), then rebuild.
    rep.undo_ok = await invoke("undo");
    rep.redo_ok = await invoke("redo");
    rep.status_rebuild = await new Promise((res, rej) => {
      let un = () => {};
      listen("build-event", (ev) => {
        const e = ev.payload;
        rep.events[e.type] = (rep.events[e.type]||0)+1;
        if (e.type === "finished") { un(); res(e.status); }
      }).then((u) => { un = u; invoke("build",{mode:"incremental"}).catch(e=>{un();rej(e);}); });
    });
    // world.json keeps both shapes.
    const ws = JSON.parse(await invoke("shapes"));
    rep.land_shape = ws.some(s => s.kind === "land" && s.id !== "coast-1");
    rep.water_shape = ws.some(s => s.kind === "water");
    await refreshShapes(); refresh();
    // Tiles render?
    const ts = tileSize * mpp();
    const img = tiles.get(`${z}/0/0`) || tiles.get(`${z}/-1/-1`);
    if (img) {
      const c2 = document.createElement("canvas"); c2.width=c2.height=tileSize;
      const cx = c2.getContext("2d"); cx.drawImage(img,0,0);
      rep.tiles_alpha = cx.getImageData(0,0,tileSize,tileSize).data.filter((_,i)=>i%4===3&&_).length;
    } else {
      const cx = $("map").getContext("2d");
      const d2 = cx.getImageData(0,0,$("map").width,$("map").height).data;
      let a=0; for (let i=3;i<d2.length;i+=400) if (d2[i]) a++;
      rep.tiles_alpha = a;
    }
  } catch (e) { rep.error = String(e); fwd("error", ["autotestNew failed", e]); }
  rep.unhandled = unhandled;
  await invoke("autotest_report", { report: JSON.stringify(rep) });
}
async function boot() {
  try {
    fwd("info", ["app.js boot, __TAURI__ keys:", Object.keys(T||{})]);
    if (await invoke("autotest_mode")) {
      if ((await invoke("autotest_wizard_parent")) !== "") autotestNew();
      else autotest();
    }
  } catch (e) { fwd("error", ["boot failed", e]); }
}
if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot);
else boot();
refresh();
