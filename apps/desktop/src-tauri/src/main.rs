//! My Builder control app — thin glue over mb-bridge.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mb_bridge::{BuildEvent, BuildMode, BuildRequest, ClearRequest};
use serde_json::json;
use tauri::Emitter;

struct Project {
    autotest: bool,
    dir: PathBuf,
    session: mb_bridge::Session,
    preview: Option<mb_bridge::PreviewSource>,
    cancel: Option<mb_bridge::CancelToken>,
    building: bool,
    dirty: bool,
}

type Shared = Arc<Mutex<Option<Project>>>;

#[tauri::command]
fn open_project(state: tauri::State<Shared>, path: String) -> Result<String, String> {
    let dir = PathBuf::from(&path);
    let mut g = state.inner().lock().map_err(|e| e.to_string())?;
    if g.as_ref().is_some_and(|p| p.building) {
        return Err("a build is running".to_string());
    }
    let session = mb_bridge::Session::open(&dir).map_err(|e| format!("{e:?}"))?;
    let preview = mb_bridge::PreviewSource::open(&dir).ok();
    *g = Some(Project {
        autotest: false,
        dir,
        session,
        preview,
        cancel: None,
        building: false,
        dirty: false,
    });
    get_state(&g)
}

fn get_state(g: &Option<Project>) -> Result<String, String> {
    let p = g.as_ref().ok_or("no project open")?;
    let w = p.session.world();
    let preset = w.terrain.feel.preset();
    Ok(json!({
        "name": p.session.project().name,
        "seed": format!("{:016x}", p.session.project().seed),
        "extent": { "w": w.extent.width_m, "d": w.extent.depth_m },
        "feel": format!("{:?}", w.terrain.feel).to_lowercase(),
        "octaves": w.terrain.overrides.octaves.unwrap_or(preset.octaves),
        "octaves_is_override": w.terrain.overrides.octaves.is_some(),
        "can_undo": p.session.can_undo(),
        "can_redo": p.session.can_redo(),
        "has_preview": p.preview.is_some(),
        "building": p.building,
        "dirty": p.dirty,
    })
    .to_string())
}

#[tauri::command]
fn state(state: tauri::State<Shared>) -> Result<String, String> {
    let g = state.inner().lock().map_err(|e| e.to_string())?;
    get_state(&g)
}

