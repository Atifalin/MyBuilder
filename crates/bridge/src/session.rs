//! Recipe editing session: validates each edit, writes world.json
//! atomically, and keeps an undo/redo stack of full WorldRecipe
//! snapshots (recipes are small; field-level diffs can come later).

use std::path::{Path, PathBuf};

use mb_schema::{Diagnostic, HouseRecipe, HouseSpec, ProjectFile, Severity, WorldRecipe};

const HISTORY_CAP: usize = 200;

pub enum RecipeEdit {
    SetFeel(mb_schema::TerrainFeel),
    /// Some(n) sets the octaves override; None clears it.
    SetOctaves(Option<u32>),
    ReplaceWorld(Box<WorldRecipe>),
    /// Draw a new shape from a freehand stroke (clamped, DP-simplified,
    /// Chaikin-smoothed, mm-snapped). Returns the assigned id via the
    /// apply() result path `Shape { id }`.
    AddShape {
        kind: mb_schema::ShapeKind,
        stroke: Vec<[f64; 2]>,
        tolerance_m: f64,
    },
    RemoveShape {
        id: String,
    },
    /// Add a house (an empty `id` gets the next free `house-N`). The id
    /// assigned is reported through `last_added_id()`.
    AddHouse(Box<HouseSpec>),
    RemoveHouse {
        id: String,
    },
    /// Record a user texture (see `mb_bridge::import_texture`).
    SetMaterial(mb_schema::MaterialOverride),
    ClearMaterial(mb_schema::MaterialSlot),
}

const HOUSES_REL: &str = "recipes/houses.json";

/// One undo step: the whole editable state (recipes are small).
#[derive(Clone)]
struct Snap {
    world: WorldRecipe,
    houses: Option<HouseRecipe>,
}

pub enum BridgeError {
    Io(std::io::Error),
    Schema(mb_schema::SchemaError),
    /// Edit rejected by validation; diagnostics returned via apply().
    Rejected,
}

impl std::fmt::Debug for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Io(e) => write!(f, "{e}"),
            BridgeError::Schema(e) => write!(f, "{e}"),
            BridgeError::Rejected => write!(f, "edit rejected"),
        }
    }
}

impl From<std::io::Error> for BridgeError {
    fn from(e: std::io::Error) -> Self {
        BridgeError::Io(e)
    }
}

pub struct Session {
    last_added_id: Option<String>,
    dir: PathBuf,
    project: ProjectFile,
    cur: Snap,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
}