fn apply_edit(state: &Shared, edit: mb_bridge::RecipeEdit) -> Result<String, String> {
    let mut g = state.lock().map_err(|e| e.to_string())?;
    let p = g.as_mut().ok_or("no project open")?;
    if p.building {
        return Err("a build is running".to_string());
    }
    let diags = p.session.apply(edit).map_err(|e| format!("{e:?}"))?;
    p.dirty = true;
    serde_json::to_string(&diags).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_feel(state: tauri::State<Shared>, feel: String) -> Result<String, String> {
    let f = match feel.as_str() {
        "bumpy" => mb_schema::TerrainFeel::Bumpy,
        "hilly" => mb_schema::TerrainFeel::Hilly,
        "mountainous" => mb_schema::TerrainFeel::Mountainous,
        _ => return Err(format!("bad feel {feel}")),
    };
    apply_edit(&state, mb_bridge::RecipeEdit::SetFeel(f))
}

#[tauri::command]
fn set_octaves(state: tauri::State<Shared>, octaves: Option<u32>) -> Result<String, String> {
    apply_edit(&state, mb_bridge::RecipeEdit::SetOctaves(octaves))
}

#[tauri::command]
fn undo(state: tauri::State<Shared>) -> Result<bool, String> {
    let mut g = state.lock().map_err(|e| e.to_string())?;
    let p = g.as_mut().ok_or("no project open")?;
    if p.building {
        return Err("a build is running".to_string());
    }
    let r = p.session.undo().map_err(|e| format!("{e:?}"))?;
    if r {
        p.dirty = true;
    }
    Ok(r)
}

#[tauri::command]
fn redo(state: tauri::State<Shared>) -> Result<bool, String> {
    let mut g = state.lock().map_err(|e| e.to_string())?;
    let p = g.as_mut().ok_or("no project open")?;
    if p.building {
        return Err("a build is running".to_string());
    }
    let r = p.session.redo().map_err(|e| format!("{e:?}"))?;
    if r {
        p.dirty = true;
    }
    Ok(r)
}

fn spawn_build(
    app: tauri::AppHandle,
    state: Shared,
    dir: PathBuf,
    mode: BuildMode,
    cache_dir: Option<PathBuf>,
    cancel: mb_bridge::CancelToken,
) {
    std::thread::spawn(move || {
        let app2 = app.clone();
        let mut sink = |e: BuildEvent| {
            let done = matches!(e, BuildEvent::Finished { .. });
            let status_ok = matches!(
                &e,
                BuildEvent::Finished { status, .. } if status == "ok"
            );
            let _ = app2.emit("build-event", serde_json::to_value(&e).unwrap());
            if done {
                let mut g = state.lock().unwrap();
                if let Some(p) = g.as_mut() {
                    p.building = false;
                    p.cancel = None;
                    if status_ok {
                        p.dirty = false;
                        p.preview = mb_bridge::PreviewSource::open(&p.dir).ok();
                        let _ = app2.emit("preview-updated", ());
                    }
                }
            }
        };
        mb_bridge::build(&dir, &BuildRequest { mode, cache_dir }, &cancel, &mut sink);
    });
}

#[tauri::command]
fn build(state: tauri::State<Shared>, app: tauri::AppHandle, mode: String) -> Result<(), String> {
    let mut g = state.inner().lock().map_err(|e| e.to_string())?;
    let p = g.as_mut().ok_or("no project open")?;
    if p.building {
        return Err("a build is running".to_string());
    }
    p.building = true;
    let cancel = mb_bridge::CancelToken::new();
    p.cancel = Some(cancel.clone());
    let dir = p.dir.clone();
    let shared = state.inner().clone();
    drop(g);
    let mode = if mode == "full" {
        BuildMode::Full
    } else {
        BuildMode::Incremental
    };
    spawn_build(app, shared, dir, mode, None, cancel);
    Ok(())
}

#[tauri::command]
fn add_shape(
    state: tauri::State<Shared>,
    kind: String,
    stroke: Vec<[f64; 2]>,
    tolerance_m: f64,
) -> Result<String, String> {
    let k = match kind.as_str() {
        "land" => mb_schema::ShapeKind::Land,
        "water" => mb_schema::ShapeKind::Water,
        "mountain" => mb_schema::ShapeKind::Mountain,
        "forest" => mb_schema::ShapeKind::Forest,
        _ => return Err(format!("bad kind {kind}")),
    };
    let mut g = state.inner().lock().map_err(|e| e.to_string())?;
    let p = g.as_mut().ok_or("no project open")?;
    if p.building {
        return Err("a build is running".to_string());
    }
    p.session
        .apply(mb_bridge::RecipeEdit::AddShape {
            kind: k,
            stroke,
            tolerance_m,
        })
        .map_err(|e| format!("{e:?}"))?;
    p.dirty = true;
    p.session
        .last_added_id()
        .map(|s| s.to_string())
        .ok_or_else(|| "add_shape produced no id".to_string())
}

#[tauri::command]
fn remove_shape(state: tauri::State<Shared>, id: String) -> Result<String, String> {
    apply_edit(&state, mb_bridge::RecipeEdit::RemoveShape { id })
}

#[tauri::command]
fn shapes(state: tauri::State<Shared>) -> Result<String, String> {
    let g = state.inner().lock().map_err(|e| e.to_string())?;
    let p = g.as_ref().ok_or("no project open")?;
    let list: Vec<serde_json::Value> = p
        .session
        .world()
        .shapes
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "kind": format!("{:?}", s.kind).to_lowercase(),
                "points": s.points,
            })
        })
        .collect();
    Ok(serde_json::to_string(&list).unwrap())
}

#[tauri::command]
fn cancel_build(state: tauri::State<Shared>) -> Result<(), String> {
    let g = state.inner().lock().map_err(|e| e.to_string())?;
    let p = g.as_ref().ok_or("no project open")?;
    if let Some(c) = &p.cancel {
        c.cancel();
    }
    Ok(())
}

#[tauri::command]
fn clear_all(
    state: tauri::State<Shared>,
    app: tauri::AppHandle,
    keep_overrides: bool,
) -> Result<String, String> {
    let mut g = state.inner().lock().map_err(|e| e.to_string())?;
    let p = g.as_mut().ok_or("no project open")?;
    if p.building {
        return Err("a build is running".to_string());
    }
    let rep = mb_bridge::clear_all(
        &p.dir,
        &ClearRequest {
            new_seed: None,
            keep_overrides,
            cache_dir: None,
        },
    )
    .map_err(|e| e.to_string())?;
    p.preview = None;
    p.dirty = true;
    drop(g);
    // Reopen the session so the new seed is reflected.
    {
        let mut g = state.inner().lock().map_err(|e| e.to_string())?;
        if let Some(p) = g.as_mut() {
            p.session = mb_bridge::Session::open(&p.dir).map_err(|e| format!("{e:?}"))?;
        }
    }
    // Then start a full build.
    {
        let mut g = state.inner().lock().map_err(|e| e.to_string())?;
        let p = g.as_mut().ok_or("no project open")?;
        p.building = true;
        let cancel = mb_bridge::CancelToken::new();
        p.cancel = Some(cancel.clone());
        let dir = p.dir.clone();
        let shared = state.inner().clone();
        drop(g);
        spawn_build(app, shared, dir, BuildMode::Full, None, cancel);
    }
    Ok(serde_json::to_string(
        &json!({"seed": format!("{:016x}", rep.seed), "deleted": rep.deleted_files}),
    )
    .unwrap())
}

#[tauri::command]
fn preview_tile(
    state: tauri::State<Shared>,
    x0: f64,
    z0: f64,
    x1: f64,
    z1: f64,
    mpp: f64,
) -> Result<tauri::ipc::Response, String> {
    let g = state.inner().lock().map_err(|e| e.to_string())?;
    let p = g.as_ref().ok_or("no project open")?;
    let src = p.preview.as_ref().ok_or("no preview (build first)")?;
    let bytes = src
        .tile(&mb_bridge::Rect { x0, z0, x1, z1 }, mpp)
        .map_err(|e| format!("{e:?}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
fn js_log(level: String, msg: String) {
    eprintln!("[js:{level}] {msg}");
}

#[tauri::command]
fn autotest_mode(state: tauri::State<'_, Shared>) -> bool {
    state
        .inner()
        .lock()
        .map(|g| g.as_ref().map(|p| p.autotest).unwrap_or(false))
        .unwrap_or(false)
}

/// Hidden autotest hook: print the report JSON and exit 0/1.
#[tauri::command]
fn autotest_report(app: tauri::AppHandle, report: String) -> Result<(), String> {
    println!("AUTOTEST_RESULT {report}");
    let v: serde_json::Value = serde_json::from_str(&report).unwrap_or_default();
    let ok = v["status1"] == "ok"
        && v["status2"] == "ok"
        && v["status3"] == "ok"
        && v["tiles_alpha"].as_i64().unwrap_or(0) > 0
        && v["shapes_after"].as_i64().unwrap_or(0) == 1
        && v["shapes_after_undo"].as_i64().unwrap_or(-1) == 0
        && v["unhandled"].as_i64().unwrap_or(9) == 0
        && v["can_undo"] == true
        && v["can_redo"] == true;
    app.exit(if ok { 0 } else { 1 });
    Ok(())
}

fn main() {
    let autotest = std::env::var("MB_APP_AUTOTEST").ok();
    let shared: Shared = Arc::new(Mutex::new(None));
    let shared2 = shared.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(shared)
        .invoke_handler(tauri::generate_handler![
            open_project,
            state,
            set_feel,
            set_octaves,
            undo,
            redo,
            build,
            cancel_build,
            clear_all,
            preview_tile,
            autotest_report,
            autotest_mode,
            js_log,
            add_shape,
            remove_shape,
            shapes,
        ])
        .setup(move |app| {
            if autotest.is_some() {
                eprintln!(
                    "[setup] start, autotest={}",
                    std::env::var("MB_APP_AUTOTEST").unwrap_or_default()
                );
            }
            let autotest_flag = autotest.is_some();
            if autotest_flag {
                // Hard timeout: never hang silently.
                let h = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(120));
                    eprintln!("[autotest] timeout");
                    h.exit(1);
                });
            }
            if let Some(dir) = autotest {
                let session =
                    mb_bridge::Session::open(Path::new(&dir)).map_err(|e| format!("{e:?}"))?;
                *shared2.lock().unwrap() = Some(Project {
                    autotest: true,
                    dir: PathBuf::from(&dir),
                    session,
                    preview: mb_bridge::PreviewSource::open(Path::new(&dir)).ok(),
                    cancel: None,
                    building: false,
                    dirty: false,
                });
            }
            if autotest_flag {
                eprintln!("[setup] project pre-opened");
            }
            Ok(())
        })
        .on_window_event(move |w, e| {
            if let tauri::WindowEvent::Focused(_) = e {
                eprintln!("[window] {} focused", w.label());
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