impl Session {
    pub fn open(dir: &Path) -> Result<Session, BridgeError> {
        let (project, world, _) = mb_schema::load_project(dir).map_err(BridgeError::Schema)?;
        let houses = mb_schema::load_houses(dir, &project).map_err(BridgeError::Schema)?;
        Ok(Session {
            last_added_id: None,
            dir: dir.to_path_buf(),
            project,
            cur: Snap { world, houses },
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn world(&self) -> &WorldRecipe {
        &self.cur.world
    }

    /// Houses in the project (empty when it has no houses recipe yet).
    pub fn houses(&self) -> &[HouseSpec] {
        self.cur.houses.as_ref().map_or(&[], |h| &h.houses)
    }

    /// User texture overrides currently recorded.
    pub fn materials(&self) -> &[mb_schema::MaterialOverride] {
        self.cur.houses.as_ref().map_or(&[], |h| &h.materials)
    }

    /// The loaded project file (name, seed, generator version).
    pub fn project(&self) -> &ProjectFile {
        &self.project
    }

    /// id assigned by the last successful AddShape (None otherwise).
    pub fn last_added_id(&self) -> Option<&str> {
        self.last_added_id.as_deref()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Validate + write + record history. A rejected edit changes
    /// neither the file nor the history.
    /// The id an AddShape edit assigned (apply() returns it through
    /// this after a successful apply).
    pub fn apply(&mut self, edit: RecipeEdit) -> Result<Vec<Diagnostic>, BridgeError> {
        self.last_added_id = None;
        let mut next = self.cur.clone();
        let candidate = &mut next.world;
        let mut houses_edit = false;
        match edit {
            RecipeEdit::SetFeel(f) => candidate.terrain.feel = f,
            RecipeEdit::SetOctaves(o) => candidate.terrain.overrides.octaves = o,
            RecipeEdit::ReplaceWorld(w) => *candidate = *w,
            RecipeEdit::SetMaterial(o) => {
                houses_edit = true;
                let rec = next.houses.get_or_insert_with(empty_houses);
                rec.materials.retain(|m| m.slot != o.slot);
                rec.materials.push(o);
                rec.materials.sort_by_key(|m| m.slot);
            }
            RecipeEdit::ClearMaterial(slot) => {
                houses_edit = true;
                if let Some(rec) = next.houses.as_mut() {
                    rec.materials.retain(|m| m.slot != slot);
                }
            }
            RecipeEdit::AddHouse(spec) => {
                houses_edit = true;
                let rec = next.houses.get_or_insert_with(empty_houses);
                let mut spec = *spec;
                if spec.id.is_empty() {
                    let mut n = 1usize;
                    while rec.houses.iter().any(|h| h.id == format!("house-{n}")) {
                        n += 1;
                    }
                    spec.id = format!("house-{n}");
                }
                self.last_added_id = Some(spec.id.clone());
                rec.houses.push(spec);
            }
            RecipeEdit::RemoveHouse { id } => {
                houses_edit = true;
                if let Some(rec) = next.houses.as_mut() {
                    rec.houses.retain(|h| h.id != id);
                }
            }
            RecipeEdit::RemoveShape { id } => {
                candidate.shapes.retain(|s| s.id != id);
            }
            RecipeEdit::AddShape {
                kind,
                stroke,
                tolerance_m,
            } => {
                let half_w = candidate.extent.width_m / 2.0;
                let half_d = candidate.extent.depth_m / 2.0;
                let mut pts: Vec<[f64; 2]> = stroke
                    .iter()
                    .map(|&[x, z]| [x.clamp(-half_w, half_w), z.clamp(-half_d, half_d)])
                    .collect();
                let closed = true;
                pts = mb_geom::simplify_dp(&pts, tolerance_m, closed);
                pts = mb_geom::chaikin(&pts, 2, closed);
                pts = mb_geom::simplify_dp(&pts, tolerance_m * 0.25, closed);
                for p in &mut pts {
                    *p = [
                        mb_geom::to_meters(mb_geom::snap(p[0])),
                        mb_geom::to_meters(mb_geom::snap(p[1])),
                    ];
                }
                if pts.len() < 3 {
                    let mut d = Vec::new();
                    d.push(mb_schema::Diagnostic::error(
                        "world.shape.too_few_points",
                        format!("/shapes/{}/points", candidate.shapes.len()),
                        format!(
                            "stroke simplified to {} points; at least 3 are required",
                            pts.len()
                        ),
                    ));
                    return Ok(d);
                }
                let prefix = match kind {
                    mb_schema::ShapeKind::Land => "land",
                    mb_schema::ShapeKind::Water => "water",
                    mb_schema::ShapeKind::Mountain => "mountain",
                    mb_schema::ShapeKind::Forest => "forest",
                };
                let mut n = 1usize;
                loop {
                    let id = format!("{prefix}-{n}");
                    if !candidate.shapes.iter().any(|s| s.id == id) {
                        self.last_added_id = Some(id.clone());
                        candidate.shapes.push(mb_schema::Shape {
                            id,
                            kind,
                            points: pts,
                        });
                        break;
                    }
                    n += 1;
                }
            }
        }
        let mut diags = Vec::new();
        mb_schema::validate_world(
            &next.world,
            self.project.world_scale.cell_size_m,
            &mut diags,
        );
        if let Some(rec) = &next.houses {
            diags.extend(mb_schema::validate_houses(rec));
            diags.extend(mb_schema::validate_placements(rec, &next.world.extent));
        }
        if diags.iter().any(|d| d.severity == Severity::Error) {
            self.last_added_id = None;
            return Ok(diags); // rejected: file and history untouched
        }
        let prev = std::mem::replace(&mut self.cur, next);
        self.undo.push(prev);
        if self.undo.len() > HISTORY_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
        if houses_edit {
            self.write_houses()?;
        } else {
            self.write_world()?;
        }
        Ok(diags)
    }

    pub fn undo(&mut self) -> Result<bool, BridgeError> {
        match self.undo.pop() {
            Some(w) => {
                self.redo.push(std::mem::replace(&mut self.cur, w));
                self.write_all()?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn redo(&mut self) -> Result<bool, BridgeError> {
        match self.redo.pop() {
            Some(w) => {
                self.undo.push(std::mem::replace(&mut self.cur, w));
                self.write_all()?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn write_world(&self) -> Result<(), BridgeError> {
        write_json(&self.dir.join(&self.project.recipes.world), &self.cur.world)
    }

    /// Write houses.json, registering it in project.json on first use.
    fn write_houses(&mut self) -> Result<(), BridgeError> {
        let Some(rec) = &self.cur.houses else {
            return Ok(());
        };
        if self.project.recipes.houses.is_none() {
            self.project.recipes.houses = Some(HOUSES_REL.to_string());
            write_json(&self.dir.join(mb_schema::PROJECT_FILE), &self.project)?;
        }
        let rel = self.project.recipes.houses.clone().unwrap();
        write_json(&self.dir.join(rel), rec)
    }

    fn write_all(&mut self) -> Result<(), BridgeError> {
        self.write_world()?;
        self.write_houses()
    }
}

fn empty_houses() -> HouseRecipe {
    HouseRecipe {
        schema_version: mb_schema::CURRENT_SCHEMA_VERSION,
        generator_version: 1,
        houses: Vec::new(),
        materials: Vec::new(),
    }
}

fn write_json<T: serde::Serialize>(path: &Path, v: &T) -> Result<(), BridgeError> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let text = serde_json::to_string_pretty(v).unwrap() + "\n";
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
